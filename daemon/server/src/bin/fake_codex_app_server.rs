use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FakeAgentMode {
    Normal,
    WaitForCancel,
    ExitAfterSession,
    ApprovalRequests,
}

impl FakeAgentMode {
    fn from_env() -> Self {
        match std::env::var("FAKE_CODEX_MODE").as_deref() {
            Ok("wait_for_cancel") => Self::WaitForCancel,
            Ok("exit_after_session") => Self::ExitAfterSession,
            Ok("approval_requests") => Self::ApprovalRequests,
            _ => Self::Normal,
        }
    }
}

struct FakeCodexAppServer {
    mode: FakeAgentMode,
    next_thread_id: u64,
    next_turn_id: u64,
    next_server_request_id: u64,
    session_prefix: String,
    events_path: Option<PathBuf>,
    active_turns: HashMap<String, String>,
    pending_server_requests: HashMap<u64, String>,
}

impl FakeCodexAppServer {
    fn new() -> Self {
        Self {
            mode: FakeAgentMode::from_env(),
            next_thread_id: 1,
            next_turn_id: 1,
            next_server_request_id: 1,
            session_prefix: std::env::var("FAKE_CODEX_SESSION_PREFIX").unwrap_or_default(),
            events_path: std::env::var_os("FAKE_CODEX_EVENTS_PATH").map(PathBuf::from),
            active_turns: HashMap::new(),
            pending_server_requests: HashMap::new(),
        }
    }

    fn record_event(&self, event: &str) {
        let Some(path) = &self.events_path else {
            return;
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{event}");
        }
    }

    fn next_thread_id(&mut self) -> String {
        let id = format!("{}thread-{}", self.session_prefix, self.next_thread_id);
        self.next_thread_id += 1;
        id
    }

    fn next_turn_id(&mut self) -> String {
        let id = format!("turn-{}", self.next_turn_id);
        self.next_turn_id += 1;
        id
    }

    fn next_server_request_id(&mut self, method: &str) -> u64 {
        let id = self.next_server_request_id;
        self.next_server_request_id += 1;
        self.pending_server_requests.insert(id, method.to_string());
        id
    }

    fn record_server_response(&mut self, id: u64, response: &Value) {
        let Some(method) = self.pending_server_requests.remove(&id) else {
            return;
        };
        let response_json = serde_json::to_string(response).unwrap_or_else(|_| "null".into());
        self.record_event(&format!("server_response:{method}:{response_json}"));
    }

    fn thread_value(&self, thread_id: &str, cwd: &str, active: bool) -> Value {
        json!({
            "id": thread_id,
            "preview": "",
            "ephemeral": false,
            "modelProvider": "openai",
            "createdAt": 1,
            "updatedAt": 1,
            "status": if active {
                json!({"type": "active", "activeFlags": []})
            } else {
                json!({"type": "idle"})
            },
            "path": format!("/tmp/{thread_id}.jsonl"),
            "cwd": cwd,
            "cliVersion": "0.115.0",
            "source": "agentchat-test",
            "agentNickname": Value::Null,
            "agentRole": Value::Null,
            "gitInfo": Value::Null,
            "name": Value::Null,
            "turns": [],
        })
    }

    fn turn_value(&self, turn_id: &str, status: &str, error: Option<&str>) -> Value {
        json!({
            "id": turn_id,
            "items": [],
            "status": status,
            "error": error.map(|message| json!({"message": message})),
        })
    }

