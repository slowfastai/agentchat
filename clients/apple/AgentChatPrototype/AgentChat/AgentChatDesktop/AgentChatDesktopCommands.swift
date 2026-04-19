import SwiftUI

struct AgentChatDesktopActions {
    let showNewThreadSheet: () -> Void
    let showAddAgentsSheet: () -> Void
    let toggleSidebar: () -> Void
    let focusComposer: () -> Void
}

private struct AgentChatDesktopActionsKey: FocusedValueKey {
    typealias Value = AgentChatDesktopActions
}

extension FocusedValues {
    var agentChatDesktopActions: AgentChatDesktopActions? {
        get { self[AgentChatDesktopActionsKey.self] }
        set { self[AgentChatDesktopActionsKey.self] = newValue }
    }
}

struct AgentChatDesktopCommands: Commands {
    @FocusedValue(\.agentChatDesktopActions) private var actions

    let store: DaemonChatStore
    let workspaceStore: WorkspaceStore

    var body: some Commands {
        CommandMenu("AgentChat") {
            Button("New Thread") {
                actions?.showNewThreadSheet()
            }
            .keyboardShortcut("n")

            Button("Add Agents to Thread") {
                actions?.showAddAgentsSheet()
            }
            .keyboardShortcut("a", modifiers: [.command, .shift])
            .disabled(store.activeThreadID == nil)

            Divider()

            Button("Reconnect") {
                store.reconnectNow()
                Task {
                    await workspaceStore.refreshAgentsFromDaemon()
                }
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])

            Button("Refresh Workspace Agents") {
                Task {
                    await workspaceStore.refreshAgentsFromDaemon()
                }
            }

            Button("Refresh Selected Workspace Thread") {
                guard let threadID = workspaceStore.selectedThreadID else { return }
                Task {
                    await workspaceStore.refreshThreadFromDaemon(threadID: threadID)
                }
            }
            .disabled(workspaceStore.selectedThreadID == nil)

            Button("Disconnect") {
                store.disconnect()
            }
            .disabled(!store.hasConfiguredDaemonURL)

            Divider()

            Button("Toggle Sidebar") {
                actions?.toggleSidebar()
            }

            Button("Focus Composer") {
                actions?.focusComposer()
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])
            .disabled(store.activeThreadID == nil)
        }
    }
}
