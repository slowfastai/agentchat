import CryptoKit
import Foundation

private let relayWireProtocolVersion = "relay-wire/0.1"
private let relayWireCryptoSuite = "X25519_Ed25519_ChaCha20Poly1305_HKDFSHA256_v1"
private let relayChannelContext = Data("agentchat-channel".utf8)
private let relayAppToDaemonInfo = Data("agentchat-relay a2d v1".utf8)
private let relayDaemonToAppInfo = Data("agentchat-relay d2a v1".utf8)
private let relayDaemonPeerID = "daemon"
private let relayHelloTTLMS: UInt64 = 30_000
private let relayAcceptTagLength = 16

private let devDaemonIdentityLabel = "agentchat-dev-daemon-identity-v1"
private let devAppIdentityLabel = "agentchat-dev-app-identity-v1"

struct ResolvedRelayConnection {
    let wsURL: URL
    let relayToken: String
    let cryptoMode: RelayCryptoMode
}

enum RelayInboundMessage {
    case applicationJSON(String)
    case relayError(String)
    case ignored
}

enum RelayTransportError: LocalizedError {
    case invalidRelayURL(String)
    case invalidPairingURL(String)
    case missingRelayCredentials
    case pairingFailed(String)
    case invalidFrame(String)
    case unsupportedFrame(String)
    case invalidRelayReady(String)
    case invalidSignature(String)
    case invalidUTF8Payload
    case cryptoFailure(String)

    var errorDescription: String? {
        switch self {
        case .invalidRelayURL(let value):
            return "Invalid relay URL: \(value)"
        case .invalidPairingURL(let value):
            return "Invalid relay pairing URL derived from \(value)"
        case .missingRelayCredentials:
            return "Relay connection is missing a token, pairing ticket, or pairable device ID"
        case .pairingFailed(let message):
            return "Relay pairing failed: \(message)"
        case .invalidFrame(let message):
            return "Invalid relay frame: \(message)"
        case .unsupportedFrame(let type):
            return "Unsupported relay frame type: \(type)"
        case .invalidRelayReady(let message):
            return "Invalid relay_ready frame: \(message)"
        case .invalidSignature(let message):
            return "Relay signature verification failed: \(message)"
        case .invalidUTF8Payload:
            return "Relay payload was not valid UTF-8 JSON"
        case .cryptoFailure(let message):
            return "Relay crypto failed: \(message)"
        }
    }
}

extension RelayConnectionPayload {
    func resolve(appInstallationID: String, appName: String) async throws -> ResolvedRelayConnection {
        guard let relayURL = URL(string: wsURL) else {
            throw RelayTransportError.invalidRelayURL(wsURL)
        }

        if let relayToken, !relayToken.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return ResolvedRelayConnection(wsURL: relayURL, relayToken: relayToken, cryptoMode: cryptoMode)
        }

        let pairingResponse: RelayPairResponse
        switch pairingMode {
        case .dev:
            guard let deviceID else {
                throw RelayTransportError.missingRelayCredentials
            }
            pairingResponse = try await RelayPairingClient.pairDev(
                pairURL: try Self.pairingURL(from: relayURL, path: "/v1/dev/pair"),
                deviceID: deviceID,
                appInstallationID: appInstallationID,
                appName: appName
            )
        case .claim:
            guard let pairingTicket else {
                throw RelayTransportError.missingRelayCredentials
            }
            pairingResponse = try await RelayPairingClient.claimTicket(
                claimURL: try Self.pairingURL(from: relayURL, path: "/v1/pairing/claim"),
                pairingTicket: pairingTicket,
                appInstallationID: appInstallationID,
                appName: appName
            )
        case .none:
            throw RelayTransportError.missingRelayCredentials
        }

        guard let pairedRelayURL = URL(string: pairingResponse.wsURL) else {
            throw RelayTransportError.invalidRelayURL(pairingResponse.wsURL)
        }

