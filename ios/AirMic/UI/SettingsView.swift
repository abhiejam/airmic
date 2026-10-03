import SwiftUI

/// Settings (M4.9), in the mockups' visual language: cream background, rounded cards.
struct SettingsView: View {
    @Environment(StreamSession.self) private var session
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var forgetting: Computer?

    private static let sourceURL = URL(string: "https://github.com/abhiejam/airmic")!

    var body: some View {
        @Bindable var session = session
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 28) {
                    HStack {
                        Text("Settings")
                            .scaledFont(28, weight: .semibold, relativeTo: .title)
                            .tracking(-0.6)
                            .accessibilityAddTraits(.isHeader)
                        Spacer()
                        RoundIconButton(systemImage: "xmark", label: "Close") { dismiss() }
                    }

                    section("Computers") {
                        if session.knownComputers.isEmpty {
                            Text("Computers you connect to show up here.")
                                .scaledFont(15, relativeTo: .subheadline)
                                .foregroundStyle(Theme.muted)
                                .padding(16)
                        }
                        ForEach(Array(session.knownComputers.enumerated()), id: \.element.id) { index, computer in
                            if index > 0 { divider }
                            computerRow(computer)
                        }
                    }

                    section("Focus") {
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 2) {
                                Text("Session goal").scaledFont(16, relativeTo: .body)
                                Text("Fills the bar under the timer")
                                    .scaledFont(13, relativeTo: .footnote)
                                    .foregroundStyle(Theme.muted)
                            }
                            Spacer()
                            GoalStepper(minutes: $session.goalMinutes)
                        }
                        .padding(16)
                    }

                    section("Audio") {
                        VStack(alignment: .leading, spacing: 12) {
                            Picker("Audio mode", selection: $session.voiceProcessing) {
                                Text("Voice").tag(true)
                                Text("Raw").tag(false)
                            }
                            .pickerStyle(.segmented)
                            Text(session.voiceProcessing
                                 ? "Reduces background noise and echo. Best for calls and dictation."
                                 : "Sends the microphone as is, with no processing. Best for music or your own filters.")
                                .scaledFont(13, relativeTo: .footnote)
                                .foregroundStyle(Theme.muted)
                                .fixedSize(horizontal: false, vertical: true)
                            if session.isActive {
                                Text("Applies from the next session.")
                                    .scaledFont(13, relativeTo: .footnote)
                                    .foregroundStyle(Theme.muted)
                            }
                        }
                        .padding(16)
                        divider
                        Toggle(isOn: $session.hapticsEnabled) {
                            Text("Haptics").scaledFont(16, relativeTo: .body)
                        }
                        .tint(Theme.accent)
                        .padding(16)
                    }

                    section("About") {
                        row(title: "Version", value: Self.version)
                        divider
                        Button { openURL(Self.sourceURL) } label: {
                            row(title: "Source code", value: "GitHub", systemImage: "arrow.up.right")
                        }
                        .buttonStyle(.plain)
                        divider
                        row(title: "Licence", value: "MIT")
                        divider
                        NavigationLink {
                            DebugStreamView()
                                .scrollContentBackground(.hidden)
                                .background(Theme.bg)
                        } label: {
                            row(title: "Debug stream", value: "Raw UDP", systemImage: "chevron.right")
                        }
                        .buttonStyle(.plain)
                        .disabled(session.isActive)
                        .opacity(session.isActive ? 0.45 : 1)
                    }
                }
                .padding(.horizontal, Theme.screenPadding)
                .padding(.top, 20)
                .padding(.bottom, 32)
            }
            .background(Theme.bg.ignoresSafeArea())
            .foregroundStyle(Theme.ink)
            .toolbar(.hidden, for: .navigationBar)
        }
        .confirmationDialog(
            "Forget \(forgetting?.name ?? "this computer")?",
            isPresented: Binding(get: { forgetting != nil }, set: { if !$0 { forgetting = nil } }),
            titleVisibility: .visible
        ) {
            Button("Forget", role: .destructive) {
                if let forgetting { session.forget(forgetting) }
                forgetting = nil
            }
        } message: {
            Text("You'll need the code on that computer to pair again.")
        }
    }

    private static var version: String {
        let info = Bundle.main.infoDictionary
        let short = info?["CFBundleShortVersionString"] as? String ?? "–"
        let build = info?["CFBundleVersion"] as? String ?? "–"
        return "\(short) (\(build))"
    }

    // MARK: - Pieces

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).sectionLabelStyle().accessibilityAddTraits(.isHeader)
            VStack(alignment: .leading, spacing: 0, content: content)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: Theme.cardRadius))
                .overlay(RoundedRectangle(cornerRadius: Theme.cardRadius).strokeBorder(Theme.line))
        }
    }

    private var divider: some View {
        Rectangle().fill(Theme.line).frame(height: 1).padding(.leading, 16)
    }

    private func row(title: String, value: String, systemImage: String? = nil) -> some View {
        HStack(spacing: 8) {
            Text(title).scaledFont(16, relativeTo: .body)
            Spacer()
            Text(value)
                .scaledFont(15, relativeTo: .subheadline)
                .foregroundStyle(Theme.muted)
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.muted)
            }
        }
        .padding(16)
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }

    private func computerRow(_ computer: Computer) -> some View {
        let paired = PairingTokens.token(for: computer.id) != nil
        let isCurrent = session.isActive && session.computer?.id == computer.id
        return HStack(spacing: 12) {
            Image(systemName: "desktopcomputer")
                .font(.system(size: 19))
                .foregroundStyle(isCurrent ? Theme.accent : Theme.muted)
                .frame(width: 44, height: 44)
                .background(isCurrent ? Theme.accentSoft : Theme.bg, in: RoundedRectangle(cornerRadius: 14))
            VStack(alignment: .leading, spacing: 2) {
                Text(computer.name)
                    .scaledFont(16, weight: .semibold, relativeTo: .body)
                    .lineLimit(1)
                Text([isCurrent ? "Connected" : nil, paired ? "Paired" : nil, computer.host].compactMap { $0 }.joined(separator: " · "))
                    .scaledFont(12, design: .monospaced, relativeTo: .footnote)
                    .foregroundStyle(Theme.muted)
                    .lineLimit(1)
            }
            Spacer()
            Button("Forget") { forgetting = computer }
                .scaledFont(15, weight: .medium, relativeTo: .subheadline)
                .foregroundStyle(Theme.warn)
                .buttonStyle(.plain)
                .disabled(isCurrent)
                .opacity(isCurrent ? 0.4 : 1)
                .accessibilityLabel("Forget \(computer.name)")
        }
        .padding(16)
    }
}

