import Foundation
import Observation
import Synchronization
import UIKit

struct Computer: Codable, Equatable, Sendable {
    var name: String
    var host: String
    var port: UInt16
}

/// The live focus session: control channel, mic capture, audio packets, mute and the numbers
/// the home screen shows.
///
/// idle → connecting → live ⇄ reconnecting (Wi-Fi blip) / paused (call, Siri) → idle.
@MainActor
@Observable
final class StreamSession {
    enum Phase: Equatable {
        case idle
        case connecting
        case live
        case reconnecting
        case paused
        case failed(String)
    }

    static let levelHistoryCount = 28

    private(set) var phase: Phase = .idle
    private(set) var computer: Computer?
    private(set) var recentComputer: Computer?
    private(set) var isMuted = false
    private(set) var startedAt: Date?
    private(set) var muteCount = 0
    private(set) var dropouts = 0
    /// From the computer's `stats`, every 2 s while audio flows.
    private(set) var latencyMs: Int?
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

    private enum Keys {
        static let goal = "focus.goalMinutes"
        static let voiceProcessing = "audio.voiceProcessing"
        static let haptics = "ui.haptics"
        static let recent = "computer.recent"
        static let phoneID = "phone.id"
    }

    init() {
        goalMinutes = defaults.object(forKey: Keys.goal) as? Int ?? 50
        voiceProcessing = defaults.object(forKey: Keys.voiceProcessing) as? Bool ?? true
        hapticsEnabled = defaults.object(forKey: Keys.haptics) as? Bool ?? true
        if let data = defaults.data(forKey: Keys.recent),
           var computer = try? JSONDecoder().decode(Computer.self, from: data) {
            // M1 spike builds saved the netcat port.
            if computer.port == 5555 { computer.port = AirMicProtocol.controlPort }
            recentComputer = computer
        }
        if let id = defaults.string(forKey: Keys.phoneID) {
            phoneID = id
        } else {
            phoneID = UUID().uuidString.lowercased()
            defaults.set(phoneID, forKey: Keys.phoneID)
        }
    }

    /// A session is under way (the timer runs and End session shows).
    var isActive: Bool {
        switch phase {
        case .connecting, .live, .reconnecting, .paused: true
        case .idle, .failed: false
        }
    }

    func connect(to computer: Computer) async {
        if isActive { teardown() }
        guard await AudioCapture.requestPermission() else {
            phase = .failed("Microphone access is off. Turn it on in Settings.")
            return
        }
        self.computer = computer
        remember(computer)
        muteCount = 0
        dropouts = 0
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
        let finished: FocusSession? = if let startedAt, let computer {
            FocusSession(
                start: startedAt, end: .now, computerName: computer.name,
                goalMinutes: goalMinutes, mutes: muteCount, dropouts: dropouts)
        } else {
            nil
        }
        teardown()
        phase = .idle
        return finished
    }

    func forgetRecent() {
        recentComputer = nil
        defaults.removeObject(forKey: Keys.recent)
    }

    /// Maps RMS to 0...1 on a -55...-10 dBFS scale: room noise near 0, speech near the top.
    nonisolated static func normalizedLevel(rms: Float) -> Float {
        guard rms > 0 else { return 0 }
        let db = 20 * log10(rms)
        return min(1, max(0, (db + 55) / 45))
    }

    /// Delay before reconnect attempt `n` (0 based): 0.25, 0.5, 1, 2, 2, … seconds.
    /// Capped at 2 s so audio is back within 3 s of the Wi-Fi returning.
    nonisolated static func reconnectDelay(attempt: Int) -> Duration {
        .milliseconds(min(2_000, 250 << min(attempt, 4)))
    }

    // MARK: - Control channel

    private func openControl() {
        guard let computer,
              let client = ControlClient(
                host: computer.host, port: computer.port,
                hello: .hello(v: AirMicProtocol.version, phoneID: phoneID, phoneName: UIDevice.current.name),
                token: nil)
        else {
            fail("That address doesn't look right")
            return
        }
        self.client = client
        Task { [weak self] in
            for await event in client.events {
                self?.handle(event, from: client)
            }
        }
        client.start()
    }

    private func handle(_ event: ControlClient.Event, from source: ControlClient) {
        guard source === client, let computer else { return }
        switch event {
        case let .ready(sessionID, udpPort, sampleRate):
            guard sampleRate == AirMicProtocol.sampleRate else {
                fail("\(computer.name) asked for \(sampleRate) Hz audio; AirMic sends 48000 Hz")
                return
            }
            guard let sender = UDPSender(host: computer.host, port: udpPort) else {
                fail("\(computer.name) sent a bad audio port")
                return
            }
            sender.start()
            pipe.attach(sender: sender, sessionID: sessionID)
            reconnectAttempt = 0
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
            fail("\(computer.name) asks for a pairing code. Pairing is coming; for now run airmicd --no-auth.")
        case .paired, .transcript:
            break // M3.3, M6.1
        case let .error(code, message):
            switch code {
            case "busy": fail("\(computer.name) is in use by another phone")
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
            // Never got going: the computer isn't there.
            teardown()
            phase = .failed("Couldn't reach \(computer?.name ?? "the computer"). Is AirMic running on it?")
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
        recentComputer = computer
        if let data = try? JSONEncoder().encode(computer) {
            defaults.set(data, forKey: Keys.recent)
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
