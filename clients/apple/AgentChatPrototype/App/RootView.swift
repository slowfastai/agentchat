import SwiftUI

#if os(iOS)
private enum CompactTab: Hashable {
    case chats
    case agents
    case settings
}
#endif

private enum DesktopSection: String, CaseIterable, Hashable, Identifiable {
    case chat
    case projects
    case tasks
    case agents
    case settings

    var id: Self { self }

    var title: String {
        switch self {
        case .chat:
            return "Chat"
        case .projects:
            return "Projects"
        case .tasks:
            return "Tasks"
        case .agents:
            return "Agents"
        case .settings:
            return "Settings"
        }
    }

    var subtitle: String {
        switch self {
        case .chat:
            return "Recent conversations and active work."
        case .projects:
            return "Repositories and grouped initiatives."
        case .tasks:
            return "Tracked work items across the workspace."
        case .agents:
            return "Available agents and their capabilities."
        case .settings:
            return "Daemon status and local prototype controls."
        }
    }

    var systemImage: String {
        switch self {
        case .chat:
            return "bubble.left.and.bubble.right"
        case .projects:
            return "folder"
        case .tasks:
            return "checklist"
        case .agents:
            return "person.2"
        case .settings:
            return "gearshape"
        }
    }

    var accent: ColorToken {
        switch self {
        case .chat:
            return .orange
        case .projects:
            return .blue
        case .tasks:
            return .purple
        case .agents:
            return .green
        case .settings:
            return .gray
        }
    }
}

