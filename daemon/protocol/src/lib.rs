pub mod relay;
pub mod relay_crypto;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================
// Agent Configuration
// ============================================================

/// Configuration for an agent backend. Specifies how to spawn the agent subprocess.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentConfig {
    /// Unique identifier for this agent.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Backend adapter used to talk to the agent process.
    #[serde(default = "default_agent_backend")]
    pub backend: String,
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

fn default_agent_backend() -> String {
    "acp".to_string()
}

/// Timestamp helper in milliseconds since the UNIX epoch.
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A captured event in a session trajectory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEvent {
    UserPrompt {
        content: String,
        timestamp_ms: u64,
    },
    AgentUpdate {
        notification_json: Value,
        timestamp_ms: u64,
    },
    TurnEnd {
        stop_reason: String,
        timestamp_ms: u64,
    },
}

/// Full trajectory for one session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscript {
    pub session_id: String,
    pub agent_id: String,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub events: Vec<SessionEvent>,
}

/// Metadata about a stored skill file, including shared and agent-specific namespaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillInfo {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
}

/// Coarse liveness state for a configured agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Online,
    Offline,
    Starting,
    Crashed,
}

/// Compact summary of a configured daemon agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSummary {
    pub agent_id: String,
    pub name: String,
    pub kind: String,
    pub status: AgentStatus,
    pub default_working_dir: Option<String>,
    pub capabilities: Vec<String>,
}

/// Coarse session execution state exposed to reconnecting clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Prompting,
}

/// Coarse thread execution state exposed to reconnecting clients.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadState {
    Idle,
    Prompting,
}

/// Participant kind for thread snapshots and events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantKind {
    Human,
    Agent,
}

/// Coarse participant execution state inside a thread.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantState {
    Idle,
    Prompting,
    Offline,
    Error,
}

/// Compact summary of an attachable daemon session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: String,
    pub agent_id: String,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub state: SessionState,
    pub last_event_seq: u64,
    pub last_stop_reason: Option<String>,
}

/// Snapshot used by clients to rebuild session UI after reconnect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub agent_id: String,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub state: SessionState,
    pub last_event_seq: u64,
    pub last_stop_reason: Option<String>,
    pub last_error: Option<String>,
}

/// Compact summary of an attachable daemon thread.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSummary {
    pub thread_id: String,
    pub title: Option<String>,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub state: ThreadState,
    pub participant_count: u32,
    pub last_thread_seq: u64,
}

/// Participant visible in a thread snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadParticipant {
    pub participant_id: String,
    pub kind: ParticipantKind,
    pub display_name: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub state: ParticipantState,
}

/// Snapshot used by clients to rebuild thread UI after attach.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub title: Option<String>,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub last_thread_seq: u64,
    pub participants: Vec<ThreadParticipant>,
}

/// Sender metadata for a thread-scoped user message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadSender {
    pub kind: ParticipantKind,
    pub participant_id: String,
    pub display_name: String,
}

/// Delivery state for an assistant message snapshot in a thread timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantMessageState {
    Streaming,
    Completed,
    Failed,
}

// ============================================================
// Response events (daemon -> iOS app via WebSocket)
// ============================================================

