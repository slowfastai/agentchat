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

enum DaemonAgentFamily: String, Hashable {
    case claude
    case codex
    case opencode
    case pi
    case human
    case generic

    init(agentID: String?, kind: String?, name: String?) {
        let tokens = Self.tokens(from: [agentID, kind, name])

        if tokens.contains("claude") {
            self = .claude
        } else if tokens.contains("codex") {
            self = .codex
        } else if tokens.contains("opencode") || (tokens.contains("open") && tokens.contains("code")) {
            self = .opencode
        } else if tokens.contains("pi") {
            self = .pi
        } else if tokens.contains("human") || tokens.contains("user") {
            self = .human
        } else {
            self = .generic
        }
    }

    var title: String {
        switch self {
        case .claude: return "Claude Code"
        case .codex: return "Codex"
        case .opencode: return "OpenCode"
        case .pi: return "Pi"
        case .human: return "Human"
        case .generic: return "Agent"
        }
    }

    var symbolName: String {
        switch self {
        case .claude:
            return "brain.head.profile"
        case .codex:
            return "curlybraces.square.fill"
        case .opencode:
            return "terminal.fill"
        case .pi:
            return "sparkles"
        case .human:
            return "person.fill"
        case .generic:
            return "person.crop.square"
        }
    }

    var defaultAvatarAssetName: String? {
        switch self {
        case .codex:
            return "CodexAvatar"
        case .opencode:
            return "OpenCodeAvatar"
        case .claude, .pi, .human, .generic:
            return nil
        }
    }

    var tintName: String {
        switch self {
        case .claude:
            return "blue"
        case .codex:
            return "green"
        case .opencode:
            return "orange"
        case .pi:
            return "purple"
        case .human:
            return "gray"
        case .generic:
            return "indigo"
        }
    }

    private static func tokens(from values: [String?]) -> Set<String> {
        Set(
            values
                .compactMap { $0?.lowercased() }
                .flatMap { value in
                    value.split { character in
                        !(character.isLetter || character.isNumber)
                    }
                }
                .map(String.init)
        )
    }
}

func humanizeAgentIdentifier(_ value: String) -> String {
    let replaced = value
        .replacingOccurrences(of: "_", with: " ")
        .replacingOccurrences(of: "-", with: " ")
        .trimmingCharacters(in: .whitespacesAndNewlines)

    guard !replaced.isEmpty else { return value }

    return replaced
        .split(separator: " ")
        .map { token in
            let lowercased = token.lowercased()
            if lowercased == "ai" || lowercased == "pi" {
                return lowercased.uppercased()
            }
            return lowercased.prefix(1).uppercased() + lowercased.dropFirst()
        }
        .joined(separator: " ")
}

private func normalizedMentionSearchToken(_ value: String) -> String {
    value
        .lowercased()
        .filter { $0.isLetter || $0.isNumber }
}

private func mentionHandleValue(_ value: String) -> String {
    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { return "agent" }

    var result = ""
    var previousWasSeparator = false
    for character in trimmed.lowercased() {
        if character.isLetter || character.isNumber || character == "-" || character == "_" || character == "." {
            result.append(character)
            previousWasSeparator = false
        } else if !previousWasSeparator {
            result.append("-")
            previousWasSeparator = true
        }
    }

    let cleaned = result.trimmingCharacters(in: CharacterSet(charactersIn: "-_."))
    return cleaned.isEmpty ? "agent" : cleaned
}

struct DaemonAgentSummary: Codable, Identifiable, Hashable {
    let agentID: String
    let name: String
    let mentionHandleOverride: String? = nil
    let kind: String
    let status: String
    let defaultWorkingDir: String?
    let capabilities: [String]
    var customDisplayName: String?
    var avatarImageData: Data?

    enum CodingKeys: String, CodingKey {
        case agentID = "agent_id"
        case name
        case mentionHandleOverride = "mention_handle"
        case kind
        case status
        case defaultWorkingDir = "default_working_dir"
        case capabilities
        case customDisplayName = "custom_display_name"
        case avatarImageData = "avatar_image_data"
    }