struct RootView: View {
    @EnvironmentObject private var store: DemoStore
    @State private var selectedSection: DesktopSection = .chat
    @State private var selectedAgentID: UUID?
    @State private var showCreateProject = false
    @State private var showCreateIssue = false
    @State private var showCreateThread = false
    @State private var columnVisibility: NavigationSplitViewVisibility = .all

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var selectedAgent: AgentProfile? {
        guard let selectedAgentID else { return nil }
        return store.agents.first(where: { $0.id == selectedAgentID })
    }

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
            desktopSidebar
                .navigationSplitViewColumnWidth(min: 300, ideal: 340, max: 380)
                .background(Color.appSidebarBackground)
        } detail: {
            desktopDetail
                .background(Color.appWindowBackground)
        }
        #if os(macOS)
        .focusedSceneValue(\.agentChatDesktopActions, AgentChatDesktopActions(
            showNewProjectSheet: { [self] in showCreateProject = true },
            showNewIssueSheet: { [self] in if store.selectedProjectID != nil { showCreateIssue = true } },
            showNewWorkspaceThreadSheet: { [self] in if store.selectedIssueID != nil { showCreateThread = true } },
            showAddAgentsSheet: { [self] in
                selectedSection = .agents
            },
            toggleSidebar: { [self] in
                columnVisibility = columnVisibility == .detailOnly ? .all : .detailOnly
            },
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
            ensureSelection(for: selectedSection)
        }
        .onChange(of: selectedSection) { _, newValue in
            ensureSelection(for: newValue)
        }
        .onChange(of: store.agents.map(\.id)) { _, _ in
            if selectedSection == .agents {
                ensureSelection(for: .agents)
            }
        }
    }

    private var desktopSidebar: some View {
        VStack(spacing: 0) {
            DesktopPrimaryNavigation(selectedSection: $selectedSection)
                .padding(.horizontal, AppSpacing.md)
                .padding(.top, AppSpacing.md)
                .padding(.bottom, AppSpacing.sm)

            Divider()

            sectionSidebarContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)

            Divider()

            Button {
                selectedSection = .settings
            } label: {
                HStack(spacing: 10) {
                    Image(systemName: DesktopSection.settings.systemImage)
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(selectedSection == .settings ? .primary : .secondary)
                    Text("Settings")
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(selectedSection == .settings ? .primary : .secondary)
                    Spacer()
                }
                .padding(.horizontal, AppSpacing.md)
                .padding(.vertical, 10)
                .background(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .fill(selectedSection == .settings ? Color.appSelectionFill : Color.clear)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .stroke(selectedSection == .settings ? Color.appSelectionStroke : Color.clear, lineWidth: 1)
                )
            }
            .buttonStyle(.plain)
            .padding(AppSpacing.md)
        }
        .background(Color.appSidebarBackground)
    }

    @ViewBuilder
    private var sectionSidebarContent: some View {
        switch selectedSection {
        case .chat:
            DesktopChatSidebar(selectedIssueID: $store.selectedIssueID)
                .environmentObject(store)
        case .projects:
            DesktopProjectsSidebar(selectedProjectID: $store.selectedProjectID)
                .environmentObject(store)
        case .tasks:
            DesktopTasksSidebar(selectedIssueID: $store.selectedIssueID)
                .environmentObject(store)
        case .agents:
            DesktopAgentsSidebar(selectedAgentID: $selectedAgentID)
                .environmentObject(store)
        case .settings:
            DesktopSettingsSidebar()
                .environmentObject(store)
        }
    }

    @ViewBuilder
    private var desktopDetail: some View {
        switch selectedSection {
        case .chat:
            if let selectedIssueID = store.selectedIssueID {
                IssueWorkspaceView(issueID: selectedIssueID)
            } else {
                EmptyStateView(
                    title: "No chat selected",
                    message: "Choose a conversation from the left sidebar to open its workspace.",
                    systemImage: "bubble.left.and.bubble.right"
                )
            }
        case .projects:
            if let selectedProjectID = store.selectedProjectID {
                ProjectDashboardView(
                    projectID: selectedProjectID,
                    selectedIssueID: $store.selectedIssueID
                )
            } else {
                EmptyStateView(
                    title: "No project selected",
                    message: "Choose a project from the left sidebar to inspect tasks, threads, and outputs.",
                    systemImage: "folder"
                )
            }
        case .tasks:
            if let selectedIssueID = store.selectedIssueID {
                IssueWorkspaceView(issueID: selectedIssueID)
            } else {
                EmptyStateView(
                    title: "No task selected",
                    message: "Choose a task from the left sidebar to open the full workspace.",
                    systemImage: "checklist"
                )
            }
        case .agents:
            if let agent = selectedAgent {
                DesktopAgentDetailView(agent: agent)
                    .environmentObject(store)
            } else {
                Color.appCanvasBackground
                    .ignoresSafeArea()
            }
        case .settings:
            DesktopSettingsDetailView()
                .environmentObject(store)
        }
    }

    private func ensureSelection(for section: DesktopSection) {
        switch section {
        case .chat, .tasks:
            if let selectedIssueID = store.selectedIssueID,
               store.issue(for: selectedIssueID) != nil {
                return
            }

            if let firstIssueID = store.chatThreads.first?.issueID ?? store.allIssues.first?.id {
                _ = store.selectIssue(firstIssueID)
            }
        case .projects:
            if let selectedProjectID = store.selectedProjectID,
               store.project(for: selectedProjectID) != nil {
                return
            }

            if let firstProjectID = store.projects.first?.id {
                _ = store.selectProject(firstProjectID)
            }
        case .agents:
            if let selectedAgentID,
               store.agents.contains(where: { $0.id == selectedAgentID }) {
                return
            }

            selectedAgentID = nil
        case .settings:
            return
        }
    }
}

private struct DesktopPrimaryNavigation: View {
    @Binding var selectedSection: DesktopSection

    private let primarySections: [DesktopSection] = [.chat, .projects, .tasks, .agents]

    var body: some View {
        HStack(spacing: AppSpacing.sm) {
            ForEach(primarySections) { section in
                Button {
                    selectedSection = section
                } label: {
                    VStack(spacing: 4) {
                        Image(systemName: section.systemImage)
                            .font(.system(size: 15, weight: .semibold))
                            .foregroundStyle(selectedSection == section ? .primary : .secondary)
                        Text(section.title)
                            .font(.caption2.weight(.semibold))
                            .lineLimit(1)
                            .foregroundStyle(selectedSection == section ? .primary : .secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 9)
                    .background(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .fill(selectedSection == section ? Color.appSelectionFill : Color.clear)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 14, style: .continuous)
                            .stroke(selectedSection == section ? Color.appSelectionStroke : Color.clear, lineWidth: 1)
                    )
                }
                .buttonStyle(.plain)
            }
        }
    }
}

