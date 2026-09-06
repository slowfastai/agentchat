//! WebSocket server that bridges the iOS app and ACP agents.
//!
//! Bridges multiple client connections to the shared ACP session and
//! prompt/update flows.

use std::cell::RefCell;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::{interval, Duration};
use tokio_tungstenite::accept_async_with_config;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{ClientMessage, DaemonLifecycleState, DaemonStopReason, ResponseEvent};

use crate::app::{serialize_event, AppProtocolSession};

/// WebSocket server that bridges the iOS app and ACP agents.
pub struct WebSocketServer {
    port: u16,
    bind_host: IpAddr,
    ready_file: Option<PathBuf>,
}

impl WebSocketServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            bind_host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            ready_file: None,
        }
    }

    /// Restrict this server to the local machine. The default remains
    /// unchanged for the standalone/mobile daemon.
    pub fn loopback_only(mut self) -> Self {
        self.bind_host = IpAddr::V4(Ipv4Addr::LOCALHOST);
        self
    }

    /// Publish the actual bound address after the listener is ready. This is
    /// used by app-managed daemons started with port 0.
    pub fn with_ready_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.ready_file = Some(path.into());
        self
    }

    /// Start listening for WebSocket connections.
    pub async fn run(
        self,
        manager: Rc<RefCell<AgentManager>>,
        mut shutdown_rx: watch::Receiver<Option<DaemonStopReason>>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) -> Result<(), String> {
        let requested_addr = SocketAddr::new(self.bind_host, self.port);
        let listener = TcpListener::bind(requested_addr)
            .await
            .map_err(|e| format!("failed to bind {requested_addr}: {e}"))?;
        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("failed to read bound WebSocket address: {e}"))?;
        info!("WebSocket server listening on {}", actual_addr);

        let mut app_session =
            AppProtocolSession::new(manager, session_store, skill_store, distiller)
                .map_err(|event| format!("failed to initialize app protocol session: {event:?}"))?;

        let ready_file = self.ready_file.clone();
        if let Some(path) = ready_file.as_deref() {
            write_ready_file(path, actual_addr)?;
        }

        let event_tx = app_session.event_sender();
        let (client_tx, mut client_rx) = mpsc::unbounded_channel::<ClientMessage>();
        let app_task = tokio::task::spawn_local(async move {
            while let Some(client_msg) = client_rx.recv().await {
                app_session.handle_client_message(client_msg).await;
            }

            app_session.shutdown().await;
        });

        let mut connection_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();

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

            let ws = match accept_async_with_config(stream, None).await {
                Ok(ws) => ws,
                Err(err) => {
                    error!("websocket handshake failed: {err}");
                    continue;
                }
            };

            connection_tasks.retain(|handle: &tokio::task::JoinHandle<()>| !handle.is_finished());
            let client_tx = client_tx.clone();
            let event_tx = event_tx.clone();
            let shutdown_rx = shutdown_rx.clone();
            connection_tasks.push(tokio::task::spawn_local(async move {
                Self::handle_connection(ws, client_tx, event_tx, shutdown_rx).await;
            }));
        }

        for handle in connection_tasks {
            if let Err(err) = handle.await {
                error!("connection task panicked during shutdown: {err}");
            }
        }

        drop(client_tx);
        let app_result = app_task
            .await
            .map_err(|err| format!("app protocol session task panicked: {err}"));

        if let Some(path) = ready_file {
            let _ = fs::remove_file(path);
        }

        app_result?;

        Ok(())
    }

    async fn handle_connection(
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        client_tx: mpsc::UnboundedSender<ClientMessage>,
        event_tx: broadcast::Sender<ResponseEvent>,
        mut shutdown_rx: watch::Receiver<Option<DaemonStopReason>>,
    ) {
        let (mut ws_tx, mut ws_rx) = ws.split();
        let mut response_rx = event_tx.subscribe();
        let ping_interval = Duration::from_secs(10);
        let mut ping_ticker = interval(ping_interval);

        loop {
            tokio::select! {
                msg = response_rx.recv() => {
                    let event = match msg {
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
                _ = ping_ticker.tick() => {
                    if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
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
                    if let Some(reason) = shutdown_rx.borrow().clone() {
                        send_shutdown_notice(&mut ws_tx, reason).await;
                    }
                    break;
                }
            }
        }

        info!("client disconnected");
    }
}

fn write_ready_file(path: &Path, addr: SocketAddr) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create ready file directory: {err}"))?;

    let payload = serde_json::json!({
        "pid": std::process::id(),
        "websocket_url": format!("ws://127.0.0.1:{}", addr.port()),
    });
    let json = serde_json::to_vec(&payload)
        .map_err(|err| format!("failed to serialize ready file: {err}"))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("agentchat.ready.json");
    let temporary_path = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));

    fs::write(&temporary_path, json)
        .map_err(|err| format!("failed to write temporary ready file: {err}"))?;
    if let Err(err) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("failed to publish ready file: {err}"));
    }
    Ok(())
}

async fn send_shutdown_notice(
    ws_tx: &mut futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        Message,
    >,
    reason: DaemonStopReason,
) {
    let event = ResponseEvent::DaemonStatus {
        state: DaemonLifecycleState::Stopping,
        reason: Some(reason),
        message: Some("Daemon is stopping.".into()),
    };

    if let Some(json) = serialize_event(&event) {
        let _ = ws_tx.send(Message::Text(json.into())).await;
    }

    let _ = ws_tx.send(Message::Close(None)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_file_is_a_complete_atomic_json_payload() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("daemon-ready.json");
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49321);

        write_ready_file(&path, address).expect("ready file should be written");

        let data = fs::read(&path).expect("ready file should exist");
        let value: serde_json::Value = serde_json::from_slice(&data).expect("valid JSON");
        assert_eq!(value["pid"].as_u64(), Some(u64::from(std::process::id())));
        assert_eq!(
            value["websocket_url"].as_str(),
            Some("ws://127.0.0.1:49321")
        );
    }
}
