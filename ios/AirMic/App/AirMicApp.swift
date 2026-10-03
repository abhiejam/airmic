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
                // Screens are laid out for one phone screen; past this, controls would leave it.
                .dynamicTypeSize(...DynamicTypeSize.accessibility2)
                .task { await session.autoConnect() }
                // airmic://pair?... from the Camera app or another QR reader.
                .onOpenURL { url in
                    guard let link = PairingLink(url: url) else { return }
                    Task { await session.connect(to: link.computer, pairingCode: link.code) }
                }
        }
        .modelContainer(for: FocusSession.self)
    }
}
