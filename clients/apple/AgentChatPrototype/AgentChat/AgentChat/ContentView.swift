//
//  ContentView.swift
//  AgentChat
//
//  Created by Jia Li on 2026/3/24.
//

import SwiftData
import SwiftUI

private enum AppTab: Hashable {
    case feed
    case agents
    case settings
    case search
}

private enum AppPalette {
    static let canvas = Color(uiColor: .systemGroupedBackground)
    static let card = Color(uiColor: .secondarySystemGroupedBackground)
    static let accent = Color(red: 0.14, green: 0.39, blue: 0.86)
    static let accentSecondary = Color(red: 0.95, green: 0.47, blue: 0.32)
    static let agent = Color(red: 0.15, green: 0.56, blue: 0.47)
    static let success = Color(red: 0.16, green: 0.61, blue: 0.44)
}

struct ContentView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Item.timestamp, order: .reverse, animation: .default) private var items: [Item]

    @AppStorage("agentchat_has_seeded_demo_feed") private var hasSeededDemoFeed = false
    @AppStorage("agentchat_prefers_dense_cards") private var prefersDenseCards = false
    @AppStorage("agentchat_enable_haptics") private var enableHaptics = true
    @AppStorage("agentchat_show_agent_presence") private var showAgentPresence = true
    @AppStorage("agentchat_prefer_local_daemon") private var preferLocalDaemon = true

    @State private var selectedTab: AppTab = .feed
    @State private var searchText = ""

    private var agentFriends: [AgentFriend] {
        AgentDirectory.seed
    }

    private var searchedSessions: [Item] {
        let trimmed = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return items }

        let needle = trimmed.lowercased()
        return items.filter { item in
            let haystack = [item.title, item.summary]
                .joined(separator: " ")
                .lowercased()
            return haystack.contains(needle)
        }
    }

    private var searchedAgents: [AgentFriend] {
        let trimmed = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return agentFriends }

        let needle = trimmed.lowercased()
        return agentFriends.filter { agent in
            let haystack = ([agent.name, agent.summary, agent.connectionLabel] + agent.capabilities)
                .joined(separator: " ")
                .lowercased()
            return haystack.contains(needle)
        }
    }

    private var searchSuggestions: [String] {
        Array(
            Set(
                items.map(\.title).filter { !$0.isEmpty } +
                agentFriends.map(\.name)
            )
        )
        .sorted()
        .prefix(6)
        .map { $0 }
    }

    var body: some View {
        TabView(selection: $selectedTab) {
            Tab("Feed", systemImage: "square.stack.3d.up.fill", value: AppTab.feed) {
                FeedTabView(
                    items: items,
                    agentCount: agentFriends.count,
                    prefersDenseCards: prefersDenseCards,
                    onAddItem: addItem,
                    onDeleteItem: deleteItem
                )
            }

            Tab("Agents", systemImage: "person.2.fill", value: AppTab.agents) {
                AgentsTabView(
                    agents: agentFriends,
                    showAgentPresence: showAgentPresence
                )
            }

            Tab("Settings", systemImage: "gearshape.fill", value: AppTab.settings) {
                SettingsTabView(
                    itemCount: items.count,
                    agentCount: agentFriends.count,
                    prefersDenseCards: $prefersDenseCards,
                    enableHaptics: $enableHaptics,
                    showAgentPresence: $showAgentPresence,
                    preferLocalDaemon: $preferLocalDaemon,
                    onAddDemoSession: addItem,
                    onClearAll: clearAllItems
                )
            }

            Tab(value: AppTab.search, role: .search) {
                SearchTabView(
                    sessions: searchedSessions,
                    agents: searchedAgents,
                    searchText: searchText,
                    prefersDenseCards: prefersDenseCards,
                    showAgentPresence: showAgentPresence
                )
                .searchable(
                    text: $searchText,
                    placement: .toolbar,
                    prompt: "Search sessions or agents"
                )
                .searchSuggestions {
                    ForEach(searchSuggestions, id: \.self) { suggestion in
                        Text(suggestion)
                            .searchCompletion(suggestion)
                    }
                }
            }
        }
        .task {
            seedDemoItemsIfNeeded()
        }
    }

    private func addItem() {
        withAnimation(.spring(response: 0.35, dampingFraction: 0.88)) {
            let blueprint = SessionBlueprint.next(for: items.count)
            let newItem = Item(
                timestamp: Date(),
                title: blueprint.title,
                summary: blueprint.summary
            )
            modelContext.insert(newItem)
        }
    }

    private func deleteItem(_ item: Item) {
        withAnimation(.easeInOut(duration: 0.2)) {
            modelContext.delete(item)
        }
    }

    private func clearAllItems() {
        withAnimation(.easeInOut(duration: 0.2)) {
            items.forEach { modelContext.delete($0) }
        }
    }

    private func seedDemoItemsIfNeeded() {
        guard !hasSeededDemoFeed else { return }

        defer { hasSeededDemoFeed = true }

        guard items.isEmpty else { return }

        for sample in SessionBlueprint.seedItems() {
            modelContext.insert(sample)
        }
    }
}

