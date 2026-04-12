import Foundation

struct AgentChatDesktopConnectionPresentation: Equatable {
    let title: String
    let subtitle: String
    let systemImage: String
    let tintName: String
    let isWorking: Bool

    private init(title: String, subtitle: String, systemImage: String, tintName: String, isWorking: Bool) {
        self.title = title
        self.subtitle = subtitle
        self.systemImage = systemImage
        self.tintName = tintName
        self.isWorking = isWorking
    }

    init(state: DaemonConnectionState) {
        switch state {
        case .notConfigured:
            self.init(
                title: "No daemon configured",
                subtitle: "Paste a direct websocket URL or relay link in Settings to connect.",
                systemImage: "bolt.slash",
                tintName: "gray",
                isWorking: false
            )
        case .badURL:
            self.init(
                title: "Connection link is invalid",
                subtitle: "Check the saved daemon URL or relay payload, then try again.",
                systemImage: "exclamationmark.triangle.fill",
                tintName: "orange",
                isWorking: false
            )
        case .connecting:
            self.init(
                title: "Connecting to daemon",
                subtitle: "Opening the websocket and waiting for the daemon to respond.",
                systemImage: "bolt.horizontal.circle.fill",
                tintName: "orange",
                isWorking: true
            )
        case .pairingRelay:
            self.init(
                title: "Pairing with relay",
                subtitle: "Claiming the relay ticket before the desktop app joins the session.",
                systemImage: "bolt.horizontal.circle.fill",
                tintName: "orange",
                isWorking: true
            )
        case .connectingRelay:
            self.init(
                title: "Connecting to relay",
                subtitle: "The relay channel is opening now.",
                systemImage: "bolt.horizontal.circle.fill",
                tintName: "orange",
                isWorking: true
            )
        case .securingRelay:
            self.init(
                title: "Securing relay session",
                subtitle: "Finishing the relay handshake before chat data starts flowing.",
                systemImage: "lock.shield.fill",
                tintName: "orange",
                isWorking: true
            )
        case .syncing:
            self.init(
                title: "Syncing agents and threads",
                subtitle: "Refreshing your roster, thread list, and the active conversation.",
                systemImage: "arrow.triangle.2.circlepath.circle.fill",
                tintName: "orange",
                isWorking: true
            )
        case .reconnecting(let attempt):
            self.init(
                title: attempt > 1 ? "Retrying connection (\(attempt))" : "Retrying connection",
                subtitle: "The desktop app will keep trying automatically. You can also reconnect manually.",
                systemImage: "wifi.exclamationmark",
                tintName: "orange",
                isWorking: true
            )
        case .online:
            self.init(
                title: "Connected",
                subtitle: "The daemon is online and ready for new threads and live updates.",
                systemImage: "bolt.fill",
                tintName: "green",
                isWorking: false
            )
        case .attached(let threadID):
            self.init(
                title: "Connected",
                subtitle: "Attached to \(shortThreadID(threadID)) for live thread updates.",
                systemImage: "bolt.fill",
                tintName: "green",
                isWorking: false
            )
        case .unavailable:
            self.init(
                title: "Daemon unavailable",
                subtitle: "The saved daemon is offline or unreachable. Reconnect after it comes back.",
                systemImage: "wifi.exclamationmark",
                tintName: "orange",
                isWorking: false
            )
        case .stoppedByServer(let reason):
            let trimmedReason = reason?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            self.init(
                title: "Daemon stopped",
                subtitle: trimmedReason.isEmpty ? "The daemon asked the desktop app to disconnect." : trimmedReason,
                systemImage: "stop.circle.fill",
                tintName: "red",
                isWorking: false
            )
        }
    }
}

private func shortThreadID(_ threadID: String) -> String {
    guard threadID.count > 18 else { return threadID }
    return String(threadID.prefix(10)) + "..." + String(threadID.suffix(6))
}

private extension String {
    var desktopTrimmedLines: [String] {
        components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }
}

extension DaemonThreadSummary {
    var createdAtDate: Date {
        Date(timeIntervalSince1970: TimeInterval(createdAtMS) / 1000)
    }
}

extension DaemonChatStore {
    var desktopSortedThreads: [DaemonThreadSummary] {
        threads.sorted { lhs, rhs in
            if lhs.lastThreadSeq != rhs.lastThreadSeq {
                return lhs.lastThreadSeq > rhs.lastThreadSeq
            }

            if lhs.createdAtMS != rhs.createdAtMS {
                return lhs.createdAtMS > rhs.createdAtMS
            }

            return effectiveThreadTitle(for: lhs).localizedCaseInsensitiveCompare(effectiveThreadTitle(for: rhs)) == .orderedAscending
        }
    }

    var desktopOnlineAgents: [DaemonAgentSummary] {
        AgentRoster.sorted(agents).filter(\.isOnline)
    }

    var desktopConnectionErrorSummary: String? {
        guard let errorMessage else { return nil }
        return errorMessage.desktopTrimmedLines.first
    }

    var desktopConnectionErrorDetail: String? {
        guard let errorMessage else { return nil }
        let lines = errorMessage.desktopTrimmedLines
        guard lines.count > 1 else { return nil }
        return lines.dropFirst().joined(separator: "\n")
    }

    var desktopHasConnectionIssue: Bool {
        switch connectionState {
        case .badURL, .unavailable, .stoppedByServer:
            return true
        default:
            return desktopConnectionErrorSummary != nil
        }
    }

    func effectiveThreadTitle(for thread: DaemonThreadSummary) -> String {
        let trimmedTitle = thread.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !trimmedTitle.isEmpty {
            return trimmedTitle
        }

        let snapshotTitle = activeThreadSnapshot?.threadID == thread.threadID
            ? activeThreadSnapshot?.title?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            : ""
        if !snapshotTitle.isEmpty {
            return snapshotTitle
        }

        let participants = activeThreadSnapshot?.threadID == thread.threadID
            ? activeThreadSnapshot?.participants.filter(\.isAgent).map(\.displayName) ?? []
            : []
        if !participants.isEmpty {
            return participants.joined(separator: " + ")
        }

        return thread.threadID
    }

    func desktopSuggestedParticipantIDs(for snapshot: DaemonThreadSnapshot?) -> Set<String> {
        let agentParticipants = Set((snapshot?.participants ?? []).filter(\.isAgent).map(\.participantID))
        guard !agentParticipants.isEmpty else {
            return []
        }

        let customized = selectedParticipantIDs.intersection(agentParticipants)
        return customized.isEmpty ? agentParticipants : customized
    }

    func desktopAvailableAgentsToAdd(to snapshot: DaemonThreadSnapshot?) -> [DaemonAgentSummary] {
        let existingAgentIDs = Set((snapshot?.participants ?? []).compactMap(\.agentID))
        return desktopOnlineAgents.filter { !existingAgentIDs.contains($0.agentID) }
    }

    func desktopIsLoadingThreadContent(for threadID: String, snapshot: DaemonThreadSnapshot?) -> Bool {
        guard activeThreadID == threadID || desktopSortedThreads.contains(where: { $0.threadID == threadID }) else {
            return false
        }
        guard snapshot == nil else { return false }

        switch connectionState {
        case .connecting, .pairingRelay, .connectingRelay, .securingRelay, .syncing, .online, .attached, .reconnecting:
            return true
        default:
            return false
        }
    }

    func desktopUpdateSelectedParticipantIDs(_ participantIDs: Set<String>, for snapshot: DaemonThreadSnapshot?) {
        updateSelectedParticipants(participantIDs, for: snapshot)
    }
}
