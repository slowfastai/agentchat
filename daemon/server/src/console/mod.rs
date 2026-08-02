//! The local run console: routes plus the single page that drives them.
//!
//! Everything the console needs already exists behind traits — [`RunSupervisor`]
//! mirrors run state, [`SupervisorGate`] parks a phase until someone answers,
//! [`SupervisorProgress`] collects the activity log. This module is a thin shell
//! that exposes them over HTTP and hands the browser one HTML file.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[cfg(target_os = "macos")]
use std::process::Command;

use agentchat_core::run::supervisor::{RunSupervisor, SharedSupervisor};
use agentchat_protocol::run::ApprovalDecision;
use serde::Deserialize;

use crate::http::{Handler, Request, Response};

mod chat;
mod page;

pub use chat::PAGE as CHAT_PAGE;
pub use page::PAGE;

/// What the console needs in order to start a run.
///
/// Starting is delegated because spawning agents belongs to the binary that
/// owns the agent configuration, not to the HTTP layer.
pub trait RunLauncher {
    /// Agent ids that started successfully, in configuration order.
    fn agent_ids(&self) -> Vec<String>;

    /// The directory runs operate on.
    fn working_dir(&self) -> PathBuf;

    /// Begins a run in the background, returning its id.
    fn start(&self, request: StartRequest) -> Result<String, String>;
}

/// A run the console asked for.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct StartRequest {
    /// The requirement, in the user's own words.
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub planner: Option<String>,
    #[serde(default)]
    pub plan_reviewers: Vec<String>,
    #[serde(default)]
    pub implementer: Option<String>,
    #[serde(default)]
    pub code_reviewers: Vec<String>,
    /// Stop once the plan is approved, leaving the working tree untouched.
    #[serde(default)]
    pub plan_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct DecisionRequest {
    decision: String,
    #[serde(default)]
    comments: String,
}

/// Builds the console's request handler.
pub fn handler(supervisor: SharedSupervisor, launcher: Rc<dyn RunLauncher>) -> Handler {
    Rc::new(move |request: Request| route(&supervisor, launcher.as_ref(), request))
}

fn route(supervisor: &SharedSupervisor, launcher: &dyn RunLauncher, request: Request) -> Response {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => Response::html(PAGE),
        ("GET", "/chat") => Response::html(CHAT_PAGE),
        ("GET", "/favicon.ico") => Response::html(Vec::<u8>::new()),

        ("POST", "/api/select-working-directory") => match select_working_directory() {
            Ok(Some(path)) => Response::json(serde_json::json!({ "path": path }).to_string()),
            Ok(None) => Response::json(serde_json::json!({ "cancelled": true }).to_string()),
            Err(message) => Response::error(500, &message),
        },

        ("GET", "/api/config") => Response::json(
            serde_json::json!({
                "agents": launcher.agent_ids(),
                "working_dir": launcher.working_dir().display().to_string(),
            })
            .to_string(),
        ),

        ("GET", "/api/runs") => {
            let runs = supervisor.borrow().list();
            json_or_500(&serde_json::json!({ "runs": runs }))
        }

        ("POST", "/api/runs") => match serde_json::from_str::<StartRequest>(&request.body) {
            Ok(start) if start.brief.trim().is_empty() => {
                Response::error(400, "the brief is empty — describe what you want done")
            }
            Ok(start) => match launcher.start(start) {
                Ok(run_id) => Response::json(serde_json::json!({ "run_id": run_id }).to_string()),
                Err(message) => Response::error(400, &message),
            },
            Err(e) => Response::error(400, &format!("malformed request: {e}")),
        },

        ("GET", path) if path.starts_with("/api/runs/") => {
            let Some(run_id) = request.segment_after("/api/runs/") else {
                return Response::error(404, "no run id in the path");
            };

            if path.ends_with("/log") {
                let after = request
                    .query_param("after")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let supervisor = supervisor.borrow();
                let entries = supervisor.log_after(run_id, after);
                let view = supervisor.view(run_id);
                return json_or_500(&serde_json::json!({ "entries": entries, "run": view }));
            }

            match supervisor.borrow().view(run_id) {
                Some(view) => json_or_500(view),
                None => Response::error(404, "no such run"),
            }
        }

        ("POST", path) if path.starts_with("/api/runs/") && path.ends_with("/decision") => {
            let Some(run_id) = request.segment_after("/api/runs/") else {
                return Response::error(404, "no run id in the path");
            };
            let body: DecisionRequest = match serde_json::from_str(&request.body) {
                Ok(body) => body,
                Err(e) => return Response::error(400, &format!("malformed decision: {e}")),
            };
            let decision = match body.decision.as_str() {
                "approve" => ApprovalDecision::Approve,
                "request_changes" => ApprovalDecision::RequestChanges {
                    comments: body.comments,
                },
                "cancel" => ApprovalDecision::Cancel,
                other => return Response::error(400, &format!("unknown decision `{other}`")),
            };

            match supervisor.borrow_mut().decide(run_id, decision) {
                Ok(()) => Response::json(r#"{"ok":true}"#),
                Err(message) => Response::error(409, &message),
            }
        }

        ("GET", _) | ("POST", _) => Response::error(404, "no such endpoint"),
        _ => Response::error(405, "method not allowed"),
    }
}

