import Foundation

struct PrototypeDaemonAgentWire: Codable {
    let agentID: String
    let name: String
    let kind: String
    let status: String
    let capabilities: [String]

    enum CodingKeys: String, CodingKey {
        case agentID = "agent_id"
        case name
        case kind
        case status
        case capabilities
    }
}

struct PrototypeRemoteAssistantMessage: Hashable {
    let agentID: String
    let thinking: String
    let planBody: String?
    let toolActivities: [PrototypeRemoteToolActivity]
    let response: String
    let stopReason: String?
}

struct PrototypeRemoteToolActivity: Hashable {
    let id: String
    let title: String
    let status: String
    let content: String?
}

struct PrototypeRemoteThreadHandle {
    let threadID: String
}

enum PrototypeRemoteTimelineKind: Hashable {
    case userMessage(senderName: String, content: String)
    case thinking(agentID: String, text: String)
    case plan(agentID: String, body: String)
    case tool(agentID: String, activity: PrototypeRemoteToolActivity)
    case assistantMessage(agentID: String, content: String)
    case turnEnd(agentID: String, reason: String)
    case system(text: String)
}

struct PrototypeRemoteTimelineEntry: Hashable {
    let threadSeq: UInt64
    let kind: PrototypeRemoteTimelineKind
}

private struct PrototypeListAgentsRequest: Encodable {
    let type = "list_agents"
}

private struct PrototypeCreateThreadRequest: Encodable {
    let type = "create_thread"
    let title: String?
    let workingDir: String

    enum CodingKeys: String, CodingKey {
        case type
        case title
        case workingDir = "working_dir"
    }
}

private struct PrototypeAddThreadParticipantRequest: Encodable {
    let type = "add_thread_participant"
    let threadID: String
    let agentID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case agentID = "agent_id"
    }
}

private struct PrototypeSendThreadMessageRequest: Encodable {
    let type = "send_thread_message"
    let threadID: String
    let content: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case content
    }
}

private struct PrototypeAttachThreadRequest: Encodable {
    let type = "attach_thread"
    let threadID: String
    let afterSeq: UInt64?

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case afterSeq = "after_seq"
    }
}

private struct PrototypeAgentListResponse: Codable {
    let type: String
    let agents: [PrototypeDaemonAgentWire]
}

private struct PrototypeEnvelope: Decodable {
    let type: String
}

private struct PrototypeThreadCreatedResponse: Decodable {
    let type: String
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
    }
}

private struct PrototypeThreadAttachedResponse: Decodable {
    let type: String
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
    }
}

private struct PrototypeThreadReplayCompleteResponse: Decodable {
    let type: String
    let threadID: String
    let lastThreadSeq: UInt64

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case lastThreadSeq = "last_thread_seq"
    }
}

private struct PrototypeThreadMessageResponse: Decodable {
    let type: String
    let threadSeq: UInt64
    let sender: PrototypeThreadSenderResponse
    let content: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadSeq = "thread_seq"
        case sender
        case content
    }
}

private struct PrototypeThreadSenderResponse: Decodable {
    let displayName: String

    enum CodingKeys: String, CodingKey {
        case displayName = "display_name"
    }
}

private struct PrototypeParticipantAddedResponse: Decodable {
    let type: String
    let threadID: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
    }
}

private struct PrototypeThreadAssistantMessageResponse: Decodable {
    let type: String
    let threadID: String
    let threadSeq: UInt64
    let agentID: String
    let thinking: String
    let response: String
    let state: String
    let stopReason: String?

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case agentID = "agent_id"
        case thinking
        case response
        case state
        case stopReason = "stop_reason"
    }
}

private struct PrototypeThreadTurnEndResponse: Decodable {
    let type: String
    let threadID: String
    let threadSeq: UInt64
    let agentID: String
    let stopReason: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadID = "thread_id"
        case threadSeq = "thread_seq"
        case agentID = "agent_id"
        case stopReason = "stop_reason"
    }
}

private struct PrototypeThreadAgentDeltaResponse: Decodable {
    let type: String
    let threadSeq: UInt64
    let agentID: String
    let content: String
    let deltaType: String

