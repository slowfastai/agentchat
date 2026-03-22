use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Daemon-side session tracking info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub agent_id: String,
    pub working_dir: String,
    pub created_at: u64,
}

/// In-memory session store. Will be backed by SQLite in the future.
pub struct SessionStore {
    /// session_id -> SessionRecord
    sessions: HashMap<String, SessionRecord>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn insert(&mut self, record: SessionRecord) {
        self.sessions.insert(record.session_id.clone(), record);
    }

    pub fn get(&self, session_id: &str) -> Option<&SessionRecord> {
        self.sessions.get(session_id)
    }

    pub fn list_by_agent(&self, agent_id: &str) -> Vec<&SessionRecord> {
        self.sessions
            .values()
            .filter(|s| s.agent_id == agent_id)
            .collect()
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
