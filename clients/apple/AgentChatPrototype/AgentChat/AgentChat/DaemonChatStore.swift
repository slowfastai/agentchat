import Combine
import Foundation
import SwiftUI

@MainActor
final class DaemonChatStore: ObservableObject {
    @Published var connectionStatus = "Connecting…"
    @Published var agents: [DaemonAgentSummary] = []
    @Published var threads: [DaemonThreadSummary] = []
    @Published var activeThreadID: String?
    @Published var activeThreadSnapshot: DaemonThreadSnapshot?
    @Published var timeline: [DaemonTimelineEntry] = []
    @Published var selectedAgentIDs: Set<String> = []
    @Published var selectedParticipantIDs: Set<String> = []
    @Published var errorMessage: String?

    @AppStorage("agentchat_daemon_ws_url") private var daemonURLString = "ws://127.0.0.1:9390"

    var daemonURL: String {
        daemonURLString
    }

    private var socketTask: URLSessionWebSocketTask?
    private var receiveTask: Task<Void, Never>?
    private var reconnectTask: Task<Void, Never>?
    private var pendingThreadAgentIDs: [String] = []
    private var hasStarted = false
    private var snapshotsByThread: [String: DaemonThreadSnapshot] = [:]
    private var timelineByThread: [String: [DaemonTimelineEntry]] = [:]
    private var cursorByThread: [String: UInt64] = [:]

    func start() {
        guard !hasStarted else { return }
        hasStarted = true
        connect()
    }

    func createThreadWithSelectedAgents() {
        let chosenAgentIDs = Array(selectedAgentIDs.isEmpty ? Set(agents.map(\.agentID)) : selectedAgentIDs)
        pendingThreadAgentIDs = chosenAgentIDs.sorted()
        let title = chosenAgentIDs.isEmpty ? "New Chat" : chosenAgentIDs.joined(separator: " + ")
        Task {
            await send(CreateThreadRequest(title: title, workingDir: "."))
        }
    }

    func addSelectedAgentsToActiveThread() {
        guard let threadID = activeThreadID else {
            errorMessage = "Open a thread first, then add agents."
            return
        }

        let existingAgentIDs = Set(activeThreadSnapshot?.participants.compactMap(\.agentID) ?? [])
        let selectedOrAllAgents = selectedAgentIDs.isEmpty ? Set(agents.map(\.agentID)) : selectedAgentIDs
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
        daemonURLString = trimmed
        errorMessage = nil
        connect()
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
        reconnectTask?.cancel()
        receiveTask?.cancel()
        socketTask?.cancel(with: .goingAway, reason: nil)

        guard let url = URL(string: daemonURLString) else {
            connectionStatus = "Bad URL"
            errorMessage = "Invalid daemon URL: \(daemonURLString)"
            return
        }

        connectionStatus = "Connecting…"
        let task = URLSession.shared.webSocketTask(with: url)
        socketTask = task
        task.resume()
        connectionStatus = "Connected"

        Task {
            await send(ListAgentsRequest())
            await send(ListThreadsRequest())
            if let activeThreadID {
                await send(AttachThreadRequest(threadID: activeThreadID, afterSeq: cursorByThread[activeThreadID]))
            }
        }

        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }
    }

    private func scheduleReconnect() {
        guard reconnectTask == nil else { return }
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_000_000_000)
            await MainActor.run {
                self?.reconnectTask = nil
                self?.connect()
            }
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
                connectionStatus = "Disconnected"
                if !Task.isCancelled {
                    scheduleReconnect()
                }
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
                threads.removeAll { $0.threadID == event.threadID }
                threads.insert(summary, at: 0)
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
                threads = try decoder.decode(ThreadListEvent.self, from: data).threads
                    .sorted { $0.createdAtMS > $1.createdAtMS }
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
            case "thread_agent_delta":
                let event = try decoder.decode(ThreadAgentDeltaEvent.self, from: data)
                let kind: DaemonTimelineEntry.Kind = event.deltaType == "thinking" ? .thinking : .agentMessage
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: kind,
                        title: event.agentID.capitalized,
                        body: event.content,
                        tintName: colorName(for: event.agentID)
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_tool_update":
                let event = try decoder.decode(ThreadAgentToolUpdateEvent.self, from: data)
                let body = event.content.map { "\(event.title) · \(event.status)\n\($0)" } ?? "\(event.title) · \(event.status)"
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .tool,
                        title: event.agentID.capitalized,
                        body: body,
                        tintName: colorName(for: event.agentID)
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
                        title: event.agentID.capitalized,
                        body: String(describing: event.planJSON),
                        tintName: colorName(for: event.agentID)
                    ),
                    to: event.threadID
                )
                touchThread(threadID: event.threadID, lastThreadSeq: event.threadSeq)
            case "thread_agent_turn_end":
                let event = try decoder.decode(ThreadAgentTurnEndEvent.self, from: data)
                appendTimeline(
                    DaemonTimelineEntry(
                        threadID: event.threadID,
                        threadSeq: event.threadSeq,
                        kind: .turnEnd,
                        title: event.agentID.capitalized,
                        body: event.stopReason,
                        tintName: colorName(for: event.agentID)
                    ),
                    to: event.threadID
                )
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
        entries.removeAll { $0.threadSeq == entry.threadSeq }
        entries.append(entry)
        entries.sort { $0.threadSeq < $1.threadSeq }
        timelineByThread[threadID] = entries
        cursorByThread[threadID] = max(cursorByThread[threadID] ?? 0, entry.threadSeq)
        if activeThreadID == threadID {
            timeline = entries
        }
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
        if let index = threads.firstIndex(where: { $0.threadID == threadID }) {
            threads[index] = transform(threads[index])
            threads.sort { $0.createdAtMS > $1.createdAtMS }
        }
    }

    private func colorName(for agentID: String) -> String {
        switch agentID.lowercased() {
        case "pi": return "purple"
        case "beta": return "green"
        case "alpha", "claude": return "blue"
        case "codex": return "green"
        case "opencode": return "orange"
        default: return "indigo"
        }
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
            connect()
            return
        }
        do {
            let data = try JSONEncoder().encode(request)
            guard let text = String(data: data, encoding: .utf8) else { return }
            try await socketTask.send(.string(text))
        } catch {
            errorMessage = "Send failed: \(error.localizedDescription)"
            connectionStatus = "Disconnected"
            scheduleReconnect()
        }
    }
}
