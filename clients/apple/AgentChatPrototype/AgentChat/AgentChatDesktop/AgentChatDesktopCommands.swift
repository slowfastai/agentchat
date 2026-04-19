import SwiftUI

struct AgentChatDesktopActions {
    let showNewProjectSheet: () -> Void
    let showNewIssueSheet: () -> Void
    let showNewWorkspaceThreadSheet: () -> Void
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
            Button("New Project") {
                actions?.showNewProjectSheet()
            }

            Button("New Issue") {
                actions?.showNewIssueSheet()
            }
            .disabled(env.workspace.selectedProjectID == nil)

            Button("New Workspace Thread") {
                actions?.showNewWorkspaceThreadSheet()
            }
            .keyboardShortcut("n")
            .disabled(env.workspace.selectedIssueID == nil)

            Divider()

            Button("Distill Current Thread") {
                guard let issueID = env.workspace.selectedIssueID,
                      let thread = env.workspace.activeThread(for: issueID) else { return }
                env.workspace.distillThreadIntoIssueSummary(issueID: issueID, threadID: thread.id)
            }
            .disabled(env.workspace.selectedThreadID == nil)

            Divider()

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