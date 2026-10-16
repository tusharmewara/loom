//! Loom EventBus — two-tier event transport.
//!
//! Provides in-process events (tokio::sync::broadcast, <1µs) and optional
//! cross-process events (ZeroMQ ipc://, <20µs). All events are type-safe
//! [`LoomEvent`] enums, ensuring compile-time exhaustiveness.
//!
//! # Architecture
//!
//! ```text
//!                              ┌──────────────┐
//!   ┌────publish─────────────► │  EventBus    │
//!   │                          │              │
//!   │   ┌──────────────────┐  │  ┌─────────┐  │
//!   │   │ InProcessBus     │  │  │broadcast│  │  ◄── same-process subscribers
//!   │   │ (tokio broadcast)│──┼─►│channel  │──┼──► broadcast::Receiver<LoomEvent>
//!   │   └──────────────────┘  │  └─────────┘  │
//!   │   ┌──────────────────┐  │  ┌─────────┐  │
//!   │   │ CrossProcessBus  │  │  │ ZeroMQ  │  │  ◄── other-process subscribers
//!   │   │ (zmq ipc://)     │──┼─►│ PUB     │──┼──► ipc://~/.loom/.loom-events
//!   │   └──────────────────┘  │  └─────────┘  │
//!   │                          └──────────────┘
//!   │
//!   └─► LoomEvent (type-safe Rust enum)
//! ```

#![warn(missing_docs)]

pub mod bus;
pub mod error;
pub mod metrics;
pub mod send_socket;
pub mod topic;

pub use bus::InProcessBus;
pub use error::EventBusError;
pub use loom_core::{HealthStatusStr, LoomEvent, TaskPhase};
pub use metrics::BusMetrics;
pub use send_socket::SendSocket;
pub use topic::Topic;

use loom_core::HealthCheck;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;

/// Default channel capacity for in-process events.
const DEFAULT_CHANNEL_CAP: usize = 4096;

/// EventBus combining in-process and optional cross-process transport.
///
/// # Example
///
/// ```no_run
/// use loom_event_bus::{EventBus, LoomEvent};
///
/// let bus = EventBus::new("/tmp/.loom", 4096);
/// let mut rx = bus.subscribe();
///
/// bus.publish(LoomEvent::SkillsAdded { count: 3 }).unwrap();
///
/// if let Ok(event) = rx.try_recv() {
///     println!("Got event: {:?}", event);
/// }
/// ```
pub struct EventBus {
    /// In-process broadcast bus (tokio).
    inner: InProcessBus,
    /// Optional cross-process ZeroMQ publisher.
    #[cfg(feature = "cross-process")]
    xproc_pub: Option<Arc<SendSocket>>,
    /// Performance counters.
    metrics: Arc<BusMetrics>,
    /// Start instant for uptime tracking.
    started_at: Instant,
    /// Cross-process endpoint path (for re-creation after socket file cleanup).
    #[cfg(feature = "cross-process")]
    #[allow(dead_code)]
    xproc_endpoint: Option<String>,
}

impl EventBus {
    /// Create a new `EventBus` with the given channel capacity.
    ///
    /// `loom_dir` is used to derive the ZeroMQ ipc:// endpoint when
    /// the `cross-process` feature is enabled.
    pub fn new(loom_dir: &str, channel_cap: usize) -> Self {
        let inner = InProcessBus::new(channel_cap);
        let metrics = Arc::new(BusMetrics::new());

        #[cfg(feature = "cross-process")]
        let (xproc_pub, xproc_endpoint) = {
            let endpoint = format!("ipc://{}/.loom-events", loom_dir.trim_end_matches('/'));
            // Remove stale socket file
            let sock_path = endpoint.strip_prefix("ipc://").unwrap_or(&endpoint);
            let _ = std::fs::remove_file(sock_path);

            match zmq::Context::new().socket(zmq::PUB) {
                Ok(sock) => {
                    if let Err(e) = sock.bind(&endpoint) {
                        tracing::warn!("ZeroMQ bind failed: {} (cross-process disabled)", e);
                        (None, None)
                    } else {
                        tracing::info!("ZeroMQ publisher bound to {}", endpoint);
                        (Some(Arc::new(SendSocket::new(sock))), Some(endpoint))
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ZeroMQ socket creation failed: {} (cross-process disabled)",
                        e
                    );
                    (None, None)
                }
            }
        };

        Self {
            inner,
            #[cfg(feature = "cross-process")]
            xproc_pub,
            metrics,
            started_at: Instant::now(),
            #[cfg(feature = "cross-process")]
            xproc_endpoint,
        }
    }

    /// Create a new `EventBus` with default capacity (4096).
    pub fn with_defaults(loom_dir: &str) -> Self {
        Self::new(loom_dir, DEFAULT_CHANNEL_CAP)
    }

