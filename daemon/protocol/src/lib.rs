use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================
// Agent Configuration
// ============================================================

/// Configuration for an ACP agent. Specifies how to spawn the agent subprocess.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique identifier for this agent.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Command to spawn the agent subprocess (e.g., "claude-agent").
    pub command: String,
    /// Arguments for the agent command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory for sessions.
    pub working_dir: Option<String>,
    /// Environment variables to set for the agent subprocess.
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    /// Extra configuration (agent-specific).
    #[serde(default)]
    pub extra: HashMap<String, Value>,
}

// ============================================================
// Session types
// ============================================================

/// Session status from the daemon's perspective.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Active,
    Idle,
    Completed,
    Error(String),
}

// ============================================================
// Response events (daemon → iOS app via WebSocket)
// ============================================================

/// Events streamed from daemon to the iOS app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseEvent {
    /// Session created successfully.
    SessionCreated { session_id: String },

    /// Incremental text from the agent.
    Delta {
        session_id: String,
        content: String,
        delta_type: DeltaType,
    },

    /// Agent's execution plan update.
    PlanUpdate {
        session_id: String,
        plan_json: Value,
    },

    /// Tool call status update.
    ToolUpdate {
        session_id: String,
        tool_call_id: String,
        title: String,
        status: String,
        content: Option<String>,
    },

    /// Prompt turn completed.
    TurnEnd {
        session_id: String,
        stop_reason: String,
    },

    /// Error.
    Error {
        session_id: Option<String>,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaType {
    Text,
    Thinking,
    ToolUse,
}

// ============================================================
// WebSocket protocol messages (iOS app ↔ daemon)
// ============================================================

/// Messages the iOS app sends to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Create a new session.
    CreateSession { working_dir: String },
    /// Send a prompt.
    Prompt { session_id: String, content: String },
    /// Cancel an ongoing prompt turn.
    Cancel { session_id: String },
}
