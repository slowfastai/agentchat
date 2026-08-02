//! ACP client wrapper -- spawns an agent subprocess and communicates via ACP over stdio.
//!
//! This module handles the full ACP lifecycle: subprocess management, initialize handshake,
//! session creation, prompt/response streaming, and cancellation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::*;
use futures::future::LocalBoxFuture;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use agentchat_protocol::{AgentConfig, AgentSessionSettings};

use crate::backend::{AgentBackend, AgentNotification, AgentPromptResult};
use crate::capabilities::DaemonClient;

/// Handle to a running ACP agent connection.
///
/// Wraps a `ClientSideConnection` (which implements the `Agent` trait)
/// and provides methods to interact with the agent.
pub struct AcpAgent {
    /// ACP connection -- use this to call Agent methods (initialize, new_session, prompt, etc.)
    conn: ClientSideConnection,
    /// Channel to receive session update notifications from the DaemonClient.
    update_rx: RefCell<Option<mpsc::UnboundedReceiver<AgentNotification>>>,
    session_config_options: RefCell<HashMap<String, Vec<SessionConfigOption>>>,
    /// Broadcasts whether the child process is still alive.
    health_tx: watch::Sender<bool>,
    /// Signals the child-process monitor task to terminate the subprocess.
    kill_tx: watch::Sender<bool>,
}

async fn monitor_agent_process(
    mut child: tokio::process::Child,
    mut kill_rx: watch::Receiver<bool>,
    health_tx: watch::Sender<bool>,
) {
    tokio::select! {
        status = child.wait() => {
            match status {
                Ok(status) => info!("agent process exited with status {status}"),
                Err(e) => error!("failed waiting for agent process: {e}"),
            }
        }
        changed = kill_rx.changed() => {
            match changed {
                Ok(()) if *kill_rx.borrow() => {
                    info!("shutting down agent process");
                    if let Err(e) = child.kill().await {
                        warn!("failed to kill agent process: {e}");
                    }
                    if let Err(e) = child.wait().await {
                        error!("failed waiting for killed agent process: {e}");
                    }
                }
                Ok(()) => {
                    debug!("received unexpected agent kill signal state");
                }
                Err(_) => {
                    warn!("agent kill signal dropped; terminating child process");
                    if let Err(e) = child.kill().await {
                        warn!("failed to kill orphaned agent process: {e}");
                    }
                    if let Err(e) = child.wait().await {
                        error!("failed waiting for orphaned agent process: {e}");
                    }
                }
            }
        }
    }

    if health_tx.send(false).is_err() {
        debug!("agent health watchers dropped");
    }
}

impl AcpAgent {
    /// Spawn an agent subprocess and establish an ACP connection.
    ///
    /// This sets up the stdio pipes, creates the `DaemonClient` (our Client trait impl),
    /// and returns an `AcpAgent` ready for `initialize()`.
    pub fn spawn(config: &AgentConfig, project_root: PathBuf) -> std::result::Result<Self, String> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);

        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        } else {
            cmd.current_dir(&project_root);
        }

        for (k, v) in &config.env_vars {
            cmd.env(k, v);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn agent '{}': {e}", config.command))?;

        let stdin = child.stdin.take().ok_or("failed to capture agent stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("failed to capture agent stdout")?;

        // Convert tokio IO to futures IO (ACP SDK expects futures::AsyncRead/AsyncWrite)
        let write_stream = stdin.compat_write();
        let read_stream = stdout.compat();

        // Channel for forwarding session notifications from DaemonClient to the caller.
        let (update_tx, update_rx) = mpsc::unbounded_channel();

        let client = DaemonClient::new(project_root, update_tx);

        let (conn, io_task) = ClientSideConnection::new(
            client,
            write_stream,
            read_stream,
            |fut: LocalBoxFuture<'static, ()>| {
                tokio::task::spawn_local(fut);
            },
        );

        let (health_tx, _) = watch::channel(true);
        let (kill_tx, kill_rx) = watch::channel(false);

        // Spawn the IO task on the local set -- this drives the JSON-RPC communication.
        tokio::task::spawn_local(async move {
            if let Err(e) = io_task.await {
                error!("ACP IO task error: {e}");
            }
            debug!("ACP IO task ended");
        });

        tokio::task::spawn_local(monitor_agent_process(child, kill_rx, health_tx.clone()));

        info!("spawned agent process: {}", config.command);

        Ok(Self {
            conn,
            update_rx: RefCell::new(Some(update_rx)),
            session_config_options: RefCell::new(HashMap::new()),
            health_tx,
            kill_tx,
        })
    }

    async fn initialize_acp(&self) -> Result<InitializeResponse> {
        let request = InitializeRequest::new(ProtocolVersion::LATEST)
            .client_capabilities(
                ClientCapabilities::new()
                    .fs(FileSystemCapabilities::new()
                        .read_text_file(true)
                        .write_text_file(true))
                    .terminal(true),
            )
            .client_info(Implementation::new("agentchat-daemon", "0.1.0"));

        let response = self.conn.initialize(request).await?;

        info!(
            "ACP initialized: protocol_version={}, agent={:?}",
            response.protocol_version,
            response.agent_info.as_ref().map(|i| &i.name),
        );

        Ok(response)
    }
}

