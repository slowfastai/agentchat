use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use agentchat_core::agent_manager::AgentManager;
use agentchat_core::distiller::Distiller;
use agentchat_core::relay_client::{
    RelayClientConfig, RelayClientCryptoConfig, DEFAULT_RELAY_USER_AGENT,
};
use agentchat_core::run::supervisor::{SharedSupervisor, SupervisorGate, SupervisorProgress};
use agentchat_core::run::{
    FileApprovalGate, PromptSet, RoleAgent, RunOrchestrator, RunRoles, RunState, TerminalProgress,
};
use agentchat_core::session_store::SessionStore;
use agentchat_core::skills::SkillStore;
use agentchat_protocol::relay_crypto::{
    decode_base64url_exact, ed25519_public_key, seed_from_label,
};
use agentchat_protocol::run::PhaseKind;
use agentchat_protocol::DaemonStopReason;
use agentchat_protocol::{AgentConfig, AgentStatus, AgentSummary};
use agentchat_server::console::{self, shared_supervisor, RunLauncher, StartRequest};
use agentchat_server::http;
use agentchat_server::relay::RelayTransportServer;
use agentchat_server::ws::WebSocketServer;
use if_addrs::{get_if_addrs, IfAddr, Interface};
use qrcode::{render::unicode, QrCode};
use serde::Deserialize;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};
use tracing_subscriber::fmt::writer::MakeWriter;

const DEV_DAEMON_IDENTITY_LABEL: &str = "agentchat-dev-daemon-identity-v1";
const DEV_APP_IDENTITY_LABEL: &str = "agentchat-dev-app-identity-v1";

const DEFAULT_PORT: u16 = 9390;
const DEFAULT_MANAGED_AGENTCHAT_HOME_RELATIVE: &str = "Library/Application Support/AgentChat";
const DEFAULT_MANAGED_AGENTS_JSON: &str = r#"[
  {
    "id": "codex",
    "name": "Codex",
    "backend": "codex_app_server",
    "command": "codex",
    "args": []
  },
  {
    "id": "opencode",
    "name": "OpenCode",
    "backend": "acp",
    "command": "opencode",
    "args": ["acp"]
  },
  {
    "id": "claude-code",
    "name": "Claude Code",
    "backend": "acp",
    "command": "npx",
    "args": ["--yes", "@agentclientprotocol/claude-agent-acp"]
  },
  {
    "id": "pi",
    "name": "Pi",
    "backend": "acp",
    "command": "npx",
    "args": ["--yes", "pi-acp"]
  }
]"#;

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

#[derive(Debug, Clone)]
struct DaemonPaths {
    home_dir: PathBuf,
    agents_file: PathBuf,
    sessions_dir: PathBuf,
    skills_dir: PathBuf,
    log_path: PathBuf,
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

impl DaemonPaths {
    fn resolve(project_root: &Path) -> Result<Self, String> {
        let home_dir = if let Some(home) = optional_env("AGENTCHAT_HOME") {
            PathBuf::from(home)
        } else {
            project_root.to_path_buf()
        };

        let agents_file = optional_env("AGENTCHAT_AGENTS_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if optional_env("AGENTCHAT_HOME").is_some() {
                    home_dir.join("config").join("agents.json")
                } else {
                    project_root.join(".agentchat").join("agents.json")
                }
            });

