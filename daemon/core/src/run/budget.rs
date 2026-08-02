//! Cycle accounting and stop conditions for a run phase.
//!
//! A *cycle* is one full exchange: the author produces a version, reviewers fan
//! out over it, and the author dispositions every blocking finding before
//! producing the next version. The initial draft is not a cycle, so
//! `max_cycles = 2` delivers version 3.
//!
//! Two rules do most of the work:
//!
//! - Only a real exchange of opinion for a revision consumes budget. Schema
//!   failures, crashed agents, and a rejected disposition set draw on separate
//!   free-retry allowances, so one flaky agent cannot eat the discussion budget
//!   and leave the plan reviewed only once.
//! - Running out of cycles is not a failure mode. All four exit reasons lead to
//!   the same human approval gate; they differ only in what the approval
//!   packet's dispute section contains.

use agentchat_protocol::run::{CycleBudgetConfig, ExitReason, RetryKind};
use serde::{Deserialize, Serialize};

/// How many times the author may revise without a review completing in between
/// before the phase is considered stuck.
pub const MAX_CONSECUTIVE_REVISIONS_WITHOUT_REVIEW: u32 = 1;

/// Free retries consumed so far, per failure kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryTally {
    #[serde(default)]
    pub invalid_output: u32,
    #[serde(default)]
    pub agent_failure: u32,
    #[serde(default)]
    pub disposition: u32,
}

impl RetryTally {
    pub fn get(&self, kind: RetryKind) -> u32 {
        match kind {
            RetryKind::InvalidOutput => self.invalid_output,
            RetryKind::AgentFailure => self.agent_failure,
            RetryKind::Disposition => self.disposition,
        }
    }

    fn increment(&mut self, kind: RetryKind) {
        let slot = match kind {
            RetryKind::InvalidOutput => &mut self.invalid_output,
            RetryKind::AgentFailure => &mut self.agent_failure,
            RetryKind::Disposition => &mut self.disposition,
        };
        *slot = slot.saturating_add(1);
    }
}

/// Result of asking for a free retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryOutcome {
    /// Retry the step. Budget is untouched.
    Allowed { used: u32, limit: u32 },
    /// The allowance is spent. The caller decides what that means: drop this
    /// reviewer from the round, or — for a disposition failure — treat the
    /// phase as stuck.
    Exhausted { limit: u32 },
}

impl RetryOutcome {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed { .. })
    }
}

/// Cycle budget state for one phase, persisted as part of the run snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleLedger {
    config: CycleBudgetConfig,
    cycles_used: u32,
    consecutive_revisions_without_review: u32,
    retries: RetryTally,
    /// New blocking findings produced by each completed cycle, in order.
    new_blocking_history: Vec<usize>,
    human_iterations: u32,
    exit_reason: Option<ExitReason>,
}

impl CycleLedger {
    pub fn new(config: CycleBudgetConfig) -> Self {
        Self {
            config,
            cycles_used: 0,
            consecutive_revisions_without_review: 0,
            retries: RetryTally::default(),
            new_blocking_history: Vec::new(),
            human_iterations: 0,
            exit_reason: None,
        }
    }

    pub fn config(&self) -> CycleBudgetConfig {
        self.config
    }

    pub fn cycles_used(&self) -> u32 {
        self.cycles_used
    }

    pub fn human_iterations(&self) -> u32 {
        self.human_iterations
    }

    pub fn retries(&self) -> RetryTally {
        self.retries
    }

    pub fn new_blocking_history(&self) -> &[usize] {
        &self.new_blocking_history
    }

    pub fn exit_reason(&self) -> Option<ExitReason> {
        self.exit_reason
    }

    pub fn is_terminal(&self) -> bool {
        self.exit_reason.is_some()
    }

    /// Whether another cycle may start.
    pub fn has_budget(&self) -> bool {
        !self.is_terminal() && self.cycles_used < self.config.max_cycles
    }

    /// Records a completed cycle and returns the exit reason, if any.
    ///
    /// `new_blocking` is the count of blocking findings this round raised that
    /// no previous round had raised — see
    /// [`new_blocking_since`](crate::run::findings::new_blocking_since).
    pub fn record_cycle(&mut self, new_blocking: usize) -> Option<ExitReason> {
        if let Some(reason) = self.exit_reason {
            return Some(reason);
        }

        self.cycles_used = self.cycles_used.saturating_add(1);
        self.new_blocking_history.push(new_blocking);
        self.consecutive_revisions_without_review = 0;
        self.exit_reason = self.evaluate();
        self.exit_reason
    }

