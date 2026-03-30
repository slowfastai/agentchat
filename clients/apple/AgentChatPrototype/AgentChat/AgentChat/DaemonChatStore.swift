import Combine
import Foundation
import SwiftUI

@MainActor
final class DaemonChatStore: ObservableObject {
    private static let pinnedThreadsKey = "agentchat_pinned_thread_ids"
    private static let hiddenThreadsKey = "agentchat_hidden_thread_ids"
    private static let knownAgentsKey = "agentchat_known_agents"
    private static let selectedAgentsKey = "agentchat_selected_agent_ids"
    private static let persistedThreadStateKey = "agentchat_persisted_thread_state"
    private static let relayAppInstallationIDKey = "agentchat_relay_app_installation_id"
    private static let agentCustomNamesKey = "agentchat_agent_custom_names"
    private static let agentAvatarDataKey = "agentchat_agent_avatar_data"

    @Published var connectionStatus = "Not configured"
    @Published var agents: [DaemonAgentSummary] = []
    @Published var threads: [DaemonThreadSummary] = []
    @Published var activeThreadID: String?
    @Published var activeThreadSnapshot: DaemonThreadSnapshot?
    @Published var timeline: [DaemonTimelineEntry] = []
    @Published var selectedAgentIDs: Set<String> = []
    @Published var selectedParticipantIDs: Set<String> = []
    @Published var errorMessage: String?
    @Published var agentCustomNames: [String: String] = [:]
    @Published var agentAvatarData: [String: Data] = [:]
    @Published var connectingAgentIDs: Set<String> = []

    @AppStorage("agentchat_daemon_ws_url") private var daemonURLString = ""

    var daemonURL: String {
        daemonURLString
    }

    var hasConfiguredDaemonURL: Bool {
        !daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private let defaults: UserDefaults
    private var socketTask: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var connectionTask: Task<Void, Never>?
    private var relaySession: RelayAppSession?
    private var pendingThreadAgentIDs: [String] = []
    private var hasStarted = false
    private var allThreads: [DaemonThreadSummary] = []
    private var pinnedThreadIDs: Set<String>
    private var hiddenThreadIDs: Set<String>
    private var snapshotsByThread: [String: DaemonThreadSnapshot] = [:]
    private var timelineByThread: [String: [DaemonTimelineEntry]] = [:]
    private var cursorByThread: [String: UInt64] = [:]
    private var assistantTurns = AssistantTurnReducer()
    private var participantSelectionWasCustomized = false

    private struct PersistedThreadState: Codable {
        let allThreads: [DaemonThreadSummary]
        let snapshotsByThread: [String: DaemonThreadSnapshot]
        let timelineByThread: [String: [DaemonTimelineEntry]]
        let cursorByThread: [String: UInt64]
        let activeThreadID: String?
    }

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        self.pinnedThreadIDs = Set(defaults.stringArray(forKey: Self.pinnedThreadsKey) ?? [])
        self.hiddenThreadIDs = Set(defaults.stringArray(forKey: Self.hiddenThreadsKey) ?? [])
        self.agents = Self.loadKnownAgents(from: defaults)
        self.selectedAgentIDs = Set(defaults.stringArray(forKey: Self.selectedAgentsKey) ?? [])
        self.agentCustomNames = Self.loadAgentCustomNames(from: defaults)
        self.agentAvatarData = Self.loadAgentAvatarData(from: defaults)
        restorePersistedThreadState()
        refreshIdleConnectionStatus()
    }

    func start() {
        guard !hasStarted else { return }
        hasStarted = true
        refreshIdleConnectionStatus()
    }

    func createThread(withAgentIDs agentIDs: [String]) {
        let onlineAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
        let chosenAgentIDs = Array(Set(agentIDs).intersection(onlineAgentIDs)).sorted()
        guard !chosenAgentIDs.isEmpty else {
            errorMessage = "No online agents available. Reconnect to the daemon and try again."
            return
        }
        rememberSelectedAgents(chosenAgentIDs)
        pendingThreadAgentIDs = chosenAgentIDs.sorted()
        let title = chosenAgentIDs.isEmpty ? "New Chat" : chosenAgentIDs.joined(separator: " + ")
        Task {
            await send(CreateThreadRequest(title: title, workingDir: "."))
        }
    }

    func isPinnedThread(_ threadID: String) -> Bool {
        pinnedThreadIDs.contains(threadID)
    }

    func togglePinnedThread(_ threadID: String) {
        if pinnedThreadIDs.contains(threadID) {
            pinnedThreadIDs.remove(threadID)
        } else {
            pinnedThreadIDs.insert(threadID)
        }
        persistThreadPreferences()
        applyThreadPresentation()
    }

