//! Drives a run from brief to merged-ready, stopping only at the human gates.
//!
//! This is the loop that replaces the copy-and-paste: it advances each phase
//! through as many review cycles as its budget allows, persists after every
//! stage so a restart resumes rather than restarts, and hands the human a packet
//! exactly twice — once for the plan, once for the code.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use agentchat_protocol::run::{ApprovalDecision, ExitReason, PhaseKind, RunStatus, StageKind};
use tracing::info;

use crate::run::progress::ProgressSink;

use crate::run::approval::build_packet;
use crate::run::executor::{RoleAgent, StageExecutor};
use crate::run::gate::{append_feedback, ApprovalGate};
use crate::run::layout::RunLayout;
use crate::run::prompts::PromptSet;
use crate::run::state::{PhaseState, RunState};
use crate::run::store::RunStore;

/// Which agent plays which part.
///
/// Plan review gets more reviewers than code review by default: a plan is short,
/// so a third opinion is cheap and diversity pays off most there, whereas code
/// review means reading the repository and is where the cost actually lands.
pub struct RunRoles {
    pub planner: RoleAgent,
    pub plan_reviewers: Vec<RoleAgent>,
    pub implementer: RoleAgent,
    pub code_reviewers: Vec<RoleAgent>,
}

/// Runs phases, persists progress, and asks the human at the gates.
pub struct RunOrchestrator {
    executor: StageExecutor,
    store: RunStore,
    layout: RunLayout,
}

impl RunOrchestrator {
    pub fn new(working_dir: impl Into<PathBuf>, run_id: &str, prompts: PromptSet) -> Self {
        let working_dir = working_dir.into();
        let layout = RunLayout::new(&working_dir, run_id);
        Self {
            executor: StageExecutor::new(working_dir.clone(), layout.clone(), prompts),
            store: RunStore::new(&working_dir),
            layout,
        }
    }

    /// Reports live activity somewhere the operator can see it.
    pub fn with_progress(mut self, progress: Rc<dyn ProgressSink>) -> Self {
        self.executor.set_progress(progress);
        self
    }

    pub fn layout(&self) -> &RunLayout {
        &self.layout
    }

    /// Copies the human-approved brief into the run directory.
    ///
    /// The run owns its own copy so later feedback can be appended to it without
    /// editing whatever file the user pointed at.
    pub async fn import_brief(&self, source: &Path) -> Result<PathBuf, String> {
        let body = tokio::fs::read_to_string(source)
            .await
            .map_err(|e| format!("cannot read brief {}: {e}", source.display()))?;
        if body.trim().is_empty() {
            return Err(format!("brief {} is empty", source.display()));
        }

        let target = self.layout.brief();
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        tokio::fs::write(&target, body)
            .await
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        Ok(target)
    }

    /// Picks up an existing run, or starts a fresh one under the same id.
    ///
    /// A snapshot that is already finished is not resumed — reusing its id would
    /// otherwise silently reopen completed work.
    pub async fn load_or_start(
        &mut self,
        run_id: &str,
        working_dir: &Path,
    ) -> Result<(RunState, bool), String> {
        if let Ok(existing) = self.store.load_run(run_id).await {
            if !existing.status.is_terminal() {
                self.store.insert(existing.clone());
                return Ok((existing, true));
            }
            return Err(format!(
                "run {run_id} already finished ({}); use a different id",
                existing.status.as_str()
            ));
        }

        Ok((RunState::new(run_id, working_dir.to_string_lossy()), false))
    }

