import SwiftUI

@main
struct AgentChatDesktopApp: App {
    @StateObject private var store = DaemonChatStore()

    var body: some Scene {
        WindowGroup("AgentChat Desktop") {
            AgentChatDesktopRootView()
                .environmentObject(store)
                .onAppear {
                    store.start()
                }
                .onOpenURL { url in
                    store.applyScannedConnectionPayload(url.absoluteString)
                }
        }
        .defaultSize(width: 1440, height: 920)
        .windowStyle(.hiddenTitleBar)
        .commands {
            AgentChatDesktopCommands(store: store)
        }

        Settings {
            AgentChatDesktopSettingsView()
                .environmentObject(store)
        }
    }
}
