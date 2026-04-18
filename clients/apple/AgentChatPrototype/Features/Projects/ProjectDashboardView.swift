import SwiftUI

struct ProjectDashboardView: View {
    @EnvironmentObject private var store: DemoStore
    let projectID: UUID
    @Binding var selectedIssueID: UUID?

    @State private var focus: DashboardFocus = .all

    private enum DashboardFocus: String, CaseIterable, Identifiable {
        case all
        case active
        case review
        case outputs

        var id: Self { self }

        var title: String {
            switch self {
            case .all: return "All"
            case .active: return "Active"
            case .review: return "Review"
            case .outputs: return "Outputs"
            }
        }
    }

    private var project: Project? {
        store.project(for: projectID)
    }

    private var issues: [Issue] {
        project?.issues.sorted { $0.updatedAt > $1.updatedAt } ?? []
    }

    private var openIssues: [Issue] {
        issues.filter { $0.status != .done }
    }

    private var reviewIssues: [Issue] {
        issues.filter { $0.status == .review }
    }

    private var runningThreads: [Thread] {
        issues
            .flatMap { store.threads(for: $0.id) }
            .filter { $0.state == .active }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    private var recentArtifacts: [IssueArtifact] {
        issues
            .flatMap { store.artifacts(for: $0.id) }
            .sorted { $0.createdAt > $1.createdAt }
    }

    private var recentDecisions: [IssueDecision] {
        issues
            .flatMap { store.decisions(for: $0.id) }
            .sorted { $0.createdAt > $1.createdAt }
    }

    private var recentDistilledSummaryIssues: [Issue] {
        issues
            .filter { $0.latestActivityText == "Issue summary distilled from thread" }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    private var recentFollowUpIssues: [Issue] {
        issues
            .filter(\.isFollowUpIssue)
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    var body: some View {
        if let project {
            NavigationStack {
                ScrollView {
                    VStack(alignment: .leading, spacing: AppSpacing.lg) {
                        ProjectDashboardHeader(
                            project: project,
                            openIssueCount: openIssues.count,
                            reviewIssueCount: reviewIssues.count,
                            runningThreadCount: runningThreads.count
                        )

                        ProjectMetricGrid(
                            totalIssues: issues.count,
                            openIssueCount: openIssues.count,
                            reviewIssueCount: reviewIssues.count,
                            runningThreadCount: runningThreads.count
                        )

                        Picker("Focus", selection: $focus) {
                            ForEach(DashboardFocus.allCases) { focus in
                                Text(focus.title).tag(focus)
                            }
                        }
                        .pickerStyle(.segmented)

                        if focus != .outputs {
                            ProjectDashboardSection(title: "Open Issues", systemImage: "list.bullet.rectangle") {
                                if openIssues.isEmpty {
                                    ProjectDashboardEmptyState(text: "No open issues")
                                } else {
                                    VStack(spacing: AppSpacing.sm) {
                                        ForEach(openIssues.prefix(6)) { issue in
                                            NavigationLink {
                                                IssueWorkspaceView(issueID: issue.id)
                                            } label: {
                                                ProjectIssueRow(issue: issue)
                                            }
                                            .buttonStyle(.plain)
                                            .simultaneousGesture(TapGesture().onEnded {
                                                openIssue(issue.id)
                                            })
                                        }
                                    }
                                }
                            }
                        }

                        if focus == .all || focus == .active {
                            ProjectDashboardSection(title: "Running Threads", systemImage: "bolt.horizontal.circle") {
                                if runningThreads.isEmpty {
                                    ProjectDashboardEmptyState(text: "No active threads")
                                } else {
                                    VStack(spacing: AppSpacing.sm) {
                                        ForEach(runningThreads.prefix(5)) { thread in
                                            if let issue = store.issue(for: thread.issueID) {
                                                NavigationLink {
                                                    IssueWorkspaceView(issueID: issue.id)
                                                } label: {
                                                    RunningThreadRow(thread: thread, issue: issue)
                                                }
                                                .buttonStyle(.plain)
                                                .simultaneousGesture(TapGesture().onEnded {
                                                    openIssue(issue.id, selectingThreadID: thread.id)
                                                })
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        HStack(alignment: .top, spacing: AppSpacing.md) {
                            if focus != .outputs {
                                ProjectDashboardSection(title: "Needs Review", systemImage: "checkmark.circle") {
                                    if reviewIssues.isEmpty {
                                        ProjectDashboardEmptyState(text: "Nothing waiting for review")
                                    } else {
                                        VStack(spacing: AppSpacing.sm) {
                                            ForEach(reviewIssues.prefix(4)) { issue in
                                                NavigationLink {
                                                    IssueWorkspaceView(issueID: issue.id)
                                                } label: {
                                                    ProjectIssueRow(issue: issue)
                                                }
                                                .buttonStyle(.plain)
                                                .simultaneousGesture(TapGesture().onEnded {
                                                    openIssue(issue.id)
                                                })
                                            }
                                        }
                                    }
                                }
                                .frame(maxWidth: .infinity)
                            }

                            ProjectDashboardSection(title: "Recent Distillations & Outputs", systemImage: "shippingbox") {
                                if recentDistilledSummaryIssues.isEmpty &&
                                    recentFollowUpIssues.isEmpty &&
                                    recentArtifacts.isEmpty &&
                                    recentDecisions.isEmpty {
                                    ProjectDashboardEmptyState(text: "No distilled outputs yet")
                                } else {
                                    VStack(spacing: AppSpacing.sm) {
                                        ForEach(recentDistilledSummaryIssues.prefix(2)) { issue in
                                            Button {
                                                openIssue(issue.id)
                                            } label: {
                                                ProjectDistilledSummaryRow(issue: issue)
                                            }
                                            .buttonStyle(.plain)
                                        }
                                        ForEach(recentFollowUpIssues.prefix(2)) { issue in
                                            Button {
                                                openIssue(issue.id)
                                            } label: {
                                                ProjectFollowUpIssueRow(issue: issue)
                                            }
                                            .buttonStyle(.plain)
                                        }
                                        ForEach(recentArtifacts.prefix(3)) { artifact in
                                            Button {
                                                openIssue(artifact.issueID, selectingThreadID: artifact.threadID)
                                            } label: {
                                                ProjectArtifactRow(artifact: artifact, issue: store.issue(for: artifact.issueID))
                                            }
                                            .buttonStyle(.plain)
                                        }
                                        ForEach(recentDecisions.prefix(3)) { decision in
                                            Button {
                                                openIssue(decision.issueID, selectingThreadID: decision.threadID)
                                            } label: {
                                                ProjectDecisionRow(decision: decision, issue: store.issue(for: decision.issueID))
                                            }
                                            .buttonStyle(.plain)
                                        }
                                    }
                                }
                            }
                            .frame(maxWidth: .infinity)
                        }
                    }
                    .padding(AppSpacing.lg)
                }
                .background(Color.appCanvasBackground)
                .navigationTitle(project.name)
            }
        } else {
            EmptyStateView(
                title: "Select a project",
                message: "Pick a project to review its issues, active threads, and recent outputs.",
                systemImage: "folder"
            )
        }
    }

    private func openIssue(_ issueID: UUID, selectingThreadID threadID: UUID? = nil) {
        selectedIssueID = issueID
        if let threadID {
            store.selectThread(threadID)
        }
    }
}

private struct ProjectDashboardHeader: View {
    let project: Project
    let openIssueCount: Int
    let reviewIssueCount: Int
    let runningThreadCount: Int

    var body: some View {
        CardSurface(accent: project.color) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(alignment: .top, spacing: AppSpacing.md) {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            Image(systemName: "folder.fill")
                                .foregroundStyle(Color(project.color))
                            Text(project.name)
                                .font(.title2.weight(.bold))
                        }
                        Text(project.repoPath)
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                    Spacer()
                    VStack(alignment: .trailing, spacing: 6) {
                        StatusBadge(text: "\(openIssueCount) Open", color: .blue)
                        if reviewIssueCount > 0 {
                            StatusBadge(text: "\(reviewIssueCount) Review", color: .orange)
                        }
                        if runningThreadCount > 0 {
                            StatusBadge(text: "\(runningThreadCount) Running", color: .green)
                        }
                    }
                }
            }
        }
    }
}

private struct ProjectMetricGrid: View {
    let totalIssues: Int
    let openIssueCount: Int
    let reviewIssueCount: Int
    let runningThreadCount: Int

    var body: some View {
        HStack(spacing: AppSpacing.md) {
            ProjectMetricCard(title: "Total Issues", value: "\(totalIssues)", accent: .gray)
            ProjectMetricCard(title: "Open", value: "\(openIssueCount)", accent: .blue)
            ProjectMetricCard(title: "Needs Review", value: "\(reviewIssueCount)", accent: .orange)
            ProjectMetricCard(title: "Running Threads", value: "\(runningThreadCount)", accent: .green)
        }
    }
}

private struct ProjectMetricCard: View {
    let title: String
    let value: String
    let accent: ColorToken

    var body: some View {
        CardSurface(accent: accent) {
            VStack(alignment: .leading, spacing: 6) {
                Text(value)
                    .font(.title2.monospacedDigit().weight(.bold))
                Text(title)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct ProjectDashboardSection<Content: View>: View {
    let title: String
    let systemImage: String
    @ViewBuilder let content: Content

    var body: some View {
        CardSurface(accent: .gray) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                HStack(spacing: 8) {
                    Image(systemName: systemImage)
                        .foregroundStyle(.secondary)
                    Text(title)
                        .font(.headline)
                }
                content
            }
        }
    }
}

private struct ProjectIssueRow: View {
    let issue: Issue

    var body: some View {
        HStack(alignment: .top, spacing: AppSpacing.md) {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text("#\(issue.number)")
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                    Text(issue.title)
                        .font(.subheadline.weight(.semibold))
                }
                Text(issue.latestActivityText.isEmpty ? issue.summary : issue.latestActivityText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
            StatusBadge(text: issue.status.title, color: issue.status.badgeColor)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(issue.status.badgeColor.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct RunningThreadRow: View {
    let thread: Thread
    let issue: Issue

    var body: some View {
        HStack(alignment: .top, spacing: AppSpacing.md) {
            VStack(alignment: .leading, spacing: 6) {
                Text(thread.title)
                    .font(.subheadline.weight(.semibold))
                Text("#\(issue.number) \(issue.title)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(thread.latestActivityText)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
            VStack(alignment: .trailing, spacing: 6) {
                StatusBadge(text: thread.purpose.title, color: thread.purpose.badgeColor)
                StatusBadge(text: thread.state.title, color: thread.state.badgeColor)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(thread.purpose.badgeColor.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct ProjectArtifactRow: View {
    let artifact: IssueArtifact
    let issue: Issue?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: artifact.kind.systemImage)
                .foregroundStyle(artifact.kind.accent.color)
            VStack(alignment: .leading, spacing: 4) {
                Text(artifact.title)
                    .font(.subheadline.weight(.semibold))
                if let issue {
                    Text("#\(issue.number) \(issue.title)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(artifact.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(artifact.kind.accent.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct ProjectDecisionRow: View {
    let decision: IssueDecision
    let issue: Issue?

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "checkmark.seal")
                .foregroundStyle(ColorToken.green.color)
            VStack(alignment: .leading, spacing: 4) {
                Text(decision.title)
                    .font(.subheadline.weight(.semibold))
                if let issue {
                    Text("#\(issue.number) \(issue.title)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(decision.rationale)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ColorToken.green.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct ProjectDistilledSummaryRow: View {
    let issue: Issue

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "doc.text.magnifyingglass")
                .foregroundStyle(ColorToken.purple.color)
            VStack(alignment: .leading, spacing: 4) {
                Text("Issue Summary Updated")
                    .font(.subheadline.weight(.semibold))
                Text("#\(issue.number) \(issue.title)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(issue.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ColorToken.purple.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct ProjectFollowUpIssueRow: View {
    @EnvironmentObject private var store: DemoStore
    let issue: Issue

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "arrowshape.turn.up.right")
                .foregroundStyle(ColorToken.orange.color)
            VStack(alignment: .leading, spacing: 4) {
                Text("Follow-up Issue")
                    .font(.subheadline.weight(.semibold))
                Text("#\(issue.number) \(issue.title)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if let sourceIssueID = issue.sourceIssueID,
                   let parentIssue = store.issue(for: sourceIssueID) {
                    Text("From #\(parentIssue.number) \(parentIssue.title)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                Text(issue.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }
            Spacer(minLength: 0)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(ColorToken.orange.color.opacity(0.08), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}

private struct ProjectDashboardEmptyState: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
    }
}