    var id: String { agentID }
    var mentionHandle: String {
        if let mentionHandleOverride,
           !mentionHandleOverride.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return mentionHandleOverride
        }
        return mentionHandleValue(agentID)
    }
    var isOnline: Bool { status == "online" }
    var isOffline: Bool { status == "offline" }
    var family: DaemonAgentFamily { DaemonAgentFamily(agentID: agentID, kind: kind, name: name) }
    var symbolName: String { family.symbolName }
    var tintName: String { family.tintName }
    var defaultAvatarAssetName: String? { family.defaultAvatarAssetName }
    var kindTitle: String { family.title }
    var displayName: String {
        if let customName = customDisplayName?.trimmingCharacters(in: .whitespacesAndNewlines), !customName.isEmpty {
            return customName
        }
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        if family != .generic {
            return family.title
        }
        return humanizeAgentIdentifier(agentID)
    }
    var capabilitySummary: String {
        if !capabilities.isEmpty {
            return capabilities.joined(separator: " · ")
        }
        return kindTitle
    }

    nonisolated func withStatus(_ status: String) -> Self {
        Self(
            agentID: agentID,
            name: name,
            kind: kind,
            status: status,
            defaultWorkingDir: defaultWorkingDir,
            capabilities: capabilities,
            customDisplayName: customDisplayName,
            avatarImageData: avatarImageData
        )
    }

    nonisolated func withCustomDisplayName(_ customName: String?) -> Self {
        var copy = self
        copy.customDisplayName = customName?.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty == true ? nil : customName
        return copy
    }

    nonisolated func withAvatarImageData(_ data: Data?) -> Self {
        var copy = self
        copy.avatarImageData = data
        return copy
    }

    nonisolated func applyingLocalCustomizations(from existing: Self?) -> Self {
        guard let existing else { return self }

        return Self(
            agentID: agentID,
            name: name,
            kind: kind,
            status: status,
            defaultWorkingDir: defaultWorkingDir,
            capabilities: capabilities,
            customDisplayName: existing.customDisplayName,
            avatarImageData: existing.avatarImageData
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
    let mentionHandleOverride: String? = nil
    let sessionID: String?
    let state: String

    enum CodingKeys: String, CodingKey {
        case participantID = "participant_id"
        case kind
        case displayName = "display_name"
        case agentID = "agent_id"
        case mentionHandleOverride = "mention_handle"
        case sessionID = "session_id"
        case state
    }

    var id: String { participantID }
    var isAgent: Bool { kind == "agent" }
    var family: DaemonAgentFamily { DaemonAgentFamily(agentID: agentID, kind: kind, name: displayName) }
    var tintName: String { family.tintName }
    var defaultAvatarAssetName: String? { family.defaultAvatarAssetName }
    var kindTitle: String { isAgent ? family.title : humanizeAgentIdentifier(kind) }
    var mentionHandle: String {
        if let mentionHandleOverride,
           !mentionHandleOverride.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return mentionHandleOverride
        }
        if let agentID, !agentID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return mentionHandleValue(agentID)
        }
        return mentionHandleValue(displayName)
    }

    func matchesMentionQuery(_ query: String) -> Bool {
        let normalizedQuery = normalizedMentionSearchToken(query)
        if normalizedQuery.isEmpty {
            return true
        }

        return normalizedMentionSearchToken(mentionHandle).hasPrefix(normalizedQuery)
            || normalizedMentionSearchToken(displayName).contains(normalizedQuery)
            || normalizedMentionSearchToken(kindTitle).contains(normalizedQuery)
    }
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
    let turnID: String?
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let content: String
    let deltaType: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case turnID = "turn_id"
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
    let turnID: String?
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
        case turnID = "turn_id"
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
    let turnID: String?
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let planJSON: JSONValue

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case turnID = "turn_id"
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
    let turnID: String?
    let participantID: String
    let agentID: String
    let sessionID: String
    let sessionEventSeq: UInt64
    let stopReason: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case turnID = "turn_id"
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

