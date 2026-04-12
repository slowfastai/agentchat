import SwiftUI

struct AgentChatDesktopRootView: View {
    @EnvironmentObject private var store: DaemonChatStore

    @State private var selectedThreadID: String?
    @State private var sidebarSearchText = ""
    @State private var composerText = ""
    @State private var draftsByThreadID: [String: String] = [:]
    @State private var showInspector = true
    @State private var showNewThreadSheet = false
    @State private var showAddAgentsSheet = false
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
        NavigationSplitView {
            sidebar
        } detail: {
            detail
                .inspector(isPresented: $showInspector) {
                    inspector
                        .inspectorColumnWidth(min: 270, ideal: 320, max: 420)
                }
        }
        .navigationSplitViewColumnWidth(min: 280, ideal: 320, max: 380)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button {
                    showNewThreadSheet = true
                } label: {
                    Label("New Thread", systemImage: "square.and.pencil")
                }

                Button {
                    store.reconnectNow()
                } label: {
                    Label("Reconnect", systemImage: "arrow.clockwise")
                }
                .disabled(!store.hasConfiguredDaemonURL)

                Button {
                    showInspector.toggle()
                } label: {
                    Label(showInspector ? "Hide Inspector" : "Show Inspector", systemImage: "sidebar.right")
                }
            }

            ToolbarItem(placement: .automatic) {
                SettingsLink {
                    Label("Settings", systemImage: "gearshape")
                }
            }
        }
        .background(
            LinearGradient(
                colors: [
                    Color(red: 0.95, green: 0.95, blue: 0.93),
                    Color(red: 0.90, green: 0.90, blue: 0.87),
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
        .onAppear {
            synchronizeSelection()
            restoreComposerDraft(for: selectedThreadID ?? store.activeThreadID)
            scheduleComposerFocus()
        }
        .onChange(of: store.activeThreadID) { _, newValue in
            if selectedThreadID == nil || selectedThreadID != newValue {
                selectedThreadID = newValue
            }
        }
        .onChange(of: store.desktopSortedThreads.map(\.threadID)) { _, _ in
            synchronizeSelection()
        }
        .onChange(of: selectedThreadID) { oldValue, newValue in
            persistComposerDraft(for: oldValue)
            restoreComposerDraft(for: newValue)

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
            scheduleComposerFocus()
        }
        .focusedSceneValue(
            \.agentChatDesktopActions,
            AgentChatDesktopActions(
                showNewThreadSheet: { showNewThreadSheet = true },
                showAddAgentsSheet: { showAddAgentsSheet = true },
                toggleInspector: { showInspector.toggle() },
                focusComposer: { scheduleComposerFocus() }
            )
        )
    }

    private var sidebar: some View {
        List(selection: $selectedThreadID) {
            Section {
                ConnectionStatusCard()
                    .listRowInsets(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                    .listRowBackground(Color.clear)
            }

            Section("Threads") {
                if filteredThreads.isEmpty {
                    ContentUnavailableView(
                        "No Threads",
                        systemImage: "ellipsis.message",
                        description: Text("Create a thread or reconnect to your daemon to load conversations.")
                    )
                    .listRowBackground(Color.clear)
                } else {
                    ForEach(filteredThreads) { thread in
                        ThreadSidebarRow(
                            thread: thread,
                            title: store.effectiveThreadTitle(for: thread),
                            isSelected: selectedThreadID == thread.threadID,
                            isActive: store.activeThreadID == thread.threadID
                        )
                        .tag(Optional(thread.threadID))
                    }
                }
            }

            Section("Online Agents") {
                if store.desktopOnlineAgents.isEmpty {
                    Text("Reconnect to load available agents.")
                        .foregroundStyle(.secondary)
                        .listRowBackground(Color.clear)
                } else {
                    ForEach(store.desktopOnlineAgents) { agent in
                        AgentSidebarRow(agent: agent) {
                            store.connectToAgent(id: agent.agentID)
                        }
                        .listRowInsets(EdgeInsets(top: 6, leading: 12, bottom: 6, trailing: 12))
                    }
                }
            }
        }
        .scrollContentBackground(.hidden)
        .searchable(text: $sidebarSearchText, prompt: "Search threads")
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
            ContentUnavailableView(
                "Select a Thread",
                systemImage: "message.badge.circle",
                description: Text("Open an existing thread or start a new conversation from the toolbar.")
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
}
