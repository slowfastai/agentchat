//! ACP capability provider -- handles agent requests for file system and terminal access.
//!
//! The daemon acts as a "virtual editor" that fulfills ACP capability requests locally.
//! For M0, all permission requests are auto-approved (first option selected).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use agent_client_protocol::*;
use tokio::sync::mpsc;
use tracing::{debug, warn};

const MAX_TERMINAL_OUTPUT: usize = 1_048_576;
const TRUNCATED_OUTPUT_MARKER: &str = "[output truncated]\n";

/// Implements the ACP `Client` trait, handling agent -> client requests.
pub struct DaemonClient {
    project_root: PathBuf,
    update_tx: mpsc::UnboundedSender<SessionNotification>,
    terminals: std::cell::RefCell<HashMap<String, TerminalState>>,
    next_terminal_id: AtomicU32,
}

struct TerminalState {
    output: String,
    exit_code: Option<i32>,
    truncated: bool,
}

fn truncate_terminal_output(output: String) -> (String, bool) {
    if output.len() <= MAX_TERMINAL_OUTPUT {
        return (output, false);
    }

    let mut start = output.len().saturating_sub(MAX_TERMINAL_OUTPUT);
    while start < output.len() && !output.is_char_boundary(start) {
        start += 1;
    }

    (
        format!("{TRUNCATED_OUTPUT_MARKER}{}", &output[start..]),
        true,
    )
}

impl DaemonClient {
    pub fn new(
        project_root: PathBuf,
        update_tx: mpsc::UnboundedSender<SessionNotification>,
    ) -> Self {
        Self {
            project_root,
            update_tx,
            terminals: std::cell::RefCell::new(HashMap::new()),
            next_terminal_id: AtomicU32::new(1),
        }
    }

    fn validate_path(&self, path: &Path) -> std::result::Result<PathBuf, Error> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };

        let check_path = if resolved.exists() {
            resolved
                .canonicalize()
                .map_err(|e| Error::invalid_request().data(format!("cannot resolve path: {e}")))?
        } else {
            let parent = resolved
                .parent()
                .ok_or_else(|| Error::invalid_request().data("path has no parent"))?;
            let canonical_parent = parent.canonicalize().map_err(|e| {
                Error::invalid_request().data(format!("cannot resolve parent: {e}"))
            })?;
            canonical_parent.join(resolved.file_name().unwrap_or_default())
        };

        let canonical_root = self
            .project_root
            .canonicalize()
            .map_err(|e| Error::internal_error().data(format!("project root error: {e}")))?;

        if !check_path.starts_with(&canonical_root) {
            return Err(
                Error::invalid_request().data(format!("outside project root: {}", path.display()))
            );
        }

        Ok(check_path)
    }
}

