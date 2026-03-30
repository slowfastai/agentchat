use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::relay_client::{
    RelayClientConfig, RelayClientCryptoConfig, DEFAULT_RELAY_USER_AGENT,
};
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::relay_crypto::{
    decode_base64url_exact, ed25519_public_key, seed_from_label,
};
use agentchat_protocol::{AgentConfig, AgentStatus, AgentSummary};
use agentchat_server::relay::RelayTransportServer;
use agentchat_server::ws::WebSocketServer;
use agentchat_protocol::DaemonStopReason;
use if_addrs::{get_if_addrs, IfAddr, Interface};
use qrcode::{render::unicode, QrCode};
use serde::Deserialize;
use tokio::sync::{mpsc, watch};
use tracing::{error, info};
use tracing_subscriber::fmt::writer::MakeWriter;

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

const DEFAULT_PORT: u16 = 9390;

#[derive(Clone, Copy, Debug, Default)]
struct CliOptions {
    mobile_qr: bool,
}

enum InteractiveCommand {
    ShowMobile {
        reply: std_mpsc::Sender<Vec<AgentSummary>>,
    },
    RenderMobileQr {
        selected_agent_ids: Vec<String>,
        reply: std_mpsc::Sender<Result<String, String>>,
    },
    Shutdown,
}

#[derive(Clone)]
struct SharedFileWriter {
    file: Arc<Mutex<File>>,
}

struct SharedFileWriterGuard<'a> {
    guard: std::sync::MutexGuard<'a, File>,
}

#[derive(Clone, Default)]
struct MobileQrAvailability {
    relay_connected: Option<Arc<AtomicBool>>,
}

impl MobileQrAvailability {
    fn local() -> Self {
        Self::default()
    }

    fn relay() -> Self {
        Self {
            relay_connected: Some(Arc::new(AtomicBool::new(false))),
        }
    }

    fn require_ready(&self) -> Result<(), String> {
        let Some(relay_connected) = &self.relay_connected else {
            return Ok(());
        };

        if relay_connected.load(Ordering::SeqCst) {
            return Ok(());
        }

        Err(
            "relay transport is not connected yet; wait for `relay transport connected; waiting for secure channel` before printing a mobile QR code"
                .into(),
        )
    }

    fn set_relay_connected(&self, connected: bool) {
        if let Some(relay_connected) = &self.relay_connected {
            relay_connected.store(connected, Ordering::SeqCst);
        }
    }
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

impl io::Write for SharedFileWriterGuard<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.guard.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.guard.flush()
    }
}

impl<'a> MakeWriter<'a> for SharedFileWriter {
    type Writer = SharedFileWriterGuard<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        SharedFileWriterGuard {
            guard: self.file.lock().expect("daemon log file mutex poisoned"),
        }
    }
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
        "agentchat-daemon\n\nUsage:\n  agentchat-daemon [--mobile]\n\nOptions:\n  --mobile            Print a terminal QR code for the current direct or relay connection so the iOS app can scan it\n  -h, --help          Show this help text\n\nEnvironment:\n  AGENTCHAT_MOBILE_WS_URL   Override the websocket endpoint embedded in the QR payload (must be ws://... or wss://...)\n  AGENTCHAT_AGENT_BACKEND   Select the agent backend adapter (default: acp)\n\nExample:\n  AGENTCHAT_AGENT_ID=opencode \\\n  AGENTCHAT_AGENT_NAME=\"OpenCode (ACP)\" \\\n  AGENTCHAT_AGENT_BACKEND=acp \\\n  AGENTCHAT_AGENT_COMMAND=opencode \\\n  AGENTCHAT_AGENT_ARGS=\"acp\" \\\n  cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- --mobile"
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
            config.user_agent = relay_http_user_agent();
            config.crypto = Some(load_relay_crypto_config()?);
            Ok(Some(config))
        }
    }
}

fn relay_http_user_agent() -> String {
    optional_env("AGENTCHAT_RELAY_USER_AGENT")
        .unwrap_or_else(|| DEFAULT_RELAY_USER_AGENT.to_string())
}

#[derive(Debug, Deserialize)]
struct RelayPairingOpenResponse {
    pairing_ticket: String,
    ws_url: String,
    expires_at: u64,
}

fn default_log_path(project_root: &Path) -> PathBuf {
    project_root
        .join(".agentchat")
        .join("logs")
        .join("agentchat-daemon.log")
}

