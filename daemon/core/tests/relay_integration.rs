use futures::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use agentchat_core::relay_client::{RelayClient, RelayClientConfig, RelayClientCryptoConfig};
use agentchat_protocol::relay::{
    RelayEnvelope, RelayReady, SecureChannelAccept, SecureChannelHello, RELAY_PEER_DAEMON,
    RELAY_WIRE_CRYPTO_SUITE, RELAY_WIRE_PROTOCOL_VERSION,
};
use agentchat_protocol::relay_crypto::{
    derive_session_keys, ed25519_public_key, encrypt_relay_plaintext, seed_from_label,
    x25519_public_key_base64url, RelaySessionKeys,
};

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";
const DAEMON_CONNECTION_ID: &str = "rc_PbJt4ZbW2mK6o1Qx";
const APP_CONNECTION_ID: &str = "rc_8PrnFvN3vM2NgWQY";

type TestWebSocket = WebSocketStream<TcpStream>;

fn daemon_crypto_identity_seed() -> [u8; 32] {
    seed_from_label(DEV_DAEMON_IDENTITY_LABEL)
}

fn app_crypto_identity_seed() -> [u8; 32] {
    seed_from_label(DEV_APP_IDENTITY_LABEL)
}

fn app_crypto_config() -> RelayClientCryptoConfig {
    RelayClientCryptoConfig {
        identity_seed: app_crypto_identity_seed(),
        expected_remote_identity_public_key: ed25519_public_key(&daemon_crypto_identity_seed()),
    }
}

fn app_client_config(addr: std::net::SocketAddr) -> RelayClientConfig {
    let mut config = RelayClientConfig::new(
        format!("ws://{addr}"),
        "achapp.dev_local_1.app_local_1.secret_1234567890123456",
    );
    config.crypto = Some(app_crypto_config());
    config
}

fn relay_ready_for_app() -> RelayReady {
    RelayReady {
        message_type: "relay_ready".into(),
        id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e1002".into(),
        timestamp: 1774257600000,
        protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
        device_id: "dev_local_1".into(),
        role: "app".into(),
        peer_id: "app:app_local_1".into(),
        connection_id: APP_CONNECTION_ID.into(),
    }
}

async fn send_json<T: Serialize>(ws: &mut TestWebSocket, value: &T) {
    ws.send(Message::Text(serde_json::to_string(value).unwrap().into()))
        .await
        .unwrap();
}

async fn recv_hello(ws: &mut TestWebSocket) -> SecureChannelHello {
    let message = ws.next().await.unwrap().unwrap();
    let Message::Text(text) = message else {
        panic!("expected hello text frame");
    };
    serde_json::from_str(&text).unwrap()
}

fn build_signed_accept_and_session_keys(
    hello: &SecureChannelHello,
    daemon_ephemeral_label: &str,
) -> (SecureChannelAccept, RelaySessionKeys) {
    hello
        .verify_signature_with_public_key(&app_crypto_config().local_identity_public_key())
        .unwrap();

    let daemon_ephemeral_secret = seed_from_label(daemon_ephemeral_label);
    let mut accept = SecureChannelAccept {
        message_type: "secure_channel_accept".into(),
        id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e3001".into(),
        timestamp: 1774257600200,
        protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
        from: RELAY_PEER_DAEMON.into(),
        to: hello.from.clone(),
        hello_id: hello.id.clone(),
        connection_id: DAEMON_CONNECTION_ID.into(),
        crypto_suite: RELAY_WIRE_CRYPTO_SUITE.into(),
        ephemeral_public_key: x25519_public_key_base64url(&daemon_ephemeral_secret),
        expires_at: 1774257630200,
        signature: String::new(),
    };
    accept
        .sign_with_identity_seed(&daemon_crypto_identity_seed())
        .unwrap();

    let session_keys = derive_session_keys(
        &daemon_ephemeral_secret,
        &hello.ephemeral_public_key_bytes().unwrap(),
        hello,
        &accept,
    )
    .unwrap();

    (accept, session_keys)
}

