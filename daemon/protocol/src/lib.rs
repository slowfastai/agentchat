use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================
// Agent Configuration
// ============================================================

/// Configuration for an ACP agent. Specifies how to spawn the agent subprocess.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
// Response events (daemon -> iOS app via WebSocket)
// ============================================================

/// Events streamed from daemon to the iOS app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeltaType {
    Text,
    Thinking,
    ToolUse,
}

// ============================================================
// WebSocket protocol messages (iOS app <-> daemon)
// ============================================================

/// Messages the iOS app sends to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Create a new session.
    CreateSession { working_dir: String },
    /// Send a prompt.
    Prompt { session_id: String, content: String },
    /// Cancel an ongoing prompt turn.
    Cancel { session_id: String },
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::de::DeserializeOwned;
    use serde_json::json;

    use super::*;

    fn assert_round_trip<T>(value: &T)
    where
        T: serde::Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_string(value).unwrap();
        let decoded: T = serde_json::from_str(&json).unwrap();
        assert_eq!(&decoded, value);
    }

    #[test]
    fn client_messages_round_trip_through_json() {
        let messages = [
            ClientMessage::CreateSession {
                working_dir: "/tmp/project".into(),
            },
            ClientMessage::Prompt {
                session_id: "session-1".into(),
                content: "hello".into(),
            },
            ClientMessage::Cancel {
                session_id: "session-1".into(),
            },
        ];

        for message in messages {
            assert_round_trip(&message);
        }
    }

    #[test]
    fn response_events_round_trip_through_json() {
        let events = [
            ResponseEvent::SessionCreated {
                session_id: "session-1".into(),
            },
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                content: "chunk".into(),
                delta_type: DeltaType::ToolUse,
            },
            ResponseEvent::PlanUpdate {
                session_id: "session-1".into(),
                plan_json: json!({"steps": [{"title": "Inspect"}], "done": false}),
            },
            ResponseEvent::ToolUpdate {
                session_id: "session-1".into(),
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "Completed".into(),
                content: Some("ok".into()),
            },
            ResponseEvent::TurnEnd {
                session_id: "session-1".into(),
                stop_reason: "end_turn".into(),
            },
            ResponseEvent::Error {
                session_id: Some("session-1".into()),
                code: "prompt_failed".into(),
                message: "boom".into(),
            },
        ];

        for event in events {
            assert_round_trip(&event);
        }
    }

    #[test]
    fn delta_type_serializes_in_snake_case() {
        let json = serde_json::to_string(&DeltaType::ToolUse).unwrap();

        assert_eq!(json, "\"tool_use\"");
        assert_eq!(
            serde_json::from_str::<DeltaType>(&json).unwrap(),
            DeltaType::ToolUse
        );
    }

    #[test]
    fn unknown_type_tags_fail_to_deserialize() {
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"unknown"}"#).is_err());
        assert!(serde_json::from_str::<ResponseEvent>(r#"{"type":"unknown"}"#).is_err());
    }
}
