//! Types for orchestrated multi-agent runs.
//!
//! A run drives a brief through plan and code phases. Each phase repeats a
//! *cycle*: the author produces a version, reviewers fan out over it, and the
//! author responds to every blocking finding before producing the next version.
//! The types here describe what reviewers write, what survives validation, and
//! how the author must respond.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ============================================================
// Finding categories
// ============================================================

/// Categories that may block a cycle from converging.
///
/// This set is closed on purpose. Severity cannot be self-reported: different
/// models calibrate "high" differently, so a label chosen by the reviewer is
/// not comparable across reviewers. A finding is blocking only if it names one
/// of these categories and backs it with concrete evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockingCategory {
    /// Contradicts something the brief explicitly asked for.
    ContradictsBrief,
    /// Omits a requirement stated in the brief.
    MissingRequirement,
    /// Produces wrong behaviour, a crash, or wrong output.
    Incorrect,
    /// Breaks existing tests or documented behaviour.
    BreaksExisting,
    Security,
    DataLoss,
}

impl BlockingCategory {
    /// Parses a reviewer-supplied category token. Unknown tokens return `None`
    /// so the caller can demote rather than fail the whole report.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "contradicts_brief" => Some(Self::ContradictsBrief),
            "missing_requirement" => Some(Self::MissingRequirement),
            "incorrect" => Some(Self::Incorrect),
            "breaks_existing" => Some(Self::BreaksExisting),
            "security" => Some(Self::Security),
            "data_loss" => Some(Self::DataLoss),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ContradictsBrief => "contradicts_brief",
            Self::MissingRequirement => "missing_requirement",
            Self::Incorrect => "incorrect",
            Self::BreaksExisting => "breaks_existing",
            Self::Security => "security",
            Self::DataLoss => "data_loss",
        }
    }
}

/// Categories that never block. Suggestions in this set are advisory: the
/// author may ignore them without writing a disposition, and they surface to
/// the human in the approval packet as discussion history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NonBlockingCategory {
    Style,
    Naming,
    PerfHint,
    TestGap,
    Refactor,
    Readability,
    /// Fallback for anything the validator could not place, including findings
    /// demoted out of the blocking array.
    Other,
}

impl NonBlockingCategory {
    /// Parses a reviewer-supplied category token, falling back to [`Self::Other`].
    pub fn parse_or_other(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "style" => Self::Style,
            "naming" => Self::Naming,
            "perf_hint" => Self::PerfHint,
            "test_gap" => Self::TestGap,
            "refactor" => Self::Refactor,
            "readability" => Self::Readability,
            _ => Self::Other,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Naming => "naming",
            Self::PerfHint => "perf_hint",
            Self::TestGap => "test_gap",
            Self::Refactor => "refactor",
            Self::Readability => "readability",
            Self::Other => "other",
        }
    }
}

/// Severity plus category of a validated finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "severity", rename_all = "snake_case")]
pub enum FindingSeverity {
    Blocking { category: BlockingCategory },
    NonBlocking { category: NonBlockingCategory },
}

impl FindingSeverity {
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Blocking { .. })
    }

    pub fn category_str(&self) -> &'static str {
        match self {
            Self::Blocking { category } => category.as_str(),
            Self::NonBlocking { category } => category.as_str(),
        }
    }
}

/// Why the validator moved a finding out of the blocking array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DemotionReason {
    /// The category was not in [`BlockingCategory`].
    UnknownCategory,
    /// Evidence was absent, too short, a restatement of the problem, or free of
    /// any concrete reference.
    WeakEvidence,
    /// The report exceeded the per-report blocking limit.
    OverBlockingLimit,
}

impl DemotionReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnknownCategory => "unknown_category",
            Self::WeakEvidence => "weak_evidence",
            Self::OverBlockingLimit => "over_blocking_limit",
        }
    }
}

// ============================================================
// Raw reviewer output
// ============================================================

/// A review report exactly as a reviewer wrote it to disk.
///
/// Every field is lenient: an agent that omits or misspells something should
/// lose influence through demotion, not fail the run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RawReviewReport {
    #[serde(default)]
    pub reviewer: String,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub blocking: Vec<RawFinding>,
    #[serde(default)]
    pub non_blocking: Vec<RawFinding>,
}

