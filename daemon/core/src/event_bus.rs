use tokio::sync::broadcast;

/// A simple async event bus for broadcasting events to multiple subscribers.
pub struct EventBus<T: Clone + Send + 'static> {
    sender: broadcast::Sender<T>,
}

impl<T: Clone + Send + 'static> EventBus<T> {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event to all subscribers.
    pub fn publish(&self, event: T) -> Result<usize, broadcast::error::SendError<T>> {
        self.sender.send(event)
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self.sender.subscribe()
    }
}

impl<T: Clone + Send + 'static> Default for EventBus<T> {
    fn default() -> Self {
        Self::new(256)
    }
}
