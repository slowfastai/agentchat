//
//  AgentChatTests.swift
//  AgentChatTests
//
//  Created by Jia Li on 2026/3/24.
//

import Testing
@testable import AgentChat

struct AgentChatTests {
    @Test func daemonAgentSummaryRecognizesCodexFromBackendKind() async throws {
        let summary = DaemonAgentSummary(
            agentID: "workspace-codex",
            name: "Codex Main",
            kind: "codex_app_server",
            status: "online",
            defaultWorkingDir: nil,
            capabilities: ["session", "prompt"]
        )

        #expect(summary.family == .codex)
        #expect(summary.kindTitle == "Codex")
        #expect(summary.symbolName == "curlybraces.square.fill")
        #expect(summary.tintName == "green")
        #expect(summary.displayName == "Codex Main")
    }

    @Test func daemonAgentSummaryRecognizesClaudeAndOpenCode() async throws {
        let claude = DaemonAgentSummary(
            agentID: "claude-review",
            name: "Claude Code",
            kind: "claude_code",
            status: "online",
            defaultWorkingDir: nil,
            capabilities: []
        )
        let openCode = DaemonAgentSummary(
            agentID: "open-code",
            name: "",
            kind: "opencode",
            status: "online",
            defaultWorkingDir: nil,
            capabilities: []
        )

        #expect(claude.family == .claude)
        #expect(claude.tintName == "blue")
        #expect(openCode.family == .opencode)
        #expect(openCode.displayName == "OpenCode")
    }

    @Test func legacyReducerIgnoresEmptyInitialDelta() async throws {
        var reducer = LegacyAssistantMessageReducer()

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

    @Test func legacyReducerCoalescesDeltaChunksIntoOneAssistantMessage() async throws {
        var reducer = LegacyAssistantMessageReducer()

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

        let timelineEntry = completedState?.timelineEntry(status: "completed", tintName: "orange")
        #expect(timelineEntry?.id == stateAfterFirstThinking?.entryID)
        #expect(timelineEntry?.thinkingBody == "The user typed hi.")
        #expect(timelineEntry?.body == "Hi there.")
        #expect(timelineEntry?.status == "completed")
    }

}
