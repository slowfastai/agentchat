use std::collections::HashMap;

/// In-memory sequence allocator for thread-scoped timeline events.
pub struct ThreadEventLog {
    next_seq: HashMap<String, u64>,
}

impl ThreadEventLog {
    pub fn new() -> Self {
        Self {
            next_seq: HashMap::new(),
        }
    }

    pub fn init_thread(&mut self, thread_id: &str) {
        self.next_seq.entry(thread_id.to_string()).or_insert(1);
    }

    pub fn next_seq(&mut self, thread_id: &str) -> u64 {
        let next_seq = self.next_seq.entry(thread_id.to_string()).or_insert(1);
        let current = *next_seq;
        *next_seq = next_seq.saturating_add(1);
        current
    }

    pub fn tail_seq(&self, thread_id: &str) -> u64 {
        self.next_seq
            .get(thread_id)
            .copied()
            .unwrap_or(1)
            .saturating_sub(1)
    }
}

impl Default for ThreadEventLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_seq_starts_at_one_per_thread() {
        let mut log = ThreadEventLog::new();

        assert_eq!(log.next_seq("thread-1"), 1);
        assert_eq!(log.next_seq("thread-1"), 2);
        assert_eq!(log.next_seq("thread-2"), 1);
        assert_eq!(log.tail_seq("thread-1"), 2);
        assert_eq!(log.tail_seq("thread-2"), 1);
    }
}
