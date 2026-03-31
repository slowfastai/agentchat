import Combine
import Foundation
import OSLog
import SwiftUI

@MainActor
final class DaemonChatStore: ObservableObject {
    private static let pinnedThreadsKey = "agentchat_pinned_thread_ids"
    private static let hiddenThreadsKey = "agentchat_hidden_thread_ids"
    private static let knownAgentsKey = "agentchat_known_agents"
    private static let selectedAgentsKey = "agentchat_selected_agent_ids"
    private static let persistedThreadStateKey = "agentchat_persisted_thread_state"
    private static let initialNetworkWarmupKey = "agentchat_initial_network_warmup_completed"
    private static let relayAppInstallationIDKey = "agentchat_relay_app_installation_id"
    private static let agentCustomNamesKey = "agentchat_agent_custom_names"
    private static let agentAvatarDataKey = "agentchat_agent_avatar_data"
    private static let agentSettingsKey = "agentchat_agent_settings"
    private static let uiTestAvatarLatencyArgument = "UITestSeedAvatarLatency"
    private let logger = Logger(subsystem: "dev.slowfast.AgentChat", category: "DaemonConnection")

    @Published private(set) var connectionState: DaemonConnectionState = .notConfigured
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
    @Published var agentSettings: [String: AgentLocalSettings] = [:]
    @Published var connectingAgentIDs: Set<String> = []

    @AppStorage("agentchat_daemon_ws_url") private var daemonURLString = ""

    var daemonURL: String {
        daemonURLString
    }

    var connectionStatus: String {
        connectionState.statusText
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
    private var reconnectAttempt = 0
    private var reconnectTask: Task<Void, Never>?
    private var isReconnecting = false
    private var suppressAutoReconnect = false

    private static let maxReconnectAttempts = 4
    private static let baseReconnectDelaySeconds: Double = 1.0
    private static let maxReconnectDelaySeconds: Double = 30.0

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
        self.agentSettings = Self.loadAgentSettings(from: defaults)

        if Self.isUITestAvatarLatencySeedEnabled {
            configureUITestAvatarLatencySeed()
            return
        }

        restorePersistedThreadState()
        refreshIdleConnectionStatus()
    }

