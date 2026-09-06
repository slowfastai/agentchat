use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tracing::{debug, error, info, warn};

use agentchat_protocol::{
    AgentConfig, AgentSessionSettings, AgentSettingOption, AgentSettingValue,
};

use crate::backend::{AgentBackend, AgentNotification, AgentPromptResult, AgentUpdate};

type PendingRequests = Rc<RefCell<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
type TurnWaiters = Rc<RefCell<HashMap<String, oneshot::Sender<Result<AgentPromptResult, String>>>>>;
type CompletedTurns = Rc<RefCell<HashMap<String, Result<AgentPromptResult, String>>>>;
type ActiveTurns = Rc<RefCell<HashMap<String, String>>>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum CodexApprovalStrategy {
    Decline,
    Accept,
    AcceptForSession,
}

impl CodexApprovalStrategy {
    fn from_config(config: &AgentConfig) -> Result<Self, String> {
        let Some(value) = config_extra_value(config, "approval_strategy", "approvalStrategy")
        else {
            return Ok(Self::Decline);
        };

        let strategy = value
            .as_str()
            .ok_or_else(|| "codex approval_strategy must be a string".to_string())?;

        match strategy {
            "decline" => Ok(Self::Decline),
            "accept" | "approve" => Ok(Self::Accept),
            "accept_for_session" | "approve_for_session" => Ok(Self::AcceptForSession),
            other => Err(format!(
                "unsupported codex approval_strategy `{other}`; expected decline, accept, or accept_for_session"
            )),
        }
    }

    fn is_approved(&self) -> bool {
        !matches!(self, Self::Decline)
    }

    fn command_decision(&self) -> &'static str {
        match self {
            Self::Decline => "decline",
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
        }
    }

    fn file_change_decision(&self) -> &'static str {
        match self {
            Self::Decline => "decline",
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
        }
    }

    fn permission_scope(&self) -> &'static str {
        match self {
            Self::AcceptForSession => "session",
            Self::Decline | Self::Accept => "turn",
        }
    }

    fn resolved_status(&self) -> &'static str {
        match self {
            Self::Decline => "Declined",
            Self::Accept | Self::AcceptForSession => "Approved",
        }
    }

    fn permission_status(&self) -> &'static str {
        match self {
            Self::Decline => "Declined",
            Self::Accept | Self::AcceptForSession => "Granted",
        }
    }

    fn summary(&self) -> &'static str {
        match self {
            Self::Decline => "auto-declined by daemon policy",
            Self::Accept => "auto-approved for this turn by daemon policy",
            Self::AcceptForSession => "auto-approved for this session by daemon policy",
        }
    }
}

#[derive(Clone, Debug)]
struct CodexOptions {
    approval_policy: Value,
    approvals_reviewer: Option<String>,
    sandbox: String,
    experimental_raw_events: bool,
    persist_extended_history: bool,
    approval_strategy: CodexApprovalStrategy,
    default_settings: AgentSessionSettings,
}

impl CodexOptions {
    fn from_config(config: &AgentConfig) -> Result<Self, String> {
        let approval_policy = match config_extra_value(config, "approval_policy", "approvalPolicy")
        {
            Some(Value::String(policy)) => {
                if matches!(
                    policy.as_str(),
                    "untrusted" | "on-failure" | "on-request" | "never"
                ) {
                    Value::String(policy.clone())
                } else {
                    return Err(format!(
                            "unsupported codex approval_policy `{policy}`; expected untrusted, on-failure, on-request, or never"
                        ));
                }
            }
            Some(Value::Object(policy)) => Value::Object(policy.clone()),
            Some(Value::Null) | None => Value::String("never".into()),
            Some(_) => {
                return Err(
                    "codex approval_policy must be a string or granular approval object".into(),
                )
            }
        };

        let approvals_reviewer =
            match config_extra_value(config, "approvals_reviewer", "approvalsReviewer") {
                Some(Value::String(reviewer))
                    if matches!(reviewer.as_str(), "user" | "guardian_subagent") =>
                {
                    Some(reviewer.clone())
                }
                Some(Value::Null) | None => None,
                Some(Value::String(reviewer)) => {
                    return Err(format!(
                        "unsupported codex approvals_reviewer `{reviewer}`; expected user or guardian_subagent"
                    ))
                }
                Some(_) => {
                    return Err("codex approvals_reviewer must be a string".into());
                }
            };

        let sandbox = match config_extra_value(config, "sandbox", "sandbox") {
            Some(Value::String(mode))
                if matches!(
                    mode.as_str(),
                    "read-only" | "workspace-write" | "danger-full-access"
                ) =>
            {
                mode.clone()
            }
            Some(Value::Null) | None => "workspace-write".into(),
            Some(Value::String(mode)) => {
                return Err(format!(
                    "unsupported codex sandbox `{mode}`; expected read-only, workspace-write, or danger-full-access"
                ))
            }
            Some(_) => {
                return Err("codex sandbox must be a string".into());
            }
        };

        Ok(Self {
            approval_policy,
            approvals_reviewer,
            sandbox,
            experimental_raw_events: config_extra_bool(
                config,
                "experimental_raw_events",
                "experimentalRawEvents",
                false,
            )?,
            persist_extended_history: config_extra_bool(
                config,
                "persist_extended_history",
                "persistExtendedHistory",
                false,
            )?,
            approval_strategy: CodexApprovalStrategy::from_config(config)?,
            default_settings: AgentSessionSettings {
                model: config_extra_string(config, &["model", "default_model"]),
                reasoning_effort: config_extra_string(
                    config,
                    &["reasoning_effort", "default_reasoning_effort"],
                ),
            },
        })
    }

