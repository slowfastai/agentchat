use std::rc::Rc;

use agent_client_protocol::{ContentBlock, SessionId, SessionNotification, SessionUpdate};
use tokio::sync::mpsc;

use agentchat_protocol::{SessionEvent, SessionTranscript};

use crate::acp_client::AcpAgent;
use crate::skills::SkillStore;

/// Runs a follow-up agent pass that turns transcripts into reusable shared markdown skills.
pub struct Distiller {
    skill_store: Rc<SkillStore>,
}

impl Distiller {
    pub fn new(skill_store: Rc<SkillStore>) -> Self {
        Self { skill_store }
    }

    pub fn build_distillation_prompt(&self, transcript: &SessionTranscript) -> String {
        format!(
            concat!(
                "You are analyzing a completed coding session to extract reusable knowledge.\n\n",
                "Session metadata:\n",
                "- Session ID: {}\n",
                "- Agent ID: {}\n",
                "- Working directory: {}\n\n",
                "Session transcript:\n{}\n\n",
                "Extract actionable, project-specific knowledge. For each piece, output:\n\n",
                "---SKILL: {{topic-name}}---\n",
                "{{markdown content}}\n",
                "---END SKILL---\n\n",
                "Focus on patterns discovered, conventions established, debugging approaches,\n",
                "architecture decisions, and gotchas specific to this project.\n"
            ),
            transcript.session_id,
            transcript.agent_id,
            transcript.working_dir,
            render_transcript(transcript),
        )
    }

    pub fn parse_skill_blocks(&self, response_text: &str) -> Vec<(String, String)> {
        let mut skills = Vec::new();
        let mut current_name: Option<String> = None;
        let mut current_lines = Vec::new();

        for line in response_text.lines() {
            if let Some(name) = line
                .strip_prefix("---SKILL:")
                .and_then(|rest| rest.strip_suffix("---"))
            {
                current_name = Some(name.trim().to_string());
                current_lines.clear();
                continue;
            }

            if line == "---END SKILL---" {
                if let Some(name) = current_name.take() {
                    let content = current_lines.join("\n").trim().to_string();
                    if !name.is_empty() && !content.is_empty() {
                        skills.push((name, format!("{content}\n")));
                    }
                }
                current_lines.clear();
                continue;
            }

            if current_name.is_some() {
                current_lines.push(line.to_string());
            }
        }

        skills
    }

    pub async fn distill(
        &self,
        agent: Rc<AcpAgent>,
        session_id: String,
        transcript: SessionTranscript,
        mut update_rx: mpsc::UnboundedReceiver<SessionNotification>,
    ) -> Result<Vec<String>, String> {
        let prompt = self.build_distillation_prompt(&transcript);
        let prompt_session_id = session_id.clone();
        let prompt_task = tokio::task::spawn_local(async move {
            agent
                .prompt(SessionId::new(prompt_session_id), prompt)
                .await
        });
        tokio::pin!(prompt_task);

        let mut response_text = String::new();

        loop {
            tokio::select! {
                result = &mut prompt_task => {
                    let prompt_result = result
                        .map_err(|e| format!("distillation prompt task failed: {e}"))?;
                    prompt_result.map_err(|e| format!("distillation prompt failed: {e}"))?;

                    // Late ACP chunks can still be in flight when the prompt call resolves.
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    break;
                }
                maybe_notification = update_rx.recv() => {
                    let Some(notification) = maybe_notification else {
                        continue;
                    };
                    collect_response_text(&mut response_text, &notification);
                }
            }
        }

        while let Ok(notification) = update_rx.try_recv() {
            collect_response_text(&mut response_text, &notification);
        }

        let skills = self.parse_skill_blocks(&response_text);
        let mut written = Vec::with_capacity(skills.len());
        for (name, content) in skills {
            let target_name = if name.starts_with("shared/") {
                name
            } else {
                format!("shared/{name}")
            };
            let target_name = if target_name.ends_with(".md") {
                target_name
            } else {
                format!("{target_name}.md")
            };
            self.skill_store.write_skill(&target_name, &content).await?;
            written.push(target_name);
        }

        Ok(written)
    }
}

fn render_transcript(transcript: &SessionTranscript) -> String {
    let mut lines = Vec::new();

    for event in &transcript.events {
        match event {
            SessionEvent::UserPrompt { content, .. } => {
                lines.push(format!("[User] {content}"));
            }
            SessionEvent::AgentUpdate {
                notification_json, ..
            } => {
                if let Ok(notification) =
                    serde_json::from_value::<SessionNotification>(notification_json.clone())
                {
                    lines.push(render_notification(&notification));
                } else {
                    lines.push(format!("[Notification] {notification_json}"));
                }
            }
            SessionEvent::TurnEnd { stop_reason, .. } => {
                lines.push(format!("[TurnEnd] {stop_reason}"));
            }
        }
    }

    lines.join("\n")
}

