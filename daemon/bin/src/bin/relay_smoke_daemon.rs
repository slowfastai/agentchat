use std::env;

use agentchat_core::relay_client::{
    RelayClient, RelayClientConfig, RelayClientCryptoConfig, RelayClientFrame,
};
use agentchat_protocol::relay_crypto::{ed25519_public_key, seed_from_label};
use serde_json::json;
use tracing::{error, info, warn};

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
        identity_seed: seed_from_label(DEV_DAEMON_IDENTITY_LABEL),
        expected_remote_identity_public_key: ed25519_public_key(&seed_from_label(
            DEV_APP_IDENTITY_LABEL,
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

    let mut replay_seen = false;
    let mut reply_sent = false;

    loop {
        match client.next_frame().await {
            Ok(RelayClientFrame::SecureChannelHello(hello)) => {
                match client.send_accept_for_hello(&hello).await {
                    Ok(outcome) => {
                        info!(
                            from = %outcome.hello.from,
                            hello_id = %outcome.hello.id,
                            accept_id = %outcome.accept.id,
                            channel_id = %outcome.channel_id,
                            has_session_keys = outcome.session_keys.is_some(),
                            "accepted secure_channel_hello"
                        );
                    }
                    Err(error) => {
                        error!("failed to accept secure_channel_hello: {error}");
                        std::process::exit(1);
                    }
                }
            }
            Ok(RelayClientFrame::RelayEnvelope(envelope)) => {
                match client.decrypt_envelope_json(&envelope) {
                    Ok(plaintext_json) => {
                        info!(
                            from = %envelope.from,
                            to = %envelope.to,
                            channel_id = %envelope.channel_id,
                            seq = envelope.seq,
                            plaintext_json = %plaintext_json,
                            "decrypted relay_envelope"
                        );

                        if !reply_sent {
                            let reply = match client
                                .send_encrypted_json(&json!({
                                    "type": "ack",
                                    "request_id": "req_001",
                                    "payload": {
                                        "text": "daemon received hello relay"
                                    }
                                }))
                                .await
                            {
                                Ok(reply) => reply,
                                Err(error) => {
                                    error!(
                                        "failed to send encrypted relay_envelope reply: {error}"
                                    );
                                    std::process::exit(1);
                                }
                            };

                            info!(
                                envelope_id = %reply.id,
                                seq = reply.seq,
                                channel_id = %reply.channel_id,
                                "sent encrypted relay_envelope"
                            );
                            reply_sent = true;
                        }
                    }
                    Err(error) if error.message.contains("SEQ_REPLAY") => {
                        warn!(
                            from = %envelope.from,
                            to = %envelope.to,
                            channel_id = %envelope.channel_id,
                            seq = envelope.seq,
                            error = %error,
                            "rejected replayed relay_envelope"
                        );
                        replay_seen = true;
                        if reply_sent {
                            return;
                        }
                    }
                    Err(error) => {
                        error!("failed to decrypt relay_envelope: {error}");
                        std::process::exit(1);
                    }
                }
            }
            Ok(RelayClientFrame::RelayError(error_frame)) => {
                warn!(
                    code = %error_frame.code,
                    message = %error_frame.message,
                    ref_id = ?error_frame.ref_id,
                    "received relay_error"
                );
            }
            Ok(RelayClientFrame::SecureChannelAccept(frame)) => {
                warn!(
                    from = %frame.from,
                    to = %frame.to,
                    hello_id = %frame.hello_id,
                    "received unexpected secure_channel_accept on daemon client"
                );
            }
            Ok(RelayClientFrame::RelayReady(_)) => {
                warn!("received unexpected relay_ready after initial handshake");
            }
            Err(error) => {
                if replay_seen {
                    return;
                }
                error!("relay client stopped: {error}");
                std::process::exit(1);
            }
        }
    }
}
