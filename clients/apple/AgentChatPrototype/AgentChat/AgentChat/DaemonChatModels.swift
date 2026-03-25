import Foundation

enum JSONValue: Codable, Hashable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: JSONValue])
    case array([JSONValue])
    case null

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unsupported JSON value")
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value): try container.encode(value)
        case .number(let value): try container.encode(value)
        case .bool(let value): try container.encode(value)
        case .object(let value): try container.encode(value)
        case .array(let value): try container.encode(value)
        case .null: try container.encodeNil()
        }
    }

    func prettyPrintedString() -> String {
        let object = foundationObject
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(
                  withJSONObject: object,
                  options: [.prettyPrinted, .sortedKeys]
              ),
              let string = String(data: data, encoding: .utf8)
        else {
            return String(describing: self)
        }
        return string
    }

    private var foundationObject: Any {
        switch self {
        case .string(let value):
            return value
        case .number(let value):
            return value
        case .bool(let value):
            return value
        case .object(let value):
            return value.mapValues { $0.foundationObject }
        case .array(let value):
            return value.map(\.foundationObject)
        case .null:
            return NSNull()
        }
    }
}

struct DaemonAgentSummary: Codable, Identifiable, Hashable {
    let agentID: String
    let name: String
    let kind: String
    let status: String
    let defaultWorkingDir: String?
    let capabilities: [String]

    enum CodingKeys: String, CodingKey {
        case agentID = "agent_id"
        case name
        case kind
        case status
        case defaultWorkingDir = "default_working_dir"
        case capabilities
    }

    var id: String { agentID }
    var isOnline: Bool { status == "online" }
    var isOffline: Bool { status == "offline" }

    func withStatus(_ status: String) -> Self {
        Self(
            agentID: agentID,
            name: name,
            kind: kind,
            status: status,
            defaultWorkingDir: defaultWorkingDir,
            capabilities: capabilities
        )
    }
}

struct DaemonThreadSummary: Codable, Identifiable, Hashable {
    let threadID: String
    let title: String?
    let workingDir: String
    let createdAtMS: UInt64
    let state: String
    let participantCount: Int
    let lastThreadSeq: UInt64

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case title
        case workingDir = "working_dir"
        case createdAtMS = "created_at_ms"
        case state
        case participantCount = "participant_count"
        case lastThreadSeq = "last_thread_seq"
    }

    var id: String { threadID }
}

struct DaemonThreadParticipant: Codable, Identifiable, Hashable {
    let participantID: String
    let kind: String
    let displayName: String
    let agentID: String?
    let sessionID: String?
    let state: String

    enum CodingKeys: String, CodingKey {
        case participantID = "participant_id"
        case kind
        case displayName = "display_name"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case state
    }

    var id: String { participantID }
    var isAgent: Bool { kind == "agent" }
}

struct DaemonThreadSnapshot: Codable, Hashable {
    let threadID: String
    let title: String?
    let workingDir: String
    let createdAtMS: UInt64
    let lastThreadSeq: UInt64
    let participants: [DaemonThreadParticipant]

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case title
        case workingDir = "working_dir"
        case createdAtMS = "created_at_ms"
        case lastThreadSeq = "last_thread_seq"
        case participants
    }
}

struct DaemonThreadSender: Codable, Hashable {
    let kind: String
    let participantID: String
    let displayName: String

    enum CodingKeys: String, CodingKey {
        case kind
        case participantID = "participant_id"
        case displayName = "display_name"
    }
}

struct DaemonEnvelope: Decodable {
    let type: String
}

struct AgentListEvent: Decodable {
    let agents: [DaemonAgentSummary]
}

struct ThreadCreatedEvent: Decodable {
    let threadID: String
    let createdAtMS: UInt64

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case createdAtMS = "created_at_ms"
    }
}

struct ThreadListEvent: Decodable {
    let threads: [DaemonThreadSummary]
}

struct ThreadAttachedEvent: Decodable {
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
    }
}