    /// Records a revision that no review completed in between.
    ///
    /// Catches the author looping on itself, which happens when its output
    /// keeps failing validation.
    pub fn record_revision_without_review(&mut self) -> Option<ExitReason> {
        if let Some(reason) = self.exit_reason {
            return Some(reason);
        }

        self.consecutive_revisions_without_review =
            self.consecutive_revisions_without_review.saturating_add(1);
        self.exit_reason = self.evaluate();
        self.exit_reason
    }

    /// Draws on a free-retry allowance. Never consumes cycle budget.
    pub fn record_free_retry(&mut self, kind: RetryKind) -> RetryOutcome {
        let limit = match kind {
            RetryKind::InvalidOutput => self.config.free_retries.invalid_output,
            RetryKind::AgentFailure => self.config.free_retries.agent_failure,
            RetryKind::Disposition => self.config.free_retries.disposition,
        };

        if self.retries.get(kind) >= limit {
            return RetryOutcome::Exhausted { limit };
        }

        self.retries.increment(kind);
        RetryOutcome::Allowed {
            used: self.retries.get(kind),
            limit,
        }
    }

    /// Ends the phase as [`ExitReason::Stuck`].
    ///
    /// For failures the counters cannot see, chiefly an author that used up its
    /// retries at the disposition gate without ever submitting a valid answer.
    /// A ledger that already exited keeps its original reason.
    pub fn mark_stuck(&mut self) -> ExitReason {
        let reason = self.exit_reason.get_or_insert(ExitReason::Stuck);
        *reason
    }

    /// Clears the free-retry tallies at the start of a review round.
    ///
    /// Allowances are per round, not per phase. Sharing one allowance across a
    /// whole phase would mean the second reviewer to hit a transient failure
    /// gets dropped without a retry because the first one used it.
    pub fn reset_round_retries(&mut self) {
        self.retries = RetryTally::default();
    }

    /// Resets the phase after the human sent it back with comments.
    ///
    /// The human injecting new information is not the models spinning, so it
    /// earns a full fresh budget. `human_iterations` is recorded but never
    /// capped — the user self-regulates.
    pub fn record_human_rejection(&mut self) {
        self.cycles_used = 0;
        self.consecutive_revisions_without_review = 0;
        self.retries = RetryTally::default();
        self.new_blocking_history.clear();
        self.exit_reason = None;
        self.human_iterations = self.human_iterations.saturating_add(1);
    }

