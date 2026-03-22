use std::collections::HashMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

// ============================================================
// AgentAdapter Trait
// ============================================================

/// Every agent adapter must implement this trait.
#[async_trait]
pub trait AgentAdapter: Send + Sync {
    // --- Lifecycle ---

    /// Initialize the adapter (check CLI availability, version compatibility, etc.).
    /// Called once per configured agent when the daemon starts.
    async fn init(&mut self, config: AgentConfig) -> Result<AdapterInfo, AdapterError>;

    /// Health check — daemon calls this periodically to update agent online status.
    async fn health_check(&self) -> HealthStatus;

    /// Clean up resources (kill child processes, close file handles, etc.).
    /// Called when the daemon shuts down.
    async fn shutdown(&mut self) -> Result<(), AdapterError>;

    // --- Session management ---

    /// Create a new session.
    async fn create_session(
        &mut self,
        options: SessionOptions,
    ) -> Result<SessionInfo, AdapterError>;

    /// Resume an existing session (using the agent's native session resume capability).
    /// Default: returns Unsupported (not all agents support resume).
    async fn resume_session(
        &mut self,
        _session_id: &str,
    ) -> Result<SessionInfo, AdapterError> {
        Err(AdapterError::Unsupported("session resume".into()))
    }

    /// List resumable sessions.
    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AdapterError> {
        Ok(vec![])
    }

    // --- Core communication ---

    /// Send a prompt and return a streaming response channel.
    ///
    /// The adapter is responsible for:
    /// 1. Converting the prompt to the agent CLI's input format
    /// 2. Spawning / writing to the CLI subprocess
    /// 3. Reading streaming output from stdout
    /// 4. Parsing raw agent output into unified `ResponseEvent`s
    /// 5. Sending events through the returned channel sender
    async fn send_prompt(
        &mut self,
        session_id: &str,
        prompt: Prompt,
    ) -> Result<mpsc::Receiver<ResponseEvent>, AdapterError>;

    /// Abort the currently executing task.
    async fn abort(&mut self, session_id: &str) -> Result<(), AdapterError>;

    /// Get the current agent status.
    fn status(&self) -> AgentStatus;

    // --- Metadata (optional, have default implementations) ---

    /// Return the adapter's capability declarations.
    /// The daemon uses this to decide which features are available.
    fn capabilities(&self) -> AdapterCapabilities {
        AdapterCapabilities::default()
    }

    /// Get metadata for the current session (token usage, duration, etc.).
    async fn session_metadata(
        &self,
        _session_id: &str,
    ) -> Result<SessionMetadata, AdapterError> {
        Err(AdapterError::Unsupported("session metadata".into()))
    }
}

// ============================================================
// Configuration & Info
// ============================================================

/// Agent configuration (set by the user in the iOS App, passed to the daemon via WebSocket).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub adapter_type: String,
    pub cli_path: String,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub extra: HashMap<String, Value>,
}

/// Information returned after adapter initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInfo {
    pub adapter_type: String,
    pub version: String,
    pub cli_version: Option<String>,
    pub capabilities: AdapterCapabilities,
}

/// Adapter capability declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterCapabilities {
    pub session_resume: bool,
    pub streaming: bool,
    pub abort: bool,
    pub token_usage: bool,
    pub cost_tracking: bool,
    pub hooks: bool,
    pub concurrent_sessions: bool,
}

impl Default for AdapterCapabilities {
    fn default() -> Self {
        Self {
            session_resume: false,
            streaming: true,
            abort: true,
            token_usage: false,
            cost_tracking: false,
            hooks: false,
            concurrent_sessions: false,
        }
    }
}

// ============================================================
// Session types
// ============================================================

/// Options for creating a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOptions {
    pub working_dir: Option<String>,
    pub system_prompt: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
}

/// Basic session information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub created_at: u64,
    pub status: SessionStatus,
}

/// Session summary (for listing resumable sessions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: u64,
    pub last_active_at: u64,
    pub message_count: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Idle,
    Completed,
    Error(String),
}

/// Session metadata (optional capability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub total_duration_ms: u64,
    pub api_duration_ms: Option<u64>,
    pub num_turns: u32,
    pub token_usage: Option<TokenUsage>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
}

// ============================================================
// Prompt & Response
// ============================================================

/// A prompt sent to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub content: String,
    pub content_type: PromptContentType,
    pub context: Option<Vec<ContextMessage>>,
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PromptContentType {
    Text,
    FileRef(Vec<String>),
    CodeSnippet {
        code: String,
        language: Option<String>,
    },
}

/// A historical message in GroupChat context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: ContextRole,
    pub agent_name: Option<String>,
    pub content: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextRole {
    User,
    Agent,
}

/// Agent response events (streamed via channel).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseEvent {
    /// Response started.
    Start { session_id: String },

    /// Incremental text delta.
    Delta {
        content: String,
        delta_type: DeltaType,
    },

    /// Agent status changed (thinking → executing, etc.).
    StatusChange { status: AgentStatus },

    /// Response ended (includes full content and metadata).
    End {
        full_content: String,
        metadata: Option<SessionMetadata>,
    },

    /// Error.
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeltaType {
    Text,
    Thinking,
    ToolUse,
}

// ============================================================
// Status & Error
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Idle,
    Thinking,
    Executing,
    Streaming,
    Error(String),
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded(String),
    Unhealthy(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdapterError {
    CliNotFound(String),
    CliVersionMismatch { expected: String, found: String },
    SpawnFailed(String),
    ProcessCrashed { exit_code: Option<i32>, stderr: String },
    ParseError(String),
    Timeout { operation: String, duration_ms: u64 },
    SessionNotFound(String),
    Unsupported(String),
    Other(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliNotFound(path) => write!(f, "CLI not found: {path}"),
            Self::CliVersionMismatch { expected, found } => {
                write!(f, "CLI version mismatch: expected {expected}, found {found}")
            }
            Self::SpawnFailed(msg) => write!(f, "spawn failed: {msg}"),
            Self::ProcessCrashed { exit_code, stderr } => {
                write!(f, "process crashed (exit={exit_code:?}): {stderr}")
            }
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
            Self::Timeout { operation, duration_ms } => {
                write!(f, "timeout after {duration_ms}ms: {operation}")
            }
            Self::SessionNotFound(id) => write!(f, "session not found: {id}"),
            Self::Unsupported(feature) => write!(f, "unsupported: {feature}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AdapterError {}
