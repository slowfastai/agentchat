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
