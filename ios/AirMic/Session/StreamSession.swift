import Foundation
import Network
import Observation
import Synchronization
import UIKit

struct Computer: Codable, Equatable, Hashable, Sendable, Identifiable {
    /// The computer's uuid (Bonjour TXT `id`, QR `id`), or `manual:<host>:<port>`.
    var id: String
    var name: String
    /// Last known address. Nil for a Bonjour result not connected to yet.
    var host: String?
    var port: UInt16
    /// Bonjour instance name; preferred over `host`, which can change.
    var serviceName: String?

    static func manual(name: String, host: String, port: UInt16) -> Computer {
        Computer(id: "manual:\(host):\(port)", name: name, host: host, port: port, serviceName: nil)
    }

    var endpoint: NWEndpoint? {
        if let serviceName {
            return .service(name: serviceName, type: Discovery.serviceType, domain: "local.", interface: nil)
        }
        guard let host, let port = NWEndpoint.Port(rawValue: port) else { return nil }
        return .hostPort(host: NWEndpoint.Host(host), port: port)
    }
}

/// The live focus session: control channel, mic capture, audio packets, mute and the numbers
/// the home screen shows.
///
/// idle → connecting → (pairing →) live ⇄ reconnecting (Wi-Fi blip) / paused (call, Siri) → idle.
@MainActor
@Observable
final class StreamSession {
    enum PairingStep: Equatable {
        case needsCode
        case checking
        case wrongCode
    }

    enum Phase: Equatable {
        case idle
        case connecting
        /// The computer asked for its 4 digit code.
        case pairing(PairingStep)
        case live
        case reconnecting
        case paused
        case failed(String)
    }

    static let levelHistoryCount = 28

    private(set) var phase: Phase = .idle
    private(set) var computer: Computer?
    /// Computers connected to before, most recent first.
    private(set) var knownComputers: [Computer] = []
    private(set) var isMuted = false
    private(set) var startedAt: Date?
    private(set) var muteCount = 0
    private(set) var dropouts = 0
    /// From the computer's `stats`, every 2 s while audio flows.
    private(set) var latencyMs: Int?
    /// Words in final transcripts this session; nil until the computer sends one (transcription is optional).
    private(set) var wordCount: Int?
    /// The last final transcript line.
    private(set) var lastTranscript: String?
    /// Recent mic levels, 0...1, oldest first; drives the level bars.
    private(set) var levels = [Float](repeating: 0, count: levelHistoryCount)

    var goalMinutes: Int {
        didSet { defaults.set(goalMinutes, forKey: Keys.goal) }
    }
    var voiceProcessing: Bool {
        didSet { defaults.set(voiceProcessing, forKey: Keys.voiceProcessing) }
    }
    var hapticsEnabled: Bool {
        didSet { defaults.set(hapticsEnabled, forKey: Keys.haptics) }
    }

    private let defaults = UserDefaults.standard
    private let capture = AudioCapture()
    private let pipe = AudioPipe()
    private var client: ControlClient?
    private var reconnectTask: Task<Void, Never>?
    private var reconnectAttempt = 0
    private var interrupted = false
    private let phoneID: String
    /// From a QR code: sent as soon as the computer asks, without asking the user.
    private var pendingCode: String?
    private var sentToken = false
    private var peerHost: NWEndpoint.Host?

    private enum Keys {
        static let goal = "focus.goalMinutes"
        static let voiceProcessing = "audio.voiceProcessing"
        static let haptics = "ui.haptics"
        static let known = "computers.known"
        static let phoneID = "phone.id"
    }

