use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use agent_client_protocol::{
    ContentBlock, PromptResponse, SessionId, SessionNotification, SessionUpdate,
};
use tokio::sync::mpsc;
use tracing::{error, warn};

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{ClientMessage, DeltaType, ResponseEvent, SessionTranscript, SkillInfo};

pub struct AppProtocolSession {
    manager: Rc<RefCell<AgentManager>>,
    session_store: Rc<RefCell<SessionStore>>,
    skill_store: Rc<SkillStore>,
    distiller: Rc<Distiller>,
    response_tx: mpsc::UnboundedSender<ResponseEvent>,
    response_rx: Option<mpsc::UnboundedReceiver<ResponseEvent>>,
    internal_sessions: Rc<RefCell<HashMap<String, mpsc::UnboundedSender<SessionNotification>>>>,
    created_sessions: Vec<String>,
    active_prompt_session: Rc<RefCell<Option<String>>>,
}

impl AppProtocolSession {
    pub fn new(
        manager: Rc<RefCell<AgentManager>>,
        session_store: Rc<RefCell<SessionStore>>,
        skill_store: Rc<SkillStore>,
        distiller: Rc<Distiller>,
    ) -> Result<Self, ResponseEvent> {
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

        let (mut update_rx, mut health_rx) = setup?;
        let (response_tx, response_rx) = mpsc::unbounded_channel::<ResponseEvent>();
        let internal_sessions = Rc::new(RefCell::new(HashMap::<
            String,
            mpsc::UnboundedSender<SessionNotification>,
        >::new()));

        let response_tx_updates = response_tx.clone();
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
                if response_tx_updates.send(event).is_err() {
                    break;
                }
            }
        });

        let response_tx_health = response_tx.clone();
        tokio::task::spawn_local(async move {
            if !*health_rx.borrow() {
                let _ = response_tx_health.send(ResponseEvent::Error {
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
                            let _ = response_tx_health.send(ResponseEvent::Error {
                                session_id: None,
                                code: "agent_crashed".into(),
                                message: "agent process exited".into(),
                            });
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = response_tx_health.send(ResponseEvent::Error {
                            session_id: None,
                            code: "agent_crashed".into(),
                            message: "agent process exited".into(),
                        });
                        break;
                    }
                }
            }
        });

        Ok(Self {
            manager,
            session_store,
            skill_store,
            distiller,
            response_tx,
            response_rx: Some(response_rx),
            internal_sessions,
            created_sessions: Vec::new(),
            active_prompt_session: Rc::new(RefCell::new(None)),
        })
    }

    pub fn take_response_rx(&mut self) -> Option<mpsc::UnboundedReceiver<ResponseEvent>> {
        self.response_rx.take()
    }

    pub async fn handle_client_message(&mut self, client_msg: ClientMessage) {
        match client_msg {
            ClientMessage::CreateSession { working_dir } => {
                self.handle_create_session(working_dir).await;
            }
            ClientMessage::Prompt {
                session_id,
                content,
            } => {
                self.handle_prompt(session_id, content).await;
            }
            ClientMessage::Cancel { session_id } => {
                self.handle_cancel(session_id).await;
            }
            ClientMessage::ListSkills => {
                self.handle_list_skills().await;
            }
            ClientMessage::GetSkill { name } => {
                self.handle_get_skill(name).await;
            }
            ClientMessage::DistillSession { session_id } => {
                spawn_distillation_task(
                    self.response_tx.clone(),
                    self.manager.clone(),
                    self.session_store.clone(),
                    self.internal_sessions.clone(),
                    self.distiller.clone(),
                    session_id,
                );
            }
        }
    }

    pub async fn shutdown(&mut self) {
        let active_session_id = self.active_prompt_session.borrow_mut().take();
        if let Some(session_id) = active_session_id {
            let agent = {
                let mgr = self.manager.borrow();
                let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
                agent_id.as_deref().and_then(|id| mgr.get_agent(id))
            };

            if let Some(agent) = agent {
                if let Err(err) = agent.cancel(SessionId::new(session_id.clone())).await {
                    warn!("disconnect cleanup cancel failed for session {session_id}: {err}");
                }
            }
        }

        cleanup_created_sessions(&self.session_store, &self.created_sessions).await;
        self.manager
            .borrow_mut()
            .remove_sessions(&self.created_sessions);
        self.created_sessions.clear();
    }

    async fn handle_create_session(&mut self, working_dir: String) {
        let cwd = PathBuf::from(&working_dir);
        let (agent_id, agent, is_alive) = {
            let mgr = self.manager.borrow();
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
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    code: "agent_crashed".into(),
                    message: "agent process exited".into(),
                });
            }
            (Some(agent_id), Some(agent), true) => match agent.new_session(cwd).await {
                Ok(resp) => {
                    let sid = resp.session_id.to_string();
                    self.manager
                        .borrow_mut()
                        .register_session(sid.clone(), agent_id.clone());
                    self.created_sessions.push(sid.clone());
                    self.session_store
                        .borrow_mut()
                        .start_session(&sid, &agent_id, &working_dir);
                    let _ = self
                        .response_tx
                        .send(ResponseEvent::SessionCreated { session_id: sid });
                }
                Err(err) => {
                    let _ = self.response_tx.send(ResponseEvent::Error {
                        session_id: None,
                        code: "create_session_failed".into(),
                        message: err.to_string(),
                    });
                }
            },
            _ => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    code: "no_agent".into(),
                    message: "no agent configured".into(),
                });
            }
        }
    }

    async fn handle_prompt(&mut self, session_id: String, content: String) {
        if self.active_prompt_session.borrow().is_some() {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: Some(session_id),
                code: "prompt_in_progress".into(),
                message: "another prompt is already running".into(),
            });
            return;
        }

        let (agent_id, agent, is_alive) = {
            let mgr = self.manager.borrow();
            let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
            let agent = agent_id.as_deref().and_then(|id| mgr.get_agent(id));
            let is_alive = agent_id
                .as_deref()
                .map(|id| mgr.is_agent_alive(id))
                .unwrap_or(false);
            (agent_id, agent, is_alive)
        };

        match (agent_id, agent, is_alive) {
            (Some(_), Some(_), false) => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: Some(session_id),
                    code: "agent_crashed".into(),
                    message: "agent process exited".into(),
                });
            }
            (Some(agent_id), Some(agent), true) => {
                self.session_store
                    .borrow_mut()
                    .record_prompt(&session_id, &content);
                let prompt_content =
                    maybe_inject_skill_context(self.skill_store.as_ref(), Some(&agent_id), content)
                        .await;

                *self.active_prompt_session.borrow_mut() = Some(session_id.clone());
                let response_tx = self.response_tx.clone();
                let session_store = self.session_store.clone();
                let active_prompt_session = self.active_prompt_session.clone();
                tokio::task::spawn_local(async move {
                    let result = agent
                        .prompt(SessionId::new(session_id.clone()), prompt_content)
                        .await;
                    if active_prompt_session.borrow().as_deref() == Some(session_id.as_str()) {
                        *active_prompt_session.borrow_mut() = None;
                    }

                    handle_prompt_completion(response_tx, session_store, session_id, result).await;
                });
            }
            _ => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: Some(session_id),
                    code: "no_agent".into(),
                    message: "no agent for this session".into(),
                });
            }
        }
    }

    async fn handle_cancel(&self, session_id: String) {
        let agent = {
            let mgr = self.manager.borrow();
            let agent_id = mgr.agent_for_session(&session_id).map(|id| id.to_string());
            agent_id.as_deref().and_then(|id| mgr.get_agent(id))
        };

        match agent {
            Some(agent) => {
                if let Err(err) = agent.cancel(SessionId::new(session_id.clone())).await {
                    warn!("cancel failed for session {session_id}: {err}");
                }
            }
            None => {
                warn!("cancel failed: no agent for session {session_id}");
            }
        }
    }

    async fn handle_list_skills(&self) {
        match self.skill_store.list_skills().await {
            Ok(skills) => {
                let _ = self.response_tx.send(ResponseEvent::SkillList { skills });
            }
            Err(err) => {
                warn!("failed to list skills: {err}");
                let _ = self
                    .response_tx
                    .send(ResponseEvent::SkillList { skills: Vec::new() });
            }
        }
    }

    async fn handle_get_skill(&self, name: String) {
        match self.skill_store.read_skill(&name).await {
            Ok(content) => {
                let _ = self
                    .response_tx
                    .send(ResponseEvent::SkillContent { name, content });
            }
            Err(err) => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    code: "skill_not_found".into(),
                    message: err,
                });
            }
        }
    }
}

