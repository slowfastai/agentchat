//! WebSocket server that bridges the iOS app and ACP agents.
//!
//! M0: single client connection, single agent. Translates between
//! the iOS WebSocket protocol and ACP session/prompt/update flows.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::{
    ContentBlock, PromptResponse, SessionId, SessionNotification, SessionUpdate,
};
use futures::stream::StreamExt;
use futures::SinkExt;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{ClientMessage, DeltaType, ResponseEvent, SessionTranscript};

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

        // M0: accept one connection at a time.
        loop {
            let accepted = tokio::select! {
                accepted = listener.accept() => accepted,
                _ = shutdown_rx.changed() => {
                    info!("websocket server shutting down");
                    break;
                }
            };

            let (stream, peer) = match accepted {
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

        let setup = {
            let mgr = manager.borrow();
            match mgr.first_agent_id().map(|id| id.to_string()) {
                Some(agent_id) => match mgr.get_agent(&agent_id) {
                    Some(agent) if !agent.is_alive() => Err(ResponseEvent::Error {
                        session_id: None,
                        code: "agent_crashed".into(),
                        message: "agent process exited".into(),
                    }),
                    Some(agent) => match agent.take_update_rx() {
                        Some(update_rx) => Ok((update_rx, agent.subscribe_health())),
                        None => Err(ResponseEvent::Error {
                            session_id: None,
                            code: "update_stream_unavailable".into(),
                            message: "agent update stream is already in use".into(),
                        }),
                    },
                    None => Err(ResponseEvent::Error {
                        session_id: None,
                        code: "no_agent".into(),
                        message: "no agent configured".into(),
                    }),
                },
                None => Err(ResponseEvent::Error {
                    session_id: None,
                    code: "no_agent".into(),
                    message: "no agent configured".into(),
                }),
            }
        };

        let (mut update_rx, mut health_rx) = match setup {
            Ok(state) => state,
            Err(event) => {
                if let Some(msg) = serialize_event(&event) {
                    let _ = ws_tx.send(Message::Text(msg.into())).await;
                }
                return;
            }
        };

        // Channel so the incoming-message handler can send responses back to the WS.
        let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<ResponseEvent>();
        let internal_sessions = Rc::new(RefCell::new(HashMap::<
            String,
            mpsc::UnboundedSender<SessionNotification>,
        >::new()));

        // Task 1: Route ACP session updates either internally or back to the WebSocket client.
        let resp_tx_updates = resp_tx.clone();
        let session_store_updates = session_store.clone();
        let internal_sessions_updates = internal_sessions.clone();
        tokio::task::spawn_local(async move {
            while let Some(notification) = update_rx.recv().await {
                let sid = notification.session_id.to_string();

                if let Some(tx) = internal_sessions_updates.borrow().get(&sid).cloned() {
                    let _ = tx.send(notification);
                    continue;
                }

                session_store_updates
                    .borrow_mut()
                    .record_notification(&sid, &notification);
                let event = map_session_update(&notification);
                if resp_tx_updates.send(event).is_err() {
                    break;
                }
            }
        });

        // Task 2: Notify the client if the backing agent exits unexpectedly.
        let resp_tx_health = resp_tx.clone();
        tokio::task::spawn_local(async move {
            if !*health_rx.borrow() {
                let _ = resp_tx_health.send(ResponseEvent::Error {
                    session_id: None,
                    code: "agent_crashed".into(),
                    message: "agent process exited".into(),
                });
                return;
            }

            loop {
                match health_rx.changed().await {
                    Ok(()) => {
                        if !*health_rx.borrow() {
                            let _ = resp_tx_health.send(ResponseEvent::Error {
                                session_id: None,
                                code: "agent_crashed".into(),
                                message: "agent process exited".into(),
                            });
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = resp_tx_health.send(ResponseEvent::Error {
                            session_id: None,
                            code: "agent_crashed".into(),
                            message: "agent process exited".into(),
                        });
                        break;
                    }
                }
            }
        });

        // Task 3: Forward response events -> WebSocket frames.
        tokio::task::spawn_local(async move {
            while let Some(event) = resp_rx.recv().await {
                let Some(json) = serialize_event(&event) else {
                    continue;
                };

                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        });

        let (prompt_done_tx, mut prompt_done_rx) =
            mpsc::unbounded_channel::<(String, agent_client_protocol::Result<PromptResponse>)>();
        let mut created_sessions = Vec::new();
        let mut active_prompt_session: Option<String> = None;
        let mut skill_injected_sessions = HashSet::new();

        // Task 4: Process incoming WebSocket messages from iOS.
        loop {
            tokio::select! {
                maybe_prompt = prompt_done_rx.recv() => {
                    if let Some((session_id, result)) = maybe_prompt {
                        if active_prompt_session.as_deref() == Some(session_id.as_str()) {
                            active_prompt_session = None;
                        }

                        match result {
                            Ok(resp) => {
                                // Give late ACP notifications a short window to reach the routing task.
                                tokio::time::sleep(std::time::Duration::from_millis(25)).await;

                                let stop_reason = format!("{:?}", resp.stop_reason);
                                let _ = resp_tx.send(ResponseEvent::TurnEnd {
                                    session_id: session_id.clone(),
                                    stop_reason: stop_reason.clone(),
                                });
                                session_store.borrow_mut().record_turn_end(&session_id, &stop_reason);

                                let flush_store = session_store.clone();
                                let flush_session_id = session_id.clone();
                                tokio::task::spawn_local(async move {
                                    if let Err(e) = flush_session_snapshot(flush_store, flush_session_id.clone()).await {
                                        warn!("failed to flush session {}: {}", flush_session_id, e);
                                    }
                                });
                            }
                            Err(e) => {
                                let _ = resp_tx.send(ResponseEvent::Error {
                                    session_id: Some(session_id),
                                    code: "prompt_failed".into(),
                                    message: e.to_string(),
                                });
                            }
                        }
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
                            let (agent_id, agent, is_alive) = {
                                let mgr = manager.borrow();
                                let agent_id = mgr.first_agent_id().map(|id| id.to_string());
                                let agent = agent_id.as_deref().and_then(|id| mgr.get_agent(id));
                                let is_alive = agent_id
                                    .as_deref()
                                    .map(|id| mgr.is_agent_alive(id))
                                    .unwrap_or(false);
                                (agent_id, agent, is_alive)
                            };

                            match (agent_id, agent, is_alive) {
                                (Some(_), Some(_), false) => {
                                    let _ = resp_tx.send(ResponseEvent::Error {
                                        session_id: None,
                                        code: "agent_crashed".into(),
                                        message: "agent process exited".into(),
                                    });
                                }
                                (Some(agent_id), Some(agent), true) => match agent.new_session(cwd).await {
                                    Ok(resp) => {
                                        let sid = resp.session_id.to_string();
                                        manager.borrow_mut().register_session(sid.clone(), agent_id.clone());
                                        created_sessions.push(sid.clone());
                                        session_store
                                            .borrow_mut()
                                            .start_session(&sid, &agent_id, &working_dir);
                                        let _ = resp_tx.send(ResponseEvent::SessionCreated { session_id: sid });
                                    }
                                    Err(e) => {
                                        let _ = resp_tx.send(ResponseEvent::Error {
                                            session_id: None,
                                            code: "create_session_failed".into(),
                                            message: e.to_string(),
                                        });
                                    }
                                },
                                _ => {
                                    let _ = resp_tx.send(ResponseEvent::Error {
                                        session_id: None,
                                        code: "no_agent".into(),
                                        message: "no agent configured".into(),
                                    });
                                }
                            }
                        }
                        ClientMessage::Prompt {
                            session_id,
                            content,
                        } => {
                            if active_prompt_session.is_some() {
                                let _ = resp_tx.send(ResponseEvent::Error {
                                    session_id: Some(session_id),
                                    code: "prompt_in_progress".into(),
                                    message: "another prompt is already running".into(),
                                });
                                continue;
                            }

                            let (agent, is_alive) = {
                                let mgr = manager.borrow();
                                let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
                                let agent = agent_id.as_deref().and_then(|id| mgr.get_agent(id));
                                let is_alive = agent_id
                                    .as_deref()
                                    .map(|id| mgr.is_agent_alive(id))
                                    .unwrap_or(false);
                                (agent, is_alive)
                            };

                            match (agent, is_alive) {
                                (Some(_), false) => {
                                    let _ = resp_tx.send(ResponseEvent::Error {
                                        session_id: Some(session_id),
                                        code: "agent_crashed".into(),
                                        message: "agent process exited".into(),
                                    });
                                }
                                (Some(agent), true) => {
                                    session_store.borrow_mut().record_prompt(&session_id, &content);
                                    let prompt_content = maybe_inject_skill_context(
                                        skill_store.as_ref(),
                                        &mut skill_injected_sessions,
                                        &session_id,
                                        content,
                                    )
                                    .await;

                                    active_prompt_session = Some(session_id.clone());
                                    let prompt_done_tx = prompt_done_tx.clone();
                                    tokio::task::spawn_local(async move {
                                        let result = agent
                                            .prompt(SessionId::new(session_id.clone()), prompt_content)
                                            .await;
                                        if prompt_done_tx.send((session_id, result)).is_err() {
                                            warn!("prompt completion receiver dropped");
                                        }
                                    });
                                }
                                (None, _) => {
                                    let _ = resp_tx.send(ResponseEvent::Error {
                                        session_id: Some(session_id),
                                        code: "no_agent".into(),
                                        message: "no agent for this session".into(),
                                    });
                                }
                            }
                        }
                        ClientMessage::Cancel { session_id } => {
                            let agent = {
                                let mgr = manager.borrow();
                                let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
                                agent_id.as_deref().and_then(|id| mgr.get_agent(id))
                            };

                            match agent {
                                Some(agent) => {
                                    if let Err(e) = agent.cancel(SessionId::new(session_id.clone())).await {
                                        warn!("cancel failed for session {session_id}: {e}");
                                    }
                                }
                                None => {
                                    warn!("cancel failed: no agent for session {session_id}");
                                }
                            }
                        }
                        ClientMessage::ListSkills => {
                            match skill_store.list_skills().await {
                                Ok(skills) => {
                                    let _ = resp_tx.send(ResponseEvent::SkillList { skills });
                                }
                                Err(e) => {
                                    warn!("failed to list skills: {e}");
                                    let _ = resp_tx.send(ResponseEvent::SkillList { skills: Vec::new() });
                                }
                            }
                        }
                        ClientMessage::GetSkill { name } => {
                            match skill_store.read_skill(&name).await {
                                Ok(content) => {
                                    let _ = resp_tx.send(ResponseEvent::SkillContent { name, content });
                                }
                                Err(e) => {
                                    let _ = resp_tx.send(ResponseEvent::Error {
                                        session_id: None,
                                        code: "skill_not_found".into(),
                                        message: e,
                                    });
                                }
                            }
                        }
                        ClientMessage::DistillSession { session_id } => {
                            spawn_distillation_task(
                                resp_tx.clone(),
                                manager.clone(),
                                session_store.clone(),
                                internal_sessions.clone(),
                                distiller.clone(),
                                session_id,
                            );
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!("closing client connection for shutdown");
                    break;
                }
            }
        }

        if let Some(session_id) = active_prompt_session.take() {
            let agent = {
                let mgr = manager.borrow();
                let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
                agent_id.as_deref().and_then(|id| mgr.get_agent(id))
            };

            if let Some(agent) = agent {
                if let Err(e) = agent.cancel(SessionId::new(session_id.clone())).await {
                    warn!("disconnect cleanup cancel failed for session {session_id}: {e}");
                }
            }
        }

        cleanup_created_sessions(&session_store, &created_sessions).await;
        manager.borrow_mut().remove_sessions(&created_sessions);
        info!("client disconnected");
    }
}

async fn maybe_inject_skill_context(
    skill_store: &SkillStore,
    injected_sessions: &mut HashSet<String>,
    session_id: &str,
    content: String,
) -> String {
    if !injected_sessions.insert(session_id.to_string()) {
        return content;
    }

    let skills = match skill_store.list_skills().await {
        Ok(skills) => skills,
        Err(e) => {
            warn!("failed to load skills for prompt injection: {e}");
            return content;
        }
    };

    if skills.is_empty() {
        return content;
    }

    let listing = skills
        .iter()
        .map(|skill| format!("- .agentchat/skills/{}", skill.name))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "[Project knowledge available:\n{}\nRead relevant skills with read_text_file.]\n\n{}",
        listing, content
    )
}

async fn flush_session_snapshot(
    session_store: Rc<RefCell<SessionStore>>,
    session_id: String,
) -> Result<(), String> {
    let flush = {
        let store = session_store.borrow();
        store.flush_session(&session_id)
    };

    flush.await.map(|_| ())
}

async fn cleanup_created_sessions(
    session_store: &Rc<RefCell<SessionStore>>,
    created_sessions: &[String],
) {
    for session_id in created_sessions {
        if let Err(e) = flush_session_snapshot(session_store.clone(), session_id.clone()).await {
            warn!(
                "failed to flush session {} during cleanup: {}",
                session_id, e
            );
        }
        session_store.borrow_mut().remove_session(session_id);
    }
}

async fn load_transcript(
    session_store: Rc<RefCell<SessionStore>>,
    session_id: &str,
) -> Result<SessionTranscript, String> {
    if let Some(transcript) = session_store.borrow().get_transcript(session_id).cloned() {
        return Ok(transcript);
    }

    let load = {
        let store = session_store.borrow();
        store.load_transcript(session_id)
    };

    load.await
}

fn spawn_distillation_task(
    resp_tx: mpsc::UnboundedSender<ResponseEvent>,
    manager: Rc<RefCell<AgentManager>>,
    session_store: Rc<RefCell<SessionStore>>,
    internal_sessions: Rc<RefCell<HashMap<String, mpsc::UnboundedSender<SessionNotification>>>>,
    distiller: Rc<Distiller>,
    session_id: String,
) {
    tokio::task::spawn_local(async move {
        let transcript = match load_transcript(session_store.clone(), &session_id).await {
            Ok(transcript) => transcript,
            Err(e) => {
                send_distillation_status(&resp_tx, &session_id, "failed", e);
                return;
            }
        };

        let (agent, is_alive) = {
            let mgr = manager.borrow();
            let agent = mgr.get_agent(&transcript.agent_id);
            let is_alive = mgr.is_agent_alive(&transcript.agent_id);
            (agent, is_alive)
        };

        let agent = match (agent, is_alive) {
            (Some(_), false) => {
                send_distillation_status(&resp_tx, &session_id, "failed", "agent process exited");
                return;
            }
            (Some(agent), true) => agent,
            (None, _) => {
                send_distillation_status(
                    &resp_tx,
                    &session_id,
                    "failed",
                    "no agent for this session",
                );
                return;
            }
        };

        let distill_session = match agent
            .new_session(PathBuf::from(&transcript.working_dir))
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                send_distillation_status(
                    &resp_tx,
                    &session_id,
                    "failed",
                    format!("failed to create distillation session: {e}"),
                );
                return;
            }
        };

        let distill_session_id = distill_session.session_id.to_string();
        let (distill_tx, distill_rx) = mpsc::unbounded_channel();
        internal_sessions
            .borrow_mut()
            .insert(distill_session_id.clone(), distill_tx);

        send_distillation_status(&resp_tx, &session_id, "started", "distillation started");

        let result = distiller
            .distill(agent, distill_session_id.clone(), transcript, distill_rx)
            .await;

        internal_sessions.borrow_mut().remove(&distill_session_id);

        match result {
            Ok(skills) => {
                send_distillation_status(
                    &resp_tx,
                    &session_id,
                    "completed",
                    format!("Updated {} skills", skills.len()),
                );
            }
            Err(e) => {
                send_distillation_status(&resp_tx, &session_id, "failed", e);
            }
        }
    });
}

