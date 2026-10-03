import SwiftData
import SwiftUI

struct HomeView: View {
    @Environment(StreamSession.self) private var session
    @Environment(\.modelContext) private var modelContext
    @State private var showConnect = false
    @State private var showSettings = false
    @State private var summary: FocusSession?

    private enum Mode { case live, muted, idle }

    private var mode: Mode {
        guard session.isConnected else { return .idle }
        return session.isMuted ? .muted : .live
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                header
                StatusPill(dot: statusDot, text: statusText)
                    .padding(.top, 28)
                FocusTimer(startedAt: session.startedAt, goalMinutes: session.goalMinutes)
                    .padding(.top, 32)

                VStack(spacing: 20) {
                    MicHero(mode: mode, level: session.levels.last ?? 0) { showConnect = true }
                    LevelBars(levels: session.levels, active: mode == .live)
                    Text(caption)
                        .font(.system(size: 14))
                        .foregroundStyle(Theme.muted)
                }
                .frame(maxHeight: .infinity)

                footer
            }
            .padding(.horizontal, Theme.screenPadding)
            .padding(.top, 8)
            .padding(.bottom, 12)
            .background(Theme.bg.ignoresSafeArea())
            .foregroundStyle(Theme.ink)
            .toolbar(.hidden, for: .navigationBar)
            .navigationDestination(isPresented: $showConnect) { ConnectView() }
            .sheet(isPresented: $showSettings) { SettingsView() }
            .fullScreenCover(item: $summary) { finished in
                SummaryView(focus: finished) {
                    summary = nil
                    if let computer = session.recentComputer {
                        Task { await session.connect(to: computer) }
                    }
                }
            }
        }
    }

    private var header: some View {
        HStack {
            HStack(spacing: 6) {
                AirMicLogo(size: 28)
                Text("AirMic")
                    .font(.system(size: 20, weight: .semibold))
                    .tracking(-0.6)
            }
            .accessibilityElement(children: .combine)
            Spacer()
            RoundIconButton(systemImage: "slider.horizontal.3", label: "Settings") { showSettings = true }
        }
    }

    @ViewBuilder
    private var footer: some View {
        if mode == .idle {
            PrimaryButton(title: "Connect to a computer") { showConnect = true }
                .padding(.top, 32)
        } else {
            HStack {
                Button(action: endSession) {
                    Label("End session", systemImage: "stop")
                        .font(.system(size: 15, weight: .medium))
                        .padding(.horizontal, 20)
                        .frame(height: 48)
                        .background(Theme.surface, in: Capsule())
                        .overlay(Capsule().strokeBorder(Theme.line))
                }
                .buttonStyle(.plain)
                Spacer()
                MuteButton(muted: session.isMuted, haptics: session.hapticsEnabled) { session.toggleMute() }
            }
        }
    }

    private func endSession() {
        guard let finished = session.end() else { return }
        modelContext.insert(finished)
        summary = finished
    }

    private var computerName: String { session.computer?.name ?? "computer" }

    private var statusDot: Color {
        switch session.phase {
        case .idle, .failed: Theme.muted
        case .connecting: Theme.accent
        case .live: session.isMuted ? Theme.warn : Theme.ok
        }
    }

    private var statusText: String {
        switch session.phase {
        case .idle: "No computer connected"
        case .failed(let reason): reason
        case .connecting: "Connecting to \(computerName)"
        case .live: session.isMuted ? "Off air · \(computerName)" : "On air · \(computerName)"
        }
    }

    private var caption: String {
        switch mode {
        case .idle: "Tap the mic to find your computer"
        case .muted: "Muted · your PC hears silence"
        case .live: "Listening"
        }
    }

    // MARK: - Pieces

    private struct StatusPill: View {
        let dot: Color
        let text: String

        var body: some View {
            HStack(spacing: 8) {
                Circle().fill(dot).frame(width: 8, height: 8)
                Text(text).lineLimit(1)
            }
            .font(.system(size: 14))
            .padding(.vertical, 8)
            .padding(.horizontal, 14)
            .background(Theme.surface, in: Capsule())
            .overlay(Capsule().strokeBorder(Theme.line))
            .accessibilityElement(children: .combine)
        }
    }

    private struct FocusTimer: View {
        let startedAt: Date?
        let goalMinutes: Int

        var body: some View {
            TimelineView(.periodic(from: .now, by: 1)) { context in
                let elapsed = startedAt.map { Int(context.date.timeIntervalSince($0)) } ?? 0
                let progress = min(1, Double(elapsed) / Double(goalMinutes * 60))
                VStack(spacing: 6) {
                    Text(startedAt == nil ? "Ready when you are" : "Focus session")
                        .sectionLabelStyle()
                    Text(SessionFormat.timer(elapsed))
                        .font(.system(size: 72, weight: .light))
                        .tracking(-2)
                        .monospacedDigit()
                    Capsule()
                        .fill(Theme.line)
                        .frame(width: 140, height: 4)
                        .overlay(alignment: .leading) {
                            Capsule().fill(Theme.accent).frame(width: 140 * progress)
                        }
                        .padding(.top, 10)
                    Text(startedAt == nil
                         ? "Goal \(goalMinutes) min"
                         : SessionFormat.goalText(elapsedSeconds: elapsed, goalMinutes: goalMinutes))
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.muted)
                        .padding(.top, 4)
                }
                .accessibilityElement(children: .combine)
            }
        }
    }

    private struct MicHero: View {
        let mode: Mode
        let level: Float
        let onConnect: () -> Void
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        var body: some View {
            ZStack {
                switch mode {
                case .live:
                    Circle()
                        .fill(Theme.accentSoft)
                        .frame(width: 260, height: 260)
                        .scaleEffect(reduceMotion ? 1 : 0.94 + 0.06 * CGFloat(level))
                    if !reduceMotion { PulseRings() }
                    Circle()
                        .fill(Theme.accent)
                        .frame(width: 188, height: 188)
                        .shadow(color: Theme.accentShadow, radius: 24, y: 20)
                        .overlay(micIcon(slashed: false).foregroundStyle(Theme.onAccent))
                case .muted:
                    Circle()
                        .fill(Theme.surface)
                        .overlay(Circle().strokeBorder(Theme.line))
                        .frame(width: 188, height: 188)
                        .overlay(micIcon(slashed: true).foregroundStyle(Theme.muted))
                case .idle:
                    Button(action: onConnect) {
                        Circle()
                            .strokeBorder(Theme.accent, style: StrokeStyle(lineWidth: 2, dash: [7, 6]))
                            .frame(width: 188, height: 188)
                            .overlay(micIcon(slashed: false).foregroundStyle(Theme.accent))
                            .contentShape(Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Connect to a computer")
                }
            }
            .frame(width: 272, height: 272)
            .animation(.easeOut(duration: 0.12), value: level)
            .accessibilityHidden(mode != .idle)
        }

        private func micIcon(slashed: Bool) -> some View {
            Image(systemName: slashed ? "mic.slash" : "mic")
                .font(.system(size: 58, weight: .light))
        }
    }

    /// Three rings that grow and fade, 0.8 s apart.
    private struct PulseRings: View {
        var body: some View {
            TimelineView(.animation) { context in
                let time = context.date.timeIntervalSinceReferenceDate
                ZStack {
                    ForEach(0..<3, id: \.self) { ring in
                        let phase = (time - Double(ring) * 0.8).truncatingRemainder(dividingBy: 2.4) / 2.4
                        let eased = 1 - (1 - abs(phase)) * (1 - abs(phase))
                        Circle()
                            .stroke(Theme.accent, lineWidth: 2)
                            .frame(width: 188, height: 188)
                            .scaleEffect(1 + 0.55 * eased)
                            .opacity(0.55 * (1 - eased))
                    }
                }
            }
        }
    }

    /// 28 bars of recent mic level, newest on the right.
    private struct LevelBars: View {
        let levels: [Float]
        let active: Bool

        var body: some View {
            HStack(spacing: 4) {
                ForEach(levels.indices, id: \.self) { index in
                    Capsule()
                        .fill(active ? Theme.accent : Theme.line)
                        .frame(width: 4, height: active ? 4 + 32 * CGFloat(levels[index]) : 4)
                }
            }
            .frame(height: 36)
            .animation(.linear(duration: 0.08), value: levels)
            .accessibilityHidden(true)
        }
    }

    private struct MuteButton: View {
        let muted: Bool
        let haptics: Bool
        let action: () -> Void

        var body: some View {
            Button(action: action) {
                Image(systemName: muted ? "mic.slash" : "mic")
                    .font(.system(size: 26, weight: .medium))
                    .foregroundStyle(muted ? Theme.onWarn : Theme.bg)
                    .frame(width: 68, height: 68)
                    .background(muted ? Theme.warn : Theme.ink, in: Circle())
                    .shadow(color: muted ? Theme.warn.opacity(0.55) : .black.opacity(0.35), radius: 16, y: 12)
                    .contentTransition(.symbolEffect(.replace))
            }
            .buttonStyle(.plain)
            .animation(.snappy(duration: 0.2), value: muted)
            .sensoryFeedback(.impact(weight: .medium), trigger: muted) { _, _ in haptics }
            .accessibilityLabel(muted ? "Unmute" : "Mute")
            .accessibilityValue(muted ? "Muted" : "On air")
        }
    }
}

#Preview {
    HomeView()
        .environment(StreamSession())
        .modelContainer(for: FocusSession.self, inMemory: true)
}
