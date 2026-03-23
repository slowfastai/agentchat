use std::fmt;

use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::http::header::{HeaderValue, AUTHORIZATION};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Error as WsError, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::info;
use uuid::Uuid;

use agentchat_protocol::now_millis;
use agentchat_protocol::relay::{
    derive_channel_id, RelayEnvelope, RelayError, RelayReady, RelayWireDerivationError,
    RelayWireValidationError, SecureChannelAccept, SecureChannelHello, RELAY_PEER_DAEMON,
    RELAY_WIRE_CRYPTO_SUITE, RELAY_WIRE_PROTOCOL_VERSION,
};
use agentchat_protocol::relay_crypto::{
    decrypt_relay_ciphertext, derive_session_keys, ed25519_public_key, encrypt_relay_plaintext,
    generate_random_secret_bytes, x25519_public_key_base64url, RelaySessionKeys,
};

const PLACEHOLDER_HELLO_EPHEMERAL_PUBLIC_KEY: &str = "M4rG8QwH5dL9uN7wF6sI9r4XbL6R2Q1G0tS4o5y7z8A";
const PLACEHOLDER_HELLO_SIGNATURE: &str =
    "dL4eQ5HL9JfRj7v5k0W0m2hDP3SOV0Vf2Qkq9N4hUq7C8l6QH1i4n8-JkQkzD6xS9nHfB0s9b3cWz0q5u2o5BQ";
const PLACEHOLDER_ACCEPT_EPHEMERAL_PUBLIC_KEY: &str = "Q6kV2T0nY0cVh6sE3qE1Z9lM0xK4B2cJ8mD5uR7aS0U";
const PLACEHOLDER_ACCEPT_SIGNATURE: &str =
    "N5cQ2v0mG8hLs4m7qz0sH1vV7m3Pp8Lh1bQ4t6fA3eD2mR0pC7uL9g2wJ5nS8xY1qZ0hF4rB6dE7tK2mN8w1CA";
const DEFAULT_HELLO_TTL_MS: u64 = 30_000;
const DEFAULT_ACCEPT_TTL_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayClientCryptoConfig {
    pub identity_seed: [u8; 32],
    pub expected_remote_identity_public_key: [u8; 32],
}

impl RelayClientCryptoConfig {
    pub fn local_identity_public_key(&self) -> [u8; 32] {
        ed25519_public_key(&self.identity_seed)
    }
}

pub type Result<T> = std::result::Result<T, RelayClientError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayClientConfig {
    pub ws_url: String,
    pub relay_token: String,
    pub hello_ttl_ms: u64,
    pub hello_ephemeral_public_key: String,
    pub hello_signature: String,
    pub accept_ttl_ms: u64,
    pub accept_ephemeral_public_key: String,
    pub accept_signature: String,
    pub crypto: Option<RelayClientCryptoConfig>,
}