pub fn serialize_event(event: &ResponseEvent) -> Option<String> {
    match serde_json::to_string(event) {
        Ok(json) => Some(json),
        Err(err) => {
            error!("failed to serialize response event: {err}");
            None
        }
    }
}

async fn handle_prompt_completion(
    response_tx: mpsc::UnboundedSender<ResponseEvent>,
    session_store: Rc<RefCell<SessionStore>>,
    session_id: String,
    result: agent_client_protocol::Result<PromptResponse>,
) {
    match result {
        Ok(resp) => {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;

            let stop_reason = format!("{:?}", resp.stop_reason);
            let _ = response_tx.send(ResponseEvent::TurnEnd {
                session_id: session_id.clone(),
                stop_reason: stop_reason.clone(),
            });
            session_store
                .borrow_mut()
                .record_turn_end(&session_id, &stop_reason);

            let flush_store = session_store.clone();
            let flush_session_id = session_id.clone();
            tokio::task::spawn_local(async move {
                if let Err(err) =
                    flush_session_snapshot(flush_store, flush_session_id.clone()).await
                {
                    warn!("failed to flush session {}: {}", flush_session_id, err);
                }
            });
        }
        Err(err) => {
            let _ = response_tx.send(ResponseEvent::Error {
                session_id: Some(session_id),
                code: "prompt_failed".into(),
                message: err.to_string(),
            });
        }
    }
}

