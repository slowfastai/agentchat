use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::SessionId;
use tracing::info;

use agentchat_protocol::AgentConfig;

use crate::acp_client::AcpAgent;

/// Manages ACP agent instances.
///
/// For M0: single agent support. Post-M0: multi-agent with GroupChat routing.
pub struct AgentManager {
    agents: HashMap<String, AcpAgent>,
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
        let agent = AcpAgent::spawn(&config, project_root)?;

        agent
            .initialize()
            .await
            .map_err(|e| format!("ACP init failed: {e}"))?;

        self.agents.insert(agent_id.clone(), agent);
        info!("agent '{}' registered and initialized", agent_id);
        Ok(())
    }

    /// Get a mutable reference to an agent by ID.
    pub fn get_agent_mut(&mut self, agent_id: &str) -> Option<&mut AcpAgent> {
        self.agents.get_mut(agent_id)
    }

    /// Get an immutable reference to an agent by ID.
    pub fn get_agent(&self, agent_id: &str) -> Option<&AcpAgent> {
        self.agents.get(agent_id)
    }

    /// Get the first registered agent ID.
    ///
    /// M0 only supports a single configured agent, so any registered agent is valid.
    pub fn first_agent_id(&self) -> Option<&str> {
        self.agents.keys().next().map(|id| id.as_str())
    }

    /// Register a session → agent mapping.
    pub fn register_session(&mut self, session_id: String, agent_id: String) {
        self.session_to_agent.insert(session_id, agent_id);
    }

    /// Look up which agent owns a session.
    pub fn agent_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_to_agent.get(session_id).map(|s| s.as_str())
    }

    /// Get the agent for a session (mutable).
    pub fn agent_for_session_mut(&mut self, session_id: &str) -> Option<&mut AcpAgent> {
        let agent_id = self.session_to_agent.get(session_id)?.clone();
        self.agents.get_mut(&agent_id)
    }

    /// Cancel an ongoing prompt on a session.
    pub async fn cancel_session(&self, session_id: &str) -> Result<(), String> {
        let agent_id = self
            .session_to_agent
            .get(session_id)
            .ok_or_else(|| format!("no agent for session {session_id}"))?;
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| format!("agent {agent_id} not found"))?;
        agent
            .cancel(SessionId::new(session_id))
            .await
            .map_err(|e| format!("cancel failed: {e}"))
    }

    /// Shutdown all agents.
    pub async fn shutdown_all(&mut self) {
        for (id, agent) in self.agents.iter_mut() {
            info!("shutting down agent '{}'", id);
            agent.shutdown().await;
        }
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}