fn select_working_directory() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("osascript")
            .args([
                "-e",
                r#"
try
    set selectedFolder to choose folder with prompt "Choose a working directory"
    return POSIX path of selectedFolder
on error number -128
    return "__AGENTCHAT_CANCELLED__"
end try
"#,
            ])
            .output()
            .map_err(|error| format!("could not open the macOS folder picker: {error}"))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if error.contains("-128") || error.to_ascii_lowercase().contains("user canceled") {
                return Ok(None);
            }
            return Err(format!("macOS folder picker failed: {error}"));
        }

        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path == "__AGENTCHAT_CANCELLED__" || path.is_empty() {
            Ok(None)
        } else {
            Ok(Some(path))
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err("the native working directory picker is only available on macOS".into())
    }
}

fn json_or_500<T: serde::Serialize>(value: &T) -> Response {
    match serde_json::to_string(value) {
        Ok(body) => Response::json(body),
        Err(e) => Response::error(500, &format!("cannot serialize response: {e}")),
    }
}

/// A supervisor shared between the console and the tasks driving runs.
pub fn shared_supervisor() -> SharedSupervisor {
    Rc::new(RefCell::new(RunSupervisor::new()))
}

#[cfg(test)]
mod tests {
    use agentchat_core::run::state::RunState;
    use agentchat_protocol::run::RunStatus;

    use super::*;

    struct StubLauncher {
        agents: Vec<String>,
        started: RefCell<Vec<StartRequest>>,
        fail_with: Option<String>,
    }