    fn prompt_text(input: &Value) -> String {
        input
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    item.get("text").and_then(Value::as_str)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn user_message_item(&self, prompt_text: &str) -> Value {
        json!({
            "type": "userMessage",
            "id": "user-1",
            "content": [
                {
                    "type": "text",
                    "text": prompt_text,
                    "text_elements": []
                }
            ]
        })
    }

    fn command_item(&self, tool_id: &str, prompt_text: &str, cwd: &str, status: &str) -> Value {
        json!({
            "type": "commandExecution",
            "id": tool_id,
            "command": "demo tool",
            "cwd": cwd,
            "processId": Value::Null,
            "status": status,
            "commandActions": [],
            "aggregatedOutput": if status == "completed" {
                Value::String(format!("tool completed for: {prompt_text}"))
            } else {
                Value::Null
            },
            "exitCode": if status == "completed" { Value::from(0) } else { Value::Null },
            "durationMs": Value::Null
        })
    }

    fn is_distillation_prompt(prompt_text: &str) -> bool {
        prompt_text
            .contains("You are analyzing a completed coding session to extract reusable knowledge.")
    }
}

async fn write_json(writer: &mut BufWriter<tokio::io::Stdout>, value: &Value) {
    let line = serde_json::to_string(value).unwrap();
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    let mut writer = BufWriter::new(stdout);
    let mut server = FakeCodexAppServer::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = serde_json::from_str(&line).unwrap();
        if request.get("method").is_none() {
            if let Some(id) = request.get("id").and_then(Value::as_u64) {
                server.record_server_response(id, &request);
            }
            continue;
        }

        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                server.record_event("initialize");
                write_json(
                    &mut writer,
                    &json!({
                        "id": id,
                        "result": {
                            "userAgent": "fake-codex-app-server/0.1.0",
                            "platformFamily": "unix",
                            "platformOs": "macos"
                        }
                    }),
                )
                .await;
            }
            "thread/start" => {
                let thread_id = server.next_thread_id();
                let cwd = params
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or(".")
                    .to_string();
                server.record_event(&format!("new_session:{thread_id}:{cwd}"));
                server.record_event(&format!(
                    "thread_start_approval_policy:{}",
                    serde_json::to_string(params.get("approvalPolicy").unwrap_or(&Value::Null))
                        .unwrap()
                ));

                if server.mode == FakeAgentMode::ExitAfterSession {
                    tokio::task::spawn_local(async move {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        std::process::exit(0);
                    });
                }

                let thread = server.thread_value(&thread_id, &cwd, false);
                write_json(
                    &mut writer,
                    &json!({
                        "id": id,
                        "result": {
                            "thread": thread,
                            "model": "gpt-5.4",
                            "modelProvider": "openai",
                            "serviceTier": Value::Null,
                            "cwd": cwd,
                            "approvalPolicy": params.get("approvalPolicy").cloned().unwrap_or_else(|| Value::String("never".into())),
                            "approvalsReviewer": "user",
                            "sandbox": {
                                "type": "workspaceWrite",
                                "writableRoots": [],
                                "readOnlyAccess": {
                                    "type": "fullAccess"
                                },
                                "networkAccess": false,
                                "excludeTmpdirEnvVar": false,
                                "excludeSlashTmp": false
                            },
                            "reasoningEffort": "high"
                        }
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!({
                        "method": "thread/started",
                        "params": {
                            "thread": server.thread_value(&thread_id, &cwd, false)
                        }
                    }),
                )
                .await;
            }
            "turn/start" => {
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let prompt_text = FakeCodexAppServer::prompt_text(
                    params.get("input").unwrap_or(&Value::Array(Vec::new())),
                );
                server.record_event(&format!("prompt:{thread_id}:{prompt_text}"));
                server.record_event(&format!(
                    "turn_start_approval_policy:{}",
                    serde_json::to_string(params.get("approvalPolicy").unwrap_or(&Value::Null))
                        .unwrap()
                ));

                let turn_id = server.next_turn_id();
                server
                    .active_turns
                    .insert(thread_id.clone(), turn_id.clone());
                let cwd = ".";

                write_json(
                    &mut writer,
                    &json!({
                        "id": id,
                        "result": {
                            "turn": server.turn_value(&turn_id, "inProgress", None)
                        }
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!({
                        "method": "thread/status/changed",
                        "params": {
                            "threadId": thread_id,
                            "status": {
                                "type": "active",
                                "activeFlags": []
                            }
                        }
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": thread_id,
                            "turn": server.turn_value(&turn_id, "inProgress", None)
                        }
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!({
                        "method": "item/started",
                        "params": {
                            "item": server.user_message_item(&prompt_text),
                            "threadId": thread_id,
                            "turnId": turn_id
                        }
                    }),
                )
                .await;
                write_json(
                    &mut writer,
                    &json!({
                        "method": "item/completed",
                        "params": {
                            "item": server.user_message_item(&prompt_text),
                            "threadId": thread_id,
                            "turnId": turn_id
                        }
                    }),
                )
                .await;

                if FakeCodexAppServer::is_distillation_prompt(&prompt_text) {
                    write_json(
                        &mut writer,
                        &json!({
                            "method": "item/agentMessage/delta",
                            "params": {
                                "delta": concat!(
                                    "---SKILL: shared/memory-layer---\n",
                                    "# Memory Layer\n",
                                    "- Persist session transcripts under .agentchat/sessions.\n",
                                    "---END SKILL---\n",
                                    "---SKILL: agents/fake/testing-notes---\n",
                                    "# Testing Notes\n",
                                    "- Use the fake Codex app-server in websocket tests.\n",
                                    "---END SKILL---\n"
                                ),
                                "itemId": "assistant-1",
                                "threadId": thread_id,
                                "turnId": turn_id
                            }
                        }),
                    )
                    .await;
                    write_json(
                        &mut writer,
                        &json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": thread_id,
                                "turn": server.turn_value(&turn_id, "completed", None)
                            }
                        }),
                    )
                    .await;
                    write_json(
                        &mut writer,
                        &json!({
                            "method": "thread/status/changed",
                            "params": {
                                "threadId": thread_id,
                                "status": { "type": "idle" }
                            }
                        }),
                    )
                    .await;
                    server.active_turns.remove(&thread_id);
                    continue;
                }

                match server.mode {
                    FakeAgentMode::Normal | FakeAgentMode::ExitAfterSession => {
                        let tool_id = "tool-1";
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/reasoning/textDelta",
                                "params": {
                                    "contentIndex": 0,
                                    "delta": "thinking about the request",
                                    "itemId": "reasoning-1",
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/started",
                                "params": {
                                    "item": server.command_item(tool_id, &prompt_text, cwd, "inProgress"),
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/agentMessage/delta",
                                "params": {
                                    "delta": format!("echo: {prompt_text}"),
                                    "itemId": "assistant-1",
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/completed",
                                "params": {
                                    "item": server.command_item(tool_id, &prompt_text, cwd, "completed"),
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "turn/completed",
                                "params": {
                                    "threadId": thread_id,
                                    "turn": server.turn_value(&turn_id, "completed", None)
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "thread/status/changed",
                                "params": {
                                    "threadId": thread_id,
                                    "status": { "type": "idle" }
                                }
                            }),
                        )
                        .await;
                        server.active_turns.remove(&thread_id);
                    }
                    FakeAgentMode::ApprovalRequests => {
                        let command_request_id =
                            server.next_server_request_id("item/commandExecution/requestApproval");
                        write_json(
                            &mut writer,
                            &json!({
                                "id": command_request_id,
                                "method": "item/commandExecution/requestApproval",
                                "params": {
                                    "threadId": thread_id,
                                    "turnId": turn_id,
                                    "itemId": "cmd-approval-1",
                                    "command": "npm install",
                                    "cwd": cwd,
                                    "reason": "network access requested"
                                }
                            }),
                        )
                        .await;

                        let file_request_id =
                            server.next_server_request_id("item/fileChange/requestApproval");
                        write_json(
                            &mut writer,
                            &json!({
                                "id": file_request_id,
                                "method": "item/fileChange/requestApproval",
                                "params": {
                                    "threadId": thread_id,
                                    "turnId": turn_id,
                                    "itemId": "file-approval-1",
                                    "grantRoot": cwd,
                                    "reason": "needs to update project files"
                                }
                            }),
                        )
                        .await;

                        let permissions_request_id =
                            server.next_server_request_id("item/permissions/requestApproval");
                        write_json(
                            &mut writer,
                            &json!({
                                "id": permissions_request_id,
                                "method": "item/permissions/requestApproval",
                                "params": {
                                    "threadId": thread_id,
                                    "turnId": turn_id,
                                    "itemId": "perm-approval-1",
                                    "permissions": {
                                        "network": {
                                            "enabled": true
                                        }
                                    },
                                    "reason": "download dependency metadata"
                                }
                            }),
                        )
                        .await;

                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/agentMessage/delta",
                                "params": {
                                    "delta": "approval flow complete",
                                    "itemId": "assistant-1",
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "turn/completed",
                                "params": {
                                    "threadId": thread_id,
                                    "turn": server.turn_value(&turn_id, "completed", None)
                                }
                            }),
                        )
                        .await;
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "thread/status/changed",
                                "params": {
                                    "threadId": thread_id,
                                    "status": { "type": "idle" }
                                }
                            }),
                        )
                        .await;
                        server.active_turns.remove(&thread_id);
                    }
                    FakeAgentMode::WaitForCancel => {
                        write_json(
                            &mut writer,
                            &json!({
                                "method": "item/agentMessage/delta",
                                "params": {
                                    "delta": "waiting for cancel",
                                    "itemId": "assistant-1",
                                    "threadId": thread_id,
                                    "turnId": turn_id
                                }
                            }),
                        )
                        .await;
                    }
                }
            }
            "turn/interrupt" => {
                let thread_id = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let turn_id = params
                    .get("turnId")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                server.record_event(&format!("cancel:{thread_id}:{turn_id}"));

                write_json(
                    &mut writer,
                    &json!({
                        "id": id,
                        "result": {}
                    }),
                )
                .await;

                if server.active_turns.get(&thread_id).map(String::as_str) == Some(turn_id.as_str())
                {
                    write_json(
                        &mut writer,
                        &json!({
                            "method": "turn/completed",
                            "params": {
                                "threadId": thread_id,
                                "turn": server.turn_value(&turn_id, "interrupted", None)
                            }
                        }),
                    )
                    .await;
                    write_json(
                        &mut writer,
                        &json!({
                            "method": "thread/status/changed",
                            "params": {
                                "threadId": thread_id,
                                "status": { "type": "idle" }
                            }
                        }),
                    )
                    .await;
                    server.active_turns.remove(&thread_id);
                }
            }
            _ => {
                write_json(
                    &mut writer,
                    &json!({
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("unsupported method `{method}`")
                        }
                    }),
                )
                .await;
            }
        }
    }
}
