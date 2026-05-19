//! In-process event bus backed by tokio::sync::broadcast.
//!
//! Delivers events to all subscribers within the same process with
//! sub-microsecond latency.

use loom_core::LoomEvent;
use tokio::sync::broadcast;

use crate::EventBusError;

/// In-process event bus using tokio's broadcast channel.
///
/// All subscribers in the same process receive every published event.
/// The channel is lock-free and delivers events in O(1) per subscriber.
#[derive(Debug)]
pub struct InProcessBus {
    sender: broadcast::Sender<LoomEvent>,
    capacity: usize,
}

impl InProcessBus {
    /// Create a new in-process bus with the given channel capacity.
    ///
    /// Capacity should be a power of two for best performance.
    /// Default is 4096 in [`crate::EventBus`].
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender, capacity }
    }

    /// Publish an event to all in-process subscribers.
    ///
    /// Returns the number of subscribers that received the event.
    /// If all receivers are lagging, old events are dropped (ring buffer behavior).
    pub fn publish(&self, event: &LoomEvent) -> Result<u64, EventBusError> {
        let sub_count = self
            .sender
            .send(event.clone())
            .map_err(|_| EventBusError::SendFailed("broadcast channel closed".into()))?;

        Ok(sub_count as u64)
    }

    /// Subscribe to all events.
    pub fn subscribe(&self) -> broadcast::Receiver<LoomEvent> {
        self.sender.subscribe()
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Total capacity of the underlying channel.
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}
