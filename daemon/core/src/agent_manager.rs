use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use tracing::info;

use agentchat_protocol::AgentConfig;

use crate::acp_client::AcpAgent;

/// Manages ACP agent instances.
///
/// For M0: single agent support. Post-M0: multi-agent with GroupChat routing.
pub struct AgentManager {
    agents: HashMap<String, Rc<AcpAgent>>,
    /// Map session IDs to agent IDs for routing.
    session_to_agent: HashMap<String, String>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            session_to_agent: HashMap::new(),
        }
    }

    /// Spawn and initialize an ACP agent from config.
    pub async fn add_agent(
        &mut self,
        config: AgentConfig,
        project_root: PathBuf,
    ) -> Result<(), String> {
        let agent_id = config.id.clone();
        let agent = Rc::new(AcpAgent::spawn(&config, project_root)?);

        agent
            .initialize()
            .await
            .map_err(|e| format!("ACP init failed: {e}"))?;

        self.agents.insert(agent_id.clone(), agent);
        info!("agent '{}' registered and initialized", agent_id);
        Ok(())
    }

    /// Get an agent by ID.
    pub fn get_agent(&self, agent_id: &str) -> Option<Rc<AcpAgent>> {
        self.agents.get(agent_id).cloned()
    }

    /// Get the first registered agent ID.
    ///
    /// M0 only supports a single configured agent, so any registered agent is valid.
    pub fn first_agent_id(&self) -> Option<&str> {
        self.agents.keys().next().map(|id| id.as_str())
    }

    /// Register a session -> agent mapping.
    pub fn register_session(&mut self, session_id: String, agent_id: String) {
        self.session_to_agent.insert(session_id, agent_id);
    }

    /// Look up which agent owns a session.
    pub fn agent_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_to_agent.get(session_id).map(|s| s.as_str())
    }

    /// Remove a session -> agent mapping.
    pub fn remove_session(&mut self, session_id: &str) -> Option<String> {
        self.session_to_agent.remove(session_id)
    }

    /// Remove multiple session -> agent mappings.
    pub fn remove_sessions(&mut self, session_ids: &[String]) {
        for session_id in session_ids {
            self.session_to_agent.remove(session_id);
        }
    }

    /// Report whether the agent process is still alive.
    pub fn is_agent_alive(&self, agent_id: &str) -> bool {
        self.agents
            .get(agent_id)
            .map(|agent| agent.is_alive())
            .unwrap_or(false)
    }

    /// Shutdown all agents.
    pub fn shutdown_all(&self) -> impl std::future::Future<Output = ()> + 'static {
        let agents = self
            .agents
            .iter()
            .map(|(id, agent)| (id.clone(), Rc::clone(agent)))
            .collect::<Vec<_>>();

        async move {
            for (id, agent) in agents {
                info!("shutting down agent '{}'", id);
                agent.shutdown().await;
            }
        }
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_agent_id_is_none_when_empty() {
        let manager = AgentManager::new();

        assert_eq!(manager.first_agent_id(), None);
    }

    #[test]
    fn register_lookup_and_remove_session() {
        let mut manager = AgentManager::new();
        manager.register_session("session-1".into(), "agent-1".into());

        assert_eq!(manager.agent_for_session("session-1"), Some("agent-1"));
        assert_eq!(manager.remove_session("session-1"), Some("agent-1".into()));
        assert_eq!(manager.agent_for_session("session-1"), None);
    }

    #[test]
    fn remove_sessions_clears_multiple_mappings() {
        let mut manager = AgentManager::new();
        manager.register_session("session-1".into(), "agent-1".into());
        manager.register_session("session-2".into(), "agent-1".into());

        manager.remove_sessions(&["session-1".into(), "session-2".into()]);

        assert_eq!(manager.agent_for_session("session-1"), None);
        assert_eq!(manager.agent_for_session("session-2"), None);
    }
}
