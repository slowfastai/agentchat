//! Live progress reporting for a run.
//!
//! A phase can take half an hour. Without this the terminal shows nothing
//! between "starting" and the approval gate, so there is no way to tell a run
//! that is working from one that is wedged — and the first thing you want when
//! a real model behaves unexpectedly is to see what it actually did.
//!
//! Agent chatter is condensed rather than echoed. Tool calls are the signal
//! that says work is happening; the prose is noise until something goes wrong,
//! and the full transcript is in the session log either way.

use std::cell::RefCell;
use std::collections::HashSet;

use agentchat_protocol::run::{PhaseKind, StageKind};

/// Something worth telling the operator about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent<'a> {
    /// A stage began.
    Stage {
        phase: PhaseKind,
        stage: StageKind,
        round: u32,
        roles: &'a str,
    },
    /// An agent started a tool call.
    Tool {
        role: &'a str,
        tool_call_id: &'a str,
        title: &'a str,
        status: &'a str,
    },
    /// How much an agent said during one turn.
    Turn {
        role: &'a str,
        text_chars: usize,
        thinking_chars: usize,
    },
    /// A reviewer produced a usable report.
    ReviewerFinished {
        role: &'a str,
        blocking: usize,
        advisory: usize,
    },
    /// A reviewer failed and was dropped from the round.
    ReviewerDropped {
        role: &'a str,
        reason: &'a str,
    },
    /// A step is being retried without spending cycle budget.
    Retry {
        role: &'a str,
        kind: &'a str,
        reason: &'a str,
    },
    /// A review round closed.
    Round {
        round: u32,
        new_blocking: usize,
        total_blocking: usize,
    },
    Note(&'a str),
}

/// Where progress goes.
pub trait ProgressSink {
    fn emit(&self, event: RunEvent<'_>);
}

/// Discards everything. The default, and what tests use.
pub struct SilentProgress;

impl ProgressSink for SilentProgress {
    fn emit(&self, _event: RunEvent<'_>) {}
}

/// Turns events into one-line summaries, suppressing repeats.
///
/// Shared by every sink so the terminal and the web console show the same
/// activity log rather than drifting apart.
pub struct EventFormatter {
    /// Tool calls already announced, so a call that streams several status
    /// updates is reported once.
    announced: RefCell<HashSet<String>>,
}

impl Default for EventFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventFormatter {
    pub fn new() -> Self {
        Self {
            announced: RefCell::new(HashSet::new()),
        }
    }

    /// Renders an event, or `None` when it adds nothing new.
    pub fn format(&self, event: &RunEvent<'_>) -> Option<String> {
        Some(match event {
            RunEvent::Stage {
                phase,
                stage,
                round,
                roles,
            } => {
                // A new stage makes earlier tool calls irrelevant.
                self.announced.borrow_mut().clear();
                format!(
                    "▶ {} · {} · round {round} · {roles}",
                    phase.as_str(),
                    stage.as_str()
                )
            }
            RunEvent::Tool {
                role,
                tool_call_id,
                title,
                status,
            } => {
                let key = format!("{role}/{tool_call_id}");
                if self.announced.borrow_mut().insert(key) {
                    format!("    {role} → {}", truncate(title, 72))
                } else if status.eq_ignore_ascii_case("failed") {
                    format!("    {role} ✗ {}", truncate(title, 72))
                } else {
                    return None;
                }
            }
            RunEvent::Turn {
                role,
                text_chars,
                thinking_chars,
            } => format!("    {role} · {text_chars} chars out, {thinking_chars} thinking"),
            RunEvent::ReviewerFinished {
                role,
                blocking,
                advisory,
            } => format!("  ✓ {role}: {blocking} blocking, {advisory} advisory"),
            RunEvent::ReviewerDropped { role, reason } => {
                format!("  ✗ {role} dropped: {}", truncate(reason, 120))
            }
            RunEvent::Retry { role, kind, reason } => {
                format!("  ↻ retrying {role} ({kind}): {}", truncate(reason, 120))
            }
            RunEvent::Round {
                round,
                new_blocking,
                total_blocking,
            } => format!("  round {round}: {new_blocking} new blocking of {total_blocking} raised"),
            RunEvent::Note(message) => format!("  {message}"),
        })
    }
}

/// Prints a compact activity log to stdout.
pub struct TerminalProgress {
    formatter: EventFormatter,
}

impl Default for TerminalProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalProgress {
    pub fn new() -> Self {
        Self {
            formatter: EventFormatter::new(),
        }
    }
}

impl ProgressSink for TerminalProgress {
    fn emit(&self, event: RunEvent<'_>) {
        // A stage heading reads better with a blank line before it.
        let leading = matches!(event, RunEvent::Stage { .. });
        if let Some(line) = self.formatter.format(&event) {
            if leading {
                println!();
            }
            println!("{line}");
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    let cleaned = text.replace('\n', " ");
    if cleaned.chars().count() <= limit {
        return cleaned;
    }
    let kept: String = cleaned.chars().take(limit.saturating_sub(1)).collect();
    format!("{kept}…")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Records what it was told, for asserting on executor behaviour.
    pub struct RecordingProgress {
        pub lines: RefCell<Vec<String>>,
    }

    impl RecordingProgress {
        pub fn new() -> Self {
            Self {
                lines: RefCell::new(Vec::new()),
            }
        }
    }

    impl ProgressSink for RecordingProgress {
        fn emit(&self, event: RunEvent<'_>) {
            self.lines.borrow_mut().push(format!("{event:?}"));
        }
    }

    #[test]
    fn truncate_collapses_newlines_and_caps_length() {
        assert_eq!(truncate("a\nb", 10), "a b");
        assert_eq!(truncate(&"x".repeat(20), 5), "xxxx…");
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // A byte-based cut would split these and panic.
        assert_eq!(truncate("读取文件内容", 3), "读取…");
    }

    #[test]
    fn a_repeated_tool_call_is_reported_once() {
        let formatter = EventFormatter::new();
        let tool = |status| RunEvent::Tool {
            role: "opus",
            tool_call_id: "call-1",
            title: "Read src/lib.rs",
            status,
        };

        assert!(formatter.format(&tool("pending")).is_some());
        assert!(formatter.format(&tool("in_progress")).is_none());
    }

    #[test]
    fn a_tool_call_that_fails_is_reported_again() {
        let formatter = EventFormatter::new();
        let tool = |status| RunEvent::Tool {
            role: "opus",
            tool_call_id: "call-1",
            title: "Read src/lib.rs",
            status,
        };
        formatter.format(&tool("pending"));

        let failure = formatter
            .format(&tool("failed"))
            .expect("failures resurface");

        assert!(failure.contains('✗'));
    }

    #[test]
    fn a_new_stage_forgets_earlier_tool_calls() {
        let formatter = EventFormatter::new();
        let tool = RunEvent::Tool {
            role: "opus",
            tool_call_id: "call-1",
            title: "Read src/lib.rs",
            status: "completed",
        };
        formatter.format(&tool);

        formatter.format(&RunEvent::Stage {
            phase: PhaseKind::Plan,
            stage: StageKind::Reviewing,
            round: 2,
            roles: "opus",
        });

        // The same call in a new stage is news again.
        assert!(formatter.format(&tool).is_some());
    }

    #[test]
    fn the_silent_sink_accepts_everything() {
        SilentProgress.emit(RunEvent::Note("ignored"));
    }
}
