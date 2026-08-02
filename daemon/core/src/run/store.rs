//! Persistence and restart recovery for runs.
//!
//! A run that cannot survive the daemon restarting is a run you have to watch,
//! which defeats the point. Each run owns a directory holding its brief, its
//! artifacts, its reviews, and a `run.json` snapshot of the state machine.
//! [`RunStore::scan_resumable`] finds the runs that still have work left after
//! a restart.
//!
//! Follows the flush/load shape of [`SessionStore`](crate::session_store).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use agentchat_protocol::now_millis;
use tracing::warn;

use crate::run::state::RunState;

/// Snapshot file inside a run directory.
pub const RUN_SNAPSHOT_FILE: &str = "run.json";

/// Holds live runs and mirrors them to disk.
pub struct RunStore {
    runs: HashMap<String, RunState>,
    runs_dir: PathBuf,
}

impl RunStore {
    pub fn new(project_root: &Path) -> Self {
        Self::new_with_runs_dir(project_root.join(".agentchat").join("runs"))
    }

    pub fn new_with_runs_dir(runs_dir: impl Into<PathBuf>) -> Self {
        Self {
            runs: HashMap::new(),
            runs_dir: runs_dir.into(),
        }
    }

    pub fn runs_dir(&self) -> &Path {
        &self.runs_dir
    }

