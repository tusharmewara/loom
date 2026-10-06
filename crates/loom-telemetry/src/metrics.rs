use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Thread-safe metrics counters for throughput tracking.
///
/// Counters track events published, skills discovered, MCP health checks,
/// sessions exported, auth validations, hooks fired, and more.
pub struct MetricsCollector {
    counters: Arc<HashMap<&'static str, AtomicU64>>,
}

impl MetricsCollector {
    /// Create a new collector with all standard counters initialized to 0.
    pub fn new() -> Self {
        let mut counters = HashMap::new();
        counters.insert("events_published", AtomicU64::new(0));
        counters.insert("skills_discovered", AtomicU64::new(0));
        counters.insert("mcp_checks", AtomicU64::new(0));
        counters.insert("sessions_exported", AtomicU64::new(0));
        counters.insert("auth_validations", AtomicU64::new(0));
        counters.insert("hooks_fired", AtomicU64::new(0));
        counters.insert("rpc_calls", AtomicU64::new(0));
        counters.insert("queue_depth", AtomicU64::new(0));
        counters.insert("subscriber_lag_events", AtomicU64::new(0));
        // Container runtime metrics
        counters.insert("container_runs_total", AtomicU64::new(0));
        counters.insert("container_runs_success", AtomicU64::new(0));
        counters.insert("container_runs_failure", AtomicU64::new(0));
        counters.insert("container_pull_total", AtomicU64::new(0));
        counters.insert("container_pull_failure", AtomicU64::new(0));
        Self { counters: Arc::new(counters) }
    }

    /// Increment a named counter by 1.
    ///
    /// If the counter doesn't exist, the call is silently ignored.
    pub fn increment(&self, name: &str) {
        if let Some(counter) = self.counters.get(name) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Increment a named counter by a delta.
    pub fn add(&self, name: &str, delta: u64) {
        if let Some(counter) = self.counters.get(name) {
            counter.fetch_add(delta, Ordering::Relaxed);
        }
    }

    /// Get the current value of a counter (0 if unknown).
    pub fn get(&self, name: &str) -> u64 {
        self.counters
            .get(name)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// Snapshot of all counters.
    pub fn snapshot(&self) -> Vec<(&'static str, u64)> {
        self.counters
            .iter()
            .map(|(k, v)| (*k, v.load(Ordering::Relaxed)))
            .collect()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}