    fn approval_policy_summary(&self) -> String {
        match &self.approval_policy {
            Value::String(policy) => policy.clone(),
            other => serde_json::to_string(other).unwrap_or_else(|_| "granular".into()),
        }
    }
}

fn config_extra_value<'a>(
    config: &'a AgentConfig,
    snake_case: &str,
    camel_case: &str,
) -> Option<&'a Value> {
    config
        .extra
        .get(snake_case)
        .or_else(|| config.extra.get(camel_case))
}

fn config_extra_bool(
    config: &AgentConfig,
    snake_case: &str,
    camel_case: &str,
    default: bool,
) -> Result<bool, String> {
    match config_extra_value(config, snake_case, camel_case) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Null) | None => Ok(default),
        Some(_) => Err(format!("codex {snake_case} must be a boolean")),
    }
}

fn config_extra_string(config: &AgentConfig, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        config
            .extra
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[derive(Clone, Debug)]
struct RequestFlowUpdate {
    thread_id: String,
    requested_update: AgentUpdate,
    resolved_update: AgentUpdate,
}

/// Codex app-server backend using stdio JSON-RPC.
pub struct CodexAppServerAgent {
    writer: Rc<Mutex<ChildStdin>>,
    pending_requests: PendingRequests,
    active_turns: ActiveTurns,
    turn_waiters: TurnWaiters,
    completed_turns: CompletedTurns,
    update_rx: RefCell<Option<mpsc::UnboundedReceiver<AgentNotification>>>,
    next_request_id: AtomicU64,
    health_tx: watch::Sender<bool>,
    kill_tx: watch::Sender<bool>,
    project_root: PathBuf,
    options: CodexOptions,
    discovered_settings: Rc<RefCell<Vec<AgentSettingOption>>>,
    session_settings: Rc<RefCell<HashMap<String, AgentSessionSettings>>>,
}

async fn monitor_agent_process(
    mut child: tokio::process::Child,
    mut kill_rx: watch::Receiver<bool>,
    health_tx: watch::Sender<bool>,
    pending_requests: PendingRequests,
    turn_waiters: TurnWaiters,
) {
    tokio::select! {
        status = child.wait() => {
            match status {
                Ok(status) => info!("codex app-server exited with status {status}"),
                Err(e) => error!("failed waiting for codex app-server: {e}"),
            }
        }
        changed = kill_rx.changed() => {
            match changed {
                Ok(()) if *kill_rx.borrow() => {
                    info!("shutting down codex app-server process");
                    if let Err(e) = child.kill().await {
                        warn!("failed to kill codex app-server: {e}");
                    }
                    if let Err(e) = child.wait().await {
                        error!("failed waiting for killed codex app-server: {e}");
                    }
                }
                Ok(()) => {
                    debug!("received unexpected codex app-server kill signal state");
                }
                Err(_) => {
                    warn!("codex app-server kill signal dropped; terminating child process");
                    if let Err(e) = child.kill().await {
                        warn!("failed to kill orphaned codex app-server: {e}");
                    }
                    if let Err(e) = child.wait().await {
                        error!("failed waiting for orphaned codex app-server: {e}");
                    }
                }
            }
        }
    }

    fail_outstanding_requests(&pending_requests, "codex app-server disconnected");
    fail_outstanding_turns(&turn_waiters, "codex app-server disconnected");
    let _ = health_tx.send(false);
}

fn fail_outstanding_requests(pending_requests: &PendingRequests, message: &str) {
    for (_, tx) in pending_requests.borrow_mut().drain() {
        let _ = tx.send(Err(message.to_string()));
    }
}

fn fail_outstanding_turns(turn_waiters: &TurnWaiters, message: &str) {
    for (_, tx) in turn_waiters.borrow_mut().drain() {
        let _ = tx.send(Err(message.to_string()));
    }
}

fn prepare_codex_args(command: &str, args: &[String]) -> Vec<String> {
    let mut prepared = args.to_vec();
    let command_name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command);

    if matches!(command_name, "codex" | "codex.exe")
        && prepared.first().map(String::as_str) != Some("app-server")
    {
        prepared.insert(0, "app-server".into());
    }

    let has_listen = prepared
        .iter()
        .any(|arg| arg == "--listen" || arg.starts_with("--listen="));
    if !has_listen {
        prepared.push("--listen".into());
        prepared.push("stdio://".into());
    }

    prepared
}