    /// Directory holding one run's brief, plan, reviews, and snapshot.
    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir.join(run_id)
    }

    pub fn snapshot_path(&self, run_id: &str) -> PathBuf {
        self.run_dir(run_id).join(RUN_SNAPSHOT_FILE)
    }

    pub fn insert(&mut self, run: RunState) {
        self.runs.insert(run.run_id.clone(), run);
    }

    pub fn get(&self, run_id: &str) -> Option<&RunState> {
        self.runs.get(run_id)
    }

    pub fn get_mut(&mut self, run_id: &str) -> Option<&mut RunState> {
        self.runs.get_mut(run_id)
    }

    pub fn list(&self) -> Vec<RunState> {
        self.runs.values().cloned().collect()
    }

    /// Runs that are blocked on the human rather than on an agent.
    pub fn awaiting_human(&self) -> Vec<RunState> {
        self.runs
            .values()
            .filter(|run| run.status.awaits_human())
            .cloned()
            .collect()
    }

    pub fn remove(&mut self, run_id: &str) -> Option<RunState> {
        self.runs.remove(run_id)
    }

    /// Writes the snapshot, creating the run directory if needed.
    pub fn flush_run(
        &mut self,
        run_id: &str,
    ) -> impl Future<Output = Result<PathBuf, String>> + 'static {
        if let Some(run) = self.runs.get_mut(run_id) {
            run.updated_at_ms = now_millis();
        }

        let run_id = run_id.to_string();
        let run = self.runs.get(&run_id).cloned();
        let path = self.snapshot_path(&run_id);

        async move {
            let run = run.ok_or_else(|| format!("run not found: {run_id}"))?;
            let json = serde_json::to_string_pretty(&run)
                .map_err(|e| format!("failed to serialize run {run_id}: {e}"))?;

            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("failed to create run dir for {run_id}: {e}"))?;
            }

            tokio::fs::write(&path, json)
                .await
                .map_err(|e| format!("failed to write run {run_id}: {e}"))?;

            Ok(path)
        }
    }

    pub fn load_run(
        &self,
        run_id: &str,
    ) -> impl Future<Output = Result<RunState, String>> + 'static {
        let path = self.snapshot_path(run_id);
        let run_id = run_id.to_string();

        async move {
            let json = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("failed to read run {run_id}: {e}"))?;

            serde_json::from_str(&json).map_err(|e| format!("failed to parse run {run_id}: {e}"))
        }
    }

    /// Finds runs on disk that still have work left.
    ///
    /// Snapshots that fail to parse are logged and skipped rather than failing
    /// the scan: one corrupt run must not stop the daemon from resuming the
    /// others.
    pub fn scan_resumable(&self) -> impl Future<Output = Result<Vec<RunState>, String>> + 'static {
        let runs_dir = self.runs_dir.clone();

        async move {
            if !runs_dir.exists() {
                return Ok(Vec::new());
            }

            let mut entries = tokio::fs::read_dir(&runs_dir)
                .await
                .map_err(|e| format!("failed to read runs dir {}: {e}", runs_dir.display()))?;

            let mut resumable = Vec::new();
            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| format!("failed to read runs dir entry: {e}"))?
            {
                let snapshot = entry.path().join(RUN_SNAPSHOT_FILE);
                if !snapshot.is_file() {
                    continue;
                }

                let json = match tokio::fs::read_to_string(&snapshot).await {
                    Ok(json) => json,
                    Err(e) => {
                        warn!(
                            "skipping unreadable run snapshot {}: {e}",
                            snapshot.display()
                        );
                        continue;
                    }
                };

                match serde_json::from_str::<RunState>(&json) {
                    Ok(run) if run.status.is_terminal() => {}
                    Ok(run) => resumable.push(run),
                    Err(e) => {
                        warn!(
                            "skipping unparseable run snapshot {}: {e}",
                            snapshot.display()
                        )
                    }
                }
            }

            resumable.sort_by_key(|run| run.created_at_ms);
            Ok(resumable)
        }
    }

    /// Loads resumable runs from disk into memory.
    pub async fn resume(&mut self) -> Result<Vec<String>, String> {
        let runs = self.scan_resumable().await?;
        let ids = runs.iter().map(|run| run.run_id.clone()).collect();
        for run in runs {
            self.runs.insert(run.run_id.clone(), run);
        }
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{PhaseKind, RunStatus, StageKind};
    use tempfile::tempdir;

    use super::*;

    fn drafted_run(run_id: &str) -> RunState {
        let mut run = RunState::new(run_id, "/tmp/worktree");
        run.plan.record_draft().unwrap();
        run
    }

    #[test]
    fn snapshot_lives_inside_a_per_run_directory() {
        let root = tempdir().unwrap();
        let store = RunStore::new(root.path());

        assert_eq!(
            store.snapshot_path("run-1"),
            root.path()
                .join(".agentchat")
                .join("runs")
                .join("run-1")
                .join("run.json")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flush_then_load_round_trips() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        store.insert(drafted_run("run-1"));

        let path = store.flush_run("run-1").await.unwrap();
        let loaded = store.load_run("run-1").await.unwrap();

        assert!(path.exists());
        assert_eq!(loaded.run_id, "run-1");
        assert_eq!(loaded.plan.stage, StageKind::Reviewing);
        assert_eq!(loaded.plan.kind, PhaseKind::Plan);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flush_refreshes_the_update_timestamp() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        let mut run = drafted_run("run-1");
        run.updated_at_ms = 0;
        store.insert(run);

        store.flush_run("run-1").await.unwrap();

        assert!(store.get("run-1").unwrap().updated_at_ms > 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn flushing_an_unknown_run_reports_the_id() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());

        let error = store.flush_run("run-missing").await.unwrap_err();

        assert!(error.contains("run-missing"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scanning_an_absent_directory_is_not_an_error() {
        let root = tempdir().unwrap();
        let store = RunStore::new(root.path());

        assert!(store.scan_resumable().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scan_skips_terminal_runs() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        store.insert(drafted_run("run-live"));
        let mut done = drafted_run("run-done");
        done.cancel();
        store.insert(done);
        store.flush_run("run-live").await.unwrap();
        store.flush_run("run-done").await.unwrap();

        let resumable = store.scan_resumable().await.unwrap();

        assert_eq!(resumable.len(), 1);
        assert_eq!(resumable[0].run_id, "run-live");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scan_skips_corrupt_snapshots_without_failing() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        store.insert(drafted_run("run-good"));
        store.flush_run("run-good").await.unwrap();

        let corrupt = store.run_dir("run-corrupt");
        tokio::fs::create_dir_all(&corrupt).await.unwrap();
        tokio::fs::write(corrupt.join(RUN_SNAPSHOT_FILE), "{ not json")
            .await
            .unwrap();

        let resumable = store.scan_resumable().await.unwrap();

        assert_eq!(resumable.len(), 1);
        assert_eq!(resumable[0].run_id, "run-good");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scan_ignores_directories_without_a_snapshot() {
        let root = tempdir().unwrap();
        let store = RunStore::new(root.path());
        tokio::fs::create_dir_all(store.run_dir("run-empty"))
            .await
            .unwrap();

        assert!(store.scan_resumable().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resume_reloads_state_into_a_fresh_store() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        let mut run = drafted_run("run-1");
        run.plan
            .record_review_round(Vec::new())
            .expect("review round");
        store.insert(run);
        store.flush_run("run-1").await.unwrap();

        // A restart: nothing in memory, everything on disk.
        let mut restarted = RunStore::new(root.path());
        let ids = restarted.resume().await.unwrap();

        assert_eq!(ids, vec!["run-1".to_string()]);
        let run = restarted.get("run-1").expect("run resumed into memory");
        assert_eq!(
            run.plan.stage,
            StageKind::Dispositioning,
            "resumes at the recorded stage, not from the start"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn awaiting_human_selects_runs_blocked_on_approval() {
        let root = tempdir().unwrap();
        let mut store = RunStore::new(root.path());
        store.insert(drafted_run("run-working"));

        let mut waiting = drafted_run("run-waiting");
        waiting.plan.record_review_round(Vec::new()).unwrap();
        waiting.plan.record_disposition(Vec::new()).unwrap();
        waiting.sync_status();
        store.insert(waiting);

        let waiting = store.awaiting_human();

        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].run_id, "run-waiting");
        assert_eq!(waiting[0].status, RunStatus::AwaitingPlanApproval);
    }
}
