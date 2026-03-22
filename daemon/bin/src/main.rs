use oversight_core::agent_manager::AgentManager;
use oversight_server::ws::WebSocketServer;

const DEFAULT_PORT: u16 = 9390;

#[tokio::main]
async fn main() {
    println!("oversight daemon v0.1.0");

    let agent_manager = AgentManager::new();

    // TODO: load agent configs, register adapters, call init()

    let server = WebSocketServer::new(agent_manager, DEFAULT_PORT);
    server.run().await;
}
