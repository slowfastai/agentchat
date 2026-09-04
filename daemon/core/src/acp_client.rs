//! ACP client wrapper -- spawns an agent subprocess and communicates via ACP over stdio.
//!
//! This module handles the full ACP lifecycle: subprocess management, initialize handshake,
//! session creation, prompt/response streaming, and cancellation.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::*;
use futures::future::LocalBoxFuture;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use agentchat_protocol::{
    AgentConfig, AgentSessionSettings, AgentSettingOption, AgentSettingValue,
};

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
    latest_session_id: RefCell<Option<String>>,
    session_close_supported: Cell<bool>,
    discovered_setting_models: RefCell<std::collections::HashSet<String>>,
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
            latest_session_id: RefCell::new(None),
            session_close_supported: Cell::new(false),
            discovered_setting_models: RefCell::new(std::collections::HashSet::new()),
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

    fn remember_session_config_options(
        &self,
        session_id: String,
        options: Vec<SessionConfigOption>,
    ) {
        self.session_config_options
            .borrow_mut()
            .insert(session_id.clone(), options);
        *self.latest_session_id.borrow_mut() = Some(session_id);
    }
}

#[async_trait::async_trait(?Send)]
impl AgentBackend for AcpAgent {
    async fn initialize(&self) -> Result<(), String> {
        let response = self.initialize_acp().await.map_err(|err| err.to_string())?;
        self.session_close_supported.set(
            response
                .agent_capabilities
                .session_capabilities
                .close
                .is_some(),
        );
        Ok(())
    }

    async fn new_session(&self, cwd: PathBuf) -> Result<String, String> {
        let request = NewSessionRequest::new(cwd);
        let response = self
            .conn
            .new_session(request)
            .await
            .map_err(|err| err.to_string())?;
        let session_id = response.session_id.to_string();
        self.remember_session_config_options(
            session_id.clone(),
            response.config_options.unwrap_or_default(),
        );
        info!("ACP session created: {}", response.session_id);
        Ok(session_id)
    }

    async fn discover_settings(
        &self,
        cwd: PathBuf,
        settings: AgentSessionSettings,
    ) -> Result<(), String> {
        let model_key = settings.model.clone().unwrap_or_default();
        if self.discovered_setting_models.borrow().contains(&model_key) {
            return Ok(());
        }

        let session_id = self.new_session(cwd).await?;
        let result = self
            .set_session_settings(session_id.clone(), settings)
            .await;

        if self.session_close_supported.get() {
            if let Err(err) = self
                .conn
                .close_session(CloseSessionRequest::new(session_id.clone()))
                .await
            {
                warn!(
                    "failed to close temporary ACP settings session {}: {}",
                    session_id, err
                );
            }
        }

        result?;
        self.discovered_setting_models
            .borrow_mut()
            .insert(model_key);
        Ok(())
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
            self.remember_session_config_options(session_id.clone(), response.config_options);
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

    fn setting_options(&self) -> Vec<AgentSettingOption> {
        let Some(session_id) = self.latest_session_id.borrow().clone() else {
            return Vec::new();
        };
        let options = self
            .session_config_options
            .borrow()
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        acp_setting_options(&options)
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

fn acp_setting_options(options: &[SessionConfigOption]) -> Vec<AgentSettingOption> {
    let mut settings = Vec::new();
    for option in options {
        let Some(setting) = acp_setting_option(option) else {
            continue;
        };
        if let Some(existing) = settings
            .iter_mut()
            .find(|item: &&mut AgentSettingOption| item.id == setting.id)
        {
            *existing = setting;
        } else {
            settings.push(setting);
        }
    }
    settings
}

fn acp_setting_option(option: &SessionConfigOption) -> Option<AgentSettingOption> {
    let (id, name) = if acp_option_matches(option, "model") {
        ("model", "Model")
    } else if acp_option_matches(option, "reasoning_effort") {
        ("reasoning_effort", "Reasoning effort")
    } else {
        return None;
    };

    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };
    let values: Vec<AgentSettingValue> = match &select.options {
        SessionConfigSelectOptions::Ungrouped(options) => options
            .iter()
            .map(|value| AgentSettingValue {
                id: value.value.to_string(),
                label: value.name.clone(),
            })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .map(|value| AgentSettingValue {
                id: value.value.to_string(),
                label: value.name.clone(),
            })
            .collect(),
        _ => Vec::new(),
    };
    if values.is_empty() {
        return None;
    }

    Some(AgentSettingOption {
        id: id.into(),
        name: name.into(),
        category: if id == "model" {
            "model".into()
        } else {
            "thought_level".into()
        },
        values,
        values_by_model: None,
        current_value: Some(select.current_value.to_string()),
        apply_scope: "session".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acp_options_expose_model_and_reasoning_selectors() {
        let options = vec![
            SessionConfigOption::select(
                "model",
                "Model",
                "openai/gpt-5",
                vec![
                    SessionConfigSelectOption::new("openai/gpt-5", "GPT-5"),
                    SessionConfigSelectOption::new("anthropic/claude-sonnet", "Claude Sonnet"),
                ],
            )
            .category(SessionConfigOptionCategory::Model),
            SessionConfigOption::select(
                "reasoning",
                "Reasoning",
                "high",
                vec![
                    SessionConfigSelectOption::new("low", "Low"),
                    SessionConfigSelectOption::new("high", "High"),
                ],
            )
            .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::select(
                "mode",
                "Mode",
                "build",
                vec![SessionConfigSelectOption::new("build", "Build")],
            )
            .category(SessionConfigOptionCategory::Mode),
        ];

        let settings = acp_setting_options(&options);

        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].id, "model");
        assert_eq!(settings[0].current_value.as_deref(), Some("openai/gpt-5"));
        assert_eq!(settings[0].values[1].id, "anthropic/claude-sonnet");
        assert_eq!(settings[1].id, "reasoning_effort");
        assert_eq!(settings[1].current_value.as_deref(), Some("high"));
        assert_eq!(settings[1].values[0].id, "low");
    }
}
