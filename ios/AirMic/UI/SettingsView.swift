import SwiftUI

/// Not mocked yet (M4.9): a plain grouped list in the app's colors.
struct SettingsView: View {
    @Environment(StreamSession.self) private var session
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var session = session
        NavigationStack {
            Form {
                Section("Computer") {
                    if let computer = session.recentComputer {
                        LabeledContent(computer.name, value: "\(computer.host):\(computer.port)")
                        Button("Forget this computer", role: .destructive) { session.forgetRecent() }
                    } else {
                        Text("No computer yet").foregroundStyle(Theme.muted)
                    }
                }
                Section("Focus") {
                    Stepper("Goal \(session.goalMinutes) min", value: $session.goalMinutes, in: 10...180, step: 5)
                }
                Section {
                    Picker("Audio mode", selection: $session.voiceProcessing) {
                        Text("Voice").tag(true)
                        Text("Raw").tag(false)
                    }
                    Toggle("Haptics", isOn: $session.hapticsEnabled)
                } footer: {
                    Text("Voice reduces background noise and echo. Raw sends the microphone as is. Applies from the next session.")
                }
                Section("Developer") {
                    NavigationLink("Debug stream") { DebugStreamView() }
                        .disabled(session.isActive)
                }
                Section {
                    LabeledContent("Version", value: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "–")
                }
            }
            .scrollContentBackground(.hidden)
            .background(Theme.bg)
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}
