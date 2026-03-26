import Foundation

enum RelayPairingMode: String, Codable, Equatable {
    case dev
}

enum RelayCryptoMode: String, Codable, Equatable {
    case dev
}

struct RelayConnectionPayload: Codable, Equatable {
    let wsURL: String
    let deviceID: String?
    let relayToken: String?
    let pairingMode: RelayPairingMode?
    let cryptoMode: RelayCryptoMode
    let agentIDs: [String]
}

enum ScannedDaemonConnectionPayload: Equatable {
    case direct(url: String, agentIDs: [String])
    case relay(RelayConnectionPayload)

    var agentIDs: [String] {
        switch self {
        case .direct(_, let agentIDs):
            return agentIDs
        case .relay(let payload):
            return payload.agentIDs
        }
    }
}

func parseScannedDaemonConnectionPayload(from payload: String) -> ScannedDaemonConnectionPayload? {
    let trimmed = payload.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else {
        return nil
    }

    if trimmed.hasPrefix("ws://") || trimmed.hasPrefix("wss://") {
        return .direct(url: trimmed, agentIDs: [])
    }

    guard let components = URLComponents(string: trimmed) else {
        return nil
    }

    guard components.scheme?.lowercased() == "agentchat",
          components.host?.lowercased() == "connect"
    else {
        return nil
    }

    let agentIDs = components.queryItems?
        .first(where: { $0.name == "agents" })?
        .value?
        .split(separator: ",")
        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty } ?? []

    if let relayURL = components.queryItems?.first(where: { $0.name == "relay_url" })?.value?.trimmedNonEmpty,
       relayURL.hasPrefix("ws://") || relayURL.hasPrefix("wss://") {
        let relayToken = components.queryItems?
            .first(where: { $0.name == "relay_token" })?
            .value?
            .trimmedNonEmpty
        let deviceID = components.queryItems?
            .first(where: { $0.name == "device_id" })?
            .value?
            .trimmedNonEmpty

        let rawPairingMode = components.queryItems?
            .first(where: { $0.name == "relay_pairing" })?
            .value?
            .lowercased()
        let pairingMode: RelayPairingMode?
        if let rawPairingMode {
            pairingMode = RelayPairingMode(rawValue: rawPairingMode)
        } else if relayToken == nil, deviceID != nil {
            pairingMode = .dev
        } else {
            pairingMode = nil
        }

        let rawCryptoMode = components.queryItems?
            .first(where: { $0.name == "relay_crypto" })?
            .value?
            .lowercased() ?? "dev"
        guard let cryptoMode = RelayCryptoMode(rawValue: rawCryptoMode) else {
            return nil
        }

        guard relayToken != nil || (pairingMode == .dev && deviceID != nil) else {
            return nil
        }

        return .relay(
            RelayConnectionPayload(
                wsURL: relayURL,
                deviceID: deviceID,
                relayToken: relayToken,
                pairingMode: pairingMode,
                cryptoMode: cryptoMode,
                agentIDs: agentIDs
            )
        )
    }

    guard let urlItem = components.queryItems?.first(where: { $0.name == "url" })?.value?.trimmedNonEmpty,
          urlItem.hasPrefix("ws://") || urlItem.hasPrefix("wss://")
    else {
        return nil
    }

    return .direct(url: urlItem, agentIDs: agentIDs)
}

private extension String {
    var trimmedNonEmpty: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
