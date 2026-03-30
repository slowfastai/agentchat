import SwiftUI

struct AgentListSection: View {
    let agents: [DaemonAgentSummary]
    let hasConfiguredDaemonURL: Bool
    let statusColorForAgent: (DaemonAgentSummary) -> Color
    let onReconnect: () -> Void
    let onEdit: (DaemonAgentSummary) -> Void
    let onDelete: (DaemonAgentSummary) -> Void

    var body: some View {
        Section("Agent Friends") {
            if agents.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("No agents added yet")
                        .foregroundStyle(.secondary)
                    Text(hasConfiguredDaemonURL
                        ? "Reconnect or scan another QR code to discover agents and keep them in this list."
                        : "Scan a QR code or enter a daemon URL to discover agents and keep them in this list.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else {
                ForEach(agents, id: \.agentID) { agent in
                    AgentRowView(
                        agent: agent,
                        statusColor: statusColorForAgent(agent),
                        hasConfiguredDaemonURL: hasConfiguredDaemonURL,
                        onReconnect: onReconnect
                    )
                    .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                        Button(role: .destructive) {
                            onDelete(agent)
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .swipeActions(edge: .leading, allowsFullSwipe: false) {
                        Button {
                            onEdit(agent)
                        } label: {
                            Label("Edit", systemImage: "pencil")
                        }
                        .tint(.blue)
                    }
                }

                Text("Agents stay in this list after first discovery. Create threads and add agents from Feed, and only online agents can join right now.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

struct AgentRowView: View {
    let agent: DaemonAgentSummary
    let statusColor: Color
    let hasConfiguredDaemonURL: Bool
    let onReconnect: () -> Void

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            AgentAvatarView(agent: agent, size: 40)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 4) {
                    Text(agent.displayName)
                        .font(.body.weight(.medium))

                    if agent.customDisplayName != nil {
                        Image(systemName: "pencil.circle.fill")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            VStack(alignment: .trailing, spacing: 6) {
                Text(agent.status.replacingOccurrences(of: "_", with: " ").capitalized)
                    .font(.caption)
                    .foregroundStyle(statusColor)

                if agent.isOffline {
                    Button(action: onReconnect) {
                        Text("Reconnect")
                            .font(.caption2.weight(.semibold))
                    }
                    .buttonStyle(.bordered)
                    .controlSize(.small)
                    .disabled(!hasConfiguredDaemonURL)
                }
            }
        }
    }
}
