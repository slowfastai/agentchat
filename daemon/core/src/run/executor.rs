//! Drives one phase's stages against real agents.
//!
//! The executor owns the only part of the loop that touches the outside world:
//! it renders a prompt, hands it to an agent, waits for the turn to end, then
//! reads the file the agent was told to write. Agents never return structured
//! data in their reply stream — Codex, opencode, and Claude Code all frame
//! streaming output differently, and asking for JSON in prose is the least
//! reliable part of any such pipeline. A file either parses or it does not.
//!
//! Failures are separated by whether they moved the work forward. A reviewer
//! that writes unparseable JSON, or an agent process that dies, draws on a
//! free-retry allowance and leaves the cycle budget alone.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use agentchat_protocol::run::{
    DispositionSet, Finding, PhaseKind, RawReviewReport, RetryKind, StageKind,
};
use tokio::sync::mpsc::UnboundedReceiver;
use tracing::{debug, warn};

use crate::backend::{AgentBackend, AgentNotification, AgentUpdate};
use crate::run::budget::RetryOutcome;
use crate::run::findings::{blocking_findings, validate_report};
use crate::run::layout::RunLayout;
use crate::run::progress::{ProgressSink, RunEvent, SilentProgress};
use crate::run::prompts::{PromptKind, PromptSet};
use crate::run::state::{DispositionOutcome, PhaseState, RoundSummary, TransitionError};

/// How long to keep draining an agent's update stream after its turn ends,
/// before concluding the tail has arrived. Negligible next to a turn that takes
/// minutes.
const TURN_TAIL_DRAIN: std::time::Duration = std::time::Duration::from_millis(100);

/// An agent bound to a role for this run.
pub struct RoleAgent {
    /// Stable name used for the reviewer's output filename and in findings.
    pub name: String,
    pub backend: Rc<dyn AgentBackend>,
}

