import SwiftUI

struct ProjectListView: View {
    @EnvironmentObject private var store: DemoStore
    @Binding var selectedProjectID: UUID?
    @Binding var selectedIssueID: UUID?
    @Binding var showCreateProject: Bool

    @State private var projectForNewIssue: Project?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                if store.projects.isEmpty {
                    emptyState
                } else {
                    ForEach(store.projects) { project in
                        ProjectCard(
                            project: project,
                            isSelected: selectedProjectID == project.id,
                            onSelect: {
                                selectedProjectID = project.id
                                if selectedIssueID == nil {
                                    selectedIssueID = project.issues.first?.id
                                }
                            },
                            onAddIssue: {
                                projectForNewIssue = project
                            }
                        )
                    }
                }
            }
            .padding(AppSpacing.lg)
        }
        .navigationTitle("Projects")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showCreateProject = true
                } label: {
                    Label("Add Project", systemImage: "plus")
                }
            }
        }
        .sheet(item: $projectForNewIssue) { project in
            CreateIssueSheet(projectID: project.id)
        }
    }

    private var emptyState: some View {
        CardSurface(accent: .gray) {
            VStack(spacing: AppSpacing.md) {
                Image(systemName: "folder.badge.plus")
                    .font(.system(size: 42))
                    .foregroundStyle(.secondary)
                Text("No Projects")
                    .font(.title3.weight(.semibold))
                Text("Create a project to start managing issues and threads.")
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button("Create Project") {
                    showCreateProject = true
                }
                .buttonStyle(.borderedProminent)
            }
            .frame(minHeight: 260)
            .padding(AppSpacing.md)
        }
    }
}

private struct ProjectCard: View {
    let project: Project
    let isSelected: Bool
    let onSelect: () -> Void
    let onAddIssue: () -> Void

    var body: some View {
        CardSurface(accent: project.color, isSelected: isSelected) {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                Button(action: onSelect) {
                    HStack(alignment: .top, spacing: AppSpacing.md) {
                        VStack(alignment: .leading, spacing: 6) {
                            HStack(spacing: 8) {
                                Image(systemName: "folder.fill")
                                    .foregroundStyle(Color(project.color))
                                Text(project.name)
                                    .font(.title3.weight(.semibold))
                            }

                            Text(project.repoPath)
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                                .truncationMode(.middle)
                        }

                        Spacer()

                        VStack(alignment: .trailing, spacing: 4) {
                            Text("\(project.issues.count)")
                                .font(.headline.monospacedDigit())
                            Text("issues")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .buttonStyle(.plain)

                if !project.issues.isEmpty {
                    Divider()

                    VStack(alignment: .leading, spacing: AppSpacing.sm) {
                        ForEach(project.issues.prefix(3)) { issue in
                            HStack(spacing: 8) {
                                Text("#\(issue.number)")
                                    .font(.caption.monospacedDigit())
                                    .foregroundStyle(.secondary)
                                Text(issue.title)
                                    .font(.subheadline)
                                    .lineLimit(1)
                                Spacer()
                                StatusBadge(text: issue.status.title, color: issue.status.badgeColor)
                            }
                            .contentShape(Rectangle())
                        }

                        if project.issues.count > 3 {
                            Text("+ \(project.issues.count - 3) more issues")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                HStack {
                    Button {
                        onAddIssue()
                    } label: {
                        Label("Add Issue", systemImage: "plus")
                            .font(.caption)
                    }
                    .buttonStyle(.bordered)

                    Spacer()
                }
            }
        }
    }
}