        let log_path = optional_env("AGENTCHAT_LOG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                if optional_env("AGENTCHAT_HOME").is_some() {
                    home_dir.join("logs").join("agentchat-daemon.log")
                } else {
                    project_root
                        .join(".agentchat")
                        .join("logs")
                        .join("agentchat-daemon.log")
                }
            });

        let base_data_dir = if optional_env("AGENTCHAT_HOME").is_some() {
            home_dir.join("data")
        } else {
            project_root.join(".agentchat")
        };

        let paths = Self {
            home_dir,
            agents_file,
            sessions_dir: base_data_dir.join("sessions"),
            skills_dir: base_data_dir.join("skills"),
            log_path,
        };

        paths.validate()?;
        Ok(paths)
    }

    fn managed_default(home_directory: &Path) -> Self {
        let home_dir = home_directory.join(DEFAULT_MANAGED_AGENTCHAT_HOME_RELATIVE);
        Self {
            agents_file: home_dir.join("config").join("agents.json"),
            sessions_dir: home_dir.join("data").join("sessions"),
            skills_dir: home_dir.join("data").join("skills"),
            log_path: home_dir.join("logs").join("agentchat-daemon.log"),
            home_dir,
        }
    }

    fn ensure_managed_layout(&self) -> Result<(), String> {
        for dir in [
            &self.home_dir,
            self.agents_file.parent().ok_or_else(|| {
                format!("invalid agents file path: {}", self.agents_file.display())
            })?,
            self.sessions_dir.parent().ok_or_else(|| {
                format!("invalid sessions dir path: {}", self.sessions_dir.display())
            })?,
            &self.sessions_dir,
            &self.skills_dir,
            self.log_path
                .parent()
                .ok_or_else(|| format!("invalid daemon log path: {}", self.log_path.display()))?,
            &self.home_dir.join("cache"),
            &self.home_dir.join("run"),
        ] {
            fs::create_dir_all(dir).map_err(|err| {
                format!(
                    "failed to create managed daemon directory '{}': {err}",
                    dir.display()
                )
            })?;
        }

        if !self.agents_file.exists() {
            fs::write(&self.agents_file, DEFAULT_MANAGED_AGENTS_JSON).map_err(|err| {
                format!(
                    "failed to write default managed agents file '{}': {err}",
                    self.agents_file.display()
                )
            })?;
        }

        Ok(())
    }

    fn is_managed_home_enabled() -> bool {
        optional_env("AGENTCHAT_HOME").is_some()
    }

    fn validate(&self) -> Result<(), String> {
        if self.agents_file.is_dir() {
            return Err(format!(
                "daemon agents path points to a directory: {}",
                self.agents_file.display()
            ));
        }
        Ok(())
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

/// How long an agent gets to spawn and finish its handshake before the run
/// gives up on it and continues with the others.
const AGENT_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// Loopback port for the run console.
const DEFAULT_CONSOLE_PORT: u16 = 9391;

/// Starts runs on behalf of the console, using agents this process already has.
struct ConsoleLauncher {
    manager: Rc<RefCell<AgentManager>>,
    supervisor: SharedSupervisor,
    working_dir: PathBuf,
    prompts: PromptSet,
}

impl RunLauncher for ConsoleLauncher {
    fn agent_ids(&self) -> Vec<String> {
        self.manager.borrow().agent_ids()
    }

    fn working_dir(&self) -> PathBuf {
        self.working_dir.clone()
    }

    fn start(&self, request: StartRequest) -> Result<String, String> {
        let args = RunArgs {
            brief: PathBuf::new(),
            planner: request.planner.clone(),
            plan_reviewers: Some(request.plan_reviewers.clone()).filter(|r| !r.is_empty()),
            implementer: request.implementer.clone(),
            code_reviewers: Some(request.code_reviewers.clone()).filter(|r| !r.is_empty()),
            run_id: None,
            poll_secs: 0,
            plan_only: request.plan_only,
        };
        // Resolve roles before anything is created, so a bad selection is a
        // form error rather than a run that dies on its first stage.
        let roles = resolve_roles(&self.manager.borrow(), &args)?;

        let run_id = format!("run-{}", agentchat_protocol::now_millis());
        let working_dir = self.working_dir.clone();
        let supervisor = self.supervisor.clone();
        let prompts = self.prompts.clone();
        let plan_only = request.plan_only;
        let brief = request.brief.clone();
        let id = run_id.clone();

        tokio::task::spawn_local(async move {
            let outcome = drive_console_run(
                &working_dir,
                &id,
                prompts,
                supervisor.clone(),
                roles,
                brief,
                plan_only,
            )
            .await;
            let error = outcome.err();
            if let Some(message) = &error {
                supervisor
                    .borrow_mut()
                    .append_log(&id, format!("✗ run failed: {message}"));
                error!("run {id} failed: {message}");
            }
            supervisor.borrow_mut().finish(&id, error);
        });

        Ok(run_id)
    }
}

/// Drives one console-started run to its stopping point.
async fn drive_console_run(
    working_dir: &Path,
    run_id: &str,
    prompts: PromptSet,
    supervisor: SharedSupervisor,
    roles: RunRoles,
    brief: String,
    plan_only: bool,
) -> Result<(), String> {
    let mut orchestrator = RunOrchestrator::new(working_dir.to_path_buf(), run_id, prompts)
        .with_progress(Rc::new(SupervisorProgress::new(supervisor.clone(), run_id)));

    // The console supplies the brief as text; the run still owns a copy on disk
    // so human feedback can be appended to it between rounds.
    let brief_path = orchestrator.layout().brief();
    if let Some(parent) = brief_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    tokio::fs::write(&brief_path, brief)
        .await
        .map_err(|e| format!("cannot write {}: {e}", brief_path.display()))?;

    let mut run = RunState::new(run_id, working_dir.to_string_lossy());
    supervisor.borrow_mut().sync(&run);
    supervisor.borrow_mut().append_log(
        run_id,
        format!(
            "▶ started in {}{}",
            working_dir.display(),
            if plan_only { " (plan only)" } else { "" }
        ),
    );

    let gate = SupervisorGate::new(supervisor.clone(), run_id);
    let status = orchestrator
        .drive_until(
            &mut run,
            &roles,
            &gate,
            plan_only.then_some(PhaseKind::Plan),
        )
        .await?;

    supervisor.borrow_mut().sync(&run);
    supervisor
        .borrow_mut()
        .append_log(run_id, format!("■ finished: {}", status.as_str()));
    Ok(())
}

/// Arguments for the `run` subcommand.
struct RunArgs {
    brief: PathBuf,
    planner: Option<String>,
    plan_reviewers: Option<Vec<String>>,
    implementer: Option<String>,
    code_reviewers: Option<Vec<String>>,
    run_id: Option<String>,
    poll_secs: u64,
    plan_only: bool,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut brief = None;
    let mut planner = None;
    let mut plan_reviewers = None;
    let mut implementer = None;
    let mut code_reviewers = None;
    let mut run_id = None;
    let mut poll_secs = 5u64;
    let mut plan_only = false;

    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let mut value = || {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--brief" => brief = Some(PathBuf::from(value()?)),
            "--planner" => planner = Some(value()?),
            "--plan-reviewers" => plan_reviewers = Some(split_ids(&value()?)),
            "--implementer" => implementer = Some(value()?),
            "--code-reviewers" => code_reviewers = Some(split_ids(&value()?)),
            "--run-id" => run_id = Some(value()?),
            "--plan-only" => plan_only = true,
            "--poll-secs" => {
                poll_secs = value()?
                    .parse()
                    .map_err(|_| "--poll-secs needs a number".to_string())?
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}`\n\nRun `agentchat-daemon run --help` for usage."
                ))
            }
        }
    }

    Ok(RunArgs {
        brief: brief.ok_or("--brief is required")?,
        planner,
        plan_reviewers,
        implementer,
        code_reviewers,
        run_id,
        poll_secs,
        plan_only,
    })
}

fn parse_web_port(args: &[String]) -> Result<u16, String> {
    let mut port = DEFAULT_CONSOLE_PORT;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--port" => {
                port = iter
                    .next()
                    .ok_or("--port needs a value")?
                    .parse()
                    .map_err(|_| "--port needs a number".to_string())?
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}`\n\nRun `agentchat-daemon web --help` for usage."
                ))
            }
        }
    }
    Ok(port)
}

