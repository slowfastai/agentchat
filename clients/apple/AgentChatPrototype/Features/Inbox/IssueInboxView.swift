import SwiftUI

struct IssueInboxView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedIssueID: UUID?

    @State private var searchText = ""
    @State private var filter: IssueFilter = .all

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var filteredIssues: [Issue] {
        let baseIssues = store.currentProject?.issues ?? store.allIssues

        return baseIssues
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

                LazyVStack(spacing: AppSpacing.md) {
                    if filteredIssues.isEmpty {
                        CardSurface {
                            EmptyStateView(
                                title: "No matching issues",
                                message: "Try another search term or switch the active filter.",
                                systemImage: "line.3.horizontal.decrease.circle"
                            )
                            .frame(minHeight: 260)
                        }
                    } else {
                        ForEach(filteredIssues) { issue in
                            issueRow(for: issue)
                        }
                    }
                }
            }
            .padding(AppSpacing.lg)
        }
        .navigationTitle(store.currentProject?.name ?? "Issues")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #endif
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

    private var toolbar: some View {
        CardSurface(accent: .blue) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(spacing: AppSpacing.md) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Issue inbox")
                            .font(.title2.weight(.semibold))
                        Text("Scan active work, agent ownership, and the latest project activity.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    Picker("Project", selection: $store.selectedProjectID) {
                        ForEach(store.projects) { project in
                            Text(project.name).tag(Optional(project.id))
                        }
                    }
                    .pickerStyle(.menu)
                    .frame(maxWidth: 180)
                }

                HStack(spacing: AppSpacing.md) {
                    TextField("Search issues", text: $searchText)
                        .textFieldStyle(.roundedBorder)

                    Picker("Filter", selection: $filter) {
                        ForEach(IssueFilter.allCases) { item in
                            Text(item.title).tag(item)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 380)
                }
            }
        }
    }

    private var summaryHeader: some View {
        CardSurface(accent: .purple) {
            HStack(spacing: AppSpacing.xl) {
                MetricLabel(title: "Visible Issues", value: "\(filteredIssues.count)")
                MetricLabel(title: "Running", value: "\(runningCount)")
                MetricLabel(title: "Needs Review", value: "\(reviewCount)")
                MetricLabel(title: "Today Focus", value: store.currentProject?.name ?? "AgentChat")
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