    enum CodingKeys: String, CodingKey {
        case type
        case threadSeq = "thread_seq"
        case agentID = "agent_id"
        case content
        case deltaType = "delta_type"
    }
}

private struct PrototypeThreadAgentToolUpdateResponse: Decodable {
    let type: String
    let threadSeq: UInt64
    let agentID: String
    let toolCallID: String
    let title: String
    let status: String
    let content: String?

    enum CodingKeys: String, CodingKey {
        case type
        case threadSeq = "thread_seq"
        case agentID = "agent_id"
        case toolCallID = "tool_call_id"
        case title
        case status
        case content
    }
}

private struct PrototypeThreadAgentPlanUpdateResponse: Decodable {
    let type: String
    let threadSeq: UInt64
    let agentID: String
    let planJSON: PrototypeJSONValue

    enum CodingKeys: String, CodingKey {
        case type
        case threadSeq = "thread_seq"
        case agentID = "agent_id"
        case planJSON = "plan_json"
    }
}

private enum PrototypeJSONValue: Codable, Hashable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: PrototypeJSONValue])
    case array([PrototypeJSONValue])
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
        } else if let value = try? container.decode([String: PrototypeJSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([PrototypeJSONValue].self) {
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
              let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]),
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

private struct PrototypeRemoteAssistantAccumulator {
    let agentID: String
    var thinking = ""
    var planBody: String?
    var toolActivities: [String: PrototypeRemoteToolActivity] = [:]
    var response = ""
    var stopReason: String?
    var state = "streaming"

    var finalizedMessage: PrototypeRemoteAssistantMessage {
        PrototypeRemoteAssistantMessage(
            agentID: agentID,
            thinking: thinking,
            planBody: planBody,
            toolActivities: toolActivities.values.sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending },
            response: response,
            stopReason: stopReason
        )
    }
}

private struct PrototypeDaemonErrorResponse: Codable {
    let type: String
    let code: String
    let message: String
}

enum PrototypeDaemonAgentBridgeError: LocalizedError {
    case invalidURL(String)
    case invalidResponse
    case unexpectedMessage(String)
    case daemonError(String)
    case timedOut(String)

    var errorDescription: String? {
        switch self {
        case .invalidURL(let value):
            return "Invalid daemon URL: \(value)"
        case .invalidResponse:
            return "Daemon returned an unreadable response."
        case .unexpectedMessage(let type):
            return "Daemon returned unexpected message type: \(type)"
        case .daemonError(let message):
            return message
        case .timedOut(let operation):
            return "Timed out while waiting for \(operation)."
        }
    }
}

enum PrototypeDaemonAgentBridge {
    nonisolated static let defaultURLString = "ws://127.0.0.1:9390"
    nonisolated private static let assistantResponseTimeout: TimeInterval = 60

    static func fetchAgents(from urlString: String = defaultURLString) async throws -> [PrototypeDaemonAgentWire] {
        guard let url = URL(string: urlString) else {
            throw PrototypeDaemonAgentBridgeError.invalidURL(urlString)
        }

        let task = URLSession.shared.webSocketTask(with: url)
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
        }

        let requestData = try JSONEncoder().encode(PrototypeListAgentsRequest())
        guard let requestText = String(data: requestData, encoding: .utf8) else {
            throw PrototypeDaemonAgentBridgeError.invalidResponse
        }

        try await task.send(.string(requestText))
        let responseText = try await receiveText(from: task)
        let responseData = Data(responseText.utf8)

        if let errorResponse = try? JSONDecoder().decode(PrototypeDaemonErrorResponse.self, from: responseData),
           errorResponse.type == "error" {
            throw PrototypeDaemonAgentBridgeError.daemonError("\(errorResponse.code): \(errorResponse.message)")
        }