fn split_ids(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect()
}

/// Maps configured agents onto the roles a run needs.
///
/// Defaults are chosen so `run --brief x.md` works with no other flags: the
/// first configured agent authors, and everyone else reviews.
fn resolve_roles(manager: &AgentManager, args: &RunArgs) -> Result<RunRoles, String> {
    let ids = manager.agent_ids();
    let role = |id: &str| -> Result<RoleAgent, String> {
        manager
            .get_agent(id)
            .map(|backend| RoleAgent::new(id, backend))
            .ok_or_else(|| format!("unknown agent `{id}`; running agents: {}", ids.join(", ")))
    };
    let others = |exclude: &str| -> Vec<String> {
        ids.iter().filter(|id| *id != exclude).cloned().collect()
    };

    let planner_id = args
        .planner
        .clone()
        .or_else(|| ids.first().cloned())
        .ok_or("no agents available")?;
    let implementer_id = args
        .implementer
        .clone()
        .unwrap_or_else(|| planner_id.clone());

    let plan_reviewer_ids = args
        .plan_reviewers
        .clone()
        .unwrap_or_else(|| others(&planner_id));
    let code_reviewer_ids = args
        .code_reviewers
        .clone()
        .unwrap_or_else(|| others(&implementer_id));

    if plan_reviewer_ids.is_empty() || code_reviewer_ids.is_empty() {
        return Err(format!(
            "a run needs at least one reviewer besides the author, but only these agents started: {}\n\
             Configure another agent, or name reviewers explicitly with --plan-reviewers/--code-reviewers.",
            ids.join(", ")
        ));
    }

    Ok(RunRoles {
        planner: role(&planner_id)?,
        plan_reviewers: plan_reviewer_ids
            .iter()
            .map(|id| role(id))
            .collect::<Result<Vec<_>, _>>()?,
        implementer: role(&implementer_id)?,
        code_reviewers: code_reviewer_ids
            .iter()
            .map(|id| role(id))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

async fn execute_run(
    project_root: PathBuf,
    daemon_paths: &DaemonPaths,
    args: RunArgs,
) -> Result<(), String> {
    // Cheap checks before anything is spawned. Starting agents takes real time,
    // and discovering a missing brief afterwards looks exactly like a hang.
    if !args.brief.is_file() {
        return Err(format!(
            "brief not found: {}\n\
             Create it first — it is the requirement the agents work from, in your words.",
            args.brief.display()
        ));
    }

    let agent_configs = load_agent_configs(&project_root, daemon_paths)
        .map_err(|e| format!("failed to load agent configuration: {e}"))?;

    let mut manager = AgentManager::new();
    for config in agent_configs {
        let agent_id = config.id.clone();
        print!("starting agent '{agent_id}'… ");
        let _ = io::stdout().flush();

        // An agent waiting on an interactive login would otherwise wedge the run
        // with no indication of which one is stuck.
        match tokio::time::timeout(
            AGENT_STARTUP_TIMEOUT,
            manager.add_agent(config, project_root.clone()),
        )
        .await
        {
            Ok(Ok(())) => println!("ok"),
            Ok(Err(e)) => {
                println!("failed");
                warn!("skipping agent '{agent_id}': {e}");
                eprintln!("  {agent_id}: {e} — is it installed and logged in?");
            }
            Err(_) => {
                println!("timed out");
                warn!("agent '{agent_id}' did not initialize within {AGENT_STARTUP_TIMEOUT:?}");
                eprintln!(
                    "  {agent_id}: no response in {}s — try running it once by hand to finish login.",
                    AGENT_STARTUP_TIMEOUT.as_secs()
                );
            }
        }
    }
    if manager.is_empty() {
        return Err("no agents started successfully".into());
    }

    let roles = resolve_roles(&manager, &args)?;
    let run_id = args
        .run_id
        .clone()
        .unwrap_or_else(|| format!("run-{}", agentchat_protocol::now_millis()));

    let mut prompts = PromptSet::builtin();
    let overrides = prompts
        .load_overrides(&project_root.join(".agentchat").join("prompts"))
        .await?;
    if !overrides.is_empty() {
        println!("using project prompt overrides: {}", overrides.join(", "));
    }

    let mut orchestrator = RunOrchestrator::new(project_root.clone(), &run_id, prompts)
        .with_progress(Rc::new(TerminalProgress::new()));
    let (mut run, resumed) = orchestrator.load_or_start(&run_id, &project_root).await?;
    if resumed {
        println!("resuming {run_id} at {}", run.status.as_str());
    } else {
        orchestrator.import_brief(&args.brief).await?;
        println!("starting {run_id} in {}", project_root.display());
    }

    let gate = FileApprovalGate::new(orchestrator.layout().run_dir().to_path_buf())
        .with_poll_interval(Duration::from_secs(args.poll_secs));

    let stop_after = args.plan_only.then_some(PhaseKind::Plan);
    if args.plan_only {
        println!("plan-only: stopping after the plan is approved, no code will be written");
    }

    let status = orchestrator
        .drive_until(&mut run, &roles, &gate, stop_after)
        .await;
    manager.shutdown_all().await;

    let status = status?;
    println!("\nrun {run_id} finished: {}", status.as_str());
    println!("files: {}", orchestrator.layout().run_dir().display());
    if args.plan_only && !status.is_terminal() {
        println!("continue with: agentchat-daemon run --brief <file> --run-id {run_id}");
    }
    Ok(())
}

/// Boots agents, then serves the console until interrupted.
async fn execute_web(
    project_root: PathBuf,
    daemon_paths: &DaemonPaths,
    port: u16,
) -> Result<(), String> {
    let manager = Rc::new(RefCell::new(
        start_agents(&project_root, daemon_paths).await?,
    ));

    let mut prompts = PromptSet::builtin();
    let overrides = prompts
        .load_overrides(&project_root.join(".agentchat").join("prompts"))
        .await?;
    if !overrides.is_empty() {
        println!("using project prompt overrides: {}", overrides.join(", "));
    }

    let supervisor = shared_supervisor();
    let launcher = Rc::new(ConsoleLauncher {
        manager: manager.clone(),
        supervisor: supervisor.clone(),
        working_dir: project_root.clone(),
        prompts,
    });

    let session_store = Rc::new(RefCell::new(SessionStore::new_with_sessions_dir(
        daemon_paths.sessions_dir.clone(),
    )));
    let skill_store = Rc::new(SkillStore::new_with_skills_dir(
        daemon_paths.skills_dir.clone(),
    ));
    let distiller = Rc::new(Distiller::new(skill_store.clone()));
    let (shutdown_tx, shutdown_rx) = watch::channel::<Option<DaemonStopReason>>(None);
    let signal_tx = shutdown_tx.clone();
    tokio::task::spawn_local(async move {
        if let Err(err) = wait_for_shutdown_signal().await {
            error!("web command shutdown signal handler failed: {err}");
        }
        let _ = signal_tx.send(Some(DaemonStopReason::Signal));
    });

    let websocket_manager = manager.clone();
    let websocket_session_store = session_store.clone();
    let websocket_skill_store = skill_store.clone();
    let websocket_distiller = distiller.clone();
    let websocket_shutdown_rx = shutdown_rx.clone();
    let mut websocket_task = tokio::task::spawn_local(async move {
        WebSocketServer::new(DEFAULT_PORT)
            .run(
                websocket_manager,
                websocket_shutdown_rx,
                websocket_session_store,
                websocket_skill_store,
                websocket_distiller,
            )
            .await
    });

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    println!("\nconsole:  http://{addr}");
    println!("chat:     http://{addr}/chat");
    println!("socket:   ws://0.0.0.0:{DEFAULT_PORT}");
    println!("working:  {}", project_root.display());
    println!("\nOpen /chat to talk with agents, or / for the run console.\n");

    let http_server = http::serve(addr, console::handler(supervisor, launcher));
    tokio::pin!(http_server);

    tokio::select! {
        websocket_result = &mut websocket_task => {
            match websocket_result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(err)) => Err(err),
                Err(err) => Err(format!("websocket task failed: {err}")),
            }
        }
        http_result = &mut http_server => {
            let _ = shutdown_tx.send(Some(DaemonStopReason::UserShutdown));
            let _ = websocket_task.await;
            http_result.map_err(|err| http::describe_bind_error(addr, &err))
        }
    }
}

/// Spawns every configured agent, reporting each one as it comes up.
async fn start_agents(
    project_root: &Path,
    daemon_paths: &DaemonPaths,
) -> Result<AgentManager, String> {
    let agent_configs = load_agent_configs(project_root, daemon_paths)
        .map_err(|e| format!("failed to load agent configuration: {e}"))?;

    let mut manager = AgentManager::new();
    for config in agent_configs {
        let agent_id = config.id.clone();
        print!("starting agent '{agent_id}'… ");
        let _ = io::stdout().flush();

        // An agent waiting on an interactive login would otherwise wedge startup
        // with no indication of which one is stuck.
        match tokio::time::timeout(
            AGENT_STARTUP_TIMEOUT,
            manager.add_agent(config, project_root.to_path_buf()),
        )
        .await
        {
            Ok(Ok(())) => println!("ok"),
            Ok(Err(e)) => {
                println!("failed");
                warn!("skipping agent '{agent_id}': {e}");
                eprintln!("  {agent_id}: {e} — is it installed and logged in?");
            }
            Err(_) => {
                println!("timed out");
                warn!("agent '{agent_id}' did not initialize within {AGENT_STARTUP_TIMEOUT:?}");
                eprintln!(
                    "  {agent_id}: no response in {}s — try running it once by hand to finish login.",
                    AGENT_STARTUP_TIMEOUT.as_secs()
                );
            }
        }
    }

    if manager.is_empty() {
        return Err("no agents started successfully".into());
    }
    Ok(manager)
}

fn print_web_usage() {
    println!(
        r#"agentchat-daemon web — run console in your browser

Usage:
  agentchat-daemon web [--port <n>]

Run this from inside the working tree you want changed. Preparing an isolated
worktree is up to you:

  git worktree add ../feature-a -b feature-a
  cd ../feature-a
  agentchat-daemon web

Then open http://127.0.0.1:{DEFAULT_CONSOLE_PORT}. The page lets you paste a brief, pick which
agent plans and which review, watch the run live, and answer at the two
approval gates.

Options:
  --port <n>    Loopback port for the console. Default {DEFAULT_CONSOLE_PORT}.

The console binds loopback only — nothing is reachable from the network."#
    );
}

fn print_run_usage() {
    println!(
        r#"agentchat-daemon run — drive a brief through plan and code review

Usage:
  agentchat-daemon run --brief <file> [options]

Run this from inside the working tree you want changed. Preparing an isolated
worktree is up to you:

  git worktree add ../feature-a -b feature-a
  cd ../feature-a
  agentchat-daemon run --brief ./requirement.md

Options:
  --brief <file>            Requirement to work from. Required.
  --planner <agent-id>      Writes the plan. Defaults to the first agent.
  --plan-reviewers <a,b,c>  Review the plan. Defaults to every other agent.
  --implementer <agent-id>  Writes the code. Defaults to the planner.
  --code-reviewers <a,b>    Review the code. Defaults to every other agent.
  --run-id <id>             Reuse an id to resume an interrupted run.
  --plan-only               Stop once the plan is approved. Nothing is written
                            to your working tree. Good for a first outing —
                            continue later with the same --run-id.
  --poll-secs <n>           How often to check for your decision. Default 5.

The run pauses twice, once for the plan and once for the code. Each time it
writes an approval page next to the run and waits for you to answer:

  echo '{{"decision":"approve"}}' > .agentchat/runs/<id>/decision-plan.json
  echo '{{"decision":"request_changes","comments":"..."}}' > ...
  echo '{{"decision":"cancel"}}' > ...

Everything is under .agentchat/runs/<id>/, and progress is checkpointed after
every stage, so an interrupted run resumes where it stopped."#
    );
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
        "agentchat-daemon\n\nUsage:\n  agentchat-daemon [--mobile]        Start the daemon for the app to connect to\n  agentchat-daemon web               Run console in your browser (recommended).\n                                     See `agentchat-daemon web --help`.\n  agentchat-daemon run --brief <file>\n                                     Same pipeline from the terminal.\n                                     See `agentchat-daemon run --help`.\n\nOptions:\n  --mobile            Print a terminal QR code for the current direct or relay connection so the iOS app can scan it\n  -h, --help          Show this help text\n\nEnvironment:\n  AGENTCHAT_HOME            Managed daemon home, for example ~/Library/Application Support/AgentChat\n  AGENTCHAT_AGENTS_FILE     Override the daemon-owned agents.json path\n  AGENTCHAT_MOBILE_WS_URL   Override the websocket endpoint embedded in the QR payload (must be ws://... or wss://...)\n  AGENTCHAT_AGENT_BACKEND   Select the agent backend adapter for single-agent mode\n\nAgent config precedence:\n  1. AGENTCHAT_AGENTS_JSON\n  2. AGENTCHAT_AGENTS_FILE or $AGENTCHAT_HOME/config/agents.json\n  3. .agentchat/agents.json\n  4. Single-agent AGENTCHAT_AGENT_* env vars\n  5. Built-in defaults (Codex only)\n\nExamples:\n  cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon\n    Starts the default Codex agent\n\n  AGENTCHAT_HOME=\"$HOME/Library/Application Support/AgentChat\" \\\n  cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon\n\n  AGENTCHAT_AGENT_ID=opencode \\\n  AGENTCHAT_AGENT_NAME=\"OpenCode (ACP)\" \\\n  AGENTCHAT_AGENT_BACKEND=acp \\\n  AGENTCHAT_AGENT_COMMAND=opencode \\\n  AGENTCHAT_AGENT_ARGS=\"acp\" \\\n  cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- --mobile"
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

fn built_in_agent_configs() -> Vec<AgentConfig> {
    vec![
        AgentConfig {
            id: "codex".into(),
            name: "Codex".into(),
            backend: "codex_app_server".into(),
            command: "codex".into(),
            args: vec![],
            working_dir: None,
            env_vars: Default::default(),
            extra: Default::default(),
        },
        AgentConfig {
            id: "opencode".into(),
            name: "OpenCode".into(),
            backend: "acp".into(),
            command: "opencode".into(),
            args: vec!["acp".into()],
            working_dir: None,
            env_vars: Default::default(),
            extra: Default::default(),
        },
        AgentConfig {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            backend: "acp".into(),
            command: "npx".into(),
            args: vec![
                "--yes".into(),
                "@agentclientprotocol/claude-agent-acp".into(),
            ],
            working_dir: None,
            env_vars: Default::default(),
            extra: Default::default(),
        },
        AgentConfig {
            id: "pi".into(),
            name: "Pi".into(),
            backend: "acp".into(),
            command: "npx".into(),
            args: vec!["--yes".into(), "pi-acp".into()],
            working_dir: None,
            env_vars: Default::default(),
            extra: Default::default(),
        },
    ]
}

fn local_agent_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".agentchat").join("agents.json")
}

