import SwiftUI

enum TaskScope: String, CaseIterable, Hashable, Identifiable {
    case currentProject
    case allProjects

    var id: Self { self }

    var title: String {
        switch self {
        case .currentProject: return "Current Project"
        case .allProjects: return "All Projects"
        }
    }
}

struct IssueInboxView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?

    @State private var searchText = ""
    @State private var filter: IssueFilter = .all
    @State private var viewMode: SwitcherMode = .list
    @State private var scope: TaskScope = .currentProject

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var baseIssues: [Issue] {
        switch scope {
        case .currentProject:
            return store.currentProject?.issues ?? []
        case .allProjects:
            return store.allIssues
        }
    }

    private var filteredIssues: [Issue] {
        baseIssues
            .filter { issue in
                if searchText.isEmpty { return true }
                let haystack = "\(issue.title) \(issue.summary) \(issue.latestActivityText)".lowercased()
                return haystack.contains(searchText.lowercased())
            }
            .filter { issue in
                switch filter {
                case .all:
                    return true
                case .assignedToAgent:
                    return !issue.agentNames.isEmpty
                case .running:
                    return store.hasRunningSessions(for: issue.id)
                case .needsReview:
                    return issue.status == .review
                }
            }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    private var workspaceCards: [WorkspaceCardModel] {
        store.workspaceCards
            .filter { card in
                switch scope {
                case .currentProject:
                    guard let projectID = store.selectedProjectID,
                          let project = store.project(for: projectID) else {
                        return false
                    }
                    return project.issues.contains { $0.id == card.issueID }
                case .allProjects:
                    return true
                }
            }
            .filter { card in
                if searchText.isEmpty { return true }
                let haystack = "\(card.title) \(card.latestPreview)".lowercased()
                return haystack.contains(searchText.lowercased())
            }
    }

    private var runningCount: Int {
        filteredIssues.filter { store.hasRunningSessions(for: $0.id) }.count
    }

    private var reviewCount: Int {
        filteredIssues.filter { $0.status == .review }.count
    }

    private var isCompactPhoneLayout: Bool {
        #if os(iOS)
        return horizontalSizeClass == .compact
        #else
        return false
        #endif
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                toolbar
                summaryHeader
                content
            }
            .padding(AppSpacing.lg)
        }
        .navigationTitle(store.currentProject?.name ?? "Tasks")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #endif
    }

    @ViewBuilder
    private var content: some View {
        switch viewMode {
        case .list:
            LazyVStack(spacing: AppSpacing.md) {
                if filteredIssues.isEmpty {
                    emptyState
                } else {
                    ForEach(filteredIssues) { issue in
                        issueRow(for: issue)
                    }
                }
            }
        case .grid:
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 280), spacing: AppSpacing.md)], spacing: AppSpacing.md) {
                if workspaceCards.isEmpty {
                    emptyState
                } else {
                    ForEach(workspaceCards) { card in
                        gridCard(for: card)
                    }
                }
            }
        case .focus:
            HStack(alignment: .top, spacing: AppSpacing.md) {
                if let selected = workspaceCards.first(where: { $0.issueID == selectedIssueID }) ?? workspaceCards.first {
                    focusCard(for: selected)
                        .frame(maxWidth: .infinity, alignment: .topLeading)

                    VStack(spacing: AppSpacing.md) {
                        ForEach(workspaceCards.filter { $0.issueID != selectedIssueID }) { card in
                            Button {
                                selectedIssueID = card.issueID
                            } label: {
                                WorkspaceCard(card: card, isSelected: false)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .frame(width: 320)
                } else {
                    emptyState
                }
            }
        }
    }

    private var emptyState: some View {
        CardSurface {
            EmptyStateView(
                title: "No matching tasks",
                message: "Try another search term or switch the active filter.",
                systemImage: "line.3.horizontal.decrease.circle"
            )
            .frame(minHeight: 260)
        }
    }

    @ViewBuilder
    private func issueRow(for issue: Issue) -> some View {
        if isCompactPhoneLayout {
            NavigationLink {
                IssueWorkspaceView(issueID: issue.id)
            } label: {
                IssueRowCard(issue: issue, isSelected: false)
            }
            .buttonStyle(.plain)
            .simultaneousGesture(TapGesture().onEnded {
                selectedIssueID = issue.id
            })
        } else {
            Button {
                selectedIssueID = issue.id
            } label: {
                IssueRowCard(issue: issue, isSelected: selectedIssueID == issue.id)
            }
            .buttonStyle(.plain)
        }
    }

    @ViewBuilder
    private func gridCard(for card: WorkspaceCardModel) -> some View {
        Button {
            selectedIssueID = card.issueID
        } label: {
            WorkspaceCard(card: card, isSelected: selectedIssueID == card.issueID)
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private func focusCard(for card: WorkspaceCardModel) -> some View {
        WorkspaceFocusCard(card: card)
    }

    private var toolbar: some View {
        CardSurface(accent: .blue) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(spacing: AppSpacing.md) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Task inbox")
                            .font(.title2.weight(.semibold))
                        Text("Scan active work, agent ownership, and the latest project activity.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    Picker("Scope", selection: $scope) {
                        ForEach(TaskScope.allCases) { item in
                            Text(item.title).tag(item)
                        }
                    }
                    .pickerStyle(.menu)
                    .frame(maxWidth: 160)
                }

                HStack(spacing: AppSpacing.md) {
                    TextField("Search tasks", text: $searchText)
                        .textFieldStyle(.roundedBorder)

                    Picker("Filter", selection: $filter) {
                        ForEach(IssueFilter.allCases) { item in
                            Text(item.title).tag(item)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 280)

                    Spacer()

                    Picker("View", selection: $viewMode) {
                        ForEach(SwitcherMode.allCases) { mode in
                            Text(mode.title).tag(mode)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 200)
                }
            }
        }
    }

    private var summaryHeader: some View {
        CardSurface(accent: .purple) {
            HStack(spacing: AppSpacing.xl) {
                MetricLabel(title: "Visible Tasks", value: "\(filteredIssues.count)")
                MetricLabel(title: "Running", value: "\(runningCount)")
                MetricLabel(title: "Needs Review", value: "\(reviewCount)")
                MetricLabel(title: "Scope", value: scope.title)
                Spacer()
            }
        }
    }
}

private struct IssueRowCard: View {
    let issue: Issue
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: issue.status.badgeColor, isSelected: isSelected) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(alignment: .top, spacing: AppSpacing.md) {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            Text("#\(issue.number)")
                                .font(.headline.monospacedDigit())
                                .foregroundStyle(.secondary)
                            StatusBadge(text: issue.status.title, color: issue.status.badgeColor)
                            PillView(text: issue.priority.title, color: .gray)
                        }

                        Text(issue.title)
                            .font(.title3.weight(.semibold))
                            .multilineTextAlignment(.leading)

                        Text(issue.summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.leading)
                    }

                    Spacer()
                }

                HStack(alignment: .center, spacing: AppSpacing.md) {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(issue.assignees) { participant in
                                PillView(
                                    text: participant.displayName,
                                    color: participant.accent
                                )
                            }
                        }
                    }

                    Spacer(minLength: 0)
                }

                HStack(alignment: .bottom, spacing: AppSpacing.xl) {
                    MetricLabel(title: "Sessions", value: "\(issue.sessionCount)")
                    MetricLabel(title: "Active", value: AppFormatters.durationString(seconds: issue.totalActiveSeconds))
                    MetricLabel(title: "Updated", value: AppFormatters.relativeString(from: issue.updatedAt))
                    Spacer()
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("Latest Activity")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(issue.latestActivityText)
                        .font(.subheadline)
                        .multilineTextAlignment(.leading)
                }
            }
        }
    }
}

