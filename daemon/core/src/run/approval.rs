//! Assembles the approval packet a human reads at a gate.
//!
//! Built mechanically from the phase's findings and dispositions. The author
//! never writes this: it decided what to decline, so letting it also narrate
//! what it declined is a conflict of interest, it costs an extra agent turn,
//! and paraphrasing can only lose fidelity. The author contributes exactly the
//! `reason` strings it already had to write.
//!
//! The layout is progressive disclosure. The decision section is the disputes —
//! the only part that genuinely needs human judgement, because everything else
//! the reviewers and the author already agreed on. Discussion history sits
//! below it, ranked by how many reviewers independently landed on the same
//! file and category, since that is the one ranking signal available without
//! pretending to semantic precision.

use std::collections::{BTreeMap, BTreeSet};

use agentchat_protocol::run::{
    ApprovalPacket, ArchiveGroup, ArchiveSection, DiscussionSummary, Disposition,
    DispositionAction, Finding, ReviewedFinding,
};

use crate::run::findings::group_findings;
use crate::run::state::PhaseState;

/// Builds the packet for a phase that has reached its approval gate.
pub fn build_packet(run_id: &str, phase: &PhaseState) -> ApprovalPacket {
    let findings = dedupe(&phase.findings);
    let answers = disposition_map(&phase.dispositions);

    let (blocking, advisory): (Vec<Finding>, Vec<Finding>) =
        findings.into_iter().partition(Finding::is_blocking);

    let disputes: Vec<ReviewedFinding> = reviewed(&blocking, &answers)
        .into_iter()
        .filter(|item| action_of(item) == Some(DispositionAction::Disputed))
        .collect();

    let accepted_blocking: Vec<ReviewedFinding> = reviewed(&blocking, &answers)
        .into_iter()
        .filter(|item| action_of(item) == Some(DispositionAction::Accepted))
        .collect();

    // Advisory findings the author did not adopt. Absence of a disposition is
    // an implicit decline, which is the common case by design.
    let followups: Vec<ReviewedFinding> = reviewed(&advisory, &answers)
        .into_iter()
        .filter(|item| action_of(item) != Some(DispositionAction::Accepted))
        .collect();

    let summary = DiscussionSummary {
        blocking_raised: blocking.len(),
        blocking_accepted: accepted_blocking.len(),
        blocking_disputed: disputes.len(),
        non_blocking_raised: advisory.len(),
        non_blocking_adopted: advisory.len() - followups.len(),
        non_blocking_declined: followups.len(),
        cycles_used: phase.ledger.cycles_used(),
        human_iterations: phase.ledger.human_iterations(),
    };

    let advisory_groups = group_findings(&advisory);
    let archive = vec![
        ArchiveSection {
            title: "Advisory · flagged by 2+ reviewers".into(),
            expanded: true,
            groups: archive_groups(&advisory_groups, &answers, |consensus| consensus >= 2),
        },
        ArchiveSection {
            title: "Advisory · single reviewer".into(),
            expanded: false,
            groups: archive_groups(&advisory_groups, &answers, |consensus| consensus < 2),
        },
        ArchiveSection {
            title: "Blocking · accepted and addressed".into(),
            expanded: false,
            groups: archive_groups(
                &group_findings(
                    &accepted_blocking
                        .iter()
                        .map(|item| item.finding.clone())
                        .collect::<Vec<_>>(),
                ),
                &answers,
                |_| true,
            ),
        },
    ]
    .into_iter()
    .filter(|section| !section.groups.is_empty())
    .collect();

    ApprovalPacket {
        run_id: run_id.to_string(),
        phase: phase.kind,
        version: phase.version,
        exit_reason: phase.exit_reason(),
        disputes,
        summary,
        followups,
        archive,
    }
}

/// Drops repeats of the same reviewer saying the same thing across rounds,
/// while keeping distinct reviewers who landed on the same finding.
fn dedupe(findings: &[Finding]) -> Vec<Finding> {
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    findings
        .iter()
        .filter(|finding| seen.insert((finding.reviewer.as_str(), finding.finding_id.as_str())))
        .cloned()
        .collect()
}

/// Last disposition wins: a later round may revise an earlier answer.
fn disposition_map(dispositions: &[Disposition]) -> BTreeMap<&str, &Disposition> {
    let mut map = BTreeMap::new();
    for disposition in dispositions {
        map.insert(disposition.finding_id.as_str(), disposition);
    }
    map
}

