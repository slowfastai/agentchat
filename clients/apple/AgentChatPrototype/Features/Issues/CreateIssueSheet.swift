import SwiftUI

struct CreateIssueSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let projectID: UUID

    @State private var title = ""
    @State private var summary = ""
    @State private var status: IssueStatus = .backlog
    @State private var priority: IssuePriority = .medium
    @State private var selectedAssignees: Set<UUID> = []
    @State private var isValid = false

    var body: some View {
        VStack(spacing: 0) {
            header
            
            Divider()
            
            formContent
            
            Divider()
            
            footer
        }
        .frame(width: 520, height: 480)
        .onChange(of: title) { _, _ in validateForm() }
    }

    private var header: some View {
        HStack {
            Text("New Task")
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

    private var formContent: some View {
        Form {
            Section {
                TextField("Task Title", text: $title)
                    .textFieldStyle(.roundedBorder)
            } header: {
                Text("Title")
            }

            Section {
                TextEditor(text: $summary)
                    .frame(height: 80)
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(Color.primary.opacity(0.1), lineWidth: 1)
                    )
            } header: {
                Text("Description")
            }

            Section {
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
            } header: {
                Text("Properties")
            }

            Section {
                if store.agents.isEmpty {
                    Text("No agents available")
                        .foregroundStyle(.secondary)
                } else {
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
            } header: {
                Text("Assignees")
            }
        }
        .formStyle(.grouped)
    }

    private var footer: some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button("Create") {
                createIssue()
            }
            .keyboardShortcut(.defaultAction)
            .disabled(!isValid)
        }
        .padding()
    }

    private func validateForm() {
        isValid = !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func createIssue() {
        var assignees: [ParticipantRef] = []
        for agentID in selectedAssignees {
            if let agent = store.agents.first(where: { $0.id == agentID }) {
                assignees.append(ParticipantRef(
                    id: agent.id,
                    displayName: agent.name,
                    role: .agent(agent.kind),
                    accent: agent.accent
                ))
            }
        }

        store.addIssue(
            to: projectID,
            title: title.trimmingCharacters(in: .whitespacesAndNewlines),
            summary: summary.trimmingCharacters(in: .whitespacesAndNewlines),
            status: status,
            priority: priority,
            assignees: assignees
        )

        dismiss()
    }
}