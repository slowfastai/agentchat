use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, watch};

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
    async fn prompt(&self, session_id: String, text: String) -> Result<AgentPromptResult, String>;
    async fn cancel(&self, session_id: String) -> Result<(), String>;
    fn take_update_rx(&self) -> Option<mpsc::UnboundedReceiver<AgentNotification>>;
    fn subscribe_health(&self) -> watch::Receiver<bool>;
    fn is_alive(&self) -> bool;
    async fn shutdown(&self);
}