struct ThreadSnapshotEvent: Decodable {
    let snapshot: DaemonThreadSnapshot
}

struct ThreadClosedEvent: Decodable {
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
    }
}

struct ThreadReplayCompleteEvent: Decodable {
    let threadID: String
    let lastThreadSeq: UInt64

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case lastThreadSeq = "last_thread_seq"
    }
}

struct ThreadParticipantAddedEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participant: DaemonThreadParticipant

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participant
    }
}

struct ThreadParticipantRemovedEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participantID: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participantID = "participant_id"
    }
}

struct ThreadMessageEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let messageID: String
    let sender: DaemonThreadSender
    let content: String
    let targetParticipantIDs: [String]

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case messageID = "message_id"
        case sender
        case content
        case targetParticipantIDs = "target_participant_ids"
    }
}

struct ThreadAssistantMessageEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let messageID: String
    let turnID: String
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let thinking: String
    let response: String
    let state: String
    let stopReason: String?

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case messageID = "message_id"
        case turnID = "turn_id"
        case participantID = "participant_id"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case sessionEventSeq = "session_event_seq"
        case thinking
        case response
        case state
        case stopReason = "stop_reason"
    }
}

struct ThreadAgentDeltaEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let content: String
    let deltaType: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participantID = "participant_id"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case sessionEventSeq = "session_event_seq"
        case content
        case deltaType = "delta_type"
    }
}

struct ThreadAgentToolUpdateEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let toolCallID: String
    let title: String
    let status: String
    let content: String?

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participantID = "participant_id"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case sessionEventSeq = "session_event_seq"
        case toolCallID = "tool_call_id"
        case title
        case status
        case content
    }
}

struct ThreadAgentPlanUpdateEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let planJSON: JSONValue

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participantID = "participant_id"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case sessionEventSeq = "session_event_seq"
        case planJSON = "plan_json"
    }
}

struct ThreadAgentTurnEndEvent: Decodable {
    let threadID: String
    let threadSeq: UInt64
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let stopReason: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case participantID = "participant_id"
        case agentID = "agent_id"
        case sessionID = "session_id"
        case sessionEventSeq = "session_event_seq"
        case stopReason = "stop_reason"
    }
}

struct ErrorEvent: Decodable {
    let code: String
    let message: String
}

struct ListAgentsRequest: Encodable {
    let type = "list_agents"
}

struct ListThreadsRequest: Encodable {
    let type = "list_threads"
}

struct CreateThreadRequest: Encodable {
    let type = "create_thread"
    let title: String?
    let workingDir: String

    enum CodingKeys: String, CodingKey {
        case type
        case title
        case workingDir = "working_dir"
    }
}

struct AttachThreadRequest: Encodable {
    let type = "attach_thread"
    let threadID: String
    let afterSeq: UInt64?

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case afterSeq = "after_seq"
    }
}

struct AddThreadParticipantRequest: Encodable {
    let type = "add_thread_participant"
    let threadID: String
    let agentID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case agentID = "agent_id"
    }
}

struct CloseThreadRequest: Encodable {
    let type = "close_thread"
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
    }
}

struct SendThreadMessageRequest: Encodable {
    let type = "send_thread_message"
    let threadID: String
    let content: String
    let targetParticipantIDs: [String]?

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case content
        case targetParticipantIDs = "target_participant_ids"
    }
}

struct DaemonToolActivity: Identifiable, Hashable {
    let id: String
    let title: String
    let status: String
    let content: String?
}

struct DaemonTimelineEntry: Identifiable, Hashable {
    enum Kind: String, Hashable {
        case user
        case assistantTurn
        case tool
        case plan
        case turnEnd
        case system
    }

    let id: String
    let sortThreadSeq: UInt64
    let lastThreadSeq: UInt64
    let kind: Kind
    let title: String
    let body: String
    let thinkingBody: String?
    let planBody: String?
    let toolActivities: [DaemonToolActivity]
    let status: String?
    let tintName: String

