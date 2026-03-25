//
//  AgentChatTests.swift
//  AgentChatTests
//
//  Created by Jia Li on 2026/3/24.
//

import Testing
@testable import AgentChat

struct AgentChatTests {

    @Test func agentRosterKeepsKnownAgentsVisibleWhenLiveListShrinks() async throws {
        let claude = makeAgent(
            agentID: "claude",
            name: "Claude",
            status: "online",
            capabilities: ["review"]
        )
        let codex = makeAgent(
            agentID: "codex",
            name: "Codex",
            status: "online",
            capabilities: ["codegen"]
        )

        let merged = AgentRoster.merge(
            knownAgents: [claude, codex],
            incomingAgents: [claude]
        )

        #expect(merged.map(\.agentID) == ["claude", "codex"])
        #expect(merged.first(where: { $0.agentID == "claude" })?.status == "online")
        #expect(merged.first(where: { $0.agentID == "codex" })?.status == "offline")
    }

    @Test func agentRosterRefreshesKnownAgentDetailsFromLatestLiveSnapshot() async throws {
        let staleCodex = makeAgent(
            agentID: "codex",
            name: "Codex",
            status: "offline",
            capabilities: ["codegen"]
        )
        let refreshedCodex = makeAgent(
            agentID: "codex",
            name: "Codex Pro",
            status: "online",
            capabilities: ["codegen", "tests"]
        )

        let merged = AgentRoster.merge(
            knownAgents: [staleCodex],
            incomingAgents: [refreshedCodex]
        )

        #expect(merged.count == 1)
        #expect(merged[0].name == "Codex Pro")
        #expect(merged[0].status == "online")
        #expect(merged[0].capabilities == ["codegen", "tests"])
    }

    @Test func assistantTurnReducerIgnoresEmptyInitialDelta() async throws {
        var reducer = AssistantTurnReducer()

        let emptyDelta = ThreadAgentDeltaEvent(
            threadID: "thread-1",
            threadSeq: 2,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 1,
            content: "",
            deltaType: "text"
        )

        let state = reducer.consume(delta: emptyDelta)

        #expect(state == nil)
        #expect(reducer.activeStates.isEmpty)
    }

    @Test func assistantTurnReducerCoalescesDeltaChunksIntoOneAssistantMessage() async throws {
        var reducer = AssistantTurnReducer()

        let firstThinking = ThreadAgentDeltaEvent(
            threadID: "thread-1",
            threadSeq: 4,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 2,
            content: "The",
            deltaType: "thinking"
        )
        let secondThinking = ThreadAgentDeltaEvent(
            threadID: "thread-1",
            threadSeq: 5,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 3,
            content: " user typed hi.",
            deltaType: "thinking"
        )
        let responseChunk = ThreadAgentDeltaEvent(
            threadID: "thread-1",
            threadSeq: 7,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 5,
            content: "Hi there.",
            deltaType: "text"
        )
        let turnEnd = ThreadAgentTurnEndEvent(
            threadID: "thread-1",
            threadSeq: 8,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 6,
            stopReason: "EndTurn"
        )

        let stateAfterFirstThinking = reducer.consume(delta: firstThinking)
        let stateAfterSecondThinking = reducer.consume(delta: secondThinking)
        let stateAfterResponse = reducer.consume(delta: responseChunk)
        let completedState = reducer.finish(turnEnd: turnEnd)

        #expect(stateAfterFirstThinking?.sortThreadSeq == 4)
        #expect(stateAfterSecondThinking?.entryID == stateAfterFirstThinking?.entryID)
        #expect(stateAfterSecondThinking?.thinking == "The user typed hi.")
        #expect(stateAfterResponse?.entryID == stateAfterFirstThinking?.entryID)
        #expect(stateAfterResponse?.response == "Hi there.")
        #expect(completedState?.entryID == stateAfterFirstThinking?.entryID)
        #expect(completedState?.lastThreadSeq == 8)
        #expect(reducer.activeStates.isEmpty)

        let timelineEntry = completedState?.timelineEntry(tintName: "orange")
        #expect(timelineEntry?.id == stateAfterFirstThinking?.entryID)
        #expect(timelineEntry?.thinkingBody == "The user typed hi.")
        #expect(timelineEntry?.body == "Hi there.")
        #expect(timelineEntry?.status == "completed")
    }

