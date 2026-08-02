//! Run and phase state machines.
//!
//! A phase loops through three stages: the author writes a version, reviewers
//! fan out over it, and the author answers every blocking finding while
//! producing the next version. The loop ends when the cycle ledger says so, and
//! always ends at a human approval gate.
//!
//! State lives here rather than in the protocol crate because it carries the
//! transition rules. It serialises as-is into `run.json`, and every stage is
//! idempotent given the files on disk, so a daemon restart re-enters the
//! recorded stage rather than restarting the run.

use agentchat_protocol::{
    now_millis,
    run::{CycleBudgetConfig, Disposition, ExitReason, Finding, PhaseKind, RunStatus, StageKind},
};
use serde::{Deserialize, Serialize};

use crate::run::budget::CycleLedger;
use crate::run::disposition::{evaluate_gate, GateRejection, GateResult};
use crate::run::findings::new_blocking_since;

/// A stage transition that does not apply in the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// The caller tried to advance from the wrong stage.
    UnexpectedStage {
        expected: StageKind,
        actual: StageKind,
    },
    /// The phase already exited and will not iterate again without human input.
    PhaseFinished(ExitReason),
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedStage { expected, actual } => write!(
                f,
                "expected stage {}, but the phase is in {}",
                expected.as_str(),
                actual.as_str()
            ),
            Self::PhaseFinished(reason) => {
                write!(f, "phase already exited with {}", reason.as_str())
            }
        }
    }
}

impl std::error::Error for TransitionError {}

/// What a completed review round produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundSummary {
    pub round: u32,
    /// Blocking findings this round raised that no earlier round had raised.
    /// This is what the cycle ledger watches.
    pub new_blocking: usize,
    /// Total blocking findings on the table for the author to answer.
    pub blocking_total: usize,
}

/// What happened when the author submitted its dispositions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispositionOutcome {
    /// The gate rejected the submission. The stage is unchanged and the author
    /// retries; this does not consume cycle budget.
    Rejected(GateRejection),
    /// The cycle was recorded and another review round follows.
    NextRound { round: u32, version: u32 },
    /// The cycle was recorded and the phase now awaits the human.
    AwaitingApproval(ExitReason),
}

/// One phase of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhaseState {
    pub kind: PhaseKind,
    pub stage: StageKind,
    /// Version of the artifact on disk. The initial draft is 1.
    pub version: u32,
    /// Review round currently open, or just closed.
    pub round: u32,
    pub ledger: CycleLedger,
    /// Every finding from every completed round, in order.
    pub findings: Vec<Finding>,
    /// Every disposition the author has written, in order.
    pub dispositions: Vec<Disposition>,
    /// Findings from the round awaiting disposition.
    current_round_findings: Vec<Finding>,
    /// New blocking count for the round awaiting disposition.
    pending_new_blocking: usize,
}

impl PhaseState {
    pub fn new(kind: PhaseKind, budget: CycleBudgetConfig) -> Self {
        Self {
            kind,
            stage: StageKind::Authoring,
            version: 0,
            round: 0,
            ledger: CycleLedger::new(budget),
            findings: Vec::new(),
            dispositions: Vec::new(),
            current_round_findings: Vec::new(),
            pending_new_blocking: 0,
        }
    }

    pub fn exit_reason(&self) -> Option<ExitReason> {
        self.ledger.exit_reason()
    }

    pub fn is_awaiting_approval(&self) -> bool {
        self.stage == StageKind::AwaitingApproval
    }

    /// Findings from the round the author still owes an answer for.
    pub fn current_round_findings(&self) -> &[Finding] {
        &self.current_round_findings
    }

    /// Records that the author wrote a fresh draft.
    ///
    /// Used for the initial draft and for the redraft after the human sends the
    /// phase back with comments.
    pub fn record_draft(&mut self) -> Result<u32, TransitionError> {
        self.expect_stage(StageKind::Authoring)?;

        self.version = self.version.saturating_add(1);
        self.round = 1;
        self.stage = StageKind::Reviewing;
        Ok(self.version)
    }

    /// Records a completed review round.
    ///
    /// `findings` is the validated output of every reviewer that survived the
    /// round. Reviewers dropped for repeated failures simply do not appear.
    pub fn record_review_round(
        &mut self,
        findings: Vec<Finding>,
    ) -> Result<RoundSummary, TransitionError> {
        self.expect_stage(StageKind::Reviewing)?;

        let new_blocking = new_blocking_since(&self.findings, &findings).len();
        let blocking_total = findings.iter().filter(|f| f.is_blocking()).count();

        self.findings.extend(findings.iter().cloned());
        self.current_round_findings = findings;
        self.pending_new_blocking = new_blocking;
        self.stage = StageKind::Dispositioning;

        Ok(RoundSummary {
            round: self.round,
            new_blocking,
            blocking_total,
        })
    }

