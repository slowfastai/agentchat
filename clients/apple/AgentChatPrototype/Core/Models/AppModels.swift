import Foundation
import SwiftUI

extension ColorToken {
    var color: Color {
        switch self {
        case .blue: return .blue
        case .purple: return .purple
        case .green: return .green
        case .orange: return .orange
        case .red: return .red
        case .gray: return .gray
        }
    }
}

enum SidebarDestination: String, CaseIterable, Hashable, Identifiable {
    case projects
    case inbox
    case switcher
    case agents

    var id: Self { self }

    var title: String {
        switch self {
        case .projects: return "Projects"
        case .inbox: return "Issues"
        case .switcher: return "Switcher"
        case .agents: return "Agents"
        }
    }

    var systemImage: String {
        switch self {
        case .projects: return "folder"
        case .inbox: return "list.bullet.rectangle"
        case .switcher: return "square.grid.2x2"
        case .agents: return "person.2"
        }
    }
}

enum ColorToken: String, Hashable, CaseIterable, Codable {
    case blue
    case purple
    case green
    case orange
    case red
    case gray
}

enum IssueFilter: String, CaseIterable, Hashable, Identifiable {
    case all
    case assignedToAgent
    case running
    case needsReview

    var id: Self { self }

    var title: String {
        switch self {
        case .all: return "All"
        case .assignedToAgent: return "Assigned"
        case .running: return "Running"
        case .needsReview: return "Needs Review"
        }
    }
}

enum SwitcherMode: String, CaseIterable, Hashable, Identifiable {
    case list
    case grid
    case focus

    var id: Self { self }

    var title: String { rawValue.capitalized }
}

enum DistillationTemplateFamily: String, CaseIterable, Hashable, Codable, Identifiable {
    case `default`
    case pi
    case claude
    case codex

    var id: Self { self }

    var title: String {
        switch self {
        case .default: return "Default"
        case .pi: return "Pi"
        case .claude: return "Claude"
        case .codex: return "Codex"
        }
    }
}

struct Project: Identifiable, Hashable, Codable {
    let id: UUID
    var name: String
    var repoPath: String
    var color: ColorToken
    var distillationTemplateFamily: DistillationTemplateFamily = .default
    var issues: [Issue]
}

struct Issue: Identifiable, Hashable, Codable {
    let id: UUID
    var number: Int
    var title: String
    var summary: String
    var sourceIssueID: UUID? = nil
    var sourceThreadID: UUID? = nil
    var status: IssueStatus
    var priority: IssuePriority
    var assignees: [ParticipantRef]
    var latestActivityText: String
    var sessionCount: Int
    var threadCount: Int
    var totalActiveSeconds: Int
    var updatedAt: Date
}

extension Issue {
    var agentNames: [String] {
        assignees.compactMap {
            switch $0.role {
            case .human:
                return nil
            case .agent:
                return $0.displayName
            }
        }
    }

    var isFollowUpIssue: Bool {
        sourceIssueID != nil
    }
}

enum IssueStatus: String, CaseIterable, Hashable, Codable {
    case backlog
    case todo
    case inProgress
    case blocked
    case review
    case done

    var title: String {
        switch self {
        case .backlog: return "Backlog"
        case .todo: return "Todo"
        case .inProgress: return "In Progress"
        case .blocked: return "Blocked"
        case .review: return "Review"
        case .done: return "Done"
        }
    }

    var badgeColor: ColorToken {
        switch self {
        case .backlog, .todo: return .gray
        case .inProgress: return .blue
        case .blocked: return .red
        case .review: return .orange
        case .done: return .green
        }
    }
}

enum IssuePriority: String, CaseIterable, Hashable, Codable {
    case low
    case medium
    case high
    case urgent

    var title: String { rawValue.capitalized }
}

struct AgentProfile: Identifiable, Hashable, Codable {
    let id: UUID
    var daemonAgentID: String?
    var name: String
    var kind: AgentKind
    var accent: ColorToken
    var isOnline: Bool
    var capabilityTags: [String]
    var shortDescription: String
}

