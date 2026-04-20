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
    @State private var sidebarItem: SidebarItem? = .destination(.projects)
    @State private var showCreateProject = false
    @State private var showCreateIssue = false
    @State private var showCreateThread = false
    @State private var columnVisibility: NavigationSplitViewVisibility = .all

    private var destination: SidebarDestination? {
        if case .destination(let dest) = sidebarItem {
            return dest
        }
        return nil
    }

    #if os(macOS)
    @FocusedValue(\.agentChatDesktopActions) private var actions
    #endif

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
        NavigationSplitView(columnVisibility: $columnVisibility) {
            List(sidebarItems, id: \.self, selection: $sidebarItem) { item in
                Label(item.title, systemImage: item.systemImage)
                    .tag(item)
            }
            .navigationTitle("AgentChat")
            .toolbar {
                #if os(macOS)
                ToolbarItem(placement: .primaryAction) {
                    if destination == .projects {
                        Button {
                            showCreateProject = true
                        } label: {
                            Label("Add Project", systemImage: "plus")
                        }
                    }
                }
                #else
                ToolbarItem(placement: .bottomBar) {
                    if destination == .projects {
                        Button {
                            showCreateProject = true
                        } label: {
                            Label("Add Project", systemImage: "plus")
                        }
                    }
                }
                #endif
            }
        } content: {
            switch sidebarItem {
            case .destination(.projects):
                ProjectListView(
                    selectedProjectID: $store.selectedProjectID,
                    selectedIssueID: $store.selectedIssueID,
                    showCreateProject: $showCreateProject
                )
            case .destination(.inbox):
                IssueInboxView(selectedIssueID: $store.selectedIssueID)
            case .destination(.agents):
                AgentListView()
            case .settings:
                SettingsView()
            case .destination(.projects), .destination(.inbox), .destination(.agents), .none:
                EmptyStateView(
                    title: "Select an item",
                    message: "Choose an item from the sidebar.",
                    systemImage: "sidebar.left"
                )
            }
        } detail: {
            switch sidebarItem {
            case .destination(.projects):
                if let selectedProjectID = store.selectedProjectID {
                    ProjectDashboardView(
                        projectID: selectedProjectID,
                        selectedIssueID: $store.selectedIssueID
                    )
                } else {
                    EmptyStateView(
                        title: "Select a project",
                        message: "Pick a project to review open tasks, active threads, and recent outputs.",
                        systemImage: "folder"
                    )
                }
            case .destination(.inbox):
                if let selectedIssueID = store.selectedIssueID {
                    IssueWorkspaceView(issueID: selectedIssueID)
                } else {
                    EmptyStateView(
                        title: "Select a task",
                        message: "Pick a task from the inbox to open the workspace.",
                        systemImage: "rectangle.and.text.magnifyingglass"
                    )
                }
            case .destination(.agents):
                EmptyStateView(
                    title: "Agent roster",
                    message: "Use the list to define personas, capabilities, and future assignment rules.",
                    systemImage: "person.2"
                )
            case .settings:
                SettingsView()
            case .destination(.projects), .destination(.inbox), .destination(.agents), .none:
                EmptyStateView(
                    title: "Select an item",
                    message: "Choose an item from the sidebar.",
                    systemImage: "sidebar.left"
                )
            }
        }
        #if os(macOS)
        .focusedSceneValue(\.agentChatDesktopActions, AgentChatDesktopActions(
            showNewProjectSheet: { [self] in showCreateProject = true },
            showNewIssueSheet: { [self] in if store.selectedProjectID != nil { showCreateIssue = true } },
            showNewWorkspaceThreadSheet: { [self] in if store.selectedIssueID != nil { showCreateThread = true } },
            showAddAgentsSheet: { },
            toggleSidebar: { [self] in columnVisibility = columnVisibility == .all ? .detailOnly : .all },
            focusComposer: { }
        ))
        .sheet(isPresented: $showCreateProject) {
            CreateProjectSheet()
        }
        .sheet(isPresented: $showCreateIssue) {
            if let pid = store.selectedProjectID {
                CreateIssueSheet(projectID: pid)
            }
        }
        .sheet(isPresented: $showCreateThread) {
            if let iid = store.selectedIssueID {
                CreateThreadSheet(issueID: iid)
            }
        }
        #endif
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
