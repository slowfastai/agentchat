use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agentchat_protocol::ResponseEvent;

/// In-memory journal of app-facing session events, with stable per-session sequence numbers.
pub struct SessionEventLog {
    next_seq: HashMap<String, u64>,
    events: HashMap<String, Vec<ResponseEvent>>,
    sessions_dir: PathBuf,
}

impl SessionEventLog {
    pub fn new(sessions_dir: impl Into<PathBuf>) -> Self {
        Self {
            next_seq: HashMap::new(),
            events: HashMap::new(),
            sessions_dir: sessions_dir.into(),
        }
    }

    pub fn init_session(&mut self, session_id: &str) {
        self.next_seq.entry(session_id.to_string()).or_insert(1);
        self.events.entry(session_id.to_string()).or_default();
    }

    pub fn next_seq(&mut self, session_id: &str) -> u64 {
        let next_seq = self.next_seq.entry(session_id.to_string()).or_insert(1);
        let current = *next_seq;
        *next_seq = next_seq.saturating_add(1);
        current
    }

    pub fn append(&mut self, event: ResponseEvent) {
        let Some(session_id) = event.session_id().map(|id| id.to_string()) else {
            return;
        };

        self.events.entry(session_id).or_default().push(event);
    }

    pub fn tail_seq(&self, session_id: &str) -> u64 {
        self.events
            .get(session_id)
            .and_then(|events| events.last())
            .and_then(ResponseEvent::event_seq)
            .unwrap_or(0)
    }

    pub fn replay_after(&self, session_id: &str, after_seq: u64) -> Vec<ResponseEvent> {
        self.events
            .get(session_id)
            .into_iter()
            .flat_map(|events| events.iter())
            .filter(|event| event.event_seq().unwrap_or(0) > after_seq)
            .cloned()
            .collect()
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.next_seq.remove(session_id);
        self.events.remove(session_id);
    }

    pub fn event_log_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.events.jsonl"))
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use agentchat_protocol::{DeltaType, ResponseEvent};

    use super::*;

    #[test]
    fn next_seq_starts_at_one_per_session() {
        let root = tempdir().unwrap();
        let mut log = SessionEventLog::new(root.path());

        assert_eq!(log.next_seq("session-1"), 1);
        assert_eq!(log.next_seq("session-1"), 2);
        assert_eq!(log.next_seq("session-2"), 1);
    }

    #[test]
    fn append_updates_tail_and_replay_after_filters_by_seq() {
        let root = tempdir().unwrap();
        let mut log = SessionEventLog::new(root.path());
        log.append(ResponseEvent::Delta {
            session_id: "session-1".into(),
            event_seq: 1,
            content: "hello".into(),
            delta_type: DeltaType::Text,
        });
        log.append(ResponseEvent::TurnEnd {
            session_id: "session-1".into(),
            event_seq: 2,
            stop_reason: "EndTurn".into(),
        });

        assert_eq!(log.tail_seq("session-1"), 2);
        assert_eq!(log.replay_after("session-1", 0).len(), 2);
        assert_eq!(log.replay_after("session-1", 1).len(), 1);
        assert_eq!(log.replay_after("session-1", 2).len(), 0);
    }
}