fn build_daemon_envelope(
    session_keys: &RelaySessionKeys,
    to_peer_id: &str,
    seq: u64,
    plaintext_json: Value,
) -> RelayEnvelope {
    let plaintext_bytes = serde_json::to_vec(&plaintext_json).unwrap();
    let ciphertext = encrypt_relay_plaintext(
        &session_keys.key_daemon_to_app,
        RELAY_PEER_DAEMON,
        to_peer_id,
        &session_keys.channel_id,
        seq,
        &plaintext_bytes,
    )
    .unwrap();

    RelayEnvelope {
        message_type: "relay_envelope".into(),
        id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e4001".into(),
        timestamp: 1774257600300,
        from: RELAY_PEER_DAEMON.into(),
        to: to_peer_id.into(),
        channel_id: session_keys.channel_id.clone(),
        seq,
        ciphertext,
    }
}

fn mutate_channel_id(channel_id: &str) -> String {
    let mut chars: Vec<char> = channel_id.chars().collect();
    let last = chars.last_mut().unwrap();
    *last = if *last == 'A' { 'B' } else { 'A' };
    chars.into_iter().collect()
}

fn tamper_base64url(data: &str) -> String {
    let mut chars: Vec<char> = data.chars().collect();
    let first = chars.first_mut().unwrap();
    *first = if *first == 'A' { 'B' } else { 'A' };
    chars.into_iter().collect()
}

#[tokio::test]
async fn app_client_rejects_unknown_channel_envelope() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        send_json(&mut ws, &relay_ready_for_app()).await;

        let hello = recv_hello(&mut ws).await;
        let (accept, session_keys) = build_signed_accept_and_session_keys(
            &hello,
            "relay-integration-unknown-channel-daemon-ephemeral-v1",
        );
        send_json(&mut ws, &accept).await;

        let mut envelope = build_daemon_envelope(
            &session_keys,
            &hello.from,
            1,
            json!({"type": "ack", "payload": {"text": "ok"}}),
        );
        envelope.channel_id = mutate_channel_id(&envelope.channel_id);
        send_json(&mut ws, &envelope).await;
    });

    let mut client = RelayClient::connect(app_client_config(addr)).await.unwrap();
    let outcome = client.initiate_hello_to_daemon().await.unwrap();
    assert!(outcome.session_keys.is_some());

    let error = client.wait_for_next_decrypted_envelope().await.unwrap_err();
    assert!(error.message.contains("UNKNOWN_CHANNEL"));

    server.await.unwrap();
}

#[tokio::test]
async fn app_client_rejects_tampered_ciphertext_with_aad_auth_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        send_json(&mut ws, &relay_ready_for_app()).await;

        let hello = recv_hello(&mut ws).await;
        let (accept, session_keys) = build_signed_accept_and_session_keys(
            &hello,
            "relay-integration-auth-failed-daemon-ephemeral-v1",
        );
        send_json(&mut ws, &accept).await;

        let mut envelope = build_daemon_envelope(
            &session_keys,
            &hello.from,
            1,
            json!({"type": "ack", "payload": {"text": "ok"}}),
        );
        envelope.ciphertext = tamper_base64url(&envelope.ciphertext);
        send_json(&mut ws, &envelope).await;
    });

    let mut client = RelayClient::connect(app_client_config(addr)).await.unwrap();
    let outcome = client.initiate_hello_to_daemon().await.unwrap();
    assert!(outcome.session_keys.is_some());

    let error = client.wait_for_next_decrypted_envelope().await.unwrap_err();
    assert!(error.message.contains("AAD_AUTH_FAILED"));

    server.await.unwrap();
}

#[tokio::test]
async fn app_client_rejects_replayed_envelope_with_seq_replay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        send_json(&mut ws, &relay_ready_for_app()).await;

        let hello = recv_hello(&mut ws).await;
        let (accept, session_keys) = build_signed_accept_and_session_keys(
            &hello,
            "relay-integration-replay-daemon-ephemeral-v1",
        );
        send_json(&mut ws, &accept).await;

        let envelope = build_daemon_envelope(
            &session_keys,
            &hello.from,
            1,
            json!({"type": "ack", "payload": {"text": "ok"}}),
        );
        send_json(&mut ws, &envelope).await;
        send_json(&mut ws, &envelope).await;
    });

    let mut client = RelayClient::connect(app_client_config(addr)).await.unwrap();
    let outcome = client.initiate_hello_to_daemon().await.unwrap();
    assert!(outcome.session_keys.is_some());

    let first = client.wait_for_next_decrypted_envelope().await.unwrap();
    assert_eq!(
        first.plaintext_json,
        json!({"type": "ack", "payload": {"text": "ok"}})
    );

    let error = client.wait_for_next_decrypted_envelope().await.unwrap_err();
    assert!(error.message.contains("SEQ_REPLAY"));

    server.await.unwrap();
}
