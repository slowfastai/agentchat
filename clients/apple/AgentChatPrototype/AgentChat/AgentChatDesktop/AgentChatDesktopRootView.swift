import SwiftUI
import AppKit

struct AgentChatDesktopRootView: View {
    @EnvironmentObject private var store: DaemonChatStore
    @SceneStorage("agentchat.desktop.showSidebar") private var showSidebar = true
    @SceneStorage("agentchat.desktop.selectedTab") private var selectedTab: String = DesktopTab.chats.rawValue

    @State private var selectedThreadID: String?

    private enum DesktopTab: String {
        case chats = "chats"
        case agents = "agents"
        case settings = "settings"
    }
    @State private var sidebarSearchText = ""
    @State private var composerText = ""
    @State private var draftsByThreadID: [String: String] = [:]
    @State private var showNewThreadSheet = false
    @State private var showAddAgentsSheet = false
    @State private var editingAgent: DaemonAgentSummary?
    @State private var showCommandPalette = false
    @State private var showQuickConnect = false
    @AppStorage("agentchat.desktop.seenThreadSequences") private var seenThreadSequencesJSON: String = "{}"
    @State private var seenThreadSequences: [String: UInt64] = [:]
    @FocusState private var isComposerFocused: Bool

    private func loadSeenThreadSequences() -> [String: UInt64] {
        guard let data = seenThreadSequencesJSON.data(using: .utf8),
              let decoded = try? JSONDecoder().decode([String: UInt64].self, from: data) else {
            return [:]
        }
        return decoded
    }

    private func saveSeenThreadSequences(_ sequences: [String: UInt64]) {
        if let encoded = try? JSONEncoder().encode(sequences),
           let jsonString = String(data: encoded, encoding: .utf8) {
            seenThreadSequencesJSON = jsonString
        }
    }

    private func syncSeenThreadSequences() {
        seenThreadSequences = loadSeenThreadSequences()
    }

    private var filteredThreads: [DaemonThreadSummary] {
        guard !sidebarSearchText.isEmpty else {
            return store.desktopSortedThreads
        }

        let query = sidebarSearchText.lowercased()
        return store.desktopSortedThreads.filter { thread in
            let searchable = [
                store.effectiveThreadTitle(for: thread),
                thread.threadID,
                thread.workingDir,
            ]
            .joined(separator: " ")
            .lowercased()
            return searchable.contains(query)
        }
    }

    private var pinnedThreads: [DaemonThreadSummary] {
        filteredThreads.filter { store.isPinnedThread($0.threadID) }
    }

    private var recentThreads: [DaemonThreadSummary] {
        filteredThreads.filter { !store.isPinnedThread($0.threadID) }
    }

    private var activeThreadSummary: DaemonThreadSummary? {
        if let selectedThreadID {
            return store.desktopSortedThreads.first(where: { $0.threadID == selectedThreadID })
        }
        return store.activeThreadID.flatMap { threadID in
            store.desktopSortedThreads.first(where: { $0.threadID == threadID })
        }
    }

    private var activeThreadSnapshot: DaemonThreadSnapshot? {
        if store.activeThreadSnapshot?.threadID == activeThreadSummary?.threadID {
            return store.activeThreadSnapshot
        }
        return nil
    }

    private func threadSnapshot(for thread: DaemonThreadSummary) -> DaemonThreadSnapshot? {
        if store.activeThreadSnapshot?.threadID == thread.threadID {
            return store.activeThreadSnapshot
        }
        return nil
    }

    private func threadParticipants(for thread: DaemonThreadSummary) -> [DaemonThreadParticipant] {
        threadSnapshot(for: thread)?.participants ?? []
    }

    private var selectedParticipantIDs: Binding<Set<String>> {
        Binding(
            get: { store.desktopSuggestedParticipantIDs(for: activeThreadSnapshot) },
            set: { store.desktopUpdateSelectedParticipantIDs($0, for: activeThreadSnapshot) }
        )
    }