/// One finding as written by a reviewer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RawFinding {
    #[serde(default)]
    pub category: String,
    /// `path/to/file.rs` or `path/to/file.rs:88`.
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub problem: String,
    /// Concrete support: an input that reaches the bug, a brief clause that is
    /// contradicted, a test that fails. Required for blocking findings.
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub recommendation: String,
}

// ============================================================
// Validated findings
// ============================================================

/// A finding after validation, carrying a stable identity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    /// Stable across rounds: the same file, category, and problem text always
    /// hash to the same id, so a re-raised finding is recognisable.
    pub finding_id: String,
    pub reviewer: String,
    pub round: u32,
    #[serde(flatten)]
    pub severity: FindingSeverity,
    /// Normalised file path, used as the grouping key.
    pub file: String,
    /// The reviewer's original `location` string.
    pub location: String,
    pub problem: String,
    pub evidence: String,
    pub recommendation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demoted_from: Option<DemotionReason>,
}

impl Finding {
    pub fn is_blocking(&self) -> bool {
        self.severity.is_blocking()
    }
}

/// Findings that share a file and category.
///
/// Grouping is deliberately not merging: reviewers phrase the same issue
/// differently and text similarity is unreliable across languages, so each
/// reviewer's wording is preserved. `consensus` — how many distinct reviewers
/// landed on the same file and category — is the signal worth ranking by.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FindingGroup {
    pub file: String,
    pub severity: FindingSeverity,
    pub consensus: usize,
    pub findings: Vec<Finding>,
}

// ============================================================
// Dispositions
// ============================================================

/// How the author responded to a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispositionAction {
    /// The author changed the plan or code in response.
    Accepted,
    /// The author rejects the finding and argues why. Requires a reason, and
    /// carries into the next round for exactly one re-check.
    Disputed,
    /// Advisory only. Valid for non-blocking findings; rejected for blocking.
    Declined,
}

impl DispositionAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Disputed => "disputed",
            Self::Declined => "declined",
        }
    }
}

/// The author's response to one finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Disposition {
    pub finding_id: String,
    pub action: DispositionAction,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub changed_files: Vec<String>,
}

/// The author's full response for one round.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DispositionSet {
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub dispositions: Vec<Disposition>,
}

// ============================================================
// Cycle budget
// ============================================================

/// Retries that do not consume cycle budget.
///
/// A flaky agent must not be able to eat the discussion budget: only an actual
/// exchange of opinion for a revision counts as a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeRetryConfig {
    /// Reviewer wrote a report that failed schema validation.
    pub invalid_output: u32,
    /// Agent process crashed, hung, or tripped the stage watchdog.
    pub agent_failure: u32,
    /// Author failed the disposition gate.
    pub disposition: u32,
}

impl Default for FreeRetryConfig {
    fn default() -> Self {
        Self {
            invalid_output: 1,
            agent_failure: 2,
            disposition: 1,
        }
    }
}

/// Budget for one phase of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleBudgetConfig {
    /// How many times the author may revise in response to review. The initial
    /// draft is not a cycle, so `max_cycles = 2` delivers version 3.
    pub max_cycles: u32,
    #[serde(default)]
    pub free_retries: FreeRetryConfig,
}

impl Default for CycleBudgetConfig {
    fn default() -> Self {
        Self {
            max_cycles: 2,
            free_retries: FreeRetryConfig::default(),
        }
    }
}

/// Which free-retry allowance a failure draws on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryKind {
    InvalidOutput,
    AgentFailure,
    Disposition,
}

/// Why a phase stopped iterating.
///
/// All four lead to the same human approval gate; they differ only in what the
/// approval packet's dispute section contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// The last cycle produced no new blocking findings.
    Converged,
    /// New blocking findings stopped shrinking — reviewers are churning, so
    /// further cycles would burn budget without converging.
    Churn,
    /// The cycle budget ran out with disputes still open.
    CycleCap,
    /// The author kept revising without a review completing in between.
    Stuck,
}