fn send_distillation_status(
    resp_tx: &mpsc::UnboundedSender<ResponseEvent>,
    session_id: &str,
    status: &str,
    message: impl Into<String>,
) {
    let _ = resp_tx.send(ResponseEvent::DistillationStatus {
        session_id: session_id.to_string(),
        status: status.to_string(),
        message: message.into(),
    });
}

fn serialize_event(event: &ResponseEvent) -> Option<String> {
    match serde_json::to_string(event) {
        Ok(json) => Some(json),
        Err(e) => {
            error!("failed to serialize response event: {e}");
            None
        }
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

#[cfg(test)]
mod tests {
    use agent_client_protocol::{AvailableCommandsUpdate, ContentChunk, ToolCall, ToolCallStatus};

    use super::*;

    #[test]
    fn map_session_update_maps_agent_message_chunk_to_text_delta() {
        let notification = SessionNotification::new(
            "session-1",
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from("hello"))),
        );

        assert_eq!(
            map_session_update(&notification),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                content: "hello".into(),
                delta_type: DeltaType::Text,
            }
        );
    }

    #[test]
    fn map_session_update_maps_agent_thought_chunk_to_thinking_delta() {
        let notification = SessionNotification::new(
            "session-1",
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::from("thinking"))),
        );

        assert_eq!(
            map_session_update(&notification),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                content: "thinking".into(),
                delta_type: DeltaType::Thinking,
            }
        );
    }

    #[test]
    fn map_session_update_maps_tool_call_to_tool_update() {
        let notification = SessionNotification::new(
            "session-1",
            SessionUpdate::ToolCall(
                ToolCall::new("tool-1", "Read file").status(ToolCallStatus::InProgress),
            ),
        );

        assert_eq!(
            map_session_update(&notification),
            ResponseEvent::ToolUpdate {
                session_id: "session-1".into(),
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "InProgress".into(),
                content: None,
            }
        );
    }

    #[test]
    fn map_session_update_maps_unknown_variant_to_empty_delta() {
        let notification = SessionNotification::new(
            "session-1",
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(Vec::new())),
        );

        assert_eq!(
            map_session_update(&notification),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                content: String::new(),
                delta_type: DeltaType::Text,
            }
        );
    }
}