    func hideThread(_ threadID: String) {
        hiddenThreadIDs.insert(threadID)
        pinnedThreadIDs.remove(threadID)
        persistThreadPreferences()
        removeThreadFromLocalState(threadID)
    }

    func closeThread(_ threadID: String) {
        Task {
            await send(CloseThreadRequest(threadID: threadID))
        }
    }

    func addAgents(_ agentIDs: [String], toActiveThread threadID: String? = nil) {
        guard let threadID = threadID ?? activeThreadID else {
            errorMessage = "Open a thread first, then add agents."
            return
        }

        let existingAgentIDs = Set(activeThreadSnapshot?.participants.compactMap(\.agentID) ?? [])
        let onlineAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
        let chosenAgentIDs = Set(agentIDs).intersection(onlineAgentIDs)
        guard !chosenAgentIDs.isEmpty else {
            errorMessage = "No online agents available. Reconnect to the daemon and try again."
            return
        }
        rememberSelectedAgents(Array(chosenAgentIDs).sorted())
        let agentIDsToAdd = chosenAgentIDs.subtracting(existingAgentIDs).sorted()

        guard !agentIDsToAdd.isEmpty else {
            errorMessage = "No new selected agents to add."
            return
        }

        Task {
            for agentID in agentIDsToAdd {
                await send(AddThreadParticipantRequest(threadID: threadID, agentID: agentID))
            }
            await send(ListThreadsRequest())
            await send(AttachThreadRequest(threadID: threadID, afterSeq: nil))
        }
    }

    func attachThread(_ threadID: String) {
        setActiveThreadLocally(threadID)

        Task {
            let afterSeq = cursorByThread[threadID]
            await send(AttachThreadRequest(threadID: threadID, afterSeq: afterSeq))
        }
    }

    func sendCurrentMessage(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let threadID = activeThreadID else { return }
        let agentParticipants = activeThreadSnapshot?.participants.filter(\.isAgent) ?? []
        guard !agentParticipants.isEmpty else {
            errorMessage = "Add at least one agent to this thread before sending a message."
            return
        }

        let allAgentIDs = Set(agentParticipants.map(\.participantID))
        let targets = Set(selectedParticipantIDs).intersection(allAgentIDs)
        guard !targets.isEmpty else {
            selectedParticipantIDs = allAgentIDs
            let resetTargets = Set(selectedParticipantIDs).intersection(allAgentIDs)
            guard !resetTargets.isEmpty else {
                errorMessage = "Add at least one agent to this thread before sending a message."
                return
            }
            errorMessage = nil
            Task {
                await send(
                    SendThreadMessageRequest(
                        threadID: threadID,
                        content: trimmed,
                        targetParticipantIDs: nil
                    )
                )
            }
            return
        }
        let targetList: [String]? = targets == allAgentIDs ? nil : targets.sorted()
        errorMessage = nil

        Task {
            await send(
                SendThreadMessageRequest(
                    threadID: threadID,
                    content: trimmed,
                    targetParticipantIDs: targetList
                )
            )
        }
    }

    func rememberSelectedAgents(_ agentIDs: [String]) {
        selectedAgentIDs = Set(agentIDs)
        persistSelectedAgents()
    }

    func removeAgent(_ agentID: String) {
        agents.removeAll { $0.agentID == agentID }
        selectedAgentIDs.remove(agentID)
        persistKnownAgents()
        persistSelectedAgents()
    }

    func updateAgentDisplayName(_ agentID: String, displayName: String?) {
        guard let index = agents.firstIndex(where: { $0.agentID == agentID }) else { return }
        agents[index] = agents[index].withCustomDisplayName(displayName)
        persistKnownAgents()
    }

    func updateAgentAvatar(_ agentID: String, imageData: Data?) {
        guard let index = agents.firstIndex(where: { $0.agentID == agentID }) else { return }
        agents[index] = agents[index].withAvatarImageData(imageData)
        persistKnownAgents()
    }

    func isSelectedParticipant(_ participantID: String) -> Bool {
        selectedParticipantIDs.contains(participantID)
    }

    func reconnectNow() {
        connect()
    }

