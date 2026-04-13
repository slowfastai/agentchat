import SwiftUI
import AppKit

struct AgentChatDesktopRootView: View {
    @EnvironmentObject private var store: DaemonChatStore
    @SceneStorage("agentchat.desktop.showSidebar") private var showSidebar = true

    @State private var selectedThreadID: String?
    @State private var sidebarSearchText = ""
    @State private var composerText = ""
    @State private var draftsByThreadID: [String: String] = [:]
    @State private var showInspector = false
    @State private var showNewThreadSheet = false
    @State private var showAddAgentsSheet = false
    @State private var showAgentEditSheet = false
    @State private var editingAgent: DaemonAgentSummary?
    @State private var showCommandPalette = false
    @State private var showQuickConnect = false
    @State private var seenThreadSequences: [String: UInt64] = [:]
    @FocusState private var isComposerFocused: Bool

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

    private var selectedParticipantIDs: Binding<Set<String>> {
        Binding(
            get: { store.desktopSuggestedParticipantIDs(for: activeThreadSnapshot) },
            set: { store.desktopUpdateSelectedParticipantIDs($0, for: activeThreadSnapshot) }
        )
    }

    var body: some View {
        HSplitView {
            if showSidebar {
                sidebar
                    .frame(minWidth: 320, idealWidth: 340, maxWidth: 520)

                detailPane
            } else {
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

            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    showNewThreadSheet = true
                } label: {
                    Label("New Thread", systemImage: "square.and.pencil")
                }
                .buttonStyle(.borderedProminent)

                Menu {
                    Button {
                        store.reconnectNow()
                    } label: {
                        Label("Reconnect", systemImage: "arrow.clockwise")
                    }
                    .disabled(!store.hasConfiguredDaemonURL)

                    Button {
                        showQuickConnect = true
                    } label: {
                        Label("Connect", systemImage: "link")
                    }

                    Divider()

                    Button {
                        showCommandPalette = true
                    } label: {
                        Label("Command Palette", systemImage: "command")
                    }
                } label: {
                    Label("Workspace", systemImage: "slider.horizontal.3")
                }

                if activeThreadSummary != nil {
                    Button {
                        showInspector.toggle()
                    } label: {
                        Label(showInspector ? "Hide Inspector" : "Show Inspector", systemImage: "sidebar.right")
                    }
                }
            }

            ToolbarItem(placement: .automatic) {
                SettingsLink {
                    Label("Settings", systemImage: "gearshape")
                }
            }

            ToolbarItem(placement: .keyboard) {
                Button("Command Palette") {
                    showCommandPalette = true
                }
                .keyboardShortcut("k", modifiers: [.command])
            }
        }
        .background(AgentChatDesktopSidebarShortcutBridge())
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
            ) { selectedAgentIDs in
                store.createThread(withAgentIDs: selectedAgentIDs)
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
            ) { selectedAgentIDs in
                store.addAgents(selectedAgentIDs, toActiveThread: activeThreadSummary?.threadID)
            }
            .environmentObject(store)
        }
        .sheet(isPresented: $showAgentEditSheet) {
            if let agent = editingAgent {
                AgentEditSheet(
                    agent: agent,
                    initialSettings: store.agentSettings[agent.agentID] ?? AgentLocalSettings()
                ) { name, avatarData, settings in
                    store.updateAgentDisplayName(agent.agentID, displayName: name)
                    store.updateAgentAvatar(agent.agentID, imageData: avatarData)
                    store.updateAgentSettings(agent.agentID, settings: settings)
                }
            }
        }
        .overlay {
            if showCommandPalette {
                CommandPaletteView(
                    isPresented: $showCommandPalette,
                    showNewThreadSheet: { showNewThreadSheet = true },
                    showAddAgentsSheet: { showAddAgentsSheet = true },
                    toggleInspector: { showInspector.toggle() },
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
        .focusedSceneValue(
            \.agentChatDesktopActions,
            AgentChatDesktopActions(
                showNewThreadSheet: { showNewThreadSheet = true },
                showAddAgentsSheet: { showAddAgentsSheet = true },
                toggleSidebar: { toggleSidebar() },
                toggleInspector: { showInspector.toggle() },
                focusComposer: { scheduleComposerFocus() }
            )
        )
        .onReceive(NotificationCenter.default.publisher(for: .agentChatDesktopToggleSidebar)) { _ in
            toggleSidebar()
        }
    }

    private var detailPane: some View {
        detail
            .frame(minWidth: 720, maxWidth: .infinity, maxHeight: .infinity)
            .inspector(isPresented: $showInspector) {
                inspector
                    .inspectorColumnWidth(min: 270, ideal: 320, max: 420)
            }
    }

    private var sidebar: some View {
        List(selection: $selectedThreadID) {
            Section {
                SidebarOverviewCard(
                    threadCount: store.desktopSortedThreads.count,
                    onlineAgentCount: store.desktopOnlineAgents.count,
                    isOnline: store.connectionState.isOnline
                )
                .listRowInsets(EdgeInsets(top: 12, leading: 12, bottom: 6, trailing: 12))
                .listRowBackground(Color.clear)

                ConnectionStatusCard(showQuickConnect: $showQuickConnect)
                    .listRowInsets(EdgeInsets(top: 6, leading: 12, bottom: 10, trailing: 12))
                    .listRowBackground(Color.clear)
            }

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
                            }
                        )
                        .tag(Optional(thread.threadID))
                    }
                }
            }

            Section {
                OnlineAgentsPanel(
                    agents: store.desktopOnlineAgents,
                    onConnect: { agent in
                        store.connectToAgent(id: agent.agentID)
                    },
                    onEdit: { agent in
                        editingAgent = agent
                        showAgentEditSheet = true
                    }
                )
                .listRowInsets(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                .listRowBackground(Color.clear)
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .searchable(text: $sidebarSearchText, prompt: "Search threads or paths")
    }

    @ViewBuilder
    private var detail: some View {
        if let thread = activeThreadSummary {
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
            WorkspaceStartView(
                connectionState: store.connectionState,
                hasConfiguredDaemonURL: store.hasConfiguredDaemonURL,
                onlineAgentCount: store.desktopOnlineAgents.count,
                threadCount: store.desktopSortedThreads.count,
                onNewThread: { showNewThreadSheet = true },
                onReconnect: { store.reconnectNow() },
                onConnect: { showQuickConnect = true }
            )
        }
    }

    private var inspector: some View {
        ThreadInspectorView(
            thread: activeThreadSummary,
            snapshot: activeThreadSnapshot,
            participants: activeThreadSnapshot?.participants ?? [],
            selectedParticipantIDs: store.desktopSuggestedParticipantIDs(for: activeThreadSnapshot),
            showAddAgentsSheet: $showAddAgentsSheet
        )
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
        if seenThreadSequences.isEmpty {
            seenThreadSequences = Dictionary(
                uniqueKeysWithValues: store.desktopSortedThreads.map { ($0.threadID, $0.lastThreadSeq) }
            )
            return
        }

        for thread in store.desktopSortedThreads where seenThreadSequences[thread.threadID] == nil {
            seenThreadSequences[thread.threadID] = 0
        }
    }

    private func markThreadAsSeen(threadID: String?) {
        guard let threadID,
              let summary = store.desktopSortedThreads.first(where: { $0.threadID == threadID })
        else {
            return
        }
        seenThreadSequences[threadID] = max(seenThreadSequences[threadID] ?? 0, summary.lastThreadSeq)
    }

    private func markActiveThreadAsSeen() {
        if let snapshot = activeThreadSnapshot {
            seenThreadSequences[snapshot.threadID] = max(seenThreadSequences[snapshot.threadID] ?? 0, snapshot.lastThreadSeq)
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

private struct AgentChatDesktopSidebarShortcutBridge: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        SidebarShortcutResponderView(frame: .zero)
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}

private final class SidebarShortcutResponderView: NSView {
    @objc override func agentChatToggleSidebar(_ sender: Any?) {
        NotificationCenter.default.post(name: .agentChatDesktopToggleSidebar, object: nil)
    }
}