impl ExitReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Converged => "converged",
            Self::Churn => "churn",
            Self::CycleCap => "cycle_cap",
            Self::Stuck => "stuck",
        }
    }

    /// Whether the phase finished with every blocking finding resolved.
    pub fn is_clean(&self) -> bool {
        matches!(self, Self::Converged)
    }
}

// ============================================================
// Run structure
// ============================================================

/// Which artifact a phase iterates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    /// Iterating on `plan.md`.
    Plan,
    /// Iterating on the working tree.
    Code,
}

impl PhaseKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Code => "code",
        }
    }
}

/// Where a phase is in its loop.
///
/// This is the resume point: each stage is idempotent given the files on disk,
/// so a daemon restart re-enters the recorded stage rather than the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    /// The author is writing the next version of the artifact.
    Authoring,
    /// Reviewers are fanning out over the current version.
    Reviewing,
    /// The author is answering this round's blocking findings and producing the
    /// next version. Both happen in one agent turn.
    Dispositioning,
    /// The phase is finished and waiting on the human.
    AwaitingApproval,
}

impl StageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Authoring => "authoring",
            Self::Reviewing => "reviewing",
            Self::Dispositioning => "dispositioning",
            Self::AwaitingApproval => "awaiting_approval",
        }
    }
}

/// Where a run is overall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Planning,
    AwaitingPlanApproval,
    Implementing,
    AwaitingCodeApproval,
    Completed,
    Cancelled,
}

impl RunStatus {
    /// Whether the run has nothing left to do. Runs that are not terminal are
    /// picked back up when the daemon restarts.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }

    /// Whether the run is blocked on the human rather than on an agent.
    pub fn awaits_human(&self) -> bool {
        matches!(
            self,
            Self::AwaitingPlanApproval | Self::AwaitingCodeApproval
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::AwaitingPlanApproval => "awaiting_plan_approval",
            Self::Implementing => "implementing",
            Self::AwaitingCodeApproval => "awaiting_code_approval",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        }
    }
}

// ============================================================
// Approval packet
// ============================================================

/// A finding paired with how the author answered it.
///
/// Pairing matters: an author's reason for declining a suggestion says more
/// about whether it understood the suggestion than the suggestion itself does.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewedFinding {
    pub finding: Finding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<Disposition>,
}

/// Counts for the "one glance" section of the approval packet.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DiscussionSummary {
    pub blocking_raised: usize,
    pub blocking_accepted: usize,
    pub blocking_disputed: usize,
    pub non_blocking_raised: usize,
    pub non_blocking_adopted: usize,
    pub non_blocking_declined: usize,
    pub cycles_used: u32,
    pub human_iterations: u32,
}

/// One collapsible section of the discussion archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchiveSection {
    pub title: String,
    /// Whether the UI should render this section open by default.
    pub expanded: bool,
    pub groups: Vec<ArchiveGroup>,
}

/// Findings that share a file and category, with the author's answers attached.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArchiveGroup {
    pub file: String,
    pub category: String,
    pub consensus: usize,
    pub items: Vec<ReviewedFinding>,
}

/// Everything the human sees at an approval gate.
///
/// Assembled mechanically from `findings.jsonl` and `dispositions.json`. The
/// author never writes this: it decided what to decline, so letting it also
/// narrate what it declined is a conflict of interest, and the assembly is pure
/// data rendering that would only lose fidelity through a model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalPacket {
    pub run_id: String,
    pub phase: PhaseKind,
    pub version: u32,
    pub exit_reason: Option<ExitReason>,
    /// Blocking findings the author argued down. The only part that genuinely
    /// needs human judgement.
    pub disputes: Vec<ReviewedFinding>,
    pub summary: DiscussionSummary,
    /// Advisory findings the author did not adopt, ready to become Issues.
    pub followups: Vec<ReviewedFinding>,
    /// Discussion history, ranked by how many reviewers independently agreed.
    pub archive: Vec<ArchiveSection>,
}

/// What the human decided at an approval gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ApprovalDecision {
    /// Accept this phase and move on.
    Approve,
    /// Send the phase back to its author with new information. The budget
    /// resets: the human adding context is not the models spinning.
    RequestChanges {
        #[serde(default)]
        comments: String,
    },
    /// Abandon the run.
    Cancel,
}