        let agentList = try JSONDecoder().decode(PrototypeAgentListResponse.self, from: responseData)
        guard agentList.type == "agent_list" else {
            throw PrototypeDaemonAgentBridgeError.unexpectedMessage(agentList.type)
        }
        return agentList.agents
    }

    static func createRemoteThread(
        title: String,
        workingDir: String = ".",
        agentID: String,
        urlString: String = defaultURLString
    ) async throws -> PrototypeRemoteThreadHandle {
        guard let url = URL(string: urlString) else {
            throw PrototypeDaemonAgentBridgeError.invalidURL(urlString)
        }

        let task = URLSession.shared.webSocketTask(with: url)
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
        }

        try await sendJSON(
            PrototypeCreateThreadRequest(title: title, workingDir: workingDir),
            over: task
        )
        let created = try await waitForThreadCreated(on: task)

        try await sendJSON(
            PrototypeAddThreadParticipantRequest(threadID: created.threadID, agentID: agentID),
            over: task
        )
        _ = try await waitForParticipantAdded(on: task, threadID: created.threadID)
        return PrototypeRemoteThreadHandle(threadID: created.threadID)
    }

    static func runOneShotPrompt(
        agentID: String,
        title: String,
        content: String,
        workingDir: String = ".",
        urlString: String = defaultURLString
    ) async throws -> PrototypeRemoteAssistantMessage? {
        let handle = try await createRemoteThread(
            title: title,
            workingDir: workingDir,
            agentID: agentID,
            urlString: urlString
        )
        let messages = try await sendRemoteThreadMessage(
            threadID: handle.threadID,
            content: content,
            urlString: urlString
        )
        return messages.last
    }

    static func sendRemoteThreadMessage(
        threadID: String,
        content: String,
        urlString: String = defaultURLString
    ) async throws -> [PrototypeRemoteAssistantMessage] {
        guard let url = URL(string: urlString) else {
            throw PrototypeDaemonAgentBridgeError.invalidURL(urlString)
        }

        let task = URLSession.shared.webSocketTask(with: url)
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
        }

        try await sendJSON(
            PrototypeSendThreadMessageRequest(threadID: threadID, content: content),
            over: task
        )

        var messagesByAgentID: [String: PrototypeRemoteAssistantMessage] = [:]
        var accumulators: [String: PrototypeRemoteAssistantAccumulator] = [:]
        let deadline = Date().addingTimeInterval(Self.assistantResponseTimeout)

        while Date() < deadline {
            let responseText = try await receiveText(from: task)
            let responseData = Data(responseText.utf8)

            if let errorResponse = try? JSONDecoder().decode(PrototypeDaemonErrorResponse.self, from: responseData),
               errorResponse.type == "error" {
                throw PrototypeDaemonAgentBridgeError.daemonError("\(errorResponse.code): \(errorResponse.message)")
            }

            let envelope = try JSONDecoder().decode(PrototypeEnvelope.self, from: responseData)
            switch envelope.type {
            case "thread_message":
                continue
            case "thread_agent_delta":
                let event = try JSONDecoder().decode(PrototypeThreadAgentDeltaResponse.self, from: responseData)
                var accumulator = accumulators[event.agentID] ?? PrototypeRemoteAssistantAccumulator(agentID: event.agentID)
                switch event.deltaType {
                case "thinking":
                    accumulator.thinking += event.content
                case "text":
                    accumulator.response += event.content
                default:
                    break
                }
                accumulators[event.agentID] = accumulator
            case "thread_agent_tool_update":
                let event = try JSONDecoder().decode(PrototypeThreadAgentToolUpdateResponse.self, from: responseData)
                var accumulator = accumulators[event.agentID] ?? PrototypeRemoteAssistantAccumulator(agentID: event.agentID)
                accumulator.toolActivities[event.toolCallID] = PrototypeRemoteToolActivity(
                    id: event.toolCallID,
                    title: event.title,
                    status: event.status,
                    content: event.content
                )
                accumulators[event.agentID] = accumulator
            case "thread_agent_plan_update":
                let event = try JSONDecoder().decode(PrototypeThreadAgentPlanUpdateResponse.self, from: responseData)
                var accumulator = accumulators[event.agentID] ?? PrototypeRemoteAssistantAccumulator(agentID: event.agentID)
                accumulator.planBody = event.planJSON.prettyPrintedString()
                accumulators[event.agentID] = accumulator
            case "thread_assistant_message":
                let event = try JSONDecoder().decode(PrototypeThreadAssistantMessageResponse.self, from: responseData)
                var accumulator = accumulators[event.agentID] ?? PrototypeRemoteAssistantAccumulator(agentID: event.agentID)
                accumulator.thinking = event.thinking
                accumulator.response = event.response
                accumulator.stopReason = event.stopReason
                accumulator.state = event.state
                accumulators[event.agentID] = accumulator
                messagesByAgentID[event.agentID] = accumulator.finalizedMessage
                if event.state == "completed" && !messagesByAgentID.isEmpty {
                    return Array(messagesByAgentID.values)
                }
            case "thread_agent_turn_end":
                let event = try JSONDecoder().decode(PrototypeThreadTurnEndResponse.self, from: responseData)
                if var firstAccumulator = accumulators.values.first {
                    firstAccumulator.stopReason = event.stopReason
                    accumulators[firstAccumulator.agentID] = firstAccumulator
                    messagesByAgentID[firstAccumulator.agentID] = firstAccumulator.finalizedMessage
                }
                if !messagesByAgentID.isEmpty {
                    return Array(messagesByAgentID.values)
                }
            default:
                continue
            }
        }

        if !messagesByAgentID.isEmpty {
            return Array(messagesByAgentID.values)
        }
        throw PrototypeDaemonAgentBridgeError.timedOut("assistant response")
    }

    static func replayRemoteThread(
        threadID: String,
        urlString: String = defaultURLString
    ) async throws -> [PrototypeRemoteTimelineEntry] {
        guard let url = URL(string: urlString) else {
            throw PrototypeDaemonAgentBridgeError.invalidURL(urlString)
        }

        let task = URLSession.shared.webSocketTask(with: url)
        task.resume()
        defer {
            task.cancel(with: .normalClosure, reason: nil)
        }

        try await sendJSON(
            PrototypeAttachThreadRequest(threadID: threadID, afterSeq: 0),
            over: task
        )

        var replayEntries: [PrototypeRemoteTimelineEntry] = []
        let deadline = Date().addingTimeInterval(15)

        while Date() < deadline {
            let responseText = try await receiveText(from: task)
            let responseData = Data(responseText.utf8)

            if let errorResponse = try? JSONDecoder().decode(PrototypeDaemonErrorResponse.self, from: responseData),
               errorResponse.type == "error" {
                throw PrototypeDaemonAgentBridgeError.daemonError("\(errorResponse.code): \(errorResponse.message)")
            }

            let envelope = try JSONDecoder().decode(PrototypeEnvelope.self, from: responseData)
            switch envelope.type {
            case "thread_attached":
                _ = try JSONDecoder().decode(PrototypeThreadAttachedResponse.self, from: responseData)
            case "thread_snapshot":
                continue
            case "thread_message":
                let event = try JSONDecoder().decode(PrototypeThreadMessageResponse.self, from: responseData)
                replayEntries.append(
                    PrototypeRemoteTimelineEntry(
                        threadSeq: event.threadSeq,
                        kind: .userMessage(senderName: event.sender.displayName, content: event.content)
                    )
                )
            case "thread_agent_delta":
                let event = try JSONDecoder().decode(PrototypeThreadAgentDeltaResponse.self, from: responseData)
                if event.deltaType == "thinking" {
                    replayEntries.append(
                        PrototypeRemoteTimelineEntry(
                            threadSeq: event.threadSeq,
                            kind: .thinking(agentID: event.agentID, text: event.content)
                        )
                    )
                }
            case "thread_agent_plan_update":
                let event = try JSONDecoder().decode(PrototypeThreadAgentPlanUpdateResponse.self, from: responseData)
                replayEntries.append(
                    PrototypeRemoteTimelineEntry(
                        threadSeq: event.threadSeq,
                        kind: .plan(agentID: event.agentID, body: event.planJSON.prettyPrintedString())
                    )
                )
            case "thread_agent_tool_update":
                let event = try JSONDecoder().decode(PrototypeThreadAgentToolUpdateResponse.self, from: responseData)
                replayEntries.append(
                    PrototypeRemoteTimelineEntry(
                        threadSeq: event.threadSeq,
                        kind: .tool(
                            agentID: event.agentID,
                            activity: PrototypeRemoteToolActivity(
                                id: event.toolCallID,
                                title: event.title,
                                status: event.status,
                                content: event.content
                            )
                        )
                    )
                )
            case "thread_assistant_message":
                let event = try JSONDecoder().decode(PrototypeThreadAssistantMessageResponse.self, from: responseData)
                if event.state == "completed" {
                    replayEntries.append(
                        PrototypeRemoteTimelineEntry(
                            threadSeq: event.threadSeq,
                            kind: .assistantMessage(agentID: event.agentID, content: event.response)
                        )
                    )
                }
            case "thread_agent_turn_end":
                let event = try JSONDecoder().decode(PrototypeThreadTurnEndResponse.self, from: responseData)
                replayEntries.append(
                    PrototypeRemoteTimelineEntry(
                        threadSeq: event.threadSeq,
                        kind: .turnEnd(agentID: event.agentID, reason: event.stopReason)
                    )
                )
            case "thread_replay_complete":
                _ = try JSONDecoder().decode(PrototypeThreadReplayCompleteResponse.self, from: responseData)
                return replayEntries
            default:
                continue
            }
        }

        if !replayEntries.isEmpty {
            return replayEntries
        }
        throw PrototypeDaemonAgentBridgeError.timedOut("thread replay")
    }

    private static func receiveText(from task: URLSessionWebSocketTask) async throws -> String {
        let message = try await task.receive()
        switch message {
        case .string(let text):
            return text
        case .data(let data):
            guard let text = String(data: data, encoding: .utf8) else {
                throw PrototypeDaemonAgentBridgeError.invalidResponse
            }
            return text
        @unknown default:
            throw PrototypeDaemonAgentBridgeError.invalidResponse
        }
    }

    private static func sendJSON<Request: Encodable>(_ request: Request, over task: URLSessionWebSocketTask) async throws {
        let requestData = try JSONEncoder().encode(request)
        guard let requestText = String(data: requestData, encoding: .utf8) else {
            throw PrototypeDaemonAgentBridgeError.invalidResponse
        }
        try await task.send(.string(requestText))
    }

    private static func waitForThreadCreated(on task: URLSessionWebSocketTask) async throws -> PrototypeThreadCreatedResponse {
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            let responseText = try await receiveText(from: task)
            let responseData = Data(responseText.utf8)

            if let errorResponse = try? JSONDecoder().decode(PrototypeDaemonErrorResponse.self, from: responseData),
               errorResponse.type == "error" {
                throw PrototypeDaemonAgentBridgeError.daemonError("\(errorResponse.code): \(errorResponse.message)")
            }

            let envelope = try JSONDecoder().decode(PrototypeEnvelope.self, from: responseData)
            if envelope.type == "thread_created" {
                return try JSONDecoder().decode(PrototypeThreadCreatedResponse.self, from: responseData)
            }
        }
        throw PrototypeDaemonAgentBridgeError.timedOut("thread creation")
    }

    private static func waitForParticipantAdded(
        on task: URLSessionWebSocketTask,
        threadID: String
    ) async throws -> PrototypeParticipantAddedResponse {
        let deadline = Date().addingTimeInterval(10)
        while Date() < deadline {
            let responseText = try await receiveText(from: task)
            let responseData = Data(responseText.utf8)

            if let errorResponse = try? JSONDecoder().decode(PrototypeDaemonErrorResponse.self, from: responseData),
               errorResponse.type == "error" {
                throw PrototypeDaemonAgentBridgeError.daemonError("\(errorResponse.code): \(errorResponse.message)")
            }

            let envelope = try JSONDecoder().decode(PrototypeEnvelope.self, from: responseData)
            if envelope.type == "thread_participant_added" {
                let added = try JSONDecoder().decode(PrototypeParticipantAddedResponse.self, from: responseData)
                if added.threadID == threadID {
                    return added
                }
            }
        }
        throw PrototypeDaemonAgentBridgeError.timedOut("participant creation")
    }
}
