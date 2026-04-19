import Foundation
import SwiftUI
import CryptoKit
import Combine

private struct DemoStoreSnapshot: Codable {
    var projects: [Project]
    var agents: [AgentProfile]
    var sessions: [WorkspaceSession]
    var threads: [Thread]
    var artifacts: [IssueArtifact]
    var decisions: [IssueDecision]
    var timelines: [PersistedThreadTimeline]
    var selectedProjectID: UUID?
    var selectedIssueID: UUID?
    var selectedThreadID: UUID?
    var agentCustomNames: [String: String]
    var agentAvatarData: [String: Data]
    var agentDistillations: [PersistedAgentDistillation]?
}

private struct PersistedThreadTimeline: Codable {
    var threadID: UUID
    var items: [TimelineItem]
}

private struct PersistedAgentDistillation: Codable {
    var threadID: UUID
    var result: AgentDistillationResult
}

struct DistilledArtifactDraft: Codable {
    var kind: IssueArtifactKind
    var title: String
    var summary: String
    var pathOrURL: String
}

struct DistilledDecisionDraft: Codable {
    var title: String
    var rationale: String
}

struct DistilledIssueDraft: Codable {
    var title: String
    var summary: String
    var status: IssueStatus
    var priority: IssuePriority
    var assignees: [ParticipantRef]
}

private struct AgentDistillationResult: Codable {
    var summary: String?
    var decision: DistilledDecisionDraft?
    var artifact: DistilledArtifactDraft?
    var followUp: DistilledIssueDraft?
    var sourceAgentName: String
    var templateVersion: String
    var generatedAt: Date
}

private struct AgentDistillationEnvelope: Decodable {
    var summary: String?
    var decision: AgentDecisionPayload?
    var artifact: AgentArtifactPayload?
    var followUp: AgentFollowUpPayload?

    struct AgentDecisionPayload: Decodable {
        var title: String
        var rationale: String
    }

    struct AgentArtifactPayload: Decodable {
        var kind: String?
        var title: String
        var summary: String
        var pathOrURL: String?
    }

    struct AgentFollowUpPayload: Decodable {
        var title: String
        var summary: String
        var priority: String?
    }
}

@MainActor
final class WorkspaceStore: ObservableObject {
    private static let daemonURLKey = "AgentChatWorkspace.daemonURL"

    @Published var projects: [Project] = [] { didSet { schedulePersistence() } }
    @Published var agents: [AgentProfile] = [] { didSet { schedulePersistence() } }
    @Published var sessions: [WorkspaceSession] = [] { didSet { schedulePersistence() } }
    @Published var threads: [Thread] = [] { didSet { schedulePersistence() } }
    @Published var artifacts: [IssueArtifact] = [] { didSet { schedulePersistence() } }
    @Published var decisions: [IssueDecision] = [] { didSet { schedulePersistence() } }
    @Published var timelineByThread: [UUID: [TimelineItem]] = [:] { didSet { schedulePersistence() } }
    @Published var selectedProjectID: UUID? { didSet { schedulePersistence() } }
    @Published var selectedIssueID: UUID? { didSet { schedulePersistence() } }
    @Published var selectedThreadID: UUID? { didSet { schedulePersistence() } }
    
    @Published var agentCustomNames: [String: String] = [:] { didSet { schedulePersistence() } }
    @Published var agentAvatarData: [String: Data] = [:] { didSet { schedulePersistence() } }
    @Published var connectingAgentIDs: Set<String> = []
    @Published var isRefreshingAgentsFromDaemon = false
    @Published var daemonStatusText = "Using seeded prototype agents"
    @Published var daemonStatusAccent: ColorToken = .gray
    @Published var refreshingThreadIDs: Set<UUID> = []
    @Published var distillingThreadIDs: Set<UUID> = []
    @Published var daemonURL: String = UserDefaults.standard.string(forKey: WorkspaceStore.daemonURLKey)
        ?? PrototypeDaemonAgentBridge.defaultURLString {
        didSet {
            UserDefaults.standard.set(daemonURL, forKey: Self.daemonURLKey)
        }
    }

    private let legacySnapshotKey = "AgentChatPrototype.DemoStoreSnapshot.v1"
    private let snapshotDirectoryName = "AgentChatPrototype"
    private let snapshotFileName = "DemoStoreSnapshot.v1.json"
    private var isHydratingSnapshot = false
    private var persistenceTask: Task<Void, Never>?
    private var agentDistillationByThreadID: [UUID: AgentDistillationResult] = [:]
    private var daemonEndpoint: DaemonConnectionEndpoint {
        .direct(urlString: daemonURL)
    }

    init() {
        isHydratingSnapshot = true
        let restored = restoreSnapshot()
        if !restored {
            seed()
        }
        normalizeSelectionState()
        isHydratingSnapshot = false
        if !restored {
            schedulePersistence()
        }
        Task {
            await refreshAgentsFromDaemon()
            await refreshDaemonBackedThreads()
        }
    }
    
    func updateAgent(id agentID: String, name: String?, avatarData: Data?) {
        if let name = name {
            agentCustomNames[agentID] = name
        } else {
            agentCustomNames.removeValue(forKey: agentID)
        }

        if let avatarData = avatarData {
            agentAvatarData[agentID] = avatarData
        } else {
            agentAvatarData.removeValue(forKey: agentID)
        }
    }

    func removeAgent(id agentID: String) {
        agentCustomNames.removeValue(forKey: agentID)
        agentAvatarData.removeValue(forKey: agentID)
        
        if let index = agents.firstIndex(where: { $0.id.uuidString == agentID }) {
            agents.remove(at: index)
        }
    }

    func connectToAgent(id agentID: String) {
        guard !connectingAgentIDs.contains(agentID) else { return }
        connectingAgentIDs.insert(agentID)

        Task {
            await refreshAgentsFromDaemon()
            connectingAgentIDs.remove(agentID)
            if let index = agents.firstIndex(where: { $0.id.uuidString == agentID }) {
                agents[index].isOnline = true
            }
        }
    }

    func updateDaemonURL(_ value: String) {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let endpoint = DaemonConnectionEndpoint.workspaceEndpoint(from: trimmed) else {
            daemonStatusText = "Workspace requires a daemon connection link"
            daemonStatusAccent = .orange
            return
        }
        guard let resolvedURL = endpoint.directURLString else {
            daemonStatusText = "Workspace relay links are not implemented yet"
            daemonStatusAccent = .orange
            return
        }
        guard daemonURL != resolvedURL else { return }
        daemonURL = resolvedURL
    }

    nonisolated static func workspaceDaemonURL(from value: String) -> String? {
        DaemonConnectionEndpoint.workspaceEndpoint(from: value)?.directURLString
    }

