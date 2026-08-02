//! Prompt templates for each stage.
//!
//! Reviewer prompt discipline decides whether these loops converge more than
//! model choice does. The same model told to "find anything that could be
//! improved" and told to "report only what makes this incorrect, at most five,
//! each with a concrete reference" behaves an order of magnitude differently,
//! because an unbounded nitpick tail is exactly what keeps a review loop from
//! terminating.
//!
//! Templates therefore live as data, not string literals buried in the
//! executor: they are the thing you will spend the most time tuning, so they
//! must be overridable per project and diffable across runs. Drop a file named
//! after the stage into `.agentchat/prompts/` to replace one.

use std::collections::HashMap;
use std::path::Path;

use agentchat_protocol::run::PhaseKind;

use crate::run::findings::MAX_BLOCKING_PER_REPORT;

/// Which stage a template belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptKind {
    /// Produce the first version of the artifact.
    Author,
    /// Review the current version.
    Review,
    /// Answer this round's blocking findings and revise.
    Disposition,
}

impl PromptKind {
    /// File stem used for a per-project override, e.g. `plan_review.md`.
    pub fn file_stem(&self, phase: PhaseKind) -> String {
        let stage = match self {
            Self::Author => "author",
            Self::Review => "review",
            Self::Disposition => "disposition",
        };
        format!("{}_{stage}", phase.as_str())
    }
}

/// Templates for every stage, with per-project overrides applied.
#[derive(Debug, Clone)]
pub struct PromptSet {
    templates: HashMap<(PhaseKind, PromptKind), String>,
}

impl Default for PromptSet {
    fn default() -> Self {
        Self::builtin()
    }
}

impl PromptSet {
    pub fn builtin() -> Self {
        let mut templates = HashMap::new();
        templates.insert(
            (PhaseKind::Plan, PromptKind::Author),
            PLAN_AUTHOR.to_string(),
        );
        templates.insert(
            (PhaseKind::Plan, PromptKind::Review),
            PLAN_REVIEW.to_string(),
        );
        templates.insert(
            (PhaseKind::Plan, PromptKind::Disposition),
            DISPOSITION.to_string(),
        );
        templates.insert(
            (PhaseKind::Code, PromptKind::Author),
            CODE_AUTHOR.to_string(),
        );
        templates.insert(
            (PhaseKind::Code, PromptKind::Review),
            CODE_REVIEW.to_string(),
        );
        templates.insert(
            (PhaseKind::Code, PromptKind::Disposition),
            DISPOSITION.to_string(),
        );
        Self { templates }
    }

    /// Replaces built-in templates with any `<phase>_<stage>.md` files found.
    ///
    /// A missing directory is not an error: overrides are opt-in.
    pub async fn load_overrides(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut loaded = Vec::new();
        for phase in [PhaseKind::Plan, PhaseKind::Code] {
            for kind in [
                PromptKind::Author,
                PromptKind::Review,
                PromptKind::Disposition,
            ] {
                let stem = kind.file_stem(phase);
                let path = dir.join(format!("{stem}.md"));
                if !path.is_file() {
                    continue;
                }
                let body = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| format!("failed to read prompt {}: {e}", path.display()))?;
                self.templates.insert((phase, kind), body);
                loaded.push(stem);
            }
        }
        Ok(loaded)
    }

    pub fn template(&self, phase: PhaseKind, kind: PromptKind) -> &str {
        self.templates
            .get(&(phase, kind))
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// Substitutes `{name}` placeholders. Unknown placeholders are left alone
    /// so a typo in a template shows up in the prompt instead of vanishing.
    pub fn render(&self, phase: PhaseKind, kind: PromptKind, vars: &[(&str, &str)]) -> String {
        let mut out = self.template(phase, kind).to_string();
        for (name, value) in vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out = out.replace("{max_blocking}", &MAX_BLOCKING_PER_REPORT.to_string());
        out
    }
}

const PLAN_AUTHOR: &str = r#"You are the planner for this change.

Read the brief at `{brief_path}`, explore the repository as much as you need,
and write an implementation plan to `{artifact_path}`.

The plan must be specific enough to implement from: name the files to create or
modify, the functions and types involved, and the order of work. State the
assumptions you are making and what you deliberately left out of scope.

