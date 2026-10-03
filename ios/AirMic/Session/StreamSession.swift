import Foundation
import Network
import Observation
import Synchronization

struct Computer: Codable, Equatable, Sendable {
    var name: String
    var host: String
    var port: UInt16
}

/// The live focus session: mic capture, streaming, mute and the numbers the home screen shows.
///
/// Streams raw PCM over UDP for now (M1 spike receiver). The control channel and
/// packet header replace this in M2.
@MainActor
@Observable
final class StreamSession {
    enum Phase: Equatable {
        case idle
        case connecting
        case live
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
    private var sender: RawUDPSender?
    private let gate = MuteGate()

    private enum Keys {
        static let goal = "focus.goalMinutes"
        static let voiceProcessing = "audio.voiceProcessing"
        static let haptics = "ui.haptics"
        static let recent = "computer.recent"
    }

    init() {
        goalMinutes = defaults.object(forKey: Keys.goal) as? Int ?? 50
        voiceProcessing = defaults.object(forKey: Keys.voiceProcessing) as? Bool ?? true
        hapticsEnabled = defaults.object(forKey: Keys.haptics) as? Bool ?? true
        if let data = defaults.data(forKey: Keys.recent) {
            recentComputer = try? JSONDecoder().decode(Computer.self, from: data)
        }
    }

    var isConnected: Bool { phase == .connecting || phase == .live }

    func connect(to computer: Computer) async {
        if isConnected { stopStreaming() }
        guard let sender = RawUDPSender(host: computer.host, port: computer.port) else {
            phase = .failed("That address doesn't look right")
            return
        }
        guard await AudioCapture.requestPermission() else {
            phase = .failed("Microphone access is off. Turn it on in Settings.")
            return
        }

        self.computer = computer
        remember(computer)
        phase = .connecting
        isMuted = false
        gate.set(muted: false)

        sender.start { [weak self] state in
            Task { @MainActor in self?.handle(state) }
        }
        let gate = gate
        do {
            capture.voiceProcessing = voiceProcessing
            try capture.start { [weak self] frame in
                let muted = gate.isMuted
                if !muted { sender.send(frame.pcm) }
                // UI at about 30 fps.
                if frame.index % 3 == 0 {
                    let level = muted ? 0 : Self.normalizedLevel(rms: frame.rms)
                    Task { @MainActor in self?.push(level: level) }
                }
            }
        } catch {
            sender.cancel()
            phase = .failed("Couldn't start the microphone")
            return
        }
        self.sender = sender
        startedAt = .now
        muteCount = 0
        dropouts = 0
    }

    func toggleMute() {
        guard isConnected else { return }
        isMuted.toggle()
        gate.set(muted: isMuted)
        if isMuted {
            muteCount += 1
            levels = Self.silentLevels
        }
    }

    /// Stops streaming and returns the finished session, or nil if nothing was streamed.
    func end() -> FocusSession? {
        let finished: FocusSession? = if let startedAt, let computer {
            FocusSession(
                start: startedAt, end: .now, computerName: computer.name,
                goalMinutes: goalMinutes, mutes: muteCount, dropouts: dropouts)
        } else {
            nil
        }
        stopStreaming()
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

    private static let silentLevels = [Float](repeating: 0, count: levelHistoryCount)

    private func push(level: Float) {
        guard isConnected, !isMuted else { return }
        // Fast attack, slower release, so speech doesn't flicker.
        let previous = levels.last ?? 0
        let smoothed = level >= previous ? level : previous * 0.6 + level * 0.4
        levels.removeFirst()
        levels.append(smoothed)
    }

    private func handle(_ state: NWConnection.State) {
        switch state {
        case .ready:
            if phase == .connecting { phase = .live }
        case .failed(let error):
            if phase == .live { dropouts += 1 }
            stopStreaming()
            phase = .failed("Lost \(computer?.name ?? "the computer"): \(error.localizedDescription)")
        default:
            break
        }
    }

    private func stopStreaming() {
        capture.stop()
        sender?.cancel()
        sender = nil
        startedAt = nil
        isMuted = false
        gate.set(muted: false)
        levels = Self.silentLevels
    }

    private func remember(_ computer: Computer) {
        recentComputer = computer
        if let data = try? JSONEncoder().encode(computer) {
            defaults.set(data, forKey: Keys.recent)
        }
    }
}

/// Mute state readable from the audio processor thread.
private final class MuteGate: Sendable {
    private let muted = Atomic<Bool>(false)

    var isMuted: Bool { muted.load(ordering: .relaxed) }

    func set(muted value: Bool) {
        muted.store(value, ordering: .relaxed)
    }
}