    /// Records the author's answers plus the revision that accompanies them.
    ///
    /// A rejected submission leaves the stage untouched so the author can retry
    /// against [`GateRejection::feedback`] without spending cycle budget.
    pub fn record_disposition(
        &mut self,
        dispositions: Vec<Disposition>,
    ) -> Result<DispositionOutcome, TransitionError> {
        self.expect_stage(StageKind::Dispositioning)?;

        if let GateResult::Reject(rejection) =
            evaluate_gate(&self.current_round_findings, &dispositions)
        {
            return Ok(DispositionOutcome::Rejected(rejection));
        }

        self.dispositions.extend(dispositions);
        self.version = self.version.saturating_add(1);
        self.current_round_findings.clear();

        let exit = self.ledger.record_cycle(self.pending_new_blocking);
        self.pending_new_blocking = 0;

        match exit {
            Some(reason) => {
                self.stage = StageKind::AwaitingApproval;
                Ok(DispositionOutcome::AwaitingApproval(reason))
            }
            None => {
                self.round = self.round.saturating_add(1);
                self.stage = StageKind::Reviewing;
                Ok(DispositionOutcome::NextRound {
                    round: self.round,
                    version: self.version,
                })
            }
        }
    }

    /// Records a revision attempt that no review completed in between, and
    /// closes the phase if that leaves it looping on itself.
    pub fn record_revision_without_review(&mut self) -> Option<ExitReason> {
        let exit = self.ledger.record_revision_without_review();
        if exit.is_some() {
            self.stage = StageKind::AwaitingApproval;
        }
        exit
    }

    /// Ends the phase as stuck and sends it to the human gate.
    ///
    /// Used when the author exhausted its retries at the disposition gate: it
    /// kept producing answers, none of them valid, so nothing is moving. The
    /// human sees the same packet as any other exit, with the disputes section
    /// showing what never got answered.
    pub fn record_stuck(&mut self) -> ExitReason {
        let reason = self.ledger.mark_stuck();
        self.stage = StageKind::AwaitingApproval;
        reason
    }

    /// Reopens the phase after the human sent it back with comments.
    ///
    /// The budget resets in full: the human injecting new information is not
    /// the models spinning. Findings and dispositions are kept as history, but
    /// stop counting against `new_blocking` so the reviewers may legitimately
    /// re-raise anything the new direction changes.
    pub fn record_human_rejection(&mut self) {
        self.ledger.record_human_rejection();
        self.findings.clear();
        self.current_round_findings.clear();
        self.pending_new_blocking = 0;
        self.stage = StageKind::Authoring;
    }

    fn expect_stage(&self, expected: StageKind) -> Result<(), TransitionError> {
        if self.stage == expected {
            return Ok(());
        }
        if let (StageKind::AwaitingApproval, Some(reason)) = (self.stage, self.exit_reason()) {
            return Err(TransitionError::PhaseFinished(reason));
        }
        Err(TransitionError::UnexpectedStage {
            expected,
            actual: self.stage,
        })
    }
}

/// One attempt at delivering an Issue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: String,
    /// Issue this run is delivering, when it came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_id: Option<String>,
    /// Directory the run's files live under, normally a dedicated worktree.
    pub working_dir: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub status: RunStatus,
    pub plan: PhaseState,
    /// Created when the plan is approved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<PhaseState>,
    /// Held until the code phase opens.
    #[serde(default)]
    code_budget: CycleBudgetConfig,
}

