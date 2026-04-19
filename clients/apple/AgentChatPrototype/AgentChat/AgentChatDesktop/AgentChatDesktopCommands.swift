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

    let env: DesktopEnvironment

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
            .disabled(env.workspace.selectedThreadID == nil)

            Divider()

            Button("Reconnect") {
                env.reconnectNow()
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])

            Button("Refresh Workspace Agents") {
                Task {
                    await env.workspace.refreshAgentsFromDaemon()
                }
            }

            Button("Refresh Selected Workspace Thread") {
                guard let threadID = env.workspace.selectedThreadID else { return }
                Task {
                    await env.workspace.refreshThreadFromDaemon(threadID: threadID)
                }
            }
            .disabled(env.workspace.selectedThreadID == nil)

            Button("Disconnect") {
                env.disconnect()
            }
            .disabled(!env.hasConfiguredDaemonURL)

            Divider()

            Button("Toggle Sidebar") {
                actions?.toggleSidebar()
            }

            Button("Focus Composer") {
                actions?.focusComposer()
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])
            .disabled(env.workspace.selectedThreadID == nil)
        }
    }
}