impl RoleAgent {
    pub fn new(name: impl Into<String>, backend: Rc<dyn AgentBackend>) -> Self {
        Self {
            name: name.into(),
            backend,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorError {
    /// The phase was not in the stage this call advances from.
    Transition(TransitionError),
    /// The agent process failed and its retry allowance is spent.
    Agent {
        role: String,
        message: String,
    },
    /// The agent finished its turn without writing the file it was told to.
    MissingOutput {
        role: String,
        path: PathBuf,
    },
    /// Every reviewer in the round failed; there is nothing to disposition.
    NoReviewersSurvived,
    Io(String),
}

impl From<TransitionError> for ExecutorError {
    fn from(error: TransitionError) -> Self {
        Self::Transition(error)
    }
}

impl std::fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transition(error) => write!(f, "{error}"),
            Self::Agent { role, message } => write!(f, "agent {role} failed: {message}"),
            Self::MissingOutput { role, path } => {
                write!(f, "{role} finished without writing {}", path.display())
            }
            Self::NoReviewersSurvived => write!(f, "no reviewer produced a usable report"),
            Self::Io(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// Renders prompts, runs stages, and reads back what agents wrote.
pub struct StageExecutor {
    working_dir: PathBuf,
    layout: RunLayout,
    prompts: PromptSet,
    /// One session per role, reused across rounds so an agent keeps the context
    /// of what it already said.
    sessions: RefCell<HashMap<String, String>>,
    progress: Rc<dyn ProgressSink>,
    /// Update streams, checked out for the duration of a prompt so live agent
    /// activity can be reported instead of discarded.
    receivers: RefCell<HashMap<String, UnboundedReceiver<AgentNotification>>>,
    /// Roles whose stream we already tried to claim. A backend hands its
    /// receiver out once, so a second attempt would silently report nothing.
    receiver_claimed: RefCell<HashSet<String>>,
}

impl StageExecutor {
    pub fn new(working_dir: impl Into<PathBuf>, layout: RunLayout, prompts: PromptSet) -> Self {
        Self {
            working_dir: working_dir.into(),
            layout,
            prompts,
            sessions: RefCell::new(HashMap::new()),
            progress: Rc::new(SilentProgress),
            receivers: RefCell::new(HashMap::new()),
            receiver_claimed: RefCell::new(HashSet::new()),
        }
    }

    /// Reports live activity somewhere the operator can see it.
    pub fn set_progress(&mut self, progress: Rc<dyn ProgressSink>) {
        self.progress = progress;
    }

    pub fn layout(&self) -> &RunLayout {
        &self.layout
    }

    /// Has the author write the first version of the artifact.
    pub async fn execute_author(
        &self,
        phase: &mut PhaseState,
        author: &RoleAgent,
    ) -> Result<u32, ExecutorError> {
        let artifact = self.layout.artifact(phase.kind);
        self.progress.emit(RunEvent::Stage {
            phase: phase.kind,
            stage: StageKind::Authoring,
            round: phase.round.max(1),
            roles: &author.name,
        });

        let mut vars = self.base_vars(phase.kind);
        vars.push(("round".into(), phase.round.max(1).to_string()));

        let prompt = self
            .prompts
            .render(phase.kind, PromptKind::Author, &borrow(&vars));
        self.prompt_until_written(phase, author, &prompt, &artifact, PromptKind::Author)
            .await?;

        Ok(phase.record_draft()?)
    }

    /// Fans reviewers out over the current version and records the round.
    ///
    /// Reviewers run concurrently. One that keeps failing is dropped and the
    /// round proceeds on the survivors: a round with two opinions is worth far
    /// more than a stalled run.
    pub async fn execute_review_round(
        &self,
        phase: &mut PhaseState,
        reviewers: &[RoleAgent],
    ) -> Result<RoundSummary, ExecutorError> {
        let round = phase.round;
        let kind = phase.kind;
        self.layout
            .prepare_round(kind, round)
            .await
            .map_err(ExecutorError::Io)?;
        phase.ledger.reset_round_retries();

        let names = reviewers
            .iter()
            .map(|reviewer| reviewer.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.progress.emit(RunEvent::Stage {
            phase: kind,
            stage: StageKind::Reviewing,
            round,
            roles: &names,
        });

        let previous = self.render_previous_findings(phase);
        let results = futures::future::join_all(
            reviewers
                .iter()
                .map(|reviewer| self.run_one_reviewer(kind, round, reviewer, &previous)),
        )
        .await;

        let mut findings = Vec::new();
        let mut survivors = 0usize;
        for (reviewer, result) in reviewers.iter().zip(results) {
            match result {
                Ok(report) => {
                    survivors += 1;
                    let validated = validate_report(&report);
                    self.progress.emit(RunEvent::ReviewerFinished {
                        role: &reviewer.name,
                        blocking: validated.iter().filter(|f| f.is_blocking()).count(),
                        advisory: validated.iter().filter(|f| !f.is_blocking()).count(),
                    });
                    findings.extend(validated);
                }
                Err(error) => {
                    // Charge the round's allowance and drop the reviewer.
                    let outcome = phase.ledger.record_free_retry(retry_kind_for(&error));
                    let reason = error.to_string();
                    warn!(
                        "dropping reviewer {} from round {round}: {reason} ({outcome:?})",
                        reviewer.name
                    );
                    self.progress.emit(RunEvent::ReviewerDropped {
                        role: &reviewer.name,
                        reason: &reason,
                    });
                }
            }
        }

        if survivors == 0 {
            return Err(ExecutorError::NoReviewersSurvived);
        }

        self.append_findings(kind, &findings).await?;
        let summary = phase.record_review_round(findings)?;
        self.progress.emit(RunEvent::Round {
            round: summary.round,
            new_blocking: summary.new_blocking,
            total_blocking: summary.blocking_total,
        });
        Ok(summary)
    }

    /// Has the author answer the round and revise.
    ///
    /// A submission that fails the gate is fed back verbatim and retried within
    /// the round's allowance. When that runs out the phase is stuck, which ends
    /// at the same human gate as any other exit.
    pub async fn execute_disposition(
        &self,
        phase: &mut PhaseState,
        author: &RoleAgent,
    ) -> Result<DispositionOutcome, ExecutorError> {
        let kind = phase.kind;
        let round = phase.round;
        let output = self.layout.dispositions(kind, round);
        let findings_text = render_findings_for_author(phase.current_round_findings());
        let mut feedback = String::new();

        self.progress.emit(RunEvent::Stage {
            phase: kind,
            stage: StageKind::Dispositioning,
            round,
            roles: &author.name,
        });

        loop {
            let mut vars = self.base_vars(kind);
            vars.push(("round".into(), round.to_string()));
            vars.push(("findings".into(), findings_text.clone()));
            vars.push(("feedback".into(), feedback.clone()));
            vars.push((
                "output_path".into(),
                self.display(&output).display().to_string(),
            ));

            let prompt = self
                .prompts
                .render(kind, PromptKind::Disposition, &borrow(&vars));
            self.prompt_agent(author, &prompt).await?;

            let dispositions = match self.read_dispositions(&output).await {
                Ok(set) => set.dispositions,
                Err(error) => {
                    if !self.retry_allowed(phase, RetryKind::InvalidOutput) {
                        return Err(error);
                    }
                    feedback = format!("Your previous file could not be read: {error}\n");
                    continue;
                }
            };

            match phase.record_disposition(dispositions)? {
                DispositionOutcome::Rejected(rejection) => {
                    if !self.retry_allowed(phase, RetryKind::Disposition) {
                        // Out of retries: the author keeps answering, never
                        // validly. Close the phase at the human gate.
                        phase.record_stuck();
                        return Ok(DispositionOutcome::Rejected(rejection));
                    }
                    self.progress.emit(RunEvent::Retry {
                        role: &author.name,
                        kind: "disposition",
                        reason: rejection.feedback().trim(),
                    });
                    feedback = format!(
                        "Your previous answers were rejected. Fix exactly this:\n{}",
                        rejection.feedback()
                    );
                }
                outcome => {
                    self.write_followups(phase).await?;
                    return Ok(outcome);
                }
            }
        }
    }

    async fn run_one_reviewer(
        &self,
        phase: PhaseKind,
        round: u32,
        reviewer: &RoleAgent,
        previous_findings: &str,
    ) -> Result<RawReviewReport, ExecutorError> {
        let output = self.layout.review(phase, round, &reviewer.name);
        let mut vars = self.base_vars(phase);
        vars.push(("round".into(), round.to_string()));
        vars.push(("reviewer".into(), reviewer.name.clone()));
        vars.push(("previous_findings".into(), previous_findings.to_string()));
        vars.push((
            "output_path".into(),
            self.display(&output).display().to_string(),
        ));

        let prompt = self
            .prompts
            .render(phase, PromptKind::Review, &borrow(&vars));
        self.prompt_agent(reviewer, &prompt).await?;

        let raw =
            tokio::fs::read_to_string(&output)
                .await
                .map_err(|_| ExecutorError::MissingOutput {
                    role: reviewer.name.clone(),
                    path: output.clone(),
                })?;

        let mut report: RawReviewReport =
            serde_json::from_str(&raw).map_err(|e| ExecutorError::Agent {
                role: reviewer.name.clone(),
                message: format!("review is not valid JSON: {e}"),
            })?;

        // Trust our own bookkeeping over whatever the agent typed.
        report.reviewer = reviewer.name.clone();
        report.round = round;
        Ok(report)
    }

    /// Prompts until the expected file exists, within the retry allowance.
    async fn prompt_until_written(
        &self,
        phase: &mut PhaseState,
        role: &RoleAgent,
        prompt: &str,
        expected: &std::path::Path,
        _kind: PromptKind,
    ) -> Result<(), ExecutorError> {
        loop {
            self.prompt_agent(role, prompt).await?;

            match tokio::fs::metadata(expected).await {
                Ok(meta) if meta.len() > 0 => return Ok(()),
                _ => {
                    if !self.retry_allowed(phase, RetryKind::InvalidOutput) {
                        return Err(ExecutorError::MissingOutput {
                            role: role.name.clone(),
                            path: expected.to_path_buf(),
                        });
                    }
                    let reason = format!("did not write {}", expected.display());
                    self.progress.emit(RunEvent::Retry {
                        role: &role.name,
                        kind: "missing output",
                        reason: &reason,
                    });
                    debug!(
                        "{} did not write {}; retrying",
                        role.name,
                        expected.display()
                    );
                }
            }
        }
    }

    /// Prompts an agent, reporting what it does while the turn runs.
    async fn prompt_agent(&self, role: &RoleAgent, prompt: &str) -> Result<(), ExecutorError> {
        let session_id = self.ensure_session(role).await?;
        let mut receiver = self.checkout_receiver(role);
        let mut text_chars = 0usize;
        let mut thinking_chars = 0usize;

        let result = {
            let prompt_future = role.backend.prompt(session_id, prompt.to_string());
            tokio::pin!(prompt_future);

            loop {
                let Some(stream) = receiver.as_mut() else {
                    break prompt_future.await;
                };

                tokio::select! {
                    // The turn ending wins over draining, so a chatty agent
                    // cannot delay the stage.
                    biased;
                    result = &mut prompt_future => break result,
                    update = stream.recv() => match update {
                        Some(update) => {
                            self.report(role, &update, &mut text_chars, &mut thinking_chars)
                        }
                        None => receiver = None,
                    },
                }
            }
        };

        // Whatever arrived as the turn was closing.
        //
        // A plain `try_recv` is not enough: the prompt future resolves when the
        // response arrives, but the notifications for that same turn are
        // forwarded by a separate task that may not have run yet. Draining with
        // a short idle timeout keeps each turn's activity attributed to the turn
        // that produced it instead of leaking into the next one.
        if let Some(stream) = receiver.as_mut() {
            while let Ok(Some(update)) = tokio::time::timeout(TURN_TAIL_DRAIN, stream.recv()).await
            {
                self.report(role, &update, &mut text_chars, &mut thinking_chars);
            }
        }
        if let Some(stream) = receiver {
            self.receivers
                .borrow_mut()
                .insert(role.name.clone(), stream);
        }

        if text_chars > 0 || thinking_chars > 0 {
            self.progress.emit(RunEvent::Turn {
                role: &role.name,
                text_chars,
                thinking_chars,
            });
        }

        result.map(|_| ()).map_err(|message| ExecutorError::Agent {
            role: role.name.clone(),
            message,
        })
    }

    fn report(
        &self,
        role: &RoleAgent,
        update: &AgentNotification,
        text_chars: &mut usize,
        thinking_chars: &mut usize,
    ) {
        match &update.update {
            AgentUpdate::TextDelta { content } => *text_chars += content.chars().count(),
            AgentUpdate::ThinkingDelta { content } => *thinking_chars += content.chars().count(),
            AgentUpdate::ToolUpdate {
                tool_call_id,
                title,
                status,
                ..
            } => self.progress.emit(RunEvent::Tool {
                role: &role.name,
                tool_call_id,
                title,
                status,
            }),
            _ => {}
        }
    }

    /// Borrows a role's update stream, claiming it from the backend on first use.
    fn checkout_receiver(&self, role: &RoleAgent) -> Option<UnboundedReceiver<AgentNotification>> {
        if let Some(stream) = self.receivers.borrow_mut().remove(&role.name) {
            return Some(stream);
        }
        if self.receiver_claimed.borrow_mut().insert(role.name.clone()) {
            return role.backend.take_update_rx();
        }
        None
    }

    async fn ensure_session(&self, role: &RoleAgent) -> Result<String, ExecutorError> {
        let existing = self.sessions.borrow().get(&role.name).cloned();
        if let Some(session_id) = existing {
            return Ok(session_id);
        }

        let session_id = role
            .backend
            .new_session(self.working_dir.clone())
            .await
            .map_err(|message| ExecutorError::Agent {
                role: role.name.clone(),
                message,
            })?;
        self.sessions
            .borrow_mut()
            .insert(role.name.clone(), session_id.clone());
        Ok(session_id)
    }

    fn retry_allowed(&self, phase: &mut PhaseState, kind: RetryKind) -> bool {
        matches!(
            phase.ledger.record_free_retry(kind),
            RetryOutcome::Allowed { .. }
        )
    }

    fn base_vars(&self, phase: PhaseKind) -> Vec<(String, String)> {
        vec![
            (
                "brief_path".into(),
                self.display(&self.layout.brief()).display().to_string(),
            ),
            (
                "artifact_path".into(),
                self.display(&self.layout.artifact(phase))
                    .display()
                    .to_string(),
            ),
            (
                "plan_path".into(),
                self.display(&self.layout.artifact(PhaseKind::Plan))
                    .display()
                    .to_string(),
            ),
        ]
    }

    fn display<'a>(&self, path: &'a std::path::Path) -> &'a std::path::Path {
        self.layout.display_path(path, &self.working_dir)
    }

    /// What reviewers are told about earlier rounds.
    ///
    /// Round one is deliberately blind: reviewers that cannot see each other
    /// produce genuinely independent findings, which is the whole reason to run
    /// more than one. From round two on they see everything, so they stop
    /// re-raising what is already settled.
    fn render_previous_findings(&self, phase: &PhaseState) -> String {
        if phase.round <= 1 || phase.findings.is_empty() {
            return String::new();
        }

        let mut out = String::from(
            "Earlier rounds already raised the following. Do not re-raise anything\n\
             the author accepted; for anything disputed, judge only whether the\n\
             author's argument holds.\n\n",
        );
        for finding in blocking_findings(&phase.findings) {
            out.push_str(&format!(
                "- [{}] {} · {} ({})\n",
                finding.finding_id,
                finding.file,
                finding.problem,
                finding.severity.category_str()
            ));
        }
        out
    }

    async fn read_dispositions(
        &self,
        path: &std::path::Path,
    ) -> Result<DispositionSet, ExecutorError> {
        let raw = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ExecutorError::Io(format!("cannot read {}: {e}", path.display())))?;
        serde_json::from_str(&raw)
            .map_err(|e| ExecutorError::Io(format!("{} is not valid JSON: {e}", path.display())))
    }

    async fn append_findings(
        &self,
        phase: PhaseKind,
        findings: &[Finding],
    ) -> Result<(), ExecutorError> {
        if findings.is_empty() {
            return Ok(());
        }

        let path = self.layout.findings(phase);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ExecutorError::Io(format!("cannot create {}: {e}", parent.display()))
            })?;
        }

