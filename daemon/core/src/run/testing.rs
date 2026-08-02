//! Test doubles for driving the pipeline without agent processes.
//!
//! [`ReactiveAgent`] behaves the way a real agent does: it reads the prompt,
//! finds the path it was told to write to, and writes a file there. That makes
//! the end-to-end tests exercise the actual prompts — a template that forgets to
//! name its output path fails the test instead of silently producing a run that
//! never converges.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use agentchat_protocol::run::{
    Disposition, DispositionAction, DispositionSet, RawFinding, RawReviewReport, FINDING_ID_LEN,
};
use tokio::sync::{mpsc, watch};

use crate::backend::{AgentBackend, AgentNotification, AgentPromptResult};

/// What the agent was asked to produce, inferred from the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Review(PathBuf),
    Disposition(PathBuf),
    Artifact(PathBuf),
}

/// An agent that answers whatever the prompt asks for.
pub struct ReactiveAgent {
    name: String,
    working_dir: PathBuf,
    /// Blocking problem slugs to raise, one entry per review round. Rounds past
    /// the end of the list raise nothing, which converges the phase.
    blocking_per_round: Vec<Vec<String>>,
    /// Advisory slugs raised every round.
    advisory: Vec<String>,
    /// When set, the author answers `disputed` instead of `accepted`.
    dispute_with: Option<String>,
    reviews_done: RefCell<usize>,
    pub prompts_seen: RefCell<Vec<String>>,
    health: watch::Sender<bool>,
}

impl ReactiveAgent {
    pub fn new(name: &str, working_dir: &Path) -> Rc<Self> {
        let (health, _) = watch::channel(true);
        Rc::new(Self {
            name: name.to_string(),
            working_dir: working_dir.to_path_buf(),
            blocking_per_round: Vec::new(),
            advisory: Vec::new(),
            dispute_with: None,
            reviews_done: RefCell::new(0),
            prompts_seen: RefCell::new(Vec::new()),
            health,
        })
    }

    /// Raises these problems in the given round, one `Vec` per round.
    pub fn raising(name: &str, working_dir: &Path, blocking_per_round: &[&[&str]]) -> Rc<Self> {
        let (health, _) = watch::channel(true);
        Rc::new(Self {
            name: name.to_string(),
            working_dir: working_dir.to_path_buf(),
            blocking_per_round: blocking_per_round
                .iter()
                .map(|round| round.iter().map(|slug| slug.to_string()).collect())
                .collect(),
            advisory: vec!["coverage".into()],
            dispute_with: None,
            reviews_done: RefCell::new(0),
            prompts_seen: RefCell::new(Vec::new()),
            health,
        })
    }

    /// Makes the author dispute every blocking finding instead of accepting it.
    pub fn disputing(name: &str, working_dir: &Path, reason: &str) -> Rc<Self> {
        let (health, _) = watch::channel(true);
        Rc::new(Self {
            name: name.to_string(),
            working_dir: working_dir.to_path_buf(),
            blocking_per_round: Vec::new(),
            advisory: Vec::new(),
            dispute_with: Some(reason.to_string()),
            reviews_done: RefCell::new(0),
            prompts_seen: RefCell::new(Vec::new()),
            health,
        })
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.working_dir.join(path)
        }
    }

    /// Finds what the prompt asked for, preferring the most specific target.
    fn target(&self, prompt: &str) -> Option<Target> {
        let quoted: Vec<&str> = prompt
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| token.contains('/') || token.ends_with(".md"))
            .collect();

        let review = quoted
            .iter()
            .find(|token| token.contains("/reviews/") && token.ends_with(".json"));
        if let Some(path) = review {
            return Some(Target::Review(self.resolve(path)));
        }

        let dispositions = quoted
            .iter()
            .find(|token| token.contains("/dispositions/") && token.ends_with(".json"));
        if let Some(path) = dispositions {
            return Some(Target::Disposition(self.resolve(path)));
        }

        // Author prompts name their inputs before their output, and the code
        // author's inputs include the plan. Prefer the phase's own artifact
        // rather than whichever path happens to appear first.
        let artifact = quoted
            .iter()
            .find(|token| token.ends_with("changes.md"))
            .or_else(|| quoted.iter().find(|token| token.ends_with("plan.md")));
        artifact.map(|path| Target::Artifact(self.resolve(path)))
    }

    fn review_body(&self, prompt: &str) -> String {
        let round_index = {
            let mut done = self.reviews_done.borrow_mut();
            let index = *done;
            *done += 1;
            index
        };
        let slugs = self
            .blocking_per_round
            .get(round_index)
            .cloned()
            .unwrap_or_default();

        serde_json::to_string(&RawReviewReport {
            reviewer: self.name.clone(),
            round: extract_round(prompt).unwrap_or(1),
            blocking: slugs
                .iter()
                .map(|slug| RawFinding {
                    category: "incorrect".into(),
                    location: format!("src/{slug}.rs:10"),
                    problem: format!("{slug} is handled incorrectly"),
                    evidence: format!("calling {slug}_handler with an empty input returns Ok(())"),
                    recommendation: format!("guard {slug}_handler against empty input"),
                })
                .collect(),
            non_blocking: self
                .advisory
                .iter()
                .map(|slug| RawFinding {
                    category: "test_gap".into(),
                    location: format!("src/{slug}.rs"),
                    problem: format!("{slug} has no regression test"),
                    ..RawFinding::default()
                })
                .collect(),
        })
        .expect("review serializes")
    }

    fn disposition_body(&self, prompt: &str) -> String {
        let (action, reason) = match &self.dispute_with {
            Some(reason) => (DispositionAction::Disputed, reason.clone()),
            None => (DispositionAction::Accepted, String::new()),
        };

        serde_json::to_string(&DispositionSet {
            round: extract_round(prompt).unwrap_or(1),
            dispositions: extract_finding_ids(prompt)
                .into_iter()
                .map(|finding_id| Disposition {
                    finding_id,
                    action,
                    reason: reason.clone(),
                    changed_files: Vec::new(),
                })
                .collect(),
        })
        .expect("dispositions serialize")
    }
}

