use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use tokio::sync::{broadcast, mpsc};
use tracing::{error, warn};
use uuid::Uuid;

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::backend::{AgentNotification, AgentPromptResult, AgentUpdate};
use agentchat_core::distiller::Distiller;
use agentchat_core::session_event_log::SessionEventLog;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_core::thread_event_log::ThreadEventLog;
use agentchat_core::thread_store::{
    participant_to_protocol, thread_to_snapshot, thread_to_summary, ThreadStore,
};
use agentchat_protocol::{
    AgentSummary, AssistantMessageState, ClientMessage, DeltaType, ParticipantKind,
    ParticipantState, ResponseEvent, SessionEvent, SessionSnapshot, SessionState, SessionSummary,
    SessionTranscript, SkillInfo, ThreadParticipant, ThreadSender, ThreadSnapshot, ThreadState,
};

pub struct AppProtocolSession {
    manager: Rc<RefCell<AgentManager>>,
    session_store: Rc<RefCell<SessionStore>>,
    skill_store: Rc<SkillStore>,
    distiller: Rc<Distiller>,
    session_event_log: Rc<RefCell<SessionEventLog>>,
    thread_store: Rc<RefCell<ThreadStore>>,
    thread_event_log: Rc<RefCell<ThreadEventLog>>,
    response_tx: broadcast::Sender<ResponseEvent>,
    internal_sessions: Rc<RefCell<HashMap<String, mpsc::UnboundedSender<AgentNotification>>>>,
    created_sessions: Vec<String>,
    active_prompt_sessions: Rc<RefCell<HashSet<String>>>,
    active_assistant_messages: Rc<RefCell<HashMap<String, ActiveAssistantMessage>>>,
}

#[derive(Debug, Clone)]
struct ActiveAssistantMessage {
    message_id: String,
    turn_id: String,
    thinking: String,
    response: String,
}

