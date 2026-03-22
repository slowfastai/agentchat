use std::collections::HashMap;

use oversight_protocol::{
    AdapterError, AgentAdapter, AgentConfig, Prompt, ResponseEvent, SessionInfo, SessionOptions,
};
use tokio::sync::mpsc;

/// Manages all agent adapter instances.
pub struct AgentManager {
    adapters: HashMap<String, Box<dyn AgentAdapter>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register an adapter for the given agent ID.
    pub fn register(&mut self, agent_id: String, adapter: Box<dyn AgentAdapter>) {
        self.adapters.insert(agent_id, adapter);
    }

    /// Initialize a registered adapter with config.
    pub async fn init_agent(
        &mut self,
        agent_id: &str,
        config: AgentConfig,
    ) -> Result<(), AdapterError> {
        let adapter = self
            .adapters
            .get_mut(agent_id)
            .ok_or_else(|| AdapterError::Other(format!("agent not found: {agent_id}")))?;
        adapter.init(config).await?;
        Ok(())
    }

    /// Create a session on the specified agent.
    pub async fn create_session(
        &mut self,
        agent_id: &str,
        options: SessionOptions,
    ) -> Result<SessionInfo, AdapterError> {
        let adapter = self
            .adapters
            .get_mut(agent_id)
            .ok_or_else(|| AdapterError::Other(format!("agent not found: {agent_id}")))?;
        adapter.create_session(options).await
    }

    /// Send a prompt to the specified agent and return the response channel.
    pub async fn send_prompt(
        &mut self,
        agent_id: &str,
        session_id: &str,
        prompt: Prompt,
    ) -> Result<mpsc::Receiver<ResponseEvent>, AdapterError> {
        let adapter = self
            .adapters
            .get_mut(agent_id)
            .ok_or_else(|| AdapterError::Other(format!("agent not found: {agent_id}")))?;
        adapter.send_prompt(session_id, prompt).await
    }

    /// Abort a running task.
    pub async fn abort(
        &mut self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<(), AdapterError> {
        let adapter = self
            .adapters
            .get_mut(agent_id)
            .ok_or_else(|| AdapterError::Other(format!("agent not found: {agent_id}")))?;
        adapter.abort(session_id).await
    }

    /// Shutdown all adapters.
    pub async fn shutdown_all(&mut self) {
        for (_, adapter) in self.adapters.iter_mut() {
            let _ = adapter.shutdown().await;
        }
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}
