use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use agent_client_protocol::{self as acp, Client as _};
use tokio::sync::{mpsc, Notify};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeAgentMode {
    Normal,
    WaitForCancel,
    ExitAfterSession,
}

impl FakeAgentMode {
    fn from_env() -> Self {
        match std::env::var("FAKE_ACP_MODE").as_deref() {
            Ok("wait_for_cancel") => Self::WaitForCancel,
            Ok("exit_after_session") => Self::ExitAfterSession,
            _ => Self::Normal,
        }
    }
}

struct FakeAgent {
    session_update_tx: mpsc::UnboundedSender<acp::SessionNotification>,
    next_session_id: Cell<u64>,
    session_prefix: String,
    cancelled_sessions: RefCell<HashSet<String>>,
    cancel_notify: Rc<Notify>,
    mode: FakeAgentMode,
    events_path: Option<PathBuf>,
}

impl FakeAgent {
    fn new(session_update_tx: mpsc::UnboundedSender<acp::SessionNotification>) -> Self {
        Self {
            session_update_tx,
            next_session_id: Cell::new(1),
            session_prefix: std::env::var("FAKE_ACP_SESSION_PREFIX").unwrap_or_default(),
            cancelled_sessions: RefCell::new(HashSet::new()),
            cancel_notify: Rc::new(Notify::new()),
            mode: FakeAgentMode::from_env(),
            events_path: std::env::var_os("FAKE_ACP_EVENTS_PATH").map(PathBuf::from),
        }
    }

    fn record_event(&self, event: &str) {
        let Some(path) = &self.events_path else {
            return;
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{event}");
        }
    }

    fn send_update(
        &self,
        session_id: &acp::SessionId,
        update: acp::SessionUpdate,
    ) -> acp::Result<()> {
        self.session_update_tx
            .send(acp::SessionNotification::new(session_id.clone(), update))
            .map_err(|_| acp::Error::internal_error())
    }

    fn prompt_text(&self, prompt: &[acp::ContentBlock]) -> String {
        prompt
            .iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn is_distillation_prompt(&self, prompt_text: &str) -> bool {
        prompt_text
            .contains("You are analyzing a completed coding session to extract reusable knowledge.")
    }

    fn visible_user_message<'a>(&self, prompt_text: &'a str) -> &'a str {
        prompt_text
            .split_once("[Original User Message]\n")
            .map(|(_, message)| message)
            .unwrap_or(prompt_text)
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for FakeAgent {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> acp::Result<acp::InitializeResponse> {
        self.record_event("initialize");
        Ok(
            acp::InitializeResponse::new(args.protocol_version).agent_info(
                acp::Implementation::new("fake-acp-agent", "0.1.0").title("Fake ACP Agent"),
            ),
        )
    }

    async fn authenticate(
        &self,
        _args: acp::AuthenticateRequest,
    ) -> acp::Result<acp::AuthenticateResponse> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        args: acp::NewSessionRequest,
    ) -> acp::Result<acp::NewSessionResponse> {
        let session_id = acp::SessionId::new(format!(
            "{}session-{}",
            self.session_prefix,
            self.next_session_id.get()
        ));
        self.next_session_id.set(self.next_session_id.get() + 1);
        self.record_event(&format!(
            "new_session:{}:{}",
            session_id,
            args.cwd.display()
        ));

        if self.mode == FakeAgentMode::ExitAfterSession {
            tokio::task::spawn_local(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                std::process::exit(0);
            });
        }

        Ok(acp::NewSessionResponse::new(session_id))
    }

    async fn prompt(&self, args: acp::PromptRequest) -> acp::Result<acp::PromptResponse> {
        let session_id = args.session_id.clone();
        let session_id_text = session_id.to_string();
        let prompt_text = self.prompt_text(&args.prompt);
        let visible_user_message = self.visible_user_message(&prompt_text);
        self.record_event(&format!("prompt:{}:{}", session_id_text, prompt_text));

        if self.is_distillation_prompt(&prompt_text) {
            self.send_update(
                &session_id,
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                    acp::ContentBlock::from(concat!(
                        "---SKILL: shared/memory-layer---\n",
                        "# Memory Layer\n",
                        "- Persist session transcripts under .agentchat/sessions.\n",
                        "---END SKILL---\n",
                        "---SKILL: agents/fake/testing-notes---\n",
                        "# Testing Notes\n",
                        "- Use the fake ACP agent in websocket tests.\n",
                        "---END SKILL---\n"
                    )),
                )),
            )?;
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        match self.mode {
            FakeAgentMode::Normal | FakeAgentMode::ExitAfterSession => {
                self.send_update(
                    &session_id,
                    acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk::new(
                        acp::ContentBlock::from("thinking about the request"),
                    )),
                )?;
                self.send_update(
                    &session_id,
                    acp::SessionUpdate::ToolCall(
                        acp::ToolCall::new("tool-1", "Demo Tool")
                            .status(acp::ToolCallStatus::InProgress),
                    ),
                )?;
                self.send_update(
                    &session_id,
                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::from(format!("echo: {visible_user_message}")),
                    )),
                )?;
                Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
            }
            FakeAgentMode::WaitForCancel => {
                self.send_update(
                    &session_id,
                    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::from("waiting for cancel"),
                    )),
                )?;

                loop {
                    if self.cancelled_sessions.borrow().contains(&session_id_text) {
                        return Ok(acp::PromptResponse::new(acp::StopReason::Cancelled));
                    }
                    self.cancel_notify.notified().await;
                }
            }
        }
    }

    async fn cancel(&self, args: acp::CancelNotification) -> acp::Result<()> {
        let session_id = args.session_id.to_string();
        self.record_event(&format!("cancel:{session_id}"));
        self.cancelled_sessions.borrow_mut().insert(session_id);
        self.cancel_notify.notify_waiters();
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> acp::Result<()> {
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async move {
            let (update_tx, mut update_rx) = mpsc::unbounded_channel();
            let (conn, io_task) = acp::AgentSideConnection::new(
                FakeAgent::new(update_tx),
                outgoing,
                incoming,
                |fut| {
                    tokio::task::spawn_local(fut);
                },
            );

            tokio::task::spawn_local(async move {
                while let Some(notification) = update_rx.recv().await {
                    if conn.session_notification(notification).await.is_err() {
                        break;
                    }
                }
            });

            io_task.await
        })
        .await
}
