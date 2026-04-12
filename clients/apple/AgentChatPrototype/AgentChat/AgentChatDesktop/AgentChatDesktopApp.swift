import SwiftUI

@main
struct AgentChatDesktopApp: App {
    @StateObject private var store = DaemonChatStore()
    @State private var openWindows: [UUID: WindowProxy] = [:]

    var body: some Scene {
        WindowGroup("AgentChat Desktop") {
            AgentChatDesktopRootView()
                .environmentObject(store)
                .onAppear {
                    store.start()
                    Task {
                        await NotificationHelper.requestAuthorization()
                    }
                }
                .onOpenURL { url in
                    store.applyScannedConnectionPayload(url.absoluteString)
                }
        }
        .defaultSize(width: 1440, height: 920)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .commands {
            AgentChatDesktopCommands(store: store)
        }

        Settings {
            AgentChatDesktopSettingsView()
                .environmentObject(store)
        }
    }
}

struct WindowProxy: Identifiable {
    let id = UUID()
    var threadID: String
    var threadTitle: String
}