        return ResolvedRelayConnection(
            wsURL: pairedRelayURL,
            relayToken: pairingResponse.relayToken,
            cryptoMode: cryptoMode
        )
    }

    private static func pairingURL(from relayURL: URL, path: String) throws -> URL {
        guard var components = URLComponents(url: relayURL, resolvingAgainstBaseURL: false) else {
            throw RelayTransportError.invalidPairingURL(relayURL.absoluteString)
        }

        switch components.scheme?.lowercased() {
        case "wss":
            components.scheme = "https"
        case "ws":
            components.scheme = "http"
        default:
            throw RelayTransportError.invalidPairingURL(relayURL.absoluteString)
        }

        components.path = path
        components.query = nil
        components.fragment = nil

        guard let url = components.url else {
            throw RelayTransportError.invalidPairingURL(relayURL.absoluteString)
        }
        return url
    }
}

struct RelayAppSession {
    let localPeerID: String
    let remotePeerID: String
    let channelID: String
    private let outboundKeyData: Data
    private let inboundKeyData: Data
    private var nextOutboundSeq: UInt64
    private var maxInboundSeq: UInt64

    fileprivate init(
        localPeerID: String,
        remotePeerID: String,
        channelID: String,
        outboundKeyData: Data,
        inboundKeyData: Data,
        nextOutboundSeq: UInt64,
        maxInboundSeq: UInt64
    ) {
        self.localPeerID = localPeerID
        self.remotePeerID = remotePeerID
        self.channelID = channelID
        self.outboundKeyData = outboundKeyData
        self.inboundKeyData = inboundKeyData
        self.nextOutboundSeq = nextOutboundSeq
        self.maxInboundSeq = maxInboundSeq
    }

    static func handshake(
        over task: URLSessionWebSocketTask,
        resolvedConnection: ResolvedRelayConnection
    ) async throws -> RelayAppSession {
        let readyText = try await receiveText(from: task)
        let ready = try decodeFrame(RelayReadyFrame.self, from: readyText)

        guard ready.type == "relay_ready" else {
            throw RelayTransportError.invalidRelayReady("expected relay_ready, got \(ready.type)")
        }
        guard ready.peerID.hasPrefix("app:") else {
            throw RelayTransportError.invalidRelayReady("expected app:* peer_id, got \(ready.peerID)")
        }

        let cryptoConfig = try relayCryptoConfig(for: resolvedConnection.cryptoMode)
        let ephemeralPrivateKey = Curve25519.KeyAgreement.PrivateKey()
        let hello = try buildHello(ready: ready, cryptoConfig: cryptoConfig, ephemeralPrivateKey: ephemeralPrivateKey)
        try await sendText(jsonString(hello), over: task)

        while true {
            let nextText = try await receiveText(from: task)
            let frameType = try decodeFrameType(from: nextText)

            switch frameType {
            case "secure_channel_accept":
                let accept = try decodeFrame(SecureChannelAcceptFrame.self, from: nextText)
                guard accept.helloID == hello.id else {
                    throw RelayTransportError.invalidFrame(
                        "secure_channel_accept hello_id \(accept.helloID) did not match \(hello.id)"
                    )
                }
                try verifyAccept(accept, cryptoConfig: cryptoConfig)
                return try deriveSession(
                    ready: ready,
                    hello: hello,
                    accept: accept,
                    localEphemeralPrivateKey: ephemeralPrivateKey
                )
            case "relay_error":
                let frame = try decodeFrame(RelayErrorFrame.self, from: nextText)
                throw RelayTransportError.invalidFrame("\(frame.code): \(frame.message)")
            case "relay_ready", "secure_channel_hello", "relay_envelope":
                continue
            default:
                throw RelayTransportError.unsupportedFrame(frameType)
            }
        }
    }

    mutating func encryptJSONString(_ text: String) throws -> String {
        let plaintext = Data(text.utf8)
        let seq = nextOutboundSeq
        let aad = try relayAADData(from: localPeerID, to: remotePeerID, channelID: channelID, seq: seq)
        let nonce = try ChaChaPoly.Nonce(data: relayNonceData(for: seq))
        let sealedBox = try ChaChaPoly.seal(
            plaintext,
            using: SymmetricKey(data: outboundKeyData),
            nonce: nonce,
            authenticating: aad
        )

        let frame = RelayEnvelopeFrame(
            type: "relay_envelope",
            id: uuidString(),
            timestamp: nowMS(),
            from: localPeerID,
            to: remotePeerID,
            channelID: channelID,
            seq: seq,
            ciphertext: encodeBase64URL(sealedBox.ciphertext + sealedBox.tag)
        )

        nextOutboundSeq += 1
        return try jsonString(frame)
    }