Do not implement anything yet. Write only the plan file.
"#;

const PLAN_REVIEW: &str = r#"You are reviewing an implementation plan. This is round {round}.

Brief: `{brief_path}`
Plan:  `{artifact_path}`

Read both, and read whatever code you need to judge whether the plan is correct.

Write your review as JSON to `{output_path}`, exactly this shape:

{
  "reviewer": "{reviewer}",
  "round": {round},
  "blocking": [
    {
      "category": "contradicts_brief | missing_requirement | incorrect | breaks_existing | security | data_loss",
      "location": "path/to/file.rs:88",
      "problem": "what is wrong",
      "evidence": "concrete support: the brief clause contradicted, the input that produces wrong behaviour, the existing code this breaks",
      "recommendation": "what to do instead"
    }
  ],
  "non_blocking": [
    { "category": "style | naming | perf_hint | test_gap | refactor | readability", "location": "...", "problem": "...", "evidence": "", "recommendation": "..." }
  ]
}

Rules for `blocking`, which are enforced mechanically:

- At most {max_blocking} entries. Extra entries are silently downgraded.
- Only the six categories listed. Anything else is downgraded.
- `evidence` must reference something concrete — a file path, an identifier, a
  literal value, a clause of the brief. Restating the problem in other words is
  downgraded, and so is a bare assertion that something "could" go wrong.
- A finding belongs in `blocking` only if the plan as written produces incorrect
  behaviour or fails the brief. Anything about taste, structure, naming, or
  future-proofing goes in `non_blocking`, no matter how strongly you feel.

`non_blocking` has no limit and creates no obligation: the author may ignore it
entirely. Put your honest suggestions there anyway — they are shown to the human
and become follow-up work.

{previous_findings}

Write the file. Do not print the JSON in your reply.
"#;

const CODE_AUTHOR: &str = r#"You are implementing an approved plan.

Brief: `{brief_path}`
Plan:  `{plan_path}`

The plan is frozen. Implement it in the working tree, then run the project's
tests, linter, and build, and fix what you broke.

If you find that the plan itself is wrong and cannot be implemented as written,
stop and write `{artifact_path}` explaining the conflict rather than quietly
choosing a different design — a plan change needs human approval.

Otherwise write `{artifact_path}` summarising what you changed, which files you
touched, and the result of the tests you ran.
"#;

const CODE_REVIEW: &str = r#"You are reviewing an implementation. This is round {round}.

Brief:   `{brief_path}`
Plan:    `{plan_path}`
Changes: `{artifact_path}`

Read the summary, then read the actual diff and the surrounding code. Run the
tests yourself if you doubt the reported result.

Write your review as JSON to `{output_path}`, exactly this shape:

{
  "reviewer": "{reviewer}",
  "round": {round},
  "blocking": [
    {
      "category": "contradicts_brief | missing_requirement | incorrect | breaks_existing | security | data_loss",
      "location": "path/to/file.rs:88",
      "problem": "what is wrong",
      "evidence": "an input that produces the wrong result, a test that fails, the plan clause not implemented",
      "recommendation": "what to do instead"
    }
  ],
  "non_blocking": [
    { "category": "style | naming | perf_hint | test_gap | refactor | readability", "location": "...", "problem": "...", "evidence": "", "recommendation": "..." }
  ]
}

Rules for `blocking`, which are enforced mechanically:

- At most {max_blocking} entries. Extra entries are silently downgraded.
- Only the six categories listed. Anything else is downgraded.
- `evidence` must reference something concrete. The strongest evidence is a
  failing test you actually ran; the weakest is a worry about what might happen.
  Bare assertions are downgraded to advisory.
- A finding belongs in `blocking` only if the code is wrong, breaks something
  that worked, or does not deliver the plan. Everything else is `non_blocking`.

Do not report the absence of tests as blocking; that is `test_gap`, advisory.

{previous_findings}

Write the file. Do not print the JSON in your reply.
"#;

const DISPOSITION: &str = r#"Reviewers have finished round {round}. You must answer every blocking finding
before you may revise.

{findings}

Write your answers as JSON to `{output_path}`:

{
  "round": {round},
  "dispositions": [
    { "finding_id": "...", "action": "accepted", "reason": "", "changed_files": ["path/to/file.rs"] },
    { "finding_id": "...", "action": "disputed", "reason": "why the reviewer is wrong", "changed_files": [] }
  ]
}

