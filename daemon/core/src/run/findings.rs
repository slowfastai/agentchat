//! Validation, identity, and grouping for reviewer findings.
//!
//! The validator is deliberately mechanical: no model is consulted, and no
//! reviewer is ever asked to try again over a severity disagreement. A finding
//! that overstates its severity is *demoted*, which is free and deterministic —
//! a reviewer that inflates severity loses influence rather than costing money.

use std::collections::{BTreeMap, BTreeSet};

use agentchat_protocol::run::{
    finding_id, BlockingCategory, DemotionReason, Finding, FindingGroup, FindingSeverity,
    NonBlockingCategory, RawFinding, RawReviewReport, UNSPECIFIED_FILE,
};

/// How many blocking findings a single report may carry.
///
/// Uncapped reviewers produce an unbounded tail of nitpicks, and nitpicks are
/// what stop these loops from converging.
pub const MAX_BLOCKING_PER_REPORT: usize = 5;

/// Minimum evidence length, in characters, for a blocking finding.
pub const MIN_EVIDENCE_CHARS: usize = 20;

/// Lowercases and collapses whitespace.
///
/// Text with no whitespace at all, such as Chinese prose, passes through with
/// only the case fold — which is correct, since identity here only needs to
/// survive a verbatim round trip.
pub fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Extracts the grouping key from a reviewer's `location` string.
///
/// Strips a trailing `:88` or `:88-92` line reference and lowercases the path.
/// Case folding costs a vanishingly rare false merge between paths differing
/// only in case, and buys robustness against models that recapitalise paths.
pub fn normalize_file(location: &str) -> String {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return UNSPECIFIED_FILE.to_string();
    }

    let path = match trimmed.rsplit_once(':') {
        Some((head, tail))
            if tail.chars().any(|c| c.is_ascii_digit())
                && tail.chars().all(|c| c.is_ascii_digit() || c == '-') =>
        {
            head
        }
        _ => trimmed,
    };

    let path = path.trim();
    if path.is_empty() {
        UNSPECIFIED_FILE.to_string()
    } else {
        path.to_lowercase()
    }
}

/// Whether evidence is too weak to support a blocking claim.
fn evidence_is_weak(problem: &str, evidence: &str) -> bool {
    let normalized = normalize(evidence);
    normalized.is_empty()
        || normalized.chars().count() < MIN_EVIDENCE_CHARS
        || normalized == normalize(problem)
        || !has_concrete_reference(evidence)
}

/// Whether evidence points at something specific rather than restating a worry.
///
/// A path separator, a path-like token, a `::` path, a backticked identifier, a
/// snake_case identifier, or any digit all count.
fn has_concrete_reference(evidence: &str) -> bool {
    evidence.contains('/')
        || evidence.contains("::")
        || evidence.contains('`')
        || evidence.contains('_')
        || evidence.chars().any(|c| c.is_ascii_digit())
        || evidence
            .split_whitespace()
            .any(|token| token.len() > 3 && token.contains('.'))
}

/// Validates one reviewer report into canonical findings.
///
/// Entries in `blocking` stay blocking only if they name a [`BlockingCategory`],
/// carry concrete evidence, and fall within [`MAX_BLOCKING_PER_REPORT`]; the
/// limit counts findings that actually survived, so early demotions do not
/// penalise later entries. Everything else lands in the non-blocking set with
/// `demoted_from` recording why.
pub fn validate_report(report: &RawReviewReport) -> Vec<Finding> {
    let mut findings = Vec::with_capacity(report.blocking.len() + report.non_blocking.len());
    let mut kept_blocking = 0usize;

    for raw in &report.blocking {
        let parsed = BlockingCategory::parse(&raw.category);
        let demotion = match parsed {
            None => Some(DemotionReason::UnknownCategory),
            Some(_) if evidence_is_weak(&raw.problem, &raw.evidence) => {
                Some(DemotionReason::WeakEvidence)
            }
            Some(_) if kept_blocking >= MAX_BLOCKING_PER_REPORT => {
                Some(DemotionReason::OverBlockingLimit)
            }
            Some(_) => None,
        };

        let severity = match (demotion, parsed) {
            (None, Some(category)) => {
                kept_blocking += 1;
                FindingSeverity::Blocking { category }
            }
            _ => FindingSeverity::NonBlocking {
                category: NonBlockingCategory::Other,
            },
        };

        findings.push(build_finding(report, raw, severity, demotion));
    }

    for raw in &report.non_blocking {
        let severity = FindingSeverity::NonBlocking {
            category: NonBlockingCategory::parse_or_other(&raw.category),
        };
        findings.push(build_finding(report, raw, severity, None));
    }

    findings
}