async fn write_json(writer: &Rc<Mutex<ChildStdin>>, value: &Value) -> Result<(), String> {
    let line = serde_json::to_string(value)
        .map_err(|e| format!("failed to encode json-rpc message: {e}"))?;
    let mut writer = writer.lock().await;
    writer
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("failed to write codex request: {e}"))?;
    writer
        .write_all(b"\n")
        .await
        .map_err(|e| format!("failed to terminate codex request line: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("failed to flush codex request: {e}"))?;
    Ok(())
}

fn extract_jsonrpc_error(value: &Value) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("data"))
                .map(|data| data.to_string())
        })
        .unwrap_or_else(|| "unknown json-rpc error".into())
}

fn normalize_tool_status(status: &str) -> String {
    match status {
        "inProgress" => "InProgress".into(),
        "completed" => "Completed".into(),
        "failed" => "Failed".into(),
        "declined" => "Declined".into(),
        "interrupted" => "Cancelled".into(),
        other => other.to_string(),
    }
}

fn tool_update_from_thread_item(item: &Value, default_status: Option<&str>) -> Option<AgentUpdate> {
    let item_type = item.get("type")?.as_str()?;
    let item_id = item.get("id")?.as_str()?.to_string();

    let (title, status, content) = match item_type {
        "commandExecution" => {
            let title = item
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("Command Execution")
                .to_string();
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            let content = item
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .map(str::to_string);
            (title, status, content)
        }
        "fileChange" => {
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            ("File Change".into(), status, None)
        }
        "mcpToolCall" => {
            let tool = item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("MCP Tool");
            let server = item.get("server").and_then(Value::as_str).unwrap_or("mcp");
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            (format!("{server}:{tool}"), status, None)
        }
        "dynamicToolCall" => {
            let title = item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("Dynamic Tool");
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            (title.to_string(), status, None)
        }
        "collabAgentToolCall" => {
            let title = item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("Collab Agent Tool");
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            (title.to_string(), status, None)
        }
        "webSearch" => {
            let title = item
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("Web Search");
            let status = default_status.unwrap_or("InProgress").to_string();
            (title.to_string(), status, None)
        }
        "imageGeneration" => {
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(normalize_tool_status)
                .or_else(|| default_status.map(str::to_string))
                .unwrap_or_else(|| "InProgress".into());
            ("Image Generation".into(), status, None)
        }
        _ => return None,
    };

    Some(AgentUpdate::ToolUpdate {
        tool_call_id: item_id,
        title,
        status,
        content,
    })
}

fn prompt_result_from_turn(turn: &Value) -> Result<Option<AgentPromptResult>, String> {
    match turn.get("status").and_then(Value::as_str) {
        Some("completed") => Ok(Some(AgentPromptResult::new("EndTurn"))),
        Some("interrupted") => Ok(Some(AgentPromptResult::new("Cancelled"))),
        Some("failed") => Err(turn
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("turn failed")
            .to_string()),
        Some("inProgress") => Ok(None),
        Some(other) => Ok(Some(AgentPromptResult::new(other))),
        None => Err("turn status missing".into()),
    }
}

fn finish_turn(
    thread_id: &str,
    turn_id: &str,
    result: Result<AgentPromptResult, String>,
    active_turns: &ActiveTurns,
    turn_waiters: &TurnWaiters,
    completed_turns: &CompletedTurns,
) {
    {
        let mut active = active_turns.borrow_mut();
        if active.get(thread_id).map(String::as_str) == Some(turn_id) {
            active.remove(thread_id);
        }
    }

    if let Some(waiter) = turn_waiters.borrow_mut().remove(turn_id) {
        let _ = waiter.send(result);
    } else {
        completed_turns
            .borrow_mut()
            .insert(turn_id.to_string(), result);
    }
}

fn tool_update(
    tool_call_id: String,
    title: String,
    status: &str,
    content: Option<String>,
) -> AgentUpdate {
    AgentUpdate::ToolUpdate {
        tool_call_id,
        title,
        status: status.into(),
        content,
    }
}

fn push_summary_line(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    lines.push(format!("{label}: {value}"));
}

fn compact_json(value: &Value) -> Option<String> {
    if value.is_null() {
        None
    } else {
        serde_json::to_string(value).ok()
    }
}

fn request_id_from_params(prefix: &str, request_id: u64, params: &Value) -> String {
    params
        .get("approvalId")
        .and_then(Value::as_str)
        .or_else(|| params.get("itemId").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| format!("{prefix}-{request_id}"))
}