    func refreshAgentsFromDaemon() async {
        guard !isRefreshingAgentsFromDaemon else { return }

        isRefreshingAgentsFromDaemon = true
        daemonStatusText = "Checking local daemon..."
        daemonStatusAccent = .orange
        defer {
            isRefreshingAgentsFromDaemon = false
        }

        do {
            let fetchedAgents = try await PrototypeDaemonAgentBridge.fetchAgents(from: daemonEndpoint)
            let customNames = agentCustomNames

            agents = fetchedAgents.map { agent in
                let stableID = Self.stableAgentUUID(for: agent.agentID)
                let accent = Self.accentToken(for: agent)
                let kind = Self.agentKind(for: agent)

                return AgentProfile(
                    id: stableID,
                    daemonAgentID: agent.agentID,
                    name: customNames[stableID.uuidString] ?? agent.name,
                    kind: kind,
                    accent: accent,
                    isOnline: agent.status == "online",
                    capabilityTags: agent.capabilities,
                    shortDescription: agent.capabilities.isEmpty
                        ? "\(agent.name) from local daemon"
                        : agent.capabilities.joined(separator: " · ")
                )
            }

            daemonStatusText = fetchedAgents.isEmpty
                ? "Daemon connected, no agents configured"
                : "Connected to daemon • \(fetchedAgents.count) agent\(fetchedAgents.count == 1 ? "" : "s")"
            daemonStatusAccent = fetchedAgents.isEmpty ? .orange : .green
        } catch {
            daemonStatusText = "Daemon unavailable • \(error.localizedDescription)"
            daemonStatusAccent = .red
        }
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

    var currentProject: Project? {
        if let selectedProjectID {
            return projects.first(where: { $0.id == selectedProjectID })
        }
        return projects.first
    }

    var allIssues: [Issue] {
        projects.flatMap(\.issues)
    }

    var chatThreads: [ChatThreadSummary] {
        allIssues
            .map { issue in
                let issueSessions = sessions(for: issue.id)
                let latestSession = issueSessions.first
                let preview = latestTimelinePreview(for: issue.id)
                    ?? latestSession?.latestEventText
                    ?? issue.latestActivityText

                return ChatThreadSummary(
                    issueID: issue.id,
                    issueNumber: issue.number,
                    title: issue.title,
                    participants: issue.agentNames,
                    preview: preview,
                    updatedAt: issue.updatedAt,
                    unreadCount: unreadCount(for: issue),
                    isPinned: hasRunningSessions(for: issue.id) || issue.priority == .urgent,
                    state: latestSession?.state ?? .idle,
                    accent: issue.assignees.first?.accent ?? issue.status.badgeColor
                )
            }
            .sorted {
                if $0.isPinned != $1.isPinned {
                    return $0.isPinned && !$1.isPinned
                }
                return $0.updatedAt > $1.updatedAt
            }
    }

    var workspaceCards: [WorkspaceCardModel] {
        allIssues.map { issue in
            let issueSessions = sessions(for: issue.id)
            let latestSession = issueSessions.sorted { $0.startedAt > $1.startedAt }.first
            return WorkspaceCardModel(
                id: issue.id,
                issueID: issue.id,
                issueNumber: issue.number,
                title: issue.title,
                participants: issue.agentNames,
                state: latestSession?.state ?? .idle,
                latestPreview: latestSession?.latestEventText ?? issue.latestActivityText,
                activeTool: latestSession?.activeToolName,
                elapsedSeconds: max(issue.totalActiveSeconds, issueSessions.reduce(0) { $0 + $1.elapsedSeconds })
            )
        }
        .sorted { $0.elapsedSeconds > $1.elapsedSeconds }
    }

    func issue(for issueID: UUID) -> Issue? {
        allIssues.first(where: { $0.id == issueID })
    }

    func project(for projectID: UUID) -> Project? {
        projects.first(where: { $0.id == projectID })
    }

    func projectID(forIssueID issueID: UUID) -> UUID? {
        projects.first(where: { project in
            project.issues.contains(where: { $0.id == issueID })
        })?.id
    }

    func projectTemplateFamily(forIssueID issueID: UUID) -> DistillationTemplateFamily {
        guard let projectID = projectID(forIssueID: issueID),
              let project = project(for: projectID) else {
            return .default
        }
        return project.distillationTemplateFamily
    }

    func thread(for threadID: UUID) -> Thread? {
        threads.first(where: { $0.id == threadID })
    }

    func activeThread(for issueID: UUID) -> Thread? {
        let issueThreads = threads(for: issueID)
        if let selectedThreadID,
           let selectedThread = issueThreads.first(where: { $0.id == selectedThreadID }) {
            return selectedThread
        }
        return issueThreads.first
    }

    func selectThread(_ threadID: UUID) {
        selectedThreadID = threadID
    }

    @discardableResult
    func selectDaemonThread(_ daemonThreadID: String) -> Bool {
        guard let thread = threads.first(where: { $0.daemonThreadID == daemonThreadID }) else {
            return false
        }
        selectedProjectID = projectID(forIssueID: thread.issueID)
        selectedIssueID = thread.issueID
        selectedThreadID = thread.id
        return true
    }

    func timeline(forThreadID threadID: UUID) -> [TimelineItem] {
        timelineByThread[threadID] ?? []
    }

    func timeline(forIssueID issueID: UUID) -> [TimelineItem] {
        threads(for: issueID)
            .flatMap { timelineByThread[$0.id] ?? [] }
            .sorted { $0.timestamp < $1.timestamp }
    }

    func sessions(for issueID: UUID) -> [WorkspaceSession] {
        sessions.filter { $0.issueID == issueID }
            .sorted { $0.startedAt > $1.startedAt }
    }

    func sessions(forThreadID threadID: UUID) -> [WorkspaceSession] {
        sessions.filter { $0.threadID == threadID }
            .sorted { $0.startedAt > $1.startedAt }
    }

    func artifacts(for issueID: UUID) -> [IssueArtifact] {
        artifacts
            .filter { $0.issueID == issueID }
            .sorted { $0.createdAt > $1.createdAt }
    }

    func decisions(for issueID: UUID) -> [IssueDecision] {
        decisions
            .filter { $0.issueID == issueID }
            .sorted { $0.createdAt > $1.createdAt }
    }

    func followUpIssues(for sourceIssueID: UUID) -> [Issue] {
        allIssues
            .filter { $0.sourceIssueID == sourceIssueID }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    func threads(for issueID: UUID) -> [Thread] {
        threads.filter { $0.issueID == issueID }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    func createProject(
        name: String,
        repoPath: String,
        color: ColorToken = .blue,
        distillationTemplateFamily: DistillationTemplateFamily = .default
    ) {
        let project = Project(
            id: UUID(),
            name: name,
            repoPath: repoPath,
            color: color,
            distillationTemplateFamily: distillationTemplateFamily,
            issues: []
        )
        projects.append(project)
        if selectedProjectID == nil {
            selectedProjectID = project.id
        }
    }

    func updateProjectDistillationTemplate(projectID: UUID, family: DistillationTemplateFamily) {
        guard let index = projects.firstIndex(where: { $0.id == projectID }) else { return }
        projects[index].distillationTemplateFamily = family
    }

    @discardableResult
    func addIssue(
        to projectID: UUID,
        title: String,
        summary: String = "",
        sourceIssueID: UUID? = nil,
        sourceThreadID: UUID? = nil,
        status: IssueStatus = .backlog,
        priority: IssuePriority = .medium,
        assignees: [ParticipantRef] = []
    ) -> UUID? {
        guard let projectIndex = projects.firstIndex(where: { $0.id == projectID }) else { return nil }
        
        let maxNumber = projects[projectIndex].issues.map(\.number).max() ?? 0
        let issue = Issue(
            id: UUID(),
            number: maxNumber + 1,
            title: title,
            summary: summary,
            sourceIssueID: sourceIssueID,
            sourceThreadID: sourceThreadID,
            status: status,
            priority: priority,
            assignees: assignees,
            latestActivityText: "",
            sessionCount: 0,
            threadCount: 0,
            totalActiveSeconds: 0,
            updatedAt: Date()
        )
        projects[projectIndex].issues.append(issue)
        return issue.id
    }

    func createThread(for issueID: UUID, purpose: ThreadPurpose = .discussion, agentIDs: [UUID]) {
        let issueThreads = threads.filter { $0.issueID == issueID }
        let threadNumber = issueThreads.count + 1

        var participants: [ParticipantRef] = []
        for id in agentIDs {
            if let agent = agents.first(where: { $0.id == id }) {
                participants.append(ParticipantRef(
                    id: agent.id,
                    displayName: agent.name,
                    role: .agent(agent.kind),
                    accent: agent.accent
                ))
            }
        }

        let now = Date()
        let thread = Thread(
            id: UUID(),
            issueID: issueID,
            title: "\(purpose.title) \(threadNumber)",
            purpose: purpose,
            participants: participants,
            createdAt: now,
            updatedAt: now,
            state: .active,
            latestActivityText: "Thread started"
        )
        threads.append(thread)
        selectedThreadID = thread.id

        for participant in participants {
            let session = WorkspaceSession(
                id: UUID(),
                issueID: issueID,
                threadID: thread.id,
                title: thread.title,
                state: .idle,
                agentName: participant.displayName,
                startedAt: now,
                elapsedSeconds: 0,
                latestEventText: "Ready",
                activeToolName: nil
            )
            sessions.append(session)
        }

        append(
            payload: .system(SystemEvent(text: "\(thread.title) started.")),
            issueID: issueID,
            threadID: thread.id,
            sessionID: nil
        )

        for i in projects.indices {
            if let j = projects[i].issues.firstIndex(where: { $0.id == issueID }) {
                projects[i].issues[j].threadCount += 1
                projects[i].issues[j].updatedAt = now
                break
            }
        }

        if participants.count == 1,
           let selectedAgent = agents.first(where: { $0.id == agentIDs.first }),
           let daemonAgentID = selectedAgent.daemonAgentID {
            Task {
                await attachRemoteThreadIfPossible(
                    localThreadID: thread.id,
                    daemonAgentID: daemonAgentID
                )
            }
        }
    }

    func addArtifact(
        issueID: UUID,
        threadID: UUID?,
        kind: IssueArtifactKind,
        title: String,
        summary: String,
        pathOrURL: String?
    ) {
        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedTitle.isEmpty else { return }

        let artifact = IssueArtifact(
            id: UUID(),
            issueID: issueID,
            threadID: threadID,
            kind: kind,
            title: trimmedTitle,
            summary: summary.trimmingCharacters(in: .whitespacesAndNewlines),
            pathOrURL: normalizedOptionalString(pathOrURL),
            createdAt: Date()
        )
        artifacts.append(artifact)

        updateIssue(issueID: issueID) { issue in
            issue.updatedAt = artifact.createdAt
            issue.latestActivityText = "\(kind.title): \(artifact.title)"
        }
        if let threadID {
            updateThread(threadID: threadID) { thread in
                thread.updatedAt = artifact.createdAt
                thread.latestActivityText = "Saved artifact: \(artifact.title)"
            }
        }
    }

    func addDecision(
        issueID: UUID,
        threadID: UUID?,
        title: String,
        rationale: String
    ) {
        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedRationale = rationale.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedTitle.isEmpty, !trimmedRationale.isEmpty else { return }

        let decision = IssueDecision(
            id: UUID(),
            issueID: issueID,
            threadID: threadID,
            title: trimmedTitle,
            rationale: trimmedRationale,
            createdAt: Date()
        )
        decisions.append(decision)

        updateIssue(issueID: issueID) { issue in
            issue.updatedAt = decision.createdAt
            issue.latestActivityText = "Decision: \(decision.title)"
        }
        if let threadID {
            updateThread(threadID: threadID) { thread in
                thread.updatedAt = decision.createdAt
                thread.latestActivityText = "Saved decision: \(decision.title)"
            }
        }
    }

    func deleteProject(_ projectID: UUID) {
        guard let projectIndex = projects.firstIndex(where: { $0.id == projectID }) else { return }
        
        let projectIssueIDs = Set(projects[projectIndex].issues.map { $0.id })
        let deletedThreadIDs = Set(
            threads
                .filter { projectIssueIDs.contains($0.issueID) }
                .map(\.id)
        )
        
        threads.removeAll { thread in
            projectIssueIDs.contains(thread.issueID)
        }
        
        sessions.removeAll { session in
            projectIssueIDs.contains(session.issueID)
        }
        artifacts.removeAll { artifact in
            projectIssueIDs.contains(artifact.issueID)
        }
        decisions.removeAll { decision in
            projectIssueIDs.contains(decision.issueID)
        }
        
        for threadID in deletedThreadIDs {
            timelineByThread.removeValue(forKey: threadID)
            agentDistillationByThreadID.removeValue(forKey: threadID)
        }
        
        if let selectedIssueID, projectIssueIDs.contains(selectedIssueID) {
            self.selectedIssueID = nil
        }

        if let selectedThreadID, deletedThreadIDs.contains(selectedThreadID) {
            self.selectedThreadID = nil
        }
        
        projects.remove(at: projectIndex)
        
        if selectedProjectID == projectID {
            selectedProjectID = projects.first?.id
        }

        if selectedIssueID == nil {
            selectedIssueID = currentProject?.issues.first?.id
        }
    }

    func skillCards(for issueID: UUID) -> [SkillCardModel] {
        guard let issue = issue(for: issueID) else { return [] }

        var cards = [
            SkillCardModel(
                id: UUID(),
                title: "memory-layer.md",
                path: ".agentchat/skills/shared/memory-layer.md",
                summary: "Capture reusable project conventions discovered while solving #\(issue.number).",
                updatedAt: issue.updatedAt,
                scope: .shared,
                accent: .purple
            ),
            SkillCardModel(
                id: UUID(),
                title: "session-lifecycle.md",
                path: ".agentchat/skills/shared/session-lifecycle.md",
                summary: "Keep cancellation, transcript flushes, and relay cleanup aligned across every agent.",
                updatedAt: issue.updatedAt.addingTimeInterval(-900),
                scope: .shared,
                accent: .purple
            )
        ]

        let agentSpecificCards = issue.assignees.compactMap { participant -> SkillCardModel? in
            guard case .agent = participant.role else { return nil }

            let fileName = agentSpecificSkillFileName(for: participant.displayName)
            return SkillCardModel(
                id: UUID(),
                title: fileName,
                path: ".agentchat/skills/agents/\(agentSkillPathSegment(for: participant.displayName))/\(fileName)",
                summary: agentSpecificSkillSummary(for: participant.displayName, issueNumber: issue.number),
                updatedAt: issue.updatedAt,
                scope: .agentSpecific(participant.displayName),
                accent: participant.accent
            )
        }

        cards.append(contentsOf: agentSpecificCards)
        return cards
    }

    func latestTimelinePreview(for issueID: UUID) -> String? {
        guard let latest = timeline(forIssueID: issueID).last else { return nil }

        switch latest.payload {
        case .system(let event):
            return event.text
        case .userMessage(let message):
            return "You: \(message.text)"
        case .agentMessage(let message):
            let text = message.text.isEmpty ? "…" : message.text
            return "\(message.senderName): \(text)"
        case .thinking(let event):
            return "\(event.agentName) is thinking…"
        case .toolCall(let event):
            return "\(event.agentName): \(event.title)"
        case .plan(let event):
            return "\(event.agentName): \(event.title)"
        case .turnEnd(let event):
            return "\(event.agentName) finished · \(event.reason)"
        }
    }

    func unreadCount(for issue: Issue) -> Int {
        if hasRunningSessions(for: issue.id) {
            return min(max(issue.sessionCount, 1), 3)
        }
        if issue.status == .review {
            return 1
        }
        return 0
    }

    func hasRunningSessions(for issueID: UUID) -> Bool {
        sessions(for: issueID).contains(where: { $0.state == .running })
    }

    func sendMessage(threadID: UUID, text: String, targets: [String]) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let thread = thread(for: threadID) else { return }

        let issueID = thread.issueID
        let resolvedTargets = targets.isEmpty ? (issue(for: issueID)?.agentNames ?? ["Claude"]) : targets

        append(
            payload: .userMessage(
                ChatMessage(
                    senderName: "You",
                    senderRole: .human,
                    text: trimmed,
                    isStreaming: false
                )
            ),
            issueID: issueID,
            threadID: threadID,
            sessionID: nil
        )
        updateThread(threadID: threadID) { thread in
            thread.state = .active
            thread.latestActivityText = trimmed
            thread.updatedAt = Date()
        }

        updateIssue(issueID: issueID) { issue in
            issue.status = .inProgress
            issue.latestActivityText = trimmed
            issue.updatedAt = Date()
        }

        Task {
            if let daemonThreadID = thread.daemonThreadID {
                await sendRemoteMessageIfPossible(
                    localThreadID: threadID,
                    daemonThreadID: daemonThreadID,
                    input: trimmed,
                    fallbackTargets: resolvedTargets
                )
            } else {
                await simulateResponses(threadID: threadID, input: trimmed, targets: resolvedTargets)
            }
        }
    }

    func isRefreshingThread(_ threadID: UUID) -> Bool {
        refreshingThreadIDs.contains(threadID)
    }

    func isDistillingThread(_ threadID: UUID) -> Bool {
        distillingThreadIDs.contains(threadID)
    }

    func hasAgentDistillation(for threadID: UUID) -> Bool {
        agentDistillationByThreadID[threadID] != nil
    }

    func distillationSourceLabel(for threadID: UUID) -> String {
        guard let result = agentDistillationByThreadID[threadID] else {
            guard let thread = thread(for: threadID) else {
                return "Local heuristic · Template: \(DistillationPromptTemplate.currentTemplateVersion())"
            }
            let family = projectTemplateFamily(forIssueID: thread.issueID)
            return "Local heuristic · Template: \(DistillationPromptTemplate.currentTemplateVersion(family: family))"
        }
        return "Agent: \(result.sourceAgentName) · Template: \(result.templateVersion)"
    }

    func distillationGeneratedAt(for threadID: UUID) -> Date? {
        agentDistillationByThreadID[threadID]?.generatedAt
    }

    func clearAgentDistillation(for threadID: UUID) {
        agentDistillationByThreadID.removeValue(forKey: threadID)
        schedulePersistence()
    }

    func resetPrototypeData() {
        persistenceTask?.cancel()
        deleteSnapshotFile()
        UserDefaults.standard.removeObject(forKey: legacySnapshotKey)

        isHydratingSnapshot = true
        projects = []
        agents = []
        sessions = []
        threads = []
        artifacts = []
        decisions = []
        timelineByThread = [:]
        agentDistillationByThreadID = [:]
        selectedProjectID = nil
        selectedIssueID = nil
        selectedThreadID = nil
        agentCustomNames = [:]
        agentAvatarData = [:]
        isHydratingSnapshot = false

        seed()
        normalizeSelectionState()
        schedulePersistence()

        Task {
            await refreshAgentsFromDaemon()
            await refreshDaemonBackedThreads()
        }
    }

    func refreshThreadFromDaemon(threadID: UUID) async {
        guard !refreshingThreadIDs.contains(threadID),
              let currentThread = thread(for: threadID),
              let daemonThreadID = currentThread.daemonThreadID
        else {
            return
        }

        refreshingThreadIDs.insert(threadID)
        defer {
            refreshingThreadIDs.remove(threadID)
        }

        do {
            let replayEntries = try await PrototypeDaemonAgentBridge.replayRemoteThread(
                threadID: daemonThreadID,
                endpoint: daemonEndpoint
            )
            let mappedItems = replayTimelineItems(
                replayEntries,
                issueID: currentThread.issueID,
                threadID: threadID
            )

            timelineByThread[threadID] = mappedItems

            let latestPreview = latestReplayPreview(from: mappedItems) ?? "Daemon replay synced"
            let hasTurnEnd = mappedItems.contains {
                if case .turnEnd = $0.payload {
                    return true
                }
                return false
            }
            let lastToolTitle = mappedItems.reversed().compactMap { item -> String? in
                if case .toolCall(let event) = item.payload {
                    return event.title
                }
                return nil
            }.first

            updateThread(threadID: threadID) { thread in
                thread.state = hasTurnEnd ? .completed : .active
                thread.latestActivityText = latestPreview
                thread.updatedAt = Date()
            }

            updateIssue(issueID: currentThread.issueID) { issue in
                issue.status = hasTurnEnd ? .review : .inProgress
                issue.latestActivityText = latestPreview
                issue.updatedAt = Date()
            }

            for index in sessions.indices where sessions[index].threadID == threadID {
                sessions[index].state = hasTurnEnd ? .completed : .running
                sessions[index].latestEventText = latestPreview
                sessions[index].activeToolName = lastToolTitle
            }
        } catch {
            append(
                payload: .system(SystemEvent(text: "Daemon replay failed. Keeping local timeline intact.")),
                issueID: currentThread.issueID,
                threadID: threadID,
                sessionID: nil
            )
        }
    }

    func distillThreadIntoIssueSummary(issueID: UUID, threadID: UUID) {
        let summary = distilledSummary(for: threadID)
        guard !summary.isEmpty else { return }

        updateIssue(issueID: issueID) { issue in
            issue.summary = summary
            issue.updatedAt = Date()
            issue.latestActivityText = "Issue summary distilled from thread"
        }

        append(
            payload: .system(SystemEvent(text: "Distilled thread output into the issue summary.")),
            issueID: issueID,
            threadID: threadID,
            sessionID: nil
        )
    }

    func distilledIssueSummaryText(for threadID: UUID) -> String? {
        let summary = agentDistillationByThreadID[threadID]?.summary ?? distilledSummary(for: threadID)
        return summary.isEmpty ? nil : summary
    }

    func distilledDecisionDraft(for threadID: UUID) -> DistilledDecisionDraft? {
        if let decision = agentDistillationByThreadID[threadID]?.decision {
            return decision
        }
        guard let thread = thread(for: threadID) else { return nil }

        let userIntent = latestUserIntent(in: threadID)
        let agentConclusion = latestAgentConclusion(in: threadID) ?? thread.latestActivityText
        let rationaleParts = [
            userIntent,
            latestPlanSummary(in: threadID),
            latestToolSummary(in: threadID),
            agentConclusion
        ]
        .compactMap { value in
            let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return trimmed.isEmpty ? nil : trimmed
        }

        guard !rationaleParts.isEmpty else { return nil }

        let titlePrefix = thread.purpose == .review ? "Adopt review outcome" : "Adopt thread outcome"
        return DistilledDecisionDraft(
            title: "\(titlePrefix): \(thread.title)",
            rationale: rationaleParts.joined(separator: "\n\n")
        )
    }

    func saveDistilledDecision(issueID: UUID, threadID: UUID) {
        guard let draft = distilledDecisionDraft(for: threadID) else { return }
        addDecision(
            issueID: issueID,
            threadID: threadID,
            title: draft.title,
            rationale: draft.rationale
        )
    }

    func distilledArtifactDraft(for threadID: UUID) -> DistilledArtifactDraft? {
        if let artifact = agentDistillationByThreadID[threadID]?.artifact {
            return artifact
        }
        guard let thread = thread(for: threadID) else { return nil }

        if let toolEvent = timeline(forThreadID: threadID).reversed().compactMap({ item -> ToolCallEvent? in
            if case .toolCall(let event) = item.payload {
                return event
            }
            return nil
        }).first {
            return DistilledArtifactDraft(
                kind: .note,
                title: toolEvent.title,
                summary: toolEvent.contentPreview ?? thread.latestActivityText,
                pathOrURL: inferredPathOrURL(from: toolEvent.contentPreview) ?? ""
            )
        }

        let latestConclusion = latestAgentConclusion(in: threadID) ?? thread.latestActivityText
        guard !latestConclusion.isEmpty else { return nil }

        return DistilledArtifactDraft(
            kind: .note,
            title: "\(thread.title) output",
            summary: latestConclusion,
            pathOrURL: inferredPathOrURL(from: latestConclusion) ?? ""
        )
    }

    func saveDistilledArtifact(issueID: UUID, threadID: UUID) {
        guard let draft = distilledArtifactDraft(for: threadID) else { return }
        addArtifact(
            issueID: issueID,
            threadID: threadID,
            kind: draft.kind,
            title: draft.title,
            summary: draft.summary,
            pathOrURL: draft.pathOrURL
        )
    }

    func distilledFollowUpIssueDraft(for threadID: UUID) -> DistilledIssueDraft? {
        if let followUp = agentDistillationByThreadID[threadID]?.followUp {
            return followUp
        }
        guard let thread = thread(for: threadID),
              let issue = issue(for: thread.issueID)
        else {
            return nil
        }

        let action = latestPlanSummary(in: threadID)
            ?? latestToolSummary(in: threadID)
            ?? latestAgentConclusion(in: threadID)
            ?? thread.latestActivityText
        let trimmedAction = action.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedAction.isEmpty else { return nil }

        return DistilledIssueDraft(
            title: "Follow up: \(thread.title)",
            summary: [
                "Parent issue: #\(issue.number) \(issue.title)",
                "Source thread: \(thread.title)",
                trimmedAction
            ].joined(separator: "\n\n"),
            status: .todo,
            priority: issue.priority,
            assignees: thread.participants
        )
    }

    func createDistilledFollowUpIssue(sourceIssueID: UUID, threadID: UUID) {
        guard let draft = distilledFollowUpIssueDraft(for: threadID),
              let projectID = projectID(forIssueID: sourceIssueID),
              let createdIssueID = addIssue(
                to: projectID,
                title: draft.title,
                summary: draft.summary,
                sourceIssueID: sourceIssueID,
                sourceThreadID: threadID,
                status: draft.status,
                priority: draft.priority,
                assignees: draft.assignees
              )
        else {
            return
        }

        selectedProjectID = projectID
        selectedIssueID = createdIssueID
        selectedThreadID = nil
    }

    func refreshDistillationWithAgent(issueID: UUID, threadID: UUID) async {
        guard !distillingThreadIDs.contains(threadID),
              let issue = issue(for: issueID),
              let thread = thread(for: threadID),
              let distiller = preferredDistillerAgent()
        else {
            return
        }

        distillingThreadIDs.insert(threadID)
        defer {
            distillingThreadIDs.remove(threadID)
        }

        let distillerIdentifier = distiller.daemonAgentID ?? distiller.name
        let templateFamily = projectTemplateFamily(forIssueID: issue.id)
        let prompt = buildDistillationPrompt(
            issue: issue,
            thread: thread,
            templateFamily: templateFamily,
            distillerIdentifier: distillerIdentifier
        )
        let templateVersion = DistillationPromptTemplate.currentTemplateVersion(
            family: templateFamily,
            agentIdentifier: distillerIdentifier
        )

        do {
            let message = try await PrototypeDaemonAgentBridge.runOneShotPrompt(
                agentID: distiller.daemonAgentID ?? distiller.name.lowercased(),
                title: "Distill \(thread.title)",
                content: prompt,
                endpoint: daemonEndpoint
            )
            guard let response = message?.response,
                  let parsed = parseAgentDistillation(
                    response,
                    issue: issue,
                    thread: thread,
                    sourceAgentName: distiller.name,
                    templateVersion: templateVersion
                  )
            else {
                return
            }

            agentDistillationByThreadID[threadID] = parsed
            schedulePersistence()
        } catch {
            return
        }
    }

    func seed() {
        let claude = AgentProfile(
            id: UUID(),
            daemonAgentID: "claude",
            name: "Claude",
            kind: .claude,
            accent: .blue,
            isOnline: true,
            capabilityTags: ["Reasoning", "Review", "Refactor"],
            shortDescription: "Strong repo analysis and implementation planning."
        )
        let codex = AgentProfile(
            id: UUID(),
            daemonAgentID: "codex",
            name: "Codex",
            kind: .codex,
            accent: .green,
            isOnline: true,
            capabilityTags: ["Codegen", "Tests", "Diff Review"],
            shortDescription: "Fast implementation and test-oriented iteration."
        )
        let pi = AgentProfile(
            id: UUID(),
            daemonAgentID: "pi",
            name: "Pi",
            kind: .pi,
            accent: .purple,
            isOnline: true,
            capabilityTags: ["Memory", "Distill", "Summaries"],
            shortDescription: "Summarizes trajectories and turns sessions into reusable knowledge."
        )

        agents = [claude, codex, pi]

        let issueA = Issue(
            id: UUID(),
            number: 128,
            title: "Review relay reconnect recovery",
            summary: "Stress the reconnect path and clarify what needs to survive channel reactivation.",
            status: .review,
            priority: .high,
            assignees: [participant(from: claude), participant(from: codex)],
            latestActivityText: "Codex suggested a replay-safe resubscribe path.",
            sessionCount: 2,
            threadCount: 2,
            totalActiveSeconds: 18 * 60,
            updatedAt: Date().addingTimeInterval(-120)
        )

        let issueB = Issue(
            id: UUID(),
            number: 135,
            title: "Fix ws shutdown cleanup race",
            summary: "Cancel in-flight prompts before session teardown so disconnect cleanup is deterministic.",
            status: .inProgress,
            priority: .urgent,
            assignees: [participant(from: claude)],
            latestActivityText: "Claude is checking disconnect cleanup ordering.",
            sessionCount: 1,
            threadCount: 2,
            totalActiveSeconds: 12 * 60,
            updatedAt: Date().addingTimeInterval(-240)
        )

        let issueC = Issue(
            id: UUID(),
            number: 141,
            title: "Distill session knowledge into skills",
            summary: "Turn repeated debugging patterns into reusable markdown skills.",
            status: .todo,
            priority: .medium,
            assignees: [participant(from: pi)],
            latestActivityText: "Pi is ready to convert transcripts into project skills.",
            sessionCount: 1,
            threadCount: 1,
            totalActiveSeconds: 7 * 60,
            updatedAt: Date().addingTimeInterval(-900)
        )

        let project = Project(
            id: UUID(),
            name: "AgentChat",
            repoPath: "~/projects/agentchat",
            color: .blue,
            issues: [issueA, issueB, issueC]
        )

        projects = [project]
        selectedProjectID = project.id
        selectedIssueID = issueA.id

        let relayResearchThread = Thread(
            id: UUID(),
            issueID: issueA.id,
            title: "Research reconnect behavior",
            purpose: .research,
            participants: [participant(from: claude), participant(from: codex)],
            createdAt: Date().addingTimeInterval(-20 * 60),
            updatedAt: Date().addingTimeInterval(-620),
            state: .completed,
            latestActivityText: "Mapped the secure channel activation path."
        )

        let relayReviewThread = Thread(
            id: UUID(),
            issueID: issueA.id,
            title: "Review replay handling",
            purpose: .review,
            participants: [participant(from: codex)],
            createdAt: Date().addingTimeInterval(-18 * 60),
            updatedAt: Date().addingTimeInterval(-120),
            state: .active,
            latestActivityText: "Suggested rehydrate hooks after secure channel activation."
        )

        let shutdownDebugThread = Thread(
            id: UUID(),
            issueID: issueB.id,
            title: "Debug shutdown path",
            purpose: .debugging,
            participants: [participant(from: claude)],
            createdAt: Date().addingTimeInterval(-12 * 60),
            updatedAt: Date().addingTimeInterval(-240),
            state: .active,
            latestActivityText: "Reading daemon/server/src/ws.rs"
        )

        let shutdownReviewThread = Thread(
            id: UUID(),
            issueID: issueB.id,
            title: "Review cancellation edge cases",
            purpose: .review,
            participants: [participant(from: claude)],
            createdAt: Date().addingTimeInterval(-9 * 60),
            updatedAt: Date().addingTimeInterval(-500),
            state: .idle,
            latestActivityText: "Ready to inspect teardown ordering."
        )

        let skillThread = Thread(
            id: UUID(),
            issueID: issueC.id,
            title: "Plan skill distillation",
            purpose: .summary,
            participants: [participant(from: pi)],
            createdAt: Date().addingTimeInterval(-7 * 60),
            updatedAt: Date().addingTimeInterval(-500),
            state: .idle,
            latestActivityText: "Ready to distill a finished session."
        )

        threads = [
            relayResearchThread,
            relayReviewThread,
            shutdownDebugThread,
            shutdownReviewThread,
            skillThread
        ]
        selectedThreadID = relayReviewThread.id

        artifacts = [
            IssueArtifact(
                id: UUID(),
                issueID: issueA.id,
                threadID: relayReviewThread.id,
                kind: .changedFile,
                title: "daemon/server/src/relay.rs",
                summary: "Replay-safe rehydrate path now stays scoped to the active thread lifecycle.",
                pathOrURL: "daemon/server/src/relay.rs",
                createdAt: Date().addingTimeInterval(-180)
            ),
            IssueArtifact(
                id: UUID(),
                issueID: issueB.id,
                threadID: shutdownDebugThread.id,
                kind: .testLog,
                title: "Shutdown cleanup notes",
                summary: "Disconnect cleanup still races if the session mapping is released before cancellation settles.",
                pathOrURL: nil,
                createdAt: Date().addingTimeInterval(-260)
            )
        ]

        decisions = [
            IssueDecision(
                id: UUID(),
                issueID: issueA.id,
                threadID: relayReviewThread.id,
                title: "Thread owns replay context",
                rationale: "Replay handling should attach to thread state so Issue history stays explainable after daemon session changes.",
                createdAt: Date().addingTimeInterval(-150)
            ),
            IssueDecision(
                id: UUID(),
                issueID: issueC.id,
                threadID: skillThread.id,
                title: "Keep distillation manual in MVP",
                rationale: "We should capture explicit, human-reviewed notes first before auto-distilling every completed thread.",
                createdAt: Date().addingTimeInterval(-420)
            )
        ]

        sessions = [
            WorkspaceSession(
                id: UUID(),
                issueID: issueA.id,
                threadID: relayReviewThread.id,
                title: relayReviewThread.title,
                state: .waitingInput,
                agentName: "Codex",
                startedAt: Date().addingTimeInterval(-18 * 60),
                elapsedSeconds: 18 * 60,
                latestEventText: "Suggested rehydrate hooks after secure channel activation.",
                activeToolName: nil
            ),
            WorkspaceSession(
                id: UUID(),
                issueID: issueB.id,
                threadID: shutdownDebugThread.id,
                title: shutdownDebugThread.title,
                state: .running,
                agentName: "Claude",
                startedAt: Date().addingTimeInterval(-12 * 60),
                elapsedSeconds: 12 * 60,
                latestEventText: "Reading daemon/server/src/ws.rs",
                activeToolName: "read_file"
            ),
            WorkspaceSession(
                id: UUID(),
                issueID: issueC.id,
                threadID: skillThread.id,
                title: skillThread.title,
                state: .idle,
                agentName: "Pi",
                startedAt: Date().addingTimeInterval(-7 * 60),
                elapsedSeconds: 7 * 60,
                latestEventText: "Ready to distill a finished session.",
                activeToolName: nil
            )
        ]

        timelineByThread = [
            relayResearchThread.id: [
                TimelineItem(
                    id: UUID(),
                    issueID: issueA.id,
                    threadID: relayResearchThread.id,
                    sessionID: nil,
                    timestamp: Date().addingTimeInterval(-700),
                    payload: .system(SystemEvent(text: "Research reconnect behavior started."))
                ),
                TimelineItem(
                    id: UUID(),
                    issueID: issueA.id,
                    threadID: relayResearchThread.id,
                    sessionID: nil,
                    timestamp: Date().addingTimeInterval(-660),
                    payload: .agentMessage(
                        ChatMessage(
                            senderName: "Codex",
                            senderRole: .agent(.codex),
                            text: "I think reconnect recovery should preserve channel identity assumptions and replay protection semantics.",
                            isStreaming: false
                        )
                    )
                ),
                TimelineItem(
                    id: UUID(),
                    issueID: issueA.id,
                    threadID: relayResearchThread.id,
                    sessionID: nil,
                    timestamp: Date().addingTimeInterval(-620),
                    payload: .toolCall(
                        ToolCallEvent(
                            agentName: "Claude",
                            toolName: "read_file",
                            title: "Read daemon/server/src/relay.rs",
                            status: .completed,
                            contentPreview: "Secure channel activation and encrypted envelope path."
                        )
                    )
                )
            ],
            relayReviewThread.id: [
                TimelineItem(
                    id: UUID(),
                    issueID: issueA.id,
                    threadID: relayReviewThread.id,
                    sessionID: sessions.first(where: { $0.threadID == relayReviewThread.id })?.id,
                    timestamp: Date().addingTimeInterval(-220),
                    payload: .system(SystemEvent(text: "Review replay handling started."))
                ),
                TimelineItem(
                    id: UUID(),
                    issueID: issueA.id,
                    threadID: relayReviewThread.id,
                    sessionID: sessions.first(where: { $0.threadID == relayReviewThread.id })?.id,
                    timestamp: Date().addingTimeInterval(-180),
                    payload: .agentMessage(
                        ChatMessage(
                            senderName: "Codex",
                            senderRole: .agent(.codex),
                            text: "The replay path should be attached to thread state, not only the raw daemon session, so Issue history stays reconstructable.",
                            isStreaming: false
                        )
                    )
                )
            ],
            shutdownDebugThread.id: [
                TimelineItem(
                    id: UUID(),
                    issueID: issueB.id,
                    threadID: shutdownDebugThread.id,
                    sessionID: sessions.first(where: { $0.threadID == shutdownDebugThread.id })?.id,
                    timestamp: Date().addingTimeInterval(-320),
                    payload: .thinking(
                        ThinkingEvent(
                            agentName: "Claude",
                            text: "I am checking whether shutdown removes the session mapping before the cancellation path settles."
                        )
                    )
                ),
                TimelineItem(
                    id: UUID(),
                    issueID: issueB.id,
                    threadID: shutdownDebugThread.id,
                    sessionID: sessions.first(where: { $0.threadID == shutdownDebugThread.id })?.id,
                    timestamp: Date().addingTimeInterval(-300),
                    payload: .toolCall(
                        ToolCallEvent(
                            agentName: "Claude",
                            toolName: "read_file",
                            title: "Read daemon/server/src/ws.rs",
                            status: .inProgress,
                            contentPreview: nil
                        )
                    )
                )
            ],
            shutdownReviewThread.id: [
                TimelineItem(
                    id: UUID(),
                    issueID: issueB.id,
                    threadID: shutdownReviewThread.id,
                    sessionID: nil,
                    timestamp: Date().addingTimeInterval(-500),
                    payload: .system(SystemEvent(text: "Review cancellation edge cases is ready."))
                )
            ],
            skillThread.id: [
                TimelineItem(
                    id: UUID(),
                    issueID: issueC.id,
                    threadID: skillThread.id,
                    sessionID: sessions.first(where: { $0.threadID == skillThread.id })?.id,
                    timestamp: Date().addingTimeInterval(-500),
                    payload: .plan(
                        PlanEvent(
                            agentName: "Pi",
                            title: "Distillation pass",
                            steps: [
                                "Load transcript from .agentchat/sessions",
                                "Extract project-specific conventions",
                                "Write shared and agent-specific markdown skills into .agentchat/skills"
                            ]
                        )
                    )
                )
            ]
        ]
    }

    private func participant(from agent: AgentProfile) -> ParticipantRef {
        ParticipantRef(
            id: agent.id,
            displayName: agent.name,
            role: .agent(agent.kind),
            accent: agent.accent
        )
    }

    private func simulateResponses(threadID: UUID, input: String, targets: [String]) async {
        guard let thread = thread(for: threadID) else { return }
        let issueID = thread.issueID

        for target in targets {
            let sessionID = ensureSession(issueID: issueID, threadID: threadID, agentName: target)
            setSessionState(sessionID, state: .running, latestEventText: "Thinking about your request", activeToolName: nil)

            append(
                payload: .thinking(
                    ThinkingEvent(
                        agentName: target,
                        text: thinkingText(for: target, input: input)
                    )
                ),
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID
            )

            await pause(milliseconds: 280)

            let tool = toolScenario(for: target, input: input)
            append(
                payload: .toolCall(
                    ToolCallEvent(
                        agentName: target,
                        toolName: tool.name,
                        title: tool.title,
                        status: .inProgress,
                        contentPreview: nil
                    )
                ),
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID
            )
            setSessionState(sessionID, state: .running, latestEventText: tool.title, activeToolName: tool.name)

            await pause(milliseconds: 320)

            await streamAgentMessage(
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID,
                agentName: target,
                text: responseText(for: target, input: input)
            )

            append(
                payload: .toolCall(
                    ToolCallEvent(
                        agentName: target,
                        toolName: tool.name,
                        title: tool.title,
                        status: .completed,
                        contentPreview: tool.preview
                    )
                ),
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID
            )

            append(
                payload: .turnEnd(
                    TurnEndEvent(
                        agentName: target,
                        reason: "EndTurn"
                    )
                ),
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID
            )

            let finalPreview = responseText(for: target, input: input)
            setSessionState(sessionID, state: .completed, latestEventText: finalPreview, activeToolName: nil)
            updateThread(threadID: threadID) { thread in
                thread.state = .completed
                thread.latestActivityText = "\(target): \(finalPreview)"
                thread.updatedAt = Date()
            }
            updateIssue(issueID: issueID) { issue in
                issue.status = .review
                issue.latestActivityText = "\(target): \(finalPreview)"
                issue.updatedAt = Date()
                issue.sessionCount = sessions(for: issueID).count
                issue.totalActiveSeconds += 90
            }

            await pause(milliseconds: 180)
        }
    }

    private func streamAgentMessage(issueID: UUID, threadID: UUID, sessionID: UUID, agentName: String, text: String) async {
        let itemID = UUID()
        append(
            itemID: itemID,
            payload: .agentMessage(
                ChatMessage(
                    senderName: agentName,
                    senderRole: .agent(agentKind(for: agentName)),
                    text: "",
                    isStreaming: true
                )
            ),
            issueID: issueID,
            threadID: threadID,
            sessionID: sessionID
        )

        var current = ""
        for chunk in chunks(from: text) {
            await pause(milliseconds: 140)
            current += chunk
            updateTimelineItem(threadID: threadID, itemID: itemID) { item in
                guard case .agentMessage(var message) = item.payload else { return }
                message.text = current
                message.isStreaming = true
                item.payload = .agentMessage(message)
            }
            setSessionState(sessionID, state: .running, latestEventText: current, activeToolName: nil)
        }

        updateTimelineItem(threadID: threadID, itemID: itemID) { item in
            guard case .agentMessage(var message) = item.payload else { return }
            message.isStreaming = false
            item.payload = .agentMessage(message)
        }
    }

    private func append(itemID: UUID = UUID(), payload: TimelinePayload, issueID: UUID, threadID: UUID, sessionID: UUID?) {
        var items = timelineByThread[threadID] ?? []
        items.append(
            TimelineItem(
                id: itemID,
                issueID: issueID,
                threadID: threadID,
                sessionID: sessionID,
                timestamp: Date(),
                payload: payload
            )
        )
        timelineByThread[threadID] = items
    }

    private func updateTimelineItem(threadID: UUID, itemID: UUID, mutate: (inout TimelineItem) -> Void) {
        guard var items = timelineByThread[threadID], let index = items.firstIndex(where: { $0.id == itemID }) else {
            return
        }
        mutate(&items[index])
        timelineByThread[threadID] = items
    }

    private func updateIssue(issueID: UUID, mutate: (inout Issue) -> Void) {
        for projectIndex in projects.indices {
            guard let issueIndex = projects[projectIndex].issues.firstIndex(where: { $0.id == issueID }) else {
                continue
            }
            mutate(&projects[projectIndex].issues[issueIndex])
            return
        }
    }

    private func updateThread(threadID: UUID, mutate: (inout Thread) -> Void) {
        guard let index = threads.firstIndex(where: { $0.id == threadID }) else { return }
        mutate(&threads[index])
    }

    private func ensureSession(issueID: UUID, threadID: UUID, agentName: String) -> UUID {
        if let existing = sessions.first(where: { $0.threadID == threadID && $0.agentName == agentName }) {
            return existing.id
        }

        let session = WorkspaceSession(
            id: UUID(),
            issueID: issueID,
            threadID: threadID,
            title: "\(agentName) session",
            state: .idle,
            agentName: agentName,
            startedAt: Date(),
            elapsedSeconds: 0,
            latestEventText: "Ready",
            activeToolName: nil
        )
        sessions.insert(session, at: 0)
        updateIssue(issueID: issueID) { issue in
            issue.sessionCount = sessions(for: issueID).count
        }
        return session.id
    }

    private func setSessionState(_ sessionID: UUID, state: SessionState, latestEventText: String, activeToolName: String?) {
        guard let index = sessions.firstIndex(where: { $0.id == sessionID }) else { return }
        sessions[index].state = state
        sessions[index].latestEventText = latestEventText
        sessions[index].activeToolName = activeToolName
        sessions[index].elapsedSeconds += 12
    }

    private func thinkingText(for agentName: String, input: String) -> String {
        switch agentName {
        case "Claude":
            return "I am tracing the repo structure around \"\(input)\" and looking for the highest-risk edge cases."
        case "Codex":
            return "I am scanning for the fastest implementation path and what should be validated with tests."
        case "Pi":
            return "I am collecting reusable patterns and trying to compress them into stable project memory."
        default:
            return "I am thinking through the next step."
        }
    }

    private func responseText(for agentName: String, input: String) -> String {
        switch agentName {
        case "Claude":
            return "I inspected the workflow around \"\(input)\". The safest next step is to preserve session ownership until cancellation settles, then flush a final turn marker so the UI can close the task cleanly."
        case "Codex":
            return "For \"\(input)\", I would add a compact state machine: create session, stream updates, mark completion, and only then recycle the mapping. That makes reconnect and replay behavior easier to reason about."
        case "Pi":
            return "The reusable lesson from \"\(input)\" is that session transcripts, distilled skills, and relay behavior should all mirror the same lifecycle so issue history stays explainable."
        default:
            return "I finished analyzing \"\(input)\" and have a draft next step ready."
        }
    }

    private func toolScenario(for agentName: String, input: String) -> (name: String, title: String, preview: String) {
        switch agentName {
        case "Claude":
            return (
                name: "read_file",
                title: "Read daemon/server/src/ws.rs",
                preview: "Disconnect cleanup should cancel first, then release session state."
            )
        case "Codex":
            return (
                name: "cargo_test",
                title: "Run cargo test",
                preview: "WebSocket and relay tests still pass after the lifecycle adjustment."
            )
        case "Pi":
            return (
                name: "distill_session",
                title: "Distill transcript into skills",
                preview: "Updated shared/memory-layer.md and agents/pi/distillation-notes.md from the latest session."
            )
        default:
            return (
                name: "analyze",
                title: "Analyze issue context",
                preview: "Collected enough context for a first draft."
            )
        }
    }

    private func agentSpecificSkillFileName(for agentName: String) -> String {
        switch agentName {
        case "Claude":
            return "review-playbook.md"
        case "Codex":
            return "fast-paths.md"
        case "Pi":
            return "distillation-notes.md"
        default:
            return "private-notes.md"
        }
    }

    private func agentSpecificSkillSummary(for agentName: String, issueNumber: Int) -> String {
        switch agentName {
        case "Claude":
            return "Claude-specific review heuristics collected while working through #\(issueNumber)."
        case "Codex":
            return "Codex-specific implementation shortcuts and validation loops from issue #\(issueNumber)."
        case "Pi":
            return "Pi-specific distillation prompts and memory-shaping notes learned on #\(issueNumber)."
        default:
            return "Agent-local notes that should not be injected into every session."
        }
    }

    private func agentSkillPathSegment(for agentName: String) -> String {
        agentName.lowercased().replacingOccurrences(of: " ", with: "-")
    }

    private func normalizedOptionalString(_ value: String?) -> String? {
        guard let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty else {
            return nil
        }
        return trimmed
    }

    private func chunks(from text: String, wordsPerChunk: Int = 4) -> [String] {
        let words = text.split(separator: " ")
        var result: [String] = []
        var index = 0

        while index < words.count {
            let end = min(index + wordsPerChunk, words.count)
            let chunk = words[index..<end].joined(separator: " ")
            if end == words.count {
                result.append(chunk)
            } else {
                result.append(chunk + " ")
            }
            index = end
        }

        return result
    }

    private func agentKind(for agentName: String) -> AgentKind {
        agents.first(where: { $0.name == agentName })?.kind ?? .claude
    }

    private func pause(milliseconds: UInt64) async {
        try? await Task.sleep(nanoseconds: milliseconds * 1_000_000)
    }

    private func attachRemoteThreadIfPossible(localThreadID: UUID, daemonAgentID: String) async {
        guard let thread = thread(for: localThreadID) else { return }

        do {
            let handle = try await PrototypeDaemonAgentBridge.createRemoteThread(
                title: thread.title,
                agentID: daemonAgentID,
                endpoint: daemonEndpoint
            )
            updateThread(threadID: localThreadID) { thread in
                thread.daemonThreadID = handle.threadID
                thread.latestActivityText = "Connected to daemon thread \(handle.threadID)"
                thread.updatedAt = Date()
            }
            append(
                payload: .system(SystemEvent(text: "Connected to local daemon thread \(handle.threadID).")),
                issueID: thread.issueID,
                threadID: localThreadID,
                sessionID: nil
            )
        } catch {
            append(
                payload: .system(SystemEvent(text: "Daemon thread setup failed. Using prototype mode instead.")),
                issueID: thread.issueID,
                threadID: localThreadID,
                sessionID: nil
            )
        }
    }

    private func sendRemoteMessageIfPossible(
        localThreadID: UUID,
        daemonThreadID: String,
        input: String,
        fallbackTargets: [String]
    ) async {
        guard let thread = thread(for: localThreadID) else { return }

        do {
            let assistantMessages = try await PrototypeDaemonAgentBridge.sendRemoteThreadMessage(
                threadID: daemonThreadID,
                content: input,
                endpoint: daemonEndpoint
            )

            if assistantMessages.isEmpty {
                await simulateResponses(threadID: localThreadID, input: input, targets: fallbackTargets)
                return
            }

            for message in assistantMessages {
                if !message.thinking.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    append(
                        payload: .thinking(
                            ThinkingEvent(
                                agentName: displayName(forDaemonAgentID: message.agentID),
                                text: message.thinking
                            )
                        ),
                        issueID: thread.issueID,
                        threadID: localThreadID,
                        sessionID: nil
                    )
                }

                if let planBody = message.planBody,
                   !planBody.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    append(
                        payload: .plan(
                            PlanEvent(
                                agentName: displayName(forDaemonAgentID: message.agentID),
                                title: "Daemon plan update",
                                steps: planBody
                                    .split(separator: "\n")
                                    .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
                                    .filter { !$0.isEmpty }
                            )
                        ),
                        issueID: thread.issueID,
                        threadID: localThreadID,
                        sessionID: nil
                    )
                }

                for tool in message.toolActivities {
                    append(
                        payload: .toolCall(
                            ToolCallEvent(
                                agentName: displayName(forDaemonAgentID: message.agentID),
                                toolName: tool.id,
                                title: tool.title,
                                status: toolStatus(from: tool.status),
                                contentPreview: tool.content
                            )
                        ),
                        issueID: thread.issueID,
                        threadID: localThreadID,
                        sessionID: nil
                    )
                }

                append(
                    payload: .agentMessage(
                        ChatMessage(
                            senderName: displayName(forDaemonAgentID: message.agentID),
                            senderRole: .agent(agentKindForDaemonAgentID(message.agentID)),
                            text: message.response,
                            isStreaming: false
                        )
                    ),
                    issueID: thread.issueID,
                    threadID: localThreadID,
                    sessionID: nil
                )

                append(
                    payload: .turnEnd(
                        TurnEndEvent(
                            agentName: displayName(forDaemonAgentID: message.agentID),
                            reason: message.stopReason ?? "EndTurn"
                        )
                    ),
                    issueID: thread.issueID,
                    threadID: localThreadID,
                    sessionID: nil
                )

                updateThread(threadID: localThreadID) { thread in
                    thread.state = .completed
                    thread.latestActivityText = "\(displayName(forDaemonAgentID: message.agentID)): \(message.response)"
                    thread.updatedAt = Date()
                }
                updateIssue(issueID: thread.issueID) { issue in
                    issue.status = .review
                    issue.latestActivityText = "\(displayName(forDaemonAgentID: message.agentID)): \(message.response)"
                    issue.updatedAt = Date()
                }
            }
        } catch {
            append(
                payload: .system(SystemEvent(text: "Daemon message failed. Falling back to prototype response flow.")),
                issueID: thread.issueID,
                threadID: localThreadID,
                sessionID: nil
            )
            await simulateResponses(threadID: localThreadID, input: input, targets: fallbackTargets)
        }
    }

