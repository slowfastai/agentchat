use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use tracing::warn;

use agentchat_protocol::{now_millis, SessionEvent, SessionTranscript};

use crate::backend::AgentNotification;

/// Captures active session trajectories and persists them to disk.
pub struct SessionStore {
    transcripts: HashMap<String, SessionTranscript>,
    sessions_dir: PathBuf,
}

impl SessionStore {
    pub fn new(project_root: &Path) -> Self {
        Self {
            transcripts: HashMap::new(),
            sessions_dir: project_root.join(".agentchat").join("sessions"),
        }
    }

    pub fn start_session(&mut self, session_id: &str, agent_id: &str, working_dir: &str) {
        self.transcripts.insert(
            session_id.to_string(),
            SessionTranscript {
                session_id: session_id.to_string(),
                agent_id: agent_id.to_string(),
                working_dir: working_dir.to_string(),
                created_at_ms: now_millis(),
                events: Vec::new(),
            },
        );
    }

    pub fn record_prompt(&mut self, session_id: &str, content: &str) {
        let Some(transcript) = self.transcripts.get_mut(session_id) else {
            return;
        };

        transcript.events.push(SessionEvent::UserPrompt {
            content: content.to_string(),
            timestamp_ms: now_millis(),
        });
    }

    pub fn record_notification(&mut self, session_id: &str, notification: &AgentNotification) {
        let Some(transcript) = self.transcripts.get_mut(session_id) else {
            return;
        };

        let notification_json = match serde_json::to_value(notification) {
            Ok(json) => json,
            Err(e) => {
                warn!("failed to serialize session notification for {session_id}: {e}");
                return;
            }
        };

        transcript.events.push(SessionEvent::AgentUpdate {
            notification_json,
            timestamp_ms: now_millis(),
        });
    }

    pub fn record_turn_end(&mut self, session_id: &str, stop_reason: &str) {
        let Some(transcript) = self.transcripts.get_mut(session_id) else {
            return;
        };

        transcript.events.push(SessionEvent::TurnEnd {
            stop_reason: stop_reason.to_string(),
            timestamp_ms: now_millis(),
        });
    }

    pub fn flush_session(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<PathBuf, String>> + 'static {
        let session_id = session_id.to_string();
        let transcript = self.transcripts.get(&session_id).cloned();
        let path = self.sessions_dir.join(format!("{session_id}.json"));

        async move {
            let transcript =
                transcript.ok_or_else(|| format!("session not found: {session_id}"))?;
            let json = serde_json::to_string_pretty(&transcript)
                .map_err(|e| format!("failed to serialize transcript {session_id}: {e}"))?;

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("failed to create sessions dir: {e}"))?;
            }

            tokio::fs::write(&path, json)
                .await
                .map_err(|e| format!("failed to write transcript {session_id}: {e}"))?;

            Ok(path)
        }
    }

    pub fn get_transcript(&self, session_id: &str) -> Option<&SessionTranscript> {
        self.transcripts.get(session_id)
    }

    pub fn list_transcripts(&self) -> Vec<SessionTranscript> {
        self.transcripts.values().cloned().collect()
    }

    pub fn remove_session(&mut self, session_id: &str) -> Option<SessionTranscript> {
        self.transcripts.remove(session_id)
    }

    pub fn load_transcript(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<SessionTranscript, String>> + 'static {
        let path = self.sessions_dir.join(format!("{session_id}.json"));
        let session_id = session_id.to_string();

        async move {
            let json = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("failed to read transcript {session_id}: {e}"))?;

            serde_json::from_str(&json)
                .map_err(|e| format!("failed to parse transcript {session_id}: {e}"))
        }
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    use crate::backend::{AgentNotification, AgentUpdate};

    fn sample_notification() -> AgentNotification {
        AgentNotification::new(
            "session-1",
            AgentUpdate::TextDelta {
                content: "hello".into(),
            },
        )
    }

    #[test]
    fn start_session_creates_transcript_in_memory() {
        let root = tempdir().unwrap();
        let mut store = SessionStore::new(root.path());

        store.start_session("session-1", "agent-1", "/tmp/project");

        let transcript = store.get_transcript("session-1").unwrap();
        assert_eq!(transcript.session_id, "session-1");
        assert_eq!(transcript.agent_id, "agent-1");
        assert_eq!(transcript.working_dir, "/tmp/project");
        assert!(transcript.events.is_empty());
    }

    #[test]
    fn record_prompt_appends_user_prompt_event() {
        let root = tempdir().unwrap();
        let mut store = SessionStore::new(root.path());
        store.start_session("session-1", "agent-1", "/tmp/project");

        store.record_prompt("session-1", "hello");

        let transcript = store.get_transcript("session-1").unwrap();
        assert!(matches!(
            transcript.events.as_slice(),
            [SessionEvent::UserPrompt { content, .. }] if content == "hello"
        ));
    }

    #[test]
    fn record_notification_serializes_generic_notification() {
        let root = tempdir().unwrap();
        let mut store = SessionStore::new(root.path());
        let notification = sample_notification();
        store.start_session("session-1", "agent-1", "/tmp/project");

        store.record_notification("session-1", &notification);

        let transcript = store.get_transcript("session-1").unwrap();
        let SessionEvent::AgentUpdate {
            notification_json, ..
        } = &transcript.events[0]
        else {
            panic!("expected agent update event");
        };

        let decoded: AgentNotification = serde_json::from_value(notification_json.clone()).unwrap();
        assert_eq!(decoded.session_id.to_string(), "session-1");
        assert!(matches!(decoded.update, AgentUpdate::TextDelta { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flush_then_load_round_trips() {
        let root = tempdir().unwrap();
        let mut store = SessionStore::new(root.path());
        store.start_session("session-1", "agent-1", "/tmp/project");
        store.record_prompt("session-1", "hello");
        store.record_notification("session-1", &sample_notification());
        store.record_turn_end("session-1", "EndTurn");

        let original = store.get_transcript("session-1").unwrap().clone();
        let path = store.flush_session("session-1").await.unwrap();
        let loaded = store.load_transcript("session-1").await.unwrap();

        assert_eq!(
            path,
            root.path()
                .join(".agentchat")
                .join("sessions")
                .join("session-1.json")
        );
        assert_eq!(loaded, original);
    }
}