fn request_thread_id(params: &Value) -> Option<String> {
    params
        .get("threadId")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn command_request_flow(
    request_id: u64,
    params: &Value,
    options: &CodexOptions,
) -> Option<RequestFlowUpdate> {
    let thread_id = request_thread_id(params)?;
    let tool_call_id = request_id_from_params("command-approval", request_id, params);
    let title = params
        .get("command")
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .unwrap_or("Command Approval")
        .to_string();
    let mut lines = vec!["Codex requested approval for a command execution.".into()];
    push_summary_line(
        &mut lines,
        "Command",
        params.get("command").and_then(Value::as_str),
    );
    push_summary_line(
        &mut lines,
        "Working dir",
        params.get("cwd").and_then(Value::as_str),
    );
    push_summary_line(
        &mut lines,
        "Reason",
        params.get("reason").and_then(Value::as_str),
    );
    if let Some(actions) = params.get("commandActions").and_then(compact_json) {
        lines.push(format!("Actions: {actions}"));
    }
    lines.push(format!(
        "Approval policy: {}",
        options.approval_policy_summary()
    ));

    let requested_content = Some(lines.join("\n"));
    lines.push(format!(
        "Decision: {}.",
        options.approval_strategy.summary()
    ));
    let resolved_content = Some(lines.join("\n"));

    Some(RequestFlowUpdate {
        thread_id,
        requested_update: tool_update(
            tool_call_id.clone(),
            title.clone(),
            "NeedsApproval",
            requested_content,
        ),
        resolved_update: tool_update(
            tool_call_id,
            title,
            options.approval_strategy.resolved_status(),
            resolved_content,
        ),
    })
}

fn file_change_request_flow(
    request_id: u64,
    params: &Value,
    options: &CodexOptions,
) -> Option<RequestFlowUpdate> {
    let thread_id = request_thread_id(params)?;
    let tool_call_id = request_id_from_params("file-change-approval", request_id, params);
    let mut lines = vec!["Codex requested approval for file changes.".into()];
    push_summary_line(
        &mut lines,
        "Grant root",
        params.get("grantRoot").and_then(Value::as_str),
    );
    push_summary_line(
        &mut lines,
        "Reason",
        params.get("reason").and_then(Value::as_str),
    );
    lines.push(format!(
        "Approval policy: {}",
        options.approval_policy_summary()
    ));

    let requested_content = Some(lines.join("\n"));
    lines.push(format!(
        "Decision: {}.",
        options.approval_strategy.summary()
    ));
    let resolved_content = Some(lines.join("\n"));

    Some(RequestFlowUpdate {
        thread_id,
        requested_update: tool_update(
            tool_call_id.clone(),
            "File Change Approval".into(),
            "NeedsApproval",
            requested_content,
        ),
        resolved_update: tool_update(
            tool_call_id,
            "File Change Approval".into(),
            options.approval_strategy.resolved_status(),
            resolved_content,
        ),
    })
}

fn permissions_request_flow(
    request_id: u64,
    params: &Value,
    options: &CodexOptions,
) -> Option<RequestFlowUpdate> {
    let thread_id = request_thread_id(params)?;
    let tool_call_id = request_id_from_params("permissions-approval", request_id, params);
    let mut lines = vec!["Codex requested additional sandbox permissions.".into()];
    push_summary_line(
        &mut lines,
        "Reason",
        params.get("reason").and_then(Value::as_str),
    );
    if let Some(permissions) = params.get("permissions").and_then(compact_json) {
        lines.push(format!("Permissions: {permissions}"));
    }
    lines.push(format!(
        "Approval policy: {}",
        options.approval_policy_summary()
    ));

    let requested_content = Some(lines.join("\n"));
    lines.push(format!(
        "Decision: {}.",
        options.approval_strategy.summary()
    ));
    let resolved_content = Some(lines.join("\n"));

    Some(RequestFlowUpdate {
        thread_id,
        requested_update: tool_update(
            tool_call_id.clone(),
            "Permissions Approval".into(),
            "NeedsApproval",
            requested_content,
        ),
        resolved_update: tool_update(
            tool_call_id,
            "Permissions Approval".into(),
            options.approval_strategy.permission_status(),
            resolved_content,
        ),
    })
}

fn tool_user_input_flow(request_id: u64, params: &Value) -> Option<RequestFlowUpdate> {
    let thread_id = request_thread_id(params)?;
    let tool_call_id = request_id_from_params("tool-input", request_id, params);
    let mut lines = vec!["Codex requested user input for a tool call.".into()];
    if let Some(questions) = params.get("questions").and_then(compact_json) {
        lines.push(format!("Questions: {questions}"));
    }
    let requested_content = Some(lines.join("\n"));
    lines.push("Decision: daemon returned no answers.".into());
    let resolved_content = Some(lines.join("\n"));

    Some(RequestFlowUpdate {
        thread_id,
        requested_update: tool_update(
            tool_call_id.clone(),
            "Tool User Input".into(),
            "NeedsInput",
            requested_content,
        ),
        resolved_update: tool_update(
            tool_call_id,
            "Tool User Input".into(),
            "Skipped",
            resolved_content,
        ),
    })
}

fn mcp_elicitation_flow(request_id: u64, params: &Value) -> Option<RequestFlowUpdate> {
    let thread_id = request_thread_id(params)?;
    let tool_call_id = request_id_from_params("mcp-elicitation", request_id, params);
    let server_name = params
        .get("serverName")
        .and_then(Value::as_str)
        .unwrap_or("mcp");
    let mut lines = vec![format!(
        "Codex requested MCP elicitation from {server_name}."
    )];
    push_summary_line(
        &mut lines,
        "Message",
        params.get("message").and_then(Value::as_str),
    );
    push_summary_line(
        &mut lines,
        "Mode",
        params.get("mode").and_then(Value::as_str),
    );
    let requested_content = Some(lines.join("\n"));
    lines.push("Decision: daemon cancelled the elicitation.".into());
    let resolved_content = Some(lines.join("\n"));

    Some(RequestFlowUpdate {
        thread_id,
        requested_update: tool_update(
            tool_call_id.clone(),
            format!("MCP Input: {server_name}"),
            "NeedsInput",
            requested_content,
        ),
        resolved_update: tool_update(
            tool_call_id,
            format!("MCP Input: {server_name}"),
            "Cancelled",
            resolved_content,
        ),
    })
}

async fn handle_server_request(
    writer: Rc<Mutex<ChildStdin>>,
    update_tx: mpsc::UnboundedSender<AgentNotification>,
    id: u64,
    method: &str,
    params: Value,
    options: CodexOptions,
) -> Result<(), String> {
    let flow = match method {
        "item/commandExecution/requestApproval" => command_request_flow(id, &params, &options),
        "item/fileChange/requestApproval" => file_change_request_flow(id, &params, &options),
        "item/permissions/requestApproval" => permissions_request_flow(id, &params, &options),
        "item/tool/requestUserInput" => tool_user_input_flow(id, &params),
        "mcpServer/elicitation/request" => mcp_elicitation_flow(id, &params),
        _ => None,
    };

    if let Some(flow) = &flow {
        let _ = update_tx.send(AgentNotification::new(
            flow.thread_id.clone(),
            flow.requested_update.clone(),
        ));
    }

    let response = match method {
        "item/commandExecution/requestApproval" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "decision": options.approval_strategy.command_decision() }
        }),
        "item/fileChange/requestApproval" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "decision": options.approval_strategy.file_change_decision() }
        }),
        "item/permissions/requestApproval" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "permissions": if options.approval_strategy.is_approved() {
                    params.get("permissions").cloned().unwrap_or_else(|| json!({}))
                } else {
                    json!({})
                },
                "scope": options.approval_strategy.permission_scope()
            }
        }),
        "item/tool/requestUserInput" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "answers": {} }
        }),
        "mcpServer/elicitation/request" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "action": "cancel", "content": null, "_meta": null }
        }),
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("unsupported server request `{method}`") }
        }),
    };

    write_json(&writer, &response).await?;

    if let Some(flow) = flow {
        let _ = update_tx.send(AgentNotification::new(flow.thread_id, flow.resolved_update));
    }

    Ok(())
}

