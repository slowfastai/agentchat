use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use agentchat_protocol::{AgentSessionSettings, AgentSettingOption};

/// Backend-agnostic streaming update emitted by an agent session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentNotification {
    pub session_id: String,
    #[serde(flatten)]
    pub update: AgentUpdate,
}

impl AgentNotification {
    pub fn new(session_id: impl Into<String>, update: AgentUpdate) -> Self {
        Self {
            session_id: session_id.into(),
            update,
        }
    }

    pub fn with_session_id(&self, session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            update: self.update.clone(),
        }
    }
}

/// Normalized agent update payload used throughout the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentUpdate {
    TextDelta {
        content: String,
    },
    ThinkingDelta {
        content: String,
    },
    ToolUpdate {
        tool_call_id: String,
        title: String,
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
    },
    Plan {
        plan_json: Value,
    },
    Raw {
        payload: Value,
    },
}

/// Normalized prompt completion result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPromptResult {
    pub stop_reason: String,
}

impl AgentPromptResult {
    pub fn new(stop_reason: impl Into<String>) -> Self {
        Self {
            stop_reason: stop_reason.into(),
        }
    }
}

/// Common runtime interface implemented by each agent backend.
#[async_trait(?Send)]
pub trait AgentBackend {
    async fn initialize(&self) -> Result<(), String>;
    async fn new_session(&self, cwd: PathBuf) -> Result<String, String>;

    /// Discovers settings that depend on a session or on a selected model.
    ///
    /// Static backends can leave this as a no-op. ACP agents use it to expose
    /// selectors before the client creates its first user-facing session.
    async fn discover_settings(
        &self,
        _cwd: PathBuf,
        _settings: AgentSessionSettings,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Creates a session and applies settings before the first prompt.
    async fn new_session_with_settings(
        &self,
        cwd: PathBuf,
        settings: AgentSessionSettings,
    ) -> Result<String, String> {
        let session_id = self.new_session(cwd).await?;
        self.set_session_settings(session_id.clone(), settings)
            .await?;
        Ok(session_id)
    }

    /// Sets the human-readable name of an upstream session when the backend
    /// supports it. Backends without a native session-name API can leave this
    /// as a no-op.
    async fn set_session_name(&self, _session_id: String, _name: String) -> Result<(), String> {
        Ok(())
    }

    /// Changes settings for the next turn of a session.
    ///
    /// Backends that do not support runtime settings reject non-empty values
    /// instead of silently pretending that the UI change took effect.
    async fn set_session_settings(
        &self,
        _session_id: String,
        settings: AgentSessionSettings,
    ) -> Result<(), String> {
        if settings.model.is_some() || settings.reasoning_effort.is_some() {
            Err("agent backend does not support runtime session settings".into())
        } else {
            Ok(())
        }
    }

    /// Returns settings discovered from the backend's runtime protocol.
    ///
    /// Backends with static configuration can leave this empty; the agent
    /// manager will still expose settings declared in the agent config.
    fn setting_options(&self) -> Vec<AgentSettingOption> {
        Vec::new()
    }

    async fn prompt(&self, session_id: String, text: String) -> Result<AgentPromptResult, String>;
    async fn cancel(&self, session_id: String) -> Result<(), String>;
    fn take_update_rx(&self) -> Option<mpsc::UnboundedReceiver<AgentNotification>>;
    fn subscribe_health(&self) -> watch::Receiver<bool>;
    fn is_alive(&self) -> bool;
    async fn shutdown(&self);
}