fn build_finding(
    report: &RawReviewReport,
    raw: &RawFinding,
    severity: FindingSeverity,
    demoted_from: Option<DemotionReason>,
) -> Finding {
    let file = normalize_file(&raw.location);
    Finding {
        finding_id: finding_id(&file, severity.category_str(), &normalize(&raw.problem)),
        reviewer: report.reviewer.clone(),
        round: report.round,
        severity,
        file,
        location: raw.location.clone(),
        problem: raw.problem.clone(),
        evidence: raw.evidence.clone(),
        recommendation: raw.recommendation.clone(),
        demoted_from,
    }
}

/// Validates several reports from one round into a single finding list.
pub fn validate_round(reports: &[RawReviewReport]) -> Vec<Finding> {
    reports.iter().flat_map(validate_report).collect()
}

/// Groups findings by file and category, counting distinct reviewers.
///
/// Ordering is deterministic: blocking groups first, then higher consensus,
/// then file and category alphabetically.
pub fn group_findings(findings: &[Finding]) -> Vec<FindingGroup> {
    let mut buckets: BTreeMap<(String, bool, &'static str), Vec<Finding>> = BTreeMap::new();

    for finding in findings {
        buckets
            .entry((
                finding.file.clone(),
                finding.is_blocking(),
                finding.severity.category_str(),
            ))
            .or_default()
            .push(finding.clone());
    }

    let mut groups: Vec<FindingGroup> = buckets
        .into_values()
        .map(|findings| {
            let consensus = findings
                .iter()
                .map(|finding| finding.reviewer.as_str())
                .collect::<BTreeSet<_>>()
                .len();
            FindingGroup {
                file: findings[0].file.clone(),
                severity: findings[0].severity,
                consensus,
                findings,
            }
        })
        .collect();

    groups.sort_by(|a, b| {
        b.severity
            .is_blocking()
            .cmp(&a.severity.is_blocking())
            .then(b.consensus.cmp(&a.consensus))
            .then(a.file.cmp(&b.file))
            .then(a.severity.category_str().cmp(b.severity.category_str()))
    });

    groups
}

/// Blocking findings in `current` that were not blocking in `previous`.
///
/// This is the quantity the cycle budget watches: a round that surfaces nothing
/// new has converged, and a round that surfaces at least as much as the one
/// before it is churning.
pub fn new_blocking_since(previous: &[Finding], current: &[Finding]) -> Vec<Finding> {
    let seen: BTreeSet<&str> = previous
        .iter()
        .filter(|finding| finding.is_blocking())
        .map(|finding| finding.finding_id.as_str())
        .collect();

    let mut emitted: BTreeSet<String> = BTreeSet::new();
    current
        .iter()
        .filter(|finding| finding.is_blocking())
        .filter(|finding| !seen.contains(finding.finding_id.as_str()))
        .filter(|finding| emitted.insert(finding.finding_id.clone()))
        .cloned()
        .collect()
}

/// Blocking findings, deduplicated by id, preserving order.
pub fn blocking_findings(findings: &[Finding]) -> Vec<Finding> {
    let mut seen = BTreeSet::new();
    findings
        .iter()
        .filter(|finding| finding.is_blocking())
        .filter(|finding| seen.insert(finding.finding_id.clone()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(category: &str, location: &str, problem: &str, evidence: &str) -> RawFinding {
        RawFinding {
            category: category.into(),
            location: location.into(),
            problem: problem.into(),
            evidence: evidence.into(),
            recommendation: "fix it".into(),
        }
    }

    fn strong_evidence() -> &'static str {
        "with max_cycles=2 the ledger admits a third revision in budget.rs"
    }

    fn report(reviewer: &str, blocking: Vec<RawFinding>) -> RawReviewReport {
        RawReviewReport {
            reviewer: reviewer.into(),
            round: 1,
            blocking,
            non_blocking: Vec::new(),
        }
    }

    #[test]
    fn normalize_file_strips_line_and_range_references() {
        assert_eq!(
            normalize_file("core/src/Run/budget.rs:88"),
            "core/src/run/budget.rs"
        );
        assert_eq!(
            normalize_file("core/src/run/budget.rs:88-92"),
            "core/src/run/budget.rs"
        );
        assert_eq!(
            normalize_file("core/src/run/budget.rs"),
            "core/src/run/budget.rs"
        );
        assert_eq!(normalize_file("  "), UNSPECIFIED_FILE);
    }

    #[test]
    fn normalize_file_keeps_non_numeric_suffixes() {
        assert_eq!(normalize_file("fn:handle_prompt"), "fn:handle_prompt");
    }

    #[test]
    fn well_formed_blocking_finding_survives() {
        let findings = validate_report(&report(
            "opus",
            vec![raw(
                "incorrect",
                "core/src/run/budget.rs:88",
                "cap is off by one",
                strong_evidence(),
            )],
        ));

        assert_eq!(findings.len(), 1);
        assert!(findings[0].is_blocking());
        assert_eq!(findings[0].severity.category_str(), "incorrect");
        assert_eq!(findings[0].demoted_from, None);
        assert_eq!(findings[0].file, "core/src/run/budget.rs");
    }

    #[test]
    fn unknown_blocking_category_is_demoted() {
        let findings = validate_report(&report(
            "deepseek",
            vec![raw(
                "high",
                "core/src/run/budget.rs",
                "looks risky",
                strong_evidence(),
            )],
        ));

        assert!(!findings[0].is_blocking());
        assert_eq!(
            findings[0].demoted_from,
            Some(DemotionReason::UnknownCategory)
        );
        assert_eq!(findings[0].severity.category_str(), "other");
    }

    #[test]
    fn weak_evidence_is_demoted() {
        let cases = [
            ("", "empty"),
            ("it is wrong", "too short and no reference"),
            (
                "this could plausibly cause a serious problem later",
                "no concrete reference",
            ),
        ];

        for (evidence, label) in cases {
            let findings = validate_report(&report(
                "luna",
                vec![raw(
                    "incorrect",
                    "core/src/run/budget.rs",
                    "cap is off by one",
                    evidence,
                )],
            ));
            assert!(!findings[0].is_blocking(), "expected demotion for {label}");
            assert_eq!(
                findings[0].demoted_from,
                Some(DemotionReason::WeakEvidence),
                "expected weak evidence for {label}"
            );
        }
    }

    #[test]
    fn evidence_restating_the_problem_is_demoted() {
        let text = "the cycle ledger admits a third revision in budget.rs";
        let findings = validate_report(&report(
            "opus",
            vec![raw("incorrect", "core/src/run/budget.rs", text, text)],
        ));

        assert_eq!(findings[0].demoted_from, Some(DemotionReason::WeakEvidence));
    }

    #[test]
    fn blocking_findings_beyond_the_limit_are_demoted() {
        let blocking: Vec<RawFinding> = (0..8)
            .map(|i| {
                raw(
                    "incorrect",
                    &format!("core/src/run/file{i}.rs"),
                    &format!("problem {i}"),
                    strong_evidence(),
                )
            })
            .collect();

        let findings = validate_report(&report("opus", blocking));

        assert_eq!(
            findings.iter().filter(|f| f.is_blocking()).count(),
            MAX_BLOCKING_PER_REPORT
        );
        assert_eq!(
            findings[MAX_BLOCKING_PER_REPORT].demoted_from,
            Some(DemotionReason::OverBlockingLimit)
        );
    }

    #[test]
    fn the_blocking_limit_counts_survivors_not_entries() {
        // Two weak entries up front must not consume the blocking allowance.
        let mut blocking = vec![
            raw("incorrect", "core/src/a.rs", "weak one", "short"),
            raw("incorrect", "core/src/b.rs", "weak two", "short"),
        ];
        blocking.extend((0..5).map(|i| {
            raw(
                "incorrect",
                &format!("core/src/ok{i}.rs"),
                &format!("problem {i}"),
                strong_evidence(),
            )
        }));

        let findings = validate_report(&report("opus", blocking));

        assert_eq!(findings.iter().filter(|f| f.is_blocking()).count(), 5);
    }

    #[test]
    fn non_blocking_entries_keep_their_category_and_are_not_demoted() {
        let findings = validate_report(&RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: Vec::new(),
            non_blocking: vec![
                raw(
                    "test_gap",
                    "core/src/run/budget.rs",
                    "no concurrency test",
                    "",
                ),
                raw("nitpick", "core/src/run/budget.rs", "rename this", ""),
            ],
        });

        assert_eq!(findings[0].severity.category_str(), "test_gap");
        assert_eq!(findings[1].severity.category_str(), "other");
        assert!(findings.iter().all(|f| f.demoted_from.is_none()));
        assert!(findings.iter().all(|f| !f.is_blocking()));
    }

    #[test]
    fn identical_text_from_different_reviewers_shares_an_id() {
        let a = validate_report(&report(
            "opus",
            vec![raw(
                "incorrect",
                "core/src/a.rs:10",
                "cap is off by one",
                strong_evidence(),
            )],
        ));
        // Same problem, different line reference and casing in the path.
        let b = validate_report(&report(
            "luna",
            vec![raw(
                "incorrect",
                "core/src/A.rs:99",
                "Cap is off  by one",
                strong_evidence(),
            )],
        ));

        assert_eq!(a[0].finding_id, b[0].finding_id);
    }

    #[test]
    fn different_file_or_category_yields_a_different_id() {
        let base = validate_report(&report(
            "opus",
            vec![raw(
                "incorrect",
                "core/src/a.rs",
                "cap is off by one",
                strong_evidence(),
            )],
        ));
        let other_file = validate_report(&report(
            "opus",
            vec![raw(
                "incorrect",
                "core/src/b.rs",
                "cap is off by one",
                strong_evidence(),
            )],
        ));
        let other_category = validate_report(&report(
            "opus",
            vec![raw(
                "security",
                "core/src/a.rs",
                "cap is off by one",
                strong_evidence(),
            )],
        ));

        assert_ne!(base[0].finding_id, other_file[0].finding_id);
        assert_ne!(base[0].finding_id, other_category[0].finding_id);
    }

    #[test]
    fn grouping_counts_distinct_reviewers() {
        let reports: Vec<RawReviewReport> = ["opus", "luna", "deepseek"]
            .iter()
            .map(|reviewer| RawReviewReport {
                reviewer: (*reviewer).into(),
                round: 1,
                blocking: Vec::new(),
                non_blocking: vec![raw(
                    "test_gap",
                    "core/src/run/budget.rs",
                    &format!("{reviewer} phrases it differently"),
                    "",
                )],
            })
            .collect();

        let groups = group_findings(&validate_round(&reports));

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].consensus, 3);
        assert_eq!(groups[0].findings.len(), 3);
        // Grouping preserves each reviewer's wording rather than merging it.
        assert_eq!(
            groups[0]
                .findings
                .iter()
                .filter(|f| f.problem.contains("opus"))
                .count(),
            1
        );
    }

    #[test]
    fn one_reviewer_reporting_twice_does_not_inflate_consensus() {
        let groups = group_findings(&validate_report(&RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: Vec::new(),
            non_blocking: vec![
                raw("test_gap", "core/src/run/budget.rs", "one", ""),
                raw("test_gap", "core/src/run/budget.rs", "two", ""),
            ],
        }));

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].consensus, 1);
        assert_eq!(groups[0].findings.len(), 2);
    }

    #[test]
    fn grouping_sorts_blocking_first_then_by_consensus() {
        let mut reports = vec![RawReviewReport {
            reviewer: "opus".into(),
            round: 1,
            blocking: vec![raw(
                "incorrect",
                "core/src/z.rs",
                "blocking one",
                strong_evidence(),
            )],
            non_blocking: vec![raw("style", "core/src/a.rs", "solo nit", "")],
        }];
        for reviewer in ["luna", "deepseek"] {
            reports.push(RawReviewReport {
                reviewer: reviewer.into(),
                round: 1,
                blocking: Vec::new(),
                non_blocking: vec![raw("test_gap", "core/src/m.rs", "shared nit", "")],
            });
        }

        let groups = group_findings(&validate_round(&reports));

        assert!(groups[0].severity.is_blocking());
        assert_eq!(groups[1].consensus, 2);
        assert_eq!(groups[2].consensus, 1);
    }

    #[test]
    fn new_blocking_since_ignores_repeats_and_non_blocking() {
        let round1 = validate_report(&report(
            "opus",
            vec![raw(
                "incorrect",
                "core/src/a.rs",
                "problem one",
                strong_evidence(),
            )],
        ));
        let round2 = validate_report(&report(
            "opus",
            vec![
                raw(
                    "incorrect",
                    "core/src/a.rs",
                    "problem one",
                    strong_evidence(),
                ),
                raw(
                    "incorrect",
                    "core/src/b.rs",
                    "problem two",
                    strong_evidence(),
                ),
            ],
        ));

        let fresh = new_blocking_since(&round1, &round2);

        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].file, "core/src/b.rs");
    }

    #[test]
    fn new_blocking_since_deduplicates_across_reviewers() {
        let reports: Vec<RawReviewReport> = ["opus", "luna"]
            .iter()
            .map(|reviewer| {
                report(
                    reviewer,
                    vec![raw(
                        "incorrect",
                        "core/src/a.rs",
                        "same problem",
                        strong_evidence(),
                    )],
                )
            })
            .collect();

        let fresh = new_blocking_since(&[], &validate_round(&reports));

        assert_eq!(fresh.len(), 1);
    }
}