Rules:

- Every blocking finding needs an entry. A missing one sends this straight back
  to you and gets you no closer to done.
- `accepted` means you changed the artifact in response. Make the change.
- `disputed` means you believe the reviewer is wrong, and requires a real
  argument. A disputed finding is re-checked once; if the reviewer still holds,
  it goes to the human with both sides shown. Disputing is legitimate — do it
  when you are right, and say why.
- Advisory findings need no entry at all. Answer one only if you adopted it, or
  if you want the human to see your reasoning for passing on it.

Then revise `{artifact_path}` to reflect everything you accepted.

{feedback}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_a_builtin_template() {
        let prompts = PromptSet::builtin();

        for phase in [PhaseKind::Plan, PhaseKind::Code] {
            for kind in [
                PromptKind::Author,
                PromptKind::Review,
                PromptKind::Disposition,
            ] {
                assert!(
                    !prompts.template(phase, kind).is_empty(),
                    "missing template for {:?}/{:?}",
                    phase,
                    kind
                );
            }
        }
    }

    #[test]
    fn render_substitutes_placeholders() {
        let prompts = PromptSet::builtin();

        let rendered = prompts.render(
            PhaseKind::Plan,
            PromptKind::Review,
            &[
                ("round", "2"),
                ("brief_path", "runs/run-1/brief.md"),
                ("artifact_path", "runs/run-1/plan.md"),
                ("output_path", "runs/run-1/plan/reviews/r2/opus.json"),
                ("reviewer", "opus"),
                ("previous_findings", "Round 1 raised: alpha"),
            ],
        );

        assert!(rendered.contains("This is round 2"));
        assert!(rendered.contains("runs/run-1/plan/reviews/r2/opus.json"));
        assert!(rendered.contains("Round 1 raised: alpha"));
        assert!(!rendered.contains("{round}"));
        assert!(!rendered.contains("{output_path}"));
    }

    #[test]
    fn the_blocking_limit_comes_from_the_validator() {
        let prompts = PromptSet::builtin();

        let rendered = prompts.render(PhaseKind::Plan, PromptKind::Review, &[]);

        assert!(rendered.contains(&format!("At most {MAX_BLOCKING_PER_REPORT} entries")));
        assert!(!rendered.contains("{max_blocking}"));
    }

    #[test]
    fn unknown_placeholders_survive_rendering() {
        let mut prompts = PromptSet::builtin();
        prompts.templates.insert(
            (PhaseKind::Plan, PromptKind::Author),
            "see {typoed_name}".into(),
        );

        let rendered = prompts.render(PhaseKind::Plan, PromptKind::Author, &[("round", "1")]);

        assert_eq!(rendered, "see {typoed_name}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_missing_override_directory_is_not_an_error() {
        let mut prompts = PromptSet::builtin();

        let loaded = prompts
            .load_overrides(Path::new("/nonexistent/prompts"))
            .await
            .unwrap();

        assert!(loaded.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn overrides_replace_only_the_files_present() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("plan_review.md"), "custom review prompt")
            .await
            .unwrap();
        let mut prompts = PromptSet::builtin();
        let builtin_author = prompts
            .template(PhaseKind::Plan, PromptKind::Author)
            .to_string();

        let loaded = prompts.load_overrides(dir.path()).await.unwrap();

        assert_eq!(loaded, vec!["plan_review".to_string()]);
        assert_eq!(
            prompts.template(PhaseKind::Plan, PromptKind::Review),
            "custom review prompt"
        );
        assert_eq!(
            prompts.template(PhaseKind::Plan, PromptKind::Author),
            builtin_author
        );
    }

    #[test]
    fn review_prompts_state_the_mechanical_rules() {
        let prompts = PromptSet::builtin();

        for phase in [PhaseKind::Plan, PhaseKind::Code] {
            let rendered = prompts.render(phase, PromptKind::Review, &[]);
            assert!(rendered.contains("downgraded"), "{phase:?} omits demotion");
            assert!(
                rendered.contains("evidence"),
                "{phase:?} omits evidence rule"
            );
            assert!(
                rendered.contains("non_blocking"),
                "{phase:?} omits the advisory escape hatch"
            );
        }
    }
}
