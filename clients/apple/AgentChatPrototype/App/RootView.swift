import SwiftUI

struct RootView: View {
    @EnvironmentObject private var store: DemoStore
    @State private var destination: SidebarDestination? = .inbox

    var body: some View {
        NavigationSplitView {
            List(SidebarDestination.allCases, selection: $destination) { item in
                Label(item.title, systemImage: item.systemImage)
                    .tag(item)
            }
            .navigationTitle("AgentChat")
        } content: {
            switch destination ?? .inbox {
            case .inbox:
                IssueInboxView(selectedIssueID: $store.selectedIssueID)
            case .switcher:
                SwitcherView(selectedIssueID: $store.selectedIssueID)
            case .agents:
                AgentListView()
            }
        } detail: {
            switch destination ?? .inbox {
            case .inbox, .switcher:
                if let selectedIssueID = store.selectedIssueID {
                    IssueWorkspaceView(issueID: selectedIssueID)
                } else {
                    EmptyStateView(
                        title: "Select an issue",
                        message: "Pick an issue from the inbox or switcher to open the workspace.",
                        systemImage: "rectangle.and.text.magnifyingglass"
                    )
                }
            case .agents:
                EmptyStateView(
                    title: "Agent roster",
                    message: "Use the list to define personas, capabilities, and future assignment rules.",
                    systemImage: "person.2"
                )
            }
        }
        .onAppear {
            if store.selectedIssueID == nil {
                store.selectedIssueID = store.currentProject?.issues.first?.id
            }
        }
    }
}
