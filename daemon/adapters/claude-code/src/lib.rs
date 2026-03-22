use std::collections::HashMap;

use async_trait::async_trait;
use agentchat_protocol::{
    AdapterCapabilities, AdapterError, AdapterInfo, AgentAdapter, AgentConfig, AgentStatus,
    HealthStatus, Prompt, ResponseEvent, SessionInfo, SessionMetadata, SessionOptions,
    SessionSummary,
};
use tokio::sync::mpsc;

/// Claude Code adapter — wraps the `claude` CLI as a subprocess.
pub struct ClaudeCodeAdapter {
    config: Option<AgentConfig>,
    status: AgentStatus,
    /// session_id → child process handle
    active_processes: HashMap<String, tokio::process::Child>,
}

impl ClaudeCodeAdapter {
    pub fn new() -> Self {
        Self {
            config: None,
            status: AgentStatus::Offline,
            active_processes: HashMap::new(),
        }
    }
}

impl Default for ClaudeCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentAdapter for ClaudeCodeAdapter {
    async fn init(&mut self, config: AgentConfig) -> Result<AdapterInfo, AdapterError> {
        // Check that the claude CLI exists and get its version
        let output = tokio::process::Command::new(&config.cli_path)
            .arg("--version")
            .output()
            .await
            .map_err(|_| AdapterError::CliNotFound(config.cli_path.clone()))?;

        let cli_version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        self.config = Some(config);
        self.status = AgentStatus::Idle;

        Ok(AdapterInfo {
            adapter_type: "claude-code".into(),
            version: "0.1.0".into(),
            cli_version: Some(cli_version),
            capabilities: self.capabilities(),
        })
    }

    async fn health_check(&self) -> HealthStatus {
        let Some(config) = &self.config else {
            return HealthStatus::Unhealthy("adapter not initialized".into());
        };
        match tokio::process::Command::new(&config.cli_path)
            .arg("--version")
            .output()
            .await
        {
            Ok(_) => HealthStatus::Healthy,
            Err(e) => HealthStatus::Unhealthy(e.to_string()),
        }
    }

    async fn shutdown(&mut self) -> Result<(), AdapterError> {
        for (_, mut child) in self.active_processes.drain() {
            let _ = child.kill().await;
        }
        self.status = AgentStatus::Offline;
        Ok(())
    }

    async fn create_session(
        &mut self,
        _options: SessionOptions,
    ) -> Result<SessionInfo, AdapterError> {
        todo!("spawn claude CLI subprocess and create a new session")
    }

    async fn resume_session(
        &mut self,
        _session_id: &str,
    ) -> Result<SessionInfo, AdapterError> {
        todo!("resume an existing Claude Code session via --resume")
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        todo!("list resumable Claude Code sessions")
    }

    async fn send_prompt(
        &mut self,
        _session_id: &str,
        _prompt: Prompt,
    ) -> Result<mpsc::Receiver<ResponseEvent>, AdapterError> {
        todo!("spawn claude -p with --output-format stream-json and parse stdout")
    }

    async fn abort(&mut self, session_id: &str) -> Result<(), AdapterError> {
        if let Some(mut child) = self.active_processes.remove(session_id) {
            child
                .kill()
                .await
                .map_err(|e| AdapterError::Other(e.to_string()))?;
        }
        Ok(())
    }

    fn status(&self) -> AgentStatus {
        self.status.clone()
    }

    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities {
            session_resume: true,
            streaming: true,
            abort: true,
            token_usage: true,
            cost_tracking: true,
            hooks: true,
            concurrent_sessions: true,
        }
    }

    async fn session_metadata(
        &self,
        _session_id: &str,
    ) -> Result<SessionMetadata, AdapterError> {
        todo!("extract session metadata from Claude Code output")
    }
}
