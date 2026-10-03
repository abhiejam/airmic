import Network
import SwiftUI

/// M1 spike screen: stream raw PCM to a typed IP and port (the Linux netcat receiver).
@MainActor
@Observable
final class DebugStreamModel {
    var host: String {
        didSet { UserDefaults.standard.set(host, forKey: "debug.host") }
    }
    var port: String {
        didSet { UserDefaults.standard.set(port, forKey: "debug.port") }
    }
    var voiceProcessing = true

    private(set) var isRunning = false
    private(set) var rms: Float = 0
    private(set) var framesSent: UInt32 = 0
    private(set) var status = "Idle"

    private let capture = AudioCapture()
    private var sender: RawUDPSender?

    init() {
        host = UserDefaults.standard.string(forKey: "debug.host") ?? ""
        port = UserDefaults.standard.string(forKey: "debug.port") ?? "5555"
    }

    var levelDBFS: String {
        guard rms > 0 else { return "-∞ dBFS" }
        return String(format: "%.1f dBFS", 20 * log10(rms))
    }

    func toggle() async {
        if isRunning { stop() } else { await start() }
    }

    func start() async {
        guard let portNumber = UInt16(port), !host.isEmpty,
              let sender = RawUDPSender(host: host, port: portNumber)
        else {
            status = "Enter an IP address and port"
            return
        }
        guard await AudioCapture.requestPermission() else {
            status = "Microphone permission denied. Allow it in Settings."
            return
        }

        sender.start { [weak self] state in
            Task { @MainActor in self?.status = Self.describe(state) }
        }
        do {
            capture.voiceProcessing = voiceProcessing
            try capture.start { [weak self] frame in
                sender.send(frame.pcm)
                // UI at about 30 fps.
                if frame.index % 3 == 0 {
                    Task { @MainActor in
                        self?.rms = frame.rms
                        self?.framesSent = frame.index + 1
                    }
                }
            }
        } catch {
            sender.cancel()
            status = "Audio failed: \(error.localizedDescription)"
            return
        }
        self.sender = sender
        isRunning = true
    }

    func stop() {
        capture.stop()
        sender?.cancel()
        sender = nil
        isRunning = false
        rms = 0
        status = "Stopped"
    }

    private static func describe(_ state: NWConnection.State) -> String {
        switch state {
        case .setup, .preparing: "Connecting…"
        case .ready: "Streaming"
        case .waiting(let error): "Waiting: \(error.localizedDescription)"
        case .failed(let error): "Failed: \(error.localizedDescription)"
        case .cancelled: "Stopped"
        @unknown default: "Unknown"
        }
    }
}

struct DebugStreamView: View {
    @State private var model = DebugStreamModel()

    var body: some View {
        NavigationStack {
            Form {
                Section("Receiver") {
                    TextField("IP address", text: $model.host)
                        .keyboardType(.decimalPad)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    TextField("Port", text: $model.port)
                        .keyboardType(.numberPad)
                    Toggle("Voice processing", isOn: $model.voiceProcessing)
                }
                .disabled(model.isRunning)

                Section {
                    Button(model.isRunning ? "Stop" : "Start") {
                        Task { await model.toggle() }
                    }
                    .font(.headline)
                    .frame(maxWidth: .infinity)
                }

                Section("Live") {
                    LabeledContent("Status", value: model.status)
                    LabeledContent("RMS", value: String(format: "%.4f", model.rms))
                    LabeledContent("Level", value: model.levelDBFS)
                    LabeledContent("Frames sent", value: "\(model.framesSent)")
                    ProgressView(value: Double(min(1, model.rms * 4)))
                }
                .monospacedDigit()
            }
            .navigationTitle("AirMic debug")
        }
    }
}

#Preview {
    DebugStreamView()
}