/// Events streamed from daemon to the iOS app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseEvent {
    /// Session created successfully.
    SessionCreated {
        session_id: String,
        agent_id: String,
        event_seq: u64,
    },

    /// List of configured agents known to the daemon.
    AgentList { agents: Vec<AgentSummary> },

    /// Thread created successfully.
    ThreadCreated {
        thread_id: String,
        created_at_ms: u64,
    },

    /// List of live daemon threads.
    ThreadList { threads: Vec<ThreadSummary> },

    /// Thread attachment acknowledged.
    ThreadAttached { thread_id: String },

    /// Compact snapshot of a live thread.
    ThreadSnapshot { snapshot: ThreadSnapshot },

    /// Thread closed and removed from the daemon's live thread set.
    ThreadClosed { thread_id: String },

    /// Replay handoff is complete and live thread streaming resumes.
    ThreadReplayComplete {
        thread_id: String,
        last_thread_seq: u64,
    },

    /// Agent participant added to a thread.
    ThreadParticipantAdded {
        thread_id: String,
        thread_seq: u64,
        participant: ThreadParticipant,
    },

    /// Participant removed from a thread.
    ThreadParticipantRemoved {
        thread_id: String,
        thread_seq: u64,
        participant_id: String,
    },

    /// List of live sessions known to the daemon.
    SessionList { sessions: Vec<SessionSummary> },

    /// Session attachment acknowledged.
    SessionAttached { session_id: String },

    /// Compact snapshot of a live session.
    SessionSnapshot { snapshot: SessionSnapshot },

    /// Session closed and removed from the daemon.
    SessionClosed { session_id: String },

    /// Replay handoff is complete and live streaming resumes.
    SessionReplayComplete {
        session_id: String,
        last_event_seq: u64,
    },

    /// User message recorded in a thread timeline.
    ThreadMessage {
        thread_id: String,
        thread_seq: u64,
        message_id: String,
        sender: ThreadSender,
        content: String,
        target_participant_ids: Vec<String>,
    },

    /// Snapshot of a single assistant reply inside a thread.
    ThreadAssistantMessage {
        thread_id: String,
        thread_seq: u64,
        message_id: String,
        turn_id: String,
        participant_id: String,
        agent_id: String,
        session_id: String,
        session_event_seq: u64,
        thinking: String,
        response: String,
        state: AssistantMessageState,
        stop_reason: Option<String>,
    },

    /// Incremental text from an agent participant inside a thread.
    ThreadAgentDelta {
        thread_id: String,
        thread_seq: u64,
        participant_id: String,
        agent_id: String,
        session_id: String,
        session_event_seq: u64,
        content: String,
        delta_type: DeltaType,
    },

    /// Agent plan update inside a thread.
    ThreadAgentPlanUpdate {
        thread_id: String,
        thread_seq: u64,
        participant_id: String,
        agent_id: String,
        session_id: String,
        session_event_seq: u64,
        plan_json: Value,
    },

    /// Tool call status update inside a thread.
    ThreadAgentToolUpdate {
        thread_id: String,
        thread_seq: u64,
        participant_id: String,
        agent_id: String,
        session_id: String,
        session_event_seq: u64,
        tool_call_id: String,
        title: String,
        status: String,
        content: Option<String>,
    },

    /// Agent turn completed inside a thread.
    ThreadAgentTurnEnd {
        thread_id: String,
        thread_seq: u64,
        participant_id: String,
        agent_id: String,
        session_id: String,
        session_event_seq: u64,
        stop_reason: String,
    },

    /// Incremental text from the agent.
    Delta {
        session_id: String,
        event_seq: u64,
        content: String,
        delta_type: DeltaType,
    },

    /// Agent's execution plan update.
    PlanUpdate {
        session_id: String,
        event_seq: u64,
        plan_json: Value,
    },

    /// Tool call status update.
    ToolUpdate {
        session_id: String,
        event_seq: u64,
        tool_call_id: String,
        title: String,
        status: String,
        content: Option<String>,
    },

    /// Prompt turn completed.
    TurnEnd {
        session_id: String,
        event_seq: u64,
        stop_reason: String,
    },

    /// Available skills in the project, including shared and agent-specific namespaces.
    SkillList { skills: Vec<SkillInfo> },

    /// Raw markdown content for a skill file.
    SkillContent { name: String, content: String },

    /// Progress update for knowledge distillation.
    DistillationStatus {
        session_id: String,
        event_seq: u64,
        status: String,
        message: String,
    },

    /// Error.
    Error {
        session_id: Option<String>,
        event_seq: Option<u64>,
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

impl ResponseEvent {
    pub fn session_id(&self) -> Option<&str> {
        match self {
            ResponseEvent::SessionCreated { session_id, .. }
            | ResponseEvent::SessionAttached { session_id, .. }
            | ResponseEvent::SessionClosed { session_id, .. }
            | ResponseEvent::SessionReplayComplete { session_id, .. }
            | ResponseEvent::Delta { session_id, .. }
            | ResponseEvent::PlanUpdate { session_id, .. }
            | ResponseEvent::ToolUpdate { session_id, .. }
            | ResponseEvent::TurnEnd { session_id, .. }
            | ResponseEvent::DistillationStatus { session_id, .. } => Some(session_id),
            ResponseEvent::SessionSnapshot { snapshot, .. } => Some(&snapshot.session_id),
            ResponseEvent::Error { session_id, .. } => session_id.as_deref(),
            ResponseEvent::AgentList { .. }
            | ResponseEvent::ThreadCreated { .. }
            | ResponseEvent::ThreadList { .. }
            | ResponseEvent::ThreadAttached { .. }
            | ResponseEvent::ThreadSnapshot { .. }
            | ResponseEvent::ThreadClosed { .. }
            | ResponseEvent::ThreadReplayComplete { .. }
            | ResponseEvent::ThreadParticipantAdded { .. }
            | ResponseEvent::ThreadParticipantRemoved { .. }
            | ResponseEvent::ThreadMessage { .. }
            | ResponseEvent::ThreadAssistantMessage { .. }
            | ResponseEvent::ThreadAgentDelta { .. }
            | ResponseEvent::ThreadAgentPlanUpdate { .. }
            | ResponseEvent::ThreadAgentToolUpdate { .. }
            | ResponseEvent::ThreadAgentTurnEnd { .. }
            | ResponseEvent::SessionList { .. }
            | ResponseEvent::SkillList { .. }
            | ResponseEvent::SkillContent { .. } => None,
        }
    }

    pub fn thread_id(&self) -> Option<&str> {
        match self {
            ResponseEvent::ThreadCreated { thread_id, .. }
            | ResponseEvent::ThreadAttached { thread_id, .. }
            | ResponseEvent::ThreadClosed { thread_id, .. }
            | ResponseEvent::ThreadReplayComplete { thread_id, .. }
            | ResponseEvent::ThreadParticipantAdded { thread_id, .. }
            | ResponseEvent::ThreadParticipantRemoved { thread_id, .. }
            | ResponseEvent::ThreadMessage { thread_id, .. }
            | ResponseEvent::ThreadAssistantMessage { thread_id, .. }
            | ResponseEvent::ThreadAgentDelta { thread_id, .. }
            | ResponseEvent::ThreadAgentPlanUpdate { thread_id, .. }
            | ResponseEvent::ThreadAgentToolUpdate { thread_id, .. }
            | ResponseEvent::ThreadAgentTurnEnd { thread_id, .. } => Some(thread_id),
            ResponseEvent::ThreadSnapshot { snapshot, .. } => Some(&snapshot.thread_id),
            ResponseEvent::SessionCreated { .. }
            | ResponseEvent::AgentList { .. }
            | ResponseEvent::ThreadList { .. }
            | ResponseEvent::SessionList { .. }
            | ResponseEvent::SessionAttached { .. }
            | ResponseEvent::SessionSnapshot { .. }
            | ResponseEvent::SessionClosed { .. }
            | ResponseEvent::SessionReplayComplete { .. }
            | ResponseEvent::Delta { .. }
            | ResponseEvent::PlanUpdate { .. }
            | ResponseEvent::ToolUpdate { .. }
            | ResponseEvent::TurnEnd { .. }
            | ResponseEvent::SkillList { .. }
            | ResponseEvent::SkillContent { .. }
            | ResponseEvent::DistillationStatus { .. }
            | ResponseEvent::Error { .. } => None,
        }
    }

    pub fn event_seq(&self) -> Option<u64> {
        match self {
            ResponseEvent::SessionCreated { event_seq, .. }
            | ResponseEvent::Delta { event_seq, .. }
            | ResponseEvent::PlanUpdate { event_seq, .. }
            | ResponseEvent::ToolUpdate { event_seq, .. }
            | ResponseEvent::TurnEnd { event_seq, .. }
            | ResponseEvent::DistillationStatus { event_seq, .. } => Some(*event_seq),
            ResponseEvent::Error { event_seq, .. } => *event_seq,
            ResponseEvent::SessionAttached { .. }
            | ResponseEvent::SessionSnapshot { .. }
            | ResponseEvent::SessionClosed { .. }
            | ResponseEvent::SessionReplayComplete { .. }
            | ResponseEvent::AgentList { .. }
            | ResponseEvent::ThreadCreated { .. }
            | ResponseEvent::ThreadList { .. }
            | ResponseEvent::ThreadAttached { .. }
            | ResponseEvent::ThreadSnapshot { .. }
            | ResponseEvent::ThreadClosed { .. }
            | ResponseEvent::ThreadReplayComplete { .. }
            | ResponseEvent::ThreadParticipantAdded { .. }
            | ResponseEvent::ThreadParticipantRemoved { .. }
            | ResponseEvent::ThreadMessage { .. }
            | ResponseEvent::ThreadAssistantMessage { .. }
            | ResponseEvent::ThreadAgentDelta { .. }
            | ResponseEvent::ThreadAgentPlanUpdate { .. }
            | ResponseEvent::ThreadAgentToolUpdate { .. }
            | ResponseEvent::ThreadAgentTurnEnd { .. }
            | ResponseEvent::SessionList { .. }
            | ResponseEvent::SkillList { .. }
            | ResponseEvent::SkillContent { .. } => None,
        }
    }

    pub fn thread_seq(&self) -> Option<u64> {
        match self {
            ResponseEvent::ThreadParticipantAdded { thread_seq, .. }
            | ResponseEvent::ThreadParticipantRemoved { thread_seq, .. }
            | ResponseEvent::ThreadMessage { thread_seq, .. }
            | ResponseEvent::ThreadAssistantMessage { thread_seq, .. }
            | ResponseEvent::ThreadAgentDelta { thread_seq, .. }
            | ResponseEvent::ThreadAgentPlanUpdate { thread_seq, .. }
            | ResponseEvent::ThreadAgentToolUpdate { thread_seq, .. }
            | ResponseEvent::ThreadAgentTurnEnd { thread_seq, .. } => Some(*thread_seq),
            ResponseEvent::SessionCreated { .. }
            | ResponseEvent::AgentList { .. }
            | ResponseEvent::ThreadCreated { .. }
            | ResponseEvent::ThreadList { .. }
            | ResponseEvent::ThreadAttached { .. }
            | ResponseEvent::ThreadSnapshot { .. }
            | ResponseEvent::ThreadClosed { .. }
            | ResponseEvent::ThreadReplayComplete { .. }
            | ResponseEvent::SessionList { .. }
            | ResponseEvent::SessionAttached { .. }
            | ResponseEvent::SessionSnapshot { .. }
            | ResponseEvent::SessionClosed { .. }
            | ResponseEvent::SessionReplayComplete { .. }
            | ResponseEvent::Delta { .. }
            | ResponseEvent::PlanUpdate { .. }
            | ResponseEvent::ToolUpdate { .. }
            | ResponseEvent::TurnEnd { .. }
            | ResponseEvent::SkillList { .. }
            | ResponseEvent::SkillContent { .. }
            | ResponseEvent::DistillationStatus { .. }
            | ResponseEvent::Error { .. } => None,
        }
    }
}

// ============================================================
// WebSocket protocol messages (iOS app <-> daemon)
// ============================================================

/// Messages the iOS app sends to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Create a new session.
    CreateSession {
        #[serde(default)]
        agent_id: Option<String>,
        working_dir: String,
    },
    /// List configured agents known to the daemon.
    ListAgents,
    /// Create a new thread.
    CreateThread {
        #[serde(default)]
        title: Option<String>,
        working_dir: String,
    },
    /// List live threads known to the daemon.
    ListThreads,
    /// Attach the current client connection to an existing thread.
    AttachThread {
        thread_id: String,
        #[serde(default)]
        after_seq: Option<u64>,
    },
    /// Add a new agent-backed participant to an existing thread.
    AddThreadParticipant { thread_id: String, agent_id: String },
    /// Remove a participant from an existing thread.
    RemoveThreadParticipant {
        thread_id: String,
        participant_id: String,
    },
    /// Close and remove a live thread from the daemon.
    CloseThread { thread_id: String },
    /// Send a user message into a thread and fan it out to selected participants.
    SendThreadMessage {
        thread_id: String,
        content: String,
        #[serde(default)]
        target_participant_ids: Option<Vec<String>>,
    },
    /// List live sessions known to the daemon.
    ListSessions,
    /// Attach the current client connection to an existing session.
    AttachSession {
        session_id: String,
        #[serde(default)]
        after_seq: Option<u64>,
    },
    /// Close and remove a live session from the daemon.
    CloseSession { session_id: String },
    /// Send a prompt.
    Prompt { session_id: String, content: String },
    /// Cancel an ongoing prompt turn.
    Cancel { session_id: String },
    /// List project skills, including `shared/` and `agents/<agent-id>/` namespaces.
    ListSkills,
    /// Read a project skill by relative name, such as `shared/testing.md` or `agents/opencode/testing.md`.
    GetSkill { name: String },
    /// Distill a captured session into reusable shared and agent-specific skills.
    DistillSession { session_id: String },
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
                agent_id: Some("agent-1".into()),
                working_dir: "/tmp/project".into(),
            },
            ClientMessage::ListAgents,
            ClientMessage::CreateThread {
                title: Some("Review".into()),
                working_dir: "/tmp/project".into(),
            },
            ClientMessage::ListThreads,
            ClientMessage::AttachThread {
                thread_id: "thread-1".into(),
                after_seq: Some(2),
            },
            ClientMessage::AddThreadParticipant {
                thread_id: "thread-1".into(),
                agent_id: "agent-1".into(),
            },
            ClientMessage::RemoveThreadParticipant {
                thread_id: "thread-1".into(),
                participant_id: "participant-1".into(),
            },
            ClientMessage::CloseThread {
                thread_id: "thread-1".into(),
            },
            ClientMessage::SendThreadMessage {
                thread_id: "thread-1".into(),
                content: "hello group".into(),
                target_participant_ids: Some(vec!["participant-1".into()]),
            },
            ClientMessage::ListSessions,
            ClientMessage::AttachSession {
                session_id: "session-1".into(),
                after_seq: Some(3),
            },
            ClientMessage::CloseSession {
                session_id: "session-1".into(),
            },
            ClientMessage::Prompt {
                session_id: "session-1".into(),
                content: "hello".into(),
            },
            ClientMessage::Cancel {
                session_id: "session-1".into(),
            },
            ClientMessage::ListSkills,
            ClientMessage::GetSkill {
                name: "testing.md".into(),
            },
            ClientMessage::DistillSession {
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
                agent_id: "agent-1".into(),
                event_seq: 1,
            },
            ResponseEvent::AgentList {
                agents: vec![AgentSummary {
                    agent_id: "agent-1".into(),
                    name: "Agent 1".into(),
                    kind: "test".into(),
                    status: AgentStatus::Online,
                    default_working_dir: Some("/tmp/project".into()),
                    capabilities: vec!["session".into(), "prompt".into()],
                }],
            },
            ResponseEvent::ThreadCreated {
                thread_id: "thread-1".into(),
                created_at_ms: 123,
            },
            ResponseEvent::ThreadList {
                threads: vec![ThreadSummary {
                    thread_id: "thread-1".into(),
                    title: Some("Review".into()),
                    working_dir: "/tmp/project".into(),
                    created_at_ms: 123,
                    state: ThreadState::Idle,
                    participant_count: 2,
                    last_thread_seq: 4,
                }],
            },
            ResponseEvent::ThreadAttached {
                thread_id: "thread-1".into(),
            },
            ResponseEvent::ThreadSnapshot {
                snapshot: ThreadSnapshot {
                    thread_id: "thread-1".into(),
                    title: Some("Review".into()),
                    working_dir: "/tmp/project".into(),
                    created_at_ms: 123,
                    last_thread_seq: 4,
                    participants: vec![ThreadParticipant {
                        participant_id: "participant-1".into(),
                        kind: ParticipantKind::Agent,
                        display_name: "Agent 1".into(),
                        agent_id: Some("agent-1".into()),
                        session_id: Some("session-1".into()),
                        state: ParticipantState::Idle,
                    }],
                },
            },
            ResponseEvent::ThreadClosed {
                thread_id: "thread-1".into(),
            },
            ResponseEvent::ThreadReplayComplete {
                thread_id: "thread-1".into(),
                last_thread_seq: 5,
            },
            ResponseEvent::ThreadParticipantAdded {
                thread_id: "thread-1".into(),
                thread_seq: 6,
                participant: ThreadParticipant {
                    participant_id: "participant-1".into(),
                    kind: ParticipantKind::Agent,
                    display_name: "Agent 1".into(),
                    agent_id: Some("agent-1".into()),
                    session_id: Some("session-1".into()),
                    state: ParticipantState::Idle,
                },
            },
            ResponseEvent::ThreadParticipantRemoved {
                thread_id: "thread-1".into(),
                thread_seq: 7,
                participant_id: "participant-1".into(),
            },
            ResponseEvent::ThreadMessage {
                thread_id: "thread-1".into(),
                thread_seq: 1,
                message_id: "message-1".into(),
                sender: ThreadSender {
                    kind: ParticipantKind::Human,
                    participant_id: "participant-user".into(),
                    display_name: "You".into(),
                },
                content: "hello group".into(),
                target_participant_ids: vec!["participant-1".into()],
            },
            ResponseEvent::ThreadAssistantMessage {
                thread_id: "thread-1".into(),
                thread_seq: 2,
                message_id: "message-2".into(),
                turn_id: "turn-1".into(),
                participant_id: "participant-1".into(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                session_event_seq: 5,
                thinking: "thinking".into(),
                response: "final response".into(),
                state: AssistantMessageState::Completed,
                stop_reason: Some("end_turn".into()),
            },
            ResponseEvent::ThreadAgentDelta {
                thread_id: "thread-1".into(),
                thread_seq: 3,
                participant_id: "participant-1".into(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                session_event_seq: 6,
                content: "chunk".into(),
                delta_type: DeltaType::Text,
            },
            ResponseEvent::ThreadAgentPlanUpdate {
                thread_id: "thread-1".into(),
                thread_seq: 4,
                participant_id: "participant-1".into(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                session_event_seq: 7,
                plan_json: json!({"steps": [{"title": "Inspect"}], "done": false}),
            },
            ResponseEvent::ThreadAgentToolUpdate {
                thread_id: "thread-1".into(),
                thread_seq: 5,
                participant_id: "participant-1".into(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                session_event_seq: 8,
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "Completed".into(),
                content: Some("ok".into()),
            },
            ResponseEvent::ThreadAgentTurnEnd {
                thread_id: "thread-1".into(),
                thread_seq: 6,
                participant_id: "participant-1".into(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                session_event_seq: 9,
                stop_reason: "end_turn".into(),
            },
            ResponseEvent::SessionList {
                sessions: vec![SessionSummary {
                    session_id: "session-1".into(),
                    agent_id: "agent-1".into(),
                    working_dir: "/tmp/project".into(),
                    created_at_ms: 123,
                    state: SessionState::Idle,
                    last_event_seq: 4,
                    last_stop_reason: Some("EndTurn".into()),
                }],
            },
            ResponseEvent::SessionAttached {
                session_id: "session-1".into(),
            },
            ResponseEvent::SessionSnapshot {
                snapshot: SessionSnapshot {
                    session_id: "session-1".into(),
                    agent_id: "agent-1".into(),
                    working_dir: "/tmp/project".into(),
                    created_at_ms: 123,
                    state: SessionState::Prompting,
                    last_event_seq: 4,
                    last_stop_reason: Some("EndTurn".into()),
                    last_error: None,
                },
            },
            ResponseEvent::SessionClosed {
                session_id: "session-1".into(),
            },
            ResponseEvent::SessionReplayComplete {
                session_id: "session-1".into(),
                last_event_seq: 4,
            },
            ResponseEvent::Delta {
                session_id: "session-1".into(),
                event_seq: 5,
                content: "chunk".into(),
                delta_type: DeltaType::ToolUse,
            },
            ResponseEvent::PlanUpdate {
                session_id: "session-1".into(),
                event_seq: 6,
                plan_json: json!({"steps": [{"title": "Inspect"}], "done": false}),
            },
            ResponseEvent::ToolUpdate {
                session_id: "session-1".into(),
                event_seq: 7,
                tool_call_id: "tool-1".into(),
                title: "Read file".into(),
                status: "Completed".into(),
                content: Some("ok".into()),
            },
            ResponseEvent::TurnEnd {
                session_id: "session-1".into(),
                event_seq: 8,
                stop_reason: "end_turn".into(),
            },
            ResponseEvent::SkillList {
                skills: vec![SkillInfo {
                    name: "testing.md".into(),
                    path: ".agentchat/skills/testing.md".into(),
                    size_bytes: 42,
                }],
            },
            ResponseEvent::SkillContent {
                name: "testing.md".into(),
                content: "# Testing".into(),
            },
            ResponseEvent::DistillationStatus {
                session_id: "session-1".into(),
                event_seq: 9,
                status: "completed".into(),
                message: "Updated 2 skills".into(),
            },
            ResponseEvent::Error {
                session_id: Some("session-1".into()),
                event_seq: Some(10),
                code: "prompt_failed".into(),
                message: "boom".into(),
            },
        ];

        for event in events {
            assert_round_trip(&event);
        }
    }

    #[test]
    fn session_events_round_trip_through_json() {
        let events = [
            SessionEvent::UserPrompt {
                content: "hello".into(),
                timestamp_ms: 1,
            },
            SessionEvent::AgentUpdate {
                notification_json: json!({"session_id": "session-1", "update": {"kind": "agent_message_chunk"}}),
                timestamp_ms: 2,
            },
            SessionEvent::TurnEnd {
                stop_reason: "EndTurn".into(),
                timestamp_ms: 3,
            },
        ];

        for event in events {
            assert_round_trip(&event);
        }
    }

    #[test]
    fn session_transcript_round_trips_through_json() {
        let transcript = SessionTranscript {
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            working_dir: "/tmp/project".into(),
            created_at_ms: 123,
            events: vec![SessionEvent::UserPrompt {
                content: "hello".into(),
                timestamp_ms: 456,
            }],
        };

        assert_round_trip(&transcript);
    }

    #[test]
    fn skill_info_round_trips_through_json() {
        let skill = SkillInfo {
            name: "testing.md".into(),
            path: ".agentchat/skills/testing.md".into(),
            size_bytes: 42,
        };

        assert_round_trip(&skill);
    }

    #[test]
    fn agent_summary_round_trips_through_json() {
        let agent = AgentSummary {
            agent_id: "agent-1".into(),
            name: "Agent 1".into(),
            kind: "test".into(),
            status: AgentStatus::Online,
            default_working_dir: Some("/tmp/project".into()),
            capabilities: vec!["session".into(), "prompt".into()],
        };

        assert_round_trip(&agent);
    }

    #[test]
    fn session_summary_round_trips_through_json() {
        let summary = SessionSummary {
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            working_dir: "/tmp/project".into(),
            created_at_ms: 123,
            state: SessionState::Idle,
            last_event_seq: 5,
            last_stop_reason: Some("EndTurn".into()),
        };

        assert_round_trip(&summary);
    }

    #[test]
    fn session_snapshot_round_trips_through_json() {
        let snapshot = SessionSnapshot {
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            working_dir: "/tmp/project".into(),
            created_at_ms: 123,
            state: SessionState::Prompting,
            last_event_seq: 5,
            last_stop_reason: Some("EndTurn".into()),
            last_error: None,
        };

        assert_round_trip(&snapshot);
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
    fn assistant_message_state_serializes_in_snake_case() {
        let json = serde_json::to_string(&AssistantMessageState::Completed).unwrap();

        assert_eq!(json, "\"completed\"");
        assert_eq!(
            serde_json::from_str::<AssistantMessageState>(&json).unwrap(),
            AssistantMessageState::Completed
        );
    }

    #[test]
    fn unknown_type_tags_fail_to_deserialize() {
        assert!(serde_json::from_str::<ClientMessage>(r#"{"type":"unknown"}"#).is_err());
        assert!(serde_json::from_str::<ResponseEvent>(r#"{"type":"unknown"}"#).is_err());
    }

    #[test]
    fn now_millis_returns_non_zero_timestamp() {
        assert!(now_millis() > 0);
    }
}
