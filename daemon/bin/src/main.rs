use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::relay_client::{RelayClientConfig, RelayClientCryptoConfig};
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::relay_crypto::{
    decode_base64url_exact, ed25519_public_key, seed_from_label,
};
use agentchat_protocol::AgentConfig;
use agentchat_server::relay::RelayTransportServer;
use agentchat_server::ws::WebSocketServer;
use tokio::sync::watch;
use tracing::{error, info};

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

const DEFAULT_PORT: u16 = 9390;

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn env_flag(key: &str) -> bool {
    optional_env(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
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

fn load_relay_crypto_config() -> Result<RelayClientCryptoConfig, String> {
    if env_flag("AGENTCHAT_RELAY_DEV_CRYPTO") {
        return Ok(RelayClientCryptoConfig {
            identity_seed: seed_from_label(DEV_DAEMON_IDENTITY_LABEL),
            expected_remote_identity_public_key: ed25519_public_key(&seed_from_label(
                DEV_APP_IDENTITY_LABEL,
            )),
        });
    }

    let identity_seed = optional_env("AGENTCHAT_RELAY_IDENTITY_SEED_B64URL")
        .ok_or("missing AGENTCHAT_RELAY_IDENTITY_SEED_B64URL for relay mode")?;
    let remote_identity_public_key =
        optional_env("AGENTCHAT_RELAY_REMOTE_IDENTITY_PUBLIC_KEY_B64URL")
            .ok_or("missing AGENTCHAT_RELAY_REMOTE_IDENTITY_PUBLIC_KEY_B64URL for relay mode")?;

    Ok(RelayClientCryptoConfig {
        identity_seed: decode_base64url_exact::<32>(
            "AGENTCHAT_RELAY_IDENTITY_SEED_B64URL",
            &identity_seed,
        )
        .map_err(|err| err.to_string())?,
        expected_remote_identity_public_key: decode_base64url_exact::<32>(
            "AGENTCHAT_RELAY_REMOTE_IDENTITY_PUBLIC_KEY_B64URL",
            &remote_identity_public_key,
        )
        .map_err(|err| err.to_string())?,
    })
}

fn load_relay_client_config() -> Result<Option<RelayClientConfig>, String> {
    let ws_url = optional_env("AGENTCHAT_RELAY_WS_URL");
    let relay_token = optional_env("AGENTCHAT_RELAY_TOKEN");

    match (ws_url, relay_token) {
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => {
            Err("relay mode requires both AGENTCHAT_RELAY_WS_URL and AGENTCHAT_RELAY_TOKEN".into())
        }
        (Some(ws_url), Some(relay_token)) => {
            let mut config = RelayClientConfig::new(ws_url, relay_token);
            config.crypto = Some(load_relay_crypto_config()?);
            Ok(Some(config))
        }
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
    let relay_config = match load_relay_client_config() {
        Ok(config) => config,
        Err(err) => {
            error!("failed to load relay configuration: {err}");
            std::process::exit(1);
        }
    };

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

            let manager = Rc::new(RefCell::new(manager));
            let session_store = Rc::new(RefCell::new(SessionStore::new(&project_root)));
            let skill_store = Rc::new(SkillStore::new(&project_root));
            let distiller = Rc::new(Distiller::new(skill_store.clone()));
            let (_shutdown_tx, shutdown_rx) = watch::channel(false);
            let signal_tx = _shutdown_tx.clone();

            tokio::task::spawn_local(async move {
                if let Err(e) = wait_for_shutdown_signal().await {
                    error!("shutdown signal handler failed: {e}");
                }
                let _ = signal_tx.send(true);
            });

            let run_result = if let Some(relay_config) = relay_config.clone() {
                info!("agent initialized, starting relay transport");
                RelayTransportServer::new(relay_config)
                    .run(
                        manager.clone(),
                        shutdown_rx,
                        session_store,
                        skill_store,
                        distiller,
                    )
                    .await
            } else {
                info!("agent initialized, starting WebSocket server");
                WebSocketServer::new(DEFAULT_PORT)
                    .run(
                        manager.clone(),
                        shutdown_rx,
                        session_store,
                        skill_store,
                        distiller,
                    )
                    .await
            };
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
