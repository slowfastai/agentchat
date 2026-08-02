//! Owns runs on behalf of a user interface.
//!
//! The CLI drives a run by blocking on it. A console cannot: it needs the run
//! to advance in the background while it polls for progress, and it needs the
//! approval gate to park until someone clicks a button rather than until a file
//! appears on disk.
//!
//! Everything a UI needs is therefore mirrored here — status, an activity log
//! with monotonic sequence numbers so a client can ask for "everything after
//! N", and the pending approval packet. The supervisor holds no locks across
//! await points; it is borrowed briefly, mutated, and released.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use agentchat_protocol::now_millis;
use agentchat_protocol::run::{
    ApprovalDecision, ApprovalPacket, ExitReason, PhaseKind, RunStatus, StageKind,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::run::gate::ApprovalGate;
use crate::run::progress::{EventFormatter, ProgressSink, RunEvent};
use crate::run::state::RunState;

/// One line of a run's activity log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    /// Monotonic within a run, so a client can poll for what it has not seen.
    pub seq: u64,
    pub at_ms: u64,
    pub line: String,
}

/// What a UI shows for one run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunView {
    pub run_id: String,
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<PhaseKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<StageKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<ExitReason>,
    pub version: u32,
    pub round: u32,
    pub cycles_used: u32,
    pub updated_at_ms: u64,
    /// Set while the run is parked waiting for a human decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<ApprovalPacket>,
    /// Set when the run stopped because something went wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// No task is driving this run any more.
    pub finished: bool,
}

impl RunView {
    fn from_state(run: &RunState) -> Self {
        let phase = run.active_phase();
        Self {
            run_id: run.run_id.clone(),
            status: run.status,
            phase: phase.map(|phase| phase.kind),
            stage: phase.map(|phase| phase.stage),
            exit_reason: phase.and_then(|phase| phase.exit_reason()),
            version: phase.map(|phase| phase.version).unwrap_or(0),
            round: phase.map(|phase| phase.round).unwrap_or(0),
            cycles_used: phase.map(|phase| phase.ledger.cycles_used()).unwrap_or(0),
            updated_at_ms: run.updated_at_ms,
            pending: None,
            error: None,
            finished: run.status.is_terminal(),
        }
    }
}

/// Registry of runs a UI can watch and answer.
#[derive(Default)]
pub struct RunSupervisor {
    runs: HashMap<String, RunView>,
    logs: HashMap<String, Vec<LogEntry>>,
    next_seq: HashMap<String, u64>,
    /// Where to deliver a decision for a run currently parked at its gate.
    gates: HashMap<String, oneshot::Sender<ApprovalDecision>>,
    /// Newest first.
    order: Vec<String>,
}

/// How many log lines to keep per run before dropping the oldest.
pub const MAX_LOG_LINES: usize = 5_000;

