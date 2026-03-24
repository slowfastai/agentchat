import Foundation

enum SidebarDestination: String, CaseIterable, Hashable, Identifiable {
    case inbox
    case switcher
    case agents

    var id: Self { self }

    var title: String {
        switch self {
        case .inbox: return "Issues"
        case .switcher: return "Switcher"
        case .agents: return "Agents"
        }
    }

    var systemImage: String {
        switch self {
        case .inbox: return "list.bullet.rectangle"
        case .switcher: return "square.grid.2x2"
        case .agents: return "person.2"
        }
    }
}

enum ColorToken: String, Hashable, CaseIterable {
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

struct Project: Identifiable, Hashable {
    let id: UUID
    var name: String
    var color: ColorToken
    var issues: [Issue]
}

struct Issue: Identifiable, Hashable {
    let id: UUID
    var number: Int
    var title: String
    var summary: String
    var status: IssueStatus
    var priority: IssuePriority
    var assignees: [ParticipantRef]
    var latestActivityText: String
    var sessionCount: Int
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
}

enum IssueStatus: String, CaseIterable, Hashable {
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

enum IssuePriority: String, CaseIterable, Hashable {
    case low
    case medium
    case high
    case urgent

    var title: String { rawValue.capitalized }
}

struct AgentProfile: Identifiable, Hashable {
    let id: UUID
    var name: String
    var kind: AgentKind
    var accent: ColorToken
    var isOnline: Bool
    var capabilityTags: [String]
    var shortDescription: String
}

enum AgentKind: String, CaseIterable, Hashable {
    case claude
    case codex
    case pi
    case opencode
    case human
}

struct ParticipantRef: Identifiable, Hashable {
    let id: UUID
    var displayName: String
    var role: ParticipantRole
    var accent: ColorToken
}

enum ParticipantRole: Hashable {
    case human
    case agent(AgentKind)
}

struct WorkspaceSession: Identifiable, Hashable {
    let id: UUID
    var issueID: UUID
    var title: String
    var state: SessionState
    var agentName: String
    var startedAt: Date
    var elapsedSeconds: Int
    var latestEventText: String
    var activeToolName: String?
}

enum SessionState: String, CaseIterable, Hashable {
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

struct TimelineItem: Identifiable, Hashable {
    let id: UUID
    var issueID: UUID
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

struct ChatMessage: Hashable {
    var senderName: String
    var senderRole: ParticipantRole
    var text: String
    var isStreaming: Bool
}

struct ThinkingEvent: Hashable {
    var agentName: String
    var text: String
}

struct ToolCallEvent: Hashable {
    var agentName: String
    var toolName: String
    var title: String
    var status: ToolStatus
    var contentPreview: String?
}

enum ToolStatus: String, CaseIterable, Hashable {
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

struct PlanEvent: Hashable {
    var agentName: String
    var title: String
    var steps: [String]
}

struct TurnEndEvent: Hashable {
    var agentName: String
    var reason: String
}

struct SystemEvent: Hashable {
    var text: String
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

struct SkillCardModel: Identifiable, Hashable {
    let id: UUID
    var title: String
    var summary: String
    var updatedAt: Date
}
