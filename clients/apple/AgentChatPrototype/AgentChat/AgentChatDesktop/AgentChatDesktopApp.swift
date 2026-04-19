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
    @StateObject private var env = DesktopEnvironment()
    @State private var openWindows: [UUID: WindowProxy] = [:]
    @State private var didBootstrapStore = false

    private func bootstrapStoreIfNeeded() {
        guard !didBootstrapStore else { return }
        didBootstrapStore = true
        env.start()
    }

    var body: some Scene {
        WindowGroup("AgentChat Desktop") {
            RootView()
                .environmentObject(env.workspace)
                .onAppear {
                    bootstrapStoreIfNeeded()
                }
                .onOpenURL { url in
                    if let projectID = AgentChatDesktopURL.projectID(from: url) {
                        env.workspace.selectProject(projectID)
                    } else if let issueID = AgentChatDesktopURL.issueID(from: url) {
                        env.workspace.selectIssue(issueID)
                    } else if let artifactID = AgentChatDesktopURL.artifactID(from: url) {
                        env.workspace.selectArtifact(artifactID)
                    } else if let threadIDStr = AgentChatDesktopURL.threadID(from: url) {
                        if let uuid = UUID(uuidString: threadIDStr), env.workspace.selectWorkspaceThread(uuid) {
                            // navigated to local workspace thread
                        } else if !env.workspace.selectDaemonThread(threadIDStr) {
                            env.attachThread(threadIDStr)
                        }
                    } else {
                        env.applyScannedConnectionPayload(url.absoluteString)
                    }
                }
        }
        .defaultSize(width: 1440, height: 920)
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .commands {
            AgentChatDesktopCommands(env: env)
        }

        Settings {
            AgentChatDesktopSettingsView()
                .environmentObject(env.workspace)
                .environmentObject(env)
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

    static func projectID(from url: URL) -> UUID? {
        uuidComponent(from: url, host: "project")
    }

    static func issueID(from url: URL) -> UUID? {
        uuidComponent(from: url, host: "issue")
    }

    static func artifactID(from url: URL) -> UUID? {
        uuidComponent(from: url, host: "artifact")
    }

    static func projectLink(for projectID: UUID) -> URL? {
        URL(string: "agentchat://project/\(projectID.uuidString)")
    }

    static func issueLink(for issueID: UUID) -> URL? {
        URL(string: "agentchat://issue/\(issueID.uuidString)")
    }

    static func workspaceThreadLink(for threadID: UUID) -> URL? {
        URL(string: "agentchat://thread/\(threadID.uuidString)")
    }

    static func artifactLink(for artifactID: UUID) -> URL? {
        URL(string: "agentchat://artifact/\(artifactID.uuidString)")
    }

    private static func uuidComponent(from url: URL, host: String) -> UUID? {
        guard url.scheme?.localizedCaseInsensitiveCompare("agentchat") == .orderedSame,
              url.host?.localizedCaseInsensitiveCompare(host) == .orderedSame else { return nil }
        let component = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !component.isEmpty else { return nil }
        return UUID(uuidString: component.removingPercentEncoding ?? component)
    }
}