    var body: some View {
        HStack(spacing: 0) {
            DesktopVerticalTabRail(selectedTab: $selectedTab)
            Divider()
            HSplitView {
                if showSidebar {
                    sidebarContent
                }

                detailPane
            }
        }
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button {
                    toggleSidebar()
                } label: {
                    Image(systemName: showSidebar ? "sidebar.left" : "sidebar.right")
                }
                .help(showSidebar ? "Hide Sidebar (Command-B)" : "Show Sidebar (Command-B)")
            }

            ToolbarItem(placement: .keyboard) {
                Button("Command Palette") {
                    showCommandPalette = true
                }
                .keyboardShortcut("k", modifiers: [.command])
            }
        }
        .agentChatDesktopSidebarShortcut { [self] in
            toggleSidebar()
        }
        .background(
            LinearGradient(
                colors: [
                    Color(red: 0.972, green: 0.968, blue: 0.948),
                    Color(red: 0.942, green: 0.934, blue: 0.904),
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
        )
        .sheet(isPresented: $showNewThreadSheet) {
            AgentSelectionSheet(
                title: "Start New Thread",
                subtitle: "Choose the online agents that should join this conversation.",
                agents: store.desktopOnlineAgents,
                initiallySelected: store.selectedAgentIDs,
                confirmLabel: "Create Thread"
            ) { selectedAgentIDs, workingDir in
                store.createThread(withAgentIDs: selectedAgentIDs, workingDir: workingDir)
            }
            .environmentObject(store)
        }
        .sheet(isPresented: $showAddAgentsSheet) {
            AgentSelectionSheet(
                title: "Add Agents",
                subtitle: "Invite more agents into the current thread.",
                agents: store.desktopAvailableAgentsToAdd(to: activeThreadSnapshot),
                initiallySelected: [],
                confirmLabel: "Add to Thread"
            ) { selectedAgentIDs, _ in
                store.addAgents(selectedAgentIDs, toActiveThread: activeThreadSummary?.threadID)
            }
            .environmentObject(store)
        }
        .sheet(item: $editingAgent) { agent in
            AgentEditSheet(
                agent: agent,
                initialSettings: store.agentSettings[agent.agentID] ?? AgentLocalSettings()
            ) { name, avatarData, settings in
                store.updateAgentDisplayName(agent.agentID, displayName: name)
                store.updateAgentAvatar(agent.agentID, imageData: avatarData)
                store.updateAgentSettings(agent.agentID, settings: settings)
                editingAgent = nil
            }
        }
        .overlay {
            if showCommandPalette {
                CommandPaletteView(
                    isPresented: $showCommandPalette,
                    showNewThreadSheet: { showNewThreadSheet = true },
                    showAddAgentsSheet: { showAddAgentsSheet = true },
                    focusComposer: { scheduleComposerFocus() },
                    connectAction: { showQuickConnect = true }
                )
                .environmentObject(store)
            }
        }
        .onAppear {
            synchronizeSelection()
            seedSeenThreadSequencesIfNeeded()
            restoreComposerDraft(for: selectedThreadID ?? store.activeThreadID)
            scheduleComposerFocus()
            markActiveThreadAsSeen()
        }
        .onChange(of: store.activeThreadID) { _, newValue in
            if selectedThreadID == nil || selectedThreadID != newValue {
                selectedThreadID = newValue
            }
        }
        .onChange(of: selectedThreadID) { oldValue, newValue in
            persistComposerDraft(for: oldValue)
            restoreComposerDraft(for: newValue)
            markThreadAsSeen(threadID: newValue)

            guard let newValue, store.activeThreadID != newValue else {
                scheduleComposerFocus()
                return
            }

            store.attachThread(newValue)
            scheduleComposerFocus()
        }
        .onChange(of: composerText) { _, newValue in
            guard let threadID = activeThreadSummary?.threadID else { return }
            if newValue.isEmpty {
                draftsByThreadID.removeValue(forKey: threadID)
            } else {
                draftsByThreadID[threadID] = newValue
            }
        }
        .onChange(of: activeThreadSnapshot?.threadID) { _, _ in
            markActiveThreadAsSeen()
            scheduleComposerFocus()
        }
        .onChange(of: activeThreadSnapshot?.lastThreadSeq) { _, _ in
            markActiveThreadAsSeen()
        }
        .onChange(of: store.threads) { _, _ in
            seedSeenThreadSequencesIfNeeded()
            scheduleComposerFocus()
        }
        .onChange(of: selectedTab) { _, newValue in
            if (newValue == DesktopTab.agents.rawValue || newValue == DesktopTab.settings.rawValue) && !showSidebar {
                showSidebar = true
            }
        }
        .focusedSceneValue(
            \.agentChatDesktopActions,
            AgentChatDesktopActions(
                showNewThreadSheet: { showNewThreadSheet = true },
                showAddAgentsSheet: { showAddAgentsSheet = true },
                toggleSidebar: { toggleSidebar() },
                focusComposer: { scheduleComposerFocus() }
            )
        )
        .onReceive(NotificationCenter.default.publisher(for: .agentChatDesktopToggleSidebar)) { _ in
            toggleSidebar()
        }
    }

    @ViewBuilder
    private var detail: some View {
        let currentTab = DesktopTab(rawValue: selectedTab) ?? .chats

        if currentTab == .agents || currentTab == .settings {
            Color.clear
        } else if let thread = activeThreadSummary {
            ThreadDetailView(
                thread: thread,
                snapshot: activeThreadSnapshot,
                timeline: store.timeline,
                connectionState: store.connectionState,
                isLoadingThreadContent: store.desktopIsLoadingThreadContent(
                    for: thread.threadID,
                    snapshot: activeThreadSnapshot
                ),
                composerText: $composerText,
                selectedParticipantIDs: selectedParticipantIDs,
                isComposerFocused: $isComposerFocused,
                onSend: sendCurrentMessage
            )
        } else {
            ContentUnavailableView(
                "Select a Thread",
                systemImage: "message.badge.circle",
                description: Text("Open an existing thread or start a new conversation from the sidebar.")
            )
        }
    }

    private var detailPane: some View {
        detail
            .frame(minWidth: 720, maxWidth: .infinity, maxHeight: .infinity)
    }

    @ViewBuilder
    private var sidebarContent: some View {
        switch DesktopTab(rawValue: selectedTab) ?? .chats {
        case .chats:
            sidebar
                .frame(minWidth: 320, idealWidth: 340, maxWidth: 520)
        case .agents:
            agentsSidebarPanel
                .frame(minWidth: 280, idealWidth: 320, maxWidth: 480)
        case .settings:
            DesktopSettingsSidebarPanel()
                .frame(minWidth: 280, idealWidth: 320, maxWidth: 480)
        }
    }

    private var agentsSidebarPanel: some View {
        List {
            Section("Agents") {
                if store.desktopAgents.isEmpty {
                    ContentUnavailableView(
                        "No Agents",
                        systemImage: "person.2",
                        description: Text("Reconnect to your daemon to discover agents.")
                    )
                } else {
                    ForEach(store.desktopAgents) { agent in
                        AgentSidebarRow(
                            agent: agent,
                            onEdit: {
                                editingAgent = agent
                            },
                            onReconnect: {
                                store.connectToAgent(id: agent.agentID)
                            }
                        )
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
    }

    private var sidebar: some View {
        List(selection: $selectedThreadID) {
            Section("Pinned") {
                if filteredThreads.isEmpty {
                    ContentUnavailableView(
                        "No Threads",
                        systemImage: "ellipsis.message",
                        description: Text("Create a thread or reconnect to your daemon to load conversations.")
                    )
                    .overlay(alignment: .bottom) {
                        Button("New Thread") {
                            showNewThreadSheet = true
                        }
                        .buttonStyle(.borderedProminent)
                        .padding(.bottom, 10)
                    }
                    .listRowBackground(Color.clear)
                } else if pinnedThreads.isEmpty {
                    Text("Pin important threads to keep them at the top.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .listRowBackground(Color.clear)
                } else {
                    ForEach(pinnedThreads) { thread in
                        ThreadSidebarRow(
                            thread: thread,
                            title: store.effectiveThreadTitle(for: thread),
                            preview: store.threadPreview(for: thread.threadID),
                            isSelected: selectedThreadID == thread.threadID,
                            isActive: store.activeThreadID == thread.threadID,
                            isUnread: isThreadUnread(thread),
                            isPinned: store.isPinnedThread(thread.threadID),
                            onPin: {
                                store.togglePinnedThread(thread.threadID)
                            },
                            onHide: {
                                store.hideThread(thread.threadID)
                                if selectedThreadID == thread.threadID {
                                    selectedThreadID = nil
                                }
                            },
                            onClose: {
                                store.closeThread(thread.threadID)
                                if selectedThreadID == thread.threadID {
                                    selectedThreadID = nil
                                }
                            },
                            onOpenInNewWindow: {
                                openThreadInNewWindow(thread)
                            },
                            snapshot: threadSnapshot(for: thread),
                            participants: threadParticipants(for: thread),
                            onShowAddAgents: {
                                selectedThreadID = thread.threadID
                                if store.activeThreadID != thread.threadID {
                                    store.attachThread(thread.threadID)
                                }
                                showAddAgentsSheet = true
                            }
                        )
                        .tag(Optional(thread.threadID))
                    }
                }
            }

            if !recentThreads.isEmpty {
                Section("Recent") {
ForEach(recentThreads) { thread in
                        ThreadSidebarRow(
                            thread: thread,
                            title: store.effectiveThreadTitle(for: thread),
                            preview: store.threadPreview(for: thread.threadID),
                            isSelected: selectedThreadID == thread.threadID,
                            isActive: store.activeThreadID == thread.threadID,
                            isUnread: isThreadUnread(thread),
                            isPinned: store.isPinnedThread(thread.threadID),
                            onPin: {
                                store.togglePinnedThread(thread.threadID)
                            },
                            onHide: {
                                store.hideThread(thread.threadID)
                                if selectedThreadID == thread.threadID {
                                    selectedThreadID = nil
                                }
                            },
                            onClose: {
                                store.closeThread(thread.threadID)
                                if selectedThreadID == thread.threadID {
                                    selectedThreadID = nil
                                }
                            },
                            onOpenInNewWindow: {
                                openThreadInNewWindow(thread)
                            },
                            snapshot: threadSnapshot(for: thread),
                            participants: threadParticipants(for: thread),
                            onShowAddAgents: {
                                selectedThreadID = thread.threadID
                                if store.activeThreadID != thread.threadID {
                                    store.attachThread(thread.threadID)
                                }
                                showAddAgentsSheet = true
                            }
                        )
                        .tag(Optional(thread.threadID))
                    }
                }
            }
        }
    }

    private func synchronizeSelection() {
        if let selectedThreadID,
           store.desktopSortedThreads.contains(where: { $0.threadID == selectedThreadID }) {
            return
        }

        if let activeThreadID = store.activeThreadID,
           store.desktopSortedThreads.contains(where: { $0.threadID == activeThreadID }) {
            selectedThreadID = activeThreadID
            return
        }

        selectedThreadID = store.desktopSortedThreads.first?.threadID
        if let selectedThreadID {
            store.attachThread(selectedThreadID)
        }
    }

    private func sendCurrentMessage() {
        let sent = store.sendCurrentMessage(composerText)
        guard sent else {
            scheduleComposerFocus()
            return
        }

        if let threadID = activeThreadSummary?.threadID {
            draftsByThreadID.removeValue(forKey: threadID)
        }
        composerText = ""
        scheduleComposerFocus()
    }

    private func persistComposerDraft(for threadID: String?) {
        guard let threadID else { return }
        if composerText.isEmpty {
            draftsByThreadID.removeValue(forKey: threadID)
        } else {
            draftsByThreadID[threadID] = composerText
        }
    }

    private func restoreComposerDraft(for threadID: String?) {
        composerText = threadID.flatMap { draftsByThreadID[$0] } ?? ""
    }

    private func scheduleComposerFocus() {
        guard activeThreadSummary != nil else { return }
        DispatchQueue.main.async {
            isComposerFocused = true
        }
    }

    private func toggleSidebar() {
        withAnimation(.easeInOut(duration: 0.16)) {
            showSidebar.toggle()
        }
    }

    private func seedSeenThreadSequencesIfNeeded() {
        var current = loadSeenThreadSequences()
        if current.isEmpty {
            current = Dictionary(
                uniqueKeysWithValues: store.desktopSortedThreads.map { ($0.threadID, $0.lastThreadSeq) }
            )
            saveSeenThreadSequences(current)
            seenThreadSequences = current
            return
        }

        for thread in store.desktopSortedThreads where current[thread.threadID] == nil {
            current[thread.threadID] = 0
        }
        saveSeenThreadSequences(current)
        seenThreadSequences = current
    }

    private func markThreadAsSeen(threadID: String?) {
        guard let threadID,
              let summary = store.desktopSortedThreads.first(where: { $0.threadID == threadID })
        else {
            return
        }
        var current = seenThreadSequences
        current[threadID] = max(current[threadID] ?? 0, summary.lastThreadSeq)
        seenThreadSequences = current
        saveSeenThreadSequences(current)
    }

    private func markActiveThreadAsSeen() {
        if let snapshot = activeThreadSnapshot {
            var current = seenThreadSequences
            current[snapshot.threadID] = max(current[snapshot.threadID] ?? 0, snapshot.lastThreadSeq)
            seenThreadSequences = current
            saveSeenThreadSequences(current)
            return
        }

        markThreadAsSeen(threadID: activeThreadSummary?.threadID)
    }

    private func isThreadUnread(_ thread: DaemonThreadSummary) -> Bool {
        guard selectedThreadID != thread.threadID else { return false }
        return thread.lastThreadSeq > (seenThreadSequences[thread.threadID] ?? 0)
    }

    private func openThreadInNewWindow(_ thread: DaemonThreadSummary) {
        store.attachThread(thread.threadID)
        guard let url = AgentChatDesktopURL.threadLink(for: thread.threadID) else {
            store.errorMessage = "Couldn't create a desktop thread link for \(thread.threadID)."
            return
        }

        NSWorkspace.shared.open(url)
    }
}

private struct AgentChatDesktopSidebarShortcutBridge: ViewModifier {
    let toggleSidebar: () -> Void

    func body(content: Content) -> some View {
        content
            .background(ShortcutResponderViewRepresentable(toggleSidebar: toggleSidebar))
            .onReceive(NotificationCenter.default.publisher(for: .agentChatDesktopToggleSidebar)) { notification in
                if let sender = notification.object as? NSWindow, sender.isKeyWindow {
                    toggleSidebar()
                } else if notification.object == nil {
                    toggleSidebar()
                }
            }
    }
}

extension View {
    func agentChatDesktopSidebarShortcut(toggleSidebar: @escaping () -> Void) -> some View {
        modifier(AgentChatDesktopSidebarShortcutBridge(toggleSidebar: toggleSidebar))
    }
}

private struct ShortcutResponderViewRepresentable: NSViewRepresentable {
    let toggleSidebar: () -> Void

    func makeNSView(context: Context) -> SidebarShortcutResponderView {
        let view = SidebarShortcutResponderView(frame: .zero)
        view.toggleAction = toggleSidebar
        return view
    }

    func updateNSView(_ nsView: SidebarShortcutResponderView, context: Context) {
        nsView.toggleAction = toggleSidebar
    }
}

private final class SidebarShortcutResponderView: NSView {
    var toggleAction: (() -> Void)?

    @objc override func agentChatToggleSidebar(_ sender: Any?) {
        NotificationCenter.default.post(
            name: .agentChatDesktopToggleSidebar,
            object: NSApp.keyWindow
        )
    }
}
