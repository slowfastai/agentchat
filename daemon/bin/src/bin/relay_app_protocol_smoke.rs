use std::env;
use std::time::Duration;

use agentchat_core::relay_client::{RelayClient, RelayClientConfig, RelayClientCryptoConfig};
use agentchat_protocol::relay_crypto::{ed25519_public_key, seed_from_label};
use agentchat_protocol::{ClientMessage, DeltaType, ResponseEvent};
use tokio::time::timeout;
use tracing::{error, info};

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";
const EVENT_TIMEOUT: Duration = Duration::from_secs(15);

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

async fn receive_response_event(client: &mut RelayClient) -> Result<ResponseEvent, String> {
    let decrypted = timeout(EVENT_TIMEOUT, client.wait_for_next_decrypted_envelope())
        .await
        .map_err(|_| "timed out waiting for encrypted relay response".to_string())?
        .map_err(|err| err.to_string())?;

    serde_json::from_value(decrypted.plaintext_json)
        .map_err(|err| format!("failed to decode relay payload as ResponseEvent: {err}"))
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

    let handshake = match client.initiate_hello_to_daemon().await {
        Ok(outcome) => outcome,
        Err(error) => {
            error!("failed to complete relay handshake: {error}");
            std::process::exit(1);
        }
    };

    info!(
        channel_id = %handshake.channel_id,
        has_session_keys = handshake.session_keys.is_some(),
        "relay app protocol client connected"
    );

    if let Err(error) = client
        .send_encrypted_json(&ClientMessage::CreateSession {
            working_dir: ".".into(),
        })
        .await
    {
        error!("failed to send create_session over relay: {error}");
        std::process::exit(1);
    }

    let session_id = match receive_response_event(&mut client).await {
        Ok(ResponseEvent::SessionCreated { session_id, .. }) => session_id,
        Ok(other) => {
            error!("expected session_created response, got {other:?}");
            std::process::exit(1);
        }
        Err(error) => {
            error!("failed to receive session_created over relay: {error}");
            std::process::exit(1);
        }
    };

    info!(session_id = %session_id, "relay create_session completed");

    if let Err(error) = client
        .send_encrypted_json(&ClientMessage::Prompt {
            session_id: session_id.clone(),
            content: "say hello".into(),
        })
        .await
    {
        error!("failed to send prompt over relay: {error}");
        std::process::exit(1);
    }

    let mut saw_text_delta = false;
    let mut saw_thinking_delta = false;
    let mut saw_tool_update = false;
    let mut saw_turn_end = false;

    for _ in 0..10 {
        let event = match receive_response_event(&mut client).await {
            Ok(event) => event,
            Err(error) => {
                error!("failed to receive prompt response event over relay: {error}");
                std::process::exit(1);
            }
        };

        match &event {
            ResponseEvent::Delta {
                session_id: sid,
                content,
                delta_type,
                ..
            } if sid == &session_id
                && *delta_type == DeltaType::Text
                && content == "echo: say hello" =>
            {
                saw_text_delta = true;
            }
            ResponseEvent::Delta {
                session_id: sid,
                content,
                delta_type,
                ..
            } if sid == &session_id
                && *delta_type == DeltaType::Thinking
                && content == "thinking about the request" =>
            {
                saw_thinking_delta = true;
            }
            ResponseEvent::ToolUpdate {
                session_id: sid,
                tool_call_id,
                title,
                status,
                ..
            } if sid == &session_id
                && tool_call_id == "tool-1"
                && title == "Demo Tool"
                && status == "InProgress" =>
            {
                saw_tool_update = true;
            }
            ResponseEvent::TurnEnd {
                session_id: sid,
                stop_reason,
                ..
            } if sid == &session_id => {
                info!(session_id = %sid, stop_reason = %stop_reason, "relay prompt completed");
                saw_turn_end = true;
                break;
            }
            _ => {
                info!("ignoring unrelated relay response event: {event:?}");
            }
        }
    }

    if !(saw_text_delta && saw_thinking_delta && saw_tool_update && saw_turn_end) {
        error!(
            saw_text_delta,
            saw_thinking_delta,
            saw_tool_update,
            saw_turn_end,
            "relay application protocol flow did not produce the expected response sequence"
        );
        std::process::exit(1);
    }

    info!(session_id = %session_id, "relay application protocol flow succeeded");
}