fn handle_notification(
    method: &str,
    params: &Value,
    update_tx: &mpsc::UnboundedSender<AgentNotification>,
    active_turns: &ActiveTurns,
    turn_waiters: &TurnWaiters,
    completed_turns: &CompletedTurns,
) {
    match method {
        "turn/started" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(turn_id) = params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
            else {
                return;
            };
            active_turns
                .borrow_mut()
                .insert(thread_id.to_string(), turn_id.to_string());
        }
        "turn/completed" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(turn) = params.get("turn") else {
                return;
            };
            let Some(turn_id) = turn.get("id").and_then(Value::as_str) else {
                return;
            };
            let result = match prompt_result_from_turn(turn) {
                Ok(Some(result)) => Ok(result),
                Ok(None) => return,
                Err(err) => Err(err),
            };
            finish_turn(
                thread_id,
                turn_id,
                result,
                active_turns,
                turn_waiters,
                completed_turns,
            );
        }
        "item/agentMessage/delta" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                return;
            };
            let _ = update_tx.send(AgentNotification::new(
                thread_id,
                AgentUpdate::TextDelta {
                    content: delta.to_string(),
                },
            ));
        }
        "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                return;
            };
            let _ = update_tx.send(AgentNotification::new(
                thread_id,
                AgentUpdate::ThinkingDelta {
                    content: delta.to_string(),
                },
            ));
        }
        "turn/plan/updated" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let _ = update_tx.send(AgentNotification::new(
                thread_id,
                AgentUpdate::Plan {
                    plan_json: json!({
                        "explanation": params.get("explanation").cloned().unwrap_or(Value::Null),
                        "plan": params.get("plan").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                    }),
                },
            ));
        }
        "item/started" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(item) = params.get("item") else {
                return;
            };
            if let Some(update) = tool_update_from_thread_item(item, Some("InProgress")) {
                let _ = update_tx.send(AgentNotification::new(thread_id, update));
            }
        }
        "item/completed" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(item) = params.get("item") else {
                return;
            };
            if let Some(update) = tool_update_from_thread_item(item, Some("Completed")) {
                let _ = update_tx.send(AgentNotification::new(thread_id, update));
            }
        }
        "item/commandExecution/outputDelta" | "command/exec/outputDelta" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(tool_call_id) = params.get("itemId").and_then(Value::as_str) else {
                return;
            };
            let Some(delta) = params.get("delta").and_then(Value::as_str) else {
                return;
            };
            let _ = update_tx.send(AgentNotification::new(
                thread_id,
                AgentUpdate::ToolUpdate {
                    tool_call_id: tool_call_id.to_string(),
                    title: "Command Execution".into(),
                    status: "InProgress".into(),
                    content: Some(delta.to_string()),
                },
            ));
        }
        "error" => {
            let Some(thread_id) = params.get("threadId").and_then(Value::as_str) else {
                return;
            };
            let Some(turn_id) = params.get("turnId").and_then(Value::as_str) else {
                return;
            };
            let message = params
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("codex app-server turn failed")
                .to_string();
            let will_retry = params
                .get("willRetry")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            if will_retry {
                let _ = update_tx.send(AgentNotification::new(
                    thread_id,
                    AgentUpdate::Raw {
                        payload: json!({
                            "method": method,
                            "params": params,
                        }),
                    },
                ));
            } else {
                finish_turn(
                    thread_id,
                    turn_id,
                    Err(message),
                    active_turns,
                    turn_waiters,
                    completed_turns,
                );
            }
        }
        _ => {}
    }
}

