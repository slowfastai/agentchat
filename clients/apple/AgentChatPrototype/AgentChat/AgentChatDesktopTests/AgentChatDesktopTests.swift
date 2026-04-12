import Foundation
import Testing
@testable import AgentChatDesktop

struct AgentChatDesktopTests {
    @Test func desktopConnectionPresentationMapsRecoveringStates() {
        let presentation = AgentChatDesktopConnectionPresentation(state: .syncing)

        #expect(presentation.title == "Syncing agents and threads")
        #expect(presentation.subtitle.contains("Refreshing"))
        #expect(presentation.systemImage == "arrow.triangle.2.circlepath.circle.fill")
        #expect(presentation.isWorking)
    }

    @Test @MainActor func desktopSuggestedParticipantsDefaultsToAllAgents() {
        let snapshot = DaemonThreadSnapshot(
            threadID: "thread-1",
            title: "Review",
            workingDir: ".",
            createdAtMS: 1,
            lastThreadSeq: 2,
            participants: [
                DaemonThreadParticipant(participantID: "agent-a", kind: "agent", displayName: "Codex", agentID: "codex", sessionID: "session-a", state: "idle"),
                DaemonThreadParticipant(participantID: "human-1", kind: "human", displayName: "You", agentID: nil, sessionID: nil, state: "idle"),
                DaemonThreadParticipant(participantID: "agent-b", kind: "agent", displayName: "Claude", agentID: "claude", sessionID: "session-b", state: "idle"),
            ]
        )

        let suiteName = "AgentChatDesktopTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = DaemonChatStore(defaults: defaults)
        let suggested = store.desktopSuggestedParticipantIDs(for: snapshot)

        #expect(suggested == ["agent-a", "agent-b"])
    }

    @Test @MainActor func desktopAvailableAgentsToAddExcludesExistingParticipants() {
        let suiteName = "AgentChatDesktopTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = DaemonChatStore(defaults: defaults)
        store.agents = [
            DaemonAgentSummary(agentID: "codex", name: "Codex", kind: "codex_app_server", status: "online", defaultWorkingDir: nil, capabilities: []),
            DaemonAgentSummary(agentID: "claude", name: "Claude", kind: "claude_code", status: "online", defaultWorkingDir: nil, capabilities: []),
            DaemonAgentSummary(agentID: "pi", name: "Pi", kind: "pi", status: "offline", defaultWorkingDir: nil, capabilities: []),
        ]

        let snapshot = DaemonThreadSnapshot(
            threadID: "thread-1",
            title: nil,
            workingDir: ".",
            createdAtMS: 1,
            lastThreadSeq: 3,
            participants: [
                DaemonThreadParticipant(participantID: "agent-a", kind: "agent", displayName: "Codex", agentID: "codex", sessionID: "session-a", state: "idle"),
            ]
        )

        let available = store.desktopAvailableAgentsToAdd(to: snapshot)

        #expect(available.map(\.agentID) == ["claude"])
    }

    @Test @MainActor func desktopSuggestedParticipantsPreserveCustomizedSubset() {
        let snapshot = DaemonThreadSnapshot(
            threadID: "thread-1",
            title: "Review",
            workingDir: ".",
            createdAtMS: 1,
            lastThreadSeq: 2,
            participants: [
                DaemonThreadParticipant(participantID: "agent-a", kind: "agent", displayName: "Codex", agentID: "codex", sessionID: "session-a", state: "idle"),
                DaemonThreadParticipant(participantID: "agent-b", kind: "agent", displayName: "Claude", agentID: "claude", sessionID: "session-b", state: "idle"),
            ]
        )

        let suiteName = "AgentChatDesktopTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = DaemonChatStore(defaults: defaults)
        store.updateSelectedParticipants(["agent-b"], for: snapshot)

        #expect(store.desktopSuggestedParticipantIDs(for: snapshot) == ["agent-b"])
    }

    @Test @MainActor func desktopSortedThreadsPrefersHigherSequenceThenRecency() {
        let suiteName = "AgentChatDesktopTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defer { defaults.removePersistentDomain(forName: suiteName) }

        let store = DaemonChatStore(defaults: defaults)
        store.threads = [
            DaemonThreadSummary(threadID: "thread-1", title: "Older", workingDir: ".", createdAtMS: 1, state: "idle", participantCount: 1, lastThreadSeq: 2),
            DaemonThreadSummary(threadID: "thread-2", title: "Newest", workingDir: ".", createdAtMS: 10, state: "idle", participantCount: 1, lastThreadSeq: 2),
            DaemonThreadSummary(threadID: "thread-3", title: "Busiest", workingDir: ".", createdAtMS: 2, state: "idle", participantCount: 1, lastThreadSeq: 8),
        ]

        #expect(store.desktopSortedThreads.map(\.threadID) == ["thread-3", "thread-2", "thread-1"])
    }

    @Test @MainActor func directAndRelayLinksStillParseForDesktop() {
        #expect(parseScannedDaemonConnectionPayload(from: "ws://127.0.0.1:9390") == .direct(url: "ws://127.0.0.1:9390", agentIDs: []))

        let relay = parseScannedDaemonConnectionPayload(
            from: "agentchat://connect?relay_url=wss%3A%2F%2Frelay.agentchat.dev%2Fv1%2Fws&pairing_ticket=achpair.dev_local_1.pair_abc.secretvalue1234567890&relay_pairing=claim&relay_crypto=dev"
        )

        #expect(relay == .relay(
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
}
