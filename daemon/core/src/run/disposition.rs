//! The disposition gate.
//!
//! Before a cycle counts as complete, the author must have answered every
//! blocking finding — accepting it, or disputing it with an argument. This is
//! what makes the loop converge: rounds shrink to the disputed set rather than
//! re-litigating everything, and an author cannot quietly ignore a finding.
//!
//! Non-blocking findings carry no such obligation. Requiring the author to
//! answer thirty nitpicks would burn a turn producing text nobody reads, and
//! advisory suggestions are not supposed to create work. Anything the author
//! does not mention is implicitly declined and flows to the follow-up list.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use agentchat_protocol::run::{Disposition, DispositionAction, Finding};

use crate::run::findings::blocking_findings;

/// Outcome of the gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateResult {
    Pass,
    Reject(GateRejection),
}

impl GateResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass)
    }

    pub fn rejection(&self) -> Option<&GateRejection> {
        match self {
            Self::Pass => None,
            Self::Reject(rejection) => Some(rejection),
        }
    }
}

/// What the author still owes, in enough detail to feed straight back to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateRejection {
    /// Blocking findings with no disposition at all.
    pub missing: Vec<String>,
    /// Blocking findings marked `disputed` without an argument.
    pub disputed_without_reason: Vec<String>,
    /// Blocking findings marked `declined`, which is only valid for advisory
    /// findings.
    pub declined_blocking: Vec<String>,
    /// Dispositions naming a finding that does not exist in this round.
    pub unknown_finding_ids: Vec<String>,
}

impl GateRejection {
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty()
            && self.disputed_without_reason.is_empty()
            && self.declined_blocking.is_empty()
            && self.unknown_finding_ids.is_empty()
    }

    /// Renders the rejection as feedback for the author's retry prompt.
    pub fn feedback(&self) -> String {
        let mut out = String::new();
        let section = |title: &str, ids: &[String], out: &mut String| {
            if ids.is_empty() {
                return;
            }
            let _ = writeln!(out, "{title}: {}", ids.join(", "));
        };

        section(
            "Blocking findings with no disposition",
            &self.missing,
            &mut out,
        );
        section(
            "Disputed without a reason",
            &self.disputed_without_reason,
            &mut out,
        );
        section(
            "Blocking findings cannot be declined; accept or dispute them",
            &self.declined_blocking,
            &mut out,
        );
        section(
            "Dispositions for findings that were not raised this round",
            &self.unknown_finding_ids,
            &mut out,
        );

        out
    }
}

/// Checks the author's dispositions against this round's findings.
pub fn evaluate_gate(findings: &[Finding], dispositions: &[Disposition]) -> GateResult {
    let blocking = blocking_findings(findings);
    let blocking_ids: BTreeSet<&str> = blocking
        .iter()
        .map(|finding| finding.finding_id.as_str())
        .collect();
    let known_ids: BTreeSet<&str> = findings
        .iter()
        .map(|finding| finding.finding_id.as_str())
        .collect();

    let mut rejection = GateRejection::default();
    let mut answered: BTreeSet<&str> = BTreeSet::new();

    for disposition in dispositions {
        let id = disposition.finding_id.as_str();

        if !known_ids.contains(id) {
            push_once(&mut rejection.unknown_finding_ids, id);
            continue;
        }

        if !blocking_ids.contains(id) {
            // Advisory findings accept any action, including none at all.
            continue;
        }

        answered.insert(id);

        match disposition.action {
            DispositionAction::Accepted => {}
            DispositionAction::Disputed if disposition.reason.trim().is_empty() => {
                push_once(&mut rejection.disputed_without_reason, id);
            }
            DispositionAction::Disputed => {}
            DispositionAction::Declined => push_once(&mut rejection.declined_blocking, id),
        }
    }

    for finding in &blocking {
        if !answered.contains(finding.finding_id.as_str()) {
            push_once(&mut rejection.missing, &finding.finding_id);
        }
    }

    if rejection.is_empty() {
        GateResult::Pass
    } else {
        GateResult::Reject(rejection)
    }
}

fn push_once(target: &mut Vec<String>, id: &str) {
    if !target.iter().any(|existing| existing == id) {
        target.push(id.to_string());
    }
}

/// Blocking findings the author disputed, which carry into the next round for
/// exactly one re-check.
pub fn disputed_findings(findings: &[Finding], dispositions: &[Disposition]) -> Vec<Finding> {
    let disputed: BTreeSet<&str> = dispositions
        .iter()
        .filter(|disposition| disposition.action == DispositionAction::Disputed)
        .map(|disposition| disposition.finding_id.as_str())
        .collect();

    blocking_findings(findings)
        .into_iter()
        .filter(|finding| disputed.contains(finding.finding_id.as_str()))
        .collect()
}