struct DaemonToolActivity: Codable, Identifiable, Hashable {
    let id: String
    let title: String
    let status: String
    let content: String?
}

struct AssistantExecutionSummary: Hashable {
    enum Tone: Hashable {
        case neutral
        case active
        case warning
        case failure
    }

    let headline: String
    let footnote: String?
    let detailLine: String?
    let tone: Tone
    let showsProgress: Bool

    var requiresAttention: Bool {
        tone == .warning || tone == .failure
    }
}

private func normalizedExecutionStatusToken(_ value: String) -> String {
    value
        .trimmingCharacters(in: .whitespacesAndNewlines)
        .replacingOccurrences(of: "_", with: "")
        .replacingOccurrences(of: "-", with: "")
        .replacingOccurrences(of: " ", with: "")
        .lowercased()
}

private func humanizedExecutionStatus(_ value: String) -> String {
    switch normalizedExecutionStatusToken(value) {
    case "", "update":
        return "Update"
    case "streaming", "inprogress", "running":
        return "Running"
    case "completed":
        return "Done"
    case "failed":
        return "Failed"
    case "needsapproval":
        return "Approval required"
    default:
        return value
            .replacingOccurrences(of: "_", with: " ")
            .replacingOccurrences(of: "-", with: " ")
            .capitalized
    }
}

private func pluralizedExecutionLabel(_ count: Int, singular: String) -> String {
    count == 1 ? "1 \(singular)" : "\(count) \(singular)s"
}

extension DaemonToolActivity {
    fileprivate var statusToken: String {
        normalizedExecutionStatusToken(status)
    }

    var displayTitle: String {
        let trimmed = title.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Tool" : trimmed
    }

    var displayStatus: String {
        humanizedExecutionStatus(status)
    }

    var isCompleted: Bool {
        statusToken == "completed"
    }

    var isFailed: Bool {
        statusToken == "failed"
    }

    var needsApproval: Bool {
        statusToken == "needsapproval"
    }

    var isRunning: Bool {
        statusToken == "streaming" || statusToken == "inprogress" || statusToken == "running"
    }
}

struct DaemonTimelineEntry: Codable, Identifiable, Hashable {
    enum Kind: String, Codable, Hashable {
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
    let agentID: String?
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
        agentID: String? = nil,
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
        self.agentID = agentID
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
        agentID: String? = nil,
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
        self.agentID = agentID
        self.title = title
        self.body = body
        self.thinkingBody = thinkingBody
        self.planBody = planBody
        self.toolActivities = toolActivities
        self.status = status
        self.tintName = tintName
    }
}