private struct FeedTabView: View {
    let items: [Item]
    let agentCount: Int
    let prefersDenseCards: Bool
    let onAddItem: () -> Void
    let onDeleteItem: (Item) -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    FeedHeroCard(
                        sessionCount: items.count,
                        agentCount: agentCount,
                        latestTimestamp: items.first?.timestamp
                    )

                    if items.isEmpty {
                        EmptyFeedCard(onAddItem: onAddItem)
                    } else {
                        LazyVStack(spacing: prefersDenseCards ? 12 : 16) {
                            ForEach(items) { item in
                                SessionCard(
                                    item: item,
                                    prefersDenseLayout: prefersDenseCards,
                                    onDelete: { onDeleteItem(item) }
                                )
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 28)
            }
            .background(AppPalette.canvas.ignoresSafeArea())
            .navigationTitle("Feed")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    Button(action: onAddItem) {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("Add Session")
                }
            }
        }
    }
}

private struct AgentsTabView: View {
    let agents: [AgentFriend]
    let showAgentPresence: Bool

    private var onlineCount: Int {
        agents.filter(\.isOnline).count
    }

    private var skillCount: Int {
        Set(agents.flatMap { $0.capabilities }).count
    }

    private var groupedAgents: [AgentDirectorySection] {
        let sortedAgents = agents.sorted {
            $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
        }
        let grouped = Dictionary(grouping: sortedAgents) { agent in
            String(agent.name.prefix(1)).uppercased()
        }

        return grouped.keys.sorted().map { key in
            AgentDirectorySection(
                title: key,
                agents: grouped[key] ?? []
            )
        }
    }

    private var shortcuts: [AgentShortcut] {
        [
            AgentShortcut(
                title: "New Session",
                subtitle: "Kick off a daemon-backed run",
                systemImage: "plus.bubble.fill",
                tint: AppPalette.accent
            ),
            AgentShortcut(
                title: "Skill Library",
                subtitle: "Browse distilled memory",
                systemImage: "book.closed.fill",
                tint: AppPalette.accentSecondary
            ),
            AgentShortcut(
                title: "Relay Link",
                subtitle: "Pair through encrypted transport",
                systemImage: "antenna.radiowaves.left.and.right",
                tint: AppPalette.agent
            ),
            AgentShortcut(
                title: "Local Daemon",
                subtitle: "Prefer ws://127.0.0.1:9390",
                systemImage: "desktopcomputer",
                tint: AppPalette.success
            )
        ]
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    SectionIntroCard(
                        eyebrow: "Roster",
                        title: "Agent Friends",
                        subtitle: "These are the coding agents the app can route sessions to through the local daemon and ACP bridge.",
                        tint: AppPalette.agent
                    )

                    AppCard {
                        HStack(spacing: 12) {
                            MetricPill(
                                title: "Available",
                                value: "\(agents.count)",
                                tint: AppPalette.accent.opacity(0.14),
                                foreground: AppPalette.accent
                            )
                            MetricPill(
                                title: "Online",
                                value: "\(onlineCount)",
                                tint: AppPalette.agent.opacity(0.14),
                                foreground: AppPalette.agent
                            )
                            MetricPill(
                                title: "Skills",
                                value: "\(skillCount)",
                                tint: AppPalette.accentSecondary.opacity(0.14),
                                foreground: AppPalette.accentSecondary
                            )
                            Spacer(minLength: 0)
                        }
                    }

                    LazyVGrid(
                        columns: [
                            GridItem(.flexible(), spacing: 12),
                            GridItem(.flexible(), spacing: 12)
                        ],
                        spacing: 12
                    ) {
                        ForEach(shortcuts) { shortcut in
                            AgentShortcutCard(shortcut: shortcut)
                        }
                    }

                    AgentDirectoryCard(
                        groupedAgents: groupedAgents,
                        showAgentPresence: showAgentPresence
                    )
                }
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 28)
            }
            .background(AppPalette.canvas.ignoresSafeArea())
            .navigationTitle("Agents")
            .navigationBarTitleDisplayMode(.large)
        }
    }
}