#[async_trait::async_trait(?Send)]
impl Client for DaemonClient {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse> {
        // M0: auto-approve by selecting the first option.
        debug!("auto-approving permission request (M0)");
        let option_id = args
            .options
            .first()
            .map(|o| o.option_id.clone())
            .unwrap_or_else(|| PermissionOptionId::new("allow"));

        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))
    }

    async fn session_notification(&self, args: SessionNotification) -> Result<()> {
        if self.update_tx.send(args).is_err() {
            warn!("session update receiver dropped");
        }
        Ok(())
    }

    async fn read_text_file(&self, args: ReadTextFileRequest) -> Result<ReadTextFileResponse> {
        let path = self.validate_path(&args.path)?;
        debug!("reading file: {}", path.display());

        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| Error::internal_error().data(format!("read failed: {e}")))?;

        Ok(ReadTextFileResponse::new(content))
    }

    async fn write_text_file(&self, args: WriteTextFileRequest) -> Result<WriteTextFileResponse> {
        let path = self.validate_path(&args.path)?;
        debug!("writing file: {}", path.display());

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::internal_error().data(format!("mkdir failed: {e}")))?;
        }

        tokio::fs::write(&path, &args.content)
            .await
            .map_err(|e| Error::internal_error().data(format!("write failed: {e}")))?;

        Ok(WriteTextFileResponse::default())
    }

    async fn create_terminal(&self, args: CreateTerminalRequest) -> Result<CreateTerminalResponse> {
        let id_num = self.next_terminal_id.fetch_add(1, Ordering::Relaxed);
        let terminal_id = TerminalId::new(format!("term-{id_num}"));

        let cwd = args
            .cwd
            .as_ref()
            .map(|p| self.validate_path(p))
            .transpose()?
            .unwrap_or_else(|| self.project_root.clone());

        debug!("terminal {}: {} {:?}", terminal_id, args.command, args.args);

        let output = tokio::process::Command::new(&args.command)
            .args(&args.args)
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .await
            .map_err(|e| Error::internal_error().data(format!("spawn failed: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let (combined, truncated) = truncate_terminal_output(format!("{stdout}{stderr}"));
        let exit_code = output.status.code();

        self.terminals.borrow_mut().insert(
            terminal_id.to_string(),
            TerminalState {
                output: combined,
                exit_code,
                truncated,
            },
        );

        Ok(CreateTerminalResponse::new(terminal_id))
    }

    async fn terminal_output(&self, args: TerminalOutputRequest) -> Result<TerminalOutputResponse> {
        let tid = args.terminal_id.to_string();
        let terminals = self.terminals.borrow();
        let state = terminals
            .get(&tid)
            .ok_or_else(|| Error::invalid_request().data("terminal not found"))?;

        let mut resp = TerminalOutputResponse::new(&state.output, state.truncated);
        if let Some(code) = state.exit_code {
            resp = resp.exit_status(TerminalExitStatus::new().exit_code(code as u32));
        }
        Ok(resp)
    }

    async fn release_terminal(
        &self,
        args: ReleaseTerminalRequest,
    ) -> Result<ReleaseTerminalResponse> {
        self.terminals
            .borrow_mut()
            .remove(&args.terminal_id.to_string());
        Ok(ReleaseTerminalResponse::default())
    }

    async fn wait_for_terminal_exit(
        &self,
        args: WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse> {
        let tid = args.terminal_id.to_string();
        let terminals = self.terminals.borrow();
        let state = terminals
            .get(&tid)
            .ok_or_else(|| Error::invalid_request().data("terminal not found"))?;

        let exit_status = match state.exit_code {
            Some(code) => TerminalExitStatus::new().exit_code(code as u32),
            None => TerminalExitStatus::new(),
        };
        Ok(WaitForTerminalExitResponse::new(exit_status))
    }

    /// M0 only stores completed terminal results. M1 will keep streaming terminal
    /// handles so this can interrupt live processes.
    async fn kill_terminal(&self, args: KillTerminalRequest) -> Result<KillTerminalResponse> {
        let tid = args.terminal_id.to_string();
        if self.terminals.borrow().contains_key(&tid) {
            warn!("terminal {tid} already completed; kill_terminal is a no-op in M0");
            return Ok(KillTerminalResponse::default());
        }

        Err(Error::invalid_request().data("terminal not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn daemon_client(project_root: &Path) -> DaemonClient {
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        DaemonClient::new(project_root.to_path_buf(), update_tx)
    }

    #[test]
    fn validate_path_accepts_relative_path_within_root() {
        let root = tempdir().unwrap();
        let file_path = root.path().join("nested/file.txt");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "ok").unwrap();

        let client = daemon_client(root.path());
        let resolved = client.validate_path(Path::new("nested/file.txt")).unwrap();

        assert_eq!(resolved, file_path.canonicalize().unwrap());
    }

    #[test]
    fn validate_path_accepts_absolute_path_within_root() {
        let root = tempdir().unwrap();
        let file_path = root.path().join("absolute.txt");
        std::fs::write(&file_path, "ok").unwrap();

        let client = daemon_client(root.path());
        let resolved = client.validate_path(&file_path).unwrap();

        assert_eq!(resolved, file_path.canonicalize().unwrap());
    }

    #[test]
    fn validate_path_rejects_parent_escape() {
        let root = tempdir().unwrap();
        let client = daemon_client(root.path());

        assert!(client.validate_path(Path::new("../escape.txt")).is_err());
    }

    #[test]
    fn validate_path_rejects_absolute_path_outside_root() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&outside_file, "nope").unwrap();

        let client = daemon_client(root.path());

        assert!(client.validate_path(&outside_file).is_err());
    }

    #[test]
    fn validate_path_accepts_nonexistent_file_with_valid_parent() {
        let root = tempdir().unwrap();
        let dir = root.path().join("nested");
        std::fs::create_dir_all(&dir).unwrap();

        let client = daemon_client(root.path());
        let resolved = client
            .validate_path(Path::new("nested/missing.txt"))
            .unwrap();

        assert_eq!(resolved, dir.canonicalize().unwrap().join("missing.txt"));
    }

    #[test]
    fn validate_path_rejects_symlink_escape() {
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let outside_file = outside.path().join("secret.txt");
        let symlink_path = root.path().join("escape-link.txt");

        std::fs::write(&outside_file, "secret").unwrap();
        create_symlink(&outside_file, &symlink_path).unwrap();

        let client = daemon_client(root.path());

        assert!(client.validate_path(Path::new("escape-link.txt")).is_err());
    }

    #[test]
    fn truncate_terminal_output_marks_truncated_output() {
        let (output, truncated) = truncate_terminal_output("a".repeat(MAX_TERMINAL_OUTPUT + 1));

        assert!(truncated);
        assert!(output.starts_with(TRUNCATED_OUTPUT_MARKER));
        assert!(output.len() <= TRUNCATED_OUTPUT_MARKER.len() + MAX_TERMINAL_OUTPUT);
    }

    #[cfg(unix)]
    fn create_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(src, dst)
    }

    #[cfg(windows)]
    fn create_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_file(src, dst)
    }
}
