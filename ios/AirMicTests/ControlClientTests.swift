import Foundation
import Network
import Synchronization
import Testing
@testable import AirMic

/// A one-connection control server on loopback, standing in for `airmicd`.
private final class FakeServer: Sendable {
    let port: UInt16
    /// Messages from the phone, in order.
    let received: AsyncStream<ControlMessage>

    private let listener: NWListener
    private let connection = AcceptedConnection()
    private let queue = DispatchQueue(label: "fake-airmicd")

    init() async throws {
        let listener = try NWListener(using: .tcp, on: .any)
        let (stream, continuation) = AsyncStream<ControlMessage>.makeStream()
        received = stream
        self.listener = listener
        let ready = AsyncStream<UInt16>.makeStream()
        listener.stateUpdateHandler = { state in
            if case .ready = state, let port = listener.port?.rawValue { ready.continuation.yield(port) }
        }
        let connection = connection
        let queue = queue
        listener.newConnectionHandler = { newConnection in
            connection.set(newConnection)
            newConnection.start(queue: queue)
            Self.receive(on: newConnection, buffer: LineBuffer(), into: continuation)
        }
        listener.start(queue: queue)
        var iterator = ready.stream.makeAsyncIterator()
        port = await iterator.next() ?? 0
    }

    private static func receive(on connection: NWConnection, buffer: LineBuffer, into continuation: AsyncStream<ControlMessage>.Continuation) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { data, _, isComplete, error in
            var buffer = buffer
            if let data, let lines = try? buffer.append(data) {
                for line in lines {
                    if let message = try? ControlMessage.decode(line) { continuation.yield(message) }
                }
            }
            if isComplete || error != nil {
                continuation.finish()
            } else {
                receive(on: connection, buffer: buffer, into: continuation)
            }
        }
    }

    func send(_ message: ControlMessage) async {
        var data = message.encoded()
        data.append(0x0A)
        await send(raw: data)
    }

    func send(raw data: Data) async {
        // The phone may still be connecting; wait for the accepted connection.
        for _ in 0..<200 {
            if let connection = connection.get() {
                connection.send(content: data, completion: .idempotent)
                return
            }
            try? await Task.sleep(for: .milliseconds(10))
        }
    }

    func stop() {
        connection.get()?.cancel()
        listener.cancel()
    }

    var endpoint: NWEndpoint {
        .hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
    }
}

private let hello = ControlMessage.hello(v: 1, phoneID: "test-phone", phoneName: "Test")
private let fast = ControlClient.Timing(pingInterval: 0.2, peerTimeout: 0.8, connectTimeout: 1)

/// Next event that isn't `.connected`, or nil if none arrives in time.
private func nextEvent(
    _ iterator: inout AsyncStream<ControlClient.Event>.AsyncIterator, timeout: Duration = .seconds(3)
) async -> ControlClient.Event? {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline {
        guard let event = await iterator.next() else { return nil }
        if case .connected = event { continue }
        return event
    }
    return nil
}

/// Next phone message that isn't a keepalive.
private func nextMessage(_ iterator: inout AsyncStream<ControlMessage>.AsyncIterator, skipping: Set<String> = ["ping", "pong"]) async -> ControlMessage? {
    while let message = await iterator.next() {
        switch message {
        case .ping where skipping.contains("ping"), .pong where skipping.contains("pong"): continue
        default: return message
        }
    }
    return nil
}

private final class AcceptedConnection: Sendable {
    private let value = Mutex<NWConnection?>(nil)
    func get() -> NWConnection? { value.withLock { $0 } }
    func set(_ connection: NWConnection) { value.withLock { $0 = connection } }
}

