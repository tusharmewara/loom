//! Error types for the EventBus.

use std::fmt;

/// Errors that can occur during event bus operations.
#[derive(Debug, Clone)]
pub enum EventBusError {
    /// Serialization of the event failed.
    Serialize(String),
    /// Send to the internal channel failed.
    SendFailed(String),
    /// Cross-process (ZeroMQ) publish failed.
    CrossProcessPublish(String),
    /// Subscriber is too slow and events were dropped.
    SubscriberLagged(u64),
}

impl fmt::Display for EventBusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventBusError::Serialize(e) => write!(f, "event serialization failed: {}", e),
            EventBusError::SendFailed(e) => write!(f, "event send failed: {}", e),
            EventBusError::CrossProcessPublish(e) => {
                write!(f, "cross-process publish failed: {}", e)
            }
            EventBusError::SubscriberLagged(n) => {
                write!(f, "subscriber lagged, {} events dropped", n)
            }
        }
    }
}

impl std::error::Error for EventBusError {}
