import SwiftUI

struct AgentListView: View {
    @EnvironmentObject private var store: DemoStore

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: AppSpacing.md) {
                CardSurface(accent: .blue) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Agent roster")
                            .font(.title2.weight(.semibold))
                        Text("Treat each coding agent like a collaborator with a personality, capability profile, and assignment role.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }

                LazyVStack(spacing: AppSpacing.md) {
                    ForEach(store.agents) { agent in
                        CardSurface(accent: agent.accent) {
                            VStack(alignment: .leading, spacing: AppSpacing.md) {
                                HStack(spacing: AppSpacing.md) {
                                    AvatarView(title: agent.name, accent: agent.accent, size: 40)
                                    VStack(alignment: .leading, spacing: 4) {
                                        HStack(spacing: 8) {
                                            Text(agent.name)
                                                .font(.headline)
                                            StatusBadge(
                                                text: agent.isOnline ? "Online" : "Offline",
                                                color: agent.isOnline ? .green : .gray
                                            )
                                        }
                                        Text(agent.shortDescription)
                                            .font(.subheadline)
                                            .foregroundStyle(.secondary)
                                    }
                                    Spacer()
                                }

                                HStack(spacing: 8) {
                                    ForEach(agent.capabilityTags, id: \.self) { tag in
                                        PillView(text: tag, color: agent.accent)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .padding(AppSpacing.lg)
        }
        .navigationTitle("Agents")
    }
}