    @Test func assistantTurnReducerMergesToolAndPlanUpdatesIntoSingleTurn() async throws {
        var reducer = AssistantTurnReducer()

        let toolPending = ThreadAgentToolUpdateEvent(
            threadID: "thread-1",
            threadSeq: 12,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 3,
            toolCallID: "tool-1",
            title: "websearch",
            status: "Pending",
            content: nil
        )
        let toolCompleted = ThreadAgentToolUpdateEvent(
            threadID: "thread-1",
            threadSeq: 14,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 5,
            toolCallID: "tool-1",
            title: "websearch",
            status: "Completed",
            content: "Beijing weather forecast tomorrow"
        )
        let planUpdate = ThreadAgentPlanUpdateEvent(
            threadID: "thread-1",
            threadSeq: 15,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 6,
            planJSON: .object([
                "steps": .array([
                    .object(["title": .string("Check the forecast")]),
                ]),
                "done": .bool(false),
            ])
        )
        let completedSnapshot = ThreadAssistantMessageEvent(
            threadID: "thread-1",
            threadSeq: 16,
            messageID: "message-1",
            turnID: "turn-1",
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 7,
            thinking: "Need to confirm the latest weather.",
            response: "Weather looks mild tomorrow.",
            state: "completed",
            stopReason: "EndTurn"
        )

        let stateAfterPending = reducer.consume(toolUpdate: toolPending)
        let stateAfterCompletedTool = reducer.consume(toolUpdate: toolCompleted)
        let stateAfterPlan = reducer.consume(planUpdate: planUpdate)
        let completedState = reducer.consume(snapshot: completedSnapshot)

        #expect(stateAfterPending.sortThreadSeq == 12)
        #expect(stateAfterPending.toolActivities.count == 1)
        #expect(stateAfterCompletedTool.toolActivities.count == 1)
        #expect(stateAfterCompletedTool.toolActivities.first?.status == "Completed")
        #expect(stateAfterCompletedTool.toolActivities.first?.content == "Beijing weather forecast tomorrow")
        #expect(stateAfterPlan.planBody?.contains("\"steps\"") == true)
        #expect(completedState.toolActivities.count == 1)
        #expect(completedState.planBody?.contains("\"Check the forecast\"") == true)
        #expect(completedState.thinking == "Need to confirm the latest weather.")
        #expect(completedState.response == "Weather looks mild tomorrow.")
        #expect(completedState.status == "completed")
        #expect(reducer.activeStates.isEmpty)

        let timelineEntry = completedState.timelineEntry(tintName: "orange")
        #expect(timelineEntry.toolActivities.count == 1)
        #expect(timelineEntry.planBody?.contains("\"steps\"") == true)
        #expect(timelineEntry.body == "Weather looks mild tomorrow.")
        #expect(timelineEntry.status == "completed")
    }

    @Test func assistantTurnReducerKeepsToolOnlyTurnInOneEntry() async throws {
        var reducer = AssistantTurnReducer()

        let toolUpdate = ThreadAgentToolUpdateEvent(
            threadID: "thread-1",
            threadSeq: 9,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 2,
            toolCallID: "tool-1",
            title: "read_file",
            status: "Completed",
            content: "README.md"
        )
        let turnEnd = ThreadAgentTurnEndEvent(
            threadID: "thread-1",
            threadSeq: 10,
            participantID: "participant-1",
            agentID: "opencode",
            sessionID: "session-1",
            sessionEventSeq: 3,
            stopReason: "EndTurn"
        )

        _ = reducer.consume(toolUpdate: toolUpdate)
        let completedState = reducer.finish(turnEnd: turnEnd)

        #expect(completedState?.toolActivities.count == 1)
        #expect(completedState?.response.isEmpty == true)
        #expect(completedState?.status == "completed")
        #expect(reducer.activeStates.isEmpty)
    }

    private func makeAgent(
        agentID: String,
        name: String,
        status: String,
        capabilities: [String]
    ) -> DaemonAgentSummary {
        DaemonAgentSummary(
            agentID: agentID,
            name: name,
            kind: "assistant",
            status: status,
            defaultWorkingDir: nil,
            capabilities: capabilities
        )
    }
}