impl CodexAppServerAgent {
    pub fn spawn(config: &AgentConfig, project_root: PathBuf) -> Result<Self, String> {
        let options = CodexOptions::from_config(config)?;
        let mut cmd = Command::new(&config.command);
        cmd.args(prepare_codex_args(&config.command, &config.args));

        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        } else {
            cmd.current_dir(&project_root);
        }

        for (k, v) in &config.env_vars {
            cmd.env(k, v);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn codex app-server '{}': {e}", config.command))?;

        let stdin = child
            .stdin
            .take()
            .ok_or("failed to capture codex app-server stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("failed to capture codex app-server stdout")?;

        let writer = Rc::new(Mutex::new(stdin));
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        let pending_requests: PendingRequests = Rc::new(RefCell::new(HashMap::new()));
        let active_turns: ActiveTurns = Rc::new(RefCell::new(HashMap::new()));
        let turn_waiters: TurnWaiters = Rc::new(RefCell::new(HashMap::new()));
        let completed_turns: CompletedTurns = Rc::new(RefCell::new(HashMap::new()));
        let session_settings = Rc::new(RefCell::new(HashMap::new()));
        let discovered_settings = Rc::new(RefCell::new(Vec::new()));
        let (health_tx, _) = watch::channel(true);
        let (kill_tx, kill_rx) = watch::channel(false);

        {
            let writer = writer.clone();
            let pending_requests = pending_requests.clone();
            let active_turns = active_turns.clone();
            let turn_waiters = turn_waiters.clone();
            let completed_turns = completed_turns.clone();
            let update_tx_reader = update_tx.clone();
            let options = options.clone();
            let health_tx_reader = health_tx.clone();
            tokio::task::spawn_local(async move {
                let mut lines = BufReader::new(stdout).lines();

                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            let message: Value = match serde_json::from_str(trimmed) {
                                Ok(value) => value,
                                Err(err) => {
                                    warn!("failed to parse codex app-server json line: {err}");
                                    continue;
                                }
                            };

                            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                                if let Some(method) = message.get("method").and_then(Value::as_str)
                                {
                                    let writer = writer.clone();
                                    let update_tx = update_tx_reader.clone();
                                    let method = method.to_string();
                                    let params =
                                        message.get("params").cloned().unwrap_or(Value::Null);
                                    let options = options.clone();
                                    tokio::task::spawn_local(async move {
                                        if let Err(err) = handle_server_request(
                                            writer, update_tx, id, &method, params, options,
                                        )
                                        .await
                                        {
                                            warn!("failed to respond to codex server request `{method}`: {err}");
                                        }
                                    });
                                    continue;
                                }

                                if let Some(tx) = pending_requests.borrow_mut().remove(&id) {
                                    let result = if message.get("result").is_some() {
                                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                                    } else {
                                        Err(extract_jsonrpc_error(&message))
                                    };
                                    let _ = tx.send(result);
                                }
                                continue;
                            }

                            if let Some(method) = message.get("method").and_then(Value::as_str) {
                                handle_notification(
                                    method,
                                    message.get("params").unwrap_or(&Value::Null),
                                    &update_tx,
                                    &active_turns,
                                    &turn_waiters,
                                    &completed_turns,
                                );
                            }
                        }
                        Ok(None) => break,
                        Err(err) => {
                            warn!("failed reading codex app-server stdout: {err}");
                            break;
                        }
                    }
                }

                fail_outstanding_requests(&pending_requests, "codex app-server stdout closed");
                fail_outstanding_turns(&turn_waiters, "codex app-server stdout closed");
                let _ = health_tx_reader.send(false);
            });
        }

        tokio::task::spawn_local(monitor_agent_process(
            child,
            kill_rx,
            health_tx.clone(),
            pending_requests.clone(),
            turn_waiters.clone(),
        ));

        info!("spawned codex app-server process: {}", config.command);

        Ok(Self {
            writer,
            pending_requests,
            active_turns,
            turn_waiters,
            completed_turns,
            update_rx: RefCell::new(Some(update_rx)),
            next_request_id: AtomicU64::new(1),
            health_tx,
            kill_tx,
            project_root,
            options,
            discovered_settings,
            session_settings,
        })
    }

    async fn send_request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending_requests.borrow_mut().insert(id, tx);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        if let Err(err) = write_json(&self.writer, &request).await {
            self.pending_requests.borrow_mut().remove(&id);
            return Err(err);
        }

        rx.await
            .map_err(|_| format!("codex app-server dropped response for `{method}`"))?
    }

    fn resolve_cwd(&self, cwd: PathBuf) -> PathBuf {
        if cwd.is_absolute() {
            cwd
        } else {
            self.project_root.join(cwd)
        }
    }
}

