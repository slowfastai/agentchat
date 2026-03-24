use std::cell::RefCell;
use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::{
    AgentConfig, ClientMessage, DeltaType, ResponseEvent, SessionEvent, SessionTranscript,
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
async fn websocket_round_trip_streams_prompt_events_and_cleans_up_sessions() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
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
                } if sid == &session_id && content == "echo: say hello"
            )));
            assert!(events.iter().any(|event| matches!(
                event,
                ResponseEvent::Delta {
                    session_id: sid,
                    delta_type: DeltaType::Thinking,
                    content,
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
                } if sid == &session_id && stop_reason == "EndTurn"
            )));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            wait_for(|| {
                harness
                    .manager
                    .borrow()
                    .agent_for_session(&session_id)
                    .is_none()
            })
            .await;
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
            assert!(transcript
                .events
                .iter()
                .any(|event| matches!(event, SessionEvent::AgentUpdate { .. })));
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
async fn websocket_injects_skill_context_into_every_prompt() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let skill_dir = harness.project_root.join(".agentchat").join("skills");
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(skill_dir.join("testing.md"), "# Testing\n").unwrap();

            let mut ws = connect_ws(harness.port).await;
            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
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
                        } if sid == &session_id => Some(content.clone()),
                        _ => None,
                    })
                    .expect("missing text delta");

                assert!(prompt_echo.starts_with(
                    "echo: [Shared project knowledge available to every agent:"
                ));
                assert!(prompt_echo.contains(".agentchat/skills/testing.md"));
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
                .join("shared")
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
async fn websocket_injects_distilled_shared_skills_into_new_sessions() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::Normal).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
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
            wait_for(|| shared_skill.exists()).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
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
                    } if sid == &second_session_id => Some(content.clone()),
                    _ => None,
                })
                .expect("missing text delta");

            assert!(prompt_echo.contains(".agentchat/skills/shared/testing-notes.md"));
            assert!(prompt_echo.contains(".agentchat/skills/shared/memory-layer.md"));
            assert!(prompt_echo.ends_with("use memory"));

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);
            harness.finish().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn websocket_disconnect_cancels_in_flight_prompt_and_removes_session() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let harness = start_harness(FakeAgentMode::WaitForCancel).await;
            let mut ws = connect_ws(harness.port).await;

            send_client_message(
                &mut ws,
                &ClientMessage::CreateSession {
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
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event before disconnect: {event:?}"),
            }

            ws.send(Message::Close(None)).await.unwrap();
            drop(ws);

            wait_for_file_line(&harness.events_path, &format!("cancel:{session_id}")).await;
            wait_for(|| {
                harness
                    .manager
                    .borrow()
                    .agent_for_session(&session_id)
                    .is_none()
            })
            .await;
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
                    working_dir: ".".into(),
                },
            )
            .await;
            let session_id = expect_session_created(&mut ws).await;

            match receive_event(&mut ws).await {
                ResponseEvent::Error {
                    session_id: None,
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
                } => {
                    assert_eq!(sid, session_id);
                    assert_eq!(content, "waiting for cancel");
                }
                event => panic!("unexpected event before shutdown: {event:?}"),
            }

            let _ = harness.shutdown_tx.send(true);
            let server_result = harness.server_task.await.expect("server task panicked");
            assert!(
                server_result.is_ok(),
                "server returned error: {server_result:?}"
            );

            wait_for_file_line(&harness.events_path, &format!("cancel:{session_id}")).await;
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
    let temp_dir = tempfile::tempdir().unwrap();
    let events_path = temp_dir.path().join("fake-agent-events.log");
    let project_root = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();

    let mut manager = AgentManager::new();
    manager
        .add_agent(fake_agent_config(mode, &events_path), project_root.clone())
        .await
        .unwrap();

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

fn fake_agent_config(mode: FakeAgentMode, events_path: &Path) -> AgentConfig {
    let mut env_vars = HashMap::new();
    env_vars.insert("FAKE_ACP_MODE".into(), mode.as_env_value().into());
    env_vars.insert(
        "FAKE_ACP_EVENTS_PATH".into(),
        events_path.display().to_string(),
    );

    AgentConfig {
        id: "fake".into(),
        name: "Fake ACP Agent".into(),
        command: fake_agent_binary().display().to_string(),
        args: Vec::new(),
        working_dir: None,
        env_vars,
        extra: Default::default(),
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
        ResponseEvent::SessionCreated { session_id } => session_id,
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

async fn wait_for_file_line(path: &Path, expected_line: &str) {
    wait_for(|| {
        std::fs::read_to_string(path)
            .map(|content| content.lines().any(|line| line == expected_line))
            .unwrap_or(false)
    })
    .await;
}
