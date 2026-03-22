use oversight_core::agent_manager::AgentManager;

/// WebSocket server that bridges the iOS app and the agent manager.
pub struct WebSocketServer {
    _agent_manager: AgentManager,
    port: u16,
}

impl WebSocketServer {
    pub fn new(agent_manager: AgentManager, port: u16) -> Self {
        Self {
            _agent_manager: agent_manager,
            port,
        }
    }

    /// Start listening for WebSocket connections.
    /// Will integrate with tokio-tungstenite or axum in the future.
    pub async fn run(&self) {
        println!("WebSocket server listening on 0.0.0.0:{}", self.port);
        // TODO: accept connections, handle messages per WebSocket-Protocol-v0.1.md
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl-c");
    }
}
