//! WebSocket server that bridges the iOS app and ACP agents.
//!
//! M0: single client connection, single agent. Translates between
//! the iOS WebSocket protocol and ACP session/prompt/update flows.

use std::cell::RefCell;
use std::rc::Rc;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
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

        let mut app_session =
            AppProtocolSession::new(manager, session_store, skill_store, distiller)
                .map_err(|event| format!("failed to initialize app protocol session: {event:?}"))?;
        let event_tx = app_session.event_sender();
        let (client_tx, mut client_rx) = mpsc::unbounded_channel::<ClientMessage>();
        let app_task = tokio::task::spawn_local(async move {
            while let Some(client_msg) = client_rx.recv().await {
                app_session.handle_client_message(client_msg).await;
            }

            app_session.shutdown().await;
        });

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

            self.handle_connection(ws, client_tx.clone(), event_tx.clone(), shutdown_rx.clone())
                .await;
        }

        drop(client_tx);
        app_task
            .await
            .map_err(|err| format!("app protocol session task panicked: {err}"))?;

        Ok(())
    }

    async fn handle_connection(
        &self,
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        client_tx: mpsc::UnboundedSender<ClientMessage>,
        event_tx: broadcast::Sender<agentchat_protocol::ResponseEvent>,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        let (mut ws_tx, mut ws_rx) = ws.split();
        let mut response_rx = event_tx.subscribe();

        let writer_task = tokio::task::spawn_local(async move {
            loop {
                let event = match response_rx.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("websocket event subscriber lagged and skipped {skipped} events");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                };

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

                    if client_tx.send(client_msg).is_err() {
                        warn!("app protocol session is unavailable");
                        break;
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("closing client connection for shutdown");
                    break;
                }
            }
        }

        writer_task.abort();
        let _ = writer_task.await;
        info!("client disconnected");
    }
}