    fn evaluate(&self) -> Option<ExitReason> {
        if self.consecutive_revisions_without_review > MAX_CONSECUTIVE_REVISIONS_WITHOUT_REVIEW {
            return Some(ExitReason::Stuck);
        }

        let history = &self.new_blocking_history;
        let last = *history.last()?;

        // Nothing new: every blocking finding is either accepted or argued down.
        if last == 0 {
            return Some(ExitReason::Converged);
        }

        // Later rounds are scoped to the diff and the disputed set, so they
        // should shrink. Holding steady or growing means the reviewers are
        // churning and further cycles would spend budget without converging.
        if history.len() >= 2 && last >= history[history.len() - 2] {
            return Some(ExitReason::Churn);
        }

        if self.cycles_used >= self.config.max_cycles {
            return Some(ExitReason::CycleCap);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::FreeRetryConfig;

    use super::*;

    fn ledger(max_cycles: u32) -> CycleLedger {
        CycleLedger::new(CycleBudgetConfig {
            max_cycles,
            free_retries: FreeRetryConfig::default(),
        })
    }

    #[test]
    fn a_fresh_ledger_has_budget_and_no_exit() {
        let ledger = ledger(2);

        assert!(ledger.has_budget());
        assert!(!ledger.is_terminal());
        assert_eq!(ledger.exit_reason(), None);
        assert_eq!(ledger.cycles_used(), 0);
    }

    #[test]
    fn no_new_blocking_converges() {
        let mut ledger = ledger(2);

        assert_eq!(ledger.record_cycle(0), Some(ExitReason::Converged));
        assert_eq!(ledger.cycles_used(), 1);
        assert!(ledger.exit_reason().unwrap().is_clean());
    }

    #[test]
    fn a_shrinking_round_keeps_going() {
        let mut ledger = ledger(3);

        assert_eq!(ledger.record_cycle(4), None);
        assert_eq!(ledger.record_cycle(2), None);
        assert!(ledger.has_budget());
    }

    #[test]
    fn churn_needs_a_previous_round_to_compare_against() {
        let mut ledger = ledger(3);

        // A large first round is not churn — there is nothing to compare to.
        assert_eq!(ledger.record_cycle(9), None);
    }

    #[test]
    fn a_round_that_stops_shrinking_is_churn() {
        let mut ledger = ledger(5);
        ledger.record_cycle(3);

        assert_eq!(ledger.record_cycle(3), Some(ExitReason::Churn));
        // Stopped at 2 of 5 cycles rather than burning the rest.
        assert_eq!(ledger.cycles_used(), 2);
    }

    #[test]
    fn a_growing_round_is_churn() {
        let mut ledger = ledger(5);
        ledger.record_cycle(2);

        assert_eq!(ledger.record_cycle(6), Some(ExitReason::Churn));
    }

    #[test]
    fn exhausting_cycles_exits_at_the_cap() {
        let mut ledger = ledger(2);

        assert_eq!(ledger.record_cycle(5), None);
        assert_eq!(ledger.record_cycle(3), Some(ExitReason::CycleCap));
        assert!(!ledger.has_budget());
    }

    #[test]
    fn convergence_wins_over_the_cap() {
        let mut ledger = ledger(2);
        ledger.record_cycle(4);

        // Final cycle both hits the cap and resolves everything.
        assert_eq!(ledger.record_cycle(0), Some(ExitReason::Converged));
    }

    #[test]
    fn free_retries_never_consume_cycle_budget() {
        let mut ledger = ledger(2);

        for kind in [
            RetryKind::InvalidOutput,
            RetryKind::AgentFailure,
            RetryKind::Disposition,
        ] {
            assert!(ledger.record_free_retry(kind).is_allowed());
        }

        assert_eq!(ledger.cycles_used(), 0);
        assert!(!ledger.is_terminal());
        assert!(ledger.has_budget());
    }

    #[test]
    fn each_retry_kind_has_its_own_allowance() {
        let mut ledger = ledger(2);

        // Defaults: invalid_output 1, agent_failure 2, disposition 1.
        assert!(ledger
            .record_free_retry(RetryKind::InvalidOutput)
            .is_allowed());
        assert_eq!(
            ledger.record_free_retry(RetryKind::InvalidOutput),
            RetryOutcome::Exhausted { limit: 1 }
        );

        assert!(ledger
            .record_free_retry(RetryKind::AgentFailure)
            .is_allowed());
        assert!(ledger
            .record_free_retry(RetryKind::AgentFailure)
            .is_allowed());
        assert_eq!(
            ledger.record_free_retry(RetryKind::AgentFailure),
            RetryOutcome::Exhausted { limit: 2 }
        );

        assert_eq!(ledger.retries().invalid_output, 1);
        assert_eq!(ledger.retries().agent_failure, 2);
        assert_eq!(ledger.retries().disposition, 0);
    }

    #[test]
    fn one_revision_without_review_is_tolerated() {
        let mut ledger = ledger(2);

        assert_eq!(ledger.record_revision_without_review(), None);
    }

    #[test]
    fn repeated_revisions_without_review_are_stuck() {
        let mut ledger = ledger(2);
        ledger.record_revision_without_review();

        assert_eq!(
            ledger.record_revision_without_review(),
            Some(ExitReason::Stuck)
        );
    }

    #[test]
    fn a_completed_cycle_clears_the_stuck_counter() {
        let mut ledger = ledger(3);
        ledger.record_revision_without_review();
        ledger.record_cycle(4);

        assert_eq!(ledger.record_revision_without_review(), None);
    }

    #[test]
    fn a_terminal_ledger_stops_advancing() {
        let mut ledger = ledger(5);
        ledger.record_cycle(0);

        assert_eq!(ledger.record_cycle(7), Some(ExitReason::Converged));
        assert_eq!(ledger.cycles_used(), 1);
        assert_eq!(ledger.new_blocking_history(), &[0]);
    }

    #[test]
    fn human_rejection_restores_a_full_budget() {
        let mut ledger = ledger(2);
        ledger.record_free_retry(RetryKind::InvalidOutput);
        ledger.record_cycle(5);
        ledger.record_cycle(3);
        assert_eq!(ledger.exit_reason(), Some(ExitReason::CycleCap));

        ledger.record_human_rejection();

        assert_eq!(ledger.cycles_used(), 0);
        assert_eq!(ledger.human_iterations(), 1);
        assert_eq!(ledger.exit_reason(), None);
        assert_eq!(ledger.retries(), RetryTally::default());
        assert!(ledger.new_blocking_history().is_empty());
        assert!(ledger.has_budget());
    }

    #[test]
    fn ledger_round_trips_through_json() {
        let mut ledger = ledger(2);
        ledger.record_free_retry(RetryKind::AgentFailure);
        ledger.record_cycle(4);

        let json = serde_json::to_string(&ledger).unwrap();
        let decoded: CycleLedger = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, ledger);
    }
}
