import SwiftUI

struct CreateArtifactSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let issueID: UUID
    let thread: Thread?
    var initialDraft: DistilledArtifactDraft? = nil

    @State private var kind: IssueArtifactKind = .note
    @State private var title = ""
    @State private var summary = ""
    @State private var pathOrURL = ""

    var body: some View {
        VStack(spacing: 0) {
            header(title: "Add Artifact")

            Divider()

            Form {
                Section("Kind") {
                    Picker("Kind", selection: $kind) {
                        ForEach(IssueArtifactKind.allCases) { kind in
                            Text(kind.title).tag(kind)
                        }
                    }
                }

                Section("Details") {
                    TextField("Title", text: $title)
                    TextField("Path or URL", text: $pathOrURL)
                    TextEditor(text: $summary)
                        .frame(height: 90)
                }

                if let thread {
                    Section("Source Thread") {
                        Text(thread.title)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            footer(buttonTitle: "Save Artifact", isDisabled: title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) {
                store.addArtifact(
                    issueID: issueID,
                    threadID: thread?.id,
                    kind: kind,
                    title: title,
                    summary: summary,
                    pathOrURL: pathOrURL
                )
                dismiss()
            }
        }
        .frame(width: 480, height: 420)
        .onAppear {
            if let initialDraft {
                kind = initialDraft.kind
                title = initialDraft.title
                summary = initialDraft.summary
                pathOrURL = initialDraft.pathOrURL
            } else if let thread, title.isEmpty {
                title = "\(thread.title) artifact"
                summary = thread.latestActivityText
            }
        }
    }

    @ViewBuilder
    private func header(title: String) -> some View {
        HStack {
            Text(title)
                .font(.headline)
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding()
    }

    @ViewBuilder
    private func footer(buttonTitle: String, isDisabled: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button(buttonTitle, action: action)
                .keyboardShortcut(.defaultAction)
                .disabled(isDisabled)
        }
        .padding()
    }
}

struct CreateDecisionSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let issueID: UUID
    let thread: Thread?
    var initialDraft: DistilledDecisionDraft? = nil

    @State private var title = ""
    @State private var rationale = ""

    var body: some View {
        VStack(spacing: 0) {
            header(title: "Add Decision")

            Divider()

            Form {
                Section("Decision") {
                    TextField("Title", text: $title)
                    TextEditor(text: $rationale)
                        .frame(height: 120)
                }

                if let thread {
                    Section("Source Thread") {
                        Text(thread.title)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            footer(buttonTitle: "Save Decision", isDisabled: isSaveDisabled) {
                store.addDecision(
                    issueID: issueID,
                    threadID: thread?.id,
                    title: title,
                    rationale: rationale
                )
                dismiss()
            }
        }
        .frame(width: 480, height: 360)
        .onAppear {
            if let initialDraft {
                title = initialDraft.title
                rationale = initialDraft.rationale
            } else if let thread, title.isEmpty {
                title = "Decision from \(thread.title)"
                rationale = thread.latestActivityText
            }
        }
    }

    private var isSaveDisabled: Bool {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ||
        rationale.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    @ViewBuilder
    private func header(title: String) -> some View {
        HStack {
            Text(title)
                .font(.headline)
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding()
    }

    @ViewBuilder
    private func footer(buttonTitle: String, isDisabled: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button(buttonTitle, action: action)
                .keyboardShortcut(.defaultAction)
                .disabled(isDisabled)
        }
        .padding()
    }
}

struct CreateFollowUpIssueSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let projectID: UUID
    let sourceIssueID: UUID
    let draft: DistilledIssueDraft

    @State private var title: String
    @State private var summary: String
    @State private var status: IssueStatus
    @State private var priority: IssuePriority
    @State private var selectedAssignees: Set<UUID>

    init(projectID: UUID, sourceIssueID: UUID, draft: DistilledIssueDraft) {
        self.projectID = projectID
        self.sourceIssueID = sourceIssueID
        self.draft = draft
        _title = State(initialValue: draft.title)
        _summary = State(initialValue: draft.summary)
        _status = State(initialValue: draft.status)
        _priority = State(initialValue: draft.priority)
        _selectedAssignees = State(initialValue: Set(draft.assignees.map(\.id)))
    }

    var body: some View {
        VStack(spacing: 0) {
            header(title: "Create Follow-up Issue")

            Divider()

            Form {
                Section("Title") {
                    TextField("Issue title", text: $title)
                }

                Section("Summary") {
                    TextEditor(text: $summary)
                        .frame(height: 120)
                }

                Section("Properties") {
                    Picker("Status", selection: $status) {
                        ForEach(IssueStatus.allCases, id: \.self) { status in
                            Text(status.title).tag(status)
                        }
                    }

                    Picker("Priority", selection: $priority) {
                        ForEach(IssuePriority.allCases, id: \.self) { priority in
                            Text(priority.title).tag(priority)
                        }
                    }
                }

                Section("Assignees") {
                    ForEach(store.agents) { agent in
                        Button {
                            if selectedAssignees.contains(agent.id) {
                                selectedAssignees.remove(agent.id)
                            } else {
                                selectedAssignees.insert(agent.id)
                            }
                        } label: {
                            HStack {
                                AvatarView(title: agent.name, accent: agent.accent, size: 24)
                                Text(agent.name)
                                    .foregroundStyle(.primary)
                                Spacer()
                                if selectedAssignees.contains(agent.id) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .foregroundStyle(Color(agent.accent))
                                }
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .formStyle(.grouped)

            Divider()

            footer(buttonTitle: "Create Issue", isDisabled: title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) {
                let assignees = selectedAssignees.compactMap { agentID in
                    store.agents.first(where: { $0.id == agentID }).map { agent in
                        ParticipantRef(
                            id: agent.id,
                            displayName: agent.name,
                            role: .agent(agent.kind),
                            accent: agent.accent
                        )
                    }
                }

                if let createdIssueID = store.addIssue(
                    to: projectID,
                    title: title.trimmingCharacters(in: .whitespacesAndNewlines),
                    summary: summary.trimmingCharacters(in: .whitespacesAndNewlines),
                    status: status,
                    priority: priority,
                    assignees: assignees
                ) {
                    store.selectedProjectID = projectID
                    store.selectedIssueID = createdIssueID
                    store.selectedThreadID = nil
                }

                dismiss()
            }
        }
        .frame(width: 540, height: 520)
    }

    @ViewBuilder
    private func header(title: String) -> some View {
        HStack {
            Text(title)
                .font(.headline)
            Spacer()
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark.circle.fill")
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding()
    }

    @ViewBuilder
    private func footer(buttonTitle: String, isDisabled: Bool, action: @escaping () -> Void) -> some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button(buttonTitle, action: action)
                .keyboardShortcut(.defaultAction)
                .disabled(isDisabled)
        }
        .padding()
    }
}