    init() {
        goalMinutes = defaults.object(forKey: Keys.goal) as? Int ?? 50
        voiceProcessing = defaults.object(forKey: Keys.voiceProcessing) as? Bool ?? true
        hapticsEnabled = defaults.object(forKey: Keys.haptics) as? Bool ?? true
        if let data = defaults.data(forKey: Keys.known),
           let computers = try? JSONDecoder().decode([Computer].self, from: data) {
            // Earlier builds saved the interface scope too ("192.168.20.42%en0").
            knownComputers = computers.map { computer in
                var computer = computer
                computer.host = computer.host.map(Self.withoutScope)
                return computer
            }
        }
        if let id = defaults.string(forKey: Keys.phoneID) {
            phoneID = id
        } else {
            phoneID = UUID().uuidString.lowercased()
            defaults.set(phoneID, forKey: Keys.phoneID)
        }
    }

    var recentComputer: Computer? { knownComputers.first }

    /// A session is under way (the timer runs and End session shows).
    var isActive: Bool {
        switch phase {
        case .connecting, .live, .reconnecting, .paused: true
        case .idle, .pairing, .failed: false
        }
    }

    var isPairing: Bool {
        if case .pairing = phase { return true }
        return false
    }

    /// `pairingCode` comes from a QR code and is sent without asking the user.
    func connect(to computer: Computer, pairingCode: String? = nil) async {
        if isActive || isPairing { teardown() }
        guard await AudioCapture.requestPermission() else {
            phase = .failed("Microphone access is off. Turn it on in Settings.")
            return
        }
        // Keep what we learned last time (address) for a computer we know.
        var computer = computer
        if let known = knownComputers.first(where: { $0.id == computer.id }) {
            computer.host = computer.host ?? known.host
            computer.serviceName = computer.serviceName ?? known.serviceName
        }
        self.computer = computer
        pendingCode = pairingCode
        muteCount = 0
        dropouts = 0
        wordCount = nil
        lastTranscript = nil
        reconnectAttempt = 0
        phase = .connecting
        openControl()
    }

    func toggleMute() {
        guard isActive else { return }
        isMuted.toggle()
        pipe.setMuted(isMuted)
        if phase == .live || phase == .paused { client?.send(.mute(on: isMuted)) }
        if isMuted {
            muteCount += 1
            levels = Self.silentLevels
        }
    }

    /// Ends the session and returns it, or nil if audio never started.
    func end() -> FocusSession? {
        var finished: FocusSession?
        if let startedAt, let computer {
            let session = FocusSession(
                start: startedAt, end: .now, computerName: computer.name,
                goalMinutes: goalMinutes, mutes: muteCount, dropouts: dropouts)
            session.words = wordCount
            session.lastTranscript = lastTranscript
            finished = session
        }
        teardown()
        phase = .idle
        return finished
    }

    /// On launch: reconnect to the last computer, no taps.
    func autoConnect() async {
        guard phase == .idle, let computer = recentComputer else { return }
        await connect(to: computer)
    }

    func submitPairingCode(_ code: String) {
        guard isPairing, code.count == 4 else { return }
        phase = .pairing(.checking)
        client?.send(.pair(code: code))
    }

    func cancelPairing() {
        guard isPairing else { return }
        teardown()
        phase = .idle
    }

    func forget(_ computer: Computer) {
        PairingTokens.delete(for: computer.id)
        knownComputers.removeAll { $0.id == computer.id }
        saveKnown()
    }

    /// Maps RMS to 0...1 on a -55...-10 dBFS scale: room noise near 0, speech near the top.
    nonisolated static func normalizedLevel(rms: Float) -> Float {
        guard rms > 0 else { return 0 }
        let db = 20 * log10(rms)
        return min(1, max(0, (db + 55) / 45))
    }

    /// "192.168.20.42", without the "%en0" interface scope that `IPv4Address`'s description adds.
    nonisolated static func dottedQuad(_ address: IPv4Address) -> String {
        address.rawValue.map(String.init).joined(separator: ".")
    }

    nonisolated static func withoutScope(_ host: String) -> String {
        host.split(separator: "%", maxSplits: 1).first.map(String.init) ?? host
    }