private struct SettingsTabView: View {
    let itemCount: Int
    let agentCount: Int
    @Binding var prefersDenseCards: Bool
    @Binding var enableHaptics: Bool
    @Binding var showAgentPresence: Bool
    @Binding var preferLocalDaemon: Bool
    let onAddDemoSession: () -> Void
    let onClearAll: () -> Void

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    SectionIntroCard(
                        eyebrow: "Workspace",
                        title: "Settings",
                        subtitle: "Tune feed density, agent presence, and how this prototype thinks about the local daemon.",
                        tint: AppPalette.accentSecondary
                    )

                    AppCard {
                        VStack(alignment: .leading, spacing: 18) {
                            Text("Display")
                                .font(.headline)

                            Toggle("Compact cards", isOn: $prefersDenseCards)
                            Toggle("Haptic feedback", isOn: $enableHaptics)
                            Toggle("Show agent presence", isOn: $showAgentPresence)
                            Toggle("Prefer local daemon", isOn: $preferLocalDaemon)
                        }
                    }

                    AppCard {
                        VStack(alignment: .leading, spacing: 18) {
                            Text("Library")
                                .font(.headline)

                            HStack(spacing: 12) {
                                MetricPill(title: "Sessions", value: "\(itemCount)", tint: AppPalette.accent)
                                MetricPill(title: "Agents", value: "\(agentCount)", tint: AppPalette.agent)
                                Spacer(minLength: 0)
                            }

                            VStack(spacing: 12) {
                                Button(action: onAddDemoSession) {
                                    SettingsActionRow(
                                        title: "Add demo session",
                                        subtitle: "Insert another workspace card into Feed",
                                        systemImage: "plus.circle.fill",
                                        tint: AppPalette.success
                                    )
                                }
                                .buttonStyle(.plain)

                                Button(role: .destructive, action: onClearAll) {
                                    SettingsActionRow(
                                        title: "Clear all sessions",
                                        subtitle: "Remove the local demo session history on this device",
                                        systemImage: "trash.fill",
                                        tint: .red
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 28)
            }
            .background(AppPalette.canvas.ignoresSafeArea())
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.large)
        }
    }
}

private struct SearchTabView: View {
    let sessions: [Item]
    let agents: [AgentFriend]
    let searchText: String
    let prefersDenseCards: Bool
    let showAgentPresence: Bool