    init(
        threadID: String,
        threadSeq: UInt64,
        kind: Kind,
        title: String,
        body: String,
        thinkingBody: String? = nil,
        planBody: String? = nil,
        toolActivities: [DaemonToolActivity] = [],
        status: String? = nil,
        tintName: String
    ) {
        self.id = "\(threadID)-\(threadSeq)-\(kind.rawValue)"
        self.sortThreadSeq = threadSeq
        self.lastThreadSeq = threadSeq
        self.kind = kind
        self.title = title
        self.body = body
        self.thinkingBody = thinkingBody
        self.planBody = planBody
        self.toolActivities = toolActivities
        self.status = status
        self.tintName = tintName
    }

    init(
        id: String,
        sortThreadSeq: UInt64,
        lastThreadSeq: UInt64,
        kind: Kind,
        title: String,
        body: String,
        thinkingBody: String? = nil,
        planBody: String? = nil,
        toolActivities: [DaemonToolActivity] = [],
        status: String? = nil,
        tintName: String
    ) {
        self.id = id
        self.sortThreadSeq = sortThreadSeq
        self.lastThreadSeq = lastThreadSeq
        self.kind = kind
        self.title = title
        self.body = body
        self.thinkingBody = thinkingBody
        self.planBody = planBody
        self.toolActivities = toolActivities
        self.status = status
        self.tintName = tintName
    }
}

struct AssistantTurnKey: Hashable {
    let threadID: String
    let sessionID: String
}

struct AssistantTurnState: Equatable {
    let threadID: String
    let sessionID: String
    let entryID: String
    let sortThreadSeq: UInt64
    var agentID: String
    var lastThreadSeq: UInt64
    var thinking: String
    var response: String
    var planBody: String?
    var toolActivities: [DaemonToolActivity]
    var status: String

    init(
        threadID: String,
        sessionID: String,
        threadSeq: UInt64,
        agentID: String,
        status: String = "streaming"
    ) {
        self.threadID = threadID
        self.sessionID = sessionID
        self.entryID = "assistant-turn-\(threadID)-\(sessionID)-\(threadSeq)"
        self.sortThreadSeq = threadSeq
        self.agentID = agentID
        self.lastThreadSeq = threadSeq
        self.thinking = ""
        self.response = ""
        self.planBody = nil
        self.toolActivities = []
        self.status = status
    }