extension DaemonTimelineEntry {
    var hasThinkingBody: Bool {
        guard let thinkingBody else { return false }
        return !thinkingBody.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var hasPlanBody: Bool {
        guard let planBody else { return false }
        return !planBody.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var hasExecutionDetails: Bool {
        kind == .assistantTurn && (hasThinkingBody || hasPlanBody || !toolActivities.isEmpty)
    }

    var normalizedStatusToken: String {
        normalizedExecutionStatusToken(status ?? "")
    }

    var orderedToolActivities: [DaemonToolActivity] {
        toolActivities.sorted { lhs, rhs in
            let lhsRank = executionToolSortRank(lhs)
            let rhsRank = executionToolSortRank(rhs)
            if lhsRank != rhsRank {
                return lhsRank < rhsRank
            }

            let titleOrder = lhs.displayTitle.localizedCaseInsensitiveCompare(rhs.displayTitle)
            if titleOrder != .orderedSame {
                return titleOrder == .orderedAscending
            }

            return lhs.id.localizedCaseInsensitiveCompare(rhs.id) == .orderedAscending
        }
    }

    var executionSummary: AssistantExecutionSummary? {
        guard kind == .assistantTurn else { return nil }

        let runningTools = orderedToolActivities.filter(\.isRunning)
        let failedTools = orderedToolActivities.filter(\.isFailed)
        let approvalTools = orderedToolActivities.filter(\.needsApproval)
        let hasResponse = !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let metadataLabels = executionMetadataLabels(
            hasThinking: hasThinkingBody,
            hasPlan: hasPlanBody
        )
        let toolCountLabel = toolActivities.isEmpty
            ? nil
            : pluralizedExecutionLabel(toolActivities.count, singular: "tool")
        let detailLine = detailLineText(labels: metadataLabels)
        let detailLineWithToolCount = detailLineText(labels: metadataLabels + (toolCountLabel.map { [$0] } ?? []))

        if !approvalTools.isEmpty {
            return AssistantExecutionSummary(
                headline: approvalTools.count == 1 ? "Approval required" : "\(approvalTools.count) approvals required",
                footnote: approvalTools.first?.displayTitle,
                detailLine: detailLineWithToolCount,
                tone: .warning,
                showsProgress: normalizedStatusToken == "streaming"
            )
        }

        if !failedTools.isEmpty || normalizedStatusToken == "failed" {
            return AssistantExecutionSummary(
                headline: !failedTools.isEmpty
                    ? (failedTools.count == 1 ? "1 tool failed" : "\(failedTools.count) tools failed")
                    : "Response failed",
                footnote: failedTools.first?.displayTitle,
                detailLine: detailLineWithToolCount,
                tone: .failure,
                showsProgress: false
            )
        }

        if normalizedStatusToken == "streaming" {
            guard hasExecutionDetails else { return nil }

            let headline: String
            let footnote: String?

            if let activeTool = runningTools.first {
                headline = toolActivities.count > 1 ? "Running tools" : "Running tool"
                footnote = activeTool.displayTitle
            } else if hasThinkingBody && !hasResponse {
                headline = "Thinking..."
                footnote = nil
            } else {
                headline = "Working..."
                footnote = nil
            }

            return AssistantExecutionSummary(
                headline: headline,
                footnote: footnote,
                detailLine: detailLineWithToolCount,
                tone: .active,
                showsProgress: true
            )
        }

        guard hasExecutionDetails else { return nil }

        let headline: String
        if !toolActivities.isEmpty {
            headline = "Used \(pluralizedExecutionLabel(toolActivities.count, singular: "tool"))"
        } else if hasThinkingBody {
            headline = "Thought process"
        } else {
            headline = "Plan available"
        }

        return AssistantExecutionSummary(
            headline: headline,
            footnote: nil,
            detailLine: detailLine,
            tone: .neutral,
            showsProgress: false
        )
    }

    private func executionToolSortRank(_ activity: DaemonToolActivity) -> Int {
        if activity.needsApproval {
            return 0
        }
        if activity.isFailed {
            return 1
        }
        if activity.isRunning {
            return 2
        }
        if activity.isCompleted {
            return 4
        }
        return 3
    }

    private func executionMetadataLabels(hasThinking: Bool, hasPlan: Bool) -> [String] {
        var labels: [String] = []
        if hasThinking {
            labels.append("Thinking")
        }
        if hasPlan {
            labels.append("Plan")
        }
        return labels
    }

    private func detailLineText(labels: [String]) -> String? {
        labels.isEmpty ? nil : labels.joined(separator: " · ")
    }
}

private func assistantTurnIdentity(turnID: String?, sessionID: String) -> String {
    let trimmed = turnID?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return trimmed.isEmpty ? "session-\(sessionID)" : trimmed
}

struct AssistantTurnKey: Hashable {
    let threadID: String
    let turnIdentity: String
}

struct AssistantTurnState: Equatable {
    let threadID: String
    let sessionID: String
    let turnIdentity: String
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
        turnID: String?,
        threadSeq: UInt64,
        agentID: String,
        status: String = "streaming"
    ) {
        self.threadID = threadID
        self.sessionID = sessionID
        self.turnIdentity = assistantTurnIdentity(turnID: turnID, sessionID: sessionID)
        self.entryID = "assistant-turn-\(threadID)-\(turnIdentity)"
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
            turnID: event.turnID,
            threadSeq: event.threadSeq,
            agentID: event.agentID
        )
        append(delta: event)
    }

