import SwiftUI
import AppKit

extension Notification.Name {
    static let agentChatDesktopToggleSidebar = Notification.Name("agentchat.desktop.toggleSidebar")
}

extension NSResponder {
    @objc func agentChatToggleSidebar(_ sender: Any?) {
        nextResponder?.tryToPerform(#selector(agentChatToggleSidebar(_:)), with: sender)
    }
}

@MainActor
final class AgentChatDesktopAppDelegate: NSObject, NSApplicationDelegate {
    private let toggleSidebarMenuItemTag = 4_207

    func applicationDidFinishLaunching(_ notification: Notification) {
        DispatchQueue.main.async { [weak self] in
            self?.installToggleSidebarMenuItemIfNeeded()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        LocalDaemonController.shared.stopManagedDaemonIfNeeded()
    }

    private func installToggleSidebarMenuItemIfNeeded() {
        guard let mainMenu = NSApp.mainMenu else { return }
        let targetMenu = resolveViewMenu(in: mainMenu)

        for existingItem in targetMenu.items where existingItem.tag == toggleSidebarMenuItemTag {
            targetMenu.removeItem(existingItem)
        }

        if let lastItem = targetMenu.items.last, !lastItem.isSeparatorItem {
            targetMenu.addItem(.separator())
        }

        let item = NSMenuItem(
            title: "Toggle Sidebar",
            action: #selector(NSResponder.agentChatToggleSidebar(_:)),
            keyEquivalent: "b"
        )
        item.tag = toggleSidebarMenuItemTag
        item.keyEquivalentModifierMask = [.command]
        item.target = nil
        targetMenu.addItem(item)
    }

    private func resolveViewMenu(in mainMenu: NSMenu) -> NSMenu {
        for item in mainMenu.items {
            if let submenu = item.submenu {
                for subItem in submenu.items {
                    if subItem.action == #selector(NSWindow.toggleFullScreen(_:)) {
                        return submenu
                    }
                }
            }
        }

        let viewMenu = NSMenu(title: "View")
        let viewMenuItem = NSMenuItem(title: "View", action: nil, keyEquivalent: "")
        viewMenuItem.submenu = viewMenu

        if let windowMenuIndex = mainMenu.items.firstIndex(where: { $0.title == NSLocalizedString("Window", comment: "") }) {
            mainMenu.insertItem(viewMenuItem, at: windowMenuIndex)
        } else {
            mainMenu.addItem(viewMenuItem)
        }

        return viewMenu
    }
}

@main
struct AgentChatDesktopApp: App {
    @NSApplicationDelegateAdaptor(AgentChatDesktopAppDelegate.self) private var appDelegate
    @StateObject private var store = DaemonChatStore()
    @State private var openWindows: [UUID: WindowProxy] = [:]

    private func ensureLocalDaemonIfNeeded() {
        Task {
            await LocalDaemonController.shared.ensureRunning(for: store.daemonURL)
        }
    }

    var body: some Scene {
        WindowGroup("AgentChat Desktop") {
            AgentChatDesktopRootView()
                .environmentObject(store)
                .onAppear {
                    ensureLocalDaemonIfNeeded()
                    store.start()
                    Task {
                        await NotificationHelper.requestAuthorization()
                    }
                }
                .onChange(of: store.daemonURL) { _, _ in
                    ensureLocalDaemonIfNeeded()
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