    init(delta event: ThreadAgentDeltaEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            threadSeq: event.threadSeq,
            agentID: event.agentID
        )
        append(delta: event)
    }

    init(snapshot event: ThreadAssistantMessageEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            threadSeq: event.threadSeq,
            agentID: event.agentID,
            status: event.state
        )
    }

    init(toolUpdate event: ThreadAgentToolUpdateEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            threadSeq: event.threadSeq,
            agentID: event.agentID
        )
    }

    init(planUpdate event: ThreadAgentPlanUpdateEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            threadSeq: event.threadSeq,
            agentID: event.agentID
        )
    }

    var isTerminal: Bool {
        status == "completed" || status == "failed"
    }

    mutating func merge(snapshot event: ThreadAssistantMessageEvent) {
        lastThreadSeq = max(lastThreadSeq, event.threadSeq)
        agentID = event.agentID
        thinking = event.thinking
        response = event.response
        status = event.state
    }

    mutating func append(delta event: ThreadAgentDeltaEvent) {
        lastThreadSeq = max(lastThreadSeq, event.threadSeq)
        agentID = event.agentID
        switch event.deltaType {
        case "thinking":
            thinking.append(event.content)
        case "text":
            response.append(event.content)
        default:
            break
        }
        if !isTerminal {
            status = "streaming"
        }
    }

    mutating func updatePlan(_ event: ThreadAgentPlanUpdateEvent) {
        lastThreadSeq = max(lastThreadSeq, event.threadSeq)
        agentID = event.agentID
        planBody = event.planJSON.prettyPrintedString()
        if !isTerminal {
            status = "streaming"
        }
    }

    mutating func upsertTool(_ event: ThreadAgentToolUpdateEvent) {
        lastThreadSeq = max(lastThreadSeq, event.threadSeq)
        agentID = event.agentID

        if let index = toolActivities.firstIndex(where: { $0.id == event.toolCallID }) {
            let existing = toolActivities[index]
            toolActivities[index] = DaemonToolActivity(
                id: event.toolCallID,
                title: event.title.isEmpty ? existing.title : event.title,
                status: event.status.isEmpty ? existing.status : event.status,
                content: event.content ?? existing.content
            )
        } else {
            toolActivities.append(
                DaemonToolActivity(
                    id: event.toolCallID,
                    title: event.title,
                    status: event.status,
                    content: event.content
                )
            )
        }

        if !isTerminal {
            status = "streaming"
        }
    }

    mutating func finish(turnEnd event: ThreadAgentTurnEndEvent) {
        lastThreadSeq = max(lastThreadSeq, event.threadSeq)
        agentID = event.agentID
        status = "completed"
    }

    func timelineEntry(tintName: String) -> DaemonTimelineEntry {
        DaemonTimelineEntry(
            id: entryID,
            sortThreadSeq: sortThreadSeq,
            lastThreadSeq: lastThreadSeq,
            kind: .assistantTurn,
            title: agentID.capitalized,
            body: response,
            thinkingBody: thinking.isEmpty ? nil : thinking,
            planBody: planBody,
            toolActivities: toolActivities,
            status: status,
            tintName: tintName
        )
    }
}

struct AssistantTurnReducer {
    private(set) var activeStates: [AssistantTurnKey: AssistantTurnState] = [:]

    mutating func consume(snapshot event: ThreadAssistantMessageEvent) -> AssistantTurnState {
        let key = AssistantTurnKey(threadID: event.threadID, sessionID: event.sessionID)
        var state = activeStates[key] ?? AssistantTurnState(snapshot: event)
        state.merge(snapshot: event)
        if state.isTerminal {
            activeStates.removeValue(forKey: key)
        } else {
            activeStates[key] = state
        }
        return state
    }

    mutating func consume(delta event: ThreadAgentDeltaEvent) -> AssistantTurnState? {
        let key = AssistantTurnKey(threadID: event.threadID, sessionID: event.sessionID)

        if var state = activeStates[key] {
            state.append(delta: event)
            activeStates[key] = state
            return state
        }

        guard !event.content.isEmpty, event.deltaType == "thinking" || event.deltaType == "text" else {
            return nil
        }

        let state = AssistantTurnState(delta: event)
        activeStates[key] = state
        return state
    }

    mutating func consume(toolUpdate event: ThreadAgentToolUpdateEvent) -> AssistantTurnState {
        let key = AssistantTurnKey(threadID: event.threadID, sessionID: event.sessionID)
        var state = activeStates[key] ?? AssistantTurnState(toolUpdate: event)
        state.upsertTool(event)
        activeStates[key] = state
        return state
    }

    mutating func consume(planUpdate event: ThreadAgentPlanUpdateEvent) -> AssistantTurnState {
        let key = AssistantTurnKey(threadID: event.threadID, sessionID: event.sessionID)
        var state = activeStates[key] ?? AssistantTurnState(planUpdate: event)
        state.updatePlan(event)
        activeStates[key] = state
        return state
    }

    mutating func finish(turnEnd event: ThreadAgentTurnEndEvent) -> AssistantTurnState? {
        let key = AssistantTurnKey(threadID: event.threadID, sessionID: event.sessionID)
        guard var state = activeStates.removeValue(forKey: key) else {
            return nil
        }
        state.finish(turnEnd: event)
        return state
    }

    mutating func removeStates(for threadID: String) {
        activeStates = activeStates.filter { $0.key.threadID != threadID }
    }
}
