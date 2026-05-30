use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Atomic counters for routing performance telemetry.
///
/// Tracks classification/decision throughput, tier distribution,
/// error counts, and latency. All counters are lock-free atomics.
pub struct RoutingMetrics {
    total_classifications: AtomicU64,
    total_decisions: AtomicU64,
    slm_count: AtomicU64,
    mid_count: AtomicU64,
    flagship_count: AtomicU64,
    error_count: AtomicU64,
    total_latency_us: AtomicU64,
    latency_samples: AtomicU64,
}

impl RoutingMetrics {
    pub fn new() -> Self {
        Self {
            total_classifications: AtomicU64::new(0),
            total_decisions: AtomicU64::new(0),
            slm_count: AtomicU64::new(0),
            mid_count: AtomicU64::new(0),
            flagship_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
            latency_samples: AtomicU64::new(0),
        }
    }

    /// Record a classification event.
    pub fn record_classification(&self) {
        self.total_classifications.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a routing decision and which tier was selected.
    pub fn record_decision(&self, tier: &str) {
        self.total_decisions.fetch_add(1, Ordering::Relaxed);
        match tier {
            "slm" | "Slm" | "SLM" => {
                self.slm_count.fetch_add(1, Ordering::Relaxed);
            }
            "mid" | "Mid" | "MID" => {
                self.mid_count.fetch_add(1, Ordering::Relaxed);
            }
            "flagship" | "Flagship" | "FLAGSHIP" => {
                self.flagship_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Record a routing error (e.g., model resolution failure).
    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a latency measurement in microseconds.
    pub fn record_latency(&self, latency_us: u64) {
        self.total_latency_us.fetch_add(latency_us, Ordering::Relaxed);
        self.latency_samples.fetch_add(1, Ordering::Relaxed);
    }

    /// Time an operation and record its latency.
    pub fn time<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let start = Instant::now();
        let result = f();
        let elapsed = start.elapsed().as_micros() as u64;
        self.record_latency(elapsed);
        result
    }

    /// Get all counters as a flat map (for health check / telemetry snapshot).
    pub fn snapshot(&self) -> std::collections::HashMap<String, u64> {
        let samples = self.latency_samples.load(Ordering::Relaxed);
        let total_lat = self.total_latency_us.load(Ordering::Relaxed);
        let avg_latency = total_lat.checked_div(samples).unwrap_or(0);

        let mut h = std::collections::HashMap::new();
        h.insert("routing_classifications".into(), self.total_classifications.load(Ordering::Relaxed));
        h.insert("routing_decisions".into(), self.total_decisions.load(Ordering::Relaxed));
        h.insert("routing_slm".into(), self.slm_count.load(Ordering::Relaxed));
        h.insert("routing_mid".into(), self.mid_count.load(Ordering::Relaxed));
        h.insert("routing_flagship".into(), self.flagship_count.load(Ordering::Relaxed));
        h.insert("routing_errors".into(), self.error_count.load(Ordering::Relaxed));
        h.insert("routing_avg_latency_us".into(), avg_latency);
        h.insert("routing_latency_samples".into(), samples);
        h
    }
}

impl Default for RoutingMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_snapshot_empty() {
        let m = RoutingMetrics::new();
        let snap = m.snapshot();
        assert_eq!(snap["routing_classifications"], 0);
        assert_eq!(snap["routing_decisions"], 0);
        assert_eq!(snap["routing_errors"], 0);
        assert_eq!(snap["routing_latency_samples"], 0);
    }

    #[test]
    fn test_metrics_record_classification() {
        let m = RoutingMetrics::new();
        m.record_classification();
        m.record_classification();
        m.record_classification();
        assert_eq!(m.snapshot()["routing_classifications"], 3);
    }

    #[test]
    fn test_metrics_record_decision_tiers() {
        let m = RoutingMetrics::new();
        m.record_decision("slm");
        m.record_decision("mid");
        m.record_decision("flagship");
        let snap = m.snapshot();
        assert_eq!(snap["routing_slm"], 1);
        assert_eq!(snap["routing_mid"], 1);
        assert_eq!(snap["routing_flagship"], 1);
        assert_eq!(snap["routing_decisions"], 3);
    }

    #[test]
    fn test_metrics_record_error() {
        let m = RoutingMetrics::new();
        m.record_error();
        m.record_error();
        assert_eq!(m.snapshot()["routing_errors"], 2);
    }

    #[test]
    fn test_metrics_latency() {
        let m = RoutingMetrics::new();
        m.record_latency(100);
        m.record_latency(200);
        let snap = m.snapshot();
        assert_eq!(snap["routing_latency_samples"], 2);
        assert!(snap["routing_avg_latency_us"] >= 150);
    }

    #[test]
    fn test_metrics_time_block() {
        let m = RoutingMetrics::new();
        let result = m.time(|| 42);
        assert_eq!(result, 42);
        assert_eq!(m.snapshot()["routing_latency_samples"], 1);
    }

    #[test]
    fn test_metrics_case_insensitive_tier() {
        let m = RoutingMetrics::new();
        m.record_decision("SLM");
        m.record_decision("Mid");
        m.record_decision("FLAGSHIP");
        let snap = m.snapshot();
        assert_eq!(snap["routing_slm"], 1);
        assert_eq!(snap["routing_mid"], 1);
        assert_eq!(snap["routing_flagship"], 1);
    }

    #[test]
    fn test_metrics_unknown_tier_ignored() {
        let m = RoutingMetrics::new();
        m.record_decision("unknown");
        let snap = m.snapshot();
        assert_eq!(snap["routing_slm"], 0);
        assert_eq!(snap["routing_mid"], 0);
        assert_eq!(snap["routing_flagship"], 0);
        assert_eq!(snap["routing_decisions"], 1);
    }
}
