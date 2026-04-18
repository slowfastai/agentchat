import Foundation
import SwiftUI

@MainActor
final class DemoStore: ObservableObject {
    @Published var projects: [Project] = []
    @Published var agents: [AgentProfile] = []
    @Published var sessions: [WorkspaceSession] = []
    @Published var threads: [Thread] = []
    @Published var timelineByThread: [UUID: [TimelineItem]] = [:]
    @Published var selectedProjectID: UUID?
    @Published var selectedIssueID: UUID?
    @Published var selectedThreadID: UUID?
    
    @Published var agentCustomNames: [String: String] = [:]
    @Published var agentAvatarData: [String: Data] = [:]
    @Published var connectingAgentIDs: Set<String> = []

    init() {
        seed()
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
        
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
            self?.connectingAgentIDs.remove(agentID)
            if let index = self?.agents.firstIndex(where: { $0.id.uuidString == agentID }) {
                self?.agents[index].isOnline = true
            }
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

    func threads(for issueID: UUID) -> [Thread] {
        threads.filter { $0.issueID == issueID }
            .sorted { $0.updatedAt > $1.updatedAt }
    }

    func createProject(name: String, repoPath: String, color: ColorToken = .blue) {
        let project = Project(
            id: UUID(),
            name: name,
            repoPath: repoPath,
            color: color,
            issues: []
        )
        projects.append(project)
        if selectedProjectID == nil {
            selectedProjectID = project.id
        }
    }

    func addIssue(to projectID: UUID, title: String, summary: String = "", status: IssueStatus = .backlog, priority: IssuePriority = .medium, assignees: [ParticipantRef] = []) {
        guard let projectIndex = projects.firstIndex(where: { $0.id == projectID }) else { return }
        
        let maxNumber = projects.flatMap(\.issues).map(\.number).max() ?? 0
        let issue = Issue(
            id: UUID(),
            number: maxNumber + 1,
            title: title,
            summary: summary,
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
        
        for threadID in deletedThreadIDs {
            timelineByThread.removeValue(forKey: threadID)
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
            await simulateResponses(threadID: threadID, input: trimmed, targets: resolvedTargets)
        }
    }

    func seed() {
        let claude = AgentProfile(
            id: UUID(),
            name: "Claude",
            kind: .claude,
            accent: .blue,
            isOnline: true,
            capabilityTags: ["Reasoning", "Review", "Refactor"],
            shortDescription: "Strong repo analysis and implementation planning."
        )
        let codex = AgentProfile(
            id: UUID(),
            name: "Codex",
            kind: .codex,
            accent: .green,
            isOnline: true,
            capabilityTags: ["Codegen", "Tests", "Diff Review"],
            shortDescription: "Fast implementation and test-oriented iteration."
        )
        let pi = AgentProfile(
            id: UUID(),
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
}
