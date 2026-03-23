//! WebSocket server that bridges the iOS app and ACP agents.
//!
//! M0: single client connection, single agent. Translates between
//! the iOS WebSocket protocol and ACP session/prompt/update flows.

use std::cell::RefCell;
use std::rc::Rc;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::ClientMessage;

use crate::app::{serialize_event, AppProtocolSession};

/// WebSocket server that bridges the iOS app and ACP agents.
pub struct WebSocketServer {
    port: u16,
}

impl WebSocketServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Start listening for WebSocket connections.
    pub async fn run(
        self,
        manager: Rc<RefCell<AgentManager>>,
        mut shutdown_rx: watch::Receiver<bool>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("failed to bind {addr}: {e}"))?;
        info!("WebSocket server listening on {}", addr);

        loop {
            let accepted = tokio::select! {
                accepted = listener.accept() => accepted,
                _ = shutdown_rx.changed() => {
                    info!("websocket server shutting down");
                    break;
                }
            };

            let (stream, peer) = match accepted {
                Ok(value) => value,
                Err(err) => {
                    error!("accept error: {err}");
                    continue;
                }
            };

            info!("new connection from {}", peer);

            let ws = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(err) => {
                    error!("websocket handshake failed: {err}");
                    continue;
                }
            };

            self.handle_connection(
                ws,
                manager.clone(),
                shutdown_rx.clone(),
                session_store.clone(),
                skill_store.clone(),
                distiller.clone(),
            )
            .await;
        }

        Ok(())
    }

    async fn handle_connection(
        &self,
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        manager: Rc<RefCell<AgentManager>>,
        mut shutdown_rx: watch::Receiver<bool>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) {
        let (mut ws_tx, mut ws_rx) = ws.split();

        let mut session =
            match AppProtocolSession::new(manager, session_store, skill_store, distiller) {
                Ok(session) => session,
                Err(event) => {
                    if let Some(message) = serialize_event(&event) {
                        let _ = ws_tx.send(Message::Text(message.into())).await;
                    }
                    return;
                }
            };

        let mut response_rx = session
            .take_response_rx()
            .expect("response channel must exist when protocol session is created");

        tokio::task::spawn_local(async move {
            while let Some(event) = response_rx.recv().await {
                let Some(json) = serialize_event(&event) else {
                    continue;
                };

                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        });

        loop {
            tokio::select! {
                msg = ws_rx.next() => {
                    let Some(msg) = msg else {
                        break;
                    };

                    let text = match msg {
                        Ok(Message::Text(text)) => text,
                        Ok(Message::Close(_)) => break,
                        Ok(_) => continue,
                        Err(err) => {
                            warn!("ws read error: {err}");
                            break;
                        }
                    };

                    let client_msg: ClientMessage = match serde_json::from_str(&text) {
                        Ok(message) => message,
                        Err(err) => {
                            warn!("invalid message: {err}");
                            continue;
                        }
                    };

                    session.handle_client_message(client_msg).await;
                }
                _ = shutdown_rx.changed() => {
                    info!("closing client connection for shutdown");
                    break;
                }
            }
        }

        session.shutdown().await;
        info!("client disconnected");
    }
}
