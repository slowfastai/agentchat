use std::cell::RefCell;
use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use agent_client_protocol::SessionNotification;
use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{
    AgentConfig, AgentStatus, AssistantMessageState, ClientMessage, DeltaType, ResponseEvent,
    SessionEvent, SessionState, SessionTranscript,
};
use agentchat_server::ws::WebSocketServer;
use futures::{SinkExt, StreamExt};
use tempfile::TempDir;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(25);

type TestWebSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Clone, Copy)]
enum FakeAgentMode {
    Normal,
    WaitForCancel,
    ExitAfterSession,
}

impl FakeAgentMode {
    fn as_env_value(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::WaitForCancel => "wait_for_cancel",
            Self::ExitAfterSession => "exit_after_session",
        }
    }
}

struct TestHarness {
    manager: Rc<RefCell<AgentManager>>,
    shutdown_tx: watch::Sender<bool>,
    server_task: JoinHandle<Result<(), String>>,
    events_path: PathBuf,
    project_root: PathBuf,
    port: u16,
    _temp_dir: TempDir,
}

impl TestHarness {
    async fn finish(self) {
        let _ = self.shutdown_tx.send(true);
        let result = self.server_task.await.expect("server task panicked");
        assert!(result.is_ok(), "server returned error: {result:?}");

        let shutdown = { self.manager.borrow().shutdown_all() };
        shutdown.await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_round_trip_streams_prompt_events_and_survives_reconnect() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "say hello".into(),
                },
            )
            .await;

            let events = collect_prompt_events(&mut ws, &session_id).await;
            assert!(events.iter().any(|event| matches!(
                event,
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Text,
                    content,
                    ..
                } if sid == &session_id && content == "echo: say hello"
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Thinking,
                    content,
                    ..
                } if sid == &session_id && content == "thinking about the request"
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                ResponseEvent::ToolUpdate {
                    session_id: sid,
                    tool_call_id,
                    title,
                    status,
                    ..
                } if sid == &session_id && tool_call_id == "tool-1" && title == "Demo Tool" && status == "InProgress"
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                ResponseEvent::TurnEnd {
                    session_id: sid,
                    stop_reason,
                    ..
                } if sid == &session_id && stop_reason == "EndTurn"
            )));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                Some("fake")
            );
            let upstream_session_id = harness
                .manager
                .borrow()
                .upstream_session_for_session(&session_id)
                .unwrap()
                .to_string();
            assert_ne!(upstream_session_id, session_id);

            let mut ws = connect_ws(harness.port).await;
            send_client_message(&mut ws, &ClientMessage::ListSessions).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionList { sessions } => {
                    assert!(sessions.iter().any(|session| {
                        session.session_id == session_id
                            && session.agent_id == "fake"
                            && session.working_dir == "."
                    }));
                }
                event => panic!("unexpected event while listing sessions: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::AttachSession {
                    session_id: session_id.clone(),
                    after_seq: None,
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionAttached { session_id: sid } => {
                    assert_eq!(sid, session_id);
                }
                event => panic!("unexpected event while attaching session: {event:?}"),
            }
            let last_event_seq = match receive_event(&mut ws).await {
                ResponseEvent::SessionSnapshot { snapshot } => {
                    assert_eq!(snapshot.session_id, session_id);
                    assert_eq!(snapshot.agent_id, "fake");
                    assert_eq!(snapshot.working_dir, ".");
                    assert_eq!(snapshot.state, SessionState::Idle);
                    assert!(snapshot.last_event_seq > 0);
                    snapshot.last_event_seq
                }
                event => panic!("unexpected event while reading session snapshot: {event:?}"),
            };
            match receive_event(&mut ws).await {
                ResponseEvent::SessionReplayComplete {
                    session_id: sid,
                    last_event_seq: replayed_through,
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(replayed_through, last_event_seq);
                }
                event => panic!("unexpected replay completion event: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "say hello again".into(),
                },
            )
            .await;

            let reconnect_events = collect_prompt_events(&mut ws, &session_id).await;
            assert!(reconnect_events.iter().any(|event| matches!(
                event,
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Text,
                    content,
                    ..
                } if sid == &session_id && content == "echo: say hello again"
            )));
            assert!(reconnect_events.iter().any(|event| matches!(
                event,
                ResponseEvent::TurnEnd {
                    session_id: sid,
                    stop_reason,
                    ..
                } if sid == &session_id && stop_reason == "EndTurn"
            )));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_lists_agents_and_creates_sessions_for_requested_agent() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[
                ("alpha", FakeAgentMode::Normal),
                ("beta", FakeAgentMode::Normal),
            ])
            .await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(&mut ws, &ClientMessage::ListAgents).await;
            match receive_event(&mut ws).await {
                ResponseEvent::AgentList { agents } => {
                    assert_eq!(agents.len(), 2);
                    assert_eq!(agents[0].agent_id, "alpha");
                    assert_eq!(agents[0].status, AgentStatus::Online);
                    assert_eq!(agents[1].agent_id, "beta");
                    assert_eq!(agents[1].status, AgentStatus::Online);
                }
                event => panic!("unexpected event while listing agents: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: Some("beta".into()),
                    working_dir: ".".into(),
                },
            )
            .await;

            let session_id = match receive_event(&mut ws).await {
                ResponseEvent::SessionCreated {
                    session_id,
                    agent_id,
                    event_seq,
                } => {
                    assert_eq!(agent_id, "beta");
                    assert!(event_seq > 0);
                    session_id
                }
                event => panic!("expected session_created event, got {event:?}"),
            };

            send_client_message(&mut ws, &ClientMessage::ListSessions).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionList { sessions } => {
                    assert!(sessions.iter().any(|session| {
                        session.session_id == session_id && session.agent_id == "beta"
                    }));
                }
                event => panic!("unexpected event while listing sessions: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_allows_concurrent_prompts_for_different_sessions() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[
                ("alpha", FakeAgentMode::WaitForCancel),
                ("beta", FakeAgentMode::WaitForCancel),
            ])
            .await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: Some("alpha".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let alpha_session = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: Some("beta".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let beta_session = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: alpha_session.clone(),
                    content: "wait alpha".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::Delta {
                    session_id,
                    content,
                    ..
                } => {
                    assert_eq!(session_id, alpha_session);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event from first concurrent prompt: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: beta_session.clone(),
                    content: "wait beta".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::Delta {
                    session_id,
                    content,
                    ..
                } => {
                    assert_eq!(session_id, beta_session);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event from second concurrent prompt: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: alpha_session.clone(),
                    content: "should fail".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: Some(session_id),
                    code,
                    ..
                } => {
                    assert_eq!(session_id, alpha_session);
                    assert_eq!(code, "prompt_in_progress");
                }
                event => panic!("unexpected event for same-session overlap: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::Cancel {
                    session_id: alpha_session.clone(),
                },
            )
            .await;
            send_client_message(
                &mut ws,
                &ClientMessage::Cancel {
                    session_id: beta_session.clone(),
                },
            )
            .await;

            let mut saw_alpha_end = false;
            let mut saw_beta_end = false;
            for _ in 0..4 {
                match receive_event(&mut ws).await {
                    ResponseEvent::TurnEnd {
                        session_id,
                        stop_reason,
                        ..
                    } if session_id == alpha_session => {
                        assert_eq!(stop_reason, "Cancelled");
                        saw_alpha_end = true;
                    }
                    ResponseEvent::TurnEnd {
                        session_id,
                        stop_reason,
                        ..
                    } if session_id == beta_session => {
                        assert_eq!(stop_reason, "Cancelled");
                        saw_beta_end = true;
                    }
                    other => {
                        panic!("unexpected event while draining concurrent cancels: {other:?}")
                    }
                }
                if saw_alpha_end && saw_beta_end {
                    break;
                }
            }
            assert!(saw_alpha_end && saw_beta_end);

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_thread_group_chat_fans_out_to_multiple_agents() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[
                ("alpha", FakeAgentMode::Normal),
                ("beta", FakeAgentMode::Normal),
            ])
            .await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Review".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "alpha".into(),
                },
            )
            .await;
            let (alpha_participant_id, alpha_session_id) = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded {
                    thread_id: tid,
                    participant,
                    ..
                } => {
                    assert_eq!(tid, thread_id);
                    assert_eq!(participant.agent_id.as_deref(), Some("alpha"));
                    (
                        participant.participant_id,
                        participant
                            .session_id
                            .expect("missing participant session id"),
                    )
                }
                event => panic!("unexpected event while adding alpha participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "beta".into(),
                },
            )
            .await;
            let (beta_participant_id, beta_session_id) = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded {
                    thread_id: tid,
                    participant,
                    ..
                } => {
                    assert_eq!(tid, thread_id);
                    assert_eq!(participant.agent_id.as_deref(), Some("beta"));
                    (
                        participant.participant_id,
                        participant
                            .session_id
                            .expect("missing participant session id"),
                    )
                }
                event => panic!("unexpected event while adding beta participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::SendThreadMessage {
                    thread_id: thread_id.clone(),
                    content: "review this".into(),
                    target_participant_ids: None,
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::ThreadMessage {
                    thread_id: tid,
                    target_participant_ids,
                    content,
                    ..
                } => {
                    assert_eq!(tid, thread_id);
                    assert_eq!(content, "review this");
                    assert_eq!(target_participant_ids.len(), 2);
                    assert!(target_participant_ids.contains(&alpha_participant_id));
                    assert!(target_participant_ids.contains(&beta_participant_id));
                }
                event => panic!("unexpected thread message event: {event:?}"),
            }

            let mut saw_alpha_text = false;
            let mut saw_beta_text = false;
            let mut saw_alpha_end = false;
            let mut saw_beta_end = false;
            for _ in 0..16 {
                match receive_event(&mut ws).await {
                    ResponseEvent::ThreadAssistantMessage {
                        thread_id: tid,
                        participant_id,
                        session_id,
                        response,
                        state,
                        stop_reason,
                        ..
                    } => {
                        assert_eq!(tid, thread_id);
                        if participant_id == alpha_participant_id {
                            assert_eq!(session_id, alpha_session_id);
                            if response == "echo: review this" {
                                saw_alpha_text = true;
                            }
                            if state == AssistantMessageState::Completed {
                                assert_eq!(stop_reason.as_deref(), Some("EndTurn"));
                                saw_alpha_end = true;
                            }
                        } else if participant_id == beta_participant_id {
                            assert_eq!(session_id, beta_session_id);
                            if response == "echo: review this" {
                                saw_beta_text = true;
                            }
                            if state == AssistantMessageState::Completed {
                                assert_eq!(stop_reason.as_deref(), Some("EndTurn"));
                                saw_beta_end = true;
                            }
                        }
                    }
                    ResponseEvent::ThreadAgentToolUpdate { .. }
                    | ResponseEvent::Delta { .. }
                    | ResponseEvent::ToolUpdate { .. }
                    | ResponseEvent::TurnEnd { .. } => {}
                    other => panic!("unexpected thread fan-out event: {other:?}"),
                }
                if saw_alpha_text && saw_beta_text && saw_alpha_end && saw_beta_end {
                    break;
                }
            }

            assert!(saw_alpha_text && saw_beta_text && saw_alpha_end && saw_beta_end);

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_thread_targeted_send_and_attach_snapshot() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[
                ("alpha", FakeAgentMode::Normal),
                ("beta", FakeAgentMode::Normal),
            ])
            .await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Targeted".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "alpha".into(),
                },
            )
            .await;
            let (alpha_participant_id, alpha_session_id) = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => (
                    participant.participant_id,
                    participant.session_id.expect("missing alpha session id"),
                ),
                event => panic!("unexpected event while adding alpha participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "beta".into(),
                },
            )
            .await;
            let (beta_participant_id, beta_session_id) = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => (
                    participant.participant_id,
                    participant.session_id.expect("missing beta session id"),
                ),
                event => panic!("unexpected event while adding beta participant: {event:?}"),
            };

            send_client_message(&mut ws, &ClientMessage::ListThreads).await;
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadList { threads } => {
                    let thread = threads
                        .iter()
                        .find(|thread| thread.thread_id == thread_id)
                        .expect("missing created thread");
                    assert_eq!(thread.participant_count, 3);
                    assert_eq!(thread.state, agentchat_protocol::ThreadState::Idle);
                }
                event => panic!("unexpected event while listing threads: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::SendThreadMessage {
                    thread_id: thread_id.clone(),
                    content: "only beta".into(),
                    target_participant_ids: Some(vec![beta_participant_id.clone()]),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::ThreadMessage {
                    target_participant_ids,
                    content,
                    ..
                } => {
                    assert_eq!(content, "only beta");
                    assert_eq!(target_participant_ids, vec![beta_participant_id.clone()]);
                }
                event => panic!("unexpected targeted thread message event: {event:?}"),
            }

            let mut saw_beta_text = false;
            let mut saw_beta_end = false;
            for _ in 0..10 {
                match receive_event(&mut ws).await {
                    ResponseEvent::ThreadAssistantMessage {
                        participant_id,
                        session_id,
                        response,
                        state,
                        stop_reason,
                        ..
                    } => {
                        assert_eq!(participant_id, beta_participant_id);
                        assert_eq!(session_id, beta_session_id);
                        if response == "echo: only beta" {
                            saw_beta_text = true;
                        }
                        if state == AssistantMessageState::Completed {
                            assert_eq!(stop_reason.as_deref(), Some("EndTurn"));
                            saw_beta_end = true;
                        }
                    }
                    ResponseEvent::ThreadAgentToolUpdate { participant_id, .. } => {
                        assert_ne!(participant_id, alpha_participant_id);
                    }
                    ResponseEvent::Delta { .. }
                    | ResponseEvent::ToolUpdate { .. }
                    | ResponseEvent::TurnEnd { .. } => {}
                    other => panic!("unexpected targeted thread event: {other:?}"),
                }
                if saw_beta_text && saw_beta_end {
                    break;
                }
            }
            assert!(saw_beta_text && saw_beta_end);

            send_client_message(
                &mut ws,
                &ClientMessage::AttachThread {
                    thread_id: thread_id.clone(),
                    after_seq: None,
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadAttached { thread_id: tid } => assert_eq!(tid, thread_id),
                event => panic!("unexpected thread attached event: {event:?}"),
            }
            let last_thread_seq = match receive_event(&mut ws).await {
                ResponseEvent::ThreadSnapshot { snapshot } => {
                    assert_eq!(snapshot.thread_id, thread_id);
                    assert!(snapshot.last_thread_seq > 0);
                    assert!(snapshot
                        .participants
                        .iter()
                        .any(|participant| participant.session_id.as_deref()
                            == Some(alpha_session_id.as_str())));
                    assert!(snapshot
                        .participants
                        .iter()
                        .any(|participant| participant.session_id.as_deref()
                            == Some(beta_session_id.as_str())));
                    snapshot.last_thread_seq
                }
                event => panic!("unexpected thread snapshot event: {event:?}"),
            };
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadReplayComplete {
                    thread_id: tid,
                    last_thread_seq: replayed_through,
                } => {
                    assert_eq!(tid, thread_id);
                    assert_eq!(replayed_through, last_thread_seq);
                }
                event => panic!("unexpected thread replay completion event: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_attach_thread_replays_events_after_cursor() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[
                ("alpha", FakeAgentMode::Normal),
                ("beta", FakeAgentMode::Normal),
            ])
            .await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Replay".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "alpha".into(),
                },
            )
            .await;
            let alpha_participant_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => {
                    participant.participant_id
                }
                event => panic!("unexpected event while adding alpha participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "beta".into(),
                },
            )
            .await;
            let beta_participant_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => {
                    participant.participant_id
                }
                event => panic!("unexpected event while adding beta participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::SendThreadMessage {
                    thread_id: thread_id.clone(),
                    content: "replay thread".into(),
                    target_participant_ids: None,
                },
            )
            .await;

            let mut thread_events = Vec::new();
            let mut saw_alpha_end = false;
            let mut saw_beta_end = false;
            for _ in 0..24 {
                let event = receive_event(&mut ws).await;
                match &event {
                    ResponseEvent::ThreadMessage { .. }
                    | ResponseEvent::ThreadParticipantAdded { .. }
                    | ResponseEvent::ThreadParticipantRemoved { .. }
                    | ResponseEvent::ThreadAssistantMessage { .. }
                    | ResponseEvent::ThreadAgentPlanUpdate { .. }
                    | ResponseEvent::ThreadAgentToolUpdate { .. } => {
                        thread_events.push(event.clone());
                    }
                    _ => {}
                }

                if let ResponseEvent::ThreadAssistantMessage {
                    participant_id,
                    state,
                    ..
                } = &event
                {
                    if *state == AssistantMessageState::Completed {
                        if participant_id == &alpha_participant_id {
                            saw_alpha_end = true;
                        } else if participant_id == &beta_participant_id {
                            saw_beta_end = true;
                        }
                    }
                }
                if saw_alpha_end && saw_beta_end {
                    break;
                }
            }

            assert!(thread_events.len() >= 3);
            let after_seq = thread_events
                .iter()
                .find_map(ResponseEvent::thread_seq)
                .expect("expected at least one thread event with thread_seq");
            let expected_replay = thread_events
                .iter()
                .filter(|event| event.thread_seq().unwrap_or(0) > after_seq)
                .cloned()
                .collect::<Vec<_>>();
            let expected_tail = thread_events
                .iter()
                .filter_map(ResponseEvent::thread_seq)
                .max()
                .expect("expected thread tail seq");

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            let mut ws = connect_ws(harness.port).await;
            send_client_message(
                &mut ws,
                &ClientMessage::AttachThread {
                    thread_id: thread_id.clone(),
                    after_seq: Some(after_seq),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::ThreadAttached { thread_id: tid } => assert_eq!(tid, thread_id),
                event => panic!("unexpected thread attached event: {event:?}"),
            }
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadSnapshot { snapshot } => {
                    assert_eq!(snapshot.thread_id, thread_id);
                    assert_eq!(snapshot.last_thread_seq, expected_tail);
                }
                event => panic!("unexpected thread snapshot event: {event:?}"),
            }

            let mut replayed_events = Vec::new();
            for _ in 0..expected_replay.len() {
                replayed_events.push(receive_event(&mut ws).await);
            }
            assert_eq!(replayed_events, expected_replay);

            match receive_event(&mut ws).await {
                ResponseEvent::ThreadReplayComplete {
                    thread_id: tid,
                    last_thread_seq,
                } => {
                    assert_eq!(tid, thread_id);
                    assert_eq!(last_thread_seq, expected_tail);
                }
                event => panic!("unexpected thread replay completion event: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_close_thread_removes_it_from_thread_list() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[("alpha", FakeAgentMode::Normal)]).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Closable".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "alpha".into(),
                },
            )
            .await;
            let session_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => participant
                    .session_id
                    .expect("missing participant session id"),
                event => panic!("unexpected event while adding thread participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::CloseThread {
                    thread_id: thread_id.clone(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadClosed { thread_id: tid } => assert_eq!(tid, thread_id),
                event => panic!("unexpected event while closing thread: {event:?}"),
            }

            send_client_message(&mut ws, &ClientMessage::ListThreads).await;
            match receive_event(&mut ws).await {
                ResponseEvent::ThreadList { threads } => {
                    assert!(!threads.iter().any(|thread| thread.thread_id == thread_id));
                }
                event => panic!("unexpected event while re-listing threads: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::AttachThread {
                    thread_id: thread_id.clone(),
                    after_seq: None,
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code,
                    message,
                } => {
                    assert_eq!(code, "thread_not_found");
                    assert_eq!(message, "no live thread with this id");
                }
                event => panic!("unexpected event while attaching closed thread: {event:?}"),
            }

            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                None
            );

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_close_thread_rejects_busy_thread() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness =
                start_multi_agent_harness(&[("alpha", FakeAgentMode::WaitForCancel)]).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Busy".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AddThreadParticipant {
                    thread_id: thread_id.clone(),
                    agent_id: "alpha".into(),
                },
            )
            .await;
            let expected_participant_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadParticipantAdded { participant, .. } => {
                    participant.participant_id
                }
                event => panic!("unexpected event while adding thread participant: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::SendThreadMessage {
                    thread_id: thread_id.clone(),
                    content: "wait".into(),
                    target_participant_ids: None,
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::ThreadMessage { thread_id: tid, .. } => assert_eq!(tid, thread_id),
                event => panic!("unexpected event while starting busy thread prompt: {event:?}"),
            }

            loop {
                match receive_event(&mut ws).await {
                    ResponseEvent::ThreadAssistantMessage {
                        thread_id: tid,
                        participant_id,
                        response,
                        ..
                    } => {
                        assert_eq!(tid, thread_id);
                        assert_eq!(participant_id, expected_participant_id);
                        assert_eq!(response, "waiting for cancel");
                        break;
                    }
                    ResponseEvent::Delta { .. } => {}
                    event => panic!("unexpected event while waiting for busy prompt: {event:?}"),
                }
            }

            send_client_message(
                &mut ws,
                &ClientMessage::CloseThread {
                    thread_id: thread_id.clone(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code,
                    message,
                } => {
                    assert_eq!(code, "thread_busy");
                    assert_eq!(
                        message,
                        "cannot close a thread while agent work is in progress"
                    );
                }
                event => panic!("unexpected event while closing busy thread: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_attach_thread_rejects_cursor_ahead_of_tail() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_multi_agent_harness(&[("alpha", FakeAgentMode::Normal)]).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateThread {
                    title: Some("Ahead".into()),
                    working_dir: ".".into(),
                },
            )
            .await;
            let thread_id = match receive_event(&mut ws).await {
                ResponseEvent::ThreadCreated { thread_id, .. } => thread_id,
                event => panic!("unexpected event while creating thread: {event:?}"),
            };

            send_client_message(
                &mut ws,
                &ClientMessage::AttachThread {
                    thread_id: thread_id.clone(),
                    after_seq: Some(999),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code,
                    ..
                } => {
                    assert_eq!(code, "thread_replay_after_seq_ahead_of_tail");
                }
                event => {
                    panic!("unexpected event while validating thread replay cursor: {event:?}")
                }
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_attach_session_replays_events_after_cursor() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "replay me".into(),
                },
            )
            .await;

            let events = collect_prompt_events(&mut ws, &session_id).await;
            let after_seq = events
                .first()
                .and_then(ResponseEvent::event_seq)
                .expect("expected at least one session event with event_seq");
            let expected_replay = events
                .iter()
                .filter(|event| event.event_seq().unwrap_or(0) > after_seq)
                .cloned()
                .collect::<Vec<_>>();
            let expected_tail = events
                .last()
                .and_then(ResponseEvent::event_seq)
                .expect("expected turn_end with event_seq");

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            let mut ws = connect_ws(harness.port).await;
            send_client_message(
                &mut ws,
                &ClientMessage::AttachSession {
                    session_id: session_id.clone(),
                    after_seq: Some(after_seq),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::SessionAttached { session_id: sid } => {
                    assert_eq!(sid, session_id);
                }
                event => panic!("unexpected event while attaching session: {event:?}"),
            }
            match receive_event(&mut ws).await {
                ResponseEvent::SessionSnapshot { snapshot } => {
                    assert_eq!(snapshot.session_id, session_id);
                    assert_eq!(snapshot.last_event_seq, expected_tail);
                }
                event => panic!("unexpected event while reading session snapshot: {event:?}"),
            }

            let mut replayed_events = Vec::new();
            for _ in 0..expected_replay.len() {
                replayed_events.push(receive_event(&mut ws).await);
            }
            assert_eq!(replayed_events, expected_replay);

            match receive_event(&mut ws).await {
                ResponseEvent::SessionReplayComplete {
                    session_id: sid,
                    last_event_seq,
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(last_event_seq, expected_tail);
                }
                event => panic!("unexpected replay completion event: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_attach_session_rejects_cursor_ahead_of_tail() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::AttachSession {
                    session_id: session_id.clone(),
                    after_seq: Some(999),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: Some(sid),
                    event_seq: None,
                    code,
                    ..
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(code, "replay_after_seq_ahead_of_tail");
                }
                event => panic!("unexpected event while validating replay cursor: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_persists_session_transcript_after_turn_end() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "capture this".into(),
                },
            )
            .await;
            let _ = collect_prompt_events(&mut ws, &session_id).await;

            let transcript_path = harness
                .project_root
                .join(".agentchat")
                .join("sessions")
                .join(format!("{session_id}.json"));
            wait_for(|| transcript_path.exists()).await;

            let transcript: SessionTranscript =
                serde_json::from_str(&std::fs::read_to_string(&transcript_path).unwrap()).unwrap();
            assert_eq!(transcript.session_id, session_id);
            assert_eq!(transcript.agent_id, "fake");
            assert_eq!(transcript.working_dir, ".");
            assert!(matches!(
                transcript.events.first(),
                Some(SessionEvent::UserPrompt { content, .. }) if content == "capture this"
            ));
            let decoded_notification = transcript.events.iter().find_map(|event| match event {
                SessionEvent::AgentUpdate {
                    notification_json, ..
                } => serde_json::from_value::<SessionNotification>(notification_json.clone()).ok(),
                _ => None,
            });
            let decoded_notification = decoded_notification.expect("missing agent update");
            assert_eq!(decoded_notification.session_id.to_string(), session_id);
            assert!(matches!(
                transcript.events.last(),
                Some(SessionEvent::TurnEnd { stop_reason, .. }) if stop_reason == "EndTurn"
            ));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_lists_and_reads_skills() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let skill_dir = harness.project_root.join(".agentchat").join("skills");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(skill_dir.join("testing.md"), "# Testing\n").unwrap();

            let mut ws = connect_ws(harness.port).await;
            send_client_message(&mut ws, &ClientMessage::ListSkills).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillList { skills } => {
                    assert_eq!(skills.len(), 1);
                    assert_eq!(skills[0].name, "testing.md");
                    assert_eq!(skills[0].path, ".agentchat/skills/testing.md");
                }
                event => panic!("unexpected event while listing skills: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::GetSkill {
                    name: "testing.md".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillContent { name, content } => {
                    assert_eq!(name, "testing.md");
                    assert_eq!(content, "# Testing\n");
                }
                event => panic!("unexpected event while reading skill: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_lists_and_reads_shared_skills() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let shared_skill_dir = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("shared");
            std::fs::create_dir_all(&shared_skill_dir).unwrap();
            std::fs::write(shared_skill_dir.join("testing.md"), "# Shared Testing\n").unwrap();

            let mut ws = connect_ws(harness.port).await;
            send_client_message(&mut ws, &ClientMessage::ListSkills).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillList { skills } => {
                    assert_eq!(skills.len(), 1);
                    assert_eq!(skills[0].name, "shared/testing.md");
                    assert_eq!(skills[0].path, ".agentchat/skills/shared/testing.md");
                }
                event => panic!("unexpected event while listing shared skills: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::GetSkill {
                    name: "shared/testing.md".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillContent { name, content } => {
                    assert_eq!(name, "shared/testing.md");
                    assert_eq!(content, "# Shared Testing\n");
                }
                event => panic!("unexpected event while reading shared skill: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_lists_and_reads_agent_specific_skills() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let agent_skill_dir = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("agents")
                .join("fake");
            std::fs::create_dir_all(&agent_skill_dir).unwrap();
            std::fs::write(agent_skill_dir.join("testing.md"), "# Fake Testing\n").unwrap();

            let mut ws = connect_ws(harness.port).await;
            send_client_message(&mut ws, &ClientMessage::ListSkills).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillList { skills } => {
                    assert_eq!(skills.len(), 1);
                    assert_eq!(skills[0].name, "agents/fake/testing.md");
                    assert_eq!(skills[0].path, ".agentchat/skills/agents/fake/testing.md");
                }
                event => panic!("unexpected event while listing agent-specific skills: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::GetSkill {
                    name: "agents/fake/testing.md".into(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::SkillContent { name, content } => {
                    assert_eq!(name, "agents/fake/testing.md");
                    assert_eq!(content, "# Fake Testing\n");
                }
                event => panic!("unexpected event while reading agent-specific skill: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_injects_shared_and_agent_specific_skill_context() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let skill_dir = harness.project_root.join(".agentchat").join("skills");
            let shared_skill_dir = skill_dir.join("shared");
            let agent_skill_dir = skill_dir.join("agents").join("fake");
            let other_agent_skill_dir = skill_dir.join("agents").join("other");
            std::fs::create_dir_all(&shared_skill_dir).unwrap();
            std::fs::create_dir_all(&agent_skill_dir).unwrap();
            std::fs::create_dir_all(&other_agent_skill_dir).unwrap();
            std::fs::write(skill_dir.join("testing.md"), "# Testing\n").unwrap();
            std::fs::write(shared_skill_dir.join("common.md"), "# Common\n").unwrap();
            std::fs::write(agent_skill_dir.join("private.md"), "# Private\n").unwrap();
            std::fs::write(other_agent_skill_dir.join("ignore.md"), "# Ignore\n").unwrap();

            let mut ws = connect_ws(harness.port).await;
            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            for prompt in ["say hello", "say goodbye"] {
                send_client_message(
                    &mut ws,
                    &ClientMessage::Prompt {
                        session_id: session_id.clone(),
                        content: prompt.into(),
                    },
                )
                .await;

                let events = collect_prompt_events(&mut ws, &session_id).await;
                let prompt_echo = events
                    .iter()
                    .find_map(|event| match event {
                        ResponseEvent::Delta {
                            session_id: sid,
                            delta_type: DeltaType::Text,
                            content,
                            ..
                        } if sid == &session_id => Some(content.clone()),
                        _ => None,
                    })
                    .expect("missing text delta");

                assert!(prompt_echo
                    .starts_with("echo: [Shared project knowledge available to every agent:"));
                assert!(prompt_echo.contains(".agentchat/skills/testing.md"));
                assert!(prompt_echo.contains(".agentchat/skills/shared/common.md"));
                assert!(prompt_echo.contains("Agent-specific knowledge for fake:"));
                assert!(prompt_echo.contains(".agentchat/skills/agents/fake/private.md"));
                assert!(!prompt_echo.contains(".agentchat/skills/agents/other/ignore.md"));
                assert!(prompt_echo.ends_with(prompt));
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_distills_session_into_skill_files() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "teach me something".into(),
                },
            )
            .await;
            let _ = collect_prompt_events(&mut ws, &session_id).await;

            send_client_message(
                &mut ws,
                &ClientMessage::DistillSession {
                    session_id: session_id.clone(),
                },
            )
            .await;

            expect_distillation_status(&mut ws, &session_id, "started").await;
            let completed = expect_distillation_status(&mut ws, &session_id, "completed").await;
            assert_eq!(completed, "Updated 2 skills");

            let testing_skill = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("agents")
                .join("fake")
                .join("testing-notes.md");
            let memory_skill = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("shared")
                .join("memory-layer.md");
            wait_for(|| testing_skill.exists() && memory_skill.exists()).await;
            assert!(std::fs::read_to_string(&testing_skill)
                .unwrap()
                .contains("fake ACP agent"));
            assert!(std::fs::read_to_string(&memory_skill)
                .unwrap()
                .contains(".agentchat/sessions"));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_injects_distilled_shared_and_agent_specific_skills_into_new_sessions() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let first_session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: first_session_id.clone(),
                    content: "teach me something".into(),
                },
            )
            .await;
            let _ = collect_prompt_events(&mut ws, &first_session_id).await;

            send_client_message(
                &mut ws,
                &ClientMessage::DistillSession {
                    session_id: first_session_id.clone(),
                },
            )
            .await;

            expect_distillation_status(&mut ws, &first_session_id, "started").await;
            let completed =
                expect_distillation_status(&mut ws, &first_session_id, "completed").await;
            assert_eq!(completed, "Updated 2 skills");

            let shared_skill = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("shared")
                .join("memory-layer.md");
            let agent_skill = harness
                .project_root
                .join(".agentchat")
                .join("skills")
                .join("agents")
                .join("fake")
                .join("testing-notes.md");
            wait_for(|| shared_skill.exists() && agent_skill.exists()).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let second_session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: second_session_id.clone(),
                    content: "use memory".into(),
                },
            )
            .await;

            let events = collect_prompt_events(&mut ws, &second_session_id).await;
            let prompt_echo = events
                .iter()
                .find_map(|event| match event {
                    ResponseEvent::Delta {
                        session_id: sid,
                        delta_type: DeltaType::Text,
                        content,
                        ..
                    } if sid == &second_session_id => Some(content.clone()),
                    _ => None,
                })
                .expect("missing text delta");

            assert!(prompt_echo.contains(".agentchat/skills/shared/memory-layer.md"));
            assert!(prompt_echo.contains("Agent-specific knowledge for fake:"));
            assert!(prompt_echo.contains(".agentchat/skills/agents/fake/testing-notes.md"));
            assert!(prompt_echo.ends_with("use memory"));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_disconnect_keeps_in_flight_prompt_running_until_explicit_cancel() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::WaitForCancel).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "block until cancelled".into(),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Text,
                    content,
                    ..
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event before disconnect: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            let upstream_session_id = harness
                .manager
                .borrow()
                .upstream_session_for_session(&session_id)
                .unwrap()
                .to_string();
            tokio::time::sleep(Duration::from_millis(250)).await;
            assert!(
                !file_contains_line(
                    &harness.events_path,
                    &format!("cancel:{upstream_session_id}")
                ),
                "disconnect should not implicitly cancel the prompt"
            );
            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                Some("fake")
            );

            let mut ws = connect_ws(harness.port).await;
            send_client_message(
                &mut ws,
                &ClientMessage::Cancel {
                    session_id: session_id.clone(),
                },
            )
            .await;

            wait_for_file_line(
                &harness.events_path,
                &format!("cancel:{upstream_session_id}"),
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::TurnEnd {
                    session_id: sid,
                    stop_reason,
                    ..
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(stop_reason, "Cancelled");
                }
                event => panic!("unexpected event after explicit cancel: {event:?}"),
            }

            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                Some("fake")
            );

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_close_session_removes_it_from_session_list() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(&mut ws, &ClientMessage::ListSessions).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionList { sessions } => {
                    assert!(sessions
                        .iter()
                        .any(|session| session.session_id == session_id));
                }
                event => panic!("unexpected event while listing sessions: {event:?}"),
            }

            send_client_message(
                &mut ws,
                &ClientMessage::CloseSession {
                    session_id: session_id.clone(),
                },
            )
            .await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionClosed { session_id: sid } => {
                    assert_eq!(sid, session_id);
                }
                event => panic!("unexpected event while closing session: {event:?}"),
            }

            send_client_message(&mut ws, &ClientMessage::ListSessions).await;
            match receive_event(&mut ws).await {
                ResponseEvent::SessionList { sessions } => {
                    assert!(!sessions
                        .iter()
                        .any(|session| session.session_id == session_id));
                }
                event => panic!("unexpected event while re-listing sessions: {event:?}"),
            }

            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                None
            );

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_reports_agent_crash_to_the_client() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::ExitAfterSession).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: None,
                    event_seq: None,
                    code,
                    message,
                } if code == "agent_crashed" => {
                    assert_eq!(message, "agent process exited");
                }
                event => panic!("unexpected event while waiting for crash: {event:?}"),
            }
            assert_eq!(
                harness.manager.borrow().agent_for_session(&session_id),
                Some("fake")
            );

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_shutdown_cancels_in_flight_prompt() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::WaitForCancel).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
                    agent_id: None,
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            send_client_message(
                &mut ws,
                &ClientMessage::Prompt {
                    session_id: session_id.clone(),
                    content: "wait for daemon shutdown".into(),
                },
            )
            .await;

            match receive_event(&mut ws).await {
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Text,
                    content,
                    ..
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event before shutdown: {event:?}"),
            }

            let upstream_session_id = harness
                .manager
                .borrow()
                .upstream_session_for_session(&session_id)
                .unwrap()
                .to_string();

            let _ = harness.shutdown_tx.send(true);
            let server_result = harness.server_task.await.expect("server task panicked");
            assert!(
                server_result.is_ok(),
                "server returned error: {server_result:?}"
            );

            wait_for_file_line(
                &harness.events_path,
                &format!("cancel:{upstream_session_id}"),
            )
            .await;
            wait_for(|| {
                harness
                    .manager
                    .borrow()
                    .agent_for_session(&session_id)
                    .is_none()
            })
            .await;

            let shutdown = { harness.manager.borrow().shutdown_all() };
            shutdown.await;
            drop(ws);
        })
        .await;
}

