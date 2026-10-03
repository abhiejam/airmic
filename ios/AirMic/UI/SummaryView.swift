import Charts
import SwiftData
import SwiftUI

struct SummaryView: View {
    let focus: FocusSession
    let onStartAnother: () -> Void
    @Environment(\.dismiss) private var dismiss
    @Query(sort: \FocusSession.start, order: .reverse) private var sessions: [FocusSession]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Session complete").sectionLabelStyle()
                Spacer()
                RoundIconButton(systemImage: "xmark", label: "Close") { dismiss() }
            }

            Text(SessionFormat.focusTitle(seconds: focus.duration))
                .scaledFont(28, weight: .semibold, relativeTo: .title)
                .accessibilityAddTraits(.isHeader)
                .tracking(-0.6)
                .padding(.top, 20)
            Text("Streamed to \(focus.computerName) · \(timeRange)")
                .scaledFont(15, relativeTo: .subheadline)
                .foregroundStyle(Theme.muted)
                .padding(.top, 8)

            HStack(spacing: 10) {
                stat(focus.words.map { $0.formatted() } ?? "—", "words spoken")
                stat("\(focus.mutes)", focus.mutes == 1 ? "time muted" : "times muted")
                stat("\(focus.dropouts)", focus.dropouts == 1 ? "dropout" : "dropouts")
            }
            .padding(.top, 32)

            weekCard.padding(.top, 10)

            if let transcript = focus.lastTranscript {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Last thing you said")
                        .scaledFont(11, relativeTo: .caption2).tracking(1.1).textCase(.uppercase)
                        .foregroundStyle(Theme.muted)
                    Text("“\(transcript)”")
                        .scaledFont(16, relativeTo: .body)
                        .lineSpacing(4)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(16)
                .card()
                .padding(.top, 10)
            }

            Spacer()

            PrimaryButton(title: "Start another session", action: onStartAnother)
                .frame(maxWidth: .infinity)
        }
        .padding(.horizontal, Theme.screenPadding)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .background(Theme.bg.ignoresSafeArea())
        .foregroundStyle(Theme.ink)
    }

    private static let dayNames = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"]

    private var timeRange: String {
        let style = Date.FormatStyle(date: .omitted, time: .shortened)
        return "\(focus.start.formatted(style)) – \(focus.end.formatted(style))"
    }

    private func stat(_ value: String, _ label: String) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(value)
                .scaledFont(22, weight: .semibold, relativeTo: .title2)
                .tracking(-0.4)
            Text(label)
                .scaledFont(12, relativeTo: .footnote)
                .foregroundStyle(Theme.muted)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .card(radius: 18)
        .accessibilityElement(children: .combine)
    }

    private var weekCard: some View {
        let now = Date.now
        let minutes = WeekStats.minutesPerDay(sessions.map { ($0.start, $0.duration) }, now: now, calendar: .current)
        let today = WeekStats.todayIndex(now: now, calendar: .current)
        let labels = ["M", "T", "W", "T", "F", "S", "S"]
        let total = Int(minutes.reduce(0, +))
        let top = max(minutes.max() ?? 0, 1)

        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text("This week").scaledFont(14, weight: .semibold, relativeTo: .subheadline)
                Spacer()
                Text("\(SessionFormat.hoursMinutes(total)) focused")
                    .scaledFont(13, design: .monospaced, relativeTo: .footnote)
                    .foregroundStyle(Theme.muted)
            }
            Chart(minutes.indices, id: \.self) { day in
                BarMark(
                    x: .value("Day", day),
                    // A sliver for empty days, like the mockup.
                    y: .value("Minutes", max(minutes[day], top * 0.055)),
                    width: 22)
                    .cornerRadius(6)
                    .foregroundStyle(day == today ? Theme.accent : (minutes[day] > 0 ? Theme.accentBar : Theme.line))
                    .accessibilityLabel(Self.dayNames[day] + (day == today ? ", today" : ""))
                    .accessibilityValue("\(Int(minutes[day].rounded())) minutes")
            }
            .chartYAxis(.hidden)
            .chartXScale(domain: -0.5...6.5)
            .chartXAxis {
                AxisMarks(values: Array(0..<7)) { value in
                    AxisValueLabel(centered: false) {
                        if let day = value.as(Int.self) {
                            Text(labels[day])
                                .scaledFont(11, relativeTo: .caption2)
                                .foregroundStyle(day == today ? Theme.ink : Theme.muted)
                        }
                    }
                }
            }
            .frame(height: 92)
        }
        .padding(16)
        .card()
    }
}

private extension View {
    func card(radius: CGFloat = 20) -> some View {
        background(Theme.surface, in: RoundedRectangle(cornerRadius: radius))
            .overlay(RoundedRectangle(cornerRadius: radius).strokeBorder(Theme.line))
    }
}
