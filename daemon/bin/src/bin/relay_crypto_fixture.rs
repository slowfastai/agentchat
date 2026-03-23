use agentchat_protocol::relay::{
    SecureChannelAccept, SecureChannelHello, RELAY_PEER_DAEMON, RELAY_WIRE_CRYPTO_SUITE,
    RELAY_WIRE_PROTOCOL_VERSION,
};
use agentchat_protocol::relay_crypto::{
    derive_session_keys, ed25519_public_key_base64url, encode_base64url, encrypt_relay_plaintext,
    relay_envelope_aad_canonical_json, seed_from_label, x25519_public_key_base64url,
};
use serde_json::json;

fn main() {
    let app_identity_seed = seed_from_label("relay-fixture-app-identity-v1");
    let daemon_identity_seed = seed_from_label("relay-fixture-daemon-identity-v1");
    let app_ephemeral_secret = seed_from_label("relay-fixture-app-ephemeral-v1");
    let daemon_ephemeral_secret = seed_from_label("relay-fixture-daemon-ephemeral-v1");

    let mut hello = SecureChannelHello {
        message_type: "secure_channel_hello".into(),
        id: "018f6f88-5f8b-7c98-9d7d-7b10ca2e2001".into(),
        timestamp: 1774257600100,
        protocol_version: RELAY_WIRE_PROTOCOL_VERSION.into(),
        from: "app:7d44a5b8-b448-4c7a-9c4d-b1d496f0c3af".into(),
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

    let session_keys = derive_session_keys(
        &app_ephemeral_secret,
        &accept.ephemeral_public_key_bytes().unwrap(),
        &hello,
        &accept,
    )
    .unwrap();

    let plaintext_json = serde_json::to_string(&json!({
        "type": "send_prompt",
        "request_id": "req_001",
        "payload": {
            "text": "hello relay"
        }
    }))
    .unwrap();

    let aad_canonical_json =
        relay_envelope_aad_canonical_json(&hello.from, &hello.to, &session_keys.channel_id, 1)
            .unwrap();
    let ciphertext_app_to_daemon_seq_1 = encrypt_relay_plaintext(
        &session_keys.key_app_to_daemon,
        &hello.from,
        &hello.to,
        &session_keys.channel_id,
        1,
        plaintext_json.as_bytes(),
    )
    .unwrap();

    let fixture = json!({
        "app_identity_seed": encode_base64url(&app_identity_seed),
        "app_identity_public_key": ed25519_public_key_base64url(&app_identity_seed),
        "daemon_identity_seed": encode_base64url(&daemon_identity_seed),
        "daemon_identity_public_key": ed25519_public_key_base64url(&daemon_identity_seed),
        "app_ephemeral_secret": encode_base64url(&app_ephemeral_secret),
        "app_ephemeral_public_key": hello.ephemeral_public_key,
        "daemon_ephemeral_secret": encode_base64url(&daemon_ephemeral_secret),
        "daemon_ephemeral_public_key": accept.ephemeral_public_key,
        "hello": hello,
        "accept": accept,
        "transcript_hash": session_keys.transcript_hash_base64url(),
        "shared_secret": session_keys.shared_secret_base64url(),
        "channel_id": session_keys.channel_id,
        "key_app_to_daemon": session_keys.key_app_to_daemon_base64url(),
        "key_daemon_to_app": session_keys.key_daemon_to_app_base64url(),
        "aad_canonical_json": aad_canonical_json,
        "plaintext_json": plaintext_json,
        "ciphertext_app_to_daemon_seq_1": ciphertext_app_to_daemon_seq_1,
    });

    println!("{}", serde_json::to_string_pretty(&fixture).unwrap());
}