private struct DesktopSectionIntro: View {
    let section: DesktopSection
    let metricLabel: String
    let metricValue: String

    var body: some View {
        CardSurface(accent: section.accent, padding: AppSpacing.md) {
            HStack(alignment: .firstTextBaseline, spacing: AppSpacing.md) {
                VStack(alignment: .leading, spacing: AppSpacing.xs) {
                    Text(section.title)
                        .font(.headline)
                    Text(section.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                VStack(alignment: .trailing, spacing: 2) {
                    Text(metricValue)
                        .font(.title3.monospacedDigit().weight(.semibold))
                    Text(metricLabel)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.horizontal, AppSpacing.md)
        .padding(.top, AppSpacing.md)
    }
}

private struct DesktopChatSidebar: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                DesktopSectionIntro(
                    section: .chat,
                    metricLabel: "threads",
                    metricValue: "\(store.chatThreads.count)"
                )

                if store.chatThreads.isEmpty {
                    EmptyStateView(
                        title: "No chats yet",
                        message: "Create a task or thread to start a new conversation flow.",
                        systemImage: "bubble.left.and.bubble.right"
                    )
                    .frame(minHeight: 240)
                } else {
                    ForEach(store.chatThreads) { thread in
                        Button {
                            selectedIssueID = thread.issueID
                            _ = store.selectIssue(thread.issueID)
                        } label: {
                            DesktopChatRow(
                                thread: thread,
                                isSelected: selectedIssueID == thread.issueID
                            )
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, AppSpacing.md)
                    }
                }
            }
            .padding(.bottom, AppSpacing.md)
        }
    }
}

private struct DesktopChatRow: View {
    let thread: ChatThreadSummary
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: thread.accent, isSelected: isSelected, padding: 12) {
            HStack(alignment: .top, spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Color.appSubtleFill)
                        .frame(width: 40, height: 40)

                    Image(systemName: thread.participants.count > 1 ? "person.2.fill" : "bubble.left.fill")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(isSelected ? .primary : .secondary)
                }

                VStack(alignment: .leading, spacing: 5) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("#\(thread.issueNumber) \(thread.title)")
                            .font(.callout.weight(.semibold))
                            .lineLimit(1)

                        Spacer(minLength: 8)

                        Text(thread.updatedAt, style: .relative)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    Text(thread.preview)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)

                    HStack(spacing: 8) {
                        if !thread.participants.isEmpty {
                            Text(thread.participants.joined(separator: " · "))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }

                        Spacer()

                        if thread.unreadCount > 0 {
                            Text(thread.unreadCount > 99 ? "99+" : "\(thread.unreadCount)")
                                .font(.caption2.weight(.bold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 8)
                                .padding(.vertical, 4)
                                .background(AppColors.unreadBadge, in: Capsule())
                        } else {
                            StatusBadge(text: thread.state.title, color: thread.state.badgeColor)
                        }
                    }
                }
            }
        }
    }
}

private struct DesktopProjectsSidebar: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedProjectID: UUID?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                if store.projects.isEmpty {
                    EmptyStateView(
                        title: "No projects",
                        message: "Create a project to organize tasks, agent threads, and outputs.",
                        systemImage: "folder.badge.plus"
                    )
                    .frame(minHeight: 240)
                } else {
                    ForEach(store.projects) { project in
                        Button {
                            selectedProjectID = project.id
                            _ = store.selectProject(project.id)
                        } label: {
                            DesktopProjectRow(
                                project: project,
                                isSelected: selectedProjectID == project.id
                            )
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, AppSpacing.md)
                    }
                }
            }
            .padding(.top, AppSpacing.md)
            .padding(.bottom, AppSpacing.md)
        }
    }
}

private struct DesktopProjectRow: View {
    let project: Project
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: project.color, isSelected: isSelected, padding: 12) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: "folder.fill")
                        .foregroundStyle(isSelected ? .primary : .secondary)

                    VStack(alignment: .leading, spacing: 8) {
                        Text(project.name)
                            .font(.callout.weight(.semibold))
                            .lineLimit(1)
                        MetricLabel(title: "Tasks", value: "\(project.issues.count)")
                    }

                    Spacer()
                }
            }
        }
    }
}