/// − 50 min + with 44 pt round buttons.
private struct GoalStepper: View {
    @Binding var minutes: Int
    private let range = 10...180
    private let step = 5

    var body: some View {
        HStack(spacing: 10) {
            button("minus", label: "Shorter", enabled: minutes > range.lowerBound) { minutes = max(range.lowerBound, minutes - step) }
            Text("\(minutes) min")
                .scaledFont(16, weight: .medium, relativeTo: .body)
                .monospacedDigit()
                .frame(minWidth: 64)
            button("plus", label: "Longer", enabled: minutes < range.upperBound) { minutes = min(range.upperBound, minutes + step) }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Session goal")
        .accessibilityValue("\(minutes) minutes")
        .accessibilityAdjustableAction { direction in
            switch direction {
            case .increment: minutes = min(range.upperBound, minutes + step)
            case .decrement: minutes = max(range.lowerBound, minutes - step)
            @unknown default: break
            }
        }
    }

    private func button(_ systemImage: String, label: String, enabled: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: 15, weight: .semibold))
                .frame(width: 44, height: 44)
                .background(Theme.bg, in: Circle())
                .overlay(Circle().strokeBorder(Theme.line))
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .accessibilityLabel(label)
    }
}

#Preview {
    SettingsView()
        .environment(StreamSession())
}