    private var isSearching: Bool {
        !searchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    SectionIntroCard(
                        eyebrow: "Search",
                        title: "Find Sessions And Agents",
                        subtitle: isSearching
                            ? "Results for \"\(searchText)\" across session history and agent roster."
                            : "Search session titles, summaries, and the agents available to join a run.",
                        tint: AppPalette.accent
                    )

                    if !isSearching {
                        SearchHintCard()
                    } else if sessions.isEmpty && agents.isEmpty {
                        EmptyStateCard(
                            systemImage: "magnifyingglass",
                            title: "No matches",
                            message: "Try another issue title, keyword, or agent capability."
                        )
                    } else {
                        if !agents.isEmpty {
                            SearchSectionHeader(title: "Agents")

                            LazyVStack(spacing: 16) {
                                ForEach(agents) { agent in
                                    AgentFriendCard(
                                        agent: agent,
                                        showsPresence: showAgentPresence
                                    )
                                }
                            }
                        }

                        if !sessions.isEmpty {
                            SearchSectionHeader(title: "Sessions")

                            LazyVStack(spacing: prefersDenseCards ? 12 : 16) {
                                ForEach(sessions) { item in
                                    SessionCard(
                                        item: item,
                                        prefersDenseLayout: prefersDenseCards,
                                        onDelete: nil
                                    )
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 28)
            }
            .background(AppPalette.canvas.ignoresSafeArea())
            .navigationTitle("Search")
            .navigationBarTitleDisplayMode(.large)
        }
    }
}

private struct FeedHeroCard: View {
    let sessionCount: Int
    let agentCount: Int
    let latestTimestamp: Date?

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            RoundedRectangle(cornerRadius: 28, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            AppPalette.accent,
                            Color(red: 0.34, green: 0.58, blue: 0.95),
                            AppPalette.accentSecondary
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )

            VStack(alignment: .leading, spacing: 16) {
                Text("DAILY AGENT WORKSPACE")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.78))

                Text("Recent sessions, available agents, and the next actions you can route through the daemon.")
                    .font(.title2.weight(.bold))
                    .foregroundStyle(.white)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(spacing: 12) {
                    MetricPill(title: "Sessions", value: "\(sessionCount)", tint: .white.opacity(0.18), foreground: .white)
                    MetricPill(title: "Agents", value: "\(agentCount)", tint: .white.opacity(0.18), foreground: .white)

                    if let latestTimestamp {
                        MetricPill(
                            title: "Latest",
                            value: latestTimestamp.formatted(.dateTime.month(.abbreviated).day()),
                            tint: .white.opacity(0.18),
                            foreground: .white
                        )
                    }
                }
            }
            .padding(22)
        }
        .frame(minHeight: 196)
    }
}

private struct SectionIntroCard: View {
    let eyebrow: String
    let title: String
    let subtitle: String
    let tint: Color

    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: 10) {
                Text(eyebrow.uppercased())
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(tint)
                Text(title)
                    .font(.title2.weight(.bold))
                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct SearchSectionHeader: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.headline)
            .foregroundStyle(.secondary)
            .padding(.horizontal, 4)
    }
}

private struct AgentFriendCard: View {
    let agent: AgentFriend
    let showsPresence: Bool

    var body: some View {
        AppCard {
            HStack(alignment: .top, spacing: 14) {
                AgentGlyph(agent: agent)

                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 8) {
                        Text(agent.name)
                            .font(.title3.weight(.semibold))

                        if showsPresence {
                            PresenceBadge(isOnline: agent.isOnline)
                        }
                    }

                    Text(agent.summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)

                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 8) {
                            ForEach(agent.capabilities, id: \.self) { capability in
                                CapabilityChip(text: capability, tint: agent.tint)
                            }
                        }
                    }

                    Label(agent.connectionLabel, systemImage: "link")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer(minLength: 0)
            }
        }
    }
}

private struct AgentDirectoryCard: View {
    let groupedAgents: [AgentDirectorySection]
    let showAgentPresence: Bool

    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text("Directory")
                        .font(.headline)

                    Spacer()

                    Text("\(groupedAgents.count) groups")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: 18) {
                    ForEach(Array(groupedAgents.enumerated()), id: \.element.id) { sectionIndex, section in
                        VStack(alignment: .leading, spacing: 12) {
                            Text(section.title)
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)

                            VStack(spacing: 0) {
                                ForEach(Array(section.agents.enumerated()), id: \.element.id) { agentIndex, agent in
                                    AgentDirectoryRow(
                                        agent: agent,
                                        showsPresence: showAgentPresence
                                    )

                                    if agentIndex < section.agents.count - 1 {
                                        Divider()
                                            .padding(.leading, 62)
                                    }
                                }
                            }
                        }

                        if sectionIndex < groupedAgents.count - 1 {
                            Divider()
                        }
                    }
                }
            }
        }
    }
}