    func updateDaemonURL(_ newValue: String) {
        let trimmed = newValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        if let payload = parseScannedDaemonConnectionPayload(from: trimmed), !payload.agentIDs.isEmpty {
            selectedAgentIDs = Set(payload.agentIDs)
            persistSelectedAgents()
        }

        if trimmed == daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines), hasActiveConnection {
            errorMessage = nil
            return
        }
        daemonURLString = trimmed
        errorMessage = nil
        connect()
    }

    func disconnect() {
        connectionTask?.cancel()
        connectionTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        relaySession = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil
        markAgentsOffline()
        refreshIdleConnectionStatus()
    }

    func applyScannedConnectionPayload(_ payload: String) {
        let trimmed = payload.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            errorMessage = "Scanned QR code was empty."
            return
        }

        guard parseScannedDaemonConnectionPayload(from: trimmed) != nil else {
            errorMessage = "Unsupported QR payload. Encode ws://..., wss://..., agentchat://connect?url=<percent-encoded-websocket-url>&agents=<comma-separated-agent-ids>, or relay agentchat://connect?relay_url=<percent-encoded-relay-websocket-url>&pairing_ticket=<pairing-ticket>&relay_pairing=claim&relay_crypto=dev."
            return
        }

        updateDaemonURL(trimmed)
    }

    private func connect() {
        connectionTask?.cancel()
        connectionTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        relaySession = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil

        let trimmedConnection = daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnection.isEmpty else {
            refreshIdleConnectionStatus()
            errorMessage = "No daemon URL configured. Scan a QR code or enter a URL first."
            return
        }

        guard let connectionPayload = parseScannedDaemonConnectionPayload(from: trimmedConnection) else {
            connectionStatus = "Bad URL"
            errorMessage = "Invalid daemon URL or relay link: \(trimmedConnection)"
            return
        }

        connectionStatus = "Connecting…"
        connectionTask = Task { [weak self] in
            guard let self else { return }
            await self.openConnection(using: connectionPayload, rawValue: trimmedConnection)
        }
    }

    private func openConnection(using connectionPayload: ScannedDaemonConnectionPayload, rawValue: String) async {
        defer {
            if !Task.isCancelled {
                connectionTask = nil
            }
        }

        do {
            switch connectionPayload {
            case .direct(let urlString, _):
                guard let url = URL(string: urlString) else {
                    connectionStatus = "Bad URL"
                    errorMessage = "Invalid daemon URL: \(urlString)"
                    return
                }

                let task = URLSession.shared.webSocketTask(with: url)
                socketTask = task
                task.resume()
                try await bootstrapDirectConnection(using: task)
            case .relay(let relayPayload):
                connectionStatus = "Pairing with relay…"
                let resolvedRelay = try await relayPayload.resolve(
                    appInstallationID: relayAppInstallationID(),
                    appName: relayAppName()
                )
                guard daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines) == rawValue else {
                    return
                }

                var request = URLRequest(url: resolvedRelay.wsURL)
                request.setValue("Bearer \(resolvedRelay.relayToken)", forHTTPHeaderField: "Authorization")

                connectionStatus = "Connecting to relay…"
                let task = URLSession.shared.webSocketTask(with: request)
                socketTask = task
                task.resume()

                connectionStatus = "Securing relay channel…"
                relaySession = try await RelayAppSession.handshake(over: task, resolvedConnection: resolvedRelay)
                guard socketTask === task else { return }
                receiveTask = Task { [weak self] in
                    await self?.receiveLoop()
                }
                connectionStatus = "Online"
                connectingAgentIDs.removeAll()
                await refreshDaemonState()
            }
        } catch {
            guard !Task.isCancelled else { return }
            handleConnectionFailure(message: "Failed to connect to daemon: \(error.localizedDescription)")
        }
    }

    private func bootstrapDirectConnection(using task: URLSessionWebSocketTask) async throws {
        try await ping(task)
        guard socketTask === task else { return }
        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }
        connectionStatus = "Online"
        connectingAgentIDs.removeAll()
        await refreshDaemonState()
    }

    private func receiveLoop() async {
        guard let task = socketTask else { return }

        while !Task.isCancelled {
            do {
                let message = try await task.receive()
                switch message {
                case .string(let text):
                    if relaySession != nil {
                        handleRelay(text: text)
                    } else {
                        handle(text: text)
                    }
                case .data(let data):
                    if let text = String(data: data, encoding: .utf8) {
                        if relaySession != nil {
                            handleRelay(text: text)
                        } else {
                            handle(text: text)
                        }
                    }
                @unknown default:
                    break
                }
            } catch {
                guard socketTask === task else { break }
                relaySession = nil
                socketTask = nil
                receiveTask = nil
                markAgentsOffline()
                connectionStatus = "Offline"
                connectingAgentIDs.removeAll()
                break
            }
        }
    }

    private func handleRelay(text: String) {
        guard var relaySession else { return }

        do {
            let inbound = try relaySession.consumeIncomingFrame(text: text)
            self.relaySession = relaySession

            switch inbound {
            case .applicationJSON(let json):
                handle(text: json)
            case .relayError(let message):
                errorMessage = message
            case .ignored:
                break
            }
        } catch {
            self.relaySession = relaySession
            errorMessage = "Failed to decode relay frame: \(error.localizedDescription)"
        }
    }

    private func handle(text: String) {
        guard let data = text.data(using: .utf8) else { return }
        let decoder = JSONDecoder()

        do {
            let envelope = try decoder.decode(DaemonEnvelope.self, from: data)
            switch envelope.type {
            case "agent_list":
                upsertAgents(try decoder.decode(AgentListEvent.self, from: data).agents)
            case "thread_created":
                let event = try decoder.decode(ThreadCreatedEvent.self, from: data)
                let agentIDsToAdd = pendingThreadAgentIDs
                pendingThreadAgentIDs = []
                let summary = DaemonThreadSummary(
                    threadID: event.threadID,
                    title: agentIDsToAdd.isEmpty ? "New Chat" : agentIDsToAdd.joined(separator: " + "),
                    workingDir: ".",
                    createdAtMS: event.createdAtMS,
                    state: "idle",
                    participantCount: 1,
                    lastThreadSeq: 0
                )
                allThreads.removeAll { $0.threadID == event.threadID }
                allThreads.insert(summary, at: 0)
                applyThreadPresentation()
                activeThreadID = event.threadID
                activeThreadSnapshot = nil
                timelineByThread[event.threadID] = []
                timeline = []
                cursorByThread[event.threadID] = 0
                Task {
                    await send(AttachThreadRequest(threadID: event.threadID, afterSeq: nil))
                    for agentID in agentIDsToAdd {
                        await send(AddThreadParticipantRequest(threadID: event.threadID, agentID: agentID))
                    }
                    await send(ListThreadsRequest())
                    await send(AttachThreadRequest(threadID: event.threadID, afterSeq: nil))
                }
            case "thread_list":
                mergeThreadSummaries(try decoder.decode(ThreadListEvent.self, from: data).threads)
                applyThreadPresentation()
                if activeThreadID == nil, let firstThread = threads.first {
                    attachThread(firstThread.threadID)
                }
            case "thread_attached":
                let event = try decoder.decode(ThreadAttachedEvent.self, from: data)
                connectionStatus = "Attached to \(event.threadID)"
            case "thread_snapshot":
                let snapshot = try decoder.decode(ThreadSnapshotEvent.self, from: data).snapshot
                snapshotsByThread[snapshot.threadID] = snapshot
                cursorByThread[snapshot.threadID] = max(cursorByThread[snapshot.threadID] ?? 0, snapshot.lastThreadSeq)
                mergeThreadSummary(
                    DaemonThreadSummary(
                        threadID: snapshot.threadID,
                        title: snapshot.title,
                        workingDir: snapshot.workingDir,
                        createdAtMS: snapshot.createdAtMS,
                        state: "idle",
                        participantCount: snapshot.participants.count,
                        lastThreadSeq: snapshot.lastThreadSeq
                    )
                )
                applyThreadPresentation()
                if activeThreadID == snapshot.threadID {
                    activeThreadSnapshot = snapshot
                    timeline = timelineByThread[snapshot.threadID] ?? []
                    syncSelectedParticipants(with: snapshot)
                }
            case "thread_closed":
                let event = try decoder.decode(ThreadClosedEvent.self, from: data)
                pinnedThreadIDs.remove(event.threadID)
                hiddenThreadIDs.remove(event.threadID)
                persistThreadPreferences()
                removeThreadFromLocalState(event.threadID)
            case "thread_replay_complete":
                let event = try decoder.decode(ThreadReplayCompleteEvent.self, from: data)
                cursorByThread[event.threadID] = max(cursorByThread[event.threadID] ?? 0, event.lastThreadSeq)
                connectionStatus = "Online"
            case "thread_participant_added":
                let event = try decoder.decode(ThreadParticipantAddedEvent.self, from: data)
                upsertParticipant(event.participant, in: event.threadID)
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .system,
                        title: "Participant added",
                        body: "\(event.participant.displayName) joined the chat.",
                        tintName: "indigo"
                    ),
                    to: event.threadID
                )
                updateThreadSummary(threadID: event.threadID) { summary in
                    let participantCount = snapshotsByThread[event.threadID]?.participants.count ?? summary.participantCount
                    return DaemonThreadSummary(
                        threadID: summary.threadID,
                        title: summary.title,
                        workingDir: summary.workingDir,
                        createdAtMS: summary.createdAtMS,
                        state: summary.state,
                        participantCount: participantCount,
                        lastThreadSeq: max(summary.lastThreadSeq, event.threadSeq)
                    )
                }
            case "thread_participant_removed":
                let event = try decoder.decode(ThreadParticipantRemovedEvent.self, from: data)
                removeParticipant(event.participantID, from: event.threadID)
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .system,
                        title: "Participant removed",
                        body: "A participant left the chat.",
                        tintName: "red"
                    ),
                    to: event.threadID
                )
                updateThreadSummary(threadID: event.threadID) { summary in
                    let participantCount = snapshotsByThread[event.threadID]?.participants.count ?? summary.participantCount
                    return DaemonThreadSummary(
                        threadID: summary.threadID,
                        title: summary.title,
                        workingDir: summary.workingDir,
                        createdAtMS: summary.createdAtMS,
                        state: summary.state,
                        participantCount: participantCount,
                        lastThreadSeq: max(summary.lastThreadSeq, event.threadSeq)
                    )
                }
            case "thread_message":
                let event = try decoder.decode(ThreadMessageEvent.self, from: data)
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .user,
                        agentID: nil,
                        title: event.sender.displayName,
                        body: event.content,
                        tintName: "blue"
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_assistant_message":
                let event = try decoder.decode(ThreadAssistantMessageEvent.self, from: data)
                upsertAssistantMessage(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_delta":
                let event = try decoder.decode(ThreadAgentDeltaEvent.self, from: data)
                upsertAssistantDelta(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_tool_update":
                let event = try decoder.decode(ThreadAgentToolUpdateEvent.self, from: data)
                upsertAssistantToolUpdate(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_plan_update":
                let event = try decoder.decode(ThreadAgentPlanUpdateEvent.self, from: data)
                upsertAssistantPlanUpdate(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_turn_end":
                let event = try decoder.decode(ThreadAgentTurnEndEvent.self, from: data)
                finalizeAssistantTurn(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "error":
                let event = try decoder.decode(ErrorEvent.self, from: data)
                errorMessage = "\(event.code): \(event.message)"
            default:
                break
            }
        } catch {
            errorMessage = "Failed to decode daemon event: \(error.localizedDescription)"
        }
    }

    private func appendTimeline(_ entry: DaemonTimelineEntry, to threadID: String) {
        var entries = timelineByThread[threadID] ?? []
        entries.removeAll { $0.id == entry.id }
        entries.append(entry)
        entries.sort {
            if $0.sortThreadSeq == $1.sortThreadSeq {
                return $0.id < $1.id
            }
            return $0.sortThreadSeq < $1.sortThreadSeq
        }
        timelineByThread[threadID] = entries
        cursorByThread[threadID] = max(cursorByThread[threadID] ?? 0, entry.lastThreadSeq)
        if activeThreadID == threadID {
            timeline = entries
        }
        persistThreadState()
    }

    private func upsertAssistantMessage(_ event: ThreadAssistantMessageEvent) {
        let state = assistantTurns.consume(snapshot: event)
        appendTimeline(
            state.timelineEntry(tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func upsertAssistantDelta(_ event: ThreadAgentDeltaEvent) {
        guard let state = assistantTurns.consume(delta: event) else {
            return
        }
        appendTimeline(
            state.timelineEntry(tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func upsertAssistantToolUpdate(_ event: ThreadAgentToolUpdateEvent) {
        let state = assistantTurns.consume(toolUpdate: event)
        appendTimeline(
            state.timelineEntry(tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func upsertAssistantPlanUpdate(_ event: ThreadAgentPlanUpdateEvent) {
        let state = assistantTurns.consume(planUpdate: event)
        appendTimeline(
            state.timelineEntry(tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func finalizeAssistantTurn(_ event: ThreadAgentTurnEndEvent) {
        guard let state = assistantTurns.finish(turnEnd: event) else {
            return
        }
        appendTimeline(
            state.timelineEntry(tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func upsertParticipant(_ participant: DaemonThreadParticipant, in threadID: String) {
        guard var snapshot = snapshotsByThread[threadID] else { return }
        snapshot = DaemonThreadSnapshot(
            threadID: snapshot.threadID,
            title: snapshot.title,
            workingDir: snapshot.workingDir,
            createdAtMS: snapshot.createdAtMS,
            lastThreadSeq: snapshot.lastThreadSeq,
            participants: snapshot.participants.filter { $0.participantID != participant.participantID } + [participant]
        )
        snapshot = DaemonThreadSnapshot(
            threadID: snapshot.threadID,
            title: snapshot.title,
            workingDir: snapshot.workingDir,
            createdAtMS: snapshot.createdAtMS,
            lastThreadSeq: snapshot.lastThreadSeq,
            participants: snapshot.participants.sorted { $0.displayName < $1.displayName }
        )
        snapshotsByThread[threadID] = snapshot
        if activeThreadID == threadID {
            activeThreadSnapshot = snapshot
            syncSelectedParticipants(with: snapshot)
        }
        persistThreadState()
    }

    private func removeParticipant(_ participantID: String, from threadID: String) {
        guard let snapshot = snapshotsByThread[threadID] else { return }
        let updated = DaemonThreadSnapshot(
            threadID: snapshot.threadID,
            title: snapshot.title,
            workingDir: snapshot.workingDir,
            createdAtMS: snapshot.createdAtMS,
            lastThreadSeq: snapshot.lastThreadSeq,
            participants: snapshot.participants.filter { $0.participantID != participantID }
        )
        snapshotsByThread[threadID] = updated
        if activeThreadID == threadID {
            activeThreadSnapshot = updated
            syncSelectedParticipants(with: updated)
        }
        persistThreadState()
    }

    private func syncSelectedParticipants(with snapshot: DaemonThreadSnapshot) {
        let allParticipants = Set(snapshot.participants.filter(\.isAgent).map(\.participantID))
        if participantSelectionWasCustomized {
            selectedParticipantIDs = selectedParticipantIDs.intersection(allParticipants)
        } else {
            selectedParticipantIDs = allParticipants
        }
    }

    private func touchThread(threadID: String, lastThreadSeq: UInt64) {
        updateThreadSummary(threadID: threadID) { summary in
            DaemonThreadSummary(
                threadID: summary.threadID,
                title: summary.title,
                workingDir: summary.workingDir,
                createdAtMS: summary.createdAtMS,
                state: summary.state,
                participantCount: summary.participantCount,
                lastThreadSeq: max(summary.lastThreadSeq, lastThreadSeq)
            )
        }
    }

    private func updateThreadSummary(threadID: String, transform: (DaemonThreadSummary) -> DaemonThreadSummary) {
        if let index = allThreads.firstIndex(where: { $0.threadID == threadID }) {
            allThreads[index] = transform(allThreads[index])
            applyThreadPresentation()
        }
    }

    private func removeThreadFromLocalState(_ threadID: String) {
        allThreads.removeAll { $0.threadID == threadID }
        snapshotsByThread.removeValue(forKey: threadID)
        timelineByThread.removeValue(forKey: threadID)
        cursorByThread.removeValue(forKey: threadID)
        assistantTurns.removeStates(for: threadID)

        if activeThreadID == threadID {
            activeThreadID = nil
            activeThreadSnapshot = nil
            timeline = []
            selectedParticipantIDs = []
            participantSelectionWasCustomized = false
        }

        applyThreadPresentation()

        if activeThreadID == nil, let firstThread = threads.first {
            attachThread(firstThread.threadID)
        }
        persistThreadState()
    }

    private func applyThreadPresentation() {
        threads = allThreads
            .filter { !hiddenThreadIDs.contains($0.threadID) }
            .sorted { lhs, rhs in
                let lhsPinned = pinnedThreadIDs.contains(lhs.threadID)
                let rhsPinned = pinnedThreadIDs.contains(rhs.threadID)
                if lhsPinned != rhsPinned {
                    return lhsPinned && !rhsPinned
                }
                return lhs.createdAtMS > rhs.createdAtMS
            }

        if let activeThreadID, !threads.contains(where: { $0.threadID == activeThreadID }) {
            setActiveThreadLocally(threads.first?.threadID)
        }

        persistThreadState()
    }

    private func persistThreadPreferences() {
        defaults.set(Array(pinnedThreadIDs).sorted(), forKey: Self.pinnedThreadsKey)
        defaults.set(Array(hiddenThreadIDs).sorted(), forKey: Self.hiddenThreadsKey)
    }

    private func persistSelectedAgents() {
        defaults.set(Array(selectedAgentIDs).sorted(), forKey: Self.selectedAgentsKey)
    }

    func tintName(for agentID: String?) -> String {
        guard let agentID else { return "indigo" }
        if let summary = agents.first(where: { $0.agentID == agentID }) {
            return summary.tintName
        }
        return DaemonAgentFamily(agentID: agentID, kind: nil, name: nil).tintName
    }

    func agentDisplayName(for agentID: String) -> String {
        if let summary = agents.first(where: { $0.agentID == agentID }) {
            return summary.displayName
        }
        return humanizeAgentIdentifier(agentID)
    }

    private func refreshDaemonState() async {
        await send(ListAgentsRequest())
        await send(ListThreadsRequest())
        if let activeThreadID {
            await send(AttachThreadRequest(threadID: activeThreadID, afterSeq: cursorByThread[activeThreadID]))
        }
    }

    private func ping(_ task: URLSessionWebSocketTask) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            task.sendPing { error in
                if let error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume(returning: ())
                }
            }
        }
    }

    private func relayAppInstallationID() -> String {
        if let existing = defaults.string(forKey: Self.relayAppInstallationIDKey),
           !existing.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return existing
        }

        let created = UUID().uuidString.lowercased()
        defaults.set(created, forKey: Self.relayAppInstallationIDKey)
        return created
    }

    private func relayAppName() -> String {
        let appName = (Bundle.main.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String)
            ?? (Bundle.main.object(forInfoDictionaryKey: "CFBundleName") as? String)
            ?? "AgentChat"
        return "\(appName) iPhone"
    }

    private func markAgentsOffline() {
        guard !agents.isEmpty else { return }
        agents = AgentRoster.markOffline(agents)
        persistKnownAgents()
    }

    private func handleConnectionFailure(message: String) {
        errorMessage = message
        connectionTask?.cancel()
        connectionTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        relaySession = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil
        markAgentsOffline()
        connectionStatus = "Offline"
        connectingAgentIDs.removeAll()
    }

    private var hasActiveConnection: Bool {
        guard socketTask != nil else { return false }
        switch connectionStatus {
        case "Offline", "Not configured", "Bad URL":
            return false
        default:
            return true
        }
    }

    private func refreshIdleConnectionStatus() {
        connectionStatus = hasConfiguredDaemonURL ? "Offline" : "Not configured"
    }

    private func upsertAgents(_ incomingAgents: [DaemonAgentSummary]) {
        agents = AgentRoster.merge(knownAgents: agents, incomingAgents: incomingAgents)
        persistKnownAgents()

        if selectedAgentIDs.isEmpty {
            selectedAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
            persistSelectedAgents()
        }
    }

    private func persistKnownAgents() {
        guard let data = try? JSONEncoder().encode(agents) else { return }
        defaults.set(data, forKey: Self.knownAgentsKey)
    }

    private static func loadKnownAgents(from defaults: UserDefaults) -> [DaemonAgentSummary] {
        guard let data = defaults.data(forKey: Self.knownAgentsKey),
              let agents = try? JSONDecoder().decode([DaemonAgentSummary].self, from: data)
        else {
            return []
        }

        // Persist the roster across launches, but never trust the last saved liveness.
        return AgentRoster.markOffline(agents)
    }

    private static func loadAgentCustomNames(from defaults: UserDefaults) -> [String: String] {
        guard let data = defaults.data(forKey: Self.agentCustomNamesKey),
              let names = try? JSONDecoder().decode([String: String].self, from: data)
        else {
            return [:]
        }
        return names
    }

    private static func loadAgentAvatarData(from defaults: UserDefaults) -> [String: Data] {
        guard let data = defaults.data(forKey: Self.agentAvatarDataKey),
              let avatars = try? JSONDecoder().decode([String: Data].self, from: data)
        else {
            return [:]
        }
        return avatars
    }

    private func persistAgentCustomNames() {
        guard let data = try? JSONEncoder().encode(agentCustomNames) else { return }
        defaults.set(data, forKey: Self.agentCustomNamesKey)
    }

    private func persistAgentAvatarData() {
        guard let data = try? JSONEncoder().encode(agentAvatarData) else { return }
        defaults.set(data, forKey: Self.agentAvatarDataKey)
    }

    func updateAgent(id agentID: String, name: String?, avatarData: Data?) {
        if let name = name {
            agentCustomNames[agentID] = name
        } else {
            agentCustomNames.removeValue(forKey: agentID)
        }
        persistAgentCustomNames()

        if let avatarData = avatarData {
            agentAvatarData[agentID] = avatarData
        } else {
            agentAvatarData.removeValue(forKey: agentID)
        }
        persistAgentAvatarData()
    }

    func removeAgent(id agentID: String) {
        agentCustomNames.removeValue(forKey: agentID)
        agentAvatarData.removeValue(forKey: agentID)
        persistAgentCustomNames()
        persistAgentAvatarData()

        agents.removeAll { $0.agentID == agentID }
        selectedAgentIDs.remove(agentID)
        persistKnownAgents()
        persistSelectedAgents()
    }

    func connectToAgent(id agentID: String) {
        guard !connectingAgentIDs.contains(agentID) else { return }
        connectingAgentIDs.insert(agentID)
        reconnectNow()
    }

    func customName(for agentID: String) -> String? {
        agentCustomNames[agentID]
    }

    func avatarData(for agentID: String) -> Data? {
        agentAvatarData[agentID]
    }

    func isConnecting(agentID: String) -> Bool {
        connectingAgentIDs.contains(agentID)
    }

    private func send<Request: Encodable>(_ request: Request) async {
        guard let socketTask else {
            markAgentsOffline()
            refreshIdleConnectionStatus()
            errorMessage = hasConfiguredDaemonURL
                ? "Not connected to a daemon. Tap Reconnect, scan a QR code, or enter a URL first."
                : "No daemon URL configured. Scan a QR code or enter a URL first."
            return
        }
        do {
            let data = try JSONEncoder().encode(request)
            guard let text = String(data: data, encoding: .utf8) else { return }
            let outboundText: String
            if var relaySession {
                outboundText = try relaySession.encryptJSONString(text)
                self.relaySession = relaySession
            } else {
                outboundText = text
            }
            try await socketTask.send(.string(outboundText))
        } catch {
            handleConnectionFailure(message: "Send failed: \(error.localizedDescription)")
        }
    }

    private func restorePersistedThreadState() {
        guard let data = defaults.data(forKey: Self.persistedThreadStateKey),
              let state = try? JSONDecoder().decode(PersistedThreadState.self, from: data)
        else {
            return
        }

        allThreads = state.allThreads
        snapshotsByThread = state.snapshotsByThread
        timelineByThread = state.timelineByThread
        cursorByThread = state.cursorByThread
        assistantTurns.restore(from: timelineByThread)
        applyThreadPresentation()

        let candidateThreadID = state.activeThreadID.flatMap { threadID in
            threads.contains(where: { $0.threadID == threadID }) ? threadID : nil
        } ?? threads.first?.threadID
        setActiveThreadLocally(candidateThreadID)
    }

    private func persistThreadState() {
        let state = PersistedThreadState(
            allThreads: allThreads,
            snapshotsByThread: snapshotsByThread,
            timelineByThread: timelineByThread,
            cursorByThread: cursorByThread,
            activeThreadID: activeThreadID
        )

        guard let data = try? JSONEncoder().encode(state) else { return }
        defaults.set(data, forKey: Self.persistedThreadStateKey)
    }

    private func setActiveThreadLocally(_ threadID: String?) {
        activeThreadID = threadID
        activeThreadSnapshot = threadID.flatMap { snapshotsByThread[$0] }
        timeline = threadID.flatMap { timelineByThread[$0] } ?? []
        participantSelectionWasCustomized = false
        selectedParticipantIDs = Set(
            activeThreadSnapshot?.participants.filter(\.isAgent).map(\.participantID) ?? []
        )
        persistThreadState()
    }

    private func mergeThreadSummaries(_ incoming: [DaemonThreadSummary]) {
        for summary in incoming {
            mergeThreadSummary(summary)
        }
    }

    private func mergeThreadSummary(_ incoming: DaemonThreadSummary) {
        if let index = allThreads.firstIndex(where: { $0.threadID == incoming.threadID }) {
            let existing = allThreads[index]
            allThreads[index] = DaemonThreadSummary(
                threadID: incoming.threadID,
                title: incoming.title ?? existing.title,
                workingDir: incoming.workingDir,
                createdAtMS: incoming.createdAtMS,
                state: incoming.state,
                participantCount: incoming.participantCount,
                lastThreadSeq: max(existing.lastThreadSeq, incoming.lastThreadSeq)
            )
        } else {
            allThreads.append(incoming)
        }
    }
}

enum AgentRoster {
    nonisolated static func merge(
        knownAgents: [DaemonAgentSummary],
        incomingAgents: [DaemonAgentSummary]
    ) -> [DaemonAgentSummary] {
        var mergedByID = Dictionary(uniqueKeysWithValues: knownAgents.map { ($0.agentID, $0.withStatus("offline")) })

        for agent in incomingAgents {
            let existing = mergedByID[agent.agentID]
            mergedByID[agent.agentID] = agent.applyingLocalCustomizations(from: existing)
        }

        return sorted(Array(mergedByID.values))
    }

    nonisolated static func markOffline(_ agents: [DaemonAgentSummary]) -> [DaemonAgentSummary] {
        sorted(agents.map { $0.withStatus("offline") })
    }

    nonisolated static func sorted(_ agents: [DaemonAgentSummary]) -> [DaemonAgentSummary] {
        agents.sorted(by: areInIncreasingOrder)
    }

    nonisolated private static func areInIncreasingOrder(_ lhs: DaemonAgentSummary, _ rhs: DaemonAgentSummary) -> Bool {
        let lhsRank = statusRank(for: lhs.status)
        let rhsRank = statusRank(for: rhs.status)
        if lhsRank != rhsRank {
            return lhsRank < rhsRank
        }

        let nameOrder = lhs.name.localizedCaseInsensitiveCompare(rhs.name)
        if nameOrder != .orderedSame {
            return nameOrder == .orderedAscending
        }

        return lhs.agentID.localizedCaseInsensitiveCompare(rhs.agentID) == .orderedAscending
    }

    nonisolated private static func statusRank(for status: String) -> Int {
        switch status {
        case "online":
            return 0
        case "offline":
            return 2
        default:
            return 1
        }
    }
}