    mutating func consumeIncomingFrame(text: String) throws -> RelayInboundMessage {
        let frameType = try decodeFrameType(from: text)

        switch frameType {
        case "relay_envelope":
            let envelope = try decodeFrame(RelayEnvelopeFrame.self, from: text)
            return .applicationJSON(try decryptEnvelope(envelope))
        case "relay_error":
            let frame = try decodeFrame(RelayErrorFrame.self, from: text)
            return .relayError("\(frame.code): \(frame.message)")
        case "relay_ready", "secure_channel_hello", "secure_channel_accept":
            return .ignored
        default:
            throw RelayTransportError.unsupportedFrame(frameType)
        }
    }

    private mutating func decryptEnvelope(_ envelope: RelayEnvelopeFrame) throws -> String {
        guard envelope.from == remotePeerID, envelope.to == localPeerID else {
            throw RelayTransportError.invalidFrame(
                "relay_envelope from/to mismatch for active channel"
            )
        }
        guard envelope.channelID == channelID else {
            throw RelayTransportError.invalidFrame(
                "relay_envelope channel_id \(envelope.channelID) did not match \(channelID)"
            )
        }
        guard envelope.seq > maxInboundSeq else {
            throw RelayTransportError.invalidFrame(
                "relay_envelope seq \(envelope.seq) replayed last seen \(maxInboundSeq)"
            )
        }

        let rawCiphertext = try decodeBase64URL(envelope.ciphertext)
        guard rawCiphertext.count >= relayAcceptTagLength else {
            throw RelayTransportError.invalidFrame("relay_envelope ciphertext too short")
        }

        let ciphertext = rawCiphertext.prefix(rawCiphertext.count - relayAcceptTagLength)
        let tag = rawCiphertext.suffix(relayAcceptTagLength)
        let aad = try relayAADData(
            from: envelope.from,
            to: envelope.to,
            channelID: envelope.channelID,
            seq: envelope.seq
        )
        let nonce = try ChaChaPoly.Nonce(data: relayNonceData(for: envelope.seq))
        let sealedBox = try ChaChaPoly.SealedBox(
            nonce: nonce,
            ciphertext: ciphertext,
            tag: tag
        )
        let plaintext = try ChaChaPoly.open(
            sealedBox,
            using: SymmetricKey(data: inboundKeyData),
            authenticating: aad
        )

        guard let text = String(data: plaintext, encoding: .utf8) else {
            throw RelayTransportError.invalidUTF8Payload
        }

        maxInboundSeq = envelope.seq
        return text
    }
}

private struct RelayPairingClient {
    static func pairDev(
        pairURL: URL,
        deviceID: String,
        appInstallationID: String,
        appName: String
    ) async throws -> RelayPairResponse {
        try await postPairRequest(
            url: pairURL,
            body: RelayDevPairRequest(
                deviceID: deviceID,
                appInstallationID: appInstallationID,
                appName: appName
            )
        )
    }

    static func claimTicket(
        claimURL: URL,
        pairingTicket: String,
        appInstallationID: String,
        appName: String
    ) async throws -> RelayPairResponse {
        try await postPairRequest(
            url: claimURL,
            body: RelayClaimPairRequest(
                pairingTicket: pairingTicket,
                appInstallationID: appInstallationID,
                appName: appName
            )
        )
    }

    private static func postPairRequest<Body: Encodable>(url: URL, body: Body) async throws -> RelayPairResponse {
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(body)

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw RelayTransportError.pairingFailed("relay pair endpoint returned no HTTP response")
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            let body = String(data: data, encoding: .utf8) ?? ""
            throw RelayTransportError.pairingFailed("HTTP \(httpResponse.statusCode) \(body)")
        }

        return try JSONDecoder().decode(RelayPairResponse.self, from: data)
    }
}

private struct RelayReadyFrame: Decodable {
    let type: String
    let id: String
    let timestamp: UInt64
    let protocolVersion: String
    let deviceID: String
    let role: String
    let peerID: String
    let connectionID: String