impl ActiveAssistantMessage {
    fn new() -> Self {
        Self {
            message_id: format!("message-{}", Uuid::new_v4().simple()),
            turn_id: format!("turn-{}", Uuid::new_v4().simple()),
            thinking: String::new(),
            response: String::new(),
        }
    }
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
            let agent_ids = mgr.agent_ids();
            if agent_ids.is_empty() {
                Err(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "no_agent".into(),
                    message: "no agent configured".into(),
                })
            } else {
                let mut streams = Vec::new();
                for agent_id in agent_ids {
                    let Some(agent) = mgr.get_agent(&agent_id) else {
                        return Err(ResponseEvent::Error {
                            session_id: None,
                            event_seq: None,
                            code: "no_agent".into(),
                            message: "no agent configured".into(),
                        });
                    };

                    if !agent.is_alive() {
                        return Err(ResponseEvent::Error {
                            session_id: None,
                            event_seq: None,
                            code: "agent_crashed".into(),
                            message: "agent process exited".into(),
                        });
                    }

                    let Some(update_rx) = agent.take_update_rx() else {
                        return Err(ResponseEvent::Error {
                            session_id: None,
                            event_seq: None,
                            code: "update_stream_unavailable".into(),
                            message: "agent update stream is already in use".into(),
                        });
                    };

                    streams.push((agent_id, update_rx, agent.subscribe_health()));
                }
                Ok(streams)
            }
        };

        let setups = setup?;
        let (response_tx, _) = broadcast::channel::<ResponseEvent>(1024);
        let sessions_dir = session_store.borrow().sessions_dir().to_path_buf();
        let threads_dir = sessions_dir
            .parent()
            .map(|parent| parent.join("threads"))
            .unwrap_or_else(|| PathBuf::from(".agentchat").join("threads"));
        let session_event_log = Rc::new(RefCell::new(SessionEventLog::new(sessions_dir)));
        let thread_store = Rc::new(RefCell::new(ThreadStore::new()));
        let thread_event_log = Rc::new(RefCell::new(ThreadEventLog::new(threads_dir)));
        let internal_sessions = Rc::new(RefCell::new(HashMap::<
            String,
            mpsc::UnboundedSender<AgentNotification>,
        >::new()));
        let active_assistant_messages = Rc::new(RefCell::new(HashMap::<
            String,
            ActiveAssistantMessage,
        >::new()));

        for (agent_id, mut update_rx, mut health_rx) in setups {
            let manager_updates = manager.clone();
            let response_tx_updates = response_tx.clone();
            let session_store_updates = session_store.clone();
            let session_event_log_updates = session_event_log.clone();
            let thread_store_updates = thread_store.clone();
            let thread_event_log_updates = thread_event_log.clone();
            let internal_sessions_updates = internal_sessions.clone();
            let active_assistant_messages_updates = active_assistant_messages.clone();
            tokio::task::spawn_local(async move {
                while let Some(notification) = update_rx.recv().await {
                    let upstream_session_id = notification.session_id.to_string();

                    if let Some(tx) = internal_sessions_updates
                        .borrow()
                        .get(&upstream_session_id)
                        .cloned()
                    {
                        let _ = tx.send(notification);
                        continue;
                    }

                    let Some(public_session_id) = manager_updates
                        .borrow()
                        .public_session_for_upstream(&agent_id, &upstream_session_id)
                        .map(|session_id| session_id.to_string())
                    else {
                        warn!(
                            "dropping update for unknown upstream session {} from agent {}",
                            upstream_session_id, agent_id
                        );
                        continue;
                    };

                    let translated =
                        rewrite_notification_session_id(&notification, &public_session_id);
                    session_store_updates
                        .borrow_mut()
                        .record_notification(&public_session_id, &translated);
                    let event_seq = session_event_log_updates
                        .borrow_mut()
                        .next_seq(&public_session_id);
                    let event = map_session_update(&translated, event_seq);
                    journal_and_broadcast_event(
                        &session_event_log_updates,
                        &response_tx_updates,
                        event.clone(),
                    );
                    maybe_broadcast_thread_event_for_session_event(
                        &thread_store_updates,
                        &thread_event_log_updates,
                        &response_tx_updates,
                        &active_assistant_messages_updates,
                        &event,
                    );
                }
            });

            let response_tx_health = response_tx.clone();
            tokio::task::spawn_local(async move {
                if !*health_rx.borrow() {
                    let _ = response_tx_health.send(ResponseEvent::Error {
                        session_id: None,
                        event_seq: None,
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
                                    event_seq: None,
                                    code: "agent_crashed".into(),
                                    message: "agent process exited".into(),
                                });
                                break;
                            }
                        }
                        Err(_) => {
                            let _ = response_tx_health.send(ResponseEvent::Error {
                                session_id: None,
                                event_seq: None,
                                code: "agent_crashed".into(),
                                message: "agent process exited".into(),
                            });
                            break;
                        }
                    }
                }
            });
        }

        Ok(Self {
            manager,
            session_store,
            skill_store,
            distiller,
            session_event_log,
            thread_store,
            thread_event_log,
            response_tx,
            internal_sessions,
            created_sessions: Vec::new(),
            active_prompt_sessions: Rc::new(RefCell::new(HashSet::new())),
            active_assistant_messages,
        })
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<ResponseEvent> {
        self.response_tx.subscribe()
    }

    pub fn event_sender(&self) -> broadcast::Sender<ResponseEvent> {
        self.response_tx.clone()
    }

    pub async fn handle_client_message(&mut self, client_msg: ClientMessage) {
        match client_msg {
            ClientMessage::CreateSession {
                agent_id,
                working_dir,
            } => {
                self.handle_create_session(agent_id, working_dir).await;
            }
            ClientMessage::ListAgents => {
                self.handle_list_agents().await;
            }
            ClientMessage::CreateThread { title, working_dir } => {
                self.handle_create_thread(title, working_dir).await;
            }
            ClientMessage::ListThreads => {
                self.handle_list_threads().await;
            }
            ClientMessage::AttachThread {
                thread_id,
                after_seq,
            } => {
                self.handle_attach_thread(thread_id, after_seq).await;
            }
            ClientMessage::AddThreadParticipant {
                thread_id,
                agent_id,
            } => {
                self.handle_add_thread_participant(thread_id, agent_id)
                    .await;
            }
            ClientMessage::RemoveThreadParticipant {
                thread_id,
                participant_id,
            } => {
                self.handle_remove_thread_participant(thread_id, participant_id)
                    .await;
            }
            ClientMessage::CloseThread { thread_id } => {
                self.handle_close_thread(thread_id).await;
            }
            ClientMessage::SendThreadMessage {
                thread_id,
                content,
                target_participant_ids,
            } => {
                self.handle_send_thread_message(thread_id, content, target_participant_ids)
                    .await;
            }
            ClientMessage::ListSessions => {
                self.handle_list_sessions().await;
            }
            ClientMessage::AttachSession {
                session_id,
                after_seq,
            } => {
                self.handle_attach_session(session_id, after_seq).await;
            }
            ClientMessage::CloseSession { session_id } => {
                self.handle_close_session(session_id).await;
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
                    self.session_event_log.clone(),
                    self.internal_sessions.clone(),
                    self.distiller.clone(),
                    session_id,
                );
            }
        }
    }

    pub async fn shutdown(&mut self) {
        let active_session_ids = self
            .active_prompt_sessions
            .borrow()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for session_id in active_session_ids {
            let (agent, upstream_session_id) = {
                let mgr = self.manager.borrow();
                let binding = mgr.session_binding(&session_id).cloned();
                let agent = binding
                    .as_ref()
                    .and_then(|binding| mgr.get_agent(&binding.agent_id));
                let upstream_session_id = binding.map(|binding| binding.upstream_session_id);
                (agent, upstream_session_id)
            };

            if let (Some(agent), Some(upstream_session_id)) = (agent, upstream_session_id) {
                if let Err(err) = agent.cancel(upstream_session_id).await {
                    warn!("disconnect cleanup cancel failed for session {session_id}: {err}");
                }
            }
        }
        self.active_prompt_sessions.borrow_mut().clear();

        cleanup_created_sessions(&self.session_store, &self.created_sessions).await;
        self.manager
            .borrow_mut()
            .remove_sessions(&self.created_sessions);
        self.created_sessions.clear();
    }

    async fn create_session_for_agent(
        &mut self,
        requested_agent_id: Option<String>,
        working_dir: String,
    ) -> Result<(String, String), ResponseEvent> {
        let cwd = PathBuf::from(&working_dir);
        let (agent_id, agent, is_alive) = {
            let mgr = self.manager.borrow();
            let resolved_agent_id = requested_agent_id
                .as_deref()
                .map(str::to_string)
                .or_else(|| mgr.first_agent_id().map(|id| id.to_string()));
            let agent = resolved_agent_id
                .as_deref()
                .and_then(|id| mgr.get_agent(id));
            let is_alive = resolved_agent_id
                .as_deref()
                .map(|id| mgr.is_agent_alive(id))
                .unwrap_or(false);
            (resolved_agent_id, agent, is_alive)
        };

        match (agent_id, agent, is_alive) {
            (Some(_), None, _) if requested_agent_id.is_some() => Err(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "agent_not_found".into(),
                message: "no agent with this id".into(),
            }),
            (Some(_), Some(_), false) => Err(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "agent_unavailable".into(),
                message: "agent is not online".into(),
            }),
            (Some(agent_id), Some(agent), true) => match agent.new_session(cwd).await {
                Ok(upstream_session_id) => {
                    let public_session_id = format!("session-{}", Uuid::new_v4().simple());
                    self.manager.borrow_mut().register_session(
                        public_session_id.clone(),
                        agent_id.clone(),
                        upstream_session_id,
                    );
                    self.created_sessions.push(public_session_id.clone());
                    self.session_store.borrow_mut().start_session(
                        &public_session_id,
                        &agent_id,
                        &working_dir,
                    );
                    self.session_event_log
                        .borrow_mut()
                        .init_session(&public_session_id);
                    Ok((public_session_id, agent_id))
                }
                Err(err) => Err(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "create_session_failed".into(),
                    message: err.to_string(),
                }),
            },
            _ => Err(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "no_agent".into(),
                message: "no agent configured".into(),
            }),
        }
    }

    async fn handle_create_session(
        &mut self,
        requested_agent_id: Option<String>,
        working_dir: String,
    ) {
        match self
            .create_session_for_agent(requested_agent_id, working_dir)
            .await
        {
            Ok((public_session_id, agent_id)) => {
                let event_seq = self
                    .session_event_log
                    .borrow_mut()
                    .next_seq(&public_session_id);
                journal_and_broadcast_event(
                    &self.session_event_log,
                    &self.response_tx,
                    ResponseEvent::SessionCreated {
                        session_id: public_session_id,
                        agent_id,
                        event_seq,
                    },
                );
            }
            Err(error) => {
                let _ = self.response_tx.send(error);
            }
        }
    }

    async fn handle_create_thread(&self, title: Option<String>, working_dir: String) {
        let thread = self
            .thread_store
            .borrow_mut()
            .create_thread(title, working_dir);
        self.thread_event_log
            .borrow_mut()
            .init_thread(&thread.thread_id);
        let _ = self.response_tx.send(ResponseEvent::ThreadCreated {
            thread_id: thread.thread_id,
            created_at_ms: thread.created_at_ms,
        });
    }

    async fn handle_list_agents(&self) {
        let agents: Vec<AgentSummary> = self.manager.borrow().list_agents();
        let _ = self.response_tx.send(ResponseEvent::AgentList { agents });
    }

    async fn handle_list_threads(&self) {
        let mut threads = self
            .thread_store
            .borrow()
            .list_threads()
            .into_iter()
            .map(|thread| {
                thread_to_summary(
                    &thread,
                    self.thread_state(&thread.thread_id),
                    self.thread_event_log.borrow().tail_seq(&thread.thread_id),
                )
            })
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| left.created_at_ms.cmp(&right.created_at_ms));
        let _ = self.response_tx.send(ResponseEvent::ThreadList { threads });
    }

    async fn handle_attach_thread(&self, thread_id: String, after_seq: Option<u64>) {
        let snapshot = match self.thread_store.borrow().get_thread(&thread_id).cloned() {
            Some(thread) => self.build_thread_snapshot(&thread),
            None => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_not_found".into(),
                    message: "no live thread with this id".into(),
                });
                return;
            }
        };

        let tail_seq = snapshot.last_thread_seq;
        let replay_after = after_seq.unwrap_or(tail_seq);
        if replay_after > tail_seq {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "thread_replay_after_seq_ahead_of_tail".into(),
                message: format!(
                    "requested after_seq {} is ahead of current thread tail {}",
                    replay_after, tail_seq
                ),
            });
            return;
        }

        let replay_events = self
            .thread_event_log
            .borrow()
            .replay_after(&thread_id, replay_after);

        let _ = self.response_tx.send(ResponseEvent::ThreadAttached {
            thread_id: thread_id.clone(),
        });
        let _ = self
            .response_tx
            .send(ResponseEvent::ThreadSnapshot { snapshot });
        for event in replay_events {
            let _ = self.response_tx.send(event);
        }
        let _ = self.response_tx.send(ResponseEvent::ThreadReplayComplete {
            thread_id,
            last_thread_seq: tail_seq,
        });
    }

    async fn handle_add_thread_participant(&mut self, thread_id: String, agent_id: String) {
        let working_dir = match self.thread_store.borrow().get_thread(&thread_id) {
            Some(thread) => thread.working_dir.clone(),
            None => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_not_found".into(),
                    message: "no live thread with this id".into(),
                });
                return;
            }
        };

        let display_name = self
            .manager
            .borrow()
            .list_agents()
            .into_iter()
            .find(|summary| summary.agent_id == agent_id)
            .map(|summary| summary.name)
            .unwrap_or_else(|| agent_id.clone());

        let session_id = match self
            .create_session_for_agent(Some(agent_id.clone()), working_dir)
            .await
        {
            Ok((session_id, _)) => session_id,
            Err(error) => {
                let _ = self.response_tx.send(error);
                return;
            }
        };

        let participant = match self.thread_store.borrow_mut().add_agent_participant(
            &thread_id,
            agent_id,
            display_name,
            session_id,
        ) {
            Ok(participant) => participant,
            Err(_) => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_not_found".into(),
                    message: "no live thread with this id".into(),
                });
                return;
            }
        };

        let participant = self.build_thread_participant(&participant);
        let thread_seq = self.thread_event_log.borrow_mut().next_seq(&thread_id);
        journal_and_broadcast_thread_event(
            &self.thread_event_log,
            &self.response_tx,
            ResponseEvent::ThreadParticipantAdded {
                thread_id,
                thread_seq,
                participant,
            },
        );
    }

    async fn handle_remove_thread_participant(
        &mut self,
        thread_id: String,
        participant_id: String,
    ) {
        let participant = match self.thread_store.borrow().get_thread(&thread_id) {
            Some(thread) => thread
                .participants
                .iter()
                .find(|participant| participant.participant_id == participant_id)
                .cloned(),
            None => None,
        };
        let Some(participant) = participant else {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "thread_participant_not_found".into(),
                message: "no participant with this id in the thread".into(),
            });
            return;
        };

        if let Some(session_id) = participant.session_id.clone() {
            if self.active_prompt_sessions.borrow().contains(&session_id) {
                let event_seq = Some(self.session_event_log.borrow_mut().next_seq(&session_id));
                let error = ResponseEvent::Error {
                    session_id: Some(session_id),
                    event_seq,
                    code: "session_busy".into(),
                    message: "cannot close a session while a prompt is in progress".into(),
                };
                journal_and_broadcast_event(&self.session_event_log, &self.response_tx, error);
                return;
            }
        }

        let participant = self
            .thread_store
            .borrow_mut()
            .remove_participant(&thread_id, &participant_id)
            .expect("participant must still exist after preflight");

        if let Some(session_id) = participant.session_id {
            if let Err(err) =
                flush_session_snapshot(self.session_store.clone(), session_id.clone()).await
            {
                warn!(
                    "failed to flush session {session_id} before thread participant removal: {err}"
                );
            }
            self.session_store.borrow_mut().remove_session(&session_id);
            self.session_event_log
                .borrow_mut()
                .remove_session(&session_id);
            self.manager.borrow_mut().remove_session(&session_id);
            self.created_sessions
                .retain(|created| created != &session_id);
        }

        let thread_seq = self.thread_event_log.borrow_mut().next_seq(&thread_id);
        journal_and_broadcast_thread_event(
            &self.thread_event_log,
            &self.response_tx,
            ResponseEvent::ThreadParticipantRemoved {
                thread_id,
                thread_seq,
                participant_id,
            },
        );
    }

    async fn handle_close_thread(&mut self, thread_id: String) {
        let thread = match self.thread_store.borrow().get_thread(&thread_id).cloned() {
            Some(thread) => thread,
            None => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_not_found".into(),
                    message: "no live thread with this id".into(),
                });
                return;
            }
        };

        let session_ids = thread
            .participants
            .iter()
            .filter_map(|participant| participant.session_id.clone())
            .collect::<Vec<_>>();
        if session_ids
            .iter()
            .any(|session_id| self.active_prompt_sessions.borrow().contains(session_id))
        {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: None,
                event_seq: None,
                code: "thread_busy".into(),
                message: "cannot close a thread while agent work is in progress".into(),
            });
            return;
        }

        let _ = self.response_tx.send(ResponseEvent::ThreadClosed {
            thread_id: thread_id.clone(),
        });

        let removed_thread = self
            .thread_store
            .borrow_mut()
            .remove_thread(&thread_id)
            .expect("thread must still exist after preflight");

        for session_id in removed_thread
            .participants
            .into_iter()
            .filter_map(|participant| participant.session_id)
        {
            if let Err(err) =
                flush_session_snapshot(self.session_store.clone(), session_id.clone()).await
            {
                warn!("failed to flush session {session_id} before close_thread: {err}");
            }
            self.session_store.borrow_mut().remove_session(&session_id);
            self.session_event_log
                .borrow_mut()
                .remove_session(&session_id);
            self.manager.borrow_mut().remove_session(&session_id);
            self.created_sessions
                .retain(|created| created != &session_id);
        }

        self.thread_event_log.borrow_mut().remove_thread(&thread_id);
    }

    async fn handle_send_thread_message(
        &mut self,
        thread_id: String,
        content: String,
        target_participant_ids: Option<Vec<String>>,
    ) {
        let participants = match self
            .thread_store
            .borrow()
            .target_agent_participants(&thread_id, target_participant_ids.as_deref())
        {
            Ok(participants) => participants,
            Err(err) if err == "thread not found" => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_not_found".into(),
                    message: "no live thread with this id".into(),
                });
                return;
            }
            Err(_) => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code: "thread_participant_not_found".into(),
                    message: "no participant with this id in the thread".into(),
                });
                return;
            }
        };

        let targets = participants
            .iter()
            .map(|participant| participant.participant_id.clone())
            .collect::<Vec<_>>();
        let thread_seq = self.thread_event_log.borrow_mut().next_seq(&thread_id);
        journal_and_broadcast_thread_event(
            &self.thread_event_log,
            &self.response_tx,
            ResponseEvent::ThreadMessage {
                thread_id: thread_id.clone(),
                thread_seq,
                message_id: format!("message-{}", Uuid::new_v4().simple()),
                sender: ThreadSender {
                    kind: ParticipantKind::Human,
                    participant_id: "participant-user".into(),
                    display_name: "You".into(),
                },
                content: content.clone(),
                target_participant_ids: targets,
            },
        );

        for participant in participants {
            if let Some(session_id) = participant.session_id {
                self.handle_prompt(session_id, content.clone()).await;
            }
        }
    }

    async fn handle_list_sessions(&self) {
        let mut sessions = self
            .session_store
            .borrow()
            .list_transcripts()
            .into_iter()
            .filter(|transcript| {
                self.manager
                    .borrow()
                    .agent_for_session(&transcript.session_id)
                    .is_some()
            })
            .map(|transcript| self.build_session_summary(&transcript))
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.created_at_ms.cmp(&right.created_at_ms));

        let _ = self
            .response_tx
            .send(ResponseEvent::SessionList { sessions });
    }

    async fn handle_attach_session(&self, session_id: String, after_seq: Option<u64>) {
        let snapshot = match self
            .session_store
            .borrow()
            .get_transcript(&session_id)
            .cloned()
        {
            Some(transcript)
                if self
                    .manager
                    .borrow()
                    .agent_for_session(&session_id)
                    .is_some() =>
            {
                self.build_session_snapshot(&transcript)
            }
            _ => {
                let _ = self.response_tx.send(ResponseEvent::Error {
                    session_id: Some(session_id),
                    event_seq: None,
                    code: "session_not_found".into(),
                    message: "no live session with this id".into(),
                });
                return;
            }
        };

        let tail_seq = snapshot.last_event_seq;
        let replay_after = after_seq.unwrap_or(tail_seq);
        if replay_after > tail_seq {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: Some(session_id),
                event_seq: None,
                code: "replay_after_seq_ahead_of_tail".into(),
                message: format!(
                    "requested after_seq {} is ahead of current tail {}",
                    replay_after, tail_seq
                ),
            });
            return;
        }

        let replay_events = self
            .session_event_log
            .borrow()
            .replay_after(&session_id, replay_after);

        let _ = self.response_tx.send(ResponseEvent::SessionAttached {
            session_id: session_id.clone(),
        });
        let _ = self
            .response_tx
            .send(ResponseEvent::SessionSnapshot { snapshot });
        for event in replay_events {
            let _ = self.response_tx.send(event);
        }
        let _ = self.response_tx.send(ResponseEvent::SessionReplayComplete {
            session_id,
            last_event_seq: tail_seq,
        });
    }

    async fn handle_close_session(&mut self, session_id: String) {
        if self.active_prompt_sessions.borrow().contains(&session_id) {
            let event_seq = Some(self.session_event_log.borrow_mut().next_seq(&session_id));
            let error = ResponseEvent::Error {
                session_id: Some(session_id),
                event_seq,
                code: "session_busy".into(),
                message: "cannot close a session while a prompt is in progress".into(),
            };
            journal_and_broadcast_event(&self.session_event_log, &self.response_tx, error);
            return;
        }

        if self
            .manager
            .borrow()
            .agent_for_session(&session_id)
            .is_none()
        {
            let _ = self.response_tx.send(ResponseEvent::Error {
                session_id: Some(session_id),
                event_seq: None,
                code: "session_not_found".into(),
                message: "no live session with this id".into(),
            });
            return;
        }

        let _ = self.response_tx.send(ResponseEvent::SessionClosed {
            session_id: session_id.clone(),
        });

        if let Err(err) =
            flush_session_snapshot(self.session_store.clone(), session_id.clone()).await
        {
            warn!("failed to flush session {session_id} before close: {err}");
        }

        self.session_store.borrow_mut().remove_session(&session_id);
        self.session_event_log
            .borrow_mut()
            .remove_session(&session_id);
        self.manager.borrow_mut().remove_session(&session_id);
        self.created_sessions
            .retain(|created| created != &session_id);
    }

    async fn handle_prompt(&mut self, session_id: String, content: String) {
        if self.active_prompt_sessions.borrow().contains(&session_id) {
            let event_seq = Some(self.session_event_log.borrow_mut().next_seq(&session_id));
            let error = ResponseEvent::Error {
                session_id: Some(session_id),
                event_seq,
                code: "prompt_in_progress".into(),
                message: "another prompt is already running".into(),
            };
            journal_and_broadcast_event(&self.session_event_log, &self.response_tx, error);
            return;
        }

        let (agent_id, upstream_session_id, agent, is_alive) = {
            let mgr = self.manager.borrow();
            let binding = mgr.session_binding(&session_id).cloned();
            let agent_id = binding.as_ref().map(|binding| binding.agent_id.clone());
            let upstream_session_id = binding
                .as_ref()
                .map(|binding| binding.upstream_session_id.clone());
            let agent = agent_id.as_deref().and_then(|id| mgr.get_agent(id));
            let is_alive = agent_id
                .as_deref()
                .map(|id| mgr.is_agent_alive(id))
                .unwrap_or(false);
            (agent_id, upstream_session_id, agent, is_alive)
        };

        match (agent_id, upstream_session_id, agent, is_alive) {
            (Some(_), Some(_), Some(_), false) => {
                let event_seq = Some(self.session_event_log.borrow_mut().next_seq(&session_id));
                let error = ResponseEvent::Error {
                    session_id: Some(session_id),
                    event_seq,
                    code: "agent_crashed".into(),
                    message: "agent process exited".into(),
                };
                journal_and_broadcast_event(&self.session_event_log, &self.response_tx, error);
            }
            (Some(agent_id), Some(upstream_session_id), Some(agent), true) => {
                self.session_store
                    .borrow_mut()
                    .record_prompt(&session_id, &content);
                self.active_assistant_messages
                    .borrow_mut()
                    .remove(&session_id);
                let prompt_content =
                    maybe_inject_skill_context(self.skill_store.as_ref(), Some(&agent_id), content)
                        .await;

                self.active_prompt_sessions
                    .borrow_mut()
                    .insert(session_id.clone());
                let response_tx = self.response_tx.clone();
                let session_store = self.session_store.clone();
                let session_event_log = self.session_event_log.clone();
                let thread_store = self.thread_store.clone();
                let thread_event_log = self.thread_event_log.clone();
                let active_prompt_sessions = self.active_prompt_sessions.clone();
                let active_assistant_messages = self.active_assistant_messages.clone();
                tokio::task::spawn_local(async move {
                    let result = agent.prompt(upstream_session_id, prompt_content).await;
                    active_prompt_sessions.borrow_mut().remove(&session_id);

                    handle_prompt_completion(
                        response_tx,
                        session_store,
                        session_event_log,
                        thread_store,
                        thread_event_log,
                        active_assistant_messages,
                        session_id,
                        result,
                    )
                    .await;
                });
            }
            _ => {
                let event_seq = Some(self.session_event_log.borrow_mut().next_seq(&session_id));
                let error = ResponseEvent::Error {
                    session_id: Some(session_id),
                    event_seq,
                    code: "no_agent".into(),
                    message: "no agent for this session".into(),
                };
                journal_and_broadcast_event(&self.session_event_log, &self.response_tx, error);
            }
        }
    }

    async fn handle_cancel(&self, session_id: String) {
        let (agent, upstream_session_id) = {
            let mgr = self.manager.borrow();
            let binding = mgr.session_binding(&session_id).cloned();
            let agent = binding
                .as_ref()
                .and_then(|binding| mgr.get_agent(&binding.agent_id));
            let upstream_session_id = binding.map(|binding| binding.upstream_session_id);
            (agent, upstream_session_id)
        };

        match (agent, upstream_session_id) {
            (Some(agent), Some(upstream_session_id)) => {
                if let Err(err) = agent.cancel(upstream_session_id).await {
                    warn!("cancel failed for session {session_id}: {err}");
                }
            }
            _ => {
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
                    event_seq: None,
                    code: "skill_not_found".into(),
                    message: err,
                });
            }
        }
    }

    fn build_session_summary(&self, transcript: &SessionTranscript) -> SessionSummary {
        SessionSummary {
            session_id: transcript.session_id.clone(),
            agent_id: transcript.agent_id.clone(),
            working_dir: transcript.working_dir.clone(),
            created_at_ms: transcript.created_at_ms,
            state: self.session_state(&transcript.session_id),
            last_event_seq: self
                .session_event_log
                .borrow()
                .tail_seq(&transcript.session_id),
            last_stop_reason: last_stop_reason(transcript),
        }
    }

    fn build_session_snapshot(&self, transcript: &SessionTranscript) -> SessionSnapshot {
        SessionSnapshot {
            session_id: transcript.session_id.clone(),
            agent_id: transcript.agent_id.clone(),
            working_dir: transcript.working_dir.clone(),
            created_at_ms: transcript.created_at_ms,
            state: self.session_state(&transcript.session_id),
            last_event_seq: self
                .session_event_log
                .borrow()
                .tail_seq(&transcript.session_id),
            last_stop_reason: last_stop_reason(transcript),
            last_error: None,
        }
    }

    fn build_thread_participant(
        &self,
        participant: &agentchat_core::thread_store::ThreadParticipantRecord,
    ) -> ThreadParticipant {
        let state = match participant.session_id.as_deref() {
            Some(session_id) if self.active_prompt_sessions.borrow().contains(session_id) => {
                ParticipantState::Prompting
            }
            Some(session_id) => match participant.agent_id.as_deref() {
                Some(agent_id) if !self.manager.borrow().is_agent_alive(agent_id) => {
                    ParticipantState::Offline
                }
                Some(_) if self.manager.borrow().session_binding(session_id).is_none() => {
                    ParticipantState::Error
                }
                _ => ParticipantState::Idle,
            },
            None => ParticipantState::Idle,
        };
        participant_to_protocol(participant, state)
    }

    fn build_thread_snapshot(
        &self,
        thread: &agentchat_core::thread_store::ThreadRecord,
    ) -> ThreadSnapshot {
        let participants = thread
            .participants
            .iter()
            .map(|participant| self.build_thread_participant(participant))
            .collect::<Vec<_>>();
        thread_to_snapshot(
            thread,
            participants,
            self.thread_event_log.borrow().tail_seq(&thread.thread_id),
        )
    }

    fn thread_state(&self, thread_id: &str) -> ThreadState {
        let has_active = self
            .thread_store
            .borrow()
            .get_thread(thread_id)
            .map(|thread| {
                thread.participants.iter().any(|participant| {
                    participant
                        .session_id
                        .as_deref()
                        .map(|session_id| self.active_prompt_sessions.borrow().contains(session_id))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_active {
            ThreadState::Prompting
        } else {
            ThreadState::Idle
        }
    }

    fn session_state(&self, session_id: &str) -> SessionState {
        if self.active_prompt_sessions.borrow().contains(session_id) {
            SessionState::Prompting
        } else {
            SessionState::Idle
        }
    }
}

fn last_stop_reason(transcript: &SessionTranscript) -> Option<String> {
    transcript
        .events
        .iter()
        .rev()
        .find_map(|event| match event {
            SessionEvent::TurnEnd { stop_reason, .. } => Some(stop_reason.clone()),
            _ => None,
        })
}

fn rewrite_notification_session_id(
    notification: &AgentNotification,
    public_session_id: &str,
) -> AgentNotification {
    notification.with_session_id(public_session_id.to_string())
}

fn maybe_broadcast_thread_event_for_session_event(
    thread_store: &Rc<RefCell<ThreadStore>>,
    thread_event_log: &Rc<RefCell<ThreadEventLog>>,
    response_tx: &broadcast::Sender<ResponseEvent>,
    active_assistant_messages: &Rc<RefCell<HashMap<String, ActiveAssistantMessage>>>,
    event: &ResponseEvent,
) {
    let Some(session_id) = event.session_id() else {
        return;
    };
    let Some(binding) = thread_store
        .borrow()
        .binding_for_session(session_id)
        .cloned()
    else {
        return;
    };
    let Some(session_event_seq) = event.event_seq() else {
        return;
    };

    match event {
        ResponseEvent::Delta {
            session_id,
            content,
            delta_type,
            ..
        } => match delta_type {
            DeltaType::Thinking | DeltaType::Text if !content.is_empty() => {
                let thread_seq = thread_event_log.borrow_mut().next_seq(&binding.thread_id);
                let mut active_messages = active_assistant_messages.borrow_mut();
                let message = active_messages
                    .entry(session_id.clone())
                    .or_insert_with(ActiveAssistantMessage::new);
                match delta_type {
                    DeltaType::Thinking => message.thinking.push_str(content),
                    DeltaType::Text => message.response.push_str(content),
                    DeltaType::ToolUse => {}
                }
                let thread_event = ResponseEvent::ThreadAssistantMessage {
                    thread_id: binding.thread_id,
                    thread_seq,
                    message_id: message.message_id.clone(),
                    turn_id: message.turn_id.clone(),
                    participant_id: binding.participant_id,
                    agent_id: binding.agent_id,
                    session_id: session_id.clone(),
                    session_event_seq,
                    thinking: message.thinking.clone(),
                    response: message.response.clone(),
                    state: AssistantMessageState::Streaming,
                    stop_reason: None,
                };
                drop(active_messages);
                journal_and_broadcast_thread_event(thread_event_log, response_tx, thread_event);
            }
            _ => {}
        },
        ResponseEvent::PlanUpdate {
            session_id,
            plan_json,
            ..
        } => {
            let thread_seq = thread_event_log.borrow_mut().next_seq(&binding.thread_id);
            journal_and_broadcast_thread_event(
                thread_event_log,
                response_tx,
                ResponseEvent::ThreadAgentPlanUpdate {
                    thread_id: binding.thread_id,
                    thread_seq,
                    participant_id: binding.participant_id,
                    agent_id: binding.agent_id,
                    session_id: session_id.clone(),
                    session_event_seq,
                    plan_json: plan_json.clone(),
                },
            );
        }
        ResponseEvent::ToolUpdate {
            session_id,
            tool_call_id,
            title,
            status,
            content,
            ..
        } => {
            let thread_seq = thread_event_log.borrow_mut().next_seq(&binding.thread_id);
            journal_and_broadcast_thread_event(
                thread_event_log,
                response_tx,
                ResponseEvent::ThreadAgentToolUpdate {
                    thread_id: binding.thread_id,
                    thread_seq,
                    participant_id: binding.participant_id,
                    agent_id: binding.agent_id,
                    session_id: session_id.clone(),
                    session_event_seq,
                    tool_call_id: tool_call_id.clone(),
                    title: title.clone(),
                    status: status.clone(),
                    content: content.clone(),
                },
            );
        }
        ResponseEvent::TurnEnd {
            session_id,
            stop_reason,
            ..
        } => finalize_active_assistant_message(
            thread_store,
            thread_event_log,
            response_tx,
            active_assistant_messages,
            session_id,
            session_event_seq,
            AssistantMessageState::Completed,
            Some(stop_reason.clone()),
        ),
        _ => {}
    }
}

fn finalize_active_assistant_message(
    thread_store: &Rc<RefCell<ThreadStore>>,
    thread_event_log: &Rc<RefCell<ThreadEventLog>>,
    response_tx: &broadcast::Sender<ResponseEvent>,
    active_assistant_messages: &Rc<RefCell<HashMap<String, ActiveAssistantMessage>>>,
    session_id: &str,
    session_event_seq: u64,
    state: AssistantMessageState,
    stop_reason: Option<String>,
) {
    let Some(binding) = thread_store
        .borrow()
        .binding_for_session(session_id)
        .cloned()
    else {
        active_assistant_messages.borrow_mut().remove(session_id);
        return;
    };

    let Some(message) = active_assistant_messages.borrow_mut().remove(session_id) else {
        return;
    };

    let thread_seq = thread_event_log.borrow_mut().next_seq(&binding.thread_id);
    journal_and_broadcast_thread_event(
        thread_event_log,
        response_tx,
        ResponseEvent::ThreadAssistantMessage {
            thread_id: binding.thread_id,
            thread_seq,
            message_id: message.message_id,
            turn_id: message.turn_id,
            participant_id: binding.participant_id,
            agent_id: binding.agent_id,
            session_id: session_id.to_string(),
            session_event_seq,
            thinking: message.thinking,
            response: message.response,
            state,
            stop_reason,
        },
    );
}

fn journal_and_broadcast_event(
    session_event_log: &Rc<RefCell<SessionEventLog>>,
    response_tx: &broadcast::Sender<ResponseEvent>,
    event: ResponseEvent,
) {
    persist_session_event(session_event_log, &event);
    session_event_log.borrow_mut().append(event.clone());
    let _ = response_tx.send(event);
}

fn journal_and_broadcast_thread_event(
    thread_event_log: &Rc<RefCell<ThreadEventLog>>,
    response_tx: &broadcast::Sender<ResponseEvent>,
    event: ResponseEvent,
) {
    persist_thread_event(thread_event_log, &event);
    thread_event_log.borrow_mut().append(event.clone());
    let _ = response_tx.send(event);
}

fn persist_session_event(session_event_log: &Rc<RefCell<SessionEventLog>>, event: &ResponseEvent) {
    let Some(session_id) = event.session_id().map(|id| id.to_string()) else {
        return;
    };

    let Some(json) = serialize_event(event) else {
        return;
    };

    let path = session_event_log.borrow().event_log_path(&session_id);
    tokio::task::spawn_local(async move {
        if let Some(parent) = path.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                warn!(
                    "failed to create session event log dir for {}: {}",
                    session_id, err
                );
                return;
            }
        }

        let line = format!("{json}\n");
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(err) => {
                warn!(
                    "failed to open session event log for {}: {}",
                    session_id, err
                );
                return;
            }
        };

        use tokio::io::AsyncWriteExt as _;
        if let Err(err) = file.write_all(line.as_bytes()).await {
            warn!(
                "failed to append session event log for {}: {}",
                session_id, err
            );
        }
    });
}