impl RunSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, run_id: &str, view: RunView) {
        if !self.runs.contains_key(run_id) {
            self.order.insert(0, run_id.to_string());
        }
        self.runs.insert(run_id.to_string(), view);
    }

    /// Refreshes the mirrored state, keeping UI-only fields.
    pub fn sync(&mut self, run: &RunState) {
        let mut view = RunView::from_state(run);
        if let Some(existing) = self.runs.get(&run.run_id) {
            view.pending = existing.pending.clone();
            view.error = existing.error.clone();
            view.finished = existing.finished || view.finished;
        }
        self.register(&run.run_id, view);
    }

    pub fn view(&self, run_id: &str) -> Option<&RunView> {
        self.runs.get(run_id)
    }

    /// Every run, newest first.
    pub fn list(&self) -> Vec<RunView> {
        self.order
            .iter()
            .filter_map(|run_id| self.runs.get(run_id))
            .cloned()
            .collect()
    }

    pub fn append_log(&mut self, run_id: &str, line: String) -> u64 {
        let seq = self.next_seq.entry(run_id.to_string()).or_insert(0);
        *seq += 1;
        let entry = LogEntry {
            seq: *seq,
            at_ms: now_millis(),
            line,
        };
        let seq = entry.seq;

        let log = self.logs.entry(run_id.to_string()).or_default();
        log.push(entry);
        if log.len() > MAX_LOG_LINES {
            log.drain(..log.len() - MAX_LOG_LINES);
        }
        seq
    }

    /// Log lines a client has not seen yet.
    pub fn log_after(&self, run_id: &str, after: u64) -> Vec<LogEntry> {
        self.logs
            .get(run_id)
            .map(|log| {
                log.iter()
                    .filter(|entry| entry.seq > after)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn park(
        &mut self,
        run_id: &str,
        packet: ApprovalPacket,
        decision_tx: oneshot::Sender<ApprovalDecision>,
    ) {
        if let Some(view) = self.runs.get_mut(run_id) {
            view.pending = Some(packet);
            view.updated_at_ms = now_millis();
        }
        self.gates.insert(run_id.to_string(), decision_tx);
    }

    fn unpark(&mut self, run_id: &str) {
        if let Some(view) = self.runs.get_mut(run_id) {
            view.pending = None;
        }
        self.gates.remove(run_id);
    }

    /// Delivers a decision to a run parked at its gate.
    pub fn decide(&mut self, run_id: &str, decision: ApprovalDecision) -> Result<(), String> {
        let Some(sender) = self.gates.remove(run_id) else {
            return Err(format!("run {run_id} is not waiting for a decision"));
        };
        if let Some(view) = self.runs.get_mut(run_id) {
            view.pending = None;
            view.updated_at_ms = now_millis();
        }
        sender
            .send(decision)
            .map_err(|_| format!("run {run_id} stopped before the decision arrived"))
    }

    /// Records that a run stopped, successfully or not.
    pub fn finish(&mut self, run_id: &str, error: Option<String>) {
        if let Some(view) = self.runs.get_mut(run_id) {
            view.finished = true;
            view.pending = None;
            view.error = error;
            view.updated_at_ms = now_millis();
        }
        self.gates.remove(run_id);
    }

    pub fn is_running(&self, run_id: &str) -> bool {
        self.runs
            .get(run_id)
            .map(|view| !view.finished)
            .unwrap_or(false)
    }
}

/// Shared handle a driving task and a UI both hold.
pub type SharedSupervisor = Rc<RefCell<RunSupervisor>>;

/// Feeds a run's activity into the supervisor's log.
pub struct SupervisorProgress {
    supervisor: SharedSupervisor,
    run_id: String,
    formatter: EventFormatter,
}

impl SupervisorProgress {
    pub fn new(supervisor: SharedSupervisor, run_id: impl Into<String>) -> Self {
        Self {
            supervisor,
            run_id: run_id.into(),
            formatter: EventFormatter::new(),
        }
    }
}

impl ProgressSink for SupervisorProgress {
    fn emit(&self, event: RunEvent<'_>) {
        if let Some(line) = self.formatter.format(&event) {
            self.supervisor
                .borrow_mut()
                .append_log(&self.run_id, line.trim_end().to_string());
        }
    }
}

/// Parks a phase until a UI answers.
pub struct SupervisorGate {
    supervisor: SharedSupervisor,
    run_id: String,
}

impl SupervisorGate {
    pub fn new(supervisor: SharedSupervisor, run_id: impl Into<String>) -> Self {
        Self {
            supervisor,
            run_id: run_id.into(),
        }
    }
}

#[async_trait(?Send)]
impl ApprovalGate for SupervisorGate {
    async fn request(&self, packet: &ApprovalPacket) -> Result<ApprovalDecision, String> {
        let (decision_tx, decision_rx) = oneshot::channel();
        {
            let mut supervisor = self.supervisor.borrow_mut();
            supervisor.append_log(
                &self.run_id,
                format!(
                    "⏸ {} ready for review — {} dispute(s), {} follow-up(s)",
                    packet.phase.as_str(),
                    packet.disputes.len(),
                    packet.followups.len()
                ),
            );
            supervisor.park(&self.run_id, packet.clone(), decision_tx);
        }

        let decision = decision_rx.await.map_err(|_| {
            format!(
                "run {} was dropped while waiting for a decision",
                self.run_id
            )
        })?;

        let mut supervisor = self.supervisor.borrow_mut();
        supervisor.unpark(&self.run_id);
        supervisor.append_log(
            &self.run_id,
            match &decision {
                ApprovalDecision::Approve => "▶ approved".to_string(),
                ApprovalDecision::RequestChanges { .. } => {
                    "↩ changes requested — budget reset".to_string()
                }
                ApprovalDecision::Cancel => "■ cancelled".to_string(),
            },
        );
        Ok(decision)
    }
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{CycleBudgetConfig, DiscussionSummary};

    use super::*;

    fn packet(run_id: &str) -> ApprovalPacket {
        ApprovalPacket {
            run_id: run_id.into(),
            phase: PhaseKind::Plan,
            version: 2,
            exit_reason: Some(ExitReason::Converged),
            disputes: Vec::new(),
            summary: DiscussionSummary::default(),
            followups: Vec::new(),
            archive: Vec::new(),
        }
    }

    fn supervisor_with_run(run_id: &str) -> SharedSupervisor {
        let supervisor = Rc::new(RefCell::new(RunSupervisor::new()));
        let run = RunState::new(run_id, "/tmp/worktree");
        supervisor.borrow_mut().sync(&run);
        supervisor
    }

    #[test]
    fn log_sequence_numbers_let_a_client_ask_for_what_it_missed() {
        let mut supervisor = RunSupervisor::new();

        assert_eq!(supervisor.append_log("run-1", "first".into()), 1);
        assert_eq!(supervisor.append_log("run-1", "second".into()), 2);
        // Sequences are per run.
        assert_eq!(supervisor.append_log("run-2", "other".into()), 1);

        let missed = supervisor.log_after("run-1", 1);
        assert_eq!(missed.len(), 1);
        assert_eq!(missed[0].line, "second");
        assert!(supervisor.log_after("run-1", 2).is_empty());
    }

    #[test]
    fn the_log_is_capped_but_keeps_the_newest() {
        let mut supervisor = RunSupervisor::new();
        for i in 0..MAX_LOG_LINES + 10 {
            supervisor.append_log("run-1", format!("line {i}"));
        }

        let all = supervisor.log_after("run-1", 0);

        assert_eq!(all.len(), MAX_LOG_LINES);
        assert_eq!(
            all.last().unwrap().line,
            format!("line {}", MAX_LOG_LINES + 9)
        );
    }

    #[test]
    fn runs_are_listed_newest_first() {
        let mut supervisor = RunSupervisor::new();
        supervisor.sync(&RunState::new("run-1", "/tmp/a"));
        supervisor.sync(&RunState::new("run-2", "/tmp/b"));

        let listed: Vec<String> = supervisor
            .list()
            .into_iter()
            .map(|view| view.run_id)
            .collect();

        assert_eq!(listed, vec!["run-2".to_string(), "run-1".to_string()]);
    }

    #[test]
    fn syncing_keeps_the_pending_packet() {
        let mut supervisor = RunSupervisor::new();
        let mut run = RunState::new("run-1", "/tmp/a");
        supervisor.sync(&run);
        let (tx, _rx) = oneshot::channel();
        supervisor.park("run-1", packet("run-1"), tx);

        run.plan.record_draft().unwrap();
        supervisor.sync(&run);

        assert!(
            supervisor.view("run-1").unwrap().pending.is_some(),
            "a state refresh must not drop the question being asked"
        );
    }

    #[test]
    fn the_view_reflects_the_active_phase() {
        let mut supervisor = RunSupervisor::new();
        let mut run = RunState::new("run-1", "/tmp/a");
        run.plan.record_draft().unwrap();

        supervisor.sync(&run);

        let view = supervisor.view("run-1").unwrap();
        assert_eq!(view.phase, Some(PhaseKind::Plan));
        assert_eq!(view.stage, Some(StageKind::Reviewing));
        assert_eq!(view.version, 1);
        assert!(!view.finished);
    }

    #[test]
    fn deciding_for_a_run_that_is_not_parked_is_an_error() {
        let mut supervisor = RunSupervisor::new();
        supervisor.sync(&RunState::new("run-1", "/tmp/a"));

        let error = supervisor
            .decide("run-1", ApprovalDecision::Approve)
            .unwrap_err();

        assert!(error.contains("not waiting"));
    }

    #[test]
    fn finishing_clears_the_gate_and_records_the_error() {
        let mut supervisor = RunSupervisor::new();
        supervisor.sync(&RunState::new("run-1", "/tmp/a"));
        let (tx, _rx) = oneshot::channel();
        supervisor.park("run-1", packet("run-1"), tx);

        supervisor.finish("run-1", Some("agent died".into()));

        let view = supervisor.view("run-1").unwrap();
        assert!(view.finished);
        assert!(view.pending.is_none());
        assert_eq!(view.error.as_deref(), Some("agent died"));
        assert!(supervisor
            .decide("run-1", ApprovalDecision::Approve)
            .is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_gate_parks_until_a_decision_arrives() {
        let supervisor = supervisor_with_run("run-1");
        let gate = SupervisorGate::new(supervisor.clone(), "run-1");
        let packet = packet("run-1");

        let waiting = gate.request(&packet);
        tokio::pin!(waiting);

        // Nothing resolves while no one has answered.
        tokio::select! {
            _ = &mut waiting => panic!("gate resolved without a decision"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
        }
        assert!(supervisor.borrow().view("run-1").unwrap().pending.is_some());

        supervisor
            .borrow_mut()
            .decide("run-1", ApprovalDecision::Approve)
            .unwrap();

        assert_eq!(waiting.await.unwrap(), ApprovalDecision::Approve);
        assert!(supervisor.borrow().view("run-1").unwrap().pending.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_changes_carries_comments_through_the_gate() {
        let supervisor = supervisor_with_run("run-1");
        let gate = SupervisorGate::new(supervisor.clone(), "run-1");
        let packet = packet("run-1");

        let waiting = gate.request(&packet);
        tokio::pin!(waiting);
        tokio::select! {
            _ = &mut waiting => panic!("gate resolved early"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
        }

        supervisor
            .borrow_mut()
            .decide(
                "run-1",
                ApprovalDecision::RequestChanges {
                    comments: "reuse the store".into(),
                },
            )
            .unwrap();

        assert_eq!(
            waiting.await.unwrap(),
            ApprovalDecision::RequestChanges {
                comments: "reuse the store".into()
            }
        );
        let log = supervisor.borrow().log_after("run-1", 0);
        assert!(log
            .iter()
            .any(|entry| entry.line.contains("changes requested")));
    }

    #[test]
    fn progress_lines_land_in_the_run_log() {
        let supervisor = supervisor_with_run("run-1");
        let progress = SupervisorProgress::new(supervisor.clone(), "run-1");

        progress.emit(RunEvent::Stage {
            phase: PhaseKind::Plan,
            stage: StageKind::Reviewing,
            round: 1,
            roles: "opus, luna",
        });
        progress.emit(RunEvent::ReviewerFinished {
            role: "opus",
            blocking: 2,
            advisory: 3,
        });

        let log = supervisor.borrow().log_after("run-1", 0);
        assert_eq!(log.len(), 2);
        assert!(log[0].line.contains("reviewing"));
        assert!(log[1].line.contains("2 blocking"));
    }

    #[test]
    fn a_repeated_tool_call_does_not_fill_the_log() {
        let supervisor = supervisor_with_run("run-1");
        let progress = SupervisorProgress::new(supervisor.clone(), "run-1");
        let tool = |status| RunEvent::Tool {
            role: "opus",
            tool_call_id: "call-1",
            title: "Read src/lib.rs",
            status,
        };

        progress.emit(tool("pending"));
        progress.emit(tool("in_progress"));
        progress.emit(tool("completed"));

        assert_eq!(supervisor.borrow().log_after("run-1", 0).len(), 1);
    }

    #[test]
    fn budget_defaults_are_visible_to_a_ui() {
        // The console shows cycles used against the configured cap, so the cap
        // has to be reachable from the run rather than hardcoded in the UI.
        let run = RunState::new("run-1", "/tmp/a");

        assert_eq!(
            run.plan.ledger.config().max_cycles,
            CycleBudgetConfig::default().max_cycles
        );
    }
}