enum AgentKind: String, CaseIterable, Hashable, Codable {
    case claude
    case codex
    case pi
    case opencode
    case human
}

extension AgentKind {
    var defaultAvatarAssetName: String? {
        switch self {
        case .codex:
            return "CodexAvatar"
        case .claude:
            return "ClaudeCodeAvatar"
        case .opencode:
            return "OpenCodeAvatar"
        case .pi:
            return "PiAvatar"
        case .human:
            return nil
        }
    }
}

struct ParticipantRef: Identifiable, Hashable, Codable {
    let id: UUID
    var displayName: String
    var role: ParticipantRole
    var accent: ColorToken
}

enum ParticipantRole: Hashable {
    case human
    case agent(AgentKind)
}

struct WorkspaceSession: Identifiable, Hashable, Codable {
    let id: UUID
    var issueID: UUID
    var threadID: UUID
    var title: String
    var state: SessionState
    var agentName: String
    var startedAt: Date
    var elapsedSeconds: Int
    var latestEventText: String
    var activeToolName: String?
}

enum SessionState: String, CaseIterable, Hashable, Codable {
    case idle
    case running
    case waitingInput
    case completed
    case failed

    var title: String {
        switch self {
        case .idle: return "Idle"
        case .running: return "Running"
        case .waitingInput: return "Waiting"
        case .completed: return "Completed"
        case .failed: return "Failed"
        }
    }

    var badgeColor: ColorToken {
        switch self {
        case .idle: return .gray
        case .running: return .blue
        case .waitingInput: return .orange
        case .completed: return .green
        case .failed: return .red
        }
    }
}

enum ThreadState: String, CaseIterable, Hashable, Codable {
    case idle
    case active
    case completed

    var title: String {
        switch self {
        case .idle: return "Idle"
        case .active: return "Active"
        case .completed: return "Completed"
        }
    }

    var badgeColor: ColorToken {
        switch self {
        case .idle: return .gray
        case .active: return .blue
        case .completed: return .green
        }
    }
}

enum ThreadPurpose: String, CaseIterable, Hashable, Identifiable, Codable {
    case discussion
    case research
    case implementation
    case review
    case debugging
    case testing
    case summary

    var id: Self { self }

    var title: String {
        switch self {
        case .discussion: return "Discussion"
        case .research: return "Research"
        case .implementation: return "Implementation"
        case .review: return "Review"
        case .debugging: return "Debugging"
        case .testing: return "Testing"
        case .summary: return "Summary"
        }
    }

    var badgeColor: ColorToken {
        switch self {
        case .discussion: return .blue
        case .research: return .purple
        case .implementation: return .green
        case .review: return .orange
        case .debugging: return .red
        case .testing: return .gray
        case .summary: return .purple
        }
    }
}

struct Thread: Identifiable, Hashable, Codable {
    let id: UUID
    var issueID: UUID
    var daemonThreadID: String?
    var title: String
    var purpose: ThreadPurpose
    var participants: [ParticipantRef]
    var createdAt: Date
    var updatedAt: Date
    var state: ThreadState
    var latestActivityText: String
}

extension Thread {
    var agentNames: [String] {
        participants.compactMap {
            switch $0.role {
            case .human:
                return nil
            case .agent:
                return $0.displayName
            }
        }
    }
}

struct TimelineItem: Identifiable, Hashable, Codable {
    let id: UUID
    var issueID: UUID
    var threadID: UUID
    var sessionID: UUID?
    var timestamp: Date
    var payload: TimelinePayload
}

enum TimelinePayload: Hashable {
    case system(SystemEvent)
    case userMessage(ChatMessage)
    case agentMessage(ChatMessage)
    case thinking(ThinkingEvent)
    case toolCall(ToolCallEvent)
    case plan(PlanEvent)
    case turnEnd(TurnEndEvent)
}