    private func replayTimelineItems(
        _ entries: [PrototypeRemoteTimelineEntry],
        issueID: UUID,
        threadID: UUID
    ) -> [TimelineItem] {
        let sortedEntries = entries.sorted { lhs, rhs in
            if lhs.threadSeq == rhs.threadSeq {
                return replaySortOrder(for: lhs.kind) < replaySortOrder(for: rhs.kind)
            }
            return lhs.threadSeq < rhs.threadSeq
        }

        let anchor = Date()
        return sortedEntries.enumerated().map { index, entry in
            TimelineItem(
                id: UUID(),
                issueID: issueID,
                threadID: threadID,
                sessionID: nil,
                timestamp: anchor.addingTimeInterval(Double(index)),
                payload: timelinePayload(from: entry.kind)
            )
        }
    }

    private func timelinePayload(from kind: PrototypeRemoteTimelineKind) -> TimelinePayload {
        switch kind {
        case .userMessage(let senderName, let content):
            return .userMessage(
                ChatMessage(
                    senderName: senderName,
                    senderRole: .human,
                    text: content,
                    isStreaming: false
                )
            )
        case .thinking(let agentID, let text):
            return .thinking(
                ThinkingEvent(
                    agentName: displayName(forDaemonAgentID: agentID),
                    text: text
                )
            )
        case .plan(let agentID, let body):
            let steps = body
                .split(separator: "\n")
                .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
            return .plan(
                PlanEvent(
                    agentName: displayName(forDaemonAgentID: agentID),
                    title: "Daemon plan update",
                    steps: steps.isEmpty ? [body] : steps
                )
            )
        case .tool(let agentID, let activity):
            return .toolCall(
                ToolCallEvent(
                    agentName: displayName(forDaemonAgentID: agentID),
                    toolName: activity.id,
                    title: activity.title,
                    status: toolStatus(from: activity.status),
                    contentPreview: activity.content
                )
            )
        case .assistantMessage(let agentID, let content):
            return .agentMessage(
                ChatMessage(
                    senderName: displayName(forDaemonAgentID: agentID),
                    senderRole: .agent(agentKindForDaemonAgentID(agentID)),
                    text: content,
                    isStreaming: false
                )
            )
        case .turnEnd(let agentID, let reason):
            return .turnEnd(
                TurnEndEvent(
                    agentName: displayName(forDaemonAgentID: agentID),
                    reason: reason
                )
            )
        case .system(let text):
            return .system(SystemEvent(text: text))
        }
    }