private struct AgentShortcutCard: View {
    let shortcut: AgentShortcut

    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: 14) {
                Image(systemName: shortcut.systemImage)
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(.white)
                    .frame(width: 42, height: 42)
                    .background(shortcut.tint, in: RoundedRectangle(cornerRadius: 14, style: .continuous))

                VStack(alignment: .leading, spacing: 6) {
                    Text(shortcut.title)
                        .font(.headline)

                    Text(shortcut.subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct AgentDirectoryRow: View {
    let agent: AgentFriend
    let showsPresence: Bool

    private var subtitle: String {
        if !agent.capabilities.isEmpty {
            return agent.capabilities.prefix(3).joined(separator: " · ")
        }

        return agent.summary
    }

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            AgentGlyph(agent: agent, size: 48)

            VStack(alignment: .leading, spacing: 5) {
                Text(agent.name)
                    .font(.body.weight(.semibold))

                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(agent.tint)
                    .lineLimit(1)

                Text(agent.connectionLabel)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)

            if showsPresence {
                PresenceBadge(isOnline: agent.isOnline)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct SessionCard: View {
    let item: Item
    let prefersDenseLayout: Bool
    let onDelete: (() -> Void)?

    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: prefersDenseLayout ? 12 : 16) {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(timestampLabel)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)

                        Text(displayTitle)
                            .font(prefersDenseLayout ? .headline : .title3.weight(.semibold))
                            .foregroundStyle(.primary)
                            .fixedSize(horizontal: false, vertical: true)

                        Text(displaySummary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                    }

                    Spacer(minLength: 0)

                    if let onDelete {
                        Menu {
                            Button("Delete Session", role: .destructive, action: onDelete)
                        } label: {
                            Image(systemName: "ellipsis")
                                .font(.system(size: 16, weight: .semibold))
                                .foregroundStyle(.secondary)
                                .frame(width: 36, height: 36)
                                .background(Color(uiColor: .tertiarySystemBackground), in: Circle())
                        }
                    }
                }

                HStack {
                    Label(relativeTimestampLabel, systemImage: "clock")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Spacer()

                    NavigationLink {
                        SessionDetailView(item: item)
                    } label: {
                        Label("Open", systemImage: "arrow.right")
                            .font(.subheadline.weight(.semibold))
                    }
                }
            }
        }
        .contextMenu {
            if let onDelete {
                Button("Delete Session", role: .destructive, action: onDelete)
            }
        }
    }

    private var displayTitle: String {
        item.title.isEmpty ? "Session Snapshot" : item.title
    }

    private var displaySummary: String {
        item.summary.isEmpty ? "A recently created local session." : item.summary
    }

    private var timestampLabel: String {
        item.timestamp.formatted(.dateTime.month(.wide).day().hour().minute())
    }

    private var relativeTimestampLabel: String {
        RelativeDateTimeFormatter().localizedString(for: item.timestamp, relativeTo: Date())
    }
}

private struct SessionDetailView: View {
    let item: Item

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                AppCard {
                    VStack(alignment: .leading, spacing: 16) {
                        Text(item.title.isEmpty ? "Session Snapshot" : item.title)
                            .font(.largeTitle.weight(.bold))

                        Text(item.summary.isEmpty ? "A recently created local session." : item.summary)
                            .font(.body)
                            .foregroundStyle(.secondary)

                        HStack(spacing: 12) {
                            MetricPill(
                                title: "Created",
                                value: item.timestamp.formatted(.dateTime.month(.abbreviated).day().hour().minute()),
                                tint: AppPalette.accent.opacity(0.14),
                                foreground: AppPalette.accent
                            )
                            MetricPill(
                                title: "Route",
                                value: "Daemon-ready",
                                tint: AppPalette.agent.opacity(0.14),
                                foreground: AppPalette.agent
                            )
                        }
                    }
                }