    enum CodingKeys: String, CodingKey {
        case type
        case id
        case timestamp
        case protocolVersion = "protocol_version"
        case deviceID = "device_id"
        case role
        case peerID = "peer_id"
        case connectionID = "connection_id"
    }
}

private struct RelayErrorFrame: Decodable {
    let type: String
    let code: String
    let message: String
}

private struct SecureChannelHelloFrame: Encodable {
    let type: String
    let id: String
    let timestamp: UInt64
    let protocolVersion: String
    let from: String
    let to: String
    let connectionID: String
    let cryptoSuite: String
    let ephemeralPublicKey: String
    let expiresAt: UInt64
    let signature: String

    enum CodingKeys: String, CodingKey {
        case type
        case id
        case timestamp
        case protocolVersion = "protocol_version"
        case from
        case to
        case connectionID = "connection_id"
        case cryptoSuite = "crypto_suite"
        case ephemeralPublicKey = "ephemeral_public_key"
        case expiresAt = "expires_at"
        case signature
    }
}

private struct SecureChannelAcceptFrame: Decodable {
    let type: String
    let id: String
    let timestamp: UInt64
    let protocolVersion: String
    let from: String
    let to: String
    let helloID: String
    let connectionID: String
    let cryptoSuite: String
    let ephemeralPublicKey: String
    let expiresAt: UInt64
    let signature: String

    enum CodingKeys: String, CodingKey {
        case type
        case id
        case timestamp
        case protocolVersion = "protocol_version"
        case from
        case to
        case helloID = "hello_id"
        case connectionID = "connection_id"
        case cryptoSuite = "crypto_suite"
        case ephemeralPublicKey = "ephemeral_public_key"
        case expiresAt = "expires_at"
        case signature
    }
}

private struct RelayEnvelopeFrame: Codable {
    let type: String
    let id: String
    let timestamp: UInt64
    let from: String
    let to: String
    let channelID: String
    let seq: UInt64
    let ciphertext: String

    enum CodingKeys: String, CodingKey {
        case type
        case id
        case timestamp
        case from
        case to
        case channelID = "channel_id"
        case seq
        case ciphertext
    }
}

private struct RelayDevPairRequest: Encodable {
    let deviceID: String
    let appInstallationID: String
    let appName: String

    enum CodingKeys: String, CodingKey {
        case deviceID = "device_id"
        case appInstallationID = "app_installation_id"
        case appName = "app_name"
    }
}

private struct RelayClaimPairRequest: Encodable {
    let pairingTicket: String
    let appInstallationID: String
    let appName: String

    enum CodingKeys: String, CodingKey {
        case pairingTicket = "pairing_ticket"
        case appInstallationID = "app_installation_id"
        case appName = "app_name"
    }
}

private struct RelayPairResponse: Decodable {
    let deviceID: String
    let appInstallationID: String
    let peerID: String
    let relayToken: String
    let wsURL: String

    enum CodingKeys: String, CodingKey {
        case deviceID = "device_id"
        case appInstallationID = "app_installation_id"
        case peerID = "peer_id"
        case relayToken = "relay_token"
        case wsURL = "ws_url"
    }
}

private struct RelayFrameTypeEnvelope: Decodable {
    let type: String
}

private struct RelayCryptoConfig {
    let identitySeed: Data
    let expectedRemoteIdentityPublicKey: Data
}

private func relayCryptoConfig(for mode: RelayCryptoMode) throws -> RelayCryptoConfig {
    switch mode {
    case .dev:
        return RelayCryptoConfig(
            identitySeed: seedFromLabel(devAppIdentityLabel),
            expectedRemoteIdentityPublicKey: signingPublicKey(seed: seedFromLabel(devDaemonIdentityLabel))
        )
    }
}