    private func replaySortOrder(for kind: PrototypeRemoteTimelineKind) -> Int {
        switch kind {
        case .system:
            return 0
        case .userMessage:
            return 1
        case .thinking:
            return 2
        case .plan:
            return 3
        case .tool:
            return 4
        case .assistantMessage:
            return 5
        case .turnEnd:
            return 6
        }
    }

    private func latestReplayPreview(from items: [TimelineItem]) -> String? {
        for item in items.reversed() {
            switch item.payload {
            case .turnEnd:
                continue
            default:
                return item.payload.summaryText
            }
        }
        return items.last?.payload.summaryText
    }

    private func distilledSummary(for threadID: UUID) -> String {
        guard let thread = thread(for: threadID) else { return "" }

        var paragraphs: [String] = []
        if let userIntent = latestUserIntent(in: threadID) {
            paragraphs.append("Current objective: \(userIntent)")
        }
        if let plan = latestPlanSummary(in: threadID) {
            paragraphs.append("Latest plan: \(plan)")
        }
        if let toolSummary = latestToolSummary(in: threadID) {
            paragraphs.append("Key execution detail: \(toolSummary)")
        }
        if let conclusion = latestAgentConclusion(in: threadID) {
            paragraphs.append("Latest outcome: \(conclusion)")
        } else if !thread.latestActivityText.isEmpty {
            paragraphs.append("Latest outcome: \(thread.latestActivityText)")
        }

        return paragraphs.joined(separator: "\n\n")
    }

