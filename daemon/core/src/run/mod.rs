//! Orchestration for multi-agent runs.
//!
//! The core is pure: how reviewer output is validated ([`findings`]), what the
//! author owes in response ([`disposition`]), and how iteration budget is spent
//! ([`budget`]) involve no processes, no I/O, and no git, so convergence
//! behaviour is pinned down by unit tests rather than by watching a real run.
//!
//! Around that sit the parts that touch the world: [`executor`] drives agents
//! through a stage, [`store`] persists so a restart resumes, [`gate`] asks the
//! human, and [`orchestrator`] joins them into a run.
//!
//! Preparing an isolated worktree is deliberately not here. The operator points
//! the daemon at a directory; how that directory came to exist is their call.

pub mod approval;
pub mod budget;
pub mod disposition;
pub mod executor;
pub mod findings;
pub mod gate;
pub mod layout;
pub mod orchestrator;
pub mod progress;
pub mod prompts;
pub mod state;
pub mod store;
pub mod supervisor;
#[cfg(test)]
pub mod testing;

pub use approval::build_packet;
pub use budget::{CycleLedger, RetryOutcome, RetryTally, MAX_CONSECUTIVE_REVISIONS_WITHOUT_REVIEW};
pub use disposition::{
    declined_advisory, disputed_findings, evaluate_gate, GateRejection, GateResult,
};
pub use executor::{ExecutorError, RoleAgent, StageExecutor};
pub use findings::{
    blocking_findings, group_findings, new_blocking_since, normalize, normalize_file,
    validate_report, validate_round, MAX_BLOCKING_PER_REPORT, MIN_EVIDENCE_CHARS,
};
pub use gate::{render_markdown, ApprovalGate, FileApprovalGate};
pub use layout::RunLayout;
pub use orchestrator::{RunOrchestrator, RunRoles};
pub use progress::{EventFormatter, ProgressSink, RunEvent, SilentProgress, TerminalProgress};
pub use prompts::{PromptKind, PromptSet};
pub use state::{DispositionOutcome, PhaseState, RoundSummary, RunState, TransitionError};
pub use store::{RunStore, RUN_SNAPSHOT_FILE};
pub use supervisor::{
    LogEntry, RunSupervisor, RunView, SharedSupervisor, SupervisorGate, SupervisorProgress,
};

#[cfg(test)]
mod convergence_tests {
    //! Whole-phase sequences, exercising validation, the gate, and the budget
    //! together the way an orchestrator would drive them.

    use agentchat_protocol::run::{
        CycleBudgetConfig, Disposition, DispositionAction, ExitReason, Finding, RawFinding,
        RawReviewReport, RetryKind,
    };

    use super::budget::RetryOutcome;
    use super::*;

    const EVIDENCE: &str = "reaching stage_two with an empty plan panics in run/stage.rs";

    /// A reviewer report naming one blocking finding per problem slug. The same
    /// slug from two reviewers is the same finding.
    fn report(reviewer: &str, round: u32, problems: &[&str]) -> RawReviewReport {
        RawReviewReport {
            reviewer: reviewer.into(),
            round,
            blocking: problems
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
        }
    }

    fn accept_all(findings: &[Finding]) -> Vec<Disposition> {
        blocking_findings(findings)
            .iter()
            .map(|finding| Disposition {
                finding_id: finding.finding_id.clone(),
                action: DispositionAction::Accepted,
                reason: String::new(),
                changed_files: vec![finding.file.clone()],
            })
            .collect()
    }

    fn ledger(max_cycles: u32) -> CycleLedger {
        CycleLedger::new(CycleBudgetConfig {
            max_cycles,
            ..CycleBudgetConfig::default()
        })
    }

    #[test]
    fn a_phase_whose_fixes_hold_converges() {
        let mut ledger = ledger(2);
        let mut seen: Vec<Finding> = Vec::new();

        // Cycle 1: two reviewers, one finding in common.
        let round = validate_round(&[
            report("opus", 1, &["alpha", "beta"]),
            report("luna", 1, &["alpha"]),
        ]);
        let fresh = new_blocking_since(&seen, &round);
        assert_eq!(fresh.len(), 2, "the shared finding counts once");
        assert!(evaluate_gate(&round, &accept_all(&round)).is_pass());
        seen.extend(round);
        assert_eq!(ledger.record_cycle(fresh.len()), None);

        // Cycle 2: only the already-known finding resurfaces.
        let round = validate_round(&[report("opus", 2, &["alpha"])]);
        let fresh = new_blocking_since(&seen, &round);
        assert!(fresh.is_empty());

        assert_eq!(
            ledger.record_cycle(fresh.len()),
            Some(ExitReason::Converged)
        );
        assert_eq!(ledger.cycles_used(), 2);
        assert!(ledger.exit_reason().unwrap().is_clean());
    }