    init(snapshot event: ThreadAssistantMessageEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            turnID: event.turnID,
            threadSeq: event.threadSeq,
            agentID: event.agentID,
            status: event.state
        )
    }

    init(toolUpdate event: ThreadAgentToolUpdateEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            turnID: event.turnID,
            threadSeq: event.threadSeq,
            agentID: event.agentID
        )
    }

    init(planUpdate event: ThreadAgentPlanUpdateEvent) {
        self.init(
            threadID: event.threadID,
            sessionID: event.sessionID,
            turnID: event.turnID,
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
            agentID: agentID,
            title: agentID.capitalized,
            body: response,
            thinkingBody: thinking.isEmpty ? nil : thinking,
            planBody: planBody,
            toolActivities: toolActivities,
            status: status,
            tintName: tintName
        )
    }

    init?(persistedEntry entry: DaemonTimelineEntry, threadID: String) {
        guard entry.kind == .assistantTurn else { return nil }
        guard entry.normalizedStatusToken != "completed", entry.normalizedStatusToken != "failed" else {
            return nil
        }

        let prefix = "assistant-turn-\(threadID)-"
        guard entry.id.hasPrefix(prefix) else { return nil }

        let turnIdentity = String(entry.id.dropFirst(prefix.count))
        guard !turnIdentity.isEmpty else { return nil }

        self.threadID = threadID
        self.sessionID = turnIdentity
        self.turnIdentity = turnIdentity
        self.entryID = entry.id
        self.sortThreadSeq = entry.sortThreadSeq
        self.agentID = entry.title
        self.lastThreadSeq = entry.lastThreadSeq
        self.thinking = entry.thinkingBody ?? ""
        self.response = entry.body
        self.planBody = entry.planBody
        self.toolActivities = entry.toolActivities
        self.status = entry.status ?? "streaming"
    }
}

struct AssistantTurnReducer {
    private(set) var activeStates: [AssistantTurnKey: AssistantTurnState] = [:]

    mutating func restore(from timelineByThread: [String: [DaemonTimelineEntry]]) {
        activeStates.removeAll()

        for (threadID, entries) in timelineByThread {
            for entry in entries {
                guard let state = AssistantTurnState(persistedEntry: entry, threadID: threadID) else {
                    continue
                }

                let key = AssistantTurnKey(threadID: threadID, turnIdentity: state.turnIdentity)
                activeStates[key] = state
            }
        }
    }

    mutating func consume(snapshot event: ThreadAssistantMessageEvent) -> AssistantTurnState {
        let key = AssistantTurnKey(
            threadID: event.threadID,
            turnIdentity: assistantTurnIdentity(turnID: event.turnID, sessionID: event.sessionID)
        )
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
        let key = AssistantTurnKey(
            threadID: event.threadID,
            turnIdentity: assistantTurnIdentity(turnID: event.turnID, sessionID: event.sessionID)
        )

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
        let key = AssistantTurnKey(
            threadID: event.threadID,
            turnIdentity: assistantTurnIdentity(turnID: event.turnID, sessionID: event.sessionID)
        )
        var state = activeStates[key] ?? AssistantTurnState(toolUpdate: event)
        state.upsertTool(event)
        activeStates[key] = state
        return state
    }

    mutating func consume(planUpdate event: ThreadAgentPlanUpdateEvent) -> AssistantTurnState {
        let key = AssistantTurnKey(
            threadID: event.threadID,
            turnIdentity: assistantTurnIdentity(turnID: event.turnID, sessionID: event.sessionID)
        )
        var state = activeStates[key] ?? AssistantTurnState(planUpdate: event)
        state.updatePlan(event)
        activeStates[key] = state
        return state
    }

    mutating func finish(turnEnd event: ThreadAgentTurnEndEvent) -> AssistantTurnState? {
        let key = AssistantTurnKey(
            threadID: event.threadID,
            turnIdentity: assistantTurnIdentity(turnID: event.turnID, sessionID: event.sessionID)
        )
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
