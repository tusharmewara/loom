//! Performance counters and metrics for the EventBus.
//!
//! Tracks publish counts, topic distribution, cross-process errors,
//! and subscriber lag. Thread-safe via atomic operations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Thread-safe performance counters for the EventBus.
#[derive(Debug)]
pub struct BusMetrics {
    published: AtomicU64,
    xproc_published: AtomicU64,
    xproc_errors: AtomicU64,
    topic_counts: Mutex<std::collections::HashMap<String, u64>>,
}

impl BusMetrics {
    /// Create a new metrics counter.
    pub fn new() -> Self {
        Self {
            published: AtomicU64::new(0),
            xproc_published: AtomicU64::new(0),
            xproc_errors: AtomicU64::new(0),
            topic_counts: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Increment the total published event counter.
    pub fn inc_published(&self) {
        self.published.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the cross-process published counter.
    pub fn inc_xproc_published(&self) {
        self.xproc_published.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the cross-process error counter.
    pub fn inc_xproc_errors(&self) {
        self.xproc_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a topic for distribution tracking.
    pub fn record_topic(&self, topic: &str) {
        if let Ok(mut counts) = self.topic_counts.lock() {
            *counts.entry(topic.to_string()).or_insert(0) += 1;
        }
    }

    /// Total events published.
    pub fn published(&self) -> u64 {
        self.published.load(Ordering::Relaxed)
    }

    /// Cross-process events successfully published.
    pub fn xproc_published(&self) -> u64 {
        self.xproc_published.load(Ordering::Relaxed)
    }

    /// Cross-process publish errors.
    pub fn xproc_errors(&self) -> u64 {
        self.xproc_errors.load(Ordering::Relaxed)
    }

    /// Topic distribution snapshot.
    pub fn topic_distribution(&self) -> std::collections::HashMap<String, u64> {
        self.topic_counts.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

impl Default for BusMetrics {
    fn default() -> Self {
        Self::new()
    }
}