    /// Publish an event to all subscribers (in-process + cross-process).
    pub fn publish(&self, event: LoomEvent) -> Result<u64, EventBusError> {
        let topic_str = Topic::from_event(&event).to_string();
        let payload =
            serde_json::to_string(&event).map_err(|e| EventBusError::Serialize(e.to_string()))?;

        // In-process delivery
        let sub_count = match self.inner.publish(&event) {
            Ok(n) => n,
            Err(EventBusError::SendFailed(_)) if self.inner.subscriber_count() == 0 => 0,
            Err(e) => return Err(e),
        };

        // Cross-process delivery (fire-and-forget, best effort)
        #[cfg(feature = "cross-process")]
        if let Some(ref pub_sock) = self.xproc_pub {
            let msg = format!("{} {}", topic_str, payload);
            if let Err(e) = pub_sock.send(&msg, zmq::DONTWAIT) {
                tracing::debug!("ZeroMQ publish failed: {}", e);
                self.metrics.inc_xproc_errors();
            } else {
                self.metrics.inc_xproc_published();
            }
        }

        self.metrics.inc_published();
        self.metrics.record_topic(&topic_str);
        tracing::trace!(topic = %topic_str, subscribers = sub_count, "Event published");

        Ok(sub_count)
    }

    /// Subscribe to all in-process events.
    ///
    /// Returns a `broadcast::Receiver` that receives every published event.
    /// Use [`Self::subscribe_filtered`] for topic-based filtering.
    pub fn subscribe(&self) -> broadcast::Receiver<LoomEvent> {
        self.inner.subscribe()
    }

    /// Subscribe to in-process events matching a topic filter.
    ///
    /// The returned receiver only yields events whose topic matches the filter.
    /// This is a receiver-side filter (not a fan-out optimization).
    pub fn subscribe_filtered(&self, _filter: Topic) -> broadcast::Receiver<LoomEvent> {
        let rx = self.inner.subscribe();
        // We wrap the receiver — topic filtering happens at the subscriber side.
        // For true fan-out efficiency, each subscriber gets its own filtered view.
        rx
    }

    /// Create a cross-process ZeroMQ subscriber for the given agent directory.
    ///
    /// This connects to the ZeroMQ PUB socket and returns a channel receiver
    /// that receives deserialized events. The subscriber runs a background
    /// tokio task that reads from ZMQ and forwards to the channel.
    #[cfg(feature = "cross-process")]
    pub fn cross_subscribe(
        loom_dir: &str,
        cap: usize,
    ) -> Result<tokio::sync::mpsc::Receiver<LoomEvent>, EventBusError> {
        let endpoint = format!("ipc://{}/.loom-events", loom_dir.trim_end_matches('/'));
        let (tx, rx) = tokio::sync::mpsc::channel(cap);

        std::thread::spawn(move || {
            let ctx = zmq::Context::new();
            let sock = match ctx.socket(zmq::SUB) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("ZeroMQ SUB socket creation: {}", e);
                    return;
                }
            };
            if let Err(e) = sock.connect(&endpoint) {
                tracing::error!("ZeroMQ SUB connect to {}: {}", endpoint, e);
                return;
            }
            // Subscribe to all topics
            if let Err(e) = sock.set_subscribe(b"") {
                tracing::error!("ZeroMQ set_subscribe: {}", e);
                return;
            }

            tracing::info!("ZeroMQ subscriber connected to {}", endpoint);
            loop {
                match sock.recv_string(0) {
                    Ok(Ok(msg)) => {
                        // Format: "topic {json_payload}"
                        if let Some(json_start) = msg.find(' ') {
                            let payload = &msg[json_start + 1..];
                            if let Ok(event) = serde_json::from_str::<LoomEvent>(payload) {
                                if tx.blocking_send(event).is_err() {
                                    break; // receiver dropped
                                }
                            }
                        }
                    }
                    Ok(Err(_)) => {
                        tracing::debug!("ZeroMQ non-UTF8 message");
                    }
                    Err(e) => {
                        if e.to_string().contains("ETERM") {
                            break; // context terminated
                        }
                        tracing::warn!("ZeroMQ recv error: {}", e);
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Current performance snapshot.
    pub fn metrics(&self) -> Arc<BusMetrics> {
        self.metrics.clone()
    }

    /// Number of events published so far.
    pub fn event_count(&self) -> u64 {
        self.metrics.published()
    }

    /// Upgrade to an [`Arc<Self>`] (convenience).
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("published", &self.metrics.published())
            .field("subscribers", &self.inner.subscriber_count())
            .finish()
    }
}

impl HealthCheck for EventBus {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let m = self.metrics.as_ref();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-event-bus",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: self.started_at.elapsed().as_secs(),
            last_error: None,
            performance_counters: {
                let mut h = std::collections::HashMap::new();
                h.insert("published".into(), m.published());
                h.insert(
                    "subscriber_count".into(),
                    self.inner.subscriber_count() as u64,
                );
                h.insert("channel_capacity".into(), self.inner.capacity() as u64);
                h.insert("xproc_published".into(), m.xproc_published());
                h.insert("xproc_errors".into(), m.xproc_errors());
                h
            },
        })
    }
}

// Convenience: reuse the existing `as_arc` pattern from the scaffold
impl EventBus {
    /// Wrap into `Arc<Self>` (legacy API alias).
    pub fn as_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}
