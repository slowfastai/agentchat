import SwiftUI

struct AgentChatDesktopActions {
    let showNewThreadSheet: () -> Void
    let showAddAgentsSheet: () -> Void
    let toggleInspector: () -> Void
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
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])

            Button("Disconnect") {
                store.disconnect()
            }
            .disabled(!store.hasConfiguredDaemonURL)

            Divider()

            Button("Toggle Inspector") {
                actions?.toggleInspector()
            }
            .keyboardShortcut("i", modifiers: [.command, .option])

            Button("Focus Composer") {
                actions?.focusComposer()
            }
            .keyboardShortcut("l", modifiers: [.command, .shift])
            .disabled(store.activeThreadID == nil)
        }
    }
}