fn resolve_log_path(project_root: &Path) -> PathBuf {
    optional_env("AGENTCHAT_LOG_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(project_root))
}

fn init_tracing(project_root: &Path) -> Result<PathBuf, String> {
    let log_path = resolve_log_path(project_root);
    let parent = log_path
        .parent()
        .ok_or_else(|| format!("invalid daemon log path: {}", log_path.display()))?;
    fs::create_dir_all(parent).map_err(|err| {
        format!(
            "failed to create daemon log directory '{}': {err}",
            parent.display()
        )
    })?;

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| {
            format!(
                "failed to open daemon log file '{}': {err}",
                log_path.display()
            )
        })?;

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(SharedFileWriter {
            file: Arc::new(Mutex::new(file)),
        })
        .init();

    Ok(log_path)
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
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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

    #[test]
    fn mobile_qr_payload_defaults_to_raw_ws_url_without_agent_selection() {
        assert_eq!(
            mobile_qr_payload_for_ws_url("ws://127.0.0.1:9390", &[]),
            "ws://127.0.0.1:9390"
        );
    }

    #[test]
    fn mobile_qr_payload_encodes_selected_agents_into_custom_scheme() {
        let payload = mobile_qr_payload_for_ws_url(
            "ws://192.168.1.10:9390",
            &["codex-main".into(), "codex-review".into()],
        );

        assert_eq!(
            payload,
            "agentchat://connect?url=ws%3A%2F%2F192.168.1.10%3A9390&agents=codex-main%2Ccodex-review"
        );
    }

    #[test]
    fn relay_mobile_qr_payload_encodes_pairing_ticket_and_agents() {
        let payload = relay_mobile_qr_payload_for_pairing_ticket(
            "wss://relay.agentchat.dev/v1/ws",
            "achpair.dev_local_1.pair_abc.secret_value",
            &["codex-main".into()],
        );

        assert_eq!(
            payload,
            "agentchat://connect?relay_url=wss%3A%2F%2Frelay.agentchat.dev%2Fv1%2Fws&pairing_ticket=achpair.dev_local_1.pair_abc.secret_value&relay_pairing=claim&relay_crypto=dev&agents=codex-main"
        );
    }

    #[test]
    fn pairing_open_http_url_is_derived_from_websocket_url() {
        assert_eq!(
            relay_pairing_open_url_from_ws_url("wss://relay.agentchat.dev/v1/ws").unwrap(),
            "https://relay.agentchat.dev/v1/pairing/open"
        );
        assert_eq!(
            relay_pairing_open_url_from_ws_url("ws://127.0.0.1:8787/v1/ws").unwrap(),
            "http://127.0.0.1:8787/v1/pairing/open"
        );
    }

    #[test]
    fn default_log_path_uses_project_agentchat_logs_directory() {
        let root = PathBuf::from("/tmp/agentchat-project");

        assert_eq!(
            default_log_path(&root),
            PathBuf::from("/tmp/agentchat-project/.agentchat/logs/agentchat-daemon.log")
        );
    }

    #[test]
    fn resolve_log_path_prefers_environment_override() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        env::set_var("AGENTCHAT_LOG_PATH", "/tmp/custom-daemon.log");
        let root = PathBuf::from("/tmp/agentchat-project");

        assert_eq!(
            resolve_log_path(&root),
            PathBuf::from("/tmp/custom-daemon.log")
        );

        env::remove_var("AGENTCHAT_LOG_PATH");
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

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn mobile_qr_payload_for_ws_url(ws_url: &str, selected_agent_ids: &[String]) -> String {
    if selected_agent_ids.is_empty() {
        return ws_url.to_string();
    }

    format!(
        "agentchat://connect?url={}&agents={}",
        percent_encode_component(ws_url),
        percent_encode_component(&selected_agent_ids.join(","))
    )
}

fn relay_mobile_qr_payload_for_pairing_ticket(
    relay_ws_url: &str,
    pairing_ticket: &str,
    selected_agent_ids: &[String],
) -> String {
    let mut payload = format!(
        "agentchat://connect?relay_url={}&pairing_ticket={}&relay_pairing=claim&relay_crypto=dev",
        percent_encode_component(relay_ws_url),
        percent_encode_component(pairing_ticket)
    );

    if !selected_agent_ids.is_empty() {
        payload.push_str("&agents=");
        payload.push_str(&percent_encode_component(&selected_agent_ids.join(",")));
    }

    payload
}

fn relay_pairing_open_url_from_ws_url(relay_ws_url: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(relay_ws_url)
        .map_err(|err| format!("invalid relay websocket url '{relay_ws_url}': {err}"))?;

    match url.scheme() {
        "wss" => url.set_scheme("https").map_err(|_| {
            "failed to derive https pairing url from relay websocket url".to_string()
        })?,
        "ws" => url.set_scheme("http").map_err(|_| {
            "failed to derive http pairing url from relay websocket url".to_string()
        })?,
        _ => {
            return Err(format!(
                "relay websocket url must use ws:// or wss://, got '{relay_ws_url}'"
            ))
        }
    }

    url.set_path("/v1/pairing/open");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

async fn open_pairing_ticket_for_relay(
    relay_ws_url: &str,
    relay_token: &str,
) -> Result<RelayPairingOpenResponse, String> {
    let pairing_open_url = relay_pairing_open_url_from_ws_url(relay_ws_url)?;
    let client = reqwest::Client::builder()
        .user_agent(relay_http_user_agent())
        .build()
        .map_err(|err| format!("failed to build relay pairing http client: {err}"))?;
    let response = client
        .post(&pairing_open_url)
        .bearer_auth(relay_token)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .map_err(|err| format!("failed to open relay pairing session: {err}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("failed reading relay pairing response body: {err}"))?;

    if !status.is_success() {
        return Err(format!(
            "relay pairing open failed with HTTP {}: {}",
            status.as_u16(),
            body.trim()
        ));
    }

    let pairing: RelayPairingOpenResponse = serde_json::from_str(&body)
        .map_err(|err| format!("failed to decode relay pairing response: {err}"))?;
    validate_mobile_ws_url(&pairing.ws_url)?;
    if pairing.pairing_ticket.trim().is_empty() {
        return Err("relay pairing response did not include a pairing_ticket".into());
    }
    if pairing.expires_at == 0 {
        return Err("relay pairing response did not include a valid expires_at timestamp".into());
    }

    Ok(pairing)
}

async fn build_relay_mobile_qr_payload(
    selected_agent_ids: &[String],
) -> Result<Option<(String, String)>, String> {
    let Some(configured_relay_ws_url) = optional_env("AGENTCHAT_RELAY_WS_URL") else {
        return Ok(None);
    };

    let relay_ws_url = optional_env("AGENTCHAT_MOBILE_WS_URL").unwrap_or(configured_relay_ws_url);
    validate_mobile_ws_url(&relay_ws_url)?;

    if !env_flag("AGENTCHAT_RELAY_DEV_CRYPTO") {
        return Err(
            "relay mobile QR currently requires AGENTCHAT_RELAY_DEV_CRYPTO=true because app pairing for custom relay identities is not implemented yet"
                .into(),
        );
    }

    if let Some(pairing_ticket) = optional_env("AGENTCHAT_RELAY_PAIRING_TICKET") {
        let payload = relay_mobile_qr_payload_for_pairing_ticket(
            &relay_ws_url,
            &pairing_ticket,
            selected_agent_ids,
        );
        return Ok(Some((relay_ws_url, payload)));
    }

    let relay_token = optional_env("AGENTCHAT_RELAY_TOKEN")
        .ok_or("relay mobile QR requires AGENTCHAT_RELAY_TOKEN to be set")?;
    let pairing = open_pairing_ticket_for_relay(&relay_ws_url, &relay_token).await?;
    let payload = relay_mobile_qr_payload_for_pairing_ticket(
        &pairing.ws_url,
        &pairing.pairing_ticket,
        selected_agent_ids,
    );
    Ok(Some((pairing.ws_url, payload)))
}

async fn build_mobile_qr_payload(
    port: u16,
    selected_agent_ids: &[String],
) -> Result<(String, String, bool), String> {
    if let Some((ws_url, payload)) = build_relay_mobile_qr_payload(selected_agent_ids).await? {
        return Ok((ws_url, payload, true));
    }

    let ws_url = resolve_mobile_ws_url(port)?;
    let payload = mobile_qr_payload_for_ws_url(&ws_url, selected_agent_ids);
    Ok((ws_url, payload, false))
}

fn render_mobile_qr_output(
    ws_url: &str,
    payload: &str,
    is_relay: bool,
    selected_agent_ids: &[String],
) -> Result<String, String> {
    let qr = QrCode::new(payload.as_bytes())
        .map_err(|err| format!("failed to generate mobile QR code: {err}"))?
        .render::<unicode::Dense1x2>()
        .quiet_zone(true)
        .build();

    let mut output = String::new();
    output.push('\n');
    output.push_str("════════════════════════════════════════════════════════════\n");
    output.push_str(" AgentChat mobile login\n");
    output.push_str(" Scan this QR from the iPhone app: Connection → Scan QR\n");
    if is_relay {
        output.push_str(&format!(" Relay URL: {ws_url}\n"));
    } else {
        output.push_str(&format!(" WebSocket URL: {ws_url}\n"));
    }
    if !selected_agent_ids.is_empty() {
        output.push_str(&format!(
            " Preselected agents: {}\n",
            selected_agent_ids.join(", ")
        ));
    }
    if is_relay {
        output.push_str(
            " Tip: phone and Mac can be on different networks once both connect through the relay\n",
        );
    } else {
        output.push_str(" Tip: phone and Mac must be on the same Wi-Fi / LAN\n");
    }
    output.push_str("════════════════════════════════════════════════════════════\n");
    output.push_str(&qr);
    output.push('\n');
    output.push_str(payload);
    output.push_str("\n\n");

    Ok(output)
}

async fn render_mobile_qr(port: u16, selected_agent_ids: &[String]) -> Result<String, String> {
    let (ws_url, payload, is_relay) = build_mobile_qr_payload(port, selected_agent_ids).await?;
    render_mobile_qr_output(&ws_url, &payload, is_relay, selected_agent_ids)
}

fn print_mobile_qr_output(output: &str) {
    print!("{output}");
}

fn print_interactive_help() {
    println!();
    println!("Interactive commands:");
    println!("  /mobile   Select one or more agents and print a mobile QR code");
    println!("  /help     Show this help");
    println!("  /quit     Stop the daemon");
    println!();
}

fn start_interactive_console(command_tx: mpsc::UnboundedSender<InteractiveCommand>) {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return;
    }

    thread::spawn(move || {
        print_interactive_help();

        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();

        loop {
            print!("agentchat> ");
            if io::stdout().flush().is_err() {
                break;
            }

            let Some(line) = lines.next() else {
                break;
            };

            let line = match line {
                Ok(line) => line,
                Err(err) => {
                    eprintln!("failed reading CLI command: {err}");
                    break;
                }
            };

            match line.trim() {
                "" => {}
                "/help" => print_interactive_help(),
                "/quit" | "/exit" => {
                    let _ = command_tx.send(InteractiveCommand::Shutdown);
                    break;
                }
                "/mobile" => {
                    let (reply_tx, reply_rx) = std_mpsc::channel();
                    if command_tx
                        .send(InteractiveCommand::ShowMobile { reply: reply_tx })
                        .is_err()
                    {
                        break;
                    }

                    let agents = match reply_rx.recv() {
                        Ok(agents) => agents,
                        Err(_) => {
                            eprintln!("failed to read daemon agent list");
                            continue;
                        }
                    };

                    match prompt_mobile_agent_selection(&agents) {
                        Ok(Some(selected_agent_ids)) => {
                            let (reply_tx, reply_rx) = std_mpsc::channel();
                            if command_tx
                                .send(InteractiveCommand::RenderMobileQr {
                                    selected_agent_ids,
                                    reply: reply_tx,
                                })
                                .is_err()
                            {
                                eprintln!("failed to request mobile QR rendering from daemon");
                                continue;
                            }

                            match reply_rx.recv() {
                                Ok(Ok(output)) => print_mobile_qr_output(&output),
                                Ok(Err(err)) => {
                                    eprintln!("failed to prepare mobile QR output: {err}")
                                }
                                Err(_) => {
                                    eprintln!("failed to receive mobile QR output from daemon")
                                }
                            }
                        }
                        Ok(None) => println!("mobile QR selection cancelled"),
                        Err(err) => eprintln!("failed to open mobile selection: {err}"),
                    }
                }
                other => eprintln!("unknown command `{other}`; run /help"),
            }
        }
    });
}

struct RawTerminalGuard {
    original_state: String,
}

impl RawTerminalGuard {
    fn new() -> Result<Self, String> {
        let tty = File::open("/dev/tty")
            .map_err(|err| format!("failed to open /dev/tty for interactive terminal: {err}"))?;
        let output = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::from(tty.try_clone().map_err(|err| {
                format!("failed to clone /dev/tty handle: {err}")
            })?))
            .output()
            .map_err(|err| format!("failed to read terminal state with stty: {err}"))?;
        if !output.status.success() {
            return Err("stty -g failed while preparing interactive terminal".into());
        }

        let original_state = String::from_utf8(output.stdout)
            .map_err(|err| format!("failed to decode terminal state: {err}"))?
            .trim()
            .to_string();

        let status = Command::new("stty")
            .args(["-icanon", "-echo", "min", "1", "time", "0"])
            .stdin(Stdio::from(tty.try_clone().map_err(|err| {
                format!("failed to clone /dev/tty handle: {err}")
            })?))
            .status()
            .map_err(|err| format!("failed to switch terminal to raw mode: {err}"))?;
        if !status.success() {
            return Err("stty failed to switch terminal to raw mode".into());
        }

        Ok(Self { original_state })
    }
}

impl Drop for RawTerminalGuard {
    fn drop(&mut self) {
        if let Ok(tty) = File::open("/dev/tty") {
            let _ = Command::new("stty")
                .arg(&self.original_state)
                .stdin(Stdio::from(tty))
                .status();
        }
    }
}

fn draw_mobile_selection(
    agents: &[AgentSummary],
    cursor: usize,
    selected: &[bool],
    warning: Option<&str>,
) -> Result<(), String> {
    print!("\x1b[2J\x1b[H");
    println!("Select coding agents for the mobile QR");
    println!(
        "Use ↑/↓ or j/k to move, Space to toggle, Enter to confirm, a to toggle all online, q to cancel."
    );
    println!();

    for (index, agent) in agents.iter().enumerate() {
        let pointer = if index == cursor { ">" } else { " " };
        let marker = if selected[index] { "●" } else { "○" };
        let status = match agent.status {
            AgentStatus::Online => "online",
            AgentStatus::Offline => "offline",
            AgentStatus::Starting => "starting",
            AgentStatus::Crashed => "crashed",
        };
        let suffix = if matches!(agent.status, AgentStatus::Online) {
            ""
        } else {
            " (unavailable)"
        };
        println!(
            "{pointer} {marker} {} [{}] - {}{}",
            agent.name, agent.agent_id, status, suffix
        );
    }

    if let Some(warning) = warning {
        println!();
        println!("{warning}");
    }

    io::stdout()
        .flush()
        .map_err(|err| format!("failed to flush terminal output: {err}"))
}

fn prompt_mobile_agent_selection(agents: &[AgentSummary]) -> Result<Option<Vec<String>>, String> {
    if agents.is_empty() {
        return Err("no agents are configured in the daemon".into());
    }
    if !agents
        .iter()
        .any(|agent| matches!(agent.status, AgentStatus::Online))
    {
        return Err("no online agents are available to include in the QR".into());
    }

    let _guard = RawTerminalGuard::new()?;
    let mut tty = File::open("/dev/tty")
        .map_err(|err| format!("failed to open /dev/tty for keyboard input: {err}"))?;
    let mut cursor = 0usize;
    let mut selected = vec![false; agents.len()];
    let mut warning: Option<String> = None;

    loop {
        draw_mobile_selection(agents, cursor, &selected, warning.as_deref())?;
        warning = None;

        let mut byte = [0u8; 1];
        tty.read_exact(&mut byte)
            .map_err(|err| format!("failed reading keyboard input: {err}"))?;

        match byte[0] {
            b' ' => {
                if matches!(agents[cursor].status, AgentStatus::Online) {
                    selected[cursor] = !selected[cursor];
                } else {
                    warning = Some("Only online agents can be selected.".into());
                }
            }
            b'a' | b'A' => {
                let should_select_all = agents.iter().enumerate().any(|(index, agent)| {
                    matches!(agent.status, AgentStatus::Online) && !selected[index]
                });
                for (index, agent) in agents.iter().enumerate() {
                    if matches!(agent.status, AgentStatus::Online) {
                        selected[index] = should_select_all;
                    }
                }
            }
            b'k' => cursor = cursor.saturating_sub(1),
            b'j' => cursor = (cursor + 1).min(agents.len().saturating_sub(1)),
            b'\r' | b'\n' => {
                let selected_ids = agents
                    .iter()
                    .zip(selected.iter())
                    .filter(|(agent, is_selected)| {
                        **is_selected && matches!(agent.status, AgentStatus::Online)
                    })
                    .map(|(agent, _)| agent.agent_id.clone())
                    .collect::<Vec<_>>();
                if selected_ids.is_empty() {
                    warning = Some("Select at least one online agent before confirming.".into());
                } else {
                    print!("\x1b[2J\x1b[H");
                    println!();
                    return Ok(Some(selected_ids));
                }
            }
            b'q' => {
                print!("\x1b[2J\x1b[H");
                println!();
                return Ok(None);
            }
            0x1b => {
                let mut sequence = [0u8; 2];
                if tty.read_exact(&mut sequence).is_ok() && sequence[0] == b'[' {
                    match sequence[1] {
                        b'A' => cursor = cursor.saturating_sub(1),
                        b'B' => cursor = (cursor + 1).min(agents.len().saturating_sub(1)),
                        _ => {}
                    }
                } else {
                    print!("\x1b[2J\x1b[H");
                    println!();
                    return Ok(None);
                }
            }
            _ => {}
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
    // Use the current directory as the default project root.
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let cli_options = match parse_cli_options() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let log_path = match init_tracing(&project_root) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    info!("agentchat daemon v0.1.0");
    info!("daemon logs redirected to {}", log_path.display());

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
            let mobile_qr_availability = if relay_config.is_some() {
                MobileQrAvailability::relay()
            } else {
                MobileQrAvailability::local()
            };
            let (_shutdown_tx, shutdown_rx) = watch::channel::<Option<DaemonStopReason>>(None);
            let signal_tx = _shutdown_tx.clone();
            let (command_tx, mut command_rx) = mpsc::unbounded_channel::<InteractiveCommand>();

            tokio::task::spawn_local(async move {
                if let Err(e) = wait_for_shutdown_signal().await {
                    error!("shutdown signal handler failed: {e}");
                }
                let _ = signal_tx.send(Some(DaemonStopReason::Signal));
            });

            start_interactive_console(command_tx);
            let manager_for_commands = manager.clone();
            let mobile_qr_availability_for_commands = mobile_qr_availability.clone();
            let signal_tx = _shutdown_tx.clone();
            tokio::task::spawn_local(async move {
                while let Some(command) = command_rx.recv().await {
                    match command {
                        InteractiveCommand::ShowMobile { reply } => {
                            let _ = reply.send(manager_for_commands.borrow().list_agents());
                        }
                        InteractiveCommand::RenderMobileQr {
                            selected_agent_ids,
                            reply,
                        } => {
                            let result = match mobile_qr_availability_for_commands.require_ready() {
                                Ok(()) => render_mobile_qr(DEFAULT_PORT, &selected_agent_ids).await,
                                Err(err) => Err(err),
                            };
                            let _ = reply.send(result);
                        }
                        InteractiveCommand::Shutdown => {
                            let _ = signal_tx.send(Some(DaemonStopReason::UserShutdown));
                            break;
                        }
                    }
                }
            });

            let run_result = if let Some(relay_config) = relay_config.clone() {
                info!("agent initialized, starting relay transport");
                let relay_server = RelayTransportServer::new(relay_config);
                let relay_client = match relay_server.connect_client().await {
                    Ok(client) => client,
                    Err(err) => {
                        if cli_options.mobile_qr {
                            error!(
                                "failed to connect relay transport before mobile QR output: {err}"
                            );
                            eprintln!(
                                "failed to connect relay transport before mobile QR output: {err}"
                            );
                            return 1;
                        }
                        let shutdown = { manager.borrow().shutdown_all() };
                        shutdown.await;
                        error!("websocket server failed: {err}");
                        return 1;
                    }
                };

                mobile_qr_availability.set_relay_connected(true);

                if cli_options.mobile_qr {
                    match render_mobile_qr(DEFAULT_PORT, &[]).await {
                        Ok(output) => print_mobile_qr_output(&output),
                        Err(err) => {
                            error!("failed to prepare mobile QR output: {err}");
                            eprintln!("failed to prepare mobile QR output: {err}");
                            return 1;
                        }
                    }
                }

                let result = relay_server
                    .run_with_client(
                        relay_client,
                        manager.clone(),
                        shutdown_rx,
                        session_store,
                        skill_store,
                        distiller,
                    )
                    .await;
                mobile_qr_availability.set_relay_connected(false);
                result
            } else {
                if cli_options.mobile_qr {
                    match render_mobile_qr(DEFAULT_PORT, &[]).await {
                        Ok(output) => print_mobile_qr_output(&output),
                        Err(err) => {
                            error!("failed to prepare mobile QR output: {err}");
                            eprintln!("failed to prepare mobile QR output: {err}");
                            return 1;
                        }
                    }
                }

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