    private func latestUserIntent(in threadID: UUID) -> String? {
        timeline(forThreadID: threadID).reversed().compactMap { item in
            if case .userMessage(let message) = item.payload {
                return trimmedSingleLine(message.text)
            }
            return nil
        }.first
    }

    private func latestAgentConclusion(in threadID: UUID) -> String? {
        timeline(forThreadID: threadID).reversed().compactMap { item in
            if case .agentMessage(let message) = item.payload {
                return trimmedSingleLine(message.text)
            }
            return nil
        }.first
    }

    private func latestToolSummary(in threadID: UUID) -> String? {
        timeline(forThreadID: threadID).reversed().compactMap { item in
            if case .toolCall(let event) = item.payload {
                let preview = trimmedSingleLine(event.contentPreview ?? "")
                if let preview, !preview.isEmpty {
                    return "\(event.title): \(preview)"
                }
                return event.title
            }
            return nil
        }.first
    }

    private func latestPlanSummary(in threadID: UUID) -> String? {
        timeline(forThreadID: threadID).reversed().compactMap { item in
            if case .plan(let event) = item.payload {
                let steps = event.steps
                    .map(trimmedSingleLine)
                    .compactMap { $0 }
                if steps.isEmpty {
                    return event.title
                }
                return ([event.title] + steps).joined(separator: " | ")
            }
            return nil
        }.first
    }

