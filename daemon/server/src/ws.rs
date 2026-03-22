//! WebSocket server that bridges the iOS app and ACP agents.
//!
//! M0: single client connection, single agent. Translates between
//! the iOS WebSocket protocol and ACP session/prompt/update flows.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::{ContentBlock, SessionId, SessionNotification, SessionUpdate};
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_protocol::{ClientMessage, DeltaType, ResponseEvent};

/// WebSocket server that bridges the iOS app and ACP agents.
pub struct WebSocketServer {
    port: u16,
}

impl WebSocketServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Start listening for WebSocket connections.
    pub async fn run(self, manager: Rc<RefCell<AgentManager>>) {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await.expect("failed to bind");
        info!("WebSocket server listening on {}", addr);

        // M0: accept one connection at a time
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    error!("accept error: {e}");
                    continue;
                }
            };

            info!("new connection from {}", peer);

            let ws = match accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => {
                    error!("websocket handshake failed: {e}");
                    continue;
                }
            };

            self.handle_connection(ws, manager.clone()).await;
        }
    }

    // Single-threaded runtime (current_thread + LocalSet) — RefCell borrows
    // across await points are safe because no concurrent task can contend.
    #[allow(clippy::await_holding_refcell_ref)]
    async fn handle_connection(
        &self,
        ws: tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
        manager: Rc<RefCell<AgentManager>>,
    ) {
        let (mut ws_tx, mut ws_rx) = ws.split();

        // Take the update receiver from the first available agent
        let update_rx = {
            let mut mgr = manager.borrow_mut();
            let agent_id = mgr.first_agent_id().map(|id| id.to_string());
            agent_id
                .as_deref()
                .and_then(|id| mgr.get_agent_mut(id))
                .map(|a| a.take_update_rx())
        };

        let Some(mut update_rx) = update_rx else {
            error!("no agent available");
            let msg = serde_json::to_string(&ResponseEvent::Error {
                session_id: None,
                code: "no_agent".into(),
                message: "no agent configured".into(),
            })
            .unwrap();
            let _ = ws_tx.send(Message::Text(msg.into())).await;
            return;
        };

        // Channel so the incoming-message handler can send responses back to the WS
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<ResponseEvent>();

        // Task 1: Forward ACP session updates → WebSocket
        let resp_tx_updates = resp_tx.clone();
        tokio::task::spawn_local(async move {
            while let Some(notification) = update_rx.recv().await {
                let event = map_session_update(&notification);
                if resp_tx_updates.send(event).is_err() {
                    break;
                }
            }
        });

        // Task 2: Forward response events → WebSocket frames
        tokio::task::spawn_local(async move {
            while let Some(event) = resp_rx.recv().await {
                let json = serde_json::to_string(&event).unwrap();
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        });

        // Task 3: Process incoming WebSocket messages from iOS
        while let Some(msg) = ws_rx.next().await {
            let text = match msg {
                Ok(Message::Text(text)) => text,
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(e) => {
                    warn!("ws read error: {e}");
                    break;
                }
            };

            let client_msg: ClientMessage = match serde_json::from_str(&text) {
                Ok(m) => m,
                Err(e) => {
                    warn!("invalid message: {e}");
                    continue;
                }
            };

            match client_msg {
                ClientMessage::CreateSession { working_dir } => {
                    let cwd = PathBuf::from(&working_dir);
                    let result = {
                        let mgr = manager.borrow();
                        let agent_id = mgr.first_agent_id().map(|id| id.to_string());
                        match agent_id {
                            Some(agent_id) => match mgr.get_agent(&agent_id) {
                                Some(agent) => Some((agent_id, agent.new_session(cwd).await)),
                                None => None,
                            },
                            None => None,
                        }
                    };
                    if let Some((agent_id, Ok(resp))) = result {
                        let sid = resp.session_id.to_string();
                        manager
                            .borrow_mut()
                            .register_session(sid.clone(), agent_id);
                        let _ = resp_tx.send(ResponseEvent::SessionCreated { session_id: sid });
                    } else if let Some((_, Err(e))) = result {
                        let _ = resp_tx.send(ResponseEvent::Error {
                            session_id: None,
                            code: "create_session_failed".into(),
                            message: e.to_string(),
                        });
                    }
                }
                ClientMessage::Prompt {
                    session_id,
                    content,
                } => {
                    let result = {
                        let mgr = manager.borrow();
                        let agent_id = mgr.agent_for_session(&session_id).map(|s| s.to_string());
                        match agent_id.as_deref().and_then(|aid| mgr.get_agent(aid)) {
                            Some(agent) => {
                                let sid = SessionId::new(session_id.clone());
                                Some(agent.prompt(sid, content).await)
                            }
                            None => None,
                        }
                    };
                    match result {
                        Some(Ok(resp)) => {
                            let _ = resp_tx.send(ResponseEvent::TurnEnd {
                                session_id: session_id.clone(),
                                stop_reason: format!("{:?}", resp.stop_reason),
                            });
                        }
                        Some(Err(e)) => {
                            let _ = resp_tx.send(ResponseEvent::Error {
                                session_id: Some(session_id),
                                code: "prompt_failed".into(),
                                message: e.to_string(),
                            });
                        }
                        None => {
                            let _ = resp_tx.send(ResponseEvent::Error {
                                session_id: Some(session_id),
                                code: "no_agent".into(),
                                message: "no agent for this session".into(),
                            });
                        }
                    }
                }
                ClientMessage::Cancel { session_id } => {
                    let result = {
                        let mgr = manager.borrow();
                        mgr.cancel_session(&session_id).await
                    };
                    if let Err(e) = result {
                        warn!("cancel failed: {e}");
                    }
                }
            }
        }

        info!("client disconnected");
    }
}

/// Map an ACP SessionNotification to our WebSocket ResponseEvent.
fn map_session_update(notification: &SessionNotification) -> ResponseEvent {
    let sid = notification.session_id.to_string();

    match &notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            let text = extract_text_from_content(&chunk.content);
            ResponseEvent::Delta {
                session_id: sid,
                content: text,
                delta_type: DeltaType::Text,
            }
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            let text = extract_text_from_content(&chunk.content);
            ResponseEvent::Delta {
                session_id: sid,
                content: text,
                delta_type: DeltaType::Thinking,
            }
        }
        SessionUpdate::ToolCall(tc) => ResponseEvent::ToolUpdate {
            session_id: sid,
            tool_call_id: tc.tool_call_id.to_string(),
            title: tc.title.clone(),
            status: format!("{:?}", tc.status),
            content: None,
        },
        SessionUpdate::ToolCallUpdate(tcu) => ResponseEvent::ToolUpdate {
            session_id: sid,
            tool_call_id: tcu.tool_call_id.to_string(),
            title: tcu.fields.title.clone().unwrap_or_default(),
            status: tcu
                .fields
                .status
                .as_ref()
                .map(|s| format!("{s:?}"))
                .unwrap_or_default(),
            content: None,
        },
        SessionUpdate::Plan(plan) => ResponseEvent::PlanUpdate {
            session_id: sid,
            plan_json: serde_json::to_value(plan).unwrap_or_default(),
        },
        _ => {
            // Other update types (mode changes, config updates, etc.)
            ResponseEvent::Delta {
                session_id: sid,
                content: String::new(),
                delta_type: DeltaType::Text,
            }
        }
    }
}

fn extract_text_from_content(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(t) => t.text.clone(),
        _ => String::new(),
    }
}