async fn start_harness(mode: FakeAgentMode) -> TestHarness {
    start_multi_agent_harness(&[("fake", mode)]).await
}

async fn start_multi_agent_harness(agents: &[(&str, FakeAgentMode)]) -> TestHarness {
    let temp_dir = tempfile::tempdir().unwrap();
    let primary_agent_id = agents
        .first()
        .map(|(agent_id, _)| *agent_id)
        .unwrap_or("fake");
    let events_path = temp_dir
        .path()
        .join(format!("{primary_agent_id}-events.log"));
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut manager = AgentManager::new();
    for (agent_id, mode) in agents {
        let agent_events_path = temp_dir.path().join(format!("{agent_id}-events.log"));
        manager
            .add_agent(
                fake_agent_config_with_id(agent_id, *mode, &agent_events_path),
                project_root.clone(),
            )
            .await
            .unwrap();
    }

    let manager = Rc::new(RefCell::new(manager));
    let session_store = Rc::new(RefCell::new(SessionStore::new(&project_root)));
    let skill_store = Rc::new(SkillStore::new(&project_root));
    let distiller = Rc::new(Distiller::new(skill_store.clone()));
    let port = reserve_port();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server_task = tokio::task::spawn_local(WebSocketServer::new(port).run(
        manager.clone(),
        shutdown_rx,
        session_store,
        skill_store,
        distiller,
    ));

    sleep(Duration::from_millis(100)).await;

    TestHarness {
        manager,
        shutdown_tx,
        server_task,
        events_path,
        project_root,
        port,
        _temp_dir: temp_dir,
    }
}