    private func inferredPathOrURL(from text: String?) -> String? {
        guard let text else { return nil }
        let tokens = text.split(whereSeparator: \.isWhitespace)
        for token in tokens {
            let candidate = String(token).trimmingCharacters(in: CharacterSet(charactersIn: ".,()[]{}<>\"'"))
            if isLikelyPathOrURL(candidate) {
                return candidate
            }
        }
        return nil
    }

    private func isLikelyPathOrURL(_ candidate: String) -> Bool {
        guard !candidate.isEmpty else { return false }

        let lowercased = candidate.lowercased()
        if lowercased.hasPrefix("http://") || lowercased.hasPrefix("https://") {
            return true
        }

        if lowercased == "and/or" || lowercased == "n/a" || lowercased == "w/o" {
            return false
        }
        if candidate.range(of: #"^\d{1,4}/\d{1,2}(/\d{1,4})?$"#, options: .regularExpression) != nil {
            return false
        }

        if candidate.hasPrefix("/") || candidate.hasPrefix("~/") || candidate.hasPrefix("./") || candidate.hasPrefix("../") {
            return candidate.count > 1
        }

        guard candidate.contains("/") else { return false }
        guard candidate.range(of: #"^[A-Za-z0-9_~.+@/\-#%]+$"#, options: .regularExpression) != nil else {
            return false
        }

        let segments = candidate.split(separator: "/", omittingEmptySubsequences: false)
        guard segments.count >= 2, !segments.contains(where: { $0.isEmpty }) else { return false }

        let pathAnchors: Set<String> = [
            "app", "apps", "assets", "client", "clients", "core", "daemon", "doc", "docs",
            "lib", "package", "packages", "server", "source", "sources", "src", "test", "tests"
        ]
        if segments.contains(where: { pathAnchors.contains($0.lowercased()) }) {
            return true
        }

        if candidate.range(of: #"\.[A-Za-z0-9]{1,8}($|[#?])"#, options: .regularExpression) != nil {
            return true
        }

        return segments.count >= 3
    }

