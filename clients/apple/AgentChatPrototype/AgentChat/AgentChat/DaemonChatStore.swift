import Combine
import Foundation
import SwiftUI

@MainActor
final class DaemonChatStore: ObservableObject {
    @Published var connectionStatus = "Not configured"
    @Published var agents: [DaemonAgentSummary] = []
    @Published var threads: [DaemonThreadSummary] = []
    @Published var activeThreadID: String?
    @Published var activeThreadSnapshot: DaemonThreadSnapshot?
    @Published var timeline: [DaemonTimelineEntry] = []
    @Published var selectedAgentIDs: Set<String> = []
    @Published var selectedParticipantIDs: Set<String> = []
    @Published var errorMessage: String?

    @AppStorage("agentchat_daemon_ws_url") private var daemonURLString = ""

    var daemonURL: String {
        daemonURLString
    }

    var hasConfiguredDaemonURL: Bool {
        !daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private let pinnedThreadsKey = "agentchat_pinned_thread_ids"
    private let hiddenThreadsKey = "agentchat_hidden_thread_ids"

    private var socketTask: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var pendingThreadAgentIDs: [String] = []
    private var hasStarted = false
    private var allThreads: [DaemonThreadSummary] = []
    private var pinnedThreadIDs: Set<String>
    private var hiddenThreadIDs: Set<String>
    private var snapshotsByThread: [String: DaemonThreadSnapshot] = [:]
    private var timelineByThread: [String: [DaemonTimelineEntry]] = [:]
    private var cursorByThread: [String: UInt64] = [:]
    private var legacyAssistantMessages = LegacyAssistantMessageReducer()

    init() {
        let defaults = UserDefaults.standard
        self.pinnedThreadIDs = Set(defaults.stringArray(forKey: pinnedThreadsKey) ?? [])
        self.hiddenThreadIDs = Set(defaults.stringArray(forKey: hiddenThreadsKey) ?? [])
        refreshIdleConnectionStatus()
    }

    func start() {
        guard !hasStarted else { return }
        hasStarted = true
        refreshIdleConnectionStatus()
    }

    func createThreadWithSelectedAgents() {
        let onlineAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
        let chosenAgentIDs = Array(
            selectedAgentIDs.isEmpty
                ? onlineAgentIDs
                : selectedAgentIDs.intersection(onlineAgentIDs)
        )
        guard !chosenAgentIDs.isEmpty else {
            errorMessage = "No online agents available. Reconnect to the daemon and try again."
            return
        }
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

    func addSelectedAgentsToActiveThread() {
        guard let threadID = activeThreadID else {
            errorMessage = "Open a thread first, then add agents."
            return
        }

        let existingAgentIDs = Set(activeThreadSnapshot?.participants.compactMap(\.agentID) ?? [])
        let onlineAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
        let selectedOrAllAgents = selectedAgentIDs.isEmpty
            ? onlineAgentIDs
            : selectedAgentIDs.intersection(onlineAgentIDs)
        guard !selectedOrAllAgents.isEmpty else {
            errorMessage = "No online agents available. Reconnect to the daemon and try again."
            return
        }
        let agentIDsToAdd = selectedOrAllAgents.subtracting(existingAgentIDs).sorted()

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
        activeThreadID = threadID
        activeThreadSnapshot = snapshotsByThread[threadID]
        timeline = timelineByThread[threadID] ?? []
        selectedParticipantIDs = Set(
            activeThreadSnapshot?.participants.filter(\.isAgent).map(\.participantID) ?? []
        )

        Task {
            let afterSeq = cursorByThread[threadID]
            await send(AttachThreadRequest(threadID: threadID, afterSeq: afterSeq))
        }
    }

    func sendCurrentMessage(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let threadID = activeThreadID else { return }
        let agentParticipants = activeThreadSnapshot?.participants.filter(\.isAgent) ?? []
        let allAgentIDs = Set(agentParticipants.map(\.participantID))
        let targets = Set(selectedParticipantIDs)
        let targetList: [String]?
        if targets.isEmpty || targets == allAgentIDs {
            targetList = nil
        } else {
            targetList = targets.sorted()
        }

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

    func toggleAgentSelection(_ agentID: String) {
        if selectedAgentIDs.contains(agentID) {
            selectedAgentIDs.remove(agentID)
        } else {
            selectedAgentIDs.insert(agentID)
        }
    }

    func toggleParticipantSelection(_ participantID: String) {
        if selectedParticipantIDs.contains(participantID) {
            selectedParticipantIDs.remove(participantID)
        } else {
            selectedParticipantIDs.insert(participantID)
        }
    }

    func isSelectedAgent(_ agentID: String) -> Bool {
        selectedAgentIDs.contains(agentID)
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
        if trimmed == daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines), hasActiveConnection {
            errorMessage = nil
            return
        }
        daemonURLString = trimmed
        errorMessage = nil
        connect()
    }

    func disconnect() {
        receiveTask?.cancel()
        receiveTask = nil
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

        if let url = normalizedDaemonURL(from: trimmed) {
            updateDaemonURL(url)
        } else {
            errorMessage = "Unsupported QR payload. Encode either a ws:// or wss:// URL, or agentchat://connect?url=<percent-encoded websocket-url>."
        }
    }

    private func connect() {
        receiveTask?.cancel()
        receiveTask = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil

        let trimmedURL = daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedURL.isEmpty else {
            refreshIdleConnectionStatus()
            errorMessage = "No daemon URL configured. Scan a QR code or enter a URL first."
            return
        }

        guard let url = URL(string: trimmedURL) else {
            connectionStatus = "Bad URL"
            errorMessage = "Invalid daemon URL: \(trimmedURL)"
            return
        }

        connectionStatus = "Connecting…"
        let task = URLSession.shared.webSocketTask(with: url)
        socketTask = task
        task.resume()
        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }

        Task { [weak self] in
            await self?.bootstrapConnection(using: task)
        }
    }

    private func receiveLoop() async {
        guard let task = socketTask else { return }

        while !Task.isCancelled {
            do {
                let message = try await task.receive()
                switch message {
                case .string(let text):
                    handle(text: text)
                case .data(let data):
                    if let text = String(data: data, encoding: .utf8) {
                        handle(text: text)
                    }
                @unknown default:
                    break
                }
            } catch {
                guard socketTask === task else { break }
                socketTask = nil
                receiveTask = nil
                markAgentsOffline()
                connectionStatus = "Disconnected"
                break
            }
        }
    }

    private func handle(text: String) {
        guard let data = text.data(using: .utf8) else { return }
        let decoder = JSONDecoder()

        do {
            let envelope = try decoder.decode(DaemonEnvelope.self, from: data)
            switch envelope.type {
            case "agent_list":
                agents = try decoder.decode(AgentListEvent.self, from: data).agents
                    .sorted {
                        $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
                    }
                if selectedAgentIDs.isEmpty {
                    selectedAgentIDs = Set(agents.filter(\.isOnline).map(\.agentID))
                }
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
                allThreads = try decoder.decode(ThreadListEvent.self, from: data).threads
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
                if activeThreadID == snapshot.threadID {
                    activeThreadSnapshot = snapshot
                    timeline = timelineByThread[snapshot.threadID] ?? []
                    let allParticipants = Set(snapshot.participants.filter(\.isAgent).map(\.participantID))
                    if selectedParticipantIDs.isEmpty || !selectedParticipantIDs.isSubset(of: allParticipants) {
                        selectedParticipantIDs = allParticipants
                    }
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
                connectionStatus = "Synced thread"
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
                    DaemonThreadSummary(
                        threadID: summary.threadID,
                        title: summary.title,
                        workingDir: summary.workingDir,
                        createdAtMS: summary.createdAtMS,
                        state: summary.state,
                        participantCount: summary.participantCount + 1,
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
                    DaemonThreadSummary(
                        threadID: summary.threadID,
                        title: summary.title,
                        workingDir: summary.workingDir,
                        createdAtMS: summary.createdAtMS,
                        state: summary.state,
                        participantCount: max(summary.participantCount - 1, 1),
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
                        title: event.sender.displayName,
                        body: event.content,
                        tintName: "blue"
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_assistant_message":
                let event = try decoder.decode(ThreadAssistantMessageEvent.self, from: data)
                discardActiveLegacyAssistantMessage(threadID: event.threadID, sessionID: event.sessionID)
                upsertAssistantMessage(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_delta":
                let event = try decoder.decode(ThreadAgentDeltaEvent.self, from: data)
                upsertLegacyAssistantDelta(event)
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_tool_update":
                let event = try decoder.decode(ThreadAgentToolUpdateEvent.self, from: data)
                let body = event.content.map { "\(event.title) · \(event.status)\n\($0)" } ?? "\(event.title) · \(event.status)"
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .tool,
                        title: agentDisplayName(for: event.agentID),
                        body: body,
                        tintName: tintName(for: event.agentID)
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_plan_update":
                let event = try decoder.decode(ThreadAgentPlanUpdateEvent.self, from: data)
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .plan,
                        title: agentDisplayName(for: event.agentID),
                        body: String(describing: event.planJSON),
                        tintName: tintName(for: event.agentID)
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_turn_end":
                let event = try decoder.decode(ThreadAgentTurnEndEvent.self, from: data)
                finalizeLegacyAssistantMessage(event)
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
    }

    private func removeTimelineEntry(id: String, from threadID: String) {
        guard var entries = timelineByThread[threadID] else { return }
        entries.removeAll { $0.id == id }
        timelineByThread[threadID] = entries
        if activeThreadID == threadID {
            timeline = entries
        }
    }

    private func upsertAssistantMessage(_ event: ThreadAssistantMessageEvent) {
        let existing = timelineByThread[event.threadID]?.first(where: { $0.id == event.messageID })
        let entry = DaemonTimelineEntry(
            id: event.messageID,
            sortThreadSeq: existing?.sortThreadSeq ?? event.threadSeq,
            lastThreadSeq: event.threadSeq,
            kind: .assistantTurn,
            title: agentDisplayName(for: event.agentID),
            body: event.response,
            thinkingBody: event.thinking.isEmpty ? nil : event.thinking,
            status: event.state,
            tintName: tintName(for: event.agentID)
        )
        appendTimeline(entry, to: event.threadID)
    }

    private func upsertLegacyAssistantDelta(_ event: ThreadAgentDeltaEvent) {
        guard let state = legacyAssistantMessages.consume(delta: event) else {
            return
        }
        appendTimeline(
            state.timelineEntry(status: "streaming", tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func finalizeLegacyAssistantMessage(_ event: ThreadAgentTurnEndEvent) {
        guard let state = legacyAssistantMessages.finish(turnEnd: event) else {
            return
        }
        appendTimeline(
            state.timelineEntry(status: "completed", tintName: tintName(for: state.agentID)),
            to: event.threadID
        )
    }

    private func discardActiveLegacyAssistantMessage(threadID: String, sessionID: String) {
        guard let state = legacyAssistantMessages.removeActiveState(
            threadID: threadID,
            sessionID: sessionID
        ) else {
            return
        }
        removeTimelineEntry(id: state.entryID, from: threadID)
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
            let allParticipants = Set(snapshot.participants.filter(\.isAgent).map(\.participantID))
            if selectedParticipantIDs.isEmpty {
                selectedParticipantIDs = allParticipants
            }
        }
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
            selectedParticipantIDs.remove(participantID)
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
        legacyAssistantMessages.removeStates(for: threadID)

        if activeThreadID == threadID {
            activeThreadID = nil
            activeThreadSnapshot = nil
            timeline = []
            selectedParticipantIDs = []
        }

        applyThreadPresentation()

        if activeThreadID == nil, let firstThread = threads.first {
            attachThread(firstThread.threadID)
        }
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
    }

    private func persistThreadPreferences() {
        let defaults = UserDefaults.standard
        defaults.set(Array(pinnedThreadIDs).sorted(), forKey: pinnedThreadsKey)
        defaults.set(Array(hiddenThreadIDs).sorted(), forKey: hiddenThreadsKey)
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

    private func bootstrapConnection(using task: URLSessionWebSocketTask) async {
        do {
            try await ping(task)
            guard socketTask === task else { return }
            connectionStatus = "Connected"
            await refreshDaemonState()
        } catch {
            guard socketTask === task else { return }
            handleConnectionFailure(message: "Failed to connect to daemon: \(error.localizedDescription)")
        }
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

    private func markAgentsOffline() {
        guard !agents.isEmpty else { return }
        agents = agents.map { $0.withStatus("offline") }
    }

    private func handleConnectionFailure(message: String) {
        errorMessage = message
        receiveTask?.cancel()
        receiveTask = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil
        markAgentsOffline()
        connectionStatus = "Disconnected"
    }

    private var hasActiveConnection: Bool {
        guard socketTask != nil else { return false }
        switch connectionStatus {
        case "Disconnected", "Not connected", "Not configured", "Bad URL":
            return false
        default:
            return true
        }
    }

    private func refreshIdleConnectionStatus() {
        connectionStatus = hasConfiguredDaemonURL ? "Not connected" : "Not configured"
    }

    private func normalizedDaemonURL(from payload: String) -> String? {
        if payload.hasPrefix("ws://") || payload.hasPrefix("wss://") {
            return payload
        }

        guard let components = URLComponents(string: payload) else {
            return nil
        }

        guard components.scheme?.lowercased() == "agentchat",
              components.host?.lowercased() == "connect",
              let urlItem = components.queryItems?.first(where: { $0.name == "url" })?.value,
              urlItem.hasPrefix("ws://") || urlItem.hasPrefix("wss://")
        else {
            return nil
        }

        return urlItem
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
            try await socketTask.send(.string(text))
        } catch {
            handleConnectionFailure(message: "Send failed: \(error.localizedDescription)")
        }
    }
}