private func buildHello(
    ready: RelayReadyFrame,
    cryptoConfig: RelayCryptoConfig,
    ephemeralPrivateKey: Curve25519.KeyAgreement.PrivateKey
) throws -> SecureChannelHelloFrame {
    let timestamp = nowMS()
    let expiresAt = timestamp + relayHelloTTLMS
    let hello = SecureChannelHelloFrame(
        type: "secure_channel_hello",
        id: uuidString(),
        timestamp: timestamp,
        protocolVersion: relayWireProtocolVersion,
        from: ready.peerID,
        to: relayDaemonPeerID,
        connectionID: ready.connectionID,
        cryptoSuite: relayWireCryptoSuite,
        ephemeralPublicKey: encodeBase64URL(ephemeralPrivateKey.publicKey.rawRepresentation),
        expiresAt: expiresAt,
        signature: ""
    )

    let signatureInput = [
        "type": "secure_channel_hello",
        "protocol_version": relayWireProtocolVersion,
        "from": ready.peerID,
        "to": relayDaemonPeerID,
        "connection_id": ready.connectionID,
        "crypto_suite": relayWireCryptoSuite,
        "ephemeral_public_key": encodeBase64URL(ephemeralPrivateKey.publicKey.rawRepresentation),
        "expires_at": NSNumber(value: expiresAt),
    ] as [String: Any]
    let canonicalData = try canonicalJSONData(signatureInput)
    let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: cryptoConfig.identitySeed)
    let signature = try privateKey.signature(for: canonicalData)

    return SecureChannelHelloFrame(
        type: hello.type,
        id: hello.id,
        timestamp: hello.timestamp,
        protocolVersion: hello.protocolVersion,
        from: hello.from,
        to: hello.to,
        connectionID: hello.connectionID,
        cryptoSuite: hello.cryptoSuite,
        ephemeralPublicKey: hello.ephemeralPublicKey,
        expiresAt: hello.expiresAt,
        signature: encodeBase64URL(signature)
    )
}

private func verifyAccept(
    _ accept: SecureChannelAcceptFrame,
    cryptoConfig: RelayCryptoConfig
) throws {
    let signatureInput = [
        "type": "secure_channel_accept",
        "protocol_version": accept.protocolVersion,
        "from": accept.from,
        "to": accept.to,
        "hello_id": accept.helloID,
        "connection_id": accept.connectionID,
        "crypto_suite": accept.cryptoSuite,
        "ephemeral_public_key": accept.ephemeralPublicKey,
        "expires_at": NSNumber(value: accept.expiresAt),
    ] as [String: Any]
    let canonicalData = try canonicalJSONData(signatureInput)
    let signatureData = try decodeBase64URL(accept.signature)
    let publicKey = try Curve25519.Signing.PublicKey(rawRepresentation: cryptoConfig.expectedRemoteIdentityPublicKey)

    guard publicKey.isValidSignature(signatureData, for: canonicalData) else {
        throw RelayTransportError.invalidSignature("secure_channel_accept signature did not verify")
    }
}

private func deriveSession(
    ready: RelayReadyFrame,
    hello: SecureChannelHelloFrame,
    accept: SecureChannelAcceptFrame,
    localEphemeralPrivateKey: Curve25519.KeyAgreement.PrivateKey
) throws -> RelayAppSession {
    let remoteEphemeralPublicKey = try Curve25519.KeyAgreement.PublicKey(
        rawRepresentation: decodeBase64URL(accept.ephemeralPublicKey)
    )
    let sharedSecret = try localEphemeralPrivateKey.sharedSecretFromKeyAgreement(with: remoteEphemeralPublicKey)
    let sharedSecretData = sharedSecret.withUnsafeBytes { Data($0) }

    let helloCanonicalData = try canonicalJSONData([
        "type": "secure_channel_hello",
        "protocol_version": hello.protocolVersion,
        "from": hello.from,
        "to": hello.to,
        "connection_id": hello.connectionID,
        "crypto_suite": hello.cryptoSuite,
        "ephemeral_public_key": hello.ephemeralPublicKey,
        "expires_at": NSNumber(value: hello.expiresAt),
    ])
    let acceptCanonicalData = try canonicalJSONData([
        "type": "secure_channel_accept",
        "protocol_version": accept.protocolVersion,
        "from": accept.from,
        "to": accept.to,
        "hello_id": accept.helloID,
        "connection_id": accept.connectionID,
        "crypto_suite": accept.cryptoSuite,
        "ephemeral_public_key": accept.ephemeralPublicKey,
        "expires_at": NSNumber(value: accept.expiresAt),
    ])

    let transcriptHash = Data(SHA256.hash(data: helloCanonicalData + acceptCanonicalData))
    let channelID = encodeBase64URL(Data(SHA256.hash(data: relayChannelContext + transcriptHash)).prefix(16))
    let keyAppToDaemon = hkdfKey(inputKeyMaterial: sharedSecretData, salt: transcriptHash, info: relayAppToDaemonInfo)
    let keyDaemonToApp = hkdfKey(inputKeyMaterial: sharedSecretData, salt: transcriptHash, info: relayDaemonToAppInfo)

    return RelayAppSession(
        localPeerID: ready.peerID,
        remotePeerID: relayDaemonPeerID,
        channelID: channelID,
        outboundKeyData: keyAppToDaemon,
        inboundKeyData: keyDaemonToApp,
        nextOutboundSeq: 1,
        maxInboundSeq: 0
    )
}