fn reviewed(findings: &[Finding], answers: &BTreeMap<&str, &Disposition>) -> Vec<ReviewedFinding> {
    findings
        .iter()
        .map(|finding| ReviewedFinding {
            finding: finding.clone(),
            disposition: answers
                .get(finding.finding_id.as_str())
                .map(|answer| (*answer).clone()),
        })
        .collect()
}

fn action_of(item: &ReviewedFinding) -> Option<DispositionAction> {
    item.disposition.as_ref().map(|answer| answer.action)
}

fn archive_groups(
    groups: &[agentchat_protocol::run::FindingGroup],
    answers: &BTreeMap<&str, &Disposition>,
    keep: impl Fn(usize) -> bool,
) -> Vec<ArchiveGroup> {
    groups
        .iter()
        .filter(|group| keep(group.consensus))
        .map(|group| ArchiveGroup {
            file: group.file.clone(),
            category: group.severity.category_str().to_string(),
            consensus: group.consensus,
            items: reviewed(&group.findings, answers),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{
        CycleBudgetConfig, ExitReason, PhaseKind, RawFinding, RawReviewReport,
    };

    use super::*;
    use crate::run::findings::validate_round;
    use crate::run::state::PhaseState;

    const EVIDENCE: &str = "reaching stage_two with an empty plan panics in run/stage.rs";

    fn blocking(slug: &str) -> RawFinding {
        RawFinding {
            category: "incorrect".into(),
            location: format!("core/src/run/{slug}.rs"),
            problem: format!("{slug} is wrong"),
            evidence: EVIDENCE.into(),
            recommendation: "fix it".into(),
        }
    }

    fn advisory(slug: &str, note: &str) -> RawFinding {
        RawFinding {
            category: "test_gap".into(),
            location: format!("core/src/run/{slug}.rs"),
            problem: note.into(),
            recommendation: "add a test".into(),
            ..RawFinding::default()
        }
    }

    fn answer(finding: &Finding, action: DispositionAction, reason: &str) -> Disposition {
        Disposition {
            finding_id: finding.finding_id.clone(),
            action,
            reason: reason.into(),
            changed_files: Vec::new(),
        }
    }

    /// A phase driven to its gate with one accepted and one disputed blocking
    /// finding, plus advisory notes from three reviewers.
    fn phase_at_gate() -> PhaseState {
        let mut phase = PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default());
        phase.record_draft().unwrap();

        let round = validate_round(&[
            RawReviewReport {
                reviewer: "opus".into(),
                round: 1,
                blocking: vec![blocking("alpha"), blocking("beta")],
                non_blocking: vec![
                    advisory("shared", "opus wording"),
                    advisory("solo", "only opus"),
                ],
            },
            RawReviewReport {
                reviewer: "luna".into(),
                round: 1,
                blocking: Vec::new(),
                non_blocking: vec![advisory("shared", "luna wording")],
            },
            RawReviewReport {
                reviewer: "deepseek".into(),
                round: 1,
                blocking: Vec::new(),
                non_blocking: vec![advisory("shared", "deepseek wording")],
            },
        ]);
        phase.record_review_round(round.clone()).unwrap();

        let blocking_findings: Vec<Finding> =
            round.iter().filter(|f| f.is_blocking()).cloned().collect();
        let advisory_findings: Vec<Finding> =
            round.iter().filter(|f| !f.is_blocking()).cloned().collect();

        let mut dispositions = vec![
            answer(&blocking_findings[0], DispositionAction::Accepted, ""),
            answer(
                &blocking_findings[1],
                DispositionAction::Disputed,
                "the brief scopes this to v2",
            ),
        ];
        // The author adopted one advisory note and ignored the rest.
        dispositions.push(answer(
            &advisory_findings[0],
            DispositionAction::Accepted,
            "",
        ));

        phase.record_disposition(dispositions).unwrap();

        // Second round re-raises nothing new, closing the phase.
        phase.record_review_round(Vec::new()).unwrap();
        phase.record_disposition(Vec::new()).unwrap();
        phase
    }

    #[test]
    fn the_decision_section_holds_only_disputes() {
        let packet = build_packet("run-1", &phase_at_gate());

        assert_eq!(packet.disputes.len(), 1);
        let dispute = &packet.disputes[0];
        assert_eq!(dispute.finding.file, "core/src/run/beta.rs");
        assert_eq!(
            dispute.disposition.as_ref().unwrap().reason,
            "the brief scopes this to v2"
        );
    }

    #[test]
    fn the_summary_reconciles() {
        let packet = build_packet("run-1", &phase_at_gate());
        let summary = &packet.summary;

        assert_eq!(summary.blocking_raised, 2);
        assert_eq!(summary.blocking_accepted, 1);
        assert_eq!(summary.blocking_disputed, 1);
        assert_eq!(summary.non_blocking_raised, 4);
        assert_eq!(summary.non_blocking_adopted, 1);
        assert_eq!(summary.non_blocking_declined, 3);
        assert_eq!(summary.cycles_used, 2);
        assert_eq!(packet.exit_reason, Some(ExitReason::Converged));
        assert_eq!(packet.phase, PhaseKind::Plan);
    }

    #[test]
    fn consensus_advisory_notes_land_in_the_expanded_section() {
        let packet = build_packet("run-1", &phase_at_gate());

        let consensus_section = packet
            .archive
            .iter()
            .find(|section| section.expanded)
            .expect("a section is expanded by default");

        assert_eq!(consensus_section.groups.len(), 1);
        let group = &consensus_section.groups[0];
        assert_eq!(group.file, "core/src/run/shared.rs");
        assert_eq!(group.consensus, 3);
        // Each reviewer's own wording is preserved rather than merged.
        assert_eq!(group.items.len(), 3);
        assert!(group
            .items
            .iter()
            .any(|item| item.finding.problem == "luna wording"));
    }

    #[test]
    fn single_reviewer_notes_are_collapsed_separately() {
        let packet = build_packet("run-1", &phase_at_gate());

        let solo = packet
            .archive
            .iter()
            .find(|section| section.title.contains("single reviewer"))
            .expect("solo section present");

        assert!(!solo.expanded);
        assert!(solo.groups.iter().all(|group| group.consensus == 1));
    }

    #[test]
    fn follow_ups_carry_the_authors_reason_when_it_gave_one() {
        let mut phase = PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default());
        phase.record_draft().unwrap();
        let round = validate_round(&[RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: Vec::new(),
            non_blocking: vec![advisory("solo", "no concurrency test")],
        }]);
        phase.record_review_round(round.clone()).unwrap();
        phase
            .record_disposition(vec![answer(
                &round[0],
                DispositionAction::Declined,
                "concurrency lands in v2",
            )])
            .unwrap();

        let packet = build_packet("run-1", &phase);

        assert_eq!(packet.followups.len(), 1);
        assert_eq!(
            packet.followups[0].disposition.as_ref().unwrap().reason,
            "concurrency lands in v2"
        );
    }

    #[test]
    fn an_unanswered_advisory_note_is_still_a_follow_up() {
        let packet = build_packet("run-1", &phase_at_gate());

        let unanswered = packet
            .followups
            .iter()
            .filter(|item| item.disposition.is_none())
            .count();

        assert_eq!(unanswered, 3, "silence on advisory notes is a decline");
    }

    #[test]
    fn empty_archive_sections_are_dropped() {
        let mut phase = PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default());
        phase.record_draft().unwrap();
        phase.record_review_round(Vec::new()).unwrap();
        phase.record_disposition(Vec::new()).unwrap();

        let packet = build_packet("run-1", &phase);

        assert!(packet.archive.is_empty());
        assert!(packet.disputes.is_empty());
        assert!(packet.followups.is_empty());
    }

    #[test]
    fn a_reviewer_repeating_itself_across_rounds_is_counted_once() {
        let mut phase = PhaseState::new(PhaseKind::Plan, CycleBudgetConfig::default());
        phase.record_draft().unwrap();
        // A blocking finding keeps the phase iterating; without one the first
        // round would converge and there would be no second round to repeat in.
        let report = RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: vec![blocking("alpha")],
            non_blocking: vec![advisory("solo", "same note")],
        };

        let round1 = validate_round(std::slice::from_ref(&report));
        phase.record_review_round(round1.clone()).unwrap();
        phase
            .record_disposition(vec![answer(&round1[0], DispositionAction::Accepted, "")])
            .unwrap();

        // Round two repeats both verbatim, so nothing is new and the phase ends.
        let round2 = validate_round(&[report]);
        phase.record_review_round(round2.clone()).unwrap();
        phase
            .record_disposition(vec![answer(&round2[0], DispositionAction::Accepted, "")])
            .unwrap();
        assert_eq!(phase.exit_reason(), Some(ExitReason::Converged));

        let packet = build_packet("run-1", &phase);

        assert_eq!(packet.summary.non_blocking_raised, 1);
        assert_eq!(packet.summary.blocking_raised, 1);
        assert_eq!(packet.followups.len(), 1);
    }

    #[test]
    fn packet_round_trips_through_json() {
        let packet = build_packet("run-1", &phase_at_gate());

        let json = serde_json::to_string(&packet).unwrap();
        let decoded: ApprovalPacket = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, packet);
    }
}
