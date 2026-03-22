use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use agentchat_core::agent_manager::AgentManager;
use agentchat_protocol::AgentConfig;
use agentchat_server::ws::WebSocketServer;
use tracing::info;

const DEFAULT_PORT: u16 = 9390;

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn parse_agent_args() -> Vec<String> {
    env::var("AGENTCHAT_AGENT_ARGS")
        .ok()
        .map(|value| {
            value
                .split_whitespace()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|args| !args.is_empty())
        .unwrap_or_else(|| vec!["acp".into()])
}

fn load_agent_config() -> AgentConfig {
    AgentConfig {
        id: env_or_default("AGENTCHAT_AGENT_ID", "opencode"),
        name: env_or_default("AGENTCHAT_AGENT_NAME", "OpenCode (ACP)"),
        command: env_or_default("AGENTCHAT_AGENT_COMMAND", "opencode"),
        args: parse_agent_args(),
        working_dir: env::var("AGENTCHAT_AGENT_WORKING_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        env_vars: Default::default(),
        extra: Default::default(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("agentchat daemon v0.1.0");

    // M0: launch a single ACP-capable agent, configurable via environment.
    let config = load_agent_config();

    // Use current directory as project root (M0 default)
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let local = tokio::task::LocalSet::new();

    local
        .run_until(async move {
            // Initialize the agent before wrapping in Rc<RefCell<>> to avoid
            // holding a RefCell borrow across an await point.
            let mut manager = AgentManager::new();
            if let Err(e) = manager.add_agent(config, project_root.clone()).await {
                eprintln!("failed to start agent: {e}");
                eprintln!("make sure the ACP agent is installed and in PATH");
                std::process::exit(1);
            }

            info!("agent initialized, starting WebSocket server");

            let manager = Rc::new(RefCell::new(manager));
            let server = WebSocketServer::new(DEFAULT_PORT);
            server.run(manager).await;
        })
        .await;
}