impl RelayClientConfig {
    pub fn new(ws_url: impl Into<String>, relay_token: impl Into<String>) -> Self {
        Self {
            ws_url: ws_url.into(),
            relay_token: relay_token.into(),
            hello_ttl_ms: DEFAULT_HELLO_TTL_MS,
            hello_ephemeral_public_key: PLACEHOLDER_HELLO_EPHEMERAL_PUBLIC_KEY.into(),
            hello_signature: PLACEHOLDER_HELLO_SIGNATURE.into(),
            accept_ttl_ms: DEFAULT_ACCEPT_TTL_MS,
            accept_ephemeral_public_key: PLACEHOLDER_ACCEPT_EPHEMERAL_PUBLIC_KEY.into(),
            accept_signature: PLACEHOLDER_ACCEPT_SIGNATURE.into(),
            crypto: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayClientFrame {
    RelayReady(RelayReady),
    RelayError(RelayError),
    SecureChannelHello(SecureChannelHello),
    SecureChannelAccept(SecureChannelAccept),
    RelayEnvelope(RelayEnvelope),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayHandshakeOutcome {
    pub hello: SecureChannelHello,
    pub accept: SecureChannelAccept,
    pub channel_id: String,
    pub session_keys: Option<RelaySessionKeys>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelayDecryptedEnvelope {
    pub envelope: RelayEnvelope,
    pub plaintext_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRelayChannel {
    local_peer_id: String,
    remote_peer_id: String,
    channel_id: String,
    outbound_key: [u8; 32],
    inbound_key: [u8; 32],
    next_outbound_seq: u64,
    max_inbound_seq: u64,
}

pub struct RelayClient {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    ready: RelayReady,
    config: RelayClientConfig,
    pending_hello_ephemeral_secret: Option<[u8; 32]>,
    active_channel: Option<ActiveRelayChannel>,
}

impl RelayClient {
    pub async fn connect(config: RelayClientConfig) -> Result<Self> {
        let mut request = config
            .ws_url
            .clone()
            .into_client_request()
            .map_err(|err| RelayClientError::new(format!("invalid relay ws url: {err}")))?;
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", config.relay_token)).map_err(|err| {
                RelayClientError::new(format!("invalid relay token header: {err}"))
            })?,
        );

        let (mut stream, _) = connect_async(request).await?;
        let frame = Self::read_next_frame(&mut stream).await?;
        let ready = match frame {
            RelayClientFrame::RelayReady(ready) => ready,
            other => {
                return Err(RelayClientError::new(format!(
                    "expected initial relay_ready frame, got {other:?}"
                )))
            }
        };

        info!(
            peer_id = %ready.peer_id,
            connection_id = %ready.connection_id,
            device_id = %ready.device_id,
            role = %ready.role,
            "connected to relay"
        );

        Ok(Self {
            stream,
            ready,
            config,
            pending_hello_ephemeral_secret: None,
            active_channel: None,
        })
    }

    pub fn ready(&self) -> &RelayReady {
        &self.ready
    }

    pub fn has_active_channel(&self) -> bool {
        self.active_channel.is_some()
    }

    pub fn active_channel_id(&self) -> Option<&str> {
        self.active_channel
            .as_ref()
            .map(|channel| channel.channel_id.as_str())
    }

    pub async fn next_frame(&mut self) -> Result<RelayClientFrame> {
        Self::read_next_frame(&mut self.stream).await
    }

    pub fn build_hello_to_daemon(&mut self) -> Result<SecureChannelHello> {
        if !self.ready.peer_id.starts_with("app:") {
            return Err(RelayClientError::new(format!(
                "relay client is authenticated as {}, expected app:*",
                self.ready.peer_id
            )));
        }

        let timestamp = now_millis();
        let mut hello = SecureChannelHello {
            message_type: "secure_channel_hello".into(),
            id: Uuid::new_v4().to_string(),
            timestamp,
            protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
            from: self.ready.peer_id.clone(),
            to: RELAY_PEER_DAEMON.into(),
            connection_id: self.ready.connection_id.clone(),
            crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
            ephemeral_public_key: self.config.hello_ephemeral_public_key.clone(),
            expires_at: timestamp + self.config.hello_ttl_ms,
            signature: self.config.hello_signature.clone(),
        };

        if let Some(crypto) = &self.config.crypto {
            let ephemeral_secret = generate_random_secret_bytes()?;
            hello.ephemeral_public_key = x25519_public_key_base64url(&ephemeral_secret);
            hello.signature.clear();
            hello.sign_with_identity_seed(&crypto.identity_seed)?;
            self.pending_hello_ephemeral_secret = Some(ephemeral_secret);
        }

        hello.validate()?;
        Ok(hello)
    }

    pub async fn send_hello_to_daemon(&mut self) -> Result<SecureChannelHello> {
        let hello = self.build_hello_to_daemon()?;
        self.send_json(&hello).await?;
        Ok(hello)
    }

    pub async fn wait_for_accept(
        &mut self,
        hello: &SecureChannelHello,
    ) -> Result<RelayHandshakeOutcome> {
        loop {
            match self.next_frame().await? {
                RelayClientFrame::SecureChannelAccept(accept) => {
                    if accept.hello_id != hello.id {
                        return Err(RelayClientError::new(format!(
                            "received secure_channel_accept for unexpected hello_id {}",
                            accept.hello_id
                        )));
                    }

                    let session_keys = if let Some(crypto) = &self.config.crypto {
                        accept.verify_signature_with_public_key(
                            &crypto.expected_remote_identity_public_key,
                        )?;
                        let local_ephemeral_secret = self
                            .pending_hello_ephemeral_secret
                            .take()
                            .ok_or_else(|| {
                                RelayClientError::new(
                                    "missing pending hello ephemeral secret for session key derivation",
                                )
                            })?;
                        let remote_ephemeral_public_key = accept.ephemeral_public_key_bytes()?;
                        Some(derive_session_keys(
                            &local_ephemeral_secret,
                            &remote_ephemeral_public_key,
                            hello,
                            &accept,
                        )?)
                    } else {
                        None
                    };
                    let channel_id = derive_channel_id(hello, &accept)?;
                    if let Some(derived_session_keys) = session_keys.as_ref() {
                        self.activate_channel(
                            RELAY_PEER_DAEMON.to_string(),
                            derived_session_keys.clone(),
                        );
                    }
                    return Ok(RelayHandshakeOutcome {
                        hello: hello.clone(),
                        accept,
                        channel_id,
                        session_keys,
                    });
                }
                RelayClientFrame::RelayError(error) => {
                    return Err(RelayClientError::new(format!(
                        "relay returned {}: {}",
                        error.code, error.message
                    )));
                }
                RelayClientFrame::RelayReady(_)
                | RelayClientFrame::SecureChannelHello(_)
                | RelayClientFrame::RelayEnvelope(_) => continue,
            }
        }
    }

    pub async fn initiate_hello_to_daemon(&mut self) -> Result<RelayHandshakeOutcome> {
        let hello = self.send_hello_to_daemon().await?;
        self.wait_for_accept(&hello).await
    }

    fn build_accept(
        &self,
        hello: &SecureChannelHello,
    ) -> Result<(SecureChannelAccept, Option<[u8; 32]>)> {
        hello.validate()?;

        if self.ready.peer_id != RELAY_PEER_DAEMON {
            return Err(RelayClientError::new(format!(
                "relay client is authenticated as {}, expected daemon",
                self.ready.peer_id
            )));
        }

        let timestamp = now_millis();
        let mut accept = SecureChannelAccept {
            message_type: "secure_channel_accept".into(),
            id: Uuid::new_v4().to_string(),
            timestamp,
            protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
            from: RELAY_PEER_DAEMON.into(),
            to: hello.from.clone(),
            hello_id: hello.id.clone(),
            connection_id: self.ready.connection_id.clone(),
            crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
            ephemeral_public_key: self.config.accept_ephemeral_public_key.clone(),
            expires_at: timestamp + self.config.accept_ttl_ms,
            signature: self.config.accept_signature.clone(),
        };

        let ephemeral_secret = if let Some(crypto) = &self.config.crypto {
            hello.verify_signature_with_public_key(&crypto.expected_remote_identity_public_key)?;
            let ephemeral_secret = generate_random_secret_bytes()?;
            accept.ephemeral_public_key = x25519_public_key_base64url(&ephemeral_secret);
            accept.signature.clear();
            accept.sign_with_identity_seed(&crypto.identity_seed)?;
            Some(ephemeral_secret)
        } else {
            None
        };

        accept.validate()?;
        Ok((accept, ephemeral_secret))
    }

    pub async fn send_accept_for_hello(
        &mut self,
        hello: &SecureChannelHello,
    ) -> Result<RelayHandshakeOutcome> {
        let (accept, local_ephemeral_secret) = self.build_accept(hello)?;
        self.send_json(&accept).await?;
        let session_keys = if let Some(local_ephemeral_secret) = local_ephemeral_secret {
            let remote_ephemeral_public_key = hello.ephemeral_public_key_bytes()?;
            Some(derive_session_keys(
                &local_ephemeral_secret,
                &remote_ephemeral_public_key,
                hello,
                &accept,
            )?)
        } else {
            None
        };
        let channel_id = derive_channel_id(hello, &accept)?;
        if let Some(derived_session_keys) = session_keys.as_ref() {
            self.activate_channel(hello.from.clone(), derived_session_keys.clone());
        }

        Ok(RelayHandshakeOutcome {
            hello: hello.clone(),
            accept,
            channel_id,
            session_keys,
        })
    }

    pub async fn accept_next_hello(&mut self) -> Result<RelayHandshakeOutcome> {
        loop {
            match self.next_frame().await? {
                RelayClientFrame::SecureChannelHello(hello) => {
                    return self.send_accept_for_hello(&hello).await;
                }
                RelayClientFrame::RelayError(error) => {
                    return Err(RelayClientError::new(format!(
                        "relay returned {}: {}",
                        error.code, error.message
                    )));
                }
                RelayClientFrame::RelayEnvelope(_) | RelayClientFrame::SecureChannelAccept(_) => {
                    continue;
                }
                RelayClientFrame::RelayReady(_) => continue,
            }
        }
    }

    pub async fn send_encrypted_json<T: Serialize>(&mut self, value: &T) -> Result<RelayEnvelope> {
        let payload = serde_json::to_value(value)?;
        let envelope = self.encrypt_value_into_envelope(&payload)?;
        self.send_json(&envelope).await?;
        Ok(envelope)
    }

    pub fn decrypt_envelope_json(&mut self, envelope: &RelayEnvelope) -> Result<Value> {
        self.active_channel_mut()?.decrypt_json(envelope)
    }

    pub async fn wait_for_next_decrypted_envelope(&mut self) -> Result<RelayDecryptedEnvelope> {
        loop {
            match self.next_frame().await? {
                RelayClientFrame::RelayEnvelope(envelope) => {
                    let plaintext_json = self.decrypt_envelope_json(&envelope)?;
                    return Ok(RelayDecryptedEnvelope {
                        envelope,
                        plaintext_json,
                    });
                }
                RelayClientFrame::RelayError(error) => {
                    return Err(RelayClientError::new(format!(
                        "relay returned {}: {}",
                        error.code, error.message
                    )));
                }
                RelayClientFrame::RelayReady(_)
                | RelayClientFrame::SecureChannelHello(_)
                | RelayClientFrame::SecureChannelAccept(_) => continue,
            }
        }
    }

    fn activate_channel(&mut self, remote_peer_id: String, session_keys: RelaySessionKeys) {
        self.active_channel = Some(ActiveRelayChannel::new(
            self.ready.peer_id.clone(),
            remote_peer_id,
            session_keys,
        ));
    }

    fn active_channel_mut(&mut self) -> Result<&mut ActiveRelayChannel> {
        self.active_channel.as_mut().ok_or_else(|| {
            RelayClientError::new(
                "no active secure channel; complete secure_channel_hello/accept first",
            )
        })
    }

    fn encrypt_value_into_envelope(&mut self, value: &Value) -> Result<RelayEnvelope> {
        self.active_channel_mut()?.encrypt_json(value)
    }

    pub async fn send_json<T: Serialize>(&mut self, frame: &T) -> Result<()> {
        let json = serde_json::to_string(frame)?;
        self.stream.send(Message::Text(json.into())).await?;
        Ok(())
    }

    async fn read_next_frame(
        stream: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<RelayClientFrame> {
        loop {
            let next = stream.next().await.ok_or_else(|| {
                RelayClientError::new("relay websocket closed before the next frame arrived")
            })?;

            match next {
                Ok(Message::Text(text)) => return parse_frame(&text),
                Ok(Message::Binary(_)) => {
                    return Err(RelayClientError::new(
                        "relay sent a binary frame, but relay-wire requires text JSON frames",
                    ));
                }
                Ok(Message::Ping(payload)) => {
                    stream.send(Message::Pong(payload)).await?;
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    return Err(RelayClientError::new(format!(
                        "relay websocket closed: {frame:?}"
                    )));
                }
                #[allow(deprecated)]
                Ok(Message::Frame(_)) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl ActiveRelayChannel {
    fn new(local_peer_id: String, remote_peer_id: String, session_keys: RelaySessionKeys) -> Self {
        let (outbound_key, inbound_key) = if local_peer_id == RELAY_PEER_DAEMON {
            (
                session_keys.key_daemon_to_app,
                session_keys.key_app_to_daemon,
            )
        } else {
            (
                session_keys.key_app_to_daemon,
                session_keys.key_daemon_to_app,
            )
        };

        Self {
            local_peer_id,
            remote_peer_id,
            channel_id: session_keys.channel_id,
            outbound_key,
            inbound_key,
            next_outbound_seq: 1,
            max_inbound_seq: 0,
        }
    }

    fn encrypt_json(&mut self, value: &Value) -> Result<RelayEnvelope> {
        let seq = self.next_outbound_seq;
        let plaintext = serde_json::to_vec(value)?;
        let ciphertext = encrypt_relay_plaintext(
            &self.outbound_key,
            &self.local_peer_id,
            &self.remote_peer_id,
            &self.channel_id,
            seq,
            &plaintext,
        )?;
        let envelope = RelayEnvelope {
            message_type: "relay_envelope".into(),
            id: Uuid::new_v4().to_string(),
            timestamp: now_millis(),
            from: self.local_peer_id.clone(),
            to: self.remote_peer_id.clone(),
            channel_id: self.channel_id.clone(),
            seq,
            ciphertext,
        };
        envelope.validate()?;
        self.next_outbound_seq = self.next_outbound_seq.saturating_add(1);
        Ok(envelope)
    }

    fn decrypt_json(&mut self, envelope: &RelayEnvelope) -> Result<Value> {
        envelope.validate()?;

        if envelope.from != self.remote_peer_id || envelope.to != self.local_peer_id {
            return Err(RelayClientError::new(format!(
                "INVALID_PEER_PAIR: envelope from/to mismatch for active channel (from={}, to={})",
                envelope.from, envelope.to
            )));
        }
        if envelope.channel_id != self.channel_id {
            return Err(RelayClientError::new(format!(
                "UNKNOWN_CHANNEL: envelope channel_id {} does not match active channel {}",
                envelope.channel_id, self.channel_id
            )));
        }
        if envelope.seq <= self.max_inbound_seq {
            return Err(RelayClientError::new(format!(
                "SEQ_REPLAY: envelope seq {} is not greater than last seen {}",
                envelope.seq, self.max_inbound_seq
            )));
        }

        let plaintext = decrypt_relay_ciphertext(
            &self.inbound_key,
            &envelope.from,
            &envelope.to,
            &envelope.channel_id,
            envelope.seq,
            &envelope.ciphertext,
        )
        .map_err(|error| RelayClientError::new(format!("AAD_AUTH_FAILED: {error}")))?;
        let plaintext_json: Value = serde_json::from_slice(&plaintext)
            .map_err(|error| RelayClientError::new(format!("INVALID_PLAINTEXT_JSON: {error}")))?;
        self.max_inbound_seq = envelope.seq;
        Ok(plaintext_json)
    }
}

fn parse_frame(text: &str) -> Result<RelayClientFrame> {
    let value: Value = serde_json::from_str(text)?;
    let message_type = value
        .get("type")
        .and_then(|value| value.as_str())
        .ok_or_else(|| RelayClientError::new("relay frame is missing a string `type` field"))?;

    match message_type {
        "relay_ready" => {
            let frame: RelayReady = serde_json::from_value(value)?;
            frame.validate()?;
            Ok(RelayClientFrame::RelayReady(frame))
        }
        "relay_error" => {
            let frame: RelayError = serde_json::from_value(value)?;
            Ok(RelayClientFrame::RelayError(frame))
        }
        "secure_channel_hello" => {
            let frame: SecureChannelHello = serde_json::from_value(value)?;
            frame.validate()?;
            Ok(RelayClientFrame::SecureChannelHello(frame))
        }
        "secure_channel_accept" => {
            let frame: SecureChannelAccept = serde_json::from_value(value)?;
            frame.validate()?;
            Ok(RelayClientFrame::SecureChannelAccept(frame))
        }
        "relay_envelope" => {
            let frame: RelayEnvelope = serde_json::from_value(value)?;
            frame.validate()?;
            Ok(RelayClientFrame::RelayEnvelope(frame))
        }
        other => Err(RelayClientError::new(format!(
            "unsupported relay frame type {other:?}"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayClientError {
    pub message: String,
}

impl RelayClientError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RelayClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RelayClientError {}

impl From<WsError> for RelayClientError {
    fn from(value: WsError) -> Self {
        Self::new(value.to_string())
    }
}

impl From<serde_json::Error> for RelayClientError {
    fn from(value: serde_json::Error) -> Self {
        Self::new(value.to_string())
    }
}

impl From<RelayWireValidationError> for RelayClientError {
    fn from(value: RelayWireValidationError) -> Self {
        Self::new(value.to_string())
    }
}

impl From<RelayWireDerivationError> for RelayClientError {
    fn from(value: RelayWireDerivationError) -> Self {
        Self::new(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio_tungstenite::accept_hdr_async;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    use tokio_tungstenite::tungstenite::Message;

    use agentchat_protocol::relay_crypto::{
        derive_session_keys, seed_from_label, x25519_public_key_base64url,
    };

    use super::*;

    const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
    const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

    fn daemon_crypto_config() -> RelayClientCryptoConfig {
        RelayClientCryptoConfig {
            identity_seed: seed_from_label(DEV_DAEMON_IDENTITY_LABEL),
            expected_remote_identity_public_key: ed25519_public_key(&seed_from_label(
                DEV_APP_IDENTITY_LABEL,
            )),
        }
    }

    fn app_crypto_config() -> RelayClientCryptoConfig {
        RelayClientCryptoConfig {
            identity_seed: seed_from_label(DEV_APP_IDENTITY_LABEL),
            expected_remote_identity_public_key: ed25519_public_key(&seed_from_label(
                DEV_DAEMON_IDENTITY_LABEL,
            )),
        }
    }

    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn connect_reads_relay_ready_and_sends_bearer_auth() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen_auth = Arc::new(Mutex::new(None::<String>));
        let seen_auth_server = seen_auth.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_hdr_async(stream, move |request: &Request, response: Response| {
                *seen_auth_server.lock().unwrap() = request
                    .headers()
                    .get(AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_string());
                Ok(response)
            })
            .await
            .unwrap();

            let ready = RelayReady {
                message_type: "relay_ready".into(),
                id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e1001".into(),
                timestamp: 1774257600000,
                protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
                device_id: "dev_local_1".into(),
                role: "daemon".into(),
                peer_id: "daemon".into(),
                connection_id: "rc_PbJt4ZbW2mK6o1Qx".into(),
            };
            ws.send(Message::Text(serde_json::to_string(&ready).unwrap().into()))
                .await
                .unwrap();
        });

        let client = RelayClient::connect(RelayClientConfig::new(
            format!("ws://{addr}"),
            "achdm.dev_local_1.secret_1234567890123456",
        ))
        .await
        .unwrap();

        assert_eq!(client.ready().peer_id, "daemon");
        assert_eq!(client.ready().connection_id, "rc_PbJt4ZbW2mK6o1Qx");
        assert_eq!(
            seen_auth.lock().unwrap().clone(),
            Some("Bearer achdm.dev_local_1.secret_1234567890123456".into())
        );

        server.await.unwrap();
    }

    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn initiates_hello_and_derives_channel_id_from_accept() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (hello_tx, hello_rx) = oneshot::channel::<SecureChannelHello>();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_hdr_async(stream, |_request: &Request, response: Response| {
                Ok(response)
            })
            .await
            .unwrap();

            let ready = RelayReady {
                message_type: "relay_ready".into(),
                id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e1002".into(),
                timestamp: 1774257600000,
                protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
                device_id: "dev_local_1".into(),
                role: "app".into(),
                peer_id: "app:app_local_1".into(),
                connection_id: "rc_8PrnFvN3vM2NgWQY".into(),
            };
            ws.send(Message::Text(serde_json::to_string(&ready).unwrap().into()))
                .await
                .unwrap();

            let message = ws.next().await.unwrap().unwrap();
            let Message::Text(text) = message else {
                panic!("expected hello text frame");
            };
            let hello: SecureChannelHello = serde_json::from_str(&text).unwrap();
            hello
                .verify_signature_with_public_key(&app_crypto_config().local_identity_public_key())
                .unwrap();

            let daemon_ephemeral_secret = seed_from_label("relay-client-test-daemon-ephemeral-v1");
            let mut accept = SecureChannelAccept {
                message_type: "secure_channel_accept".into(),
                id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e3001".into(),
                timestamp: 1774257600200,
                protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
                from: RELAY_PEER_DAEMON.into(),
                to: hello.from.clone(),
                hello_id: hello.id.clone(),
                connection_id: "rc_PbJt4ZbW2mK6o1Qx".into(),
                crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
                ephemeral_public_key: x25519_public_key_base64url(&daemon_ephemeral_secret),
                expires_at: 1774257630200,
                signature: String::new(),
            };
            accept
                .sign_with_identity_seed(&daemon_crypto_config().identity_seed)
                .unwrap();
            ws.send(Message::Text(
                serde_json::to_string(&accept).unwrap().into(),
            ))
            .await
            .unwrap();
            hello_tx.send(hello).unwrap();
        });

        let mut config = RelayClientConfig::new(
            format!("ws://{addr}"),
            "achapp.dev_local_1.app_local_1.secret_1234567890123456",
        );
        config.crypto = Some(app_crypto_config());
        let mut client = RelayClient::connect(config).await.unwrap();
        let outcome = client.initiate_hello_to_daemon().await.unwrap();
        let hello = hello_rx.await.unwrap();

        assert_eq!(outcome.hello, hello);
        assert_eq!(hello.from, "app:app_local_1");
        assert_eq!(hello.to, RELAY_PEER_DAEMON);
        assert_eq!(hello.connection_id, client.ready().connection_id);
        assert_eq!(outcome.accept.hello_id, hello.id);
        assert_eq!(
            outcome.channel_id,
            derive_channel_id(&hello, &outcome.accept).unwrap()
        );
        assert!(outcome.session_keys.is_some());

        server.await.unwrap();
    }

    #[test]
    fn active_channel_encrypts_decrypts_and_rejects_replay() {
        let app_identity_seed = seed_from_label("relay-client-active-channel-app-identity-v1");
        let daemon_identity_seed =
            seed_from_label("relay-client-active-channel-daemon-identity-v1");
        let app_ephemeral_secret = seed_from_label("relay-client-active-channel-app-ephemeral-v1");
        let daemon_ephemeral_secret =
            seed_from_label("relay-client-active-channel-daemon-ephemeral-v1");

        let mut hello = SecureChannelHello {
            message_type: "secure_channel_hello".into(),
            id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e2001".into(),
            timestamp: 1774257600100,
            protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
            from: "app:app_local_1".into(),
            to: RELAY_PEER_DAEMON.into(),
            connection_id: "rc_8PrnFvN3vM2NgWQY".into(),
            crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
            ephemeral_public_key: x25519_public_key_base64url(&app_ephemeral_secret),
            expires_at: 1774257630100,
            signature: String::new(),
        };
        hello.sign_with_identity_seed(&app_identity_seed).unwrap();

        let mut accept = SecureChannelAccept {
            message_type: "secure_channel_accept".into(),
            id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e3001".into(),
            timestamp: 1774257600200,
            protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
            from: RELAY_PEER_DAEMON.into(),
            to: hello.from.clone(),
            hello_id: hello.id.clone(),
            connection_id: "rc_PbJt4ZbW2mK6o1Qx".into(),
            crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
            ephemeral_public_key: x25519_public_key_base64url(&daemon_ephemeral_secret),
            expires_at: 1774257630200,
            signature: String::new(),
        };
        accept
            .sign_with_identity_seed(&daemon_identity_seed)
            .unwrap();

        let app_session_keys = derive_session_keys(
            &app_ephemeral_secret,
            &accept.ephemeral_public_key_bytes().unwrap(),
            &hello,
            &accept,
        )
        .unwrap();
        let daemon_session_keys = derive_session_keys(
            &daemon_ephemeral_secret,
            &hello.ephemeral_public_key_bytes().unwrap(),
            &hello,
            &accept,
        )
        .unwrap();

        let mut app_channel = ActiveRelayChannel::new(
            hello.from.clone(),
            RELAY_PEER_DAEMON.into(),
            app_session_keys,
        );
        let mut daemon_channel = ActiveRelayChannel::new(
            RELAY_PEER_DAEMON.into(),
            hello.from.clone(),
            daemon_session_keys,
        );

        let envelope = app_channel
            .encrypt_json(
                &serde_json::json!({"type":"send_prompt","payload":{"text":"hello relay"}}),
            )
            .unwrap();
        let plaintext = daemon_channel.decrypt_json(&envelope).unwrap();
        assert_eq!(
            plaintext,
            serde_json::json!({"type":"send_prompt","payload":{"text":"hello relay"}})
        );

        let replay_error = daemon_channel.decrypt_json(&envelope).unwrap_err();
        assert!(replay_error.message.contains("SEQ_REPLAY"));
    }

    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn accepts_forwarded_hello_and_derives_channel_id() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (accept_tx, accept_rx) = oneshot::channel::<SecureChannelAccept>();
        let app_ephemeral_secret = seed_from_label("relay-client-test-app-ephemeral-v1");
        let mut hello = SecureChannelHello {
            message_type: "secure_channel_hello".into(),
            id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e2001".into(),
            timestamp: 1774257600100,
            protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
            from: "app:app_local_1".into(),
            to: RELAY_PEER_DAEMON.into(),
            connection_id: "rc_8PrnFvN3vM2NgWQY".into(),
            crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
            ephemeral_public_key: x25519_public_key_base64url(&app_ephemeral_secret),
            expires_at: 1774257630100,
            signature: String::new(),
        };
        hello
            .sign_with_identity_seed(&app_crypto_config().identity_seed)
            .unwrap();
        let hello_server = hello.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_hdr_async(stream, |_request: &Request, response: Response| {
                Ok(response)
            })
            .await
            .unwrap();

            let ready = RelayReady {
                message_type: "relay_ready".into(),
                id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e1001".into(),
                timestamp: 1774257600000,
                protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
                device_id: "dev_local_1".into(),
                role: "daemon".into(),
                peer_id: "daemon".into(),
                connection_id: "rc_PbJt4ZbW2mK6o1Qx".into(),
            };
            ws.send(Message::Text(serde_json::to_string(&ready).unwrap().into()))
                .await
                .unwrap();
            ws.send(Message::Text(
                serde_json::to_string(&hello_server).unwrap().into(),
            ))
            .await
            .unwrap();

            let message = ws.next().await.unwrap().unwrap();
            let Message::Text(text) = message else {
                panic!("expected accept text frame");
            };
            let accept: SecureChannelAccept = serde_json::from_str(&text).unwrap();
            accept_tx.send(accept).unwrap();
        });

        let mut config = RelayClientConfig::new(
            format!("ws://{addr}"),
            "achdm.dev_local_1.secret_1234567890123456",
        );
        config.crypto = Some(daemon_crypto_config());
        let mut client = RelayClient::connect(config).await.unwrap();
        let outcome = client.accept_next_hello().await.unwrap();
        let accept = accept_rx.await.unwrap();

        assert_eq!(outcome.accept, accept);
        assert_eq!(accept.to, hello.from);
        assert_eq!(accept.hello_id, hello.id);
        assert_eq!(accept.connection_id, client.ready().connection_id);
        assert_eq!(
            outcome.channel_id,
            derive_channel_id(&hello, &accept).unwrap()
        );
        let expected_session_keys = derive_session_keys(
            &app_ephemeral_secret,
            &accept.ephemeral_public_key_bytes().unwrap(),
            &hello,
            &accept,
        )
        .unwrap();
        assert_eq!(outcome.session_keys, Some(expected_session_keys));

        server.await.unwrap();
    }
}
