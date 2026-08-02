//! The human approval gate.
//!
//! Every phase ends here, whatever its exit reason. The gate's job is to make
//! the decision cheap: the packet leads with the disputes, because those are
//! the only part the agents could not settle among themselves, and everything
//! the reviewers and author already agreed on sits below, collapsed.
//!
//! [`FileApprovalGate`] is the version that needs no UI: it writes the packet
//! as markdown next to the run and waits for a decision file to appear. That is
//! enough to use the pipeline today, and the trait leaves room for the push
//! notification and two buttons that replace it later.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agentchat_protocol::run::{
    ApprovalDecision, ApprovalPacket, ArchiveSection, PhaseKind, ReviewedFinding,
};
use async_trait::async_trait;
use tracing::info;

/// How a run asks the human to decide.
#[async_trait(?Send)]
pub trait ApprovalGate {
    /// Blocks until the human decides.
    async fn request(&self, packet: &ApprovalPacket) -> Result<ApprovalDecision, String>;
}

/// Writes the packet to disk and polls for a decision file.
pub struct FileApprovalGate {
    run_dir: PathBuf,
    poll_interval: Duration,
}

impl FileApprovalGate {
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
            poll_interval: Duration::from_secs(5),
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn packet_path(&self, phase: PhaseKind) -> PathBuf {
        self.run_dir.join(format!("approval-{}.md", phase.as_str()))
    }

    pub fn packet_json_path(&self, phase: PhaseKind) -> PathBuf {
        self.run_dir
            .join(format!("approval-{}.json", phase.as_str()))
    }

    pub fn decision_path(&self, phase: PhaseKind) -> PathBuf {
        self.run_dir
            .join(format!("decision-{}.json", phase.as_str()))
    }

    /// Writes the packet and returns the path the human should read.
    pub async fn publish(&self, packet: &ApprovalPacket) -> Result<PathBuf, String> {
        tokio::fs::create_dir_all(&self.run_dir)
            .await
            .map_err(|e| format!("cannot create {}: {e}", self.run_dir.display()))?;

        let markdown = self.packet_path(packet.phase);
        tokio::fs::write(&markdown, render_markdown(packet))
            .await
            .map_err(|e| format!("cannot write {}: {e}", markdown.display()))?;

        let json = self.packet_json_path(packet.phase);
        let body = serde_json::to_string_pretty(packet)
            .map_err(|e| format!("cannot serialize packet: {e}"))?;
        tokio::fs::write(&json, body)
            .await
            .map_err(|e| format!("cannot write {}: {e}", json.display()))?;

        Ok(markdown)
    }

    /// Reads a decision if one has been written, consuming it.
    ///
    /// The file is removed once read so the next gate in the same run does not
    /// see a stale decision.
    pub async fn take_decision(
        &self,
        phase: PhaseKind,
    ) -> Result<Option<ApprovalDecision>, String> {
        let path = self.decision_path(phase);
        let Ok(raw) = tokio::fs::read_to_string(&path).await else {
            return Ok(None);
        };

        let decision: ApprovalDecision = serde_json::from_str(&raw)
            .map_err(|e| format!("{} is not a valid decision: {e}", path.display()))?;
        let _ = tokio::fs::remove_file(&path).await;
        Ok(Some(decision))
    }

    fn instructions(&self, phase: PhaseKind) -> String {
        format!(
            "\n  Read:    {}\n  Approve: echo '{{\"decision\":\"approve\"}}' > {}\n  Changes: echo '{{\"decision\":\"request_changes\",\"comments\":\"...\"}}' > {}\n",
            self.packet_path(phase).display(),
            self.decision_path(phase).display(),
            self.decision_path(phase).display(),
        )
    }
}