    /// Delay before reconnect attempt `n` (0 based): 0.25, 0.5, 1, 2, 2, … seconds.
    /// Capped at 2 s so audio is back within 3 s of the Wi-Fi returning.
    nonisolated static func reconnectDelay(attempt: Int) -> Duration {
        .milliseconds(min(2_000, 250 << min(attempt, 4)))
    }

    // MARK: - Control channel

    private func openControl() {
        guard let computer, let endpoint = computer.endpoint else {
            fail("That address doesn't look right")
            return
        }
        let token = PairingTokens.token(for: computer.id)
        sentToken = token != nil
        peerHost = nil
        let client = ControlClient(
            endpoint: endpoint,
            hello: .hello(v: AirMicProtocol.version, phoneID: phoneID, phoneName: UIDevice.current.name),
            token: token)
        self.client = client
        Task { [weak self] in
            for await event in client.events {
                self?.handle(event, from: client)
            }
        }
        client.start()
    }

    private func handle(_ event: ControlClient.Event, from source: ControlClient) {
        guard source === client, var computer else { return }
        switch event {
        case let .connected(host):
            peerHost = host
        case let .ready(sessionID, udpPort, sampleRate):
            guard sampleRate == AirMicProtocol.sampleRate else {
                fail("\(computer.name) asked for \(sampleRate) Hz audio; AirMic sends 48000 Hz")
                return
            }
            guard let host = peerHost ?? computer.host.map({ NWEndpoint.Host($0) }),
                  let sender = UDPSender(host: host, port: udpPort)
            else {
                fail("\(computer.name) sent a bad audio port")
                return
            }
            sender.start()
            pipe.attach(sender: sender, sessionID: sessionID)
            reconnectAttempt = 0
            // Keep a routable address for display and fallback, not a 169.254 link-local one.
            if case let .ipv4(address) = host, !address.isLinkLocal { computer.host = Self.dottedQuad(address) }
            self.computer = computer
            remember(computer)
            if isMuted { source.send(.mute(on: true)) }
            if startedAt == nil {
                do {
                    try startCapture()
                } catch {
                    fail("Couldn't start the microphone")
                    return
                }
                startedAt = .now
            }
            phase = interrupted ? .paused : .live
        case let .stats(_, _, latency):
            latencyMs = Int(latency.rounded())
        case .pairRequired:
            // A token we sent is no longer known to the computer.
            if sentToken { PairingTokens.delete(for: computer.id) }
            sentToken = false
            if let code = pendingCode {
                pendingCode = nil
                phase = .pairing(.checking)
                source.send(.pair(code: code))
            } else {
                phase = .pairing(.needsCode)
            }
        case let .paired(token):
            PairingTokens.save(token, for: computer.id)
        case let .transcript(text, isFinal):
            // Partial lines are replaced by the next one; only final lines count.
            guard isFinal else { break }
            let words = Transcript.wordCount(text)
            guard words > 0 else { break }
            wordCount = (wordCount ?? 0) + words
            lastTranscript = Transcript.clean(text)
        case let .error(code, message):
            switch code {
            case "busy": fail("\(computer.name) is in use by another phone")
            case "bad_code": phase = .pairing(.wrongCode)
            case "pair_locked": fail("Too many wrong codes. Show a new code on \(computer.name) and try again.")
            case "unsupported_version": fail("\(computer.name) runs a different AirMic version")
            default: fail(message.isEmpty ? "Error from \(computer.name): \(code)" : message)
            }
        case let .closed(reason):
            connectionLost(reason: reason)
        }
    }