    #[test]
    fn a_phase_that_keeps_surfacing_new_problems_stops_early() {
        // Generous cap: the churn detector, not the cap, must end this.
        let mut ledger = ledger(5);
        let mut seen: Vec<Finding> = Vec::new();

        let round = validate_round(&[report("opus", 1, &["alpha", "beta", "gamma"])]);
        let fresh = new_blocking_since(&seen, &round);
        seen.extend(round);
        assert_eq!(ledger.record_cycle(fresh.len()), None);

        // The revision fixed the first three and introduced three more.
        let round = validate_round(&[report("opus", 2, &["delta", "epsilon", "zeta"])]);
        let fresh = new_blocking_since(&seen, &round);
        assert_eq!(fresh.len(), 3);

        assert_eq!(ledger.record_cycle(fresh.len()), Some(ExitReason::Churn));
        assert_eq!(
            ledger.cycles_used(),
            2,
            "stopped without burning the remaining cap"
        );
    }

    #[test]
    fn a_flaky_reviewer_does_not_eat_the_discussion_budget() {
        let mut ledger = ledger(2);

        // One reviewer's file fails to parse twice; that is not a discussion.
        assert!(ledger
            .record_free_retry(RetryKind::InvalidOutput)
            .is_allowed());
        assert_eq!(
            ledger.record_free_retry(RetryKind::InvalidOutput),
            RetryOutcome::Exhausted { limit: 1 }
        );
        assert_eq!(ledger.cycles_used(), 0);
        assert!(ledger.has_budget());

        // Dropping that reviewer, the round completes on the survivors and the
        // phase still has both cycles available.
        let round = validate_round(&[report("luna", 1, &["alpha"])]);
        let fresh = new_blocking_since(&[], &round);
        assert!(evaluate_gate(&round, &accept_all(&round)).is_pass());

        assert_eq!(ledger.record_cycle(fresh.len()), None);
        assert_eq!(ledger.cycles_used(), 1);
    }

    #[test]
    fn an_author_that_ignores_blocking_findings_ends_up_stuck() {
        let mut ledger = ledger(3);
        let round = validate_round(&[report("opus", 1, &["alpha", "beta"])]);

        // First attempt answers only one of the two blocking findings.
        let partial = vec![accept_all(&round)[0].clone()];
        let rejection = evaluate_gate(&round, &partial)
            .rejection()
            .cloned()
            .expect("gate must reject an unanswered blocking finding");
        assert_eq!(rejection.missing.len(), 1);
        assert!(rejection.feedback().contains(&rejection.missing[0]));

        assert!(ledger
            .record_free_retry(RetryKind::Disposition)
            .is_allowed());
        assert_eq!(ledger.record_revision_without_review(), None);

        // Second attempt still ignores it.
        assert!(!evaluate_gate(&round, &partial).is_pass());
        assert_eq!(
            ledger.record_free_retry(RetryKind::Disposition),
            RetryOutcome::Exhausted { limit: 1 }
        );

        assert_eq!(
            ledger.record_revision_without_review(),
            Some(ExitReason::Stuck)
        );
        assert_eq!(ledger.cycles_used(), 0, "no cycle was ever completed");
    }

    #[test]
    fn sending_a_phase_back_gives_it_a_fresh_budget() {
        let mut ledger = ledger(2);
        let mut seen: Vec<Finding> = Vec::new();

        for (round_no, slugs) in [(1u32, &["alpha", "beta"][..]), (2, &["gamma"][..])] {
            let round = validate_round(&[report("opus", round_no, slugs)]);
            let fresh = new_blocking_since(&seen, &round);
            seen.extend(round);
            ledger.record_cycle(fresh.len());
        }
        assert_eq!(ledger.exit_reason(), Some(ExitReason::CycleCap));

        // The human reads the dispute list and sends it back with new context.
        ledger.record_human_rejection();

        assert!(ledger.has_budget());
        assert_eq!(ledger.human_iterations(), 1);
        let round = validate_round(&[report("opus", 3, &["alpha"])]);
        assert_eq!(
            ledger.record_cycle(new_blocking_since(&[], &round).len()),
            None
        );
    }
}