        let mut body = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        for finding in findings {
            let line = serde_json::to_string(finding)
                .map_err(|e| ExecutorError::Io(format!("cannot serialize finding: {e}")))?;
            body.push_str(&line);
            body.push('\n');
        }

        tokio::fs::write(&path, body)
            .await
            .map_err(|e| ExecutorError::Io(format!("cannot write {}: {e}", path.display())))
    }

    /// Writes advisory findings nobody adopted, for the human to turn into
    /// Issues at the approval gate.
    async fn write_followups(&self, phase: &PhaseState) -> Result<(), ExecutorError> {
        let declined =
            crate::run::disposition::declined_advisory(&phase.findings, &phase.dispositions);
        if declined.is_empty() {
            return Ok(());
        }

        let mut body = String::from("# Follow-ups\n\nAdvisory findings nobody adopted.\n\n");
        for finding in declined {
            body.push_str(&format!(
                "- **{}** · `{}` — {} _(raised by {})_\n",
                finding.severity.category_str(),
                finding.file,
                finding.problem,
                finding.reviewer
            ));
        }

        let path = self.layout.followups();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ExecutorError::Io(format!("cannot create {}: {e}", parent.display()))
            })?;
        }
        tokio::fs::write(&path, body)
            .await
            .map_err(|e| ExecutorError::Io(format!("cannot write {}: {e}", path.display())))
    }
}

