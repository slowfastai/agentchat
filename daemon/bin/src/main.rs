use std::cell::RefCell;
use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
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
use if_addrs::{get_if_addrs, IfAddr, Interface};
use qrcode::{render::unicode, QrCode};
use tokio::sync::watch;
use tracing::{error, info};

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

const DEFAULT_PORT: u16 = 9390;

#[derive(Clone, Copy, Debug, Default)]
struct CliOptions {
    mobile_qr: bool,
}

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn command_name(command: &str) -> &str {
    Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
}

fn env_flag(key: &str) -> bool {
    optional_env(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
        .unwrap_or(false)
}

fn parse_cli_options() -> Result<Option<CliOptions>, String> {
    let mut options = CliOptions::default();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--mobile" => options.mobile_qr = true,
            "-h" | "--help" => {
                print_usage();
                return Ok(None);
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}`\n\nRun `agentchat-daemon --help` for usage."
                ));
            }
        }
    }

    Ok(Some(options))
}

fn print_usage() {
    println!(
        "agentchat-daemon\n\nUsage:\n  agentchat-daemon [--mobile]\n\nOptions:\n  --mobile            Print a terminal QR code for the local WebSocket URL so the iOS app can scan it\n  -h, --help          Show this help text\n\nEnvironment:\n  AGENTCHAT_MOBILE_WS_URL   Override the QR payload (must be ws://... or wss://...)\n  AGENTCHAT_AGENT_BACKEND   Select the agent backend adapter (default: acp)\n\nExample:\n  AGENTCHAT_AGENT_ID=opencode \\\n  AGENTCHAT_AGENT_NAME=\"OpenCode (ACP)\" \\\n  AGENTCHAT_AGENT_BACKEND=acp \\\n  AGENTCHAT_AGENT_COMMAND=opencode \\\n  AGENTCHAT_AGENT_ARGS=\"acp\" \\\n  cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- --mobile"
    );
}

fn configured_agent_args() -> Option<Vec<String>> {
    env::var("AGENTCHAT_AGENT_ARGS")
        .ok()
        .map(|value| {
            value
                .split_whitespace()
                .map(|arg| arg.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|args| !args.is_empty())
}

fn default_agent_args(backend: &str) -> Vec<String> {
    match backend {
        "acp" => vec!["acp".into()],
        _ => Vec::new(),
    }
}

fn detect_agent_backend(command: &str, args: &[String]) -> String {
    if let Some(backend) = optional_env("AGENTCHAT_AGENT_BACKEND") {
        return backend;
    }

    let command_name = command_name(command);
    if matches!(command_name, "codex" | "codex.exe")
        || args.first().map(String::as_str) == Some("app-server")
    {
        "codex_app_server".into()
    } else {
        "acp".into()
    }
}

fn load_agent_config() -> AgentConfig {
    let command = env_or_default("AGENTCHAT_AGENT_COMMAND", "opencode");
    let configured_args = configured_agent_args();
    let backend = detect_agent_backend(&command, configured_args.as_deref().unwrap_or(&[]));
    let args = configured_args.unwrap_or_else(|| default_agent_args(&backend));
    let mut extra = std::collections::HashMap::new();

    if let Some(approval_policy) = optional_env("AGENTCHAT_AGENT_APPROVAL_POLICY") {
        extra.insert(
            "approval_policy".into(),
            serde_json::Value::String(approval_policy),
        );
    }
    if let Some(approval_strategy) = optional_env("AGENTCHAT_AGENT_APPROVAL_STRATEGY") {
        extra.insert(
            "approval_strategy".into(),
            serde_json::Value::String(approval_strategy),
        );
    }
    if let Some(approvals_reviewer) = optional_env("AGENTCHAT_AGENT_APPROVALS_REVIEWER") {
        extra.insert(
            "approvals_reviewer".into(),
            serde_json::Value::String(approvals_reviewer),
        );
    }
    if let Some(sandbox) = optional_env("AGENTCHAT_AGENT_SANDBOX") {
        extra.insert("sandbox".into(), serde_json::Value::String(sandbox));
    }
    if env_flag("AGENTCHAT_AGENT_EXPERIMENTAL_RAW_EVENTS") {
        extra.insert(
            "experimental_raw_events".into(),
            serde_json::Value::Bool(true),
        );
    }
    if env_flag("AGENTCHAT_AGENT_PERSIST_EXTENDED_HISTORY") {
        extra.insert(
            "persist_extended_history".into(),
            serde_json::Value::Bool(true),
        );
    }

    AgentConfig {
        id: env_or_default("AGENTCHAT_AGENT_ID", "opencode"),
        name: env_or_default("AGENTCHAT_AGENT_NAME", "OpenCode (ACP)"),
        backend,
        command,
        args,
        working_dir: env::var("AGENTCHAT_AGENT_WORKING_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty()),
        env_vars: Default::default(),
        extra,
    }
}

fn load_agent_configs() -> Result<Vec<AgentConfig>, String> {
    match optional_env("AGENTCHAT_AGENTS_JSON") {
        Some(raw) => {
            let configs: Vec<AgentConfig> = serde_json::from_str(&raw)
                .map_err(|err| format!("failed to parse AGENTCHAT_AGENTS_JSON: {err}"))?;
            if configs.is_empty() {
                return Err("AGENTCHAT_AGENTS_JSON must contain at least one agent config".into());
            }
            Ok(configs)
        }
        None => Ok(vec![load_agent_config()]),
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

fn resolve_mobile_ws_url(port: u16) -> Result<String, String> {
    if let Some(ws_url) = optional_env("AGENTCHAT_MOBILE_WS_URL") {
        return validate_mobile_ws_url(&ws_url).map(|_| ws_url);
    }

    let ip = detect_mobile_ip()?;
    Ok(format_mobile_ws_url(ip, port))
}

fn validate_mobile_ws_url(ws_url: &str) -> Result<(), String> {
    if ws_url.starts_with("ws://") || ws_url.starts_with("wss://") {
        Ok(())
    } else {
        Err(
            "AGENTCHAT_MOBILE_WS_URL must start with ws:// or wss:// so the iOS app can connect"
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_agent_backend_recognizes_codex_binary_without_explicit_args() {
        assert_eq!(detect_agent_backend("codex", &[]), "codex_app_server");
        assert_eq!(
            detect_agent_backend("/usr/local/bin/codex", &[]),
            "codex_app_server"
        );
    }

    #[test]
    fn default_agent_args_are_backend_specific() {
        assert_eq!(default_agent_args("acp"), vec!["acp".to_string()]);
        assert!(default_agent_args("codex_app_server").is_empty());
    }
}

fn detect_mobile_ip() -> Result<IpAddr, String> {
    let mut interfaces =
        get_if_addrs().map_err(|err| format!("failed to inspect network interfaces: {err}"))?;
    interfaces.sort_by_key(mobile_interface_sort_key);

    for interface in interfaces {
        if should_skip_mobile_interface(&interface) {
            continue;
        }

        match interface.addr {
            IfAddr::V4(addr) if is_usable_mobile_ipv4(addr.ip) => return Ok(IpAddr::V4(addr.ip)),
            IfAddr::V6(addr) if is_usable_mobile_ipv6(addr.ip) => return Ok(IpAddr::V6(addr.ip)),
            _ => {}
        }
    }

    Err(
        "could not determine a non-loopback LAN IP automatically; set AGENTCHAT_MOBILE_WS_URL=ws://<your-mac-ip>:9390 explicitly"
            .into(),
    )
}

fn mobile_interface_sort_key(interface: &Interface) -> (u8, u8, String) {
    let family_rank = match interface.addr {
        IfAddr::V4(_) => 0,
        IfAddr::V6(_) => 1,
    };

    (
        mobile_interface_name_rank(&interface.name),
        family_rank,
        interface.name.clone(),
    )
}

fn mobile_interface_name_rank(name: &str) -> u8 {
    if name.starts_with("en") {
        0
    } else if name.starts_with("eth") || name.starts_with("wlan") {
        1
    } else if name.starts_with("bridge") {
        2
    } else if name.starts_with("awdl") || name.starts_with("llw") || name.starts_with("utun") {
        9
    } else {
        3
    }
}

fn should_skip_mobile_interface(interface: &Interface) -> bool {
    let name = interface.name.as_str();
    name.starts_with("lo")
        || name.starts_with("utun")
        || name.starts_with("awdl")
        || name.starts_with("llw")
        || name.starts_with("docker")
        || name.starts_with("veth")
}

fn is_usable_mobile_ipv4(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified()
}

fn is_usable_mobile_ipv6(ip: Ipv6Addr) -> bool {
    !ip.is_loopback() && !ip.is_unspecified() && !ip.is_unicast_link_local()
}

fn format_mobile_ws_url(ip: IpAddr, port: u16) -> String {
    match ip {
        IpAddr::V4(ip) => format!("ws://{ip}:{port}"),
        IpAddr::V6(ip) => format!("ws://[{ip}]:{port}"),
    }
}

fn print_mobile_qr(port: u16) -> Result<(), String> {
    let ws_url = resolve_mobile_ws_url(port)?;
    let qr = QrCode::new(ws_url.as_bytes())
        .map_err(|err| format!("failed to generate mobile QR code: {err}"))?
        .render::<unicode::Dense1x2>()
        .quiet_zone(true)
        .build();

    println!();
    println!("════════════════════════════════════════════════════════════");
    println!(" AgentChat mobile login");
    println!(" Scan this QR from the iPhone app: Connection → Scan QR");
    println!(" WebSocket URL: {ws_url}");
    println!(" Tip: phone and Mac must be on the same Wi-Fi / LAN");
    println!("════════════════════════════════════════════════════════════");
    println!("{qr}");
    println!("{ws_url}");
    println!();

    Ok(())
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
    let cli_options = match parse_cli_options() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    tracing_subscriber::fmt::init();

    info!("agentchat daemon v0.1.0");

    // Launch one or more configured agent backends from the environment.
    let agent_configs = match load_agent_configs() {
        Ok(configs) => configs,
        Err(err) => {
            error!("failed to load agent configuration: {err}");
            std::process::exit(1);
        }
    };
    let relay_config = match load_relay_client_config() {
        Ok(config) => config,
        Err(err) => {
            error!("failed to load relay configuration: {err}");
            std::process::exit(1);
        }
    };

    if cli_options.mobile_qr && relay_config.is_some() {
        eprintln!(
            "--mobile currently supports the direct local WebSocket server only; unset relay mode and try again"
        );
        std::process::exit(1);
    }

    // Use the current directory as the default project root.
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let local = tokio::task::LocalSet::new();

    let exit_code = local
        .run_until(async move {
            // Initialize the agents before wrapping in Rc<RefCell<>> to avoid
            // holding a RefCell borrow across an await point.
            let mut manager = AgentManager::new();
            for config in agent_configs {
                let agent_id = config.id.clone();
                if let Err(e) = manager.add_agent(config, project_root.clone()).await {
                    error!("failed to start agent '{agent_id}': {e}");
                    eprintln!("make sure the configured agent command is installed and in PATH");
                    return 1;
                }
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
                if cli_options.mobile_qr {
                    if let Err(err) = print_mobile_qr(DEFAULT_PORT) {
                        error!("failed to prepare mobile QR output: {err}");
                        eprintln!("failed to prepare mobile QR output: {err}");
                        return 1;
                    }
                }
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
