//! Where a run's files live.
//!
//! Agents exchange work through files on disk rather than through the
//! orchestrator's memory: the orchestrator passes paths and prompts, never
//! content. That keeps reviewers as real CLI agents inside a real repository —
//! able to grep, read neighbouring files, and run tests — which is what makes
//! their review worth more than a summary pasted into a prompt.
//!
//! The working directory is whatever the daemon was pointed at. Preparing an
//! isolated worktree is the operator's job, not the daemon's.

use std::path::{Path, PathBuf};

use agentchat_protocol::{canonical_mention_handle, run::PhaseKind};

/// Paths inside one run's directory.
///
/// ```text
/// <working_dir>/.agentchat/runs/<run-id>/
///   brief.md
///   plan.md
///   run.json
///   followups.md
///   plan/
///     reviews/r1/<reviewer>.json
///     dispositions/r1.json
///     findings.jsonl
///   code/
///     ...
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLayout {
    run_dir: PathBuf,
}

impl RunLayout {
    pub fn new(working_dir: &Path, run_id: &str) -> Self {
        Self {
            run_dir: working_dir.join(".agentchat").join("runs").join(run_id),
        }
    }

    pub fn from_run_dir(run_dir: impl Into<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
        }
    }

    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }

    /// The requirement the human signed off on before automation started.
    pub fn brief(&self) -> PathBuf {
        self.run_dir.join("brief.md")
    }

    /// The artifact a phase iterates on.
    ///
    /// The plan phase writes a document; the code phase edits the working tree
    /// itself, and `changes.md` is where the implementer records what it did.
    pub fn artifact(&self, phase: PhaseKind) -> PathBuf {
        match phase {
            PhaseKind::Plan => self.run_dir.join("plan.md"),
            PhaseKind::Code => self.run_dir.join("changes.md"),
        }
    }

    /// Advisory findings nobody adopted, ready to become Issues.
    pub fn followups(&self) -> PathBuf {
        self.run_dir.join("followups.md")
    }

    fn phase_dir(&self, phase: PhaseKind) -> PathBuf {
        self.run_dir.join(phase.as_str())
    }

    pub fn reviews_dir(&self, phase: PhaseKind, round: u32) -> PathBuf {
        self.phase_dir(phase)
            .join("reviews")
            .join(format!("r{round}"))
    }

    /// Where one reviewer writes its report.
    ///
    /// The reviewer name is slugified, so an agent id with a slash or a space
    /// cannot escape the reviews directory.
    pub fn review(&self, phase: PhaseKind, round: u32, reviewer: &str) -> PathBuf {
        self.reviews_dir(phase, round)
            .join(format!("{}.json", canonical_mention_handle(reviewer)))
    }

    pub fn dispositions(&self, phase: PhaseKind, round: u32) -> PathBuf {
        self.phase_dir(phase)
            .join("dispositions")
            .join(format!("r{round}.json"))
    }

    /// Append-only journal of every validated finding in a phase.
    pub fn findings(&self, phase: PhaseKind) -> PathBuf {
        self.phase_dir(phase).join("findings.jsonl")
    }

    /// Renders a path for an agent prompt, relative to the working directory
    /// when possible so the agent sees the same path it would type.
    pub fn display_path<'a>(&self, path: &'a Path, working_dir: &Path) -> &'a Path {
        path.strip_prefix(working_dir).unwrap_or(path)
    }

    /// Creates every directory a round needs before agents start writing.
    pub async fn prepare_round(&self, phase: PhaseKind, round: u32) -> Result<(), String> {
        for dir in [
            self.reviews_dir(phase, round),
            self.dispositions(phase, round)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ] {
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> RunLayout {
        RunLayout::new(Path::new("/work/tree"), "run-1")
    }

    #[test]
    fn run_files_sit_under_the_working_directory() {
        let layout = layout();

        assert_eq!(
            layout.run_dir(),
            Path::new("/work/tree/.agentchat/runs/run-1")
        );
        assert_eq!(
            layout.brief(),
            Path::new("/work/tree/.agentchat/runs/run-1/brief.md")
        );
        assert_eq!(
            layout.artifact(PhaseKind::Plan),
            Path::new("/work/tree/.agentchat/runs/run-1/plan.md")
        );
        assert_eq!(
            layout.artifact(PhaseKind::Code),
            Path::new("/work/tree/.agentchat/runs/run-1/changes.md")
        );
    }

    #[test]
    fn phases_and_rounds_get_separate_directories() {
        let layout = layout();

        assert!(layout
            .review(PhaseKind::Plan, 2, "opus")
            .ends_with("plan/reviews/r2/opus.json"));
        assert!(layout
            .review(PhaseKind::Code, 1, "deepseek")
            .ends_with("code/reviews/r1/deepseek.json"));
        assert!(layout
            .dispositions(PhaseKind::Plan, 3)
            .ends_with("plan/dispositions/r3.json"));
        assert!(layout
            .findings(PhaseKind::Code)
            .ends_with("code/findings.jsonl"));
    }

    #[test]
    fn reviewer_names_cannot_escape_the_reviews_directory() {
        let layout = layout();

        let path = layout.review(PhaseKind::Plan, 1, "../../etc/passwd");

        assert!(path.starts_with(layout.reviews_dir(PhaseKind::Plan, 1)));
        assert!(!path.to_string_lossy().contains(".."));
    }

    #[test]
    fn reviewer_names_with_spaces_become_slugs() {
        let layout = layout();

        assert!(layout
            .review(PhaseKind::Plan, 1, "GPT-5.6 Luna")
            .ends_with("gpt-5.6-luna.json"));
    }

    #[test]
    fn prompt_paths_are_shown_relative_to_the_working_directory() {
        let layout = layout();
        let working_dir = Path::new("/work/tree");

        assert_eq!(
            layout.display_path(&layout.artifact(PhaseKind::Plan), working_dir),
            Path::new(".agentchat/runs/run-1/plan.md")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prepare_round_creates_review_and_disposition_directories() {
        let root = tempfile::tempdir().unwrap();
        let layout = RunLayout::new(root.path(), "run-1");

        layout.prepare_round(PhaseKind::Plan, 1).await.unwrap();

        assert!(layout.reviews_dir(PhaseKind::Plan, 1).is_dir());
        assert!(layout
            .dispositions(PhaseKind::Plan, 1)
            .parent()
            .unwrap()
            .is_dir());
    }
}