fn fake_agent_config_with_id(
    agent_id: &str,
    mode: FakeAgentMode,
    events_path: &Path,
) -> AgentConfig {
    let mut env_vars = HashMap::new();
    env_vars.insert("FAKE_ACP_MODE".into(), mode.as_env_value().into());
    env_vars.insert(
        "FAKE_ACP_EVENTS_PATH".into(),
        events_path.display().to_string(),
    );
    env_vars.insert("FAKE_ACP_SESSION_PREFIX".into(), format!("{agent_id}-"));

    let mut extra = serde_json::Map::new();
    extra.insert("kind".into(), serde_json::Value::String("fake".into()));

    AgentConfig {
        id: agent_id.into(),
        name: format!("Fake ACP Agent ({agent_id})"),
        command: fake_agent_binary().display().to_string(),
        args: Vec::new(),
        working_dir: None,
        env_vars,
        extra: extra.into_iter().collect(),
    }
}

fn fake_agent_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_fake_acp_agent")
        .map(PathBuf::from)
        .expect("fake ACP agent binary path is not available")
}

fn reserve_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

async fn connect_ws(port: u16) -> TestWebSocket {
    timeout(
        TEST_TIMEOUT,
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")),
    )
    .await
    .expect("timed out connecting websocket")
    .expect("failed to connect websocket")
    .0
}

