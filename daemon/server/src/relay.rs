use std::cell::RefCell;
use std::rc::Rc;

use tokio::sync::watch;
use tracing::{error, info, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::relay_client::{RelayClient, RelayClientConfig, RelayClientFrame};
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{ClientMessage, ResponseEvent};

use crate::app::AppProtocolSession;

pub struct RelayTransportServer {
    client_config: RelayClientConfig,
}

impl RelayTransportServer {
    pub fn new(client_config: RelayClientConfig) -> Self {
        Self { client_config }
    }

    pub async fn connect_client(&self) -> Result<RelayClient, String> {
        let relay_client = RelayClient::connect(self.client_config.clone())
            .await
            .map_err(|err| format!("failed to connect relay transport: {err}"))?;

        info!(
            peer_id = %relay_client.ready().peer_id,
            connection_id = %relay_client.ready().connection_id,
            "relay transport connected; waiting for secure channel"
        );

        Ok(relay_client)
    }

    pub async fn run_with_client(
        self,
        mut relay_client: RelayClient,
        manager: Rc<RefCell<AgentManager>>,
        mut shutdown_rx: watch::Receiver<bool>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) -> Result<(), String> {
        let handshake = tokio::select! {
            result = relay_client.accept_next_hello() => result,
            _ = shutdown_rx.changed() => {
                info!("relay transport shutting down before secure channel activation");
                return Ok(());
            }
        }
        .map_err(|err| format!("failed to activate relay secure channel: {err}"))?;

        info!(
            channel_id = %handshake.channel_id,
            has_session_keys = handshake.session_keys.is_some(),
            "relay secure channel active"
        );

        let mut session =
            match AppProtocolSession::new(manager, session_store, skill_store, distiller) {
                Ok(session) => session,
                Err(event) => {
                    send_startup_error_over_relay(&mut relay_client, &event).await;
                    return Err(format!(
                    "failed to initialize daemon application session for relay transport: {event:?}"
                ));
                }
            };
        let mut response_rx = session.subscribe_events();

        loop {
            tokio::select! {
                result = response_rx.recv() => {
                    let event = match result {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            warn!("relay event subscriber lagged and skipped {skipped} events");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };

                    if let Err(err) = relay_client.send_encrypted_json(&event).await {
                        session.shutdown().await;
                        return Err(format!("failed to send encrypted relay response: {err}"));
                    }
                }
                frame = relay_client.next_frame() => {
                    let frame = match frame {
                        Ok(frame) => frame,
                        Err(err) => {
                            session.shutdown().await;
                            return Err(format!("relay transport frame error: {err}"));
                        }
                    };

                    match frame {
                        RelayClientFrame::RelayEnvelope(envelope) => {
                            let payload = match relay_client.decrypt_envelope_json(&envelope) {
                                Ok(payload) => payload,
                                Err(err) => {
                                    warn!("failed to decrypt relay envelope: {err}");
                                    continue;
                                }
                            };

                            let client_message: ClientMessage = match serde_json::from_value(payload) {
                                Ok(message) => message,
                                Err(err) => {
                                    warn!("failed to decode decrypted relay payload as ClientMessage: {err}");
                                    continue;
                                }
                            };

                            session.handle_client_message(client_message).await;
                        }
                        RelayClientFrame::RelayError(error_frame) => {
                            warn!(
                                code = %error_frame.code,
                                message = %error_frame.message,
                                ref_id = ?error_frame.ref_id,
                                "received relay_error while relay transport was active"
                            );
                        }
                        RelayClientFrame::SecureChannelHello(hello) => {
                            warn!(
                                hello_id = %hello.id,
                                from = %hello.from,
                                "received unexpected secure_channel_hello while a relay channel was already active"
                            );
                        }
                        RelayClientFrame::SecureChannelAccept(frame) => {
                            warn!(
                                accept_id = %frame.id,
                                hello_id = %frame.hello_id,
                                "received unexpected secure_channel_accept on daemon relay transport"
                            );
                        }
                        RelayClientFrame::RelayReady(_) => {
                            warn!("received unexpected relay_ready while relay transport was already active");
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("relay transport shutting down");
                    break;
                }
            }
        }

        session.shutdown().await;
        Ok(())
    }

    pub async fn run(
        self,
        manager: Rc<RefCell<AgentManager>>,
        shutdown_rx: watch::Receiver<bool>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) -> Result<(), String> {
        let relay_client = self.connect_client().await?;
        self.run_with_client(
            relay_client,
            manager,
            shutdown_rx,
            session_store,
            skill_store,
            distiller,
        )
        .await
    }
}

async fn send_startup_error_over_relay(relay_client: &mut RelayClient, event: &ResponseEvent) {
    if !relay_client.has_active_channel() {
        return;
    }

    if let Err(err) = relay_client.send_encrypted_json(event).await {
        error!("failed to send encrypted relay startup error: {err}");
    }
}