private struct DesktopTasksSidebar: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?

    private var issues: [Issue] {
        store.allIssues.sorted { lhs, rhs in
            if lhs.updatedAt != rhs.updatedAt {
                return lhs.updatedAt > rhs.updatedAt
            }
            return lhs.number > rhs.number
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                DesktopSectionIntro(
                    section: .tasks,
                    metricLabel: "tasks",
                    metricValue: "\(issues.count)"
                )

                if issues.isEmpty {
                    EmptyStateView(
                        title: "No tasks",
                        message: "Tasks will appear here once the workspace has active work items.",
                        systemImage: "checklist"
                    )
                    .frame(minHeight: 240)
                } else {
                    ForEach(issues) { issue in
                        Button {
                            selectedIssueID = issue.id
                            _ = store.selectIssue(issue.id)
                        } label: {
                            DesktopTaskRow(
                                issue: issue,
                                project: store.project(for: store.projectID(forIssueID: issue.id) ?? UUID()),
                                isSelected: selectedIssueID == issue.id
                            )
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, AppSpacing.md)
                    }
                }
            }
            .padding(.bottom, AppSpacing.md)
        }
    }
}

private struct DesktopTaskRow: View {
    let issue: Issue
    let project: Project?
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: issue.status.badgeColor, isSelected: isSelected, padding: 12) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text("#\(issue.number)")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.secondary)

                    Text(issue.title)
                        .font(.callout.weight(.semibold))
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    StatusBadge(text: issue.status.title, color: issue.status.badgeColor)
                }

                Text(issue.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 8) {
                    if let project {
                        Text(project.name)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    Text(issue.updatedAt, style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

private struct DesktopAgentsSidebar: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedAgentID: UUID?

    private var agents: [AgentProfile] {
        store.agents.sorted { lhs, rhs in
            lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                DesktopSectionIntro(
                    section: .agents,
                    metricLabel: "online",
                    metricValue: "\(agents.filter(\.isOnline).count)"
                )

                if agents.isEmpty {
                    EmptyStateView(
                        title: "No agents available",
                        message: "Reconnect to the daemon or add seed data to populate the roster.",
                        systemImage: "person.badge.questionmark"
                    )
                    .frame(minHeight: 240)
                } else {
                    ForEach(agents) { agent in
                        Button {
                            selectedAgentID = agent.id
                        } label: {
                            DesktopAgentRow(
                                agent: agent,
                                customName: store.customName(for: agent.id.uuidString),
                                avatarData: store.avatarData(for: agent.id.uuidString),
                                isSelected: selectedAgentID == agent.id
                            )
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, AppSpacing.md)
                    }
                }
            }
            .padding(.bottom, AppSpacing.md)
        }
    }
}

private struct DesktopAgentRow: View {
    let agent: AgentProfile
    let customName: String?
    let avatarData: Data?
    let isSelected: Bool

    private var displayName: String {
        customName ?? agent.name
    }

    var body: some View {
        CardSurface(accent: agent.accent, isSelected: isSelected, padding: 12) {
            HStack(alignment: .center, spacing: 10) {
                Group {
                    if let assetName = agent.resolvedDefaultAvatarAssetName {
                        PrototypeDefaultAvatarArtwork(
                            assetName: assetName,
                            size: 38,
                            shape: .circle
                        )
                    } else {
                        AvatarView(title: displayName, accent: agent.accent, size: 38)
                    }
                }
                .overlay(alignment: .bottomTrailing) {
                    Circle()
                        .fill(agent.isOnline ? AppColors.onlineStatus : Color.secondary.opacity(0.35))
                        .frame(width: 10, height: 10)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text(displayName)
                        .font(.callout.weight(.semibold))
                        .lineLimit(1)

                    Text(agent.shortDescription)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Spacer()

                if avatarData != nil {
                    Image(systemName: "photo")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

private struct DesktopSettingsSidebar: View {
    @EnvironmentObject private var store: DemoStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                DesktopSectionIntro(
                    section: .settings,
                    metricLabel: "status",
                    metricValue: store.isRefreshingAgentsFromDaemon ? "Syncing" : "Ready"
                )

                CardSurface(accent: store.daemonStatusAccent) {
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Local daemon", systemImage: "bolt.horizontal.circle.fill")
                            .font(.callout.weight(.semibold))
                        Text(store.daemonStatusText)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(.horizontal, AppSpacing.md)

                CardSurface {
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Main window settings", systemImage: "slider.horizontal.3")
                            .font(.callout.weight(.semibold))
                        Text("Open the detail pane to refresh daemon state or reset prototype data.")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
                .padding(.horizontal, AppSpacing.md)
            }
            .padding(.bottom, AppSpacing.md)
        }
    }
}

private struct DesktopSettingsDetailView: View {
    @EnvironmentObject private var store: DemoStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.lg) {
                CardSurface(accent: store.daemonStatusAccent) {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack(spacing: 12) {
                            Circle()
                                .fill(store.daemonStatusAccent.color)
                                .frame(width: 10, height: 10)

                            VStack(alignment: .leading, spacing: 4) {
                                Text("Local Daemon")
                                    .font(.callout.weight(.semibold))
                                Text(store.daemonStatusText)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }

                            Spacer()

                            if store.isRefreshingAgentsFromDaemon {
                                ProgressView()
                                    .controlSize(.small)
                            }
                        }

                        HStack(spacing: 10) {
                            Button {
                                Task {
                                    await store.refreshAgentsFromDaemon()
                                }
                            } label: {
                                Label("Refresh Agents", systemImage: "arrow.clockwise")
                            }
                            .buttonStyle(.borderedProminent)
                            .controlSize(.small)

                            Button("Reset Prototype Data") {
                                store.resetPrototypeData()
                            }
                            .buttonStyle(.bordered)
                            .controlSize(.small)
                        }
                    }
                }

                CardSurface(accent: .gray) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Workspace")
                            .font(.callout.weight(.semibold))
                        Text("Use this area for daemon connectivity, prototype reset actions, and future desktop-level preferences.")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        HStack(spacing: AppSpacing.lg) {
                            MetricLabel(title: "Projects", value: "\(store.projects.count)")
                            MetricLabel(title: "Tasks", value: "\(store.allIssues.count)")
                            MetricLabel(title: "Agents", value: "\(store.agents.count)")
                        }
                    }
                }
            }
            .padding(AppSpacing.lg)
        }
        .background(Color.appCanvasBackground)
        .navigationTitle("Settings")
    }
}