    /// Advances one phase until it reaches its approval gate.
    ///
    /// Entry is by recorded stage, not from the beginning, so a run interrupted
    /// mid-round picks up where it stopped.
    pub async fn drive_phase(
        &mut self,
        run: &mut RunState,
        which: PhaseKind,
        author: &RoleAgent,
        reviewers: &[RoleAgent],
    ) -> Result<ExitReason, String> {
        loop {
            let stage = phase(run, which)?.stage;
            match stage {
                StageKind::Authoring => {
                    let target = phase_mut(run, which)?;
                    self.executor
                        .execute_author(target, author)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                StageKind::Reviewing => {
                    let target = phase_mut(run, which)?;
                    let summary = self
                        .executor
                        .execute_review_round(target, reviewers)
                        .await
                        .map_err(|e| e.to_string())?;
                    info!(
                        "{} round {} raised {} new blocking finding(s)",
                        which.as_str(),
                        summary.round,
                        summary.new_blocking
                    );
                }
                StageKind::Dispositioning => {
                    let target = phase_mut(run, which)?;
                    self.executor
                        .execute_disposition(target, author)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                StageKind::AwaitingApproval => break,
            }

            self.checkpoint(run).await?;
        }

        phase(run, which)?
            .exit_reason()
            .ok_or_else(|| "phase reached the gate without an exit reason".to_string())
    }

    /// Drives the whole run, pausing at each gate until the human decides.
    pub async fn drive(
        &mut self,
        run: &mut RunState,
        roles: &RunRoles,
        gate: &dyn ApprovalGate,
    ) -> Result<RunStatus, String> {
        self.drive_until(run, roles, gate, None).await
    }

    /// Drives the run, optionally stopping once a phase clears its gate.
    ///
    /// Stopping after the plan is how a first outing stays safe: the plan phase
    /// only reads the repository and writes one document, so nothing touches
    /// the working tree until you have seen how the agents behave. The run stays
    /// resumable — rerun with the same id and no limit to continue.
    pub async fn drive_until(
        &mut self,
        run: &mut RunState,
        roles: &RunRoles,
        gate: &dyn ApprovalGate,
        stop_after: Option<PhaseKind>,
    ) -> Result<RunStatus, String> {
        loop {
            let which = match run.status {
                RunStatus::Planning | RunStatus::AwaitingPlanApproval => PhaseKind::Plan,
                RunStatus::Implementing | RunStatus::AwaitingCodeApproval => PhaseKind::Code,
                terminal => return Ok(terminal),
            };
            let (author, reviewers) = match which {
                PhaseKind::Plan => (&roles.planner, &roles.plan_reviewers),
                PhaseKind::Code => (&roles.implementer, &roles.code_reviewers),
            };

            let exit = self.drive_phase(run, which, author, reviewers).await?;
            run.sync_status();
            self.checkpoint(run).await?;
            info!("{} phase exited: {}", which.as_str(), exit.as_str());

            let packet = build_packet(&run.run_id, phase(run, which)?);
            match gate.request(&packet).await? {
                ApprovalDecision::Approve => match which {
                    PhaseKind::Plan => run.approve_plan().map_err(|e| e.to_string())?,
                    PhaseKind::Code => run.approve_code().map_err(|e| e.to_string())?,
                },
                ApprovalDecision::RequestChanges { comments } => {
                    let round = phase(run, which)?.ledger.human_iterations() + 1;
                    append_feedback(&self.layout.brief(), round, &comments).await?;
                    run.request_changes().map_err(|e| e.to_string())?;
                }
                ApprovalDecision::Cancel => run.cancel(),
            }

            self.checkpoint(run).await?;

            if stop_after == Some(which) && !run.status.is_terminal() {
                return Ok(run.status);
            }
        }
    }

    /// Mirrors the run to disk. Called after every stage so an interrupted run
    /// resumes at the stage it was in rather than at the beginning.
    async fn checkpoint(&mut self, run: &RunState) -> Result<(), String> {
        self.store.insert(run.clone());
        self.store.flush_run(&run.run_id).await.map(|_| ())
    }

    pub fn store(&self) -> &RunStore {
        &self.store
    }
}

fn phase(run: &RunState, which: PhaseKind) -> Result<&PhaseState, String> {
    match which {
        PhaseKind::Plan => Ok(&run.plan),
        PhaseKind::Code => run
            .code
            .as_ref()
            .ok_or_else(|| "code phase has not been opened".to_string()),
    }
}

fn phase_mut(run: &mut RunState, which: PhaseKind) -> Result<&mut PhaseState, String> {
    match which {
        PhaseKind::Plan => Ok(&mut run.plan),
        PhaseKind::Code => run
            .code
            .as_mut()
            .ok_or_else(|| "code phase has not been opened".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use agentchat_protocol::run::ApprovalPacket;
    use async_trait::async_trait;

    use super::*;
    use crate::run::testing::ReactiveAgent;

    /// A gate that answers from a script and keeps what it was shown.
    struct ScriptedGate {
        decisions: RefCell<Vec<ApprovalDecision>>,
        seen: RefCell<Vec<ApprovalPacket>>,
    }

    impl ScriptedGate {
        fn new(decisions: Vec<ApprovalDecision>) -> Self {
            Self {
                decisions: RefCell::new(decisions),
                seen: RefCell::new(Vec::new()),
            }
        }
    }

    #[async_trait(?Send)]
    impl ApprovalGate for ScriptedGate {
        async fn request(&self, packet: &ApprovalPacket) -> Result<ApprovalDecision, String> {
            self.seen.borrow_mut().push(packet.clone());
            if self.decisions.borrow().is_empty() {
                return Err("gate ran out of scripted decisions".into());
            }
            Ok(self.decisions.borrow_mut().remove(0))
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        working_dir: PathBuf,
    }

    async fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let working_dir = dir.path().to_path_buf();
        Fixture {
            _dir: dir,
            working_dir,
        }
    }

    async fn orchestrator(fixture: &Fixture) -> RunOrchestrator {
        let orchestrator =
            RunOrchestrator::new(fixture.working_dir.clone(), "run-1", PromptSet::builtin());
        let source = fixture.working_dir.join("requirement.md");
        tokio::fs::write(&source, "# Goal\n\nAdd the thing.\n")
            .await
            .unwrap();
        orchestrator.import_brief(&source).await.unwrap();
        orchestrator
    }

    /// Reviewers that raise one problem in round one and nothing after, so the
    /// phase converges on the second cycle.
    fn converging_reviewers(fixture: &Fixture, names: &[&str]) -> Vec<RoleAgent> {
        names
            .iter()
            .map(|name| {
                RoleAgent::new(
                    *name,
                    ReactiveAgent::raising(name, &fixture.working_dir, &[&["alpha"]]),
                )
            })
            .collect()
    }

    fn roles(fixture: &Fixture) -> RunRoles {
        RunRoles {
            planner: RoleAgent::new(
                "planner",
                ReactiveAgent::new("planner", &fixture.working_dir),
            ),
            plan_reviewers: converging_reviewers(fixture, &["opus", "luna"]),
            implementer: RoleAgent::new(
                "implementer",
                ReactiveAgent::new("implementer", &fixture.working_dir),
            ),
            code_reviewers: converging_reviewers(fixture, &["deepseek"]),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_run_stops_at_exactly_two_gates() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![ApprovalDecision::Approve, ApprovalDecision::Approve]);

        let status = orchestrator
            .drive(&mut run, &roles(&fixture), &gate)
            .await
            .unwrap();

        assert_eq!(status, RunStatus::Completed);
        let seen = gate.seen.borrow();
        assert_eq!(seen.len(), 2, "the human was asked twice, no more");
        assert_eq!(seen[0].phase, PhaseKind::Plan);
        assert_eq!(seen[1].phase, PhaseKind::Code);
        assert_eq!(seen[0].exit_reason, Some(ExitReason::Converged));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_plan_artifact_is_written_before_review() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![ApprovalDecision::Approve, ApprovalDecision::Approve]);

        orchestrator
            .drive(&mut run, &roles(&fixture), &gate)
            .await
            .unwrap();

        assert!(orchestrator.layout().artifact(PhaseKind::Plan).is_file());
        assert!(orchestrator.layout().artifact(PhaseKind::Code).is_file());
        assert!(orchestrator.layout().findings(PhaseKind::Plan).is_file());
        assert!(orchestrator.layout().followups().is_file());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn requesting_changes_reopens_the_phase_with_a_fresh_budget() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![
            ApprovalDecision::RequestChanges {
                comments: "reuse the existing store".into(),
            },
            ApprovalDecision::Approve,
            ApprovalDecision::Approve,
        ]);

        let status = orchestrator
            .drive(&mut run, &roles(&fixture), &gate)
            .await
            .unwrap();

        assert_eq!(status, RunStatus::Completed);
        assert_eq!(gate.seen.borrow().len(), 3);
        assert_eq!(run.plan.ledger.human_iterations(), 1);

        // The comments went where the planner will actually read them.
        let brief = tokio::fs::read_to_string(orchestrator.layout().brief())
            .await
            .unwrap();
        assert!(brief.contains("## Human feedback (round 1)"));
        assert!(brief.contains("reuse the existing store"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_ends_the_run_without_touching_the_code_phase() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![ApprovalDecision::Cancel]);

        let status = orchestrator
            .drive(&mut run, &roles(&fixture), &gate)
            .await
            .unwrap();

        assert_eq!(status, RunStatus::Cancelled);
        assert!(run.code.is_none());
        assert!(!orchestrator.layout().artifact(PhaseKind::Code).exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn every_stage_is_checkpointed_so_a_restart_resumes() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let roles = roles(&fixture);

        // Drive only the plan phase, as if the daemon died at its gate.
        orchestrator
            .drive_phase(
                &mut run,
                PhaseKind::Plan,
                &roles.planner,
                &roles.plan_reviewers,
            )
            .await
            .unwrap();
        run.sync_status();
        orchestrator.checkpoint(&run).await.unwrap();

        let mut restarted = RunStore::new(&fixture.working_dir);
        let ids = restarted.resume().await.unwrap();

        assert_eq!(ids, vec!["run-1".to_string()]);
        let resumed = restarted.get("run-1").unwrap();
        assert_eq!(resumed.status, RunStatus::AwaitingPlanApproval);
        assert_eq!(resumed.plan.stage, StageKind::AwaitingApproval);
        assert_eq!(resumed.plan.exit_reason(), Some(ExitReason::Converged));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_phase_resumes_from_its_recorded_stage() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let roles = roles(&fixture);

        // Pretend the author already wrote the plan before the restart.
        tokio::fs::write(orchestrator.layout().artifact(PhaseKind::Plan), "# Plan\n")
            .await
            .unwrap();
        run.plan.record_draft().unwrap();
        assert_eq!(run.plan.stage, StageKind::Reviewing);

        orchestrator
            .drive_phase(
                &mut run,
                PhaseKind::Plan,
                &roles.planner,
                &roles.plan_reviewers,
            )
            .await
            .unwrap();

        // The planner was never asked to author again.
        assert_eq!(run.plan.version, 3, "one draft plus two revisions");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_disputed_finding_reaches_the_human() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let roles = RunRoles {
            planner: RoleAgent::new(
                "planner",
                ReactiveAgent::disputing("planner", &fixture.working_dir, "out of scope per brief"),
            ),
            plan_reviewers: converging_reviewers(&fixture, &["opus"]),
            implementer: RoleAgent::new(
                "implementer",
                ReactiveAgent::new("implementer", &fixture.working_dir),
            ),
            code_reviewers: converging_reviewers(&fixture, &["deepseek"]),
        };
        let gate = ScriptedGate::new(vec![ApprovalDecision::Cancel]);

        orchestrator.drive(&mut run, &roles, &gate).await.unwrap();

        let seen = gate.seen.borrow();
        assert_eq!(seen[0].disputes.len(), 1);
        assert_eq!(
            seen[0].disputes[0].disposition.as_ref().unwrap().reason,
            "out of scope per brief"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn advisory_findings_survive_to_the_packet() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![ApprovalDecision::Cancel]);

        orchestrator
            .drive(&mut run, &roles(&fixture), &gate)
            .await
            .unwrap();

        let seen = gate.seen.borrow();
        assert!(
            !seen[0].followups.is_empty(),
            "suggestions nobody adopted must still reach the human"
        );
        assert!(!seen[0].archive.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stopping_after_the_plan_leaves_the_code_phase_untouched() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        let gate = ScriptedGate::new(vec![ApprovalDecision::Approve]);

        let status = orchestrator
            .drive_until(&mut run, &roles(&fixture), &gate, Some(PhaseKind::Plan))
            .await
            .unwrap();

        assert_eq!(status, RunStatus::Implementing);
        assert_eq!(
            gate.seen.borrow().len(),
            1,
            "only the plan gate was reached"
        );
        assert!(!orchestrator.layout().artifact(PhaseKind::Code).exists());
        assert!(!status.is_terminal(), "the run can still be continued");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_run_stopped_after_the_plan_resumes_into_the_code_phase() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        orchestrator
            .drive_until(
                &mut run,
                &roles(&fixture),
                &ScriptedGate::new(vec![ApprovalDecision::Approve]),
                Some(PhaseKind::Plan),
            )
            .await
            .unwrap();

        let (mut resumed, was_resumed) = orchestrator
            .load_or_start("run-1", &fixture.working_dir)
            .await
            .unwrap();
        assert!(was_resumed);

        let status = orchestrator
            .drive(
                &mut resumed,
                &roles(&fixture),
                &ScriptedGate::new(vec![ApprovalDecision::Approve]),
            )
            .await
            .unwrap();

        assert_eq!(status, RunStatus::Completed);
        assert!(orchestrator.layout().artifact(PhaseKind::Code).is_file());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn progress_reports_each_stage_and_reviewer() {
        let fixture = fixture().await;
        let recorder = Rc::new(crate::run::progress::tests::RecordingProgress::new());
        let mut orchestrator = orchestrator(&fixture).await.with_progress(recorder.clone());
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());

        orchestrator
            .drive_until(
                &mut run,
                &roles(&fixture),
                &ScriptedGate::new(vec![ApprovalDecision::Approve]),
                Some(PhaseKind::Plan),
            )
            .await
            .unwrap();

        let lines = recorder.lines.borrow().join("\n");
        assert!(lines.contains("Authoring"), "authoring stage not reported");
        assert!(lines.contains("Reviewing"), "review stage not reported");
        assert!(
            lines.contains("Dispositioning"),
            "disposition stage not reported"
        );
        assert!(lines.contains("ReviewerFinished"), "reviewers not reported");
        assert!(lines.contains("Round"), "round result not reported");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_or_start_resumes_an_unfinished_run() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let roles = roles(&fixture);
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        orchestrator
            .drive_phase(
                &mut run,
                PhaseKind::Plan,
                &roles.planner,
                &roles.plan_reviewers,
            )
            .await
            .unwrap();
        run.sync_status();
        orchestrator.checkpoint(&run).await.unwrap();

        let (resumed, was_resumed) = orchestrator
            .load_or_start("run-1", &fixture.working_dir)
            .await
            .unwrap();

        assert!(was_resumed);
        assert_eq!(resumed.status, RunStatus::AwaitingPlanApproval);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_or_start_creates_a_run_when_none_exists() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;

        let (run, was_resumed) = orchestrator
            .load_or_start("run-1", &fixture.working_dir)
            .await
            .unwrap();

        assert!(!was_resumed);
        assert_eq!(run.status, RunStatus::Planning);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_finished_run_is_not_silently_reopened() {
        let fixture = fixture().await;
        let mut orchestrator = orchestrator(&fixture).await;
        let mut run = RunState::new("run-1", fixture.working_dir.to_string_lossy());
        run.cancel();
        orchestrator.checkpoint(&run).await.unwrap();

        let error = orchestrator
            .load_or_start("run-1", &fixture.working_dir)
            .await
            .unwrap_err();

        assert!(error.contains("already finished"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_empty_brief_is_rejected_before_any_agent_runs() {
        let fixture = fixture().await;
        let orchestrator =
            RunOrchestrator::new(fixture.working_dir.clone(), "run-2", PromptSet::builtin());
        let source = fixture.working_dir.join("empty.md");
        tokio::fs::write(&source, "   \n").await.unwrap();

        assert!(orchestrator.import_brief(&source).await.is_err());
    }
}
