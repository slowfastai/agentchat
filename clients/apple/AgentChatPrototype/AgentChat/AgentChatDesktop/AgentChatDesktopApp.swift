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
                    if let threadID = AgentChatDesktopURL.threadID(from: url) {
                        store.attachThread(threadID)
                    } else {
                        store.applyScannedConnectionPayload(url.absoluteString)
                    }
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

enum AgentChatDesktopURL {
    private static let threadPathAllowed: CharacterSet = {
        var allowed = CharacterSet.urlPathAllowed
        allowed.remove(charactersIn: "/")
        return allowed
    }()

    static func threadLink(for threadID: String) -> URL? {
        guard let encodedThreadID = threadID.addingPercentEncoding(withAllowedCharacters: threadPathAllowed) else {
            return nil
        }

        return URL(string: "agentchat://thread/\(encodedThreadID)")
    }

    static func threadID(from url: URL) -> String? {
        guard url.scheme?.localizedCaseInsensitiveCompare("agentchat") == .orderedSame,
              url.host?.localizedCaseInsensitiveCompare("thread") == .orderedSame else {
            return nil
        }

        let encodedThreadID = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !encodedThreadID.isEmpty else {
            return nil
        }

        return encodedThreadID.removingPercentEncoding ?? encodedThreadID
    }
}