// ============================================================
// Identity
// ============================================================

/// Placeholder file key for findings that name no location.
pub const UNSPECIFIED_FILE: &str = "(unspecified)";

/// Length of the hex-encoded finding id.
pub const FINDING_ID_LEN: usize = 12;

/// Computes the stable identity of a finding.
///
/// Inputs are expected to be normalised already (see `agentchat_core::run`).
/// Identity is exact-text based, which is enough for its actual job: matching a
/// finding a reviewer re-raises after being fed the previous round's text
/// verbatim. It is deliberately not used to merge different reviewers' wording.
pub fn finding_id(file: &str, category: &str, problem: &str) -> String {
    let digest = Sha256::digest(format!("{file}\u{1f}{category}\u{1f}{problem}").as_bytes());
    let mut out = String::with_capacity(FINDING_ID_LEN);
    for byte in digest.iter() {
        if out.len() >= FINDING_ID_LEN {
            break;
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out.truncate(FINDING_ID_LEN);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_id_is_stable_and_fixed_length() {
        let a = finding_id("core/src/run/budget.rs", "incorrect", "cap is off by one");
        let b = finding_id("core/src/run/budget.rs", "incorrect", "cap is off by one");

        assert_eq!(a, b);
        assert_eq!(a.len(), FINDING_ID_LEN);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn finding_id_separates_fields() {
        // Without a separator these two would hash the same concatenation.
        let a = finding_id("ab", "c", "d");
        let b = finding_id("a", "bc", "d");

        assert_ne!(a, b);
    }

    #[test]
    fn blocking_category_parse_rejects_unknown() {
        assert_eq!(
            BlockingCategory::parse("  Incorrect "),
            Some(BlockingCategory::Incorrect)
        );
        assert_eq!(BlockingCategory::parse("high"), None);
        assert_eq!(BlockingCategory::parse("style"), None);
    }

    #[test]
    fn non_blocking_category_falls_back_to_other() {
        assert_eq!(
            NonBlockingCategory::parse_or_other("TEST_GAP"),
            NonBlockingCategory::TestGap
        );
        assert_eq!(
            NonBlockingCategory::parse_or_other("nitpick"),
            NonBlockingCategory::Other
        );
    }

    #[test]
    fn finding_serializes_severity_inline() {
        let finding = Finding {
            finding_id: "0123456789ab".into(),
            reviewer: "opus".into(),
            round: 1,
            severity: FindingSeverity::Blocking {
                category: BlockingCategory::Incorrect,
            },
            file: "core/src/run/budget.rs".into(),
            location: "core/src/run/budget.rs:88".into(),
            problem: "off by one".into(),
            evidence: "max_cycles=2 allows 3 revisions".into(),
            recommendation: "use >=".into(),
            demoted_from: None,
        };

        let json = serde_json::to_value(&finding).unwrap();
        assert_eq!(json["severity"], "blocking");
        assert_eq!(json["category"], "incorrect");
        assert!(json.get("demoted_from").is_none());

        let decoded: Finding = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, finding);
    }

    #[test]
    fn raw_report_tolerates_missing_fields() {
        let report: RawReviewReport = serde_json::from_str(r#"{"reviewer":"deepseek"}"#).unwrap();

        assert_eq!(report.reviewer, "deepseek");
        assert_eq!(report.round, 0);
        assert!(report.blocking.is_empty());
        assert!(report.non_blocking.is_empty());
    }

    #[test]
    fn budget_and_disposition_types_round_trip() {
        let config = CycleBudgetConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(
            serde_json::from_str::<CycleBudgetConfig>(&json).unwrap(),
            config
        );

        let set = DispositionSet {
            round: 1,
            dispositions: vec![Disposition {
                finding_id: "0123456789ab".into(),
                action: DispositionAction::Disputed,
                reason: "the brief scopes this to v2".into(),
                changed_files: vec![],
            }],
        };
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(serde_json::from_str::<DispositionSet>(&json).unwrap(), set);

        for reason in [
            ExitReason::Converged,
            ExitReason::Churn,
            ExitReason::CycleCap,
            ExitReason::Stuck,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(serde_json::from_str::<ExitReason>(&json).unwrap(), reason);
        }
    }
}