    private func trimmedSingleLine(_ text: String) -> String? {
        let trimmed = text
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private func preferredDistillerAgent() -> AgentProfile? {
        agents.first(where: { $0.daemonAgentID == "pi" && $0.isOnline }) ??
        agents.first(where: { agent in
            agent.isOnline &&
            agent.daemonAgentID != nil &&
            agent.capabilityTags.joined(separator: " ").localizedCaseInsensitiveContains("distill")
        }) ??
        agents.first(where: { $0.isOnline && $0.daemonAgentID != nil })
    }

    private func buildDistillationPrompt(
        issue: Issue,
        thread: Thread,
        templateFamily: DistillationTemplateFamily = .default,
        distillerIdentifier: String? = nil
    ) -> String {
        let transcriptLines = timeline(forThreadID: thread.id)
            .map { item in
                switch item.payload {
                case .system(let event):
                    return "SYSTEM: \(event.text)"
                case .userMessage(let message):
                    return "USER: \(message.text)"
                case .agentMessage(let message):
                    return "\(message.senderName.uppercased()): \(message.text)"
                case .thinking(let event):
                    return "\(event.agentName.uppercased()) THINKING: \(event.text)"
                case .toolCall(let event):
                    return "\(event.agentName.uppercased()) TOOL: \(event.title) [\(event.status.title)] \(event.contentPreview ?? "")"
                case .plan(let event):
                    return "\(event.agentName.uppercased()) PLAN: \(([event.title] + event.steps).joined(separator: " | "))"
                case .turnEnd(let event):
                    return "\(event.agentName.uppercased()) END: \(event.reason)"
                }
            }

        return DistillationPromptTemplate.render(
            issueNumber: issue.number,
            issueTitle: issue.title,
            issueSummary: issue.summary,
            threadTitle: thread.title,
            threadPurpose: thread.purpose.title,
            transcriptLines: transcriptLines,
            family: templateFamily,
            agentIdentifier: distillerIdentifier
        )
    }

    private func parseAgentDistillation(
        _ response: String,
        issue: Issue,
        thread: Thread,
        sourceAgentName: String,
        templateVersion: String
    ) -> AgentDistillationResult? {
        let jsonCandidates = extractJSONObjects(from: response)
        let candidateTexts = jsonCandidates.isEmpty ? [response] : jsonCandidates
        guard let envelope = candidateTexts.lazy.compactMap({ jsonText -> AgentDistillationEnvelope? in
            guard let data = jsonText.data(using: .utf8) else { return nil }
            return try? JSONDecoder().decode(AgentDistillationEnvelope.self, from: data)
        }).first else {
            return nil
        }

        let summary = normalizedOptionalString(envelope.summary)

        let decision: DistilledDecisionDraft? = {
            guard let payload = envelope.decision,
                  let title = normalizedOptionalString(payload.title),
                  let rationale = normalizedOptionalString(payload.rationale)
            else {
                return nil
            }
            return DistilledDecisionDraft(title: title, rationale: rationale)
        }()

        let artifact: DistilledArtifactDraft? = {
            guard let payload = envelope.artifact,
                  let title = normalizedOptionalString(payload.title),
                  let summary = normalizedOptionalString(payload.summary)
            else {
                return nil
            }
            let kind = IssueArtifactKind(rawValue: payload.kind ?? "") ?? .note
            return DistilledArtifactDraft(
                kind: kind,
                title: title,
                summary: summary,
                pathOrURL: normalizedOptionalString(payload.pathOrURL) ?? ""
            )
        }()

        let followUp: DistilledIssueDraft? = {
            guard let payload = envelope.followUp,
                  let title = normalizedOptionalString(payload.title),
                  let summary = normalizedOptionalString(payload.summary)
            else {
                return nil
            }
            let priority = IssuePriority(rawValue: payload.priority ?? "") ?? issue.priority
            return DistilledIssueDraft(
                title: title,
                summary: summary,
                status: .todo,
                priority: priority,
                assignees: thread.participants
            )
        }()

        if summary == nil, decision == nil, artifact == nil, followUp == nil {
            return nil
        }

        return AgentDistillationResult(
            summary: summary,
            decision: decision,
            artifact: artifact,
            followUp: followUp,
            sourceAgentName: sourceAgentName,
            templateVersion: templateVersion,
            generatedAt: Date()
        )
    }

    private func extractJSONObject(from text: String) -> String? {
        extractJSONObjects(from: text).first
    }

    private func extractJSONObjects(from text: String) -> [String] {
        var objects: [String] = []
        var searchStart = text.startIndex

        while searchStart < text.endIndex {
            guard let start = text[searchStart...].firstIndex(of: "{") else {
                break
            }

            var index = start
            var depth = 0
            var isInString = false
            var isEscaped = false
            var balancedEnd: String.Index?

            while index < text.endIndex {
                let character = text[index]
                if isInString {
                    if isEscaped {
                        isEscaped = false
                    } else if character == "\\" {
                        isEscaped = true
                    } else if character == "\"" {
                        isInString = false
                    }
                } else if character == "\"" {
                    isInString = true
                } else if character == "{" {
                    depth += 1
                } else if character == "}" {
                    depth -= 1
                    if depth == 0 {
                        balancedEnd = index
                        break
                    }
                }

                index = text.index(after: index)
            }

            if let balancedEnd {
                objects.append(String(text[start...balancedEnd]))
                searchStart = text.index(after: balancedEnd)
            } else {
                searchStart = text.index(after: start)
            }
        }

        return objects
    }


    private static func stableAgentUUID(for agentID: String) -> UUID {
        let digest = SHA256.hash(data: Data(agentID.utf8))
        let bytes = Array(digest.prefix(16))
        let uuid = uuid_t(bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15])
        return UUID(uuid: uuid)
    }

