import SwiftUI

#if os(iOS)
private enum CompactTab: Hashable {
    case chats
    case agents
    case settings
}
#endif

struct RootView: View {
    @EnvironmentObject private var store: DemoStore
    @State private var destination: SidebarDestination? = .inbox

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    var body: some View {
        #if os(iOS)
        if horizontalSizeClass == .compact {
            CompactRootView(selectedIssueID: $store.selectedIssueID)
        } else {
            splitView
        }
        #else
        splitView
        #endif
    }

    private var splitView: some View {
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

#if os(iOS)
private struct CompactRootView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?
    @State private var tab: CompactTab = .chats

    var body: some View {
        TabView(selection: $tab) {
            NavigationStack {
                ChatListView(selectedIssueID: $selectedIssueID)
            }
            .tabItem {
                Label("Chats", systemImage: "message")
            }
            .tag(CompactTab.chats)

            NavigationStack {
                AgentListView()
            }
            .tabItem {
                Label("Agents", systemImage: "person.2.fill")
            }
            .tag(CompactTab.agents)

            NavigationStack {
                SettingsPlaceholderView()
            }
            .tabItem {
                Label("Settings", systemImage: "gearshape")
            }
            .tag(CompactTab.settings)
        }
        .onAppear {
            if store.selectedIssueID == nil {
                store.selectedIssueID = store.currentProject?.issues.first?.id
            }
        }
    }
}

private struct SettingsPlaceholderView: View {
    var body: some View {
        EmptyStateView(
            title: "Settings",
            message: "Reserve this tab for relay configuration, connected agents, appearance, and device preferences.",
            systemImage: "gearshape"
        )
        .navigationTitle("Settings")
    }
}
#endif
