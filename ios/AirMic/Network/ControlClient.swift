import Foundation
import Network
import os

private let log = Logger(subsystem: "io.airmic.AirMic", category: "control")

/// TCP control channel to `airmicd` (docs/protocol.md §2).
///
/// Sends `hello` (and `auth` when a token is known), answers pings, pings every 2 s and
/// closes after 6 s of silence. Everything runs on one serial queue; events come out of `events`.
final class ControlClient: @unchecked Sendable {
    enum Event: Sendable {
        /// TCP is up. `host` is the computer's address; audio goes there (§3.2).
        case connected(host: NWEndpoint.Host?)
        case ready(sessionID: UInt32, udpPort: UInt16, sampleRate: Int)
        case pairRequired
        case paired(token: String)
        case stats(lossPct: Double, jitterMs: Double, latencyMs: Double)
        case transcript(text: String, isFinal: Bool)
        case error(code: String, message: String)
        /// Always the last event. `reason` is nil after a clean `bye`.
        case closed(reason: String?)
    }

    static let pingInterval: TimeInterval = 2
    static let peerTimeout: TimeInterval = 6
    static let connectTimeout: TimeInterval = 5

    let events: AsyncStream<Event>

    private let continuation: AsyncStream<Event>.Continuation
    private let connection: NWConnection
    private let queue = DispatchQueue(label: "io.airmic.control")
    private let greeting: [ControlMessage]

    // Confined to `queue`.
    private var lineBuffer = LineBuffer()
    private var isOpen = false
    private var isFinished = false
    private var startedAt = Date()
    private var lastReceived = Date()
    private var timer: DispatchSourceTimer?

    init(endpoint: NWEndpoint, hello: ControlMessage, token: String?) {
        let tcp = NWProtocolTCP.Options()
        tcp.noDelay = true
        connection = NWConnection(to: endpoint, using: NWParameters(tls: nil, tcp: tcp))
        greeting = [hello] + (token.map { [.auth(token: $0)] } ?? [])
        (events, continuation) = AsyncStream.makeStream(bufferingPolicy: .unbounded)
    }

    func start() {
        queue.async { [self] in
            startedAt = Date()
            connection.stateUpdateHandler = { [weak self] state in self?.handle(state) }
            connection.start(queue: queue)
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now() + 1, repeating: Self.pingInterval)
            timer.setEventHandler { [weak self] in self?.tick() }
            timer.resume()
            self.timer = timer
        }
    }

    func send(_ message: ControlMessage) {
        queue.async { [self] in write(message) }
    }

    /// Ends the session, sending `bye` first when connected.
    func close() {
        queue.async { [self] in
            if isOpen { write(.bye) }
            finish(reason: nil)
        }
    }

    // MARK: - Queue confined

    private func handle(_ state: NWConnection.State) {
        switch state {
        case .ready:
            isOpen = true
            lastReceived = Date()
            var host: NWEndpoint.Host?
            if case let .hostPort(remote, _) = connection.currentPath?.remoteEndpoint { host = remote }
            continuation.yield(.connected(host: host))
            greeting.forEach(write)
            receive()
        case .waiting(let error):
            // Refused or unreachable: NWConnection would wait forever.
            finish(reason: error.localizedDescription)
        case .failed(let error):
            finish(reason: error.localizedDescription)
        default:
            break
        }
    }

    private func receive() {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, isComplete, error in
            guard let self, !isFinished else { return }
            if let data, !data.isEmpty {
                do {
                    for line in try lineBuffer.append(data) {
                        handle(line: line)
                        if isFinished { return }
                    }
                } catch {
                    finish(reason: "Message too long")
                    return
                }
            }
            if let error {
                finish(reason: error.localizedDescription)
            } else if isComplete {
                finish(reason: "Connection closed by the computer")
            } else {
                receive()
            }
        }
    }

    private func handle(line: Data) {
        lastReceived = Date()
        let message: ControlMessage?
        do {
            message = try ControlMessage.decode(line)
        } catch {
            log.error("Bad message: \(String(decoding: line, as: UTF8.self), privacy: .public)")
            finish(reason: "The computer sent a message AirMic doesn't understand")
            return
        }
        guard let message else { return }
        switch message {
        case .ping: write(.pong)
        case .pong: break
        case .bye: finish(reason: nil)
        case let .ready(sessionID, udpPort, sampleRate): continuation.yield(.ready(sessionID: sessionID, udpPort: udpPort, sampleRate: sampleRate))
        case .pairRequired: continuation.yield(.pairRequired)
        case let .paired(token): continuation.yield(.paired(token: token))
        case let .stats(loss, jitter, latency): continuation.yield(.stats(lossPct: loss, jitterMs: jitter, latencyMs: latency))
        case let .transcript(text, isFinal): continuation.yield(.transcript(text: text, isFinal: isFinal))
        case let .error(code, text): continuation.yield(.error(code: code, message: text))
        case .hello, .pair, .auth, .mute: break // phone → PC only
        }
    }

    private func tick() {
        guard !isFinished else { return }
        guard isOpen else {
            if Date().timeIntervalSince(startedAt) > Self.connectTimeout {
                finish(reason: "Timed out connecting")
            }
            return
        }
        if Date().timeIntervalSince(lastReceived) > Self.peerTimeout {
            finish(reason: "The computer stopped responding")
        } else {
            write(.ping)
        }
    }

    private func write(_ message: ControlMessage) {
        guard isOpen, !isFinished else { return }
        var data = message.encoded()
        data.append(0x0A)
        connection.send(content: data, completion: .contentProcessed { _ in })
    }

    private func finish(reason: String?) {
        guard !isFinished else { return }
        isFinished = true
        timer?.cancel()
        timer = nil
        if isOpen {
            // Let a queued `bye` go out before the cancel.
            connection.send(content: nil, isComplete: true, completion: .contentProcessed { [connection] _ in connection.cancel() })
        } else {
            connection.cancel()
        }
        isOpen = false
        if let reason { log.info("Control closed: \(reason, privacy: .public)") }
        continuation.yield(.closed(reason: reason))
        continuation.finish()
    }
}
