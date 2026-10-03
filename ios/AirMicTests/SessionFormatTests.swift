import Foundation
import Testing
@testable import AirMic

struct SessionFormatTests {
    @Test func timer() {
        #expect(SessionFormat.timer(0) == "00:00")
        #expect(SessionFormat.timer(24 * 60 + 18) == "24:18")
        #expect(SessionFormat.timer(3723) == "1:02:03")
    }

    @Test func focusTitle() {
        #expect(SessionFormat.focusTitle(seconds: 30) == "Under a minute of deep work")
        #expect(SessionFormat.focusTitle(seconds: 90) == "1 minute of deep work")
        #expect(SessionFormat.focusTitle(seconds: 52 * 60 + 10) == "52 minutes of deep work")
    }

    @Test func goalText() {
        #expect(SessionFormat.goalText(elapsedSeconds: 24 * 60 + 18, goalMinutes: 50) == "26 min to your 50 min goal")
        #expect(SessionFormat.goalText(elapsedSeconds: 50 * 60, goalMinutes: 50) == "Goal reached · 50 min")
    }

    @Test func hoursMinutes() {
        #expect(SessionFormat.hoursMinutes(260) == "4h 20m")
        #expect(SessionFormat.hoursMinutes(35) == "35m")
    }

    @Test func levelMapping() {
        #expect(StreamSession.normalizedLevel(rms: 0) == 0)
        #expect(StreamSession.normalizedLevel(rms: 1) == 1)
        // -55 dBFS and below is silence.
        #expect(StreamSession.normalizedLevel(rms: 0.001) == 0)
        let speech = StreamSession.normalizedLevel(rms: 0.1) // -20 dBFS
        #expect(speech > 0.7 && speech < 0.8)
    }

    @Test func manualHostValidation() {
        #expect(ManualEntrySheet.isValidHost("192.168.20.212"))
        #expect(ManualEntrySheet.isValidHost("ubuntu-desk.local"))
        #expect(!ManualEntrySheet.isValidHost("192.168.20"))
        #expect(!ManualEntrySheet.isValidHost("192.168.1.300"))
        #expect(!ManualEntrySheet.isValidHost(""))
        #expect(!ManualEntrySheet.isValidHost("bad host"))
    }
}

struct WeekStatsTests {
    private var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Australia/Sydney")!
        return calendar
    }

    private func date(_ day: Int, _ hour: Int) -> Date {
        calendar.date(from: DateComponents(year: 2026, month: 10, day: day, hour: hour))!
    }

    @Test func groupsByDayMondayFirst() {
        // Saturday 3 Oct 2026; the week runs Mon 28 Sep – Sun 4 Oct.
        let now = date(3, 15)
        let sessions: [(start: Date, duration: TimeInterval)] = [
            (calendar.date(from: DateComponents(year: 2026, month: 9, day: 28, hour: 9))!, 48 * 60),
            (date(3, 9), 30 * 60),
            (date(3, 11), 22 * 60),
            (calendar.date(from: DateComponents(year: 2026, month: 9, day: 27, hour: 9))!, 60 * 60), // last week
        ]
        let minutes = WeekStats.minutesPerDay(sessions, now: now, calendar: calendar)
        #expect(minutes == [48, 0, 0, 0, 0, 52, 0])
        #expect(WeekStats.todayIndex(now: now, calendar: calendar) == 5)
    }
}