fn persist_thread_event(thread_event_log: &Rc<RefCell<ThreadEventLog>>, event: &ResponseEvent) {
    let Some(thread_id) = event.thread_id().map(|id| id.to_string()) else {
        return;
    };

    let Some(json) = serialize_event(event) else {
        return;
    };

    let path = thread_event_log.borrow().event_log_path(&thread_id);
    tokio::task::spawn_local(async move {
        if let Some(parent) = path.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                warn!(
                    "failed to create thread event log dir for {}: {}",
                    thread_id, err
                );
                return;
            }
        }

        let line = format!("{json}\n");
        let mut file = match tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(err) => {
                warn!("failed to open thread event log for {}: {}", thread_id, err);
                return;
            }
        };

        use tokio::io::AsyncWriteExt as _;
        if let Err(err) = file.write_all(line.as_bytes()).await {
            warn!(
                "failed to append thread event log for {}: {}",
                thread_id, err
            );
        }
    });
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
    response_tx: broadcast::Sender<ResponseEvent>,
    session_store: Rc<RefCell<SessionStore>>,
    session_event_log: Rc<RefCell<SessionEventLog>>,
    thread_store: Rc<RefCell<ThreadStore>>,
    thread_event_log: Rc<RefCell<ThreadEventLog>>,
    active_assistant_messages: Rc<RefCell<HashMap<String, ActiveAssistantMessage>>>,
    session_id: String,
    result: Result<AgentPromptResult, String>,
) {
    match result {
        Ok(resp) => {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;

            let stop_reason = resp.stop_reason;
            let event_seq = session_event_log.borrow_mut().next_seq(&session_id);
            let event = ResponseEvent::TurnEnd {
                session_id: session_id.clone(),
                event_seq,
                stop_reason: stop_reason.clone(),
            };
            journal_and_broadcast_event(&session_event_log, &response_tx, event.clone());
            maybe_broadcast_thread_event_for_session_event(
                &thread_store,
                &thread_event_log,
                &response_tx,
                &active_assistant_messages,
                &event,
            );
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
            let event_seq = session_event_log.borrow_mut().next_seq(&session_id);
            finalize_active_assistant_message(
                &thread_store,
                &thread_event_log,
                &response_tx,
                &active_assistant_messages,
                &session_id,
                event_seq,
                AssistantMessageState::Failed,
                None,
            );
            journal_and_broadcast_event(
                &session_event_log,
                &response_tx,
                ResponseEvent::Error {
                    session_id: Some(session_id),
                    event_seq: Some(event_seq),
                    code: "prompt_failed".into(),
                    message: err.to_string(),
                },
            );
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
    response_tx: broadcast::Sender<ResponseEvent>,
    manager: Rc<RefCell<AgentManager>>,
    session_store: Rc<RefCell<SessionStore>>,
    session_event_log: Rc<RefCell<SessionEventLog>>,
    internal_sessions: Rc<RefCell<HashMap<String, mpsc::UnboundedSender<AgentNotification>>>>,
    distiller: Rc<Distiller>,
    session_id: String,
) {
    tokio::task::spawn_local(async move {
        let transcript = match load_transcript(session_store.clone(), &session_id).await {
            Ok(transcript) => transcript,
            Err(err) => {
                send_distillation_status(
                    &session_event_log,
                    &response_tx,
                    &session_id,
                    "failed",
                    err,
                );
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
                    &session_event_log,
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
                    &session_event_log,
                    &response_tx,
                    &session_id,
                    "failed",
                    "no agent for this session",
                );
                return;
            }
        };

        let distill_session_id = match agent
            .new_session(PathBuf::from(&transcript.working_dir))
            .await
        {
            Ok(session_id) => session_id,
            Err(err) => {
                send_distillation_status(
                    &session_event_log,
                    &response_tx,
                    &session_id,
                    "failed",
                    format!("failed to create distillation session: {err}"),
                );
                return;
            }
        };

        let (distill_tx, distill_rx) = mpsc::unbounded_channel();
        internal_sessions
            .borrow_mut()
            .insert(distill_session_id.clone(), distill_tx);

        send_distillation_status(
            &session_event_log,
            &response_tx,
            &session_id,
            "started",
            "distillation started",
        );

        let result = distiller
            .distill(agent, distill_session_id.clone(), transcript, distill_rx)
            .await;

        internal_sessions.borrow_mut().remove(&distill_session_id);

        match result {
            Ok(skills) => {
                send_distillation_status(
                    &session_event_log,
                    &response_tx,
                    &session_id,
                    "completed",
                    format!("Updated {} skills", skills.len()),
                );
            }
            Err(err) => {
                send_distillation_status(
                    &session_event_log,
                    &response_tx,
                    &session_id,
                    "failed",
                    err,
                );
            }
        }
    });
}

fn send_distillation_status(
    session_event_log: &Rc<RefCell<SessionEventLog>>,
    response_tx: &broadcast::Sender<ResponseEvent>,
    session_id: &str,
    status: &str,
    message: impl Into<String>,
) {
    let event_seq = session_event_log.borrow_mut().next_seq(session_id);
    journal_and_broadcast_event(
        session_event_log,
        response_tx,
        ResponseEvent::DistillationStatus {
            session_id: session_id.to_string(),
            event_seq,
            status: status.to_string(),
            message: message.into(),
        },
    );
}

fn map_session_update(notification: &AgentNotification, event_seq: u64) -> ResponseEvent {
    let sid = notification.session_id.clone();

    match &notification.update {
        AgentUpdate::TextDelta { content } => ResponseEvent::Delta {
            session_id: sid,
            event_seq,
            content: content.clone(),
            delta_type: DeltaType::Text,
        },
        AgentUpdate::ThinkingDelta { content } => ResponseEvent::Delta {
            session_id: sid,
            event_seq,
            content: content.clone(),
            delta_type: DeltaType::Thinking,
        },
        AgentUpdate::ToolUpdate {
            tool_call_id,
            title,
            status,
            content,
        } => ResponseEvent::ToolUpdate {
            session_id: sid,
            event_seq,
            tool_call_id: tool_call_id.clone(),
            title: title.clone(),
            status: status.clone(),
            content: content.clone(),
        },
        AgentUpdate::Plan { plan_json } => ResponseEvent::PlanUpdate {
            session_id: sid,
            event_seq,
            plan_json: plan_json.clone(),
        },
        AgentUpdate::Raw { .. } => ResponseEvent::Delta {
            session_id: sid,
            event_seq,
            content: String::new(),
            delta_type: DeltaType::Text,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_session_update_maps_agent_message_chunk_to_text_delta() {
        let notification = AgentNotification::new(
            "session-1",
            AgentUpdate::TextDelta {
                content: "hello".into(),
            },
        );

        assert_eq!(
            map_session_update(&notification, 1),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                event_seq: 1,
                content: "hello".into(),
                delta_type: DeltaType::Text,
            }
        );
    }

    #[test]
    fn map_session_update_maps_agent_thought_chunk_to_thinking_delta() {
        let notification = AgentNotification::new(
            "session-1",
            AgentUpdate::ThinkingDelta {
                content: "thinking".into(),
            },
        );

        assert_eq!(
            map_session_update(&notification, 2),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                event_seq: 2,
                content: "thinking".into(),
                delta_type: DeltaType::Thinking,
            }
        );
    }

    #[test]
    fn map_session_update_maps_tool_call_to_tool_update() {
        let notification = AgentNotification::new(
            "session-1",
            AgentUpdate::ToolUpdate {
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "InProgress".into(),
                content: None,
            },
        );

        assert_eq!(
            map_session_update(&notification, 3),
            ResponseEvent::ToolUpdate {
                session_id: "session-1".into(),
                event_seq: 3,
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "InProgress".into(),
                content: None,
            }
        );
    }

    #[test]
    fn map_session_update_maps_unknown_variant_to_empty_delta() {
        let notification = AgentNotification::new(
            "session-1",
            AgentUpdate::Raw {
                payload: serde_json::json!({"kind": "unknown"}),
            },
        );

        assert_eq!(
            map_session_update(&notification, 4),
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                event_seq: 4,
                content: String::new(),
                delta_type: DeltaType::Text,
            }
        );
    }
}
