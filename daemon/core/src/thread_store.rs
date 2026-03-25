use std::collections::HashMap;

use uuid::Uuid;

use agentchat_protocol::{
    now_millis, ParticipantKind, ThreadParticipant, ThreadSnapshot, ThreadSummary,
};

#[derive(Debug, Clone)]
pub struct ThreadRecord {
    pub thread_id: String,
    pub title: Option<String>,
    pub working_dir: String,
    pub created_at_ms: u64,
    pub participants: Vec<ThreadParticipantRecord>,
}

#[derive(Debug, Clone)]
pub struct ThreadParticipantRecord {
    pub participant_id: String,
    pub kind: ParticipantKind,
    pub display_name: String,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSessionBinding {
    pub thread_id: String,
    pub participant_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub display_name: String,
}

pub struct ThreadStore {
    threads: HashMap<String, ThreadRecord>,
    session_to_binding: HashMap<String, ThreadSessionBinding>,
}

impl ThreadStore {
    pub fn new() -> Self {
        Self {
            threads: HashMap::new(),
            session_to_binding: HashMap::new(),
        }
    }

    pub fn create_thread(&mut self, title: Option<String>, working_dir: String) -> ThreadRecord {
        let thread_id = format!("thread-{}", Uuid::new_v4().simple());
        let record = ThreadRecord {
            thread_id: thread_id.clone(),
            title,
            working_dir,
            created_at_ms: now_millis(),
            participants: vec![ThreadParticipantRecord {
                participant_id: "participant-user".into(),
                kind: ParticipantKind::Human,
                display_name: "You".into(),
                agent_id: None,
                session_id: None,
            }],
        };
        self.threads.insert(thread_id, record.clone());
        record
    }

    pub fn get_thread(&self, thread_id: &str) -> Option<&ThreadRecord> {
        self.threads.get(thread_id)
    }

    pub fn list_threads(&self) -> Vec<ThreadRecord> {
        self.threads.values().cloned().collect()
    }

    pub fn add_agent_participant(
        &mut self,
        thread_id: &str,
        agent_id: String,
        display_name: String,
        session_id: String,
    ) -> Result<ThreadParticipantRecord, String> {
        let thread = self
            .threads
            .get_mut(thread_id)
            .ok_or_else(|| "thread not found".to_string())?;
        let participant = ThreadParticipantRecord {
            participant_id: format!("participant-{}", Uuid::new_v4().simple()),
            kind: ParticipantKind::Agent,
            display_name: display_name.clone(),
            agent_id: Some(agent_id.clone()),
            session_id: Some(session_id.clone()),
        };
        thread.participants.push(participant.clone());
        self.session_to_binding.insert(
            session_id.clone(),
            ThreadSessionBinding {
                thread_id: thread_id.to_string(),
                participant_id: participant.participant_id.clone(),
                agent_id,
                session_id,
                display_name,
            },
        );
        Ok(participant)
    }

    pub fn remove_participant(
        &mut self,
        thread_id: &str,
        participant_id: &str,
    ) -> Option<ThreadParticipantRecord> {
        let thread = self.threads.get_mut(thread_id)?;
        let index = thread
            .participants
            .iter()
            .position(|participant| participant.participant_id == participant_id)?;
        let participant = thread.participants.remove(index);
        if let Some(session_id) = &participant.session_id {
            self.session_to_binding.remove(session_id);
        }
        Some(participant)
    }

    pub fn remove_thread(&mut self, thread_id: &str) -> Option<ThreadRecord> {
        let thread = self.threads.remove(thread_id)?;
        for participant in &thread.participants {
            if let Some(session_id) = &participant.session_id {
                self.session_to_binding.remove(session_id);
            }
        }
        Some(thread)
    }

    pub fn binding_for_session(&self, session_id: &str) -> Option<&ThreadSessionBinding> {
        self.session_to_binding.get(session_id)
    }

    pub fn target_agent_participants(
        &self,
        thread_id: &str,
        target_participant_ids: Option<&[String]>,
    ) -> Result<Vec<ThreadParticipantRecord>, String> {
        let thread = self
            .threads
            .get(thread_id)
            .ok_or_else(|| "thread not found".to_string())?;
        let participants = match target_participant_ids {
            Some(targets) => {
                let mut resolved = Vec::new();
                for target_id in targets {
                    let participant = thread
                        .participants
                        .iter()
                        .find(|participant| participant.participant_id == *target_id)
                        .ok_or_else(|| format!("participant not found: {target_id}"))?;
                    if participant.kind == ParticipantKind::Agent {
                        resolved.push(participant.clone());
                    }
                }
                resolved
            }
            None => thread
                .participants
                .iter()
                .filter(|participant| participant.kind == ParticipantKind::Agent)
                .cloned()
                .collect::<Vec<_>>(),
        };
        Ok(participants)
    }
}

impl Default for ThreadStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn participant_to_protocol(
    participant: &ThreadParticipantRecord,
    state: agentchat_protocol::ParticipantState,
) -> ThreadParticipant {
    ThreadParticipant {
        participant_id: participant.participant_id.clone(),
        kind: participant.kind.clone(),
        display_name: participant.display_name.clone(),
        agent_id: participant.agent_id.clone(),
        session_id: participant.session_id.clone(),
        state,
    }
}

pub fn thread_to_summary(
    thread: &ThreadRecord,
    state: agentchat_protocol::ThreadState,
    last_thread_seq: u64,
) -> ThreadSummary {
    ThreadSummary {
        thread_id: thread.thread_id.clone(),
        title: thread.title.clone(),
        working_dir: thread.working_dir.clone(),
        created_at_ms: thread.created_at_ms,
        state,
        participant_count: thread.participants.len() as u32,
        last_thread_seq,
    }
}

pub fn thread_to_snapshot(
    thread: &ThreadRecord,
    participants: Vec<ThreadParticipant>,
    last_thread_seq: u64,
) -> ThreadSnapshot {
    ThreadSnapshot {
        thread_id: thread.thread_id.clone(),
        title: thread.title.clone(),
        working_dir: thread.working_dir.clone(),
        created_at_ms: thread.created_at_ms,
        last_thread_seq,
        participants,
    }
}

#[cfg(test)]
mod tests {
    use agentchat_protocol::{ParticipantKind, ParticipantState, ThreadState};

    use super::*;

    #[test]
    fn create_and_bind_agent_participant() {
        let mut store = ThreadStore::new();
        let thread = store.create_thread(Some("Test".into()), ".".into());
        let participant = store
            .add_agent_participant(
                &thread.thread_id,
                "agent-1".into(),
                "Agent 1".into(),
                "session-1".into(),
            )
            .unwrap();

        assert_eq!(participant.kind, ParticipantKind::Agent);
        assert_eq!(
            store.binding_for_session("session-1"),
            Some(&ThreadSessionBinding {
                thread_id: thread.thread_id.clone(),
                participant_id: participant.participant_id.clone(),
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                display_name: "Agent 1".into(),
            })
        );

        let summary = thread_to_summary(&thread, ThreadState::Idle, 0);
        assert_eq!(summary.participant_count, 1);
        let snapshot = thread_to_snapshot(
            &thread,
            vec![participant_to_protocol(
                &participant,
                ParticipantState::Idle,
            )],
            0,
        );
        assert_eq!(snapshot.last_thread_seq, 0);
    }
}