async fn maybe_inject_skill_context(
    skill_store: &SkillStore,
    agent_id: Option<&str>,
    content: String,
) -> String {
    let shared_skills = match skill_store.list_shared_skills().await {
        Ok(skills) => skills,
        Err(err) => {
            warn!("failed to load shared skills for prompt injection: {err}");
            return content;
        }
    };

    let agent_skills = match agent_id {
        Some(agent_id) => match skill_store.list_agent_skills(agent_id).await {
            Ok(skills) => skills,
            Err(err) => {
                warn!("failed to load agent-specific skills for prompt injection: {err}");
                Vec::new()
            }
        },
        None => Vec::new(),
    };

    if shared_skills.is_empty() && agent_skills.is_empty() {
        return content;
    }

    let mut sections = Vec::new();
    if !shared_skills.is_empty() {
        sections.push(format!(
            "Shared project knowledge available to every agent:\n{}",
            render_skill_listing(&shared_skills),
        ));
    }
    if let Some(agent_id) = agent_id {
        if !agent_skills.is_empty() {
            sections.push(format!(
                "Agent-specific knowledge for {agent_id}:\n{}",
                render_skill_listing(&agent_skills),
            ));
        }
    }
    sections.push("Read relevant skills with read_text_file.".into());

    format!("[{}]\n\n{}", sections.join("\n"), content)
}

fn render_skill_listing(skills: &[SkillInfo]) -> String {
    skills
        .iter()
        .map(|skill| format!("- {}", skill.path))
        .collect::<Vec<_>>()
        .join("\n")
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
        if let Err(err) = flush_session_snapshot(session_store.clone(), session_id.clone()).await {
            warn!(
                "failed to flush session {} during cleanup: {}",
                session_id, err
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
    response_tx: mpsc::UnboundedSender<ResponseEvent>,
    manager: Rc<RefCell<AgentManager>>,
    session_store: Rc<RefCell<SessionStore>>,
    internal_sessions: Rc<RefCell<HashMap<String, mpsc::UnboundedSender<SessionNotification>>>>,
    distiller: Rc<Distiller>,
    session_id: String,
) {
    tokio::task::spawn_local(async move {
        let transcript = match load_transcript(session_store.clone(), &session_id).await {
            Ok(transcript) => transcript,
            Err(err) => {
                send_distillation_status(&response_tx, &session_id, "failed", err);
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
                send_distillation_status(
                    &response_tx,
                    &session_id,
                    "failed",
                    "agent process exited",
                );
                return;
            }
            (Some(agent), true) => agent,
            (None, _) => {
                send_distillation_status(
                    &response_tx,
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
            Err(err) => {
                send_distillation_status(
                    &response_tx,
                    &session_id,
                    "failed",
                    format!("failed to create distillation session: {err}"),
                );
                return;
            }
        };

        let distill_session_id = distill_session.session_id.to_string();
        let (distill_tx, distill_rx) = mpsc::unbounded_channel();
        internal_sessions
            .borrow_mut()
            .insert(distill_session_id.clone(), distill_tx);

        send_distillation_status(&response_tx, &session_id, "started", "distillation started");

        let result = distiller
            .distill(agent, distill_session_id.clone(), transcript, distill_rx)
            .await;

        internal_sessions.borrow_mut().remove(&distill_session_id);

        match result {
            Ok(skills) => {
                send_distillation_status(
                    &response_tx,
                    &session_id,
                    "completed",
                    format!("Updated {} skills", skills.len()),
                );
            }
            Err(err) => {
                send_distillation_status(&response_tx, &session_id, "failed", err);
            }
        }
    });
}

fn send_distillation_status(
    response_tx: &mpsc::UnboundedSender<ResponseEvent>,
    session_id: &str,
    status: &str,
    message: impl Into<String>,
) {
    let _ = response_tx.send(ResponseEvent::DistillationStatus {
        session_id: session_id.to_string(),
        status: status.to_string(),
        message: message.into(),
    });
}

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
                .map(|status| format!("{status:?}"))
                .unwrap_or_default(),
            content: None,
        },
        SessionUpdate::Plan(plan) => ResponseEvent::PlanUpdate {
            session_id: sid,
            plan_json: serde_json::to_value(plan).unwrap_or_default(),
        },
        _ => ResponseEvent::Delta {
            session_id: sid,
            content: String::new(),
            delta_type: DeltaType::Text,
        },
    }
}

fn extract_text_from_content(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(text) => text.text.clone(),
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
