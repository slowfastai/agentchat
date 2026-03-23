use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use agentchat_core::agent_manager::AgentManager;
use agentchat_protocol::AgentConfig;
use agentchat_server::ws::WebSocketServer;
use tokio::sync::watch;
use tracing::{error, info};

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

async fn wait_for_shutdown_signal() -> Result<(), String> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| format!("failed to register SIGTERM handler: {e}"))?;

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|e| format!("failed to listen for Ctrl-C: {e}"))?;
            }
            _ = sigterm.recv() => {}
        }

        Ok(())
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| format!("failed to listen for Ctrl-C: {e}"))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("agentchat daemon v0.1.0");

    // M0: launch a single ACP-capable agent, configurable via environment.
    let config = load_agent_config();

    // Use current directory as project root (M0 default).
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let local = tokio::task::LocalSet::new();

    let exit_code = local
        .run_until(async move {
            // Initialize the agent before wrapping in Rc<RefCell<>> to avoid
            // holding a RefCell borrow across an await point.
            let mut manager = AgentManager::new();
            if let Err(e) = manager.add_agent(config, project_root.clone()).await {
                error!("failed to start agent: {e}");
                eprintln!("make sure the ACP agent is installed and in PATH");
                return 1;
            }

            info!("agent initialized, starting WebSocket server");

            let manager = Rc::new(RefCell::new(manager));
            let server = WebSocketServer::new(DEFAULT_PORT);
            let (_shutdown_tx, shutdown_rx) = watch::channel(false);
            let signal_tx = _shutdown_tx.clone();

            tokio::task::spawn_local(async move {
                if let Err(e) = wait_for_shutdown_signal().await {
                    error!("shutdown signal handler failed: {e}");
                }
                let _ = signal_tx.send(true);
            });

            let run_result = server.run(manager.clone(), shutdown_rx).await;
            let shutdown = { manager.borrow().shutdown_all() };
            shutdown.await;

            if let Err(e) = run_result {
                error!("websocket server failed: {e}");
                return 1;
            }

            0
        })
        .await;

    std::process::exit(exit_code);
}
