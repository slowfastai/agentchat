//
//  AgentChatTests.swift
//  AgentChatTests
//
//  Created by Jia Li on 2026/3/24.
//

import Foundation
import Testing
@testable import AgentChat

struct AgentChatTests {
    @Test func scannedDaemonPayloadParsesRawWebSocketURL() async throws {
        let payload = parseScannedDaemonConnectionPayload(from: "ws://192.168.1.8:9390")

        #expect(payload == .direct(url: "ws://192.168.1.8:9390", agentIDs: []))
    }

    @Test func scannedDaemonPayloadParsesPreselectedAgents() async throws {
        let payload = parseScannedDaemonConnectionPayload(
            from: "agentchat://connect?url=ws%3A%2F%2F192.168.1.8%3A9390&agents=codex-main%2Ccodex-review"
        )

        #expect(payload == .direct(
            url: "ws://192.168.1.8:9390",
            agentIDs: ["codex-main", "codex-review"]
        ))
    }

    @Test func scannedDaemonPayloadParsesRelayDevPairingLink() async throws {
        let payload = parseScannedDaemonConnectionPayload(
            from: "agentchat://connect?relay_url=wss%3A%2F%2Frelay.agentchat.dev%2Fv1%2Fws&device_id=dev_local_1&relay_pairing=dev&relay_crypto=dev&agents=codex-main"
        )

        #expect(payload == .relay(
            RelayConnectionPayload(
                wsURL: "wss://relay.agentchat.dev/v1/ws",
                deviceID: "dev_local_1",
                relayToken: nil,
                pairingMode: .dev,
                pairingTicket: nil,
                cryptoMode: .dev,
                agentIDs: ["codex-main"]
            )
        ))
    }

    @Test func scannedDaemonPayloadParsesRelayClaimLink() async throws {
        let payload = parseScannedDaemonConnectionPayload(
            from: "agentchat://connect?relay_url=wss%3A%2F%2Frelay.agentchat.dev%2Fv1%2Fws&pairing_ticket=achpair.dev_local_1.pair_abc.secretvalue1234567890&relay_pairing=claim&relay_crypto=dev"
        )

        #expect(payload == .relay(
            RelayConnectionPayload(
                wsURL: "wss://relay.agentchat.dev/v1/ws",
                deviceID: nil,
                relayToken: nil,
                pairingMode: .claim,
                pairingTicket: "achpair.dev_local_1.pair_abc.secretvalue1234567890",
                cryptoMode: .dev,
                agentIDs: []
            )
        ))
    }

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
        #expect(claude.defaultAvatarAssetName == "ClaudeCodeAvatar")
        #expect(openCode.family == .opencode)
        #expect(openCode.displayName == "OpenCode")
        #expect(openCode.defaultAvatarAssetName == "OpenCodeAvatar")
    }

    @Test func daemonAgentSummaryUsesPiAvatarAsset() async throws {
        let pi = DaemonAgentSummary(
            agentID: "pi",
            name: "Pi",
            kind: "pi",
            status: "online",
            defaultWorkingDir: nil,
            capabilities: []
        )

        #expect(pi.family == .pi)
        #expect(pi.defaultAvatarAssetName == "PiAvatar")
    }

    @Test func daemonModelsDecodeExplicitMentionHandles() async throws {
        let agentJSON = #"{"agent_id":"open-code","name":"OpenCode","mention_handle":"opencode","kind":"opencode","status":"online","default_working_dir":null,"capabilities":["session"]}"#
        let participantJSON = #"{"participant_id":"participant-1","kind":"agent","display_name":"OpenCode","agent_id":"open-code","mention_handle":"opencode","session_id":"session-1","state":"idle"}"#

        let decoder = JSONDecoder()
        let agent = try decoder.decode(DaemonAgentSummary.self, from: Data(agentJSON.utf8))
        let participant = try decoder.decode(DaemonThreadParticipant.self, from: Data(participantJSON.utf8))

        #expect(agent.mentionHandle == "opencode")
        #expect(participant.mentionHandle == "opencode")
    }

    @Test @MainActor func daemonChatStoreRestoresPersistedAgentsAsOfflineOnColdStart() async throws {
        let suiteName = "AgentChatTests.\(UUID().uuidString)"
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            Issue.record("Failed to create isolated UserDefaults suite")
            return
        }
        defer {
            defaults.removePersistentDomain(forName: suiteName)
        }

        let persistedAgent = makeAgent(
            agentID: "opencode",
            name: "OpenCode",
            status: "online",
            capabilities: ["session", "prompt"]
        )

        defaults.set(try JSONEncoder().encode([persistedAgent]), forKey: "agentchat_known_agents")

        let store = DaemonChatStore(defaults: defaults)

        #expect(store.agents.count == 1)
        #expect(store.agents[0].agentID == "opencode")
        #expect(store.agents[0].status == "offline")
    }

    @Test @MainActor func daemonChatStoreOpensThreadLocallyWhileOfflineWithoutShowingError() async throws {
        let suiteName = "AgentChatTests.\(UUID().uuidString)"
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            Issue.record("Failed to create isolated UserDefaults suite")
            return
        }
        defer {
            defaults.removePersistentDomain(forName: suiteName)
        }

        let store = DaemonChatStore(defaults: defaults)

        store.attachThread("thread-1")
        await Task.yield()

        #expect(store.activeThreadID == "thread-1")
        #expect(store.connectionStatus == "Not configured")
        #expect(store.errorMessage == nil)
    }

    @Test func daemonConnectionStateExposesStoppedAndUnavailableStatusText() async throws {
        #expect(DaemonConnectionState.unavailable.statusText == "Daemon unavailable")
        #expect(DaemonConnectionState.stoppedByServer(reason: "user_shutdown").statusText == "Daemon stopped")
        #expect(DaemonConnectionState.online.isOnline)
        #expect(DaemonConnectionState.attached(threadID: "thread-1").isOnline)
        #expect(!DaemonConnectionState.reconnecting(attempt: 1).isOnline)
    }

    @Test func daemonStatusEventDecodesShutdownReason() async throws {
        let json = #"{"type":"daemon_status","state":"stopping","reason":"user_shutdown","message":"Daemon is stopping."}"#
        let event = try JSONDecoder().decode(DaemonStatusEvent.self, from: Data(json.utf8))

        #expect(event.state == "stopping")
        #expect(event.reason == "user_shutdown")
        #expect(event.message == "Daemon is stopping.")
    }

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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
            turnID: "turn-1",
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
        #expect(timelineEntry.executionSummary?.headline == "Used 1 tool")
        #expect(timelineEntry.executionSummary?.detailLine == "Plan")
    }

    @Test func assistantTurnReducerKeepsToolOnlyTurnInOneEntry() async throws {
        var reducer = AssistantTurnReducer()

        let toolUpdate = ThreadAgentToolUpdateEvent(
            threadID: "thread-1",
            threadSeq: 9,
            turnID: "turn-1",
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
            turnID: "turn-1",
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

    @Test func thoughtProcessSummaryOmitsRedundantThinkingDetail() async throws {
        let entry = DaemonTimelineEntry(
            id: "entry-1",
            sortThreadSeq: 1,
            lastThreadSeq: 1,
            kind: .assistantTurn,
            agentID: "opencode",
            title: "OpenCode",
            body: "Here is the answer.",
            thinkingBody: "Need to inspect the repository state first.",
            status: "completed",
            tintName: "orange"
        )

        #expect(entry.executionSummary?.headline == "Thought process")
        #expect(entry.executionSummary?.detailLine == nil)
    }

    @Test func createThreadDraftSelectionStartsEmptyWithoutRememberedAgents() async throws {
        let selection = AgentPickerDraftSelection.createThread(
            selectableIDs: ["codex", "opencode"],
            rememberedIDs: []
        )

        #expect(selection.isEmpty)
    }

    @Test func createThreadDraftSelectionIgnoresRememberedAgents() async throws {
        let selection = AgentPickerDraftSelection.createThread(
            selectableIDs: ["codex", "opencode"],
            rememberedIDs: ["codex", "offline-agent"]
        )

        #expect(selection.isEmpty)
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