fn retry_kind_for(error: &ExecutorError) -> RetryKind {
    match error {
        ExecutorError::Agent { .. } => RetryKind::AgentFailure,
        _ => RetryKind::InvalidOutput,
    }
}

fn borrow(vars: &[(String, String)]) -> Vec<(&str, &str)> {
    vars.iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

fn render_findings_for_author(findings: &[Finding]) -> String {
    let blocking = blocking_findings(findings);
    if blocking.is_empty() {
        return "No blocking findings were raised this round.".into();
    }

    let mut out = String::from("Blocking findings you must answer:\n\n");
    for finding in blocking {
        out.push_str(&format!(
            "- `{}` [{}] `{}`\n  problem: {}\n  evidence: {}\n  suggested: {}\n",
            finding.finding_id,
            finding.severity.category_str(),
            finding.location,
            finding.problem,
            finding.evidence,
            finding.recommendation,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use agentchat_protocol::run::{
        CycleBudgetConfig, Disposition, DispositionAction, ExitReason, RawFinding,
    };
    use tokio::sync::{mpsc, watch};

    use super::*;
    use crate::backend::{AgentNotification, AgentPromptResult};

    const EVIDENCE: &str = "reaching stage_two with an empty plan panics in run/stage.rs";

    /// An agent that writes a scripted file each time it is prompted.
    struct ScriptedAgent {
        /// One entry per prompt: `Some((path, body))` writes, `None` writes
        /// nothing, simulating an agent that ignored its instructions.
        script: RefCell<Vec<Option<(PathBuf, String)>>>,
        fail_with: Option<String>,
        prompts: AtomicUsize,
        health: watch::Sender<bool>,
    }

    impl ScriptedAgent {
        fn new(script: Vec<Option<(PathBuf, String)>>) -> Rc<Self> {
            let (health, _) = watch::channel(true);
            Rc::new(Self {
                script: RefCell::new(script),
                fail_with: None,
                prompts: AtomicUsize::new(0),
                health,
            })
        }

        fn failing(message: &str) -> Rc<Self> {
            let (health, _) = watch::channel(true);
            Rc::new(Self {
                script: RefCell::new(Vec::new()),
                fail_with: Some(message.into()),
                prompts: AtomicUsize::new(0),
                health,
            })
        }
    }

    #[async_trait::async_trait(?Send)]
    impl AgentBackend for ScriptedAgent {
        async fn initialize(&self) -> Result<(), String> {
            Ok(())
        }

        async fn new_session(&self, _cwd: PathBuf) -> Result<String, String> {
            Ok("session-scripted".into())
        }

        async fn prompt(
            &self,
            _session: String,
            _text: String,
        ) -> Result<AgentPromptResult, String> {
            if let Some(message) = &self.fail_with {
                return Err(message.clone());
            }
            self.prompts.fetch_add(1, Ordering::Relaxed);

            let step = if self.script.borrow().is_empty() {
                None
            } else {
                self.script.borrow_mut().remove(0)
            };
            if let Some((path, body)) = step {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await.unwrap();
                }
                tokio::fs::write(&path, body).await.unwrap();
            }
            Ok(AgentPromptResult::new("end_turn"))
        }

        async fn cancel(&self, _session: String) -> Result<(), String> {
            Ok(())
        }

        fn take_update_rx(&self) -> Option<mpsc::UnboundedReceiver<AgentNotification>> {
            None
        }

        fn subscribe_health(&self) -> watch::Receiver<bool> {
            self.health.subscribe()
        }

        fn is_alive(&self) -> bool {
            true
        }

        async fn shutdown(&self) {}
    }

    struct Harness {
        _dir: tempfile::TempDir,
        working_dir: PathBuf,
        layout: RunLayout,
    }

    fn harness() -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let working_dir = dir.path().to_path_buf();
        let layout = RunLayout::new(&working_dir, "run-1");
        Harness {
            _dir: dir,
            working_dir,
            layout,
        }
    }

    fn executor(harness: &Harness) -> StageExecutor {
        StageExecutor::new(
            harness.working_dir.clone(),
            harness.layout.clone(),
            PromptSet::builtin(),
        )
    }

    fn phase() -> PhaseState {
        PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default())
    }

    fn review_json(reviewer: &str, blocking: &[&str], advisory: &[&str]) -> String {
        serde_json::to_string(&RawReviewReport {
            reviewer: reviewer.into(),
            round: 1,
            blocking: blocking
                .iter()
                .map(|slug| RawFinding {
                    category: "incorrect".into(),
                    location: format!("core/src/{slug}.rs"),
                    problem: format!("{slug} is wrong"),
                    evidence: EVIDENCE.into(),
                    recommendation: "fix it".into(),
                })
                .collect(),
            non_blocking: advisory
                .iter()
                .map(|slug| RawFinding {
                    category: "test_gap".into(),
                    location: format!("core/src/{slug}.rs"),
                    problem: format!("{slug} lacks a test"),
                    ..RawFinding::default()
                })
                .collect(),
        })
        .unwrap()
    }

    fn dispositions_json(findings: &[Finding], action: DispositionAction, reason: &str) -> String {
        serde_json::to_string(&DispositionSet {
            round: 1,
            dispositions: blocking_findings(findings)
                .iter()
                .map(|finding| Disposition {
                    finding_id: finding.finding_id.clone(),
                    action,
                    reason: reason.into(),
                    changed_files: Vec::new(),
                })
                .collect(),
        })
        .unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn the_author_stage_requires_the_artifact_to_appear() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        let agent = ScriptedAgent::new(vec![Some((
            harness.layout.artifact(PhaseKind::Plan),
            "# Plan\n".into(),
        ))]);
        let author = RoleAgent::new("planner", agent.clone());

        let version = executor.execute_author(&mut phase, &author).await.unwrap();

        assert_eq!(version, 1);
        assert_eq!(phase.stage.as_str(), "reviewing");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_author_that_writes_nothing_is_retried_then_reported() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        // Never writes the artifact.
        let agent = ScriptedAgent::new(vec![None, None, None, None]);
        let author = RoleAgent::new("planner", agent.clone());

        let error = executor
            .execute_author(&mut phase, &author)
            .await
            .unwrap_err();

        assert!(matches!(error, ExecutorError::MissingOutput { .. }));
        // One initial attempt plus the invalid_output allowance.
        assert_eq!(
            agent.prompts.load(Ordering::Relaxed),
            1 + CycleBudgetConfig::default().free_retries.invalid_output as usize
        );
        assert_eq!(phase.ledger.cycles_used(), 0, "no cycle was consumed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_review_round_collects_every_reviewer() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        let reviewers: Vec<RoleAgent> = ["opus", "deepseek"]
            .iter()
            .map(|name| {
                let path = harness.layout.review(PhaseKind::Plan, 1, name);
                RoleAgent::new(
                    *name,
                    ScriptedAgent::new(vec![Some((
                        path,
                        review_json(name, &["alpha"], &["beta"]),
                    ))]),
                )
            })
            .collect();

        let summary = executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap();

        // Both reviewers named the same problem, so it counts once.
        assert_eq!(summary.new_blocking, 1);
        assert_eq!(summary.blocking_total, 2);
        assert!(harness.layout.findings(PhaseKind::Plan).is_file());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_broken_reviewer_is_dropped_and_the_round_continues() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        let good_path = harness.layout.review(PhaseKind::Plan, 1, "opus");
        let bad_path = harness.layout.review(PhaseKind::Plan, 1, "deepseek");
        let reviewers = vec![
            RoleAgent::new(
                "opus",
                ScriptedAgent::new(vec![Some((
                    good_path,
                    review_json("opus", &["alpha"], &[]),
                ))]),
            ),
            RoleAgent::new(
                "deepseek",
                ScriptedAgent::new(vec![Some((bad_path, "{ not json".into()))]),
            ),
        ];

        let summary = executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap();

        assert_eq!(summary.blocking_total, 1, "only the good reviewer counted");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_round_where_every_reviewer_fails_is_an_error() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        let reviewers = vec![RoleAgent::new(
            "opus",
            ScriptedAgent::failing("spawn failed"),
        )];

        let error = executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap_err();

        assert_eq!(error, ExecutorError::NoReviewersSurvived);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_full_cycle_advances_to_the_next_round() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        let review_path = harness.layout.review(PhaseKind::Plan, 1, "opus");
        let reviewers = vec![RoleAgent::new(
            "opus",
            ScriptedAgent::new(vec![Some((
                review_path,
                review_json("opus", &["alpha"], &["beta"]),
            ))]),
        )];
        executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap();

        let answers = dispositions_json(
            phase.current_round_findings(),
            DispositionAction::Accepted,
            "",
        );
        let author = RoleAgent::new(
            "planner",
            ScriptedAgent::new(vec![Some((
                harness.layout.dispositions(PhaseKind::Plan, 1),
                answers,
            ))]),
        );

        let outcome = executor
            .execute_disposition(&mut phase, &author)
            .await
            .unwrap();

        assert_eq!(
            outcome,
            DispositionOutcome::NextRound {
                round: 2,
                version: 2
            }
        );
        // The advisory finding nobody adopted became a follow-up.
        let followups = tokio::fs::read_to_string(harness.layout.followups())
            .await
            .unwrap();
        assert!(followups.contains("beta"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_author_that_ignores_the_gate_is_fed_back_then_stuck() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        let review_path = harness.layout.review(PhaseKind::Plan, 1, "opus");
        let reviewers = vec![RoleAgent::new(
            "opus",
            ScriptedAgent::new(vec![Some((
                review_path,
                review_json("opus", &["alpha"], &[]),
            ))]),
        )];
        executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap();

        // Always answers with an empty disposition set, never addressing the
        // blocking finding.
        let empty = serde_json::to_string(&DispositionSet::default()).unwrap();
        let disposition_path = harness.layout.dispositions(PhaseKind::Plan, 1);
        let author = RoleAgent::new(
            "planner",
            ScriptedAgent::new(vec![
                Some((disposition_path.clone(), empty.clone())),
                Some((disposition_path.clone(), empty.clone())),
                Some((disposition_path, empty)),
            ]),
        );

        let outcome = executor
            .execute_disposition(&mut phase, &author)
            .await
            .unwrap();

        assert!(matches!(outcome, DispositionOutcome::Rejected(_)));
        assert_eq!(phase.exit_reason(), Some(ExitReason::Stuck));
        assert_eq!(phase.ledger.cycles_used(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn round_one_reviewers_are_blind_to_each_other() {
        let harness = harness();
        let executor = executor(&harness);
        let mut phase = phase();
        phase.record_draft().unwrap();

        assert_eq!(executor.render_previous_findings(&phase), "");

        let review_path = harness.layout.review(PhaseKind::Plan, 1, "opus");
        let reviewers = vec![RoleAgent::new(
            "opus",
            ScriptedAgent::new(vec![Some((
                review_path,
                review_json("opus", &["alpha"], &[]),
            ))]),
        )];
        executor
            .execute_review_round(&mut phase, &reviewers)
            .await
            .unwrap();
        let answers = dispositions_json(
            phase.current_round_findings(),
            DispositionAction::Accepted,
            "",
        );
        let author = RoleAgent::new(
            "planner",
            ScriptedAgent::new(vec![Some((
                harness.layout.dispositions(PhaseKind::Plan, 1),
                answers,
            ))]),
        );
        executor
            .execute_disposition(&mut phase, &author)
            .await
            .unwrap();

        // From round two on they see what is already settled.
        let carried = executor.render_previous_findings(&phase);
        assert!(carried.contains("Do not re-raise"));
        assert!(carried.contains("alpha"));
    }
}
