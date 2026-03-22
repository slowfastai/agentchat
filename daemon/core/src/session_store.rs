use std::collections::HashMap;

use agentchat_protocol::SessionInfo;

/// In-memory session store. Will be backed by SQLite in the future.
pub struct SessionStore {
    /// agent_id -> session_id -> SessionInfo
    sessions: HashMap<String, HashMap<String, SessionInfo>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Store a session.
    pub fn insert(&mut self, agent_id: &str, session: SessionInfo) {
        self.sessions
            .entry(agent_id.to_string())
            .or_default()
            .insert(session.session_id.clone(), session);
    }

    /// Get a session by agent ID and session ID.
    pub fn get(&self, agent_id: &str, session_id: &str) -> Option<&SessionInfo> {
        self.sessions.get(agent_id)?.get(session_id)
    }

    /// List all sessions for a given agent.
    pub fn list_by_agent(&self, agent_id: &str) -> Vec<&SessionInfo> {
        self.sessions
            .get(agent_id)
            .map(|m| m.values().collect())
            .unwrap_or_default()
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}