private struct WorkspaceCard: View {
    let card: WorkspaceCardModel
    let isSelected: Bool

    var body: some View {
        CardSurface(accent: card.state.badgeColor, isSelected: isSelected) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(alignment: .top) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("#\(card.issueNumber)")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                        Text(card.title)
                            .font(.headline)
                            .multilineTextAlignment(.leading)
                    }
                    Spacer()
                    StatusBadge(text: card.state.title, color: card.state.badgeColor)
                }

                HStack(spacing: 8) {
                    ForEach(card.participants, id: \.self) { participant in
                        PillView(text: participant, color: accent(for: participant))
                    }
                }

                Text(card.latestPreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
                    .lineLimit(3)

                HStack {
                    MetricLabel(title: "Elapsed", value: AppFormatters.durationString(seconds: card.elapsedSeconds))
                    Spacer()
                    if let activeTool = card.activeTool {
                        PillView(text: activeTool, color: .orange)
                    }
                }
            }
        }
    }

    private func accent(for participant: String) -> ColorToken {
        switch participant {
        case "Claude": return .blue
        case "Codex": return .green
        case "Pi": return .purple
        default: return .gray
        }
    }
}

private struct WorkspaceFocusCard: View {
    let card: WorkspaceCardModel

    var body: some View {
        CardSurface(accent: card.state.badgeColor, isSelected: true) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Focused workspace")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text("#\(card.issueNumber) \(card.title)")
                            .font(.title2.weight(.bold))
                    }
                    Spacer()
                    StatusBadge(text: card.state.title, color: card.state.badgeColor)
                }

                HStack(spacing: 8) {
                    ForEach(card.participants, id: \.self) { participant in
                        PillView(text: participant, color: .gray)
                    }
                }

                CardSurface(accent: .gray) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Latest output")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                        Text(card.latestPreview)
                            .font(.body)
                    }
                }

                HStack(spacing: AppSpacing.xl) {
                    MetricLabel(title: "Elapsed", value: AppFormatters.durationString(seconds: card.elapsedSeconds))
                    MetricLabel(title: "Tool", value: card.activeTool ?? "None")
                    Spacer()
                }
            }
        }
    }
}