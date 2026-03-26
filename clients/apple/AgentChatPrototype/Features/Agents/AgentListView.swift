import SwiftUI

enum AppColors {
    static var onlineStatus: Color {
        Color(red: 0.3, green: 0.85, blue: 0.5)
    }

    static var unreadBadge: Color {
        Color(red: 1.0, green: 0.35, blue: 0.35)
    }
}

struct AgentListView: View {
    @EnvironmentObject private var store: DemoStore
    @State private var searchText = ""

    private var shortcutItems: [AgentShortcutItem] {
        [
            AgentShortcutItem(title: "New Agent", systemImage: "person.badge.plus", color: .orange),
            AgentShortcutItem(title: "Group Chats", systemImage: "person.3.fill", color: .green),
            AgentShortcutItem(title: "Labels", systemImage: "tag.fill", color: .blue),
            AgentShortcutItem(title: "Skill Channels", systemImage: "book.closed.fill", color: .purple)
        ]
    }

    private var filteredAgents: [AgentProfile] {
        guard !searchText.isEmpty else {
            return store.agents.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        }

        return store.agents
            .filter { agent in
                let searchable = [
                    agent.name,
                    agent.shortDescription,
                    agent.capabilityTags.joined(separator: " ")
                ]
                .joined(separator: " ")
                .lowercased()

                return searchable.contains(searchText.lowercased())
            }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    private var groupedAgents: [AgentSection] {
        let grouped = Dictionary(grouping: filteredAgents) { agent in
            String(agent.name.prefix(1)).uppercased()
        }

        return grouped.keys.sorted().map { key in
            AgentSection(
                title: key,
                agents: grouped[key]?.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending } ?? []
            )
        }
    }

    var body: some View {
        ZStack(alignment: .trailing) {
            List {
                Section {
                    ForEach(shortcutItems) { item in
                        AgentShortcutRow(item: item)
                            .listRowInsets(EdgeInsets(top: 10, leading: 16, bottom: 10, trailing: 16))
                    }
                }

                ForEach(groupedAgents) { section in
                    Section(section.title) {
                        ForEach(section.agents) { agent in
                            AgentFriendRow(agent: agent)
                                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
                        }
                    }
                }
            }
            .listStyle(.plain)
            .scrollContentBackground(.hidden)
            .background(Color.appCanvasBackground)
            .searchable(text: $searchText, prompt: "Search agents")
            .navigationTitle("Agent Friends")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button {
                    } label: {
                        Image(systemName: "person.badge.plus")
                    }
                }
            }

            if !groupedAgents.isEmpty {
                AgentSectionIndexOverlay(letters: groupedAgents.map(\.title))
                    .padding(.trailing, 6)
            }
        }
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }
}

private struct AgentShortcutItem: Identifiable {
    let id = UUID()
    let title: String
    let systemImage: String
    let color: ColorToken
}

private struct AgentSection: Identifiable {
    let title: String
    let agents: [AgentProfile]

    var id: String { title }
}

private struct AgentShortcutRow: View {
    let item: AgentShortcutItem

    var body: some View {
        HStack(spacing: 14) {
            ContactIconTile(title: item.title, accent: item.color, systemImage: item.systemImage)

            Text(item.title)
                .font(.body)
                .foregroundStyle(.primary)

            Spacer()
        }
        .contentShape(Rectangle())
    }
}

private struct AgentFriendRow: View {
    let agent: AgentProfile

    var body: some View {
        HStack(spacing: 14) {
            ContactIconTile(
                title: agent.name,
                accent: agent.accent,
                systemImage: systemImage(for: agent.kind)
            )

            VStack(alignment: .leading, spacing: 3) {
                Text(agent.name)
                    .font(.body)
                    .foregroundStyle(.primary)

                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            if agent.isOnline {
                HStack(spacing: 6) {
                    Circle()
                        .fill(AppColors.onlineStatus)
                        .frame(width: 8, height: 8)
                    Text("online")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .contentShape(Rectangle())
    }

    private var subtitle: String {
        if !agent.capabilityTags.isEmpty {
            return agent.capabilityTags.prefix(3).joined(separator: " · ")
        }
        return agent.shortDescription
    }

    private func systemImage(for kind: AgentKind) -> String {
        switch kind {
        case .claude:
            return "brain.head.profile"
        case .codex:
            return "curlybraces.square.fill"
        case .pi:
            return "sparkles"
        case .opencode:
            return "terminal.fill"
        case .human:
            return "person.fill"
        }
    }
}

private struct ContactIconTile: View {
    let title: String
    let accent: ColorToken
    let systemImage: String

    var body: some View {
        RoundedRectangle(cornerRadius: 10, style: .continuous)
            .fill(accent.color)
            .frame(width: 42, height: 42)
            .overlay {
                Image(systemName: systemImage)
                    .font(.system(size: 19, weight: .semibold))
                    .foregroundStyle(.white)
            }
    }
}

private struct AgentSectionIndexOverlay: View {
    let letters: [String]

    var body: some View {
        VStack(spacing: 3) {
            Image(systemName: "magnifyingglass")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
                .padding(.bottom, 2)

            ForEach(letters, id: \.self) { letter in
                Text(letter)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 8)
        .frame(width: 18)
        .background(Color.clear)
    }
}
