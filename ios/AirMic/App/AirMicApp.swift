import SwiftData
import SwiftUI

@main
struct AirMicApp: App {
    @State private var session = StreamSession()

    var body: some Scene {
        WindowGroup {
            HomeView()
                .environment(session)
                .tint(Theme.accent)
        }
        .modelContainer(for: FocusSession.self)
    }
}