async fn send_client_message(ws: &mut TestWebSocket, message: &ClientMessage) {
    ws.send(Message::Text(
        serde_json::to_string(message).unwrap().into(),
    ))
    .await
    .unwrap();
}

async fn expect_session_created(ws: &mut TestWebSocket) -> String {
    match receive_event(ws).await {
        ResponseEvent::SessionCreated {
            session_id,
            agent_id: _,
            event_seq,
        } => {
            assert!(event_seq > 0);
            session_id
        }
        event => panic!("expected session_created event, got {event:?}"),
    }
}

async fn collect_prompt_events(ws: &mut TestWebSocket, session_id: &str) -> Vec<ResponseEvent> {
    let mut events = Vec::new();

    for _ in 0..10 {
        let event = receive_event(ws).await;
        let finished = matches!(
            &event,
            ResponseEvent::TurnEnd {
                session_id: sid,
                ..
            } if sid == session_id
        );
        events.push(event);
        if finished {
            break;
        }
    }

    events
}

async fn expect_distillation_status(
    ws: &mut TestWebSocket,
    session_id: &str,
    status: &str,
) -> String {
    match receive_event(ws).await {
        ResponseEvent::DistillationStatus {
            session_id: sid,
            status: current_status,
            message,
            ..
        } if sid == session_id && current_status == status => message,
        event => panic!("unexpected distillation event: {event:?}"),
    }
}

async fn receive_event(ws: &mut TestWebSocket) -> ResponseEvent {
    loop {
        let message = timeout(TEST_TIMEOUT, ws.next())
            .await
            .expect("timed out waiting for websocket message")
            .expect("websocket closed unexpectedly")
            .expect("websocket returned an error");

        match message {
            Message::Text(text) => return serde_json::from_str(&text).unwrap(),
            Message::Ping(payload) => {
                ws.send(Message::Pong(payload)).await.unwrap();
            }
            Message::Close(frame) => panic!("websocket closed unexpectedly: {frame:?}"),
            _ => {}
        }
    }
}

async fn wait_for(mut predicate: impl FnMut() -> bool) {
    timeout(TEST_TIMEOUT, async {
        loop {
            if predicate() {
                return;
            }
            sleep(POLL_INTERVAL).await;
        }
    })
    .await
    .expect("timed out waiting for condition");
}

fn file_contains_line(path: &Path, expected_line: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|content| content.lines().any(|line| line == expected_line))
        .unwrap_or(false)
}

async fn wait_for_file_line(path: &Path, expected_line: &str) {
    wait_for(|| file_contains_line(path, expected_line)).await;
}
