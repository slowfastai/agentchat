use std::{env, time::Duration};

use agentchat_core::relay_client::{RelayClient, RelayClientConfig, RelayClientCryptoConfig};
use agentchat_protocol::relay_crypto::{ed25519_public_key, seed_from_label};
use serde_json::json;
use tracing::{error, info};

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

fn required_env(key: &str) -> Result<String, String> {
    env::var(key)
        .map_err(|_| format!("missing required environment variable {key}"))
        .and_then(|value| {
            if value.trim().is_empty() {
                Err(format!("environment variable {key} must not be empty"))
            } else {
                Ok(value)
            }
        })
}

fn dev_crypto_config() -> RelayClientCryptoConfig {
    RelayClientCryptoConfig {
        identity_seed: seed_from_label(DEV_APP_IDENTITY_LABEL),
        expected_remote_identity_public_key: ed25519_public_key(&seed_from_label(
            DEV_DAEMON_IDENTITY_LABEL,
        )),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt::init();

    let ws_url = match required_env("AGENTCHAT_RELAY_WS_URL") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let relay_token = match required_env("AGENTCHAT_RELAY_TOKEN") {
        Ok(value) => value,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    let mut config = RelayClientConfig::new(ws_url, relay_token);
    config.crypto = Some(dev_crypto_config());

    let mut client = match RelayClient::connect(config).await {
        Ok(client) => client,
        Err(error) => {
            error!("failed to connect to relay: {error}");
            std::process::exit(1);
        }
    };

    info!(
        peer_id = %client.ready().peer_id,
        connection_id = %client.ready().connection_id,
        device_id = %client.ready().device_id,
        "relay_ready received"
    );

    let outcome = match client.initiate_hello_to_daemon().await {
        Ok(outcome) => outcome,
        Err(error) => {
            error!("failed to complete relay handshake: {error}");
            std::process::exit(1);
        }
    };

    info!(
        hello_id = %outcome.hello.id,
        accept_id = %outcome.accept.id,
        daemon_connection_id = %outcome.accept.connection_id,
        channel_id = %outcome.channel_id,
        has_session_keys = outcome.session_keys.is_some(),
        "secure channel established"
    );

    let outbound = match client
        .send_encrypted_json(&json!({
            "type": "send_prompt",
            "request_id": "req_001",
            "payload": {
                "text": "hello relay"
            }
        }))
        .await
    {
        Ok(envelope) => envelope,
        Err(error) => {
            error!("failed to send encrypted relay_envelope: {error}");
            std::process::exit(1);
        }
    };

    info!(
        envelope_id = %outbound.id,
        seq = outbound.seq,
        channel_id = %outbound.channel_id,
        "sent encrypted relay_envelope"
    );

    let reply = match client.wait_for_next_decrypted_envelope().await {
        Ok(reply) => reply,
        Err(error) => {
            error!("failed to receive decrypted relay_envelope: {error}");
            std::process::exit(1);
        }
    };

    info!(
        envelope_id = %reply.envelope.id,
        seq = reply.envelope.seq,
        channel_id = %reply.envelope.channel_id,
        plaintext_json = %reply.plaintext_json,
        "decrypted relay_envelope"
    );

    if let Err(error) = client.send_json(&outbound).await {
        error!("failed to replay encrypted relay_envelope: {error}");
        std::process::exit(1);
    }

    info!(
        envelope_id = %outbound.id,
        seq = outbound.seq,
        channel_id = %outbound.channel_id,
        "replayed relay_envelope"
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
}