#[async_trait(?Send)]
impl ApprovalGate for FileApprovalGate {
    async fn request(&self, packet: &ApprovalPacket) -> Result<ApprovalDecision, String> {
        self.publish(packet).await?;
        // Clear anything left from a previous gate before announcing this one.
        let _ = tokio::fs::remove_file(self.decision_path(packet.phase)).await;

        info!(
            "{} phase awaiting approval{}",
            packet.phase.as_str(),
            self.instructions(packet.phase)
        );
        println!(
            "\n=== {} ready for your review ===\n{} disputes, {} follow-ups, exit: {}\n{}",
            packet.phase.as_str(),
            packet.disputes.len(),
            packet.followups.len(),
            packet
                .exit_reason
                .map(|reason| reason.as_str())
                .unwrap_or("unknown"),
            self.instructions(packet.phase)
        );

        loop {
            if let Some(decision) = self.take_decision(packet.phase).await? {
                return Ok(decision);
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

/// Renders the packet as the page a human actually reads.
pub fn render_markdown(packet: &ApprovalPacket) -> String {
    let mut out = String::new();
    let summary = &packet.summary;

    out.push_str(&format!(
        "# {} approval · {} v{}\n\n",
        packet.phase.as_str(),
        packet.run_id,
        packet.version
    ));
    out.push_str(&format!(
        "Exit: **{}** after {} cycle(s){}\n\n",
        packet
            .exit_reason
            .map(|reason| reason.as_str())
            .unwrap_or("unknown"),
        summary.cycles_used,
        if summary.human_iterations > 0 {
            format!(", {} human round-trip(s)", summary.human_iterations)
        } else {
            String::new()
        }
    ));

    out.push_str("## Decide\n\n");
    if packet.disputes.is_empty() {
        out.push_str(
            "Nothing is disputed. The reviewers and the author agreed on everything blocking.\n\n",
        );
    } else {
        out.push_str(
            "The author rejected these blocking findings. This is the part that needs you.\n\n",
        );
        for item in &packet.disputes {
            out.push_str(&render_item(item));
        }
    }

    out.push_str("## Summary\n\n");
    out.push_str(&format!(
        "| | raised | accepted | disputed |\n|---|---|---|---|\n| blocking | {} | {} | {} |\n\n",
        summary.blocking_raised, summary.blocking_accepted, summary.blocking_disputed
    ));
    out.push_str(&format!(
        "Advisory: {} raised, {} adopted, {} left as follow-ups.\n\n",
        summary.non_blocking_raised, summary.non_blocking_adopted, summary.non_blocking_declined
    ));

    if !packet.followups.is_empty() {
        out.push_str("## Follow-ups\n\nSuggestions nobody adopted. Worth an Issue?\n\n");
        for item in &packet.followups {
            out.push_str(&format!(
                "- `{}` **{}** — {} _(#{})_{}\n",
                item.finding.file,
                item.finding.severity.category_str(),
                item.finding.problem,
                item.finding.reviewer,
                item.disposition
                    .as_ref()
                    .filter(|answer| !answer.reason.trim().is_empty())
                    .map(|answer| format!("\n  - author passed: {}", answer.reason))
                    .unwrap_or_default(),
            ));
        }
        out.push('\n');
    }

    if !packet.archive.is_empty() {
        out.push_str("## Discussion\n\n");
        for section in &packet.archive {
            out.push_str(&render_section(section));
        }
    }

    out
}

fn render_section(section: &ArchiveSection) -> String {
    let mut out = format!("### {}\n\n", section.title);
    for group in &section.groups {
        out.push_str(&format!(
            "**`{}` · {}** — {} reviewer(s) independently flagged this\n\n",
            group.file, group.category, group.consensus
        ));
        for item in &group.items {
            out.push_str(&format!(
                "- _{}_: {}\n",
                item.finding.reviewer, item.finding.problem
            ));
        }
        out.push('\n');
    }
    out
}

fn render_item(item: &ReviewedFinding) -> String {
    let finding = &item.finding;
    let mut out = format!(
        "### `{}` · {}\n\n- **Reviewer** ({}): {}\n- **Evidence**: {}\n- **Suggested**: {}\n",
        finding.location,
        finding.severity.category_str(),
        finding.reviewer,
        finding.problem,
        finding.evidence,
        finding.recommendation,
    );
    if let Some(answer) = &item.disposition {
        out.push_str(&format!("- **Author disputes**: {}\n", answer.reason));
    }
    out.push('\n');
    out
}

/// Where the human's comments are appended so the author sees them on redraft.
pub fn feedback_heading(round: u32) -> String {
    format!("\n\n## Human feedback (round {round})\n\n")
}

/// Appends the human's comments to the brief.
///
/// The brief is what the author re-reads, so feedback belongs there rather than
/// in a file the prompt would have to remember to mention.
pub async fn append_feedback(brief: &Path, round: u32, comments: &str) -> Result<(), String> {
    if comments.trim().is_empty() {
        return Ok(());
    }

    let mut body = tokio::fs::read_to_string(brief).await.unwrap_or_default();
    body.push_str(&feedback_heading(round));
    body.push_str(comments.trim());
    body.push('\n');

    tokio::fs::write(brief, body)
        .await
        .map_err(|e| format!("cannot update {}: {e}", brief.display()))
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{
        ArchiveGroup, BlockingCategory, DiscussionSummary, Disposition, DispositionAction,
        ExitReason, Finding, FindingSeverity, NonBlockingCategory,
    };

    use super::*;

    fn finding(reviewer: &str, blocking: bool, problem: &str) -> Finding {
        Finding {
            finding_id: format!("id-{reviewer}-{problem}"),
            reviewer: reviewer.into(),
            round: 1,
            severity: if blocking {
                FindingSeverity::Blocking {
                    category: BlockingCategory::Incorrect,
                }
            } else {
                FindingSeverity::NonBlocking {
                    category: NonBlockingCategory::TestGap,
                }
            },
            file: "core/src/run/budget.rs".into(),
            location: "core/src/run/budget.rs:88".into(),
            problem: problem.into(),
            evidence: "max_cycles=2 admits a third revision".into(),
            recommendation: "use >=".into(),
            demoted_from: None,
        }
    }

    fn packet() -> ApprovalPacket {
        let disputed = finding("opus", true, "cap is off by one");
        let advisory = finding("luna", false, "no concurrency test");
        ApprovalPacket {
            run_id: "run-1".into(),
            phase: PhaseKind::Plan,
            version: 3,
            exit_reason: Some(ExitReason::CycleCap),
            disputes: vec![ReviewedFinding {
                disposition: Some(Disposition {
                    finding_id: disputed.finding_id.clone(),
                    action: DispositionAction::Disputed,
                    reason: "the brief scopes this to v2".into(),
                    changed_files: vec![],
                }),
                finding: disputed,
            }],
            summary: DiscussionSummary {
                blocking_raised: 3,
                blocking_accepted: 2,
                blocking_disputed: 1,
                non_blocking_raised: 4,
                non_blocking_adopted: 1,
                non_blocking_declined: 3,
                cycles_used: 2,
                human_iterations: 0,
            },
            followups: vec![ReviewedFinding {
                finding: advisory.clone(),
                disposition: None,
            }],
            archive: vec![ArchiveSection {
                title: "Advisory · flagged by 2+ reviewers".into(),
                expanded: true,
                groups: vec![ArchiveGroup {
                    file: "core/src/run/budget.rs".into(),
                    category: "test_gap".into(),
                    consensus: 2,
                    items: vec![ReviewedFinding {
                        finding: advisory,
                        disposition: None,
                    }],
                }],
            }],
        }
    }

    #[test]
    fn the_rendered_page_leads_with_the_disputes() {
        let markdown = render_markdown(&packet());

        let decide = markdown.find("## Decide").unwrap();
        let summary = markdown.find("## Summary").unwrap();
        let discussion = markdown.find("## Discussion").unwrap();

        assert!(decide < summary && summary < discussion);
        assert!(markdown.contains("the brief scopes this to v2"));
        assert!(markdown.contains("cycle_cap"));
    }

    #[test]
    fn a_packet_with_no_disputes_says_so_plainly() {
        let mut packet = packet();
        packet.disputes.clear();

        let markdown = render_markdown(&packet);

        assert!(markdown.contains("Nothing is disputed"));
    }

    #[test]
    fn the_archive_reports_how_many_reviewers_agreed() {
        let markdown = render_markdown(&packet());

        assert!(markdown.contains("2 reviewer(s) independently flagged this"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publish_writes_both_renderings() {
        let dir = tempfile::tempdir().unwrap();
        let gate = FileApprovalGate::new(dir.path());

        let path = gate.publish(&packet()).await.unwrap();

        assert!(path.ends_with("approval-plan.md"));
        assert!(gate.packet_json_path(PhaseKind::Plan).is_file());
        let restored: ApprovalPacket = serde_json::from_str(
            &tokio::fs::read_to_string(gate.packet_json_path(PhaseKind::Plan))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(restored, packet());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_decision_is_consumed_once() {
        let dir = tempfile::tempdir().unwrap();
        let gate = FileApprovalGate::new(dir.path());
        tokio::fs::write(
            gate.decision_path(PhaseKind::Plan),
            r#"{"decision":"approve"}"#,
        )
        .await
        .unwrap();

        let first = gate.take_decision(PhaseKind::Plan).await.unwrap();
        let second = gate.take_decision(PhaseKind::Plan).await.unwrap();

        assert_eq!(first, Some(ApprovalDecision::Approve));
        assert_eq!(
            second, None,
            "a stale decision must not leak to the next gate"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn request_changes_carries_comments() {
        let dir = tempfile::tempdir().unwrap();
        let gate = FileApprovalGate::new(dir.path());
        tokio::fs::write(
            gate.decision_path(PhaseKind::Code),
            r#"{"decision":"request_changes","comments":"use the existing store"}"#,
        )
        .await
        .unwrap();

        let decision = gate.take_decision(PhaseKind::Code).await.unwrap();

        assert_eq!(
            decision,
            Some(ApprovalDecision::RequestChanges {
                comments: "use the existing store".into()
            })
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_malformed_decision_is_reported_not_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let gate = FileApprovalGate::new(dir.path());
        tokio::fs::write(gate.decision_path(PhaseKind::Plan), "approve")
            .await
            .unwrap();

        assert!(gate.take_decision(PhaseKind::Plan).await.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn feedback_lands_in_the_brief_where_the_author_will_read_it() {
        let dir = tempfile::tempdir().unwrap();
        let brief = dir.path().join("brief.md");
        tokio::fs::write(&brief, "# Goal\n\nShip the thing.\n")
            .await
            .unwrap();

        append_feedback(&brief, 1, "  reuse the existing store  ")
            .await
            .unwrap();

        let body = tokio::fs::read_to_string(&brief).await.unwrap();
        assert!(body.starts_with("# Goal"));
        assert!(body.contains("## Human feedback (round 1)"));
        assert!(body.contains("reuse the existing store"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_feedback_leaves_the_brief_alone() {
        let dir = tempfile::tempdir().unwrap();
        let brief = dir.path().join("brief.md");
        tokio::fs::write(&brief, "# Goal\n").await.unwrap();

        append_feedback(&brief, 1, "   ").await.unwrap();

        assert_eq!(tokio::fs::read_to_string(&brief).await.unwrap(), "# Goal\n");
    }
}
