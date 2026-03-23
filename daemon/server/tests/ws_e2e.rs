use std::cell::RefCell;
use std::collections::HashMap;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use agentchat_core::agent_manager::AgentManager;
use agentchat_protocol::{AgentConfig, ClientMessage, DeltaType, ResponseEvent};
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

            let mut saw_text_delta = false;
            let mut saw_thinking_delta = false;
            let mut saw_tool_update = false;
            let mut saw_turn_end = false;

            for _ in 0..10 {
                match receive_event(&mut ws).await {
                    ResponseEvent::Delta {
                        session_id: sid,
                        delta_type: DeltaType::Text,
                        content,
                    } if sid == session_id => {
                        assert_eq!(content, "echo: say hello");
                        saw_text_delta = true;
                    }
                    ResponseEvent::Delta {
                        session_id: sid,
                        delta_type: DeltaType::Thinking,
                        content,
                    } if sid == session_id => {
                        assert_eq!(content, "thinking about the request");
                        saw_thinking_delta = true;
                    }
                    ResponseEvent::ToolUpdate {
                        session_id: sid,
                        tool_call_id,
                        title,
                        status,
                        ..
                    } if sid == session_id => {
                        assert_eq!(tool_call_id, "tool-1");
                        assert_eq!(title, "Demo Tool");
                        assert_eq!(status, "InProgress");
                        saw_tool_update = true;
                    }
                    ResponseEvent::TurnEnd {
                        session_id: sid,
                        stop_reason,
                    } if sid == session_id => {
                        assert_eq!(stop_reason, "EndTurn");
                        saw_turn_end = true;
                    }
                    event => panic!("unexpected event during prompt round trip: {event:?}"),
                }

                if saw_text_delta && saw_thinking_delta && saw_tool_update && saw_turn_end {
                    break;
                }
            }

            assert!(saw_text_delta, "missing text delta");
            assert!(saw_thinking_delta, "missing thinking delta");
            assert!(saw_tool_update, "missing tool update");
            assert!(saw_turn_end, "missing turn end");

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
        .add_agent(fake_agent_config(mode, &events_path), project_root)
        .await
        .unwrap();

    let manager = Rc::new(RefCell::new(manager));
    let port = reserve_port();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server_task =
        tokio::task::spawn_local(WebSocketServer::new(port).run(manager.clone(), shutdown_rx));

    sleep(Duration::from_millis(100)).await;

    TestHarness {
        manager,
        shutdown_tx,
        server_task,
        events_path,
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