/// Non-blocking findings the author did not adopt.
///
/// These are the follow-up candidates, and the discussion history the human
/// sees at the approval gate.
pub fn declined_advisory(findings: &[Finding], dispositions: &[Disposition]) -> Vec<Finding> {
    let accepted: BTreeSet<&str> = dispositions
        .iter()
        .filter(|disposition| disposition.action == DispositionAction::Accepted)
        .map(|disposition| disposition.finding_id.as_str())
        .collect();

    findings
        .iter()
        .filter(|finding| !finding.is_blocking())
        .filter(|finding| !accepted.contains(finding.finding_id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::run::{RawFinding, RawReviewReport};

    use super::*;
    use crate::run::findings::validate_report;

    const EVIDENCE: &str = "with max_cycles=2 the ledger admits a third revision in budget.rs";

    fn findings_with(blocking: usize, non_blocking: usize) -> Vec<Finding> {
        validate_report(&RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: (0..blocking)
                .map(|i| RawFinding {
                    category: "incorrect".into(),
                    location: format!("core/src/b{i}.rs"),
                    problem: format!("blocking problem {i}"),
                    evidence: EVIDENCE.into(),
                    recommendation: String::new(),
                })
                .collect(),
            non_blocking: (0..non_blocking)
                .map(|i| RawFinding {
                    category: "test_gap".into(),
                    location: format!("core/src/n{i}.rs"),
                    problem: format!("advisory {i}"),
                    ..RawFinding::default()
                })
                .collect(),
        })
    }

    fn disposition(finding: &Finding, action: DispositionAction, reason: &str) -> Disposition {
        Disposition {
            finding_id: finding.finding_id.clone(),
            action,
            reason: reason.into(),
            changed_files: Vec::new(),
        }
    }

    #[test]
    fn all_blocking_accepted_passes() {
        let findings = findings_with(2, 0);
        let dispositions: Vec<Disposition> = findings
            .iter()
            .map(|f| disposition(f, DispositionAction::Accepted, ""))
            .collect();

        assert_eq!(evaluate_gate(&findings, &dispositions), GateResult::Pass);
    }

    #[test]
    fn missing_blocking_disposition_is_listed() {
        let findings = findings_with(2, 0);
        let dispositions = vec![disposition(&findings[0], DispositionAction::Accepted, "")];

        let result = evaluate_gate(&findings, &dispositions);

        let rejection = result.rejection().expect("expected rejection");
        assert_eq!(rejection.missing, vec![findings[1].finding_id.clone()]);
        assert!(rejection.feedback().contains(&findings[1].finding_id));
    }

    #[test]
    fn dispute_without_a_reason_is_rejected() {
        let findings = findings_with(1, 0);
        let dispositions = vec![disposition(
            &findings[0],
            DispositionAction::Disputed,
            "   ",
        )];

        let rejection = evaluate_gate(&findings, &dispositions)
            .rejection()
            .cloned()
            .expect("expected rejection");

        assert_eq!(
            rejection.disputed_without_reason,
            vec![findings[0].finding_id.clone()]
        );
        assert!(rejection.missing.is_empty());
    }

    #[test]
    fn dispute_with_a_reason_passes() {
        let findings = findings_with(1, 0);
        let dispositions = vec![disposition(
            &findings[0],
            DispositionAction::Disputed,
            "the brief scopes this to v2",
        )];

        assert!(evaluate_gate(&findings, &dispositions).is_pass());
    }

    #[test]
    fn blocking_findings_cannot_be_declined() {
        let findings = findings_with(1, 0);
        let dispositions = vec![disposition(
            &findings[0],
            DispositionAction::Declined,
            "meh",
        )];

        let rejection = evaluate_gate(&findings, &dispositions)
            .rejection()
            .cloned()
            .expect("expected rejection");

        assert_eq!(
            rejection.declined_blocking,
            vec![findings[0].finding_id.clone()]
        );
    }

    #[test]
    fn non_blocking_findings_need_no_disposition() {
        let findings = findings_with(0, 3);

        assert!(evaluate_gate(&findings, &[]).is_pass());
    }

    #[test]
    fn disposition_for_an_unraised_finding_is_rejected() {
        let findings = findings_with(1, 0);
        let mut dispositions = vec![disposition(&findings[0], DispositionAction::Accepted, "")];
        dispositions.push(Disposition {
            finding_id: "ffffffffffff".into(),
            action: DispositionAction::Accepted,
            reason: String::new(),
            changed_files: Vec::new(),
        });

        let rejection = evaluate_gate(&findings, &dispositions)
            .rejection()
            .cloned()
            .expect("expected rejection");

        assert_eq!(
            rejection.unknown_finding_ids,
            vec!["ffffffffffff".to_string()]
        );
    }

    #[test]
    fn disputed_findings_carry_forward() {
        let findings = findings_with(2, 0);
        let dispositions = vec![
            disposition(&findings[0], DispositionAction::Accepted, ""),
            disposition(&findings[1], DispositionAction::Disputed, "out of scope"),
        ];

        let disputed = disputed_findings(&findings, &dispositions);

        assert_eq!(disputed.len(), 1);
        assert_eq!(disputed[0].finding_id, findings[1].finding_id);
    }

    #[test]
    fn unmentioned_advisory_findings_become_follow_ups() {
        let findings = findings_with(0, 3);
        let dispositions = vec![disposition(&findings[0], DispositionAction::Accepted, "")];

        let declined = declined_advisory(&findings, &dispositions);

        assert_eq!(declined.len(), 2);
        assert!(declined
            .iter()
            .all(|f| f.finding_id != findings[0].finding_id));
    }
}