#[async_trait::async_trait(?Send)]
impl AgentBackend for CodexAppServerAgent {
    async fn initialize(&self) -> Result<(), String> {
        self.send_request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "agentchat-daemon",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "experimentalApi": false
                }
            }),
        )
        .await?;

        let mut models = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = json!({ "includeHidden": false, "limit": 100 });
            if let Some(cursor) = cursor.as_ref() {
                params["cursor"] = Value::String(cursor.clone());
            }
            let result = match self.send_request("model/list", params).await {
                Ok(result) => result,
                Err(error) => {
                    warn!("codex model/list discovery unavailable: {error}");
                    return Ok(());
                }
            };
            let Some(page) = result.get("data").and_then(Value::as_array) else {
                warn!("codex model/list response missing data");
                return Ok(());
            };
            models.extend(page.iter().cloned());
            let next_cursor = result
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            if next_cursor == cursor {
                warn!("codex model/list returned a repeated cursor");
                break;
            }
            let Some(next_cursor) = next_cursor else {
                break;
            };
            cursor = Some(next_cursor);
        }

        let mut model_values: Vec<AgentSettingValue> = Vec::new();
        let mut reasoning_values_by_model = HashMap::new();
        let mut default_model = None;
        for model in &models {
            let Some(model_id) = model
                .get("model")
                .or_else(|| model.get("id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let label = model
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(model_id);
            if model_values.iter().any(|value| value.id == model_id) {
                continue;
            }
            model_values.push(AgentSettingValue {
                id: model_id.into(),
                label: label.into(),
            });
            if model.get("isDefault").and_then(Value::as_bool) == Some(true) {
                default_model = Some(model_id.to_string());
            }
            let mut reasoning_values: Vec<AgentSettingValue> = Vec::new();
            if let Some(options) = model
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
            {
                for option in options {
                    let Some(effort) = option.get("reasoningEffort").and_then(Value::as_str) else {
                        continue;
                    };
                    if !reasoning_values.iter().any(|value| value.id == effort) {
                        reasoning_values.push(AgentSettingValue {
                            id: effort.into(),
                            label: effort.into(),
                        });
                    }
                }
            }
            reasoning_values_by_model.insert(model_id.to_string(), reasoning_values);
        }
        if model_values.is_empty() {
            warn!("codex model/list returned no visible models");
            return Ok(());
        }

        let selected_model = self
            .options
            .default_settings
            .model
            .as_ref()
            .filter(|model| model_values.iter().any(|value| &value.id == *model))
            .map(|model| model.clone())
            .or_else(|| default_model.clone())
            .or_else(|| model_values.first().map(|value| value.id.clone()));
        let default_reasoning = selected_model.as_ref().and_then(|model| {
            models.iter().find_map(|entry| {
                let entry_model = entry
                    .get("model")
                    .or_else(|| entry.get("id"))
                    .and_then(Value::as_str);
                (entry_model == Some(model.as_str()))
                    .then(|| entry.get("defaultReasoningEffort").and_then(Value::as_str))
                    .flatten()
                    .map(str::to_string)
            })
        });
        let default_reasoning_values = selected_model
            .as_ref()
            .and_then(|model| reasoning_values_by_model.get(model).cloned())
            .unwrap_or_default();
        let mut settings = vec![AgentSettingOption {
            id: "model".into(),
            name: "Model".into(),
            category: "model".into(),
            values: model_values,
            values_by_model: None,
            current_value: default_model,
            apply_scope: "session".into(),
        }];
        if !reasoning_values_by_model.is_empty() {
            settings.push(AgentSettingOption {
                id: "reasoning_effort".into(),
                name: "Reasoning effort".into(),
                category: "thought_level".into(),
                values: default_reasoning_values,
                values_by_model: Some(reasoning_values_by_model),
                current_value: default_reasoning,
                apply_scope: "session".into(),
            });
        }
        *self.discovered_settings.borrow_mut() = settings;
        Ok(())
    }

    fn setting_options(&self) -> Vec<AgentSettingOption> {
        self.discovered_settings.borrow().clone()
    }

    async fn new_session(&self, cwd: PathBuf) -> Result<String, String> {
        self.new_session_with_settings(cwd, self.options.default_settings.clone())
            .await
    }

    async fn new_session_with_settings(
        &self,
        cwd: PathBuf,
        settings: AgentSessionSettings,
    ) -> Result<String, String> {
        let resolved_cwd = self.resolve_cwd(cwd);
        let mut params = json!({
            "cwd": resolved_cwd.display().to_string(),
            "approvalPolicy": self.options.approval_policy.clone(),
            "sandbox": self.options.sandbox.clone(),
            "experimentalRawEvents": self.options.experimental_raw_events,
            "persistExtendedHistory": self.options.persist_extended_history,
        });
        if let Some(model) = &settings.model {
            params["model"] = Value::String(model.clone());
        }
        if let Some(reasoning_effort) = &settings.reasoning_effort {
            params["reasoningEffort"] = Value::String(reasoning_effort.clone());
        }
        if let Some(reviewer) = &self.options.approvals_reviewer {
            params["approvalsReviewer"] = Value::String(reviewer.clone());
        }
        let result = self.send_request("thread/start", params).await?;

        let session_id = result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| String::from("codex thread/start response missing thread.id"))?;
        self.session_settings
            .borrow_mut()
            .insert(session_id.clone(), settings);
        Ok(session_id)
    }

    async fn set_session_name(&self, session_id: String, name: String) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }

        self.send_request(
            "thread/name/set",
            json!({
                "threadId": session_id,
                "name": name,
            }),
        )
        .await
        .map(|_| ())
    }

    async fn set_session_settings(
        &self,
        session_id: String,
        settings: AgentSessionSettings,
    ) -> Result<(), String> {
        if self.active_turns.borrow().contains_key(&session_id) {
            return Err("cannot change Codex settings while a turn is running".into());
        }
        self.session_settings
            .borrow_mut()
            .insert(session_id, settings);
        Ok(())
    }

    async fn prompt(&self, session_id: String, text: String) -> Result<AgentPromptResult, String> {
        let thread_id = session_id.clone();
        let mut params = json!({
            "threadId": thread_id,
            "input": [
                {
                    "type": "text",
                    "text": text,
                }
            ],
            "approvalPolicy": self.options.approval_policy.clone(),
        });
        if let Some(settings) = self.session_settings.borrow().get(&thread_id) {
            if let Some(model) = &settings.model {
                params["model"] = Value::String(model.clone());
            }
            if let Some(reasoning_effort) = &settings.reasoning_effort {
                params["reasoningEffort"] = Value::String(reasoning_effort.clone());
            }
        }
        if let Some(reviewer) = &self.options.approvals_reviewer {
            params["approvalsReviewer"] = Value::String(reviewer.clone());
        }
        let result = self.send_request("turn/start", params).await?;

        let turn = result
            .get("turn")
            .ok_or_else(|| "codex turn/start response missing turn".to_string())?;
        let turn_id = turn
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "codex turn/start response missing turn.id".to_string())?
            .to_string();

        if let Some(result) = self.completed_turns.borrow_mut().remove(&turn_id) {
            return result;
        }

        if let Some(result) = prompt_result_from_turn(turn)? {
            return Ok(result);
        }

        let (tx, rx) = oneshot::channel();
        self.turn_waiters.borrow_mut().insert(turn_id.clone(), tx);
        self.active_turns.borrow_mut().insert(session_id, turn_id);

        rx.await
            .map_err(|_| "codex turn waiter dropped before completion".to_string())?
    }

    async fn cancel(&self, session_id: String) -> Result<(), String> {
        let Some(turn_id) = self.active_turns.borrow().get(&session_id).cloned() else {
            return Ok(());
        };

        self.send_request(
            "turn/interrupt",
            json!({
                "threadId": session_id,
                "turnId": turn_id,
            }),
        )
        .await
        .map(|_| ())
    }

    fn take_update_rx(&self) -> Option<mpsc::UnboundedReceiver<AgentNotification>> {
        self.update_rx.borrow_mut().take()
    }

    fn subscribe_health(&self) -> watch::Receiver<bool> {
        self.health_tx.subscribe()
    }

    fn is_alive(&self) -> bool {
        *self.health_tx.borrow()
    }

    async fn shutdown(&self) {
        if !self.is_alive() {
            return;
        }

        if self.kill_tx.send(true).is_err() {
            warn!("codex app-server shutdown signal receiver dropped");
            return;
        }

        let mut health_rx = self.health_tx.subscribe();
        while *health_rx.borrow() {
            if health_rx.changed().await.is_err() {
                break;
            }
        }
    }
}