    private func connectionLost(reason: String?) {
        client = nil
        pipe.detach()
        latencyMs = nil
        guard let computer, startedAt != nil else {
            // Never got going: the computer isn't there, or pairing ended.
            let wasPairing = isPairing
            teardown()
            phase = .failed(wasPairing
                ? "Pairing with \(computer?.name ?? "the computer") stopped. Try again."
                : "Couldn't reach \(computer?.name ?? "the computer"). Is AirMic running on it?")
            return
        }
        guard reason != nil else {
            // The computer said bye.
            teardown()
            phase = .failed("\(computer.name) ended the session")
            return
        }
        if phase == .live || phase == .paused { dropouts += 1 }
        phase = .reconnecting
        let delay = Self.reconnectDelay(attempt: reconnectAttempt)
        reconnectAttempt += 1
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(for: delay)
            guard !Task.isCancelled, let self, self.phase == .reconnecting else { return }
            self.openControl()
        }
    }

    private func fail(_ message: String) {
        teardown()
        phase = .failed(message)
    }

    /// Stops everything; the caller sets the next phase.
    private func teardown() {
        reconnectTask?.cancel()
        reconnectTask = nil
        client?.close()
        client = nil
        pipe.detach()
        capture.stop()
        startedAt = nil
        isMuted = false
        interrupted = false
        pendingCode = nil
        peerHost = nil
        pipe.setMuted(false)
        latencyMs = nil
        levels = Self.silentLevels
    }

    // MARK: - Audio

    private func startCapture() throws {
        capture.voiceProcessing = voiceProcessing
        capture.onInterruption = { [weak self] interruption in
            self?.handleInterruption(interruption)
        }
        let pipe = pipe
        try capture.start { [weak self] frame in
            let muted = pipe.process(frame)
            // UI at about 30 fps.
            if frame.index % 3 == 0 {
                let level = muted ? 0 : Self.normalizedLevel(rms: frame.rms)
                Task { @MainActor in self?.push(level: level) }
            }
        }
    }

    private func handleInterruption(_ interruption: AudioCapture.Interruption) {
        switch interruption {
        case .began:
            interrupted = true
            levels = Self.silentLevels
            if phase == .live { phase = .paused }
        case .ended:
            interrupted = false
            if phase == .paused { phase = .live }
        }
    }

    private static let silentLevels = [Float](repeating: 0, count: levelHistoryCount)

    private func push(level: Float) {
        guard phase == .live, !isMuted else { return }
        // Fast attack, slower release, so speech doesn't flicker.
        let previous = levels.last ?? 0
        let smoothed = level >= previous ? level : previous * 0.6 + level * 0.4
        levels.removeFirst()
        levels.append(smoothed)
    }

    private func remember(_ computer: Computer) {
        knownComputers.removeAll { $0.id == computer.id }
        knownComputers.insert(computer, at: 0)
        saveKnown()
    }

    private func saveKnown() {
        if let data = try? JSONEncoder().encode(knownComputers) {
            defaults.set(data, forKey: Keys.known)
        }
    }
}

/// Hands captured frames to the current session's packetizer and UDP sender.
/// Shared between the main actor (attach, detach, mute) and the frame processor thread.
private final class AudioPipe: Sendable {
    private struct Route {
        let sender: UDPSender
        var packetizer: Packetizer
    }

    private struct State {
        var route: Route?
        var muted = false
    }

    private let state = Mutex(State())

    func attach(sender: UDPSender, sessionID: UInt32) {
        state.withLock { state in
            state.route?.sender.cancel()
            state.route = Route(sender: sender, packetizer: Packetizer(sessionID: sessionID))
        }
    }

    func detach() {
        state.withLock { state in
            state.route?.sender.cancel()
            state.route = nil
        }
    }

    func setMuted(_ muted: Bool) {
        state.withLock { $0.muted = muted }
    }

    /// Sends the frame (or a muted header) if a session is attached. Returns whether muted.
    func process(_ frame: AudioFrame) -> Bool {
        state.withLock { state in
            if var route = state.route {
                if let packet = route.packetizer.packet(pcm: frame.pcm, muted: state.muted) {
                    route.sender.send(packet)
                }
                state.route = route
            }
            return state.muted
        }
    }
}