extension TimelinePayload {
    var summaryText: String {
        switch self {
        case .system(let event):
            return event.text
        case .userMessage(let message):
            return message.text
        case .agentMessage(let message):
            return message.text
        case .thinking(let event):
            return "Thinking: \(event.text)"
        case .toolCall(let event):
            return "\(event.title) · \(event.status.title)"
        case .plan(let event):
            return event.title
        case .turnEnd(let event):
            return "\(event.agentName) finished · \(event.reason)"
        }
    }
}

struct ChatMessage: Hashable, Codable {
    var senderName: String
    var senderRole: ParticipantRole
    var text: String
    var isStreaming: Bool
}

struct ThinkingEvent: Hashable, Codable {
    var agentName: String
    var text: String
}

struct ToolCallEvent: Hashable, Codable {
    var agentName: String
    var toolName: String
    var title: String
    var status: ToolStatus
    var contentPreview: String?
}

enum ToolStatus: String, CaseIterable, Hashable, Codable {
    case queued
    case inProgress
    case completed
    case failed

    var title: String {
        switch self {
        case .queued: return "Queued"
        case .inProgress: return "In Progress"
        case .completed: return "Completed"
        case .failed: return "Failed"
        }
    }

    var badgeColor: ColorToken {
        switch self {
        case .queued: return .gray
        case .inProgress: return .blue
        case .completed: return .green
        case .failed: return .red
        }
    }
}

struct PlanEvent: Hashable, Codable {
    var agentName: String
    var title: String
    var steps: [String]
}

struct TurnEndEvent: Hashable, Codable {
    var agentName: String
    var reason: String
}

struct SystemEvent: Hashable, Codable {
    var text: String
}

enum IssueArtifactKind: String, CaseIterable, Hashable, Identifiable, Codable {
    case branch
    case commit
    case pullRequest
    case changedFile
    case testLog
    case screenshot
    case document
    case note

    var id: Self { self }

    var title: String {
        switch self {
        case .branch: return "Branch"
        case .commit: return "Commit"
        case .pullRequest: return "Pull Request"
        case .changedFile: return "Changed File"
        case .testLog: return "Test Log"
        case .screenshot: return "Screenshot"
        case .document: return "Document"
        case .note: return "Note"
        }
    }

    var systemImage: String {
        switch self {
        case .branch: return "arrow.triangle.branch"
        case .commit: return "number"
        case .pullRequest: return "arrow.triangle.pull"
        case .changedFile: return "doc.text"
        case .testLog: return "checklist"
        case .screenshot: return "photo"
        case .document: return "doc.richtext"
        case .note: return "note.text"
        }
    }

    var accent: ColorToken {
        switch self {
        case .branch: return .green
        case .commit: return .orange
        case .pullRequest: return .blue
        case .changedFile: return .gray
        case .testLog: return .purple
        case .screenshot: return .orange
        case .document: return .blue
        case .note: return .gray
        }
    }
}

struct IssueArtifact: Identifiable, Hashable, Codable {
    let id: UUID
    var issueID: UUID
    var threadID: UUID?
    var kind: IssueArtifactKind
    var title: String
    var summary: String
    var pathOrURL: String?
    var createdAt: Date
}

struct IssueDecision: Identifiable, Hashable, Codable {
    let id: UUID
    var issueID: UUID
    var threadID: UUID?
    var title: String
    var rationale: String
    var createdAt: Date
}

struct WorkspaceCardModel: Identifiable, Hashable {
    let id: UUID
    var issueID: UUID
    var issueNumber: Int
    var title: String
    var participants: [String]
    var state: SessionState
    var latestPreview: String
    var activeTool: String?
    var elapsedSeconds: Int
}

struct ChatThreadSummary: Identifiable, Hashable {
    var issueID: UUID
    var issueNumber: Int
    var title: String
    var participants: [String]
    var preview: String
    var updatedAt: Date
    var unreadCount: Int
    var isPinned: Bool
    var state: SessionState
    var accent: ColorToken