private func relayAADData(from: String, to: String, channelID: String, seq: UInt64) throws -> Data {
    try canonicalJSONData([
        "type": "relay_envelope",
        "from": from,
        "to": to,
        "channel_id": channelID,
        "seq": NSNumber(value: seq),
    ])
}

private func hkdfKey(inputKeyMaterial: Data, salt: Data, info: Data) -> Data {
    let key = HKDF<SHA256>.deriveKey(
        inputKeyMaterial: SymmetricKey(data: inputKeyMaterial),
        salt: salt,
        info: info,
        outputByteCount: 32
    )
    return key.withUnsafeBytes { Data($0) }
}

private func signingPublicKey(seed: Data) -> Data {
    (try? Curve25519.Signing.PrivateKey(rawRepresentation: seed).publicKey.rawRepresentation) ?? Data()
}

private func seedFromLabel(_ label: String) -> Data {
    Data(SHA256.hash(data: Data(label.utf8)))
}

private func relayNonceData(for seq: UInt64) -> Data {
    var nonce = Data(repeating: 0, count: 12)
    var bigEndianSeq = seq.bigEndian
    withUnsafeBytes(of: &bigEndianSeq) { buffer in
        nonce.replaceSubrange(4..<12, with: buffer)
    }
    return nonce
}

private func canonicalJSONData(_ object: Any) throws -> Data {
    guard JSONSerialization.isValidJSONObject(object) else {
        throw RelayTransportError.cryptoFailure("object could not be canonicalized to JSON")
    }

    return try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
}

private func encodeBase64URL(_ data: Data) -> String {
    data.base64EncodedString()
        .replacingOccurrences(of: "+", with: "-")
        .replacingOccurrences(of: "/", with: "_")
        .replacingOccurrences(of: "=", with: "")
}

private func decodeBase64URL(_ value: String) throws -> Data {
    let paddedLength = ((value.count + 3) / 4) * 4
    let padded = value
        .replacingOccurrences(of: "-", with: "+")
        .replacingOccurrences(of: "_", with: "/")
        .padding(toLength: paddedLength, withPad: "=", startingAt: 0)

    guard let data = Data(base64Encoded: padded) else {
        throw RelayTransportError.cryptoFailure("failed to decode base64url value")
    }
    return data
}

private func decodeFrame<T: Decodable>(_ type: T.Type, from text: String) throws -> T {
    let data = Data(text.utf8)
    do {
        return try JSONDecoder().decode(type, from: data)
    } catch {
        throw RelayTransportError.invalidFrame(error.localizedDescription)
    }
}

private func decodeFrameType(from text: String) throws -> String {
    try decodeFrame(RelayFrameTypeEnvelope.self, from: text).type
}

private func jsonString<T: Encodable>(_ value: T) throws -> String {
    let data = try JSONEncoder().encode(value)
    guard let string = String(data: data, encoding: .utf8) else {
        throw RelayTransportError.invalidUTF8Payload
    }
    return string
}

private func sendText(_ text: String, over task: URLSessionWebSocketTask) async throws {
    try await task.send(.string(text))
}

private func receiveText(from task: URLSessionWebSocketTask) async throws -> String {
    let message = try await task.receive()
    switch message {
    case .string(let text):
        return text
    case .data(let data):
        guard let text = String(data: data, encoding: .utf8) else {
            throw RelayTransportError.invalidUTF8Payload
        }
        return text
    @unknown default:
        throw RelayTransportError.invalidFrame("received unknown websocket message type")
    }
}

private func uuidString() -> String {
    UUID().uuidString.lowercased()
}

private func nowMS() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1000)
}
