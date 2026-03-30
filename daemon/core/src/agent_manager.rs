use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::Value;
use tracing::info;

use agentchat_protocol::{canonical_mention_handle, AgentConfig, AgentStatus, AgentSummary};

use crate::acp_client::AcpAgent;
use crate::backend::AgentBackend;
use crate::codex_app_server::CodexAppServerAgent;

struct ManagedAgent {
    config: AgentConfig,
    agent: Rc<dyn AgentBackend>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBinding {
    pub agent_id: String,
    pub upstream_session_id: String,
}

/// Manages configured agent backend instances and session bindings.
pub struct AgentManager {
    agents: HashMap<String, ManagedAgent>,
    /// Map public daemon session IDs to their owning agent and upstream session IDs.
    session_bindings: HashMap<String, SessionBinding>,
    /// Reverse lookup from (agent_id, upstream_session_id) to the public daemon session ID.
    upstream_to_public: HashMap<(String, String), String>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
            session_bindings: HashMap::new(),
            upstream_to_public: HashMap::new(),
        }
    }

    /// Spawn and initialize an agent backend from config.
    pub async fn add_agent(
        &mut self,
        config: AgentConfig,
        project_root: PathBuf,
    ) -> Result<(), String> {
        let agent_id = config.id.clone();
        let agent: Rc<dyn AgentBackend> = match config.backend.as_str() {
            "acp" => Rc::new(AcpAgent::spawn(&config, project_root)?),
            "codex_app_server" | "codex-app-server" => {
                Rc::new(CodexAppServerAgent::spawn(&config, project_root)?)
            }
            other => {
                return Err(format!(
                    "unsupported agent backend `{other}` for agent `{}`",
                    config.id
                ));
            }
        };

        agent
            .initialize()
            .await
            .map_err(|e| format!("agent init failed: {e}"))?;

        self.agents
            .insert(agent_id.clone(), ManagedAgent { config, agent });
        info!("agent '{}' registered and initialized", agent_id);
        Ok(())
    }

    /// Get an agent by ID.
    pub fn get_agent(&self, agent_id: &str) -> Option<Rc<dyn AgentBackend>> {
        self.agents
            .get(agent_id)
            .map(|managed| managed.agent.clone())
    }

    /// Get the first registered agent ID.
    ///
    /// Returns the first configured agent, which remains the default when the client omits `agent_id`.
    pub fn first_agent_id(&self) -> Option<&str> {
        self.agents.keys().min().map(|id| id.as_str())
    }

    /// Return all configured agent IDs in stable sorted order.
    pub fn agent_ids(&self) -> Vec<String> {
        let mut ids = self.agents.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    /// Report whether any agents are currently registered.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Return app-facing summaries for all configured agents.
    pub fn list_agents(&self) -> Vec<AgentSummary> {
        let mut summaries = self
            .agents
            .values()
            .map(|managed| AgentSummary {
                agent_id: managed.config.id.clone(),
                name: managed.config.name.clone(),
                mention_handle: Some(canonical_mention_handle(&managed.config.id)),
                kind: managed
                    .config
                    .extra
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or(&managed.config.backend)
                    .to_string(),
                status: if managed.agent.is_alive() {
                    AgentStatus::Online
                } else {
                    AgentStatus::Crashed
                },
                default_working_dir: managed.config.working_dir.clone(),
                capabilities: vec![
                    "session".into(),
                    "prompt".into(),
                    "cancel".into(),
                    "distill".into(),
                ],
            })
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
        summaries
    }

    /// Register a public daemon session ID and bind it to the owning agent and upstream session.
    pub fn register_session(
        &mut self,
        public_session_id: String,
        agent_id: String,
        upstream_session_id: String,
    ) {
        self.upstream_to_public.insert(
            (agent_id.clone(), upstream_session_id.clone()),
            public_session_id.clone(),
        );
        self.session_bindings.insert(
            public_session_id,
            SessionBinding {
                agent_id,
                upstream_session_id,
            },
        );
    }

    /// Look up the full binding for a public daemon session.
    pub fn session_binding(&self, session_id: &str) -> Option<&SessionBinding> {
        self.session_bindings.get(session_id)
    }

    /// Look up which agent owns a public daemon session.
    pub fn agent_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_binding(session_id)
            .map(|binding| binding.agent_id.as_str())
    }

    /// Look up the upstream agent session ID for a public daemon session.
    pub fn upstream_session_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_binding(session_id)
            .map(|binding| binding.upstream_session_id.as_str())
    }

    /// Resolve a public daemon session ID from an agent ID and upstream session ID.
    pub fn public_session_for_upstream(
        &self,
        agent_id: &str,
        upstream_session_id: &str,
    ) -> Option<&str> {
        self.upstream_to_public
            .get(&(agent_id.to_string(), upstream_session_id.to_string()))
            .map(|session_id| session_id.as_str())
    }

    /// Remove a session binding.
    pub fn remove_session(&mut self, session_id: &str) -> Option<SessionBinding> {
        let binding = self.session_bindings.remove(session_id)?;
        self.upstream_to_public.remove(&(
            binding.agent_id.clone(),
            binding.upstream_session_id.clone(),
        ));
        Some(binding)
    }

    /// Remove multiple session bindings.
    pub fn remove_sessions(&mut self, session_ids: &[String]) {
        for session_id in session_ids {
            let _ = self.remove_session(session_id);
        }
    }

    /// Report whether the agent process is still alive.
    pub fn is_agent_alive(&self, agent_id: &str) -> bool {
        self.agents
            .get(agent_id)
            .map(|managed| managed.agent.is_alive())
            .unwrap_or(false)
    }

    /// Shutdown all agents.
    pub fn shutdown_all(&self) -> impl std::future::Future<Output = ()> + 'static {
        let agents = self
            .agents
            .iter()
            .map(|(id, managed)| (id.clone(), Rc::clone(&managed.agent)))
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
    fn is_empty_is_true_when_no_agents_are_registered() {
        let manager = AgentManager::new();

        assert!(manager.is_empty());
    }

    #[test]
    fn register_lookup_and_remove_session() {
        let mut manager = AgentManager::new();
        manager.register_session("session-1".into(), "agent-1".into(), "upstream-1".into());

        assert_eq!(manager.agent_for_session("session-1"), Some("agent-1"));
        assert_eq!(
            manager.upstream_session_for_session("session-1"),
            Some("upstream-1")
        );
        assert_eq!(
            manager.public_session_for_upstream("agent-1", "upstream-1"),
            Some("session-1")
        );
        assert_eq!(
            manager.remove_session("session-1"),
            Some(SessionBinding {
                agent_id: "agent-1".into(),
                upstream_session_id: "upstream-1".into(),
            })
        );
        assert_eq!(manager.agent_for_session("session-1"), None);
    }

    #[test]
    fn remove_sessions_clears_multiple_mappings() {
        let mut manager = AgentManager::new();
        manager.register_session("session-1".into(), "agent-1".into(), "upstream-1".into());
        manager.register_session("session-2".into(), "agent-1".into(), "upstream-2".into());

        manager.remove_sessions(&["session-1".into(), "session-2".into()]);

        assert_eq!(manager.agent_for_session("session-1"), None);
        assert_eq!(manager.agent_for_session("session-2"), None);
        assert_eq!(
            manager.public_session_for_upstream("agent-1", "upstream-1"),
            None
        );
        assert_eq!(
            manager.public_session_for_upstream("agent-1", "upstream-2"),
            None
        );
    }
}