    private static func agentKind(for agent: PrototypeDaemonAgentWire) -> AgentKind {
        let token = "\(agent.agentID) \(agent.kind) \(agent.name)".lowercased()
        if token.contains("claude") {
            return .claude
        }
        if token.contains("codex") {
            return .codex
        }
        if token.contains("pi") {
            return .pi
        }
        if token.contains("opencode") || (token.contains("open") && token.contains("code")) {
            return .opencode
        }
        return .human
    }

    private static func accentToken(for agent: PrototypeDaemonAgentWire) -> ColorToken {
        switch agentKind(for: agent) {
        case .claude:
            return .blue
        case .codex:
            return .green
        case .pi:
            return .purple
        case .opencode:
            return .orange
        case .human:
            return .gray
        }
    }

    private func displayName(forDaemonAgentID daemonAgentID: String) -> String {
        agents.first(where: { $0.daemonAgentID == daemonAgentID })?.name
            ?? humanizedDaemonAgentID(daemonAgentID)
    }

    private func agentKindForDaemonAgentID(_ daemonAgentID: String) -> AgentKind {
        agents.first(where: { $0.daemonAgentID == daemonAgentID })?.kind
            ?? Self.agentKind(for: PrototypeDaemonAgentWire(
                agentID: daemonAgentID,
                name: daemonAgentID,
                kind: daemonAgentID,
                status: "online",
                capabilities: []
            ))
    }

    private func humanizedDaemonAgentID(_ daemonAgentID: String) -> String {
        daemonAgentID
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "_", with: " ")
            .split(separator: " ")
            .map { $0.capitalized }
            .joined(separator: " ")
    }

    private func toolStatus(from status: String) -> ToolStatus {
        switch status.lowercased() {
        case "queued":
            return .queued
        case "failed":
            return .failed
        case "completed":
            return .completed
        default:
            return .inProgress
        }
    }

    private func refreshDaemonBackedThreads() async {
        let daemonThreadIDs = threads.compactMap { thread in
            thread.daemonThreadID == nil ? nil : thread.id
        }
        for threadID in daemonThreadIDs {
            await refreshThreadFromDaemon(threadID: threadID)
        }
    }

    private func schedulePersistence() {
        guard !isHydratingSnapshot else { return }
        persistenceTask?.cancel()
        persistenceTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 250_000_000)
            guard let self else { return }
            self.persistSnapshot()
        }
    }

    private func persistSnapshot() {
        guard !isHydratingSnapshot else { return }

        do {
            let data = try JSONEncoder.prototypeStoreEncoder.encode(currentSnapshot())
            try writeSnapshotData(data)
            UserDefaults.standard.removeObject(forKey: legacySnapshotKey)
        } catch {
            print("Failed to persist snapshot: \(error)")
        }
    }

    private func restoreSnapshot() -> Bool {
        do {
            let fileURL = try snapshotFileURL()
            if FileManager.default.fileExists(atPath: fileURL.path) {
                do {
                    let data = try Data(contentsOf: fileURL)
                    let snapshot = try JSONDecoder.prototypeStoreDecoder.decode(DemoStoreSnapshot.self, from: data)
                    return applySnapshot(snapshot)
                } catch {
                    print("Failed to restore snapshot: \(error)")
                    preserveInvalidSnapshot(at: fileURL)
                    return restoreLegacySnapshot()
                }
            }
        } catch {
            print("Failed to locate snapshot file: \(error)")
        }

        return restoreLegacySnapshot()
    }

    private func restoreLegacySnapshot() -> Bool {
        guard let data = UserDefaults.standard.data(forKey: legacySnapshotKey) else {
            return false
        }
        do {
            let snapshot = try JSONDecoder.prototypeStoreDecoder.decode(DemoStoreSnapshot.self, from: data)
            let restored = applySnapshot(snapshot)
            if restored {
                do {
                    try writeSnapshotData(data)
                    UserDefaults.standard.removeObject(forKey: legacySnapshotKey)
                } catch {
                    print("Failed to migrate snapshot from UserDefaults: \(error)")
                }
            }
            return restored
        } catch {
            print("Failed to restore legacy snapshot: \(error)")
            return false
        }
    }

    private func currentSnapshot() -> DemoStoreSnapshot {
        DemoStoreSnapshot(
            projects: projects,
            agents: agents,
            sessions: sessions,
            threads: threads,
            artifacts: artifacts,
            decisions: decisions,
            timelines: timelineByThread.map { PersistedThreadTimeline(threadID: $0.key, items: $0.value) }
                .sorted { $0.threadID.uuidString < $1.threadID.uuidString },
            selectedProjectID: selectedProjectID,
            selectedIssueID: selectedIssueID,
            selectedThreadID: selectedThreadID,
            agentCustomNames: agentCustomNames,
            agentAvatarData: agentAvatarData,
            agentDistillations: agentDistillationByThreadID
                .map { PersistedAgentDistillation(threadID: $0.key, result: $0.value) }
                .sorted { $0.threadID.uuidString < $1.threadID.uuidString }
        )
    }

    private func applySnapshot(_ snapshot: DemoStoreSnapshot) -> Bool {
        projects = snapshot.projects
        agents = snapshot.agents
        sessions = snapshot.sessions
        threads = snapshot.threads
        artifacts = snapshot.artifacts
        decisions = snapshot.decisions
        timelineByThread = Dictionary(
            uniqueKeysWithValues: snapshot.timelines.map { ($0.threadID, $0.items) }
        )
        selectedProjectID = snapshot.selectedProjectID
        selectedIssueID = snapshot.selectedIssueID
        selectedThreadID = snapshot.selectedThreadID
        agentCustomNames = snapshot.agentCustomNames
        agentAvatarData = snapshot.agentAvatarData
        agentDistillationByThreadID = Dictionary(
            uniqueKeysWithValues: (snapshot.agentDistillations ?? []).map { ($0.threadID, $0.result) }
        )
        return !projects.isEmpty
    }

    private func writeSnapshotData(_ data: Data) throws {
        let fileURL = try snapshotFileURL()
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true,
            attributes: nil
        )
        try data.write(to: fileURL, options: [.atomic])
    }

    private func snapshotFileURL() throws -> URL {
        guard let applicationSupportURL = FileManager.default.urls(
            for: .applicationSupportDirectory,
            in: .userDomainMask
        ).first else {
            throw NSError(
                domain: "AgentChatPrototype.DemoStore",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Application Support directory is unavailable."]
            )
        }
        return applicationSupportURL
            .appendingPathComponent(snapshotDirectoryName, isDirectory: true)
            .appendingPathComponent(snapshotFileName)
    }

    private func deleteSnapshotFile() {
        do {
            let fileURL = try snapshotFileURL()
            if FileManager.default.fileExists(atPath: fileURL.path) {
                try FileManager.default.removeItem(at: fileURL)
            }
        } catch {
            print("Failed to delete snapshot file: \(error)")
        }
    }

    private func preserveInvalidSnapshot(at fileURL: URL) {
        let backupName = "DemoStoreSnapshot.v1.invalid-\(Int(Date().timeIntervalSince1970))-\(UUID().uuidString).json"
        let backupURL = fileURL.deletingLastPathComponent().appendingPathComponent(backupName)
        do {
            try FileManager.default.moveItem(at: fileURL, to: backupURL)
        } catch {
            print("Failed to preserve invalid snapshot: \(error)")
        }
    }

    private func normalizeSelectionState() {
        if selectedProjectID == nil || project(for: selectedProjectID ?? UUID()) == nil {
            selectedProjectID = projects.first?.id
        }

        let allIssueIDs = Set(allIssues.map(\.id))
        if let selectedIssueID, !allIssueIDs.contains(selectedIssueID) {
            self.selectedIssueID = nil
        }
        if selectedIssueID == nil {
            selectedIssueID = currentProject?.issues.first?.id
        }

        let allThreadIDs = Set(threads.map(\.id))
        if let selectedThreadID, !allThreadIDs.contains(selectedThreadID) {
            self.selectedThreadID = nil
        }
        if selectedThreadID == nil, let selectedIssueID {
            self.selectedThreadID = threads(for: selectedIssueID).first?.id
        }
    }
}

typealias DemoStore = WorkspaceStore

private extension JSONEncoder {
    static var prototypeStoreEncoder: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }
}

private extension JSONDecoder {
    static var prototypeStoreDecoder: JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }
}