private struct DesktopAgentDetailView: View {
    @EnvironmentObject private var store: DemoStore
    let agent: AgentProfile
    @State private var showEditSheet = false

    private var displayName: String {
        store.customName(for: agent.id.uuidString) ?? agent.name
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.lg) {
                Text("Profile")
                    .font(.title2.weight(.semibold))

                CardSurface(accent: agent.accent) {
                    HStack(alignment: .top, spacing: AppSpacing.md) {
                        Group {
                            if let assetName = agent.resolvedDefaultAvatarAssetName {
                                PrototypeDefaultAvatarArtwork(
                                    assetName: assetName,
                                    size: 72,
                                    shape: .circle
                                )
                            } else {
                                AvatarView(title: displayName, accent: agent.accent, size: 72)
                            }
                        }

                        VStack(alignment: .leading, spacing: 8) {
                            HStack(spacing: 8) {
                                Text(displayName)
                                    .font(.title3.weight(.semibold))
                                StatusBadge(
                                    text: agent.isOnline ? "Online" : "Offline",
                                    color: agent.isOnline ? .green : .gray
                                )
                            }

                            Text(agent.shortDescription)
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }

                        Spacer(minLength: 0)

                        Button {
                            showEditSheet = true
                        } label: {
                            Label("Edit Profile", systemImage: "pencil")
                        }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(AppSpacing.lg)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color.appCanvasBackground)
        .navigationTitle(displayName)
        .sheet(isPresented: $showEditSheet) {
            EditAgentSheet(agent: agent)
                .environmentObject(store)
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