@Suite(.serialized, .timeLimit(.minutes(1)))
struct ControlClientTests {
    @Test func sendsHelloThenAuth() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: "abc123", timing: fast)
        client.start()
        defer { client.close() }
        var messages = server.received.makeAsyncIterator()
        #expect(await nextMessage(&messages) == hello)
        #expect(await nextMessage(&messages) == .auth(token: "abc123"))
    }

    @Test func helloOnlyWithoutToken() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        client.start()
        var messages = server.received.makeAsyncIterator()
        #expect(await nextMessage(&messages) == hello)
        client.close()
        // Next after hello is the bye from close(): no auth was sent.
        #expect(await nextMessage(&messages) == .bye)
    }

    @Test func reportsConnectedThenReady() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        defer { client.close() }
        guard case let .connected(host)? = await events.next() else {
            Issue.record("expected .connected first")
            return
        }
        #expect(host != nil)
        await server.send(.ready(sessionID: 439_041_101, udpPort: 47801, sampleRate: 48000))
        guard case let .ready(sessionID, udpPort, sampleRate)? = await nextEvent(&events) else {
            Issue.record("expected .ready")
            return
        }
        #expect(sessionID == 439_041_101 && udpPort == 47801 && sampleRate == 48000)
    }

    @Test func passesPairingAndStatsAndTranscripts() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        defer { client.close() }
        await server.send(.pairRequired)
        guard case .pairRequired? = await nextEvent(&events) else { Issue.record("pair_required"); return }
        await server.send(.error(code: "bad_code", message: "Wrong code"))
        guard case .error("bad_code", _)? = await nextEvent(&events) else { Issue.record("bad_code"); return }
        await server.send(.paired(token: "t"))
        guard case .paired("t")? = await nextEvent(&events) else { Issue.record("paired"); return }
        await server.send(.stats(lossPct: 0.5, jitterMs: 3.2, latencyMs: 48))
        guard case .stats(_, _, 48)? = await nextEvent(&events) else { Issue.record("stats"); return }
        await server.send(.transcript(text: "hello there", isFinal: true))
        guard case .transcript("hello there", true)? = await nextEvent(&events) else { Issue.record("transcript"); return }
    }

    @Test func answersPingWithPong() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        client.start()
        defer { client.close() }
        var messages = server.received.makeAsyncIterator()
        #expect(await nextMessage(&messages) == hello)
        await server.send(.ping)
        #expect(await nextMessage(&messages, skipping: ["ping"]) == .pong)
    }

    @Test func pingsOnItsOwn() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        client.start()
        defer { client.close() }
        var messages = server.received.makeAsyncIterator()
        #expect(await nextMessage(&messages) == hello)
        #expect(await nextMessage(&messages, skipping: []) == .ping)
    }

    @Test func closesWhenThePeerGoesQuiet() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        // The server never answers: after peerTimeout the client gives up.
        guard case let .closed(reason)? = await nextEvent(&events) else { Issue.record("expected .closed"); return }
        #expect(reason != nil)
    }

    @Test func byeFromTheComputerClosesCleanly() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        await server.send(.bye)
        guard case let .closed(reason)? = await nextEvent(&events) else { Issue.record("expected .closed"); return }
        #expect(reason == nil)
    }

    @Test func unknownTypesAreIgnored() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        defer { client.close() }
        await server.send(raw: Data(#"{"type":"future_thing","x":1}"# .utf8 + [0x0A]))
        await server.send(.stats(lossPct: 0, jitterMs: 0, latencyMs: 12))
        guard case .stats(_, _, 12)? = await nextEvent(&events) else { Issue.record("expected stats after unknown type"); return }
    }

    @Test func invalidJSONClosesTheConnection() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        await server.send(raw: Data("not json\n".utf8))
        guard case let .closed(reason)? = await nextEvent(&events) else { Issue.record("expected .closed"); return }
        #expect(reason != nil)
    }

    @Test func refusedConnectionCloses() async throws {
        let server = try await FakeServer()
        let endpoint = server.endpoint
        server.stop() // nothing listens on the port now
        let client = ControlClient(endpoint: endpoint, hello: hello, token: nil, timing: fast)
        var events = client.events.makeAsyncIterator()
        client.start()
        guard case let .closed(reason)? = await nextEvent(&events) else { Issue.record("expected .closed"); return }
        #expect(reason != nil)
    }

    @Test func closeSendsBye() async throws {
        let server = try await FakeServer()
        defer { server.stop() }
        let client = ControlClient(endpoint: server.endpoint, hello: hello, token: nil, timing: fast)
        client.start()
        var messages = server.received.makeAsyncIterator()
        #expect(await nextMessage(&messages) == hello)
        client.close()
        #expect(await nextMessage(&messages) == .bye)
    }
}