#[async_trait::async_trait(?Send)]
impl AgentBackend for AcpAgent {
    async fn initialize(&self) -> Result<(), String> {
        self.initialize_acp()
            .await
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    async fn new_session(&self, cwd: PathBuf) -> Result<String, String> {
        let request = NewSessionRequest::new(cwd);
        let response = self
            .conn
            .new_session(request)
            .await
            .map_err(|err| err.to_string())?;
        if let Some(options) = response.config_options.clone() {
            self.session_config_options
                .borrow_mut()
                .insert(response.session_id.to_string(), options);
        }
        info!("ACP session created: {}", response.session_id);
        Ok(response.session_id.to_string())
    }

    async fn set_session_settings(
        &self,
        session_id: String,
        settings: AgentSessionSettings,
    ) -> Result<(), String> {
        let values = [
            ("model", settings.model),
            ("reasoning_effort", settings.reasoning_effort),
        ];

        for (category, value) in values {
            let Some(value) = value else {
                continue;
            };
            let options = self
                .session_config_options
                .borrow()
                .get(&session_id)
                .cloned()
                .unwrap_or_default();
            let option = options
                .iter()
                .find(|option| acp_option_matches(option, category))
                .ok_or_else(|| {
                    format!(
                        "ACP agent does not expose a {category} setting for session {session_id}"
                    )
                })?;

            let response = self
                .conn
                .set_session_config_option(SetSessionConfigOptionRequest::new(
                    SessionId::new(session_id.clone()),
                    option.id.clone(),
                    value.as_str(),
                ))
                .await
                .map_err(|err| err.to_string())?;
            self.session_config_options
                .borrow_mut()
                .insert(session_id.clone(), response.config_options);
        }

        Ok(())
    }

    async fn prompt(&self, session_id: String, text: String) -> Result<AgentPromptResult, String> {
        let request =
            PromptRequest::new(SessionId::new(session_id), vec![ContentBlock::from(text)]);
        let response = self
            .conn
            .prompt(request)
            .await
            .map_err(|err| err.to_string())?;
        Ok(AgentPromptResult::new(format!(
            "{:?}",
            response.stop_reason
        )))
    }

    async fn cancel(&self, session_id: String) -> Result<(), String> {
        self.conn
            .cancel(CancelNotification::new(SessionId::new(session_id)))
            .await
            .map_err(|err| err.to_string())
    }

    fn take_update_rx(&self) -> Option<mpsc::UnboundedReceiver<AgentNotification>> {
        self.update_rx.borrow_mut().take()
    }

    fn subscribe_health(&self) -> watch::Receiver<bool> {
        self.health_tx.subscribe()
    }

    fn is_alive(&self) -> bool {
        *self.health_tx.borrow()
    }

    async fn shutdown(&self) {
        if !self.is_alive() {
            return;
        }

        if self.kill_tx.send(true).is_err() {
            warn!("agent shutdown signal receiver dropped");
            return;
        }

        let mut health_rx = self.health_tx.subscribe();
        while *health_rx.borrow() {
            if health_rx.changed().await.is_err() {
                break;
            }
        }
    }
}

fn acp_option_matches(option: &SessionConfigOption, category: &str) -> bool {
    let id = option.id.to_string().to_ascii_lowercase();
    match category {
        "model" => {
            matches!(option.category, Some(SessionConfigOptionCategory::Model))
                || id == "model"
                || id.contains("model")
        }
        "reasoning_effort" => {
            matches!(
                option.category,
                Some(SessionConfigOptionCategory::ThoughtLevel)
            ) || id.contains("reason")
                || id.contains("thought")
                || id.contains("effort")
        }
        _ => false,
    }
}
