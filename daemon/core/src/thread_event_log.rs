use std::collections::HashMap;
use std::path::{Path, PathBuf};

use agentchat_protocol::ResponseEvent;

/// In-memory journal of thread-scoped app-facing events, with stable per-thread sequence numbers.
pub struct ThreadEventLog {
    next_seq: HashMap<String, u64>,
    events: HashMap<String, Vec<ResponseEvent>>,
    threads_dir: PathBuf,
}

impl ThreadEventLog {
    pub fn new(threads_dir: impl Into<PathBuf>) -> Self {
        Self {
            next_seq: HashMap::new(),
            events: HashMap::new(),
            threads_dir: threads_dir.into(),
        }
    }

    pub fn init_thread(&mut self, thread_id: &str) {
        self.next_seq.entry(thread_id.to_string()).or_insert(1);
        self.events.entry(thread_id.to_string()).or_default();
    }

    pub fn next_seq(&mut self, thread_id: &str) -> u64 {
        let next_seq = self.next_seq.entry(thread_id.to_string()).or_insert(1);
        let current = *next_seq;
        *next_seq = next_seq.saturating_add(1);
        current
    }

    pub fn append(&mut self, event: ResponseEvent) {
        let Some(thread_id) = event.thread_id().map(|id| id.to_string()) else {
            return;
        };

        self.events.entry(thread_id).or_default().push(event);
    }

    pub fn tail_seq(&self, thread_id: &str) -> u64 {
        self.events
            .get(thread_id)
            .and_then(|events| events.last())
            .and_then(ResponseEvent::thread_seq)
            .unwrap_or(0)
    }

    pub fn replay_after(&self, thread_id: &str, after_seq: u64) -> Vec<ResponseEvent> {
        self.events
            .get(thread_id)
            .into_iter()
            .flat_map(|events| events.iter())
            .filter(|event| event.thread_seq().unwrap_or(0) > after_seq)
            .cloned()
            .collect()
    }

    pub fn event_log_path(&self, thread_id: &str) -> PathBuf {
        self.threads_dir.join(format!("{thread_id}.events.jsonl"))
    }

    pub fn threads_dir(&self) -> &Path {
        &self.threads_dir
    }
}

impl Default for ThreadEventLog {
    fn default() -> Self {
        Self::new(PathBuf::from(".agentchat").join("threads"))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use agentchat_protocol::{DeltaType, ParticipantKind, ResponseEvent, ThreadSender};

    use super::*;

    #[test]
    fn next_seq_starts_at_one_per_thread() {
        let root = tempdir().unwrap();
        let mut log = ThreadEventLog::new(root.path());

        assert_eq!(log.next_seq("thread-1"), 1);
        assert_eq!(log.next_seq("thread-1"), 2);
        assert_eq!(log.next_seq("thread-2"), 1);
    }

    #[test]
    fn append_updates_tail_and_replay_after_filters_by_seq() {
        let root = tempdir().unwrap();
        let mut log = ThreadEventLog::new(root.path());
        log.append(ResponseEvent::ThreadMessage {
            thread_id: "thread-1".into(),
            thread_seq: 1,
            message_id: "message-1".into(),
            sender: ThreadSender {
                kind: ParticipantKind::Human,
                participant_id: "participant-user".into(),
                display_name: "You".into(),
            },
            content: "hello".into(),
            target_participant_ids: vec!["participant-1".into()],
        });
        log.append(ResponseEvent::ThreadAgentDelta {
            thread_id: "thread-1".into(),
            thread_seq: 2,
            participant_id: "participant-1".into(),
            agent_id: "agent-1".into(),
            session_id: "session-1".into(),
            session_event_seq: 3,
            content: "chunk".into(),
            delta_type: DeltaType::Text,
        });

        assert_eq!(log.tail_seq("thread-1"), 2);
        assert_eq!(log.replay_after("thread-1", 0).len(), 2);
        assert_eq!(log.replay_after("thread-1", 1).len(), 1);
        assert_eq!(log.replay_after("thread-1", 2).len(), 0);
    }
}