/// Reads the round number out of "This is round N" or "round N".
fn extract_round(prompt: &str) -> Option<u32> {
    let index = prompt.find("round ")? + "round ".len();
    prompt[index..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// Reads the finding ids the author was told to answer.
///
/// They arrive as list items shaped `` - `id` [category] `location` ``. The
/// surrounding template uses the same list-and-backtick shape for its rules, so
/// candidates must also look like a finding id — otherwise the agent "answers"
/// a rule and the gate rightly rejects it for naming a finding nobody raised.
fn extract_finding_ids(prompt: &str) -> Vec<String> {
    prompt
        .lines()
        .filter_map(|line| line.trim().strip_prefix("- `"))
        .filter_map(|rest| rest.split('`').next())
        .filter(|id| id.len() == FINDING_ID_LEN && id.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
        .collect()
}

#[async_trait::async_trait(?Send)]
impl AgentBackend for ReactiveAgent {
    async fn initialize(&self) -> Result<(), String> {
        Ok(())
    }

    async fn new_session(&self, _cwd: PathBuf) -> Result<String, String> {
        Ok(format!("session-{}", self.name))
    }

    async fn prompt(&self, _session: String, text: String) -> Result<AgentPromptResult, String> {
        self.prompts_seen.borrow_mut().push(text.clone());

        let Some(target) = self.target(&text) else {
            return Ok(AgentPromptResult::new("end_turn"));
        };

        let (path, body) = match target {
            Target::Review(path) => {
                let body = self.review_body(&text);
                (path, body)
            }
            Target::Disposition(path) => {
                let body = self.disposition_body(&text);
                (path, body)
            }
            Target::Artifact(path) => (path, format!("# Written by {}\n", self.name)),
        };

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        tokio::fs::write(&path, body)
            .await
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_review_target_wins_over_other_quoted_paths() {
        let agent = ReactiveAgent::new("opus", Path::new("/work"));
        let prompt = "Plan: `.agentchat/runs/r1/plan.md`\nWrite to `.agentchat/runs/r1/plan/reviews/r1/opus.json`";

        assert_eq!(
            agent.target(prompt),
            Some(Target::Review(PathBuf::from(
                "/work/.agentchat/runs/r1/plan/reviews/r1/opus.json"
            )))
        );
    }

    #[test]
    fn the_artifact_is_the_target_when_nothing_more_specific_appears() {
        let agent = ReactiveAgent::new("planner", Path::new("/work"));
        let prompt = "Read `.agentchat/runs/r1/brief.md` and write `.agentchat/runs/r1/plan.md`";

        assert_eq!(
            agent.target(prompt),
            Some(Target::Artifact(PathBuf::from(
                "/work/.agentchat/runs/r1/plan.md"
            )))
        );
    }

    #[test]
    fn the_code_author_writes_changes_not_the_plan_it_was_given() {
        let agent = ReactiveAgent::new("implementer", Path::new("/work"));
        let prompt = "Plan: `.agentchat/runs/r1/plan.md`\nWrite `.agentchat/runs/r1/changes.md`";

        assert_eq!(
            agent.target(prompt),
            Some(Target::Artifact(PathBuf::from(
                "/work/.agentchat/runs/r1/changes.md"
            )))
        );
    }

    #[test]
    fn round_numbers_come_from_the_prompt() {
        assert_eq!(extract_round("This is round 3.\n"), Some(3));
        assert_eq!(extract_round("no number here"), None);
    }

    #[test]
    fn finding_ids_are_read_from_the_authors_task_list() {
        let prompt = "Blocking findings you must answer:\n\n\
             - `abc123def456` [incorrect] `src/alpha.rs:10`\n  problem: x\n\
             - `0011aabbccdd` [security] `src/beta.rs:4`\n";

        assert_eq!(
            extract_finding_ids(prompt),
            vec!["abc123def456".to_string(), "0011aabbccdd".to_string()]
        );
    }

    #[test]
    fn locations_are_not_mistaken_for_finding_ids() {
        let prompt = "- `src/alpha.rs:10`\n";

        assert!(extract_finding_ids(prompt).is_empty());
    }

    #[test]
    fn the_templates_own_bulleted_rules_are_not_mistaken_for_findings() {
        // The disposition template explains itself using the same shape.
        let prompt = "- `accepted` means you changed the artifact.\n\
                      - `disputed` means you believe the reviewer is wrong.\n";

        assert!(extract_finding_ids(prompt).is_empty());
    }
}