impl RunState {
    pub fn new(run_id: impl Into<String>, working_dir: impl Into<String>) -> Self {
        let now = now_millis();
        Self {
            run_id: run_id.into(),
            issue_id: None,
            working_dir: working_dir.into(),
            created_at_ms: now,
            updated_at_ms: now,
            status: RunStatus::Planning,
            plan: PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default()),
            code: None,
            code_budget: CycleBudgetConfig {
                // Tests give objective signal in the code phase, so an extra
                // cycle there produces evidence; a fourth opinion on a plan
                // mostly produces prose.
                max_cycles: 3,
                ..CycleBudgetConfig::default()
            },
        }
    }

    /// Overrides the per-phase budgets.
    pub fn with_budgets(mut self, plan: CycleBudgetConfig, code: CycleBudgetConfig) -> Self {
        self.plan = PhaseState::new(PhaseKind::Plan, plan);
        self.code_budget = code;
        self
    }

    /// The phase an agent should currently be working in, if any.
    pub fn active_phase(&self) -> Option<&PhaseState> {
        match self.status {
            RunStatus::Planning | RunStatus::AwaitingPlanApproval => Some(&self.plan),
            RunStatus::Implementing | RunStatus::AwaitingCodeApproval => self.code.as_ref(),
            RunStatus::Completed | RunStatus::Cancelled => None,
        }
    }

    pub fn active_phase_mut(&mut self) -> Option<&mut PhaseState> {
        match self.status {
            RunStatus::Planning | RunStatus::AwaitingPlanApproval => Some(&mut self.plan),
            RunStatus::Implementing | RunStatus::AwaitingCodeApproval => self.code.as_mut(),
            RunStatus::Completed | RunStatus::Cancelled => None,
        }
    }

    /// Moves the run's status to match the active phase's stage.
    ///
    /// Call after advancing a phase so `run.json` reflects whether the run is
    /// waiting on an agent or on the human.
    pub fn sync_status(&mut self) {
        self.updated_at_ms = now_millis();
        self.status = match self.status {
            RunStatus::Planning | RunStatus::AwaitingPlanApproval => {
                if self.plan.is_awaiting_approval() {
                    RunStatus::AwaitingPlanApproval
                } else {
                    RunStatus::Planning
                }
            }
            RunStatus::Implementing | RunStatus::AwaitingCodeApproval => {
                match self.code.as_ref().map(PhaseState::is_awaiting_approval) {
                    Some(true) => RunStatus::AwaitingCodeApproval,
                    _ => RunStatus::Implementing,
                }
            }
            terminal => terminal,
        };
    }

    /// Accepts the plan and opens the code phase.
    pub fn approve_plan(&mut self) -> Result<(), TransitionError> {
        if !self.plan.is_awaiting_approval() {
            return Err(TransitionError::UnexpectedStage {
                expected: StageKind::AwaitingApproval,
                actual: self.plan.stage,
            });
        }

        self.code = Some(PhaseState::new(PhaseKind::Code, self.code_budget));
        self.status = RunStatus::Implementing;
        self.updated_at_ms = now_millis();
        Ok(())
    }

    /// Sends the current phase back to its author with a fresh budget.
    pub fn request_changes(&mut self) -> Result<(), TransitionError> {
        let status = self.status;
        let Some(phase) = self.active_phase_mut() else {
            return Err(TransitionError::UnexpectedStage {
                expected: StageKind::AwaitingApproval,
                actual: StageKind::Authoring,
            });
        };

        if !phase.is_awaiting_approval() {
            let actual = phase.stage;
            return Err(TransitionError::UnexpectedStage {
                expected: StageKind::AwaitingApproval,
                actual,
            });
        }

        phase.record_human_rejection();
        self.status = match status {
            RunStatus::AwaitingCodeApproval | RunStatus::Implementing => RunStatus::Implementing,
            _ => RunStatus::Planning,
        };
        self.updated_at_ms = now_millis();
        Ok(())
    }

    /// Accepts the code and finishes the run.
    pub fn approve_code(&mut self) -> Result<(), TransitionError> {
        match self.code.as_ref() {
            Some(code) if code.is_awaiting_approval() => {
                self.status = RunStatus::Completed;
                self.updated_at_ms = now_millis();
                Ok(())
            }
            Some(code) => Err(TransitionError::UnexpectedStage {
                expected: StageKind::AwaitingApproval,
                actual: code.stage,
            }),
            None => Err(TransitionError::UnexpectedStage {
                expected: StageKind::AwaitingApproval,
                actual: self.plan.stage,
            }),
        }
    }

    pub fn cancel(&mut self) {
        self.status = RunStatus::Cancelled;
        self.updated_at_ms = now_millis();
    }
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{DispositionAction, RawFinding, RawReviewReport};

    use super::*;
    use crate::run::findings::{blocking_findings, validate_round};

    const EVIDENCE: &str = "reaching stage_two with an empty plan panics in run/stage.rs";

    fn round(reviewer: &str, round: u32, slugs: &[&str]) -> Vec<Finding> {
        validate_round(&[RawReviewReport {
            reviewer: reviewer.into(),
            round,
            blocking: slugs
                .iter()
                .map(|slug| RawFinding {
                    category: "incorrect".into(),
                    location: format!("core/src/run/{slug}.rs"),
                    problem: format!("{slug} is wrong"),
                    evidence: EVIDENCE.into(),
                    recommendation: "fix it".into(),
                })
                .collect(),
            non_blocking: Vec::new(),
        }])
    }

    fn accept_all(findings: &[Finding]) -> Vec<Disposition> {
        blocking_findings(findings)
            .iter()
            .map(|finding| Disposition {
                finding_id: finding.finding_id.clone(),
                action: DispositionAction::Accepted,
                reason: String::new(),
                changed_files: Vec::new(),
            })
            .collect()
    }

    fn phase() -> PhaseState {
        PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default())
    }

    #[test]
    fn a_new_phase_starts_by_authoring() {
        let phase = phase();

        assert_eq!(phase.stage, StageKind::Authoring);
        assert_eq!(phase.version, 0);
        assert_eq!(phase.exit_reason(), None);
    }

    #[test]
    fn stages_advance_in_order() {
        let mut phase = phase();

        assert_eq!(phase.record_draft().unwrap(), 1);
        assert_eq!(phase.stage, StageKind::Reviewing);
        assert_eq!(phase.round, 1);

        let findings = round("opus", 1, &["alpha", "beta"]);
        let summary = phase.record_review_round(findings.clone()).unwrap();
        assert_eq!(phase.stage, StageKind::Dispositioning);
        assert_eq!(summary.new_blocking, 2);
        assert_eq!(summary.blocking_total, 2);

        let outcome = phase.record_disposition(accept_all(&findings)).unwrap();
        assert_eq!(
            outcome,
            DispositionOutcome::NextRound {
                round: 2,
                version: 2
            }
        );
        assert_eq!(phase.stage, StageKind::Reviewing);
    }

    #[test]
    fn advancing_from_the_wrong_stage_is_rejected() {
        let mut phase = phase();

        let error = phase.record_review_round(Vec::new()).unwrap_err();

        assert_eq!(
            error,
            TransitionError::UnexpectedStage {
                expected: StageKind::Reviewing,
                actual: StageKind::Authoring,
            }
        );
    }

    #[test]
    fn a_rejected_disposition_leaves_the_stage_and_budget_untouched() {
        let mut phase = phase();
        phase.record_draft().unwrap();
        let findings = round("opus", 1, &["alpha", "beta"]);
        phase.record_review_round(findings.clone()).unwrap();

        // Answer only one of the two blocking findings.
        let partial = vec![accept_all(&findings)[0].clone()];
        let outcome = phase.record_disposition(partial).unwrap();

        assert!(matches!(outcome, DispositionOutcome::Rejected(_)));
        assert_eq!(phase.stage, StageKind::Dispositioning);
        assert_eq!(phase.version, 1, "no revision was accepted");
        assert_eq!(phase.ledger.cycles_used(), 0, "no cycle was consumed");

        // The author can still retry from the same stage.
        let outcome = phase.record_disposition(accept_all(&findings)).unwrap();
        assert!(matches!(outcome, DispositionOutcome::NextRound { .. }));
    }

    #[test]
    fn a_phase_reaches_approval_when_the_ledger_exits() {
        let mut phase = phase();
        phase.record_draft().unwrap();

        // Cycle 1 raises two findings.
        let findings = round("opus", 1, &["alpha", "beta"]);
        phase.record_review_round(findings.clone()).unwrap();
        phase.record_disposition(accept_all(&findings)).unwrap();

        // Cycle 2 re-raises only what is already known.
        let repeat = round("opus", 2, &["alpha"]);
        let summary = phase.record_review_round(repeat.clone()).unwrap();
        assert_eq!(summary.new_blocking, 0);

        let outcome = phase.record_disposition(accept_all(&repeat)).unwrap();

        assert_eq!(
            outcome,
            DispositionOutcome::AwaitingApproval(ExitReason::Converged)
        );
        assert!(phase.is_awaiting_approval());
        assert_eq!(phase.version, 3, "two revisions on top of the draft");
    }

    #[test]
    fn a_finished_phase_reports_why_it_will_not_advance() {
        let mut phase = phase();
        phase.record_draft().unwrap();
        let findings = round("opus", 1, &[]);
        phase.record_review_round(findings).unwrap();
        phase.record_disposition(Vec::new()).unwrap();

        let error = phase.record_review_round(Vec::new()).unwrap_err();

        assert_eq!(error, TransitionError::PhaseFinished(ExitReason::Converged));
    }

    #[test]
    fn human_rejection_reopens_the_phase_for_a_redraft() {
        let mut phase = phase();
        phase.record_draft().unwrap();
        let findings = round("opus", 1, &[]);
        phase.record_review_round(findings).unwrap();
        phase.record_disposition(Vec::new()).unwrap();
        assert!(phase.is_awaiting_approval());

        phase.record_human_rejection();

        assert_eq!(phase.stage, StageKind::Authoring);
        assert_eq!(phase.exit_reason(), None);
        assert_eq!(phase.ledger.cycles_used(), 0);
        assert_eq!(phase.ledger.human_iterations(), 1);
        assert_eq!(phase.record_draft().unwrap(), 3);
    }

    #[test]
    fn reviewers_may_re_raise_after_the_human_changes_direction() {
        let mut phase = phase();
        phase.record_draft().unwrap();
        let findings = round("opus", 1, &["alpha"]);
        phase.record_review_round(findings.clone()).unwrap();
        phase.record_disposition(accept_all(&findings)).unwrap();
        let repeat = round("opus", 2, &["alpha"]);
        phase.record_review_round(repeat.clone()).unwrap();
        phase.record_disposition(accept_all(&repeat)).unwrap();

        phase.record_human_rejection();
        phase.record_draft().unwrap();

        // The same finding counts as new again: the direction changed under it.
        let summary = phase
            .record_review_round(round("opus", 1, &["alpha"]))
            .unwrap();
        assert_eq!(summary.new_blocking, 1);
    }

    #[test]
    fn a_run_tracks_which_phase_is_active() {
        let mut run = RunState::new("run-1", "/tmp/worktree");

        assert_eq!(run.status, RunStatus::Planning);
        assert_eq!(run.active_phase().map(|p| p.kind), Some(PhaseKind::Plan));
        assert!(run.code.is_none());

        run.plan.record_draft().unwrap();
        let findings = round("opus", 1, &[]);
        run.plan.record_review_round(findings).unwrap();
        run.plan.record_disposition(Vec::new()).unwrap();
        run.sync_status();

        assert_eq!(run.status, RunStatus::AwaitingPlanApproval);
        assert!(run.status.awaits_human());

        run.approve_plan().unwrap();

        assert_eq!(run.status, RunStatus::Implementing);
        assert_eq!(run.active_phase().map(|p| p.kind), Some(PhaseKind::Code));
    }

    #[test]
    fn approving_a_plan_that_is_still_iterating_is_rejected() {
        let mut run = RunState::new("run-1", "/tmp/worktree");
        run.plan.record_draft().unwrap();

        assert!(run.approve_plan().is_err());
        assert_eq!(run.status, RunStatus::Planning);
    }

    #[test]
    fn requesting_changes_returns_the_active_phase_to_its_author() {
        let mut run = RunState::new("run-1", "/tmp/worktree");
        run.plan.record_draft().unwrap();
        run.plan.record_review_round(round("opus", 1, &[])).unwrap();
        run.plan.record_disposition(Vec::new()).unwrap();
        run.sync_status();

        run.request_changes().unwrap();

        assert_eq!(run.status, RunStatus::Planning);
        assert_eq!(run.plan.stage, StageKind::Authoring);
        assert_eq!(run.plan.ledger.human_iterations(), 1);
    }

    #[test]
    fn a_run_completes_after_the_code_phase_is_approved() {
        let mut run = RunState::new("run-1", "/tmp/worktree");
        run.plan.record_draft().unwrap();
        run.plan.record_review_round(round("opus", 1, &[])).unwrap();
        run.plan.record_disposition(Vec::new()).unwrap();
        run.approve_plan().unwrap();

        let code = run.code.as_mut().unwrap();
        code.record_draft().unwrap();
        code.record_review_round(round("deepseek", 1, &[])).unwrap();
        code.record_disposition(Vec::new()).unwrap();
        run.sync_status();
        assert_eq!(run.status, RunStatus::AwaitingCodeApproval);

        run.approve_code().unwrap();

        assert_eq!(run.status, RunStatus::Completed);
        assert!(run.status.is_terminal());
        assert!(run.active_phase().is_none());
    }

    #[test]
    fn run_state_round_trips_through_json() {
        let mut run = RunState::new("run-1", "/tmp/worktree");
        run.issue_id = Some("issue-7".into());
        run.plan.record_draft().unwrap();
        let findings = round("opus", 1, &["alpha"]);
        run.plan.record_review_round(findings.clone()).unwrap();
        run.plan.record_disposition(accept_all(&findings)).unwrap();

        let json = serde_json::to_string_pretty(&run).unwrap();
        let decoded: RunState = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, run);
        // Resuming re-enters the recorded stage rather than the run.
        assert_eq!(decoded.plan.stage, StageKind::Reviewing);
        assert_eq!(decoded.plan.round, 2);
    }
}