                AppCard {
                    VStack(alignment: .leading, spacing: 14) {
                        Text("Protocol Flow")
                            .font(.headline)

                        ProtocolStepRow(
                            command: "create_session",
                            description: "Open a new local daemon session for the working directory."
                        )
                        ProtocolStepRow(
                            command: "prompt",
                            description: "Send the user request to the selected coding agent."
                        )
                        ProtocolStepRow(
                            command: "delta / tool_update",
                            description: "Stream text, thinking, and tool activity back into the iOS client."
                        )
                        ProtocolStepRow(
                            command: "turn_end",
                            description: "Finish the turn with a stop reason once the agent completes."
                        )
                    }
                }
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 28)
        }
        .background(AppPalette.canvas.ignoresSafeArea())
        .navigationTitle("Session")
        .navigationBarTitleDisplayMode(.inline)
    }
}

private struct EmptyFeedCard: View {
    let onAddItem: () -> Void

    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: 16) {
                Image(systemName: "sparkles.rectangle.stack")
                    .font(.system(size: 28))
                    .foregroundStyle(AppPalette.accent)

                Text("Start your feed")
                    .font(.title3.weight(.semibold))

                Text("Add a session to populate Feed. The Agents tab shows who can pick it up next.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Button(action: onAddItem) {
                    Label("Add first session", systemImage: "plus.circle.fill")
                        .font(.subheadline.weight(.semibold))
                }
                .buttonStyle(.borderedProminent)
                .tint(AppPalette.accent)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

private struct EmptyStateCard: View {
    let systemImage: String
    let title: String
    let message: String

    var body: some View {
        AppCard {
            VStack(spacing: 12) {
                Image(systemName: systemImage)
                    .font(.system(size: 28))
                    .foregroundStyle(.secondary)
                Text(title)
                    .font(.title3.weight(.semibold))
                Text(message)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
        }
    }
}

private struct SearchHintCard: View {
    var body: some View {
        AppCard {
            VStack(alignment: .leading, spacing: 16) {
                Image(systemName: "sparkle.magnifyingglass")
                    .font(.system(size: 28))
                    .foregroundStyle(AppPalette.accent)

                Text("Start typing to search")
                    .font(.title3.weight(.semibold))

                Text("This search destination scans both saved sessions and the agent roster, so the product feels like a real multi-agent workspace.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct SettingsActionRow: View {
    let title: String
    let subtitle: String
    let systemImage: String
    let tint: Color

    var body: some View {
        HStack(spacing: 14) {
            Image(systemName: systemImage)
                .font(.system(size: 18, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 40, height: 40)
                .background(tint, in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.primary)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 2)
    }
}

private struct ProtocolStepRow: View {
    let command: String
    let description: String

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Text(command)
                .font(.caption.monospaced().weight(.semibold))
                .foregroundStyle(AppPalette.accent)
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .background(AppPalette.accent.opacity(0.12), in: Capsule())

            Text(description)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

private struct AgentGlyph: View {
    let agent: AgentFriend
    var size: CGFloat = 56

    var body: some View {
        RoundedRectangle(cornerRadius: 16, style: .continuous)
            .fill(agent.tint.opacity(0.18))
            .frame(width: size, height: size)
            .overlay {
                Image(systemName: agent.systemImage)
                    .font(.system(size: size * 0.42, weight: .semibold))
                    .foregroundStyle(agent.tint)
            }
    }
}

private struct CapabilityChip: View {
    let text: String
    let tint: Color

    var body: some View {
        Text(text)
            .font(.caption.weight(.semibold))
            .foregroundStyle(tint)
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background(tint.opacity(0.12), in: Capsule())
    }
}

private struct PresenceBadge: View {
    let isOnline: Bool

    var body: some View {
        HStack(spacing: 6) {
            Circle()
                .fill(isOnline ? AppPalette.agent : Color.secondary.opacity(0.45))
                .frame(width: 8, height: 8)
            Text(isOnline ? "online" : "offline")
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color(uiColor: .tertiarySystemBackground), in: Capsule())
    }
}

private struct AppCard<Content: View>: View {
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        content
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(AppPalette.card, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(Color.primary.opacity(0.05), lineWidth: 1)
            }
    }
}

private struct MetricPill: View {
    let title: String
    let value: String
    let tint: Color
    var foreground: Color = .primary

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.caption2.weight(.semibold))
                .foregroundStyle(foreground.opacity(0.8))
            Text(value)
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(foreground)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(tint, in: Capsule())
    }
}

private struct AgentFriend: Identifiable, Hashable {
    let id: String
    let name: String
    let systemImage: String
    let tint: Color
    let isOnline: Bool
    let summary: String
    let capabilities: [String]
    let connectionLabel: String
}

private struct AgentDirectorySection: Identifiable, Hashable {
    let title: String
    let agents: [AgentFriend]

    var id: String { title }
}

private struct AgentShortcut: Identifiable, Hashable {
    let id = UUID()
    let title: String
    let subtitle: String
    let systemImage: String
    let tint: Color
}

private enum AgentDirectory {
    static let seed: [AgentFriend] = [
        AgentFriend(
            id: "claude",
            name: "Claude",
            systemImage: "brain.head.profile",
            tint: AppPalette.accent,
            isOnline: true,
            summary: "Strong at repo analysis, implementation planning, and careful review of tricky runtime paths.",
            capabilities: ["Reasoning", "Review", "Refactor"],
            connectionLabel: "ACP adapter • claude-code"
        ),
        AgentFriend(
            id: "codex",
            name: "Codex",
            systemImage: "curlybraces.square.fill",
            tint: AppPalette.agent,
            isOnline: true,
            summary: "Fast implementation partner for code changes, tests, and iteration on concrete engineering tasks.",
            capabilities: ["Codegen", "Tests", "Diff Review"],
            connectionLabel: "ACP process • coding session"
        ),
        AgentFriend(
            id: "pi",
            name: "Pi",
            systemImage: "sparkles",
            tint: AppPalette.accentSecondary,
            isOnline: true,
            summary: "Turns finished sessions into reusable memory and skill documents for the next agent run.",
            capabilities: ["Memory", "Distill", "Summaries"],
            connectionLabel: "Daemon distillation flow • .agentchat/skills"
        )
    ]
}

private enum SessionBlueprint {
    private static let titles = [
        "Ship a cleaner workspace feed",
        "Surface the agent roster",
        "Route prompts through the daemon",
        "Polish typography and spacing",
        "Tune local session behavior"
    ]

    private static let summaries = [
        "Use Feed, Agents, Settings, and Search as the top-level structure for the app.",
        "Make the second tab feel like an agent friend list instead of a generic content shelf.",
        "Model the create_session → prompt → stream updates flow from the daemon protocol.",
        "Replace template spacing with larger cards, stronger hierarchy, and better scanability.",
        "Keep local demo controls lightweight while preserving a believable multi-agent product shell."
    ]

    static func next(for count: Int) -> (title: String, summary: String) {
        let index = count % titles.count
        return (titles[index], summaries[index])
    }

    static func seedItems() -> [Item] {
        [
            Item(
                timestamp: Date().addingTimeInterval(-60 * 22),
                title: "Ship a cleaner workspace feed",
                summary: "Turn the starter template into a feed of session cards that reads like a real agent workspace."
            ),
            Item(
                timestamp: Date().addingTimeInterval(-60 * 95),
                title: "Launch the agent roster",
                summary: "Use the second tab for agent friends so the app reflects its multi-agent routing model."
            ),
            Item(
                timestamp: Date().addingTimeInterval(-60 * 180),
                title: "Document daemon protocol flow",
                summary: "Make the session detail screen explain create_session, prompt, streamed updates, and turn_end."
            )
        ]
    }
}

#Preview {
    ContentView()
        .modelContainer(for: Item.self, inMemory: true)
}