    func start() {
        guard !hasStarted else { return }
        hasStarted = true

        if Self.isUITestAvatarLatencySeedEnabled {
            return
        }

        refreshIdleConnectionStatus()
        performInitialNetworkWarmupIfNeeded()
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
            await send(CreateThreadRequest(title: title, workingDir: "."), reportFailureToUser: true)
        }
    }

    func isPinnedThread(_ threadID: String) -> Bool {
        pinnedThreadIDs.contains(threadID)
    }

    func participants(for threadID: String) -> [DaemonThreadParticipant] {
        snapshotsByThread[threadID]?.participants ?? []
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
            await send(CloseThreadRequest(threadID: threadID), reportFailureToUser: true)
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
                await send(AddThreadParticipantRequest(threadID: threadID, agentID: agentID), reportFailureToUser: true)
            }
            await send(ListThreadsRequest())
            await send(AttachThreadRequest(threadID: threadID, afterSeq: nil))
        }
    }

    func attachThread(_ threadID: String) {
        setActiveThreadLocally(threadID)

        guard hasActiveConnection else {
            return
        }

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
                    ),
                    reportFailureToUser: true
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
                ),
                reportFailureToUser: true
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
        agentCustomNames.removeValue(forKey: agentID)
        agentAvatarData.removeValue(forKey: agentID)
        agentSettings.removeValue(forKey: agentID)
        persistKnownAgents()
        persistSelectedAgents()
        persistAgentCustomNames()
        persistAgentAvatarData()
        persistAgentSettings()
    }

    func updateAgentDisplayName(_ agentID: String, displayName: String?) {
        let trimmedName = displayName?.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedName = trimmedName?.isEmpty == true ? nil : trimmedName

        if let normalizedName {
            agentCustomNames[agentID] = normalizedName
        } else {
            agentCustomNames.removeValue(forKey: agentID)
        }
        persistAgentCustomNames()

        if let index = agents.firstIndex(where: { $0.agentID == agentID }) {
            agents[index] = agents[index].withCustomDisplayName(normalizedName)
            persistKnownAgents()
        }
    }

    func updateAgentAvatar(_ agentID: String, imageData: Data?) {
        if let imageData {
            agentAvatarData[agentID] = imageData
        } else {
            agentAvatarData.removeValue(forKey: agentID)
        }
        persistAgentAvatarData()

        if let index = agents.firstIndex(where: { $0.agentID == agentID }) {
            agents[index] = agents[index].withAvatarImageData(imageData)
            persistKnownAgents()
        }
    }

    func updateAgentSettings(_ agentID: String, settings: AgentLocalSettings?) {
        let normalized = settings?.normalized
        if let normalized, !normalized.isEmpty {
            agentSettings[agentID] = normalized
        } else {
            agentSettings.removeValue(forKey: agentID)
        }
        persistAgentSettings()
    }

    func isSelectedParticipant(_ participantID: String) -> Bool {
        selectedParticipantIDs.contains(participantID)
    }

    func reconnectNow() {
        suppressAutoReconnect = false
        connect(resetReconnectAttempt: true)
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
        connect(resetReconnectAttempt: true)
    }

    func disconnect() {
        suppressAutoReconnect = true
        cancelScheduledReconnect()
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

    private func connect(resetReconnectAttempt: Bool = true) {
        suppressAutoReconnect = false
        cancelScheduledReconnect(resetAttempt: resetReconnectAttempt)
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
            logger.warning("Rejecting invalid daemon connection link")
            connectionState = .badURL
            errorMessage = "Invalid daemon URL or relay link: \(trimmedConnection)"
            return
        }

        logger.info("Starting daemon connection; reset_reconnect_attempt=\(resetReconnectAttempt, privacy: .public)")
        connectionState = .connecting
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
                    logger.warning("Rejecting invalid direct daemon URL")
                    connectionState = .badURL
                    errorMessage = "Invalid daemon URL: \(urlString)"
                    return
                }

                logger.info("Opening direct daemon websocket to \(urlString, privacy: .public)")
                let task = URLSession.shared.webSocketTask(with: url)
                socketTask = task
                task.resume()
                try await bootstrapDirectConnection(using: task)
            case .relay(let relayPayload):
                logger.info("Resolving relay daemon connection via \(relayPayload.wsURL, privacy: .public)")
                connectionState = .pairingRelay
                let resolvedRelay = try await relayPayload.resolve(
                    appInstallationID: relayAppInstallationID(),
                    appName: relayAppName()
                )
                guard daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines) == rawValue else {
                    return
                }

                var request = URLRequest(url: resolvedRelay.wsURL)
                request.setValue("Bearer \(resolvedRelay.relayToken)", forHTTPHeaderField: "Authorization")

                connectionState = .connectingRelay
                logger.info("Opening relay websocket to \(resolvedRelay.wsURL.absoluteString, privacy: .public)")
                let task = URLSession.shared.webSocketTask(with: request)
                socketTask = task
                task.resume()

                connectionState = .securingRelay
                logger.info("Securing relay channel")
                relaySession = try await RelayAppSession.handshake(over: task, resolvedConnection: resolvedRelay)
                guard socketTask === task else { return }
                receiveTask = Task { [weak self] in
                    await self?.receiveLoop()
                }
                connectionState = .syncing
                connectingAgentIDs.removeAll()
                _ = await refreshDaemonState()
            }
        } catch {
            guard !Task.isCancelled else { return }
            handleConnectionFailure(
                message: connectionFailureMessage(
                    for: error,
                    connectionPayload: connectionPayload,
                    rawValue: rawValue
                )
            )
        }
    }

    private func bootstrapDirectConnection(using task: URLSessionWebSocketTask) async throws {
        guard socketTask === task else { return }
        logger.info("Direct daemon websocket connected; syncing daemon state")
        receiveTask = Task { [weak self] in
            await self?.receiveLoop()
        }
        connectionState = .syncing
        connectingAgentIDs.removeAll()
        _ = await refreshDaemonState()
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
                logger.warning("Daemon websocket receive loop ended: \(error.localizedDescription, privacy: .public)")
                handleTransportLoss(message: nil)
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
            let confirmsConnection = handledDaemonEventConfirmsConnection(envelope.type)
            if confirmsConnection || envelope.type == "daemon_status" || envelope.type == "error" || isRecoveringConnectionState(connectionState) {
                logger.debug(
                    "Received daemon event \(envelope.type, privacy: .public) while state \(self.connectionState.statusText, privacy: .public)"
                )
            }

            switch envelope.type {
            case "daemon_status":
                handleDaemonStatus(try decoder.decode(DaemonStatusEvent.self, from: data))
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
                connectionState = .attached(threadID: event.threadID)
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
                connectionState = .online
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

            if confirmsConnection {
                noteSuccessfulConnection(because: envelope.type)
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

    @discardableResult
    private func refreshDaemonState() async -> Bool {
        guard await send(ListAgentsRequest()) else { return false }
        guard await send(ListThreadsRequest()) else { return false }
        if let activeThreadID {
            guard await send(
                AttachThreadRequest(threadID: activeThreadID, afterSeq: cursorByThread[activeThreadID])
            ) else {
                return false
            }
        }
        return true
    }

    private func performInitialNetworkWarmupIfNeeded() {
        guard defaults.object(forKey: Self.initialNetworkWarmupKey) == nil else {
            return
        }

        defaults.set(true, forKey: Self.initialNetworkWarmupKey)

        Task.detached(priority: .background) {
            guard let url = URL(string: "https://www.google.com/generate_204") else {
                return
            }

            let configuration = URLSessionConfiguration.ephemeral
            configuration.waitsForConnectivity = true

            let session = URLSession(configuration: configuration)
            defer {
                session.finishTasksAndInvalidate()
            }

            var request = URLRequest(url: url)
            request.timeoutInterval = 10
            request.cachePolicy = .reloadIgnoringLocalCacheData

            _ = try? await session.data(for: request)
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

    private func handleDaemonStatus(_ event: DaemonStatusEvent) {
        guard event.state == "stopping" else { return }

        logger.warning(
            "Daemon announced shutdown; reason=\((event.reason ?? event.message ?? "unknown"), privacy: .public)"
        )
        suppressAutoReconnect = true
        cancelScheduledReconnect()
        connectionTask?.cancel()
        connectionTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        relaySession = nil
        socketTask?.cancel(with: .goingAway, reason: nil)
        socketTask = nil
        markAgentsOffline()
        connectingAgentIDs.removeAll()
        connectionState = .stoppedByServer(reason: event.reason ?? event.message)
        errorMessage = nil
    }

    private func handleTransportLoss(message: String?) {
        let previousStatus = connectionState.statusText
        logger.warning(
            "Daemon transport lost while state \(previousStatus, privacy: .public); message=\((message ?? "none"), privacy: .public)"
        )
        if let message {
            errorMessage = message
        }
        connectionTask?.cancel()
        connectionTask = nil
        receiveTask?.cancel()
        receiveTask = nil
        relaySession = nil
        socketTask = nil
        markAgentsOffline()
        connectingAgentIDs.removeAll()

        if suppressAutoReconnect {
            if case .stoppedByServer = connectionState {
                return
            }
            refreshIdleConnectionStatus()
            return
        }

        scheduleReconnect()
    }

    private func handleConnectionFailure(message: String) {
        socketTask?.cancel(with: .goingAway, reason: nil)
        handleTransportLoss(message: message)
    }

    private func handledDaemonEventConfirmsConnection(_ eventType: String) -> Bool {
        switch eventType {
        case "agent_list",
            "thread_created",
            "thread_list",
            "thread_attached",
            "thread_snapshot",
            "thread_closed",
            "thread_replay_complete",
            "thread_participant_added",
            "thread_participant_removed",
            "thread_message",
            "thread_assistant_message",
            "thread_agent_delta",
            "thread_agent_tool_update",
            "thread_agent_plan_update",
            "thread_agent_turn_end":
            return true
        default:
            return false
        }
    }

    private func isRecoveringConnectionState(_ state: DaemonConnectionState) -> Bool {
        switch state {
        case .connecting, .pairingRelay, .connectingRelay, .securingRelay, .syncing, .reconnecting:
            return true
        default:
            return false
        }
    }

    private func noteSuccessfulConnection(because eventType: String) {
        cancelScheduledReconnect(resetAttempt: true)

        switch connectionState {
        case .online, .attached, .stoppedByServer:
            return
        default:
            logger.info(
                "Confirmed daemon connection from \(eventType, privacy: .public) while state \(self.connectionState.statusText, privacy: .public)"
            )
            connectionState = .online
        }
    }

    private func connectionFailureMessage(
        for error: Error,
        connectionPayload: ScannedDaemonConnectionPayload,
        rawValue: String
    ) -> String {
        let nsError = error as NSError
        var components = [
            "Failed to connect to daemon",
            "\(nsError.domain) (\(nsError.code)): \(nsError.localizedDescription)"
        ]

        switch connectionPayload {
        case .direct(let urlString, _):
            components.append("URL: \(urlString)")
            if let hint = directConnectionHint(for: urlString) {
                components.append(hint)
            }
        case .relay(let relayPayload):
            components.append("Relay URL: \(relayPayload.wsURL)")
            components.append("If this is a public endpoint, it must use wss://.")
        }

        if rawValue != daemonURLString.trimmingCharacters(in: .whitespacesAndNewlines) {
            components.append("The saved connection link changed while connecting.")
        }

        return components.joined(separator: "\n")
    }

    private func directConnectionHint(for urlString: String) -> String? {
        guard let url = URL(string: urlString),
              let scheme = url.scheme?.lowercased(),
              let host = url.host?.lowercased()
        else {
            return nil
        }

        if scheme == "ws" && !isLocalNetworkHost(host) {
            return "Remote iPhone connections to non-local hosts require wss://, not ws://."
        }

        if isLocalNetworkHost(host) {
            return "If this is a LAN URL, allow AgentChat in Settings > Privacy & Security > Local Network."
        }

        return nil
    }

    private func isLocalNetworkHost(_ host: String) -> Bool {
        if host == "localhost" || host.hasSuffix(".local") {
            return true
        }

        if host.hasPrefix("10.") || host.hasPrefix("192.168.") || host.hasPrefix("127.") {
            return true
        }

        if host.hasPrefix("172.") {
            let parts = host.split(separator: ".")
            if parts.count >= 2, let secondOctet = Int(parts[1]), (16...31).contains(secondOctet) {
                return true
            }
        }

        if host.hasPrefix("fe80:") || host.hasPrefix("fc") || host.hasPrefix("fd") || host == "::1" {
            return true
        }

        return false
    }

    private func scheduleReconnect() {
        guard !isReconnecting, hasConfiguredDaemonURL else { return }
        guard !suppressAutoReconnect else { return }
        guard reconnectAttempt < Self.maxReconnectAttempts else {
            logger.error("Daemon reconnect exhausted after \(Self.maxReconnectAttempts, privacy: .public) attempts")
            reconnectAttempt = 0
            connectionState = .unavailable
            errorMessage = "Daemon unavailable. Tap Reconnect to try again."
            return
        }

        isReconnecting = true
        reconnectAttempt += 1

        let delay = min(
            Self.baseReconnectDelaySeconds * pow(2.0, Double(reconnectAttempt - 1)),
            Self.maxReconnectDelaySeconds
        )

        logger.info(
            "Scheduling daemon reconnect attempt \(self.reconnectAttempt, privacy: .public) in \(delay, privacy: .public)s"
        )
        connectionState = .reconnecting(attempt: reconnectAttempt)

        reconnectTask?.cancel()
        reconnectTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            guard !Task.isCancelled else { return }
            await MainActor.run { [weak self] in
                guard let self else { return }
                self.isReconnecting = false
                if !self.hasActiveConnection {
                    self.connect(resetReconnectAttempt: false)
                }
            }
        }
    }

    private func cancelScheduledReconnect(resetAttempt: Bool = true) {
        reconnectTask?.cancel()
        reconnectTask = nil
        isReconnecting = false
        if resetAttempt {
            reconnectAttempt = 0
        }
    }

    private var hasActiveConnection: Bool {
        guard socketTask != nil else { return false }
        switch connectionState {
        case .notConfigured, .badURL, .unavailable, .stoppedByServer:
            return false
        default:
            return true
        }
    }

    private func refreshIdleConnectionStatus() {
        connectionState = hasConfiguredDaemonURL ? .unavailable : .notConfigured
    }

    private func upsertAgents(_ incomingAgents: [DaemonAgentSummary]) {
        agents = AgentRoster.merge(knownAgents: agents, incomingAgents: incomingAgents)
        persistKnownAgents()
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

    private static func loadAgentSettings(from defaults: UserDefaults) -> [String: AgentLocalSettings] {
        guard let data = defaults.data(forKey: Self.agentSettingsKey),
              let settings = try? JSONDecoder().decode([String: AgentLocalSettings].self, from: data)
        else {
            return [:]
        }
        return settings
    }

    private func persistAgentCustomNames() {
        guard let data = try? JSONEncoder().encode(agentCustomNames) else { return }
        defaults.set(data, forKey: Self.agentCustomNamesKey)
    }

    private func persistAgentAvatarData() {
        guard let data = try? JSONEncoder().encode(agentAvatarData) else { return }
        defaults.set(data, forKey: Self.agentAvatarDataKey)
    }

    private func persistAgentSettings() {
        guard let data = try? JSONEncoder().encode(agentSettings) else { return }
        defaults.set(data, forKey: Self.agentSettingsKey)
    }

    func updateAgent(id agentID: String, name: String?, avatarData: Data?, settings: AgentLocalSettings? = nil) {
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

        let normalizedSettings = settings?.normalized
        if let normalizedSettings, !normalizedSettings.isEmpty {
            agentSettings[agentID] = normalizedSettings
        } else if settings != nil {
            agentSettings.removeValue(forKey: agentID)
        }
        persistAgentSettings()

        if let index = agents.firstIndex(where: { $0.agentID == agentID }) {
            agents[index] = agents[index]
                .withCustomDisplayName(name)
                .withAvatarImageData(avatarData)
            persistKnownAgents()
        }
    }

    func removeAgent(id agentID: String) {
        agentCustomNames.removeValue(forKey: agentID)
        agentAvatarData.removeValue(forKey: agentID)
        agentSettings.removeValue(forKey: agentID)
        persistAgentCustomNames()
        persistAgentAvatarData()
        persistAgentSettings()

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

    func settings(for agentID: String) -> AgentLocalSettings {
        agentSettings[agentID]?.normalized ?? AgentLocalSettings()
    }

    func agentSummary(for participant: DaemonThreadParticipant) -> DaemonAgentSummary? {
        guard let agentID = participant.agentID else { return nil }

        if let existing = agents.first(where: { $0.agentID == agentID }) {
            return existing
        }

        return DaemonAgentSummary(
            agentID: agentID,
            name: participant.displayName,
            mentionHandleOverride: participant.mentionHandleOverride,
            kind: participant.kind,
            status: participant.state,
            defaultWorkingDir: nil,
            capabilities: [],
            customDisplayName: agentCustomNames[agentID],
            avatarImageData: agentAvatarData[agentID]
        )
    }

    func isConnecting(agentID: String) -> Bool {
        connectingAgentIDs.contains(agentID)
    }

    @discardableResult
    private func send<Request: Encodable>(
        _ request: Request,
        reportFailureToUser: Bool = false
    ) async -> Bool {
        guard let socketTask else {
            markAgentsOffline()
            preserveRecoveryStateOrRefreshIdleStatus()
            if reportFailureToUser {
                errorMessage = hasConfiguredDaemonURL
                    ? "Not connected to a daemon. Tap Reconnect, scan a QR code, or enter a URL first."
                    : "No daemon URL configured. Scan a QR code or enter a URL first."
            }
            return false
        }
        do {
            let data = try JSONEncoder().encode(request)
            guard let text = String(data: data, encoding: .utf8) else { return false }
            let outboundText: String
            if var relaySession {
                outboundText = try relaySession.encryptJSONString(text)
                self.relaySession = relaySession
            } else {
                outboundText = text
            }
            try await socketTask.send(.string(outboundText))
            return true
        } catch {
            if reportFailureToUser {
                handleConnectionFailure(message: "Send failed: \(error.localizedDescription)")
            } else {
                socketTask.cancel(with: .goingAway, reason: nil)
                handleTransportLoss(message: nil)
            }
            return false
        }
    }

    private func preserveRecoveryStateOrRefreshIdleStatus() {
        switch connectionState {
        case .connecting, .pairingRelay, .connectingRelay, .securingRelay, .syncing, .reconnecting:
            return
        case .stoppedByServer:
            return
        default:
            refreshIdleConnectionStatus()
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

    private static var isUITestAvatarLatencySeedEnabled: Bool {
        ProcessInfo.processInfo.arguments.contains(uiTestAvatarLatencyArgument)
    }

    private func configureUITestAvatarLatencySeed() {
        let threadID = "thread-avatar-latency"
        let createdAtMS: UInt64 = 1_743_379_200_000

        let agent = DaemonAgentSummary(
            agentID: "codex",
            name: "Codex",
            kind: "codex_app_server",
            status: "online",
            defaultWorkingDir: ".",
            capabilities: ["session", "prompt", "cancel"]
        )

        let participant = DaemonThreadParticipant(
            participantID: "participant-codex",
            kind: "agent",
            displayName: "Codex",
            agentID: "codex",
            sessionID: "session-codex",
            state: "idle"
        )

        let snapshot = DaemonThreadSnapshot(
            threadID: threadID,
            title: "Avatar Latency Thread",
            workingDir: ".",
            createdAtMS: createdAtMS,
            lastThreadSeq: 2,
            participants: [participant]
        )

        let summary = DaemonThreadSummary(
            threadID: threadID,
            title: "Avatar Latency Thread",
            workingDir: ".",
            createdAtMS: createdAtMS,
            state: "idle",
            participantCount: 1,
            lastThreadSeq: 2
        )

        let timelineEntry = DaemonTimelineEntry(
            id: "avatar-latency-assistant-turn",
            sortThreadSeq: 2,
            lastThreadSeq: 2,
            kind: .assistantTurn,
            agentID: "codex",
            title: "Codex",
            body: "Seeded assistant turn for avatar latency measurement.",
            status: "completed",
            tintName: "green"
        )

        agents = [agent]
        selectedAgentIDs = [agent.agentID]
        allThreads = [summary]
        threads = [summary]
        snapshotsByThread = [threadID: snapshot]
        timelineByThread = [threadID: [timelineEntry]]
        cursorByThread = [threadID: 2]
        connectionState = .online
        errorMessage = nil
        setActiveThreadLocally(nil)
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

#if DEBUG
extension DaemonChatStore {
    func testingHandleIncomingText(_ text: String) {
        handle(text: text)
    }

    func testingSetConnectionState(_ state: DaemonConnectionState) {
        connectionState = state
    }

    func testingSetReconnectAttempt(_ attempt: Int, isReconnecting: Bool) {
        reconnectAttempt = attempt
        self.isReconnecting = isReconnecting
    }

    func testingSetConfiguredDaemonURL(_ value: String) {
        daemonURLString = value
    }

    @discardableResult
    func testingSendWithoutSocket<Request: Encodable>(
        _ request: Request,
        reportFailureToUser: Bool = false
    ) async -> Bool {
        await send(request, reportFailureToUser: reportFailureToUser)
    }

    var testingReconnectAttempt: Int {
        reconnectAttempt
    }
}
#endif
