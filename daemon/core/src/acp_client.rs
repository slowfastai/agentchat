//! ACP client wrapper — spawns an agent subprocess and communicates via ACP over stdio.
//!
//! This module handles the full ACP lifecycle: subprocess management, initialize handshake,
//! session creation, prompt/response streaming, and cancellation.

use std::path::PathBuf;

use agent_client_protocol::*;
use futures::future::LocalBoxFuture;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info};

use agentchat_protocol::AgentConfig;

use crate::capabilities::DaemonClient;

/// Handle to a running ACP agent connection.
///
/// Wraps a `ClientSideConnection` (which implements the `Agent` trait)
/// and provides methods to interact with the agent.
pub struct AcpAgent {
    /// ACP connection — use this to call Agent methods (initialize, new_session, prompt, etc.)
    conn: ClientSideConnection,
    /// The agent subprocess handle.
    child: tokio::process::Child,
    /// Channel to receive session update notifications from the DaemonClient.
    update_rx: mpsc::UnboundedReceiver<SessionNotification>,
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

        // Channel for forwarding session notifications from DaemonClient to the caller
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

        // Spawn the IO task on the local set — this drives the JSON-RPC communication
        tokio::task::spawn_local(async move {
            if let Err(e) = io_task.await {
                error!("ACP IO task error: {e}");
            }
            debug!("ACP IO task ended");
        });

        info!("spawned agent process: {}", config.command);

        Ok(Self {
            conn,
            child,
            update_rx,
        })
    }

    /// Run the ACP initialize handshake.
    pub async fn initialize(&self) -> Result<InitializeResponse> {
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

    /// Create a new session.
    pub async fn new_session(&self, cwd: PathBuf) -> Result<NewSessionResponse> {
        let request = NewSessionRequest::new(cwd);
        let response = self.conn.new_session(request).await?;
        info!("ACP session created: {}", response.session_id);
        Ok(response)
    }

    /// Send a prompt (blocking until the turn completes).
    ///
    /// Session updates are streamed via the `update_rx` channel concurrently.
    pub async fn prompt(&self, session_id: SessionId, text: String) -> Result<PromptResponse> {
        let request = PromptRequest::new(session_id, vec![ContentBlock::from(text)]);
        self.conn.prompt(request).await
    }

    /// Cancel an ongoing prompt turn.
    pub async fn cancel(&self, session_id: SessionId) -> Result<()> {
        self.conn.cancel(CancelNotification::new(session_id)).await
    }

    /// Take the update notification receiver.
    ///
    /// Call this once after construction to get the stream of session updates
    /// that arrive while prompts are being processed.
    pub fn take_update_rx(&mut self) -> mpsc::UnboundedReceiver<SessionNotification> {
        let (_, empty_rx) = mpsc::unbounded_channel();
        std::mem::replace(&mut self.update_rx, empty_rx)
    }

    /// Subscribe to the raw ACP stream (all JSON-RPC messages).
    pub fn subscribe(&self) -> StreamReceiver {
        self.conn.subscribe()
    }

    /// Kill the agent subprocess.
    pub async fn shutdown(&mut self) {
        info!("shutting down agent process");
        let _ = self.child.kill().await;
    }
}
