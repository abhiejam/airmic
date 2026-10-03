import Foundation
import SwiftData

/// One streaming session, saved for the summary and the weekly chart.
@Model
final class FocusSession {
    var start: Date
    var end: Date
    var computerName: String
    var goalMinutes: Int
    var mutes: Int
    var dropouts: Int
    /// Nil until the desktop sends transcripts (S4).
    var words: Int?
    var lastTranscript: String?

    init(start: Date, end: Date, computerName: String, goalMinutes: Int, mutes: Int, dropouts: Int) {
        self.start = start
        self.end = end
        self.computerName = computerName
        self.goalMinutes = goalMinutes
        self.mutes = mutes
        self.dropouts = dropouts
    }

    var duration: TimeInterval { end.timeIntervalSince(start) }
}

enum SessionFormat {
    /// "24:18", or "1:02:03" past an hour.
    static func timer(_ seconds: Int) -> String {
        let seconds = max(0, seconds)
        let h = seconds / 3600, m = seconds / 60 % 60, s = seconds % 60
        return h > 0
            ? String(format: "%d:%02d:%02d", h, m, s)
            : String(format: "%02d:%02d", m, s)
    }

    static func focusTitle(seconds: TimeInterval) -> String {
        let minutes = Int(seconds / 60)
        switch minutes {
        case 0: return "Under a minute of deep work"
        case 1: return "1 minute of deep work"
        default: return "\(minutes) minutes of deep work"
        }
    }

    static func goalText(elapsedSeconds: Int, goalMinutes: Int) -> String {
        let remaining = goalMinutes - elapsedSeconds / 60
        return remaining > 0
            ? "\(remaining) min to your \(goalMinutes) min goal"
            : "Goal reached · \(goalMinutes) min"
    }

    /// "4h 20m", "35m".
    static func hoursMinutes(_ minutes: Int) -> String {
        minutes >= 60 ? "\(minutes / 60)h \(minutes % 60)m" : "\(minutes)m"
    }
}

enum WeekStats {
    /// Minutes streamed per day of the week containing `now`, Monday first.
    static func minutesPerDay(_ sessions: [(start: Date, duration: TimeInterval)], now: Date, calendar: Calendar) -> [Double] {
        var calendar = calendar
        calendar.firstWeekday = 2
        var days = [Double](repeating: 0, count: 7)
        guard let week = calendar.dateInterval(of: .weekOfYear, for: now) else { return days }
        for session in sessions where week.contains(session.start) {
            if let index = dayIndex(of: session.start, weekStart: week.start, calendar: calendar) {
                days[index] += session.duration / 60
            }
        }
        return days
    }

    /// 0 = Monday … 6 = Sunday.
    static func todayIndex(now: Date, calendar: Calendar) -> Int {
        var calendar = calendar
        calendar.firstWeekday = 2
        guard let week = calendar.dateInterval(of: .weekOfYear, for: now) else { return 0 }
        return dayIndex(of: now, weekStart: week.start, calendar: calendar) ?? 0
    }

    private static func dayIndex(of date: Date, weekStart: Date, calendar: Calendar) -> Int? {
        let days = calendar.dateComponents([.day], from: weekStart, to: calendar.startOfDay(for: date)).day
        return days.flatMap { (0..<7).contains($0) ? $0 : nil }
    }
}

enum Transcript {
    /// Words as people count them: runs of letters or digits, so "PipeWire's" and "48,000" are one each.
    static func wordCount(_ text: String) -> Int {
        text.split { $0.isWhitespace }.filter { $0.contains { $0.isLetter || $0.isNumber } }.count
    }

    /// Trimmed, with runs of whitespace collapsed, for "Last thing you said".
    static func clean(_ text: String) -> String {
        text.split { $0.isWhitespace }.joined(separator: " ")
    }
}