fn load_agent_configs_from_json(source: &str, raw: &str) -> Result<Vec<AgentConfig>, String> {
    let configs: Vec<AgentConfig> =
        serde_json::from_str(raw).map_err(|err| format!("failed to parse {source}: {err}"))?;
    if configs.is_empty() {
        return Err(format!("{source} must contain at least one agent config"));
    }
    Ok(configs)
}

fn load_agent_configs(
    project_root: &Path,
    daemon_paths: &DaemonPaths,
) -> Result<Vec<AgentConfig>, String> {
    if let Some(raw) = optional_env("AGENTCHAT_AGENTS_JSON") {
        return load_agent_configs_from_json("AGENTCHAT_AGENTS_JSON", &raw);
    }

    let daemon_owned_config_path = daemon_paths.agents_file.as_path();
    if daemon_owned_config_path.exists() {
        let raw = fs::read_to_string(daemon_owned_config_path).map_err(|err| {
            format!(
                "failed to read {}: {err}",
                daemon_owned_config_path.display()
            )
        })?;
        return load_agent_configs_from_json(&daemon_owned_config_path.display().to_string(), &raw);
    }

    let local_config_path = local_agent_config_path(project_root);
    if local_config_path.exists() {
        let raw = fs::read_to_string(&local_config_path)
            .map_err(|err| format!("failed to read {}: {err}", local_config_path.display()))?;
        return load_agent_configs_from_json(&format!("{}", local_config_path.display()), &raw);
    }

    if optional_env("AGENTCHAT_AGENT_COMMAND").is_some()
        || optional_env("AGENTCHAT_AGENT_ID").is_some()
    {
        return Ok(vec![load_agent_config()]);
    }

    Ok(built_in_agent_configs())
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

fn init_tracing(daemon_paths: &DaemonPaths) -> Result<PathBuf, String> {
    let log_path = daemon_paths.log_path.clone();
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
    fn built_in_agent_configs_include_all_supported_agents() {
        let configs = built_in_agent_configs();
        let ids = configs
            .into_iter()
            .map(|config| config.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["codex", "opencode", "claude-code", "pi"]);
    }

    #[test]
    fn load_agent_configs_defaults_to_built_in_agents() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();

        let daemon_paths = daemon_paths_for_test(Path::new("/tmp"));
        let configs = load_agent_configs(Path::new("/tmp"), &daemon_paths)
            .expect("expected built-in configs");
        let ids = configs
            .into_iter()
            .map(|config| config.id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["codex", "opencode", "claude-code", "pi"]);
    }

    #[test]
    fn load_agent_configs_uses_single_agent_mode_when_command_is_set() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();
        env::set_var("AGENTCHAT_AGENT_COMMAND", "opencode");
        env::set_var("AGENTCHAT_AGENT_ID", "custom-opencode");

        let daemon_paths = daemon_paths_for_test(Path::new("/tmp"));
        let configs = load_agent_configs(Path::new("/tmp"), &daemon_paths)
            .expect("expected single-agent config");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].id, "custom-opencode");
        assert_eq!(configs[0].command, "opencode");

        clear_agent_config_env();
    }

    #[test]
    fn load_agent_configs_prefers_explicit_json_over_single_agent_env() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();
        env::set_var("AGENTCHAT_AGENT_COMMAND", "opencode");
        env::set_var(
            "AGENTCHAT_AGENTS_JSON",
            r#"[{"id":"pi","name":"Pi","backend":"acp","command":"npx","args":["--yes","pi-acp"]}]"#,
        );

        let daemon_paths = daemon_paths_for_test(Path::new("/tmp"));
        let configs =
            load_agent_configs(Path::new("/tmp"), &daemon_paths).expect("expected json config");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].id, "pi");
        assert_eq!(configs[0].command, "npx");

        clear_agent_config_env();
    }

    #[test]
    fn load_agent_configs_prefers_local_project_file_over_single_agent_env() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();

        let test_root =
            std::env::temp_dir().join(format!("agentchat-daemon-test-{}", std::process::id()));
        let local_config_dir = test_root.join(".agentchat");
        fs::create_dir_all(&local_config_dir).expect("expected local config dir");
        fs::write(
            local_config_dir.join("agents.json"),
            r#"[{"id":"claude-code","name":"Claude Code","backend":"acp","command":"npx","args":["--yes","@agentclientprotocol/claude-agent-acp"],"env_vars":{"HTTP_PROXY":"http://127.0.0.1:7897","HTTPS_PROXY":"http://127.0.0.1:7897"}}]"#,
        )
        .expect("expected local config file");

        env::set_var("AGENTCHAT_AGENT_COMMAND", "opencode");
        env::set_var("AGENTCHAT_AGENT_ID", "custom-opencode");

        let daemon_paths = daemon_paths_for_test(&test_root);
        let configs =
            load_agent_configs(&test_root, &daemon_paths).expect("expected local project config");

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].id, "claude-code");
        assert_eq!(configs[0].command, "npx");
        assert_eq!(
            configs[0].env_vars.get("HTTP_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7897")
        );

        let _ = fs::remove_dir_all(&test_root);
        clear_agent_config_env();
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
    fn managed_daemon_paths_default_to_application_support_layout() {
        let home_dir = PathBuf::from("/Users/tester");
        let paths = DaemonPaths::managed_default(&home_dir);

        assert_eq!(
            paths.home_dir,
            PathBuf::from("/Users/tester/Library/Application Support/AgentChat")
        );
        assert_eq!(
            paths.agents_file,
            PathBuf::from("/Users/tester/Library/Application Support/AgentChat/config/agents.json")
        );
        assert_eq!(
            paths.log_path,
            PathBuf::from(
                "/Users/tester/Library/Application Support/AgentChat/logs/agentchat-daemon.log"
            )
        );
    }

    #[test]
    fn daemon_paths_use_project_agentchat_logs_directory_by_default() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();
        let root = PathBuf::from("/tmp/agentchat-project");
        let paths = DaemonPaths::resolve(&root).expect("expected project-root daemon paths");

        assert_eq!(
            paths.log_path,
            PathBuf::from("/tmp/agentchat-project/.agentchat/logs/agentchat-daemon.log")
        );
    }

    #[test]
    fn daemon_paths_prefer_environment_overrides() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        clear_agent_config_env();
        env::set_var("AGENTCHAT_HOME", "/tmp/managed-agentchat");
        env::set_var("AGENTCHAT_AGENTS_FILE", "/tmp/custom-agents.json");
        env::set_var("AGENTCHAT_LOG_PATH", "/tmp/custom-daemon.log");
        let root = PathBuf::from("/tmp/agentchat-project");
        let paths = DaemonPaths::resolve(&root).expect("expected overridden daemon paths");

        assert_eq!(paths.home_dir, PathBuf::from("/tmp/managed-agentchat"));
        assert_eq!(paths.agents_file, PathBuf::from("/tmp/custom-agents.json"));
        assert_eq!(paths.log_path, PathBuf::from("/tmp/custom-daemon.log"));

        env::remove_var("AGENTCHAT_HOME");
        env::remove_var("AGENTCHAT_AGENTS_FILE");
        env::remove_var("AGENTCHAT_LOG_PATH");
    }

    #[test]
    fn managed_layout_bootstraps_default_multi_agent_config() {
        let root = tempfile::tempdir().expect("temp dir");
        let paths = DaemonPaths::managed_default(root.path());

        paths
            .ensure_managed_layout()
            .expect("expected managed layout to initialize");

        let raw = fs::read_to_string(&paths.agents_file).expect("agents file");
        assert!(raw.contains("\"id\": \"codex\""));
        assert!(raw.contains("\"id\": \"opencode\""));
        assert!(raw.contains("\"id\": \"claude-code\""));
        assert!(raw.contains("\"id\": \"pi\""));
    }

    fn daemon_paths_for_test(project_root: &Path) -> DaemonPaths {
        DaemonPaths::resolve(project_root).expect("expected daemon paths")
    }

    fn clear_agent_config_env() {
        for key in [
            "AGENTCHAT_HOME",
            "AGENTCHAT_AGENTS_FILE",
            "AGENTCHAT_AGENTS_JSON",
            "AGENTCHAT_AGENT_ID",
            "AGENTCHAT_AGENT_NAME",
            "AGENTCHAT_AGENT_BACKEND",
            "AGENTCHAT_AGENT_COMMAND",
            "AGENTCHAT_AGENT_ARGS",
            "AGENTCHAT_AGENT_WORKING_DIR",
            "AGENTCHAT_AGENT_APPROVAL_POLICY",
            "AGENTCHAT_AGENT_APPROVAL_STRATEGY",
            "AGENTCHAT_AGENT_APPROVALS_REVIEWER",
            "AGENTCHAT_AGENT_SANDBOX",
            "AGENTCHAT_AGENT_EXPERIMENTAL_RAW_EVENTS",
            "AGENTCHAT_AGENT_PERSIST_EXTENDED_HISTORY",
            "AGENTCHAT_LOG_PATH",
        ] {
            env::remove_var(key);
        }
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
    let daemon_paths = match DaemonPaths::resolve(&project_root) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    if DaemonPaths::is_managed_home_enabled() {
        if let Err(err) = daemon_paths.ensure_managed_layout() {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }

    let raw_args: Vec<String> = env::args().skip(1).collect();
    if raw_args.first().map(String::as_str) == Some("web") {
        if raw_args.iter().any(|arg| arg == "-h" || arg == "--help") {
            print_web_usage();
            return;
        }
        let port = match parse_web_port(&raw_args[1..]) {
            Ok(port) => port,
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        };
        if let Err(err) = init_tracing(&daemon_paths) {
            eprintln!("{err}");
            std::process::exit(1);
        }

        // Agents and run state are `!Send`, so the console shares their thread.
        let local = tokio::task::LocalSet::new();
        if let Err(err) = local
            .run_until(execute_web(project_root, &daemon_paths, port))
            .await
        {
            error!("console failed: {err}");
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }

    if raw_args.first().map(String::as_str) == Some("run") {
        if raw_args.iter().any(|arg| arg == "-h" || arg == "--help") {
            print_run_usage();
            return;
        }
        let run_args = match parse_run_args(&raw_args[1..]) {
            Ok(args) => args,
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        };
        if let Err(err) = init_tracing(&daemon_paths) {
            eprintln!("{err}");
            std::process::exit(1);
        }

        // Agents are `!Send`, so the whole run lives on one thread.
        let local = tokio::task::LocalSet::new();
        let outcome = local
            .run_until(execute_run(project_root, &daemon_paths, run_args))
            .await;
        if let Err(err) = outcome {
            error!("run failed: {err}");
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }

    let cli_options = match parse_cli_options() {
        Ok(Some(options)) => options,
        Ok(None) => return,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    let log_path = match init_tracing(&daemon_paths) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    };

    info!("agentchat daemon v0.1.0");
    info!("daemon logs redirected to {}", log_path.display());

    // Launch one or more configured agent backends from the environment.
    let agent_configs = match load_agent_configs(&project_root, &daemon_paths) {
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
                    warn!("skipping agent '{agent_id}': {e}");
                    eprintln!("warning: agent '{agent_id}' could not start — is it installed?");
                }
            }

            if manager.is_empty() {
                error!("no agents started successfully");
                return 1;
            }

            let manager = Rc::new(RefCell::new(manager));
            let session_store = Rc::new(RefCell::new(SessionStore::new_with_sessions_dir(
                daemon_paths.sessions_dir.clone(),
            )));
            let skill_store = Rc::new(SkillStore::new_with_skills_dir(
                daemon_paths.skills_dir.clone(),
            ));
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