    var id: UUID { issueID }
}

enum SkillScope: Hashable {
    case shared
    case agentSpecific(String)

    var id: String {
        switch self {
        case .shared:
            return "shared"
        case .agentSpecific(let agentName):
            return "agent-\(agentName)"
        }
    }

    var title: String {
        switch self {
        case .shared:
            return "Shared Memory"
        case .agentSpecific(let agentName):
            return "\(agentName) Notes"
        }
    }

    var subtitle: String {
        switch self {
        case .shared:
            return "Injected into every agent session as project-wide memory."
        case .agentSpecific(let agentName):
            return "Only injected when \(agentName) owns the daemon session."
        }
    }

    var sortRank: Int {
        switch self {
        case .shared:
            return 0
        case .agentSpecific:
            return 1
        }
    }

    var systemImage: String {
        switch self {
        case .shared:
            return "square.stack.3d.down.right.fill"
        case .agentSpecific:
            return "person.crop.rectangle.stack.fill"
        }
    }
}

struct SkillCardModel: Identifiable, Hashable {
    let id: UUID
    var title: String
    var path: String
    var summary: String
    var updatedAt: Date
    var scope: SkillScope
    var accent: ColorToken
}

extension ParticipantRole: Codable {
    private enum CodingKeys: String, CodingKey {
        case type
        case agentKind
    }

    private enum Kind: String, Codable {
        case human
        case agent
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(Kind.self, forKey: .type)
        switch type {
        case .human:
            self = .human
        case .agent:
            self = .agent(try container.decode(AgentKind.self, forKey: .agentKind))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .human:
            try container.encode(Kind.human, forKey: .type)
        case .agent(let kind):
            try container.encode(Kind.agent, forKey: .type)
            try container.encode(kind, forKey: .agentKind)
        }
    }
}

extension TimelinePayload: Codable {
    private enum CodingKeys: String, CodingKey {
        case type
        case system
        case userMessage
        case agentMessage
        case thinking
        case toolCall
        case plan
        case turnEnd
    }

    private enum Kind: String, Codable {
        case system
        case userMessage
        case agentMessage
        case thinking
        case toolCall
        case plan
        case turnEnd
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(Kind.self, forKey: .type)
        switch type {
        case .system:
            self = .system(try container.decode(SystemEvent.self, forKey: .system))
        case .userMessage:
            self = .userMessage(try container.decode(ChatMessage.self, forKey: .userMessage))
        case .agentMessage:
            self = .agentMessage(try container.decode(ChatMessage.self, forKey: .agentMessage))
        case .thinking:
            self = .thinking(try container.decode(ThinkingEvent.self, forKey: .thinking))
        case .toolCall:
            self = .toolCall(try container.decode(ToolCallEvent.self, forKey: .toolCall))
        case .plan:
            self = .plan(try container.decode(PlanEvent.self, forKey: .plan))
        case .turnEnd:
            self = .turnEnd(try container.decode(TurnEndEvent.self, forKey: .turnEnd))
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .system(let event):
            try container.encode(Kind.system, forKey: .type)
            try container.encode(event, forKey: .system)
        case .userMessage(let message):
            try container.encode(Kind.userMessage, forKey: .type)
            try container.encode(message, forKey: .userMessage)
        case .agentMessage(let message):
            try container.encode(Kind.agentMessage, forKey: .type)
            try container.encode(message, forKey: .agentMessage)
        case .thinking(let event):
            try container.encode(Kind.thinking, forKey: .type)
            try container.encode(event, forKey: .thinking)
        case .toolCall(let event):
            try container.encode(Kind.toolCall, forKey: .type)
            try container.encode(event, forKey: .toolCall)
        case .plan(let event):
            try container.encode(Kind.plan, forKey: .type)
            try container.encode(event, forKey: .plan)
        case .turnEnd(let event):
            try container.encode(Kind.turnEnd, forKey: .type)
            try container.encode(event, forKey: .turnEnd)
        }
    }
}
