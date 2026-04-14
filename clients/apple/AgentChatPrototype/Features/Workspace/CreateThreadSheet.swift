import SwiftUI

private struct ThreadCard: View {
    let thread: Thread

    var body: some View {
        CardSurface(accent: thread.state.badgeColor) {
            VStack(alignment: .leading, spacing: AppSpacing.sm) {
                HStack {
                    Text(thread.title)
                        .font(.headline)
                    Spacer()
                    StatusBadge(text: thread.state.title, color: thread.state.badgeColor)
                }

                if !thread.participants.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(thread.participants) { participant in
                            AvatarView(title: participant.displayName, accent: participant.accent, size: 20)
                        }
                    }
                }

                if !thread.latestActivityText.isEmpty {
                    Text(thread.latestActivityText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Text(AppFormatters.relativeString(from: thread.createdAt))
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

struct CreateThreadSheet: View {
    @EnvironmentObject private var store: DemoStore
    @Environment(\.dismiss) private var dismiss

    let issueID: UUID

    @State private var selectedAgents: Set<UUID> = []
    @State private var isValid = false

    var body: some View {
        VStack(spacing: 0) {
            header

            Divider()

            formContent

            Divider()

            footer
        }
        .frame(width: 400, height: 360)
        .onChange(of: selectedAgents) { _, _ in validateForm() }
    }

    private var header: some View {
        HStack {
            Text("Start New Thread")
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
        VStack(alignment: .leading, spacing: AppSpacing.md) {
            Text("Select agents to participate in this thread")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if store.agents.isEmpty {
                Text("No agents available")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding()
            } else {
                VStack(spacing: AppSpacing.sm) {
                    ForEach(store.agents.filter { $0.isOnline }) { agent in
                        Button {
                            if selectedAgents.contains(agent.id) {
                                selectedAgents.remove(agent.id)
                            } else {
                                selectedAgents.insert(agent.id)
                            }
                        } label: {
                            HStack {
                                AvatarView(title: agent.name, accent: agent.accent, size: 32)
                                VStack(alignment: .leading) {
                                    Text(agent.name)
                                        .font(.body.weight(.medium))
                                    Text(agent.shortDescription)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                                Spacer()
                                if selectedAgents.contains(agent.id) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .foregroundStyle(Color(agent.accent))
                                }
                            }
                            .padding(AppSpacing.sm)
                            .background(
                                selectedAgents.contains(agent.id)
                                    ? Color(agent.accent).opacity(0.1)
                                    : Color.clear,
                                in: RoundedRectangle(cornerRadius: 8)
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding()
    }

    private var footer: some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Button("Start Thread") {
                createThread()
            }
            .keyboardShortcut(.defaultAction)
            .disabled(!isValid)
        }
        .padding()
    }

    private func validateForm() {
        isValid = !selectedAgents.isEmpty
    }

    private func createThread() {
        let agentNames = store.agents
            .filter { selectedAgents.contains($0.id) }
            .map { $0.name }

        store.createThread(for: issueID, agentNames: agentNames)
        dismiss()
    }
}