fn render_notification(notification: &SessionNotification) -> String {
    match &notification.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            format!("[Agent] {}", extract_text(&chunk.content))
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            format!("[Thinking] {}", extract_text(&chunk.content))
        }
        SessionUpdate::ToolCall(tool_call) => {
            format!("[Tool] {} - {:?}", tool_call.title, tool_call.status)
        }
        SessionUpdate::ToolCallUpdate(update) => format!(
            "[Tool Update] {} - {}",
            update.fields.title.clone().unwrap_or_default(),
            update
                .fields
                .status
                .as_ref()
                .map(|status| format!("{status:?}"))
                .unwrap_or_default()
        ),
        SessionUpdate::Plan(plan) => format!(
            "[Plan] {}",
            serde_json::to_string(plan).unwrap_or_else(|_| "{}".into())
        ),
        _ => format!(
            "[Notification] {}",
            serde_json::to_string(notification).unwrap_or_else(|_| "{}".into())
        ),
    }
}

fn collect_response_text(buffer: &mut String, notification: &SessionNotification) {
    if let SessionUpdate::AgentMessageChunk(chunk) = &notification.update {
        buffer.push_str(&extract_text(&chunk.content));
    }
}

fn extract_text(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => text.text.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::{
        ContentChunk, SessionNotification, SessionUpdate, ToolCall, ToolCallStatus,
    };

    use super::*;

    fn sample_transcript() -> SessionTranscript {
        SessionTranscript {
            session_id: "session-1".into(),
            agent_id: "agent-1".into(),
            working_dir: "/tmp/project".into(),
            created_at_ms: 1,
            events: vec![
                SessionEvent::UserPrompt {
                    content: "Investigate failing build".into(),
                    timestamp_ms: 2,
                },
                SessionEvent::AgentUpdate {
                    notification_json: serde_json::to_value(SessionNotification::new(
                        "session-1",
                        SessionUpdate::ToolCall(
                            ToolCall::new("tool-1", "Run cargo test")
                                .status(ToolCallStatus::InProgress),
                        ),
                    ))
                    .unwrap(),
                    timestamp_ms: 3,
                },
                SessionEvent::AgentUpdate {
                    notification_json: serde_json::to_value(SessionNotification::new(
                        "session-1",
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(
                            "The workspace needs a test helper.",
                        ))),
                    ))
                    .unwrap(),
                    timestamp_ms: 4,
                },
                SessionEvent::TurnEnd {
                    stop_reason: "EndTurn".into(),
                    timestamp_ms: 5,
                },
            ],
        }
    }

    #[test]
    fn build_distillation_prompt_includes_session_context() {
        let skill_store = Rc::new(SkillStore::new(std::path::Path::new("/tmp/project")));
        let distiller = Distiller::new(skill_store);
        let prompt = distiller.build_distillation_prompt(&sample_transcript());

        assert!(prompt.contains("Session ID: session-1"));
        assert!(prompt.contains("[User] Investigate failing build"));
        assert!(prompt.contains("[Tool] Run cargo test - InProgress"));
        assert!(prompt.contains("[Agent] The workspace needs a test helper."));
    }

    #[test]
    fn parse_skill_blocks_extracts_multiple_markdown_sections() {
        let skill_store = Rc::new(SkillStore::new(std::path::Path::new("/tmp/project")));
        let distiller = Distiller::new(skill_store);
        let response = concat!(
            "---SKILL: testing-notes---\n",
            "# Testing Notes\n",
            "- Use the fake ACP agent in server tests.\n",
            "---END SKILL---\n",
            "---SKILL: session-persistence---\n",
            "# Session Persistence\n",
            "- Transcripts live under .agentchat/sessions.\n",
            "---END SKILL---\n"
        );

        let skills = distiller.parse_skill_blocks(response);

        assert_eq!(
            skills,
            vec![
                (
                    "testing-notes".into(),
                    "# Testing Notes\n- Use the fake ACP agent in server tests.\n".into(),
                ),
                (
                    "session-persistence".into(),
                    "# Session Persistence\n- Transcripts live under .agentchat/sessions.\n".into(),
                ),
            ]
        );
    }
}