    impl StubLauncher {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                agents: vec!["codex".into(), "claude-code".into()],
                started: RefCell::new(Vec::new()),
                fail_with: None,
            })
        }

        fn failing(message: &str) -> Rc<Self> {
            Rc::new(Self {
                agents: vec!["codex".into()],
                started: RefCell::new(Vec::new()),
                fail_with: Some(message.into()),
            })
        }
    }

    impl RunLauncher for StubLauncher {
        fn agent_ids(&self) -> Vec<String> {
            self.agents.clone()
        }

        fn working_dir(&self) -> PathBuf {
            PathBuf::from("/work/tree")
        }

        fn start(&self, request: StartRequest) -> Result<String, String> {
            if let Some(message) = &self.fail_with {
                return Err(message.clone());
            }
            self.started.borrow_mut().push(request);
            Ok("run-1".into())
        }
    }

    fn get(path: &str, query: &str) -> Request {
        Request {
            method: "GET".into(),
            path: path.into(),
            query: query.into(),
            body: String::new(),
        }
    }

    fn post(path: &str, body: &str) -> Request {
        Request {
            method: "POST".into(),
            path: path.into(),
            query: String::new(),
            body: body.into(),
        }
    }

    fn body_of(response: &Response) -> serde_json::Value {
        serde_json::from_slice(&response.body).expect("response is json")
    }

    #[test]
    fn the_root_path_serves_the_page() {
        let supervisor = shared_supervisor();
        let response = route(&supervisor, StubLauncher::new().as_ref(), get("/", ""));

        assert_eq!(response.status, 200);
        assert!(response.content_type.starts_with("text/html"));
        assert!(!response.body.is_empty());
    }

    #[test]
    fn config_reports_the_agents_and_directory() {
        let supervisor = shared_supervisor();
        let response = route(
            &supervisor,
            StubLauncher::new().as_ref(),
            get("/api/config", ""),
        );

        let body = body_of(&response);
        assert_eq!(body["agents"][0], "codex");
        assert_eq!(body["working_dir"], "/work/tree");
    }

    #[test]
    fn starting_a_run_passes_the_request_through() {
        let supervisor = shared_supervisor();
        let launcher = StubLauncher::new();

        let response = route(
            &supervisor,
            launcher.as_ref(),
            post(
                "/api/runs",
                r#"{"brief":"add a health endpoint","plan_only":true,"plan_reviewers":["claude-code"]}"#,
            ),
        );

        assert_eq!(response.status, 200);
        assert_eq!(body_of(&response)["run_id"], "run-1");
        let started = launcher.started.borrow();
        assert_eq!(started[0].brief, "add a health endpoint");
        assert!(started[0].plan_only);
        assert_eq!(started[0].plan_reviewers, vec!["claude-code".to_string()]);
    }

    #[test]
    fn an_empty_brief_is_refused_before_anything_starts() {
        let supervisor = shared_supervisor();
        let launcher = StubLauncher::new();

        let response = route(
            &supervisor,
            launcher.as_ref(),
            post("/api/runs", r#"{"brief":"   "}"#),
        );

        assert_eq!(response.status, 400);
        assert!(launcher.started.borrow().is_empty());
    }

    #[test]
    fn a_launcher_failure_reaches_the_console() {
        let supervisor = shared_supervisor();

        let response = route(
            &supervisor,
            StubLauncher::failing("only one agent started").as_ref(),
            post("/api/runs", r#"{"brief":"do the thing"}"#),
        );

        assert_eq!(response.status, 400);
        assert_eq!(body_of(&response)["error"], "only one agent started");
    }

    #[test]
    fn the_log_endpoint_returns_only_what_the_client_has_not_seen() {
        let supervisor = shared_supervisor();
        supervisor.borrow_mut().sync(&RunState::new("run-1", "/w"));
        for line in ["first", "second", "third"] {
            supervisor.borrow_mut().append_log("run-1", line.into());
        }

        let response = route(
            &supervisor,
            StubLauncher::new().as_ref(),
            get("/api/runs/run-1/log", "after=2"),
        );

        let body = body_of(&response);
        assert_eq!(body["entries"].as_array().unwrap().len(), 1);
        assert_eq!(body["entries"][0]["line"], "third");
        // The run's state rides along so the page needs one poll, not two.
        assert_eq!(body["run"]["status"], "planning");
    }

    #[test]
    fn an_unknown_run_is_a_404() {
        let supervisor = shared_supervisor();

        let response = route(
            &supervisor,
            StubLauncher::new().as_ref(),
            get("/api/runs/nope", ""),
        );

        assert_eq!(response.status, 404);
    }

    #[test]
    fn a_decision_reaches_a_parked_run() {
        let supervisor = shared_supervisor();
        let mut run = RunState::new("run-1", "/w");
        run.status = RunStatus::AwaitingPlanApproval;
        supervisor.borrow_mut().sync(&run);

        // Nothing is parked yet, so the decision has nowhere to go.
        let response = route(
            &supervisor,
            StubLauncher::new().as_ref(),
            post("/api/runs/run-1/decision", r#"{"decision":"approve"}"#),
        );

        assert_eq!(response.status, 409);
        assert!(body_of(&response)["error"]
            .as_str()
            .unwrap()
            .contains("not waiting"));
    }

    #[test]
    fn an_unknown_decision_is_rejected() {
        let supervisor = shared_supervisor();

        let response = route(
            &supervisor,
            StubLauncher::new().as_ref(),
            post("/api/runs/run-1/decision", r#"{"decision":"maybe"}"#),
        );

        assert_eq!(response.status, 400);
        assert!(body_of(&response)["error"]
            .as_str()
            .unwrap()
            .contains("maybe"));
    }

    #[test]
    fn request_changes_carries_comments() {
        let body: DecisionRequest =
            serde_json::from_str(r#"{"decision":"request_changes","comments":"reuse the store"}"#)
                .unwrap();

        assert_eq!(body.decision, "request_changes");
        assert_eq!(body.comments, "reuse the store");
    }

    #[test]
    fn unknown_endpoints_and_methods_are_distinguished() {
        let supervisor = shared_supervisor();
        let launcher = StubLauncher::new();

        assert_eq!(
            route(&supervisor, launcher.as_ref(), get("/nope", "")).status,
            404
        );
        assert_eq!(
            route(
                &supervisor,
                launcher.as_ref(),
                Request {
                    method: "DELETE".into(),
                    path: "/api/runs".into(),
                    query: String::new(),
                    body: String::new(),
                }
            )
            .status,
            405
        );
    }
}
