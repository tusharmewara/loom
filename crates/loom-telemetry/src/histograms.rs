//! Latency histograms with 3-sigma anomaly detection.
//!
//! Maintains online statistics (streaming mean + variance) for each RPC method.
//! When a new observation exceeds mean + 3*stddev, it's flagged as anomalous.
//! Uses Welford's online algorithm for numerical stability.

use std::collections::HashMap;
use std::sync::Mutex;

/// Per-method running statistics.
#[derive(Debug, Clone)]
pub struct MethodStats {
    /// Number of observations.
    pub count: u64,
    /// Running mean (microseconds).
    pub mean: f64,
    /// Running M2 (sum of squares of differences from the mean).
    pub m2: f64,
    /// Running minimum observed value.
    pub min: f64,
    /// Running maximum observed value.
    pub max: f64,
}

impl MethodStats {
    fn new(first: f64) -> Self {
        Self {
            count: 1,
            mean: first,
            m2: 0.0,
            min: first,
            max: first,
        }
    }

    /// Update with a new observation using Welford's online algorithm.
    fn update(&mut self, x: f64) {
        self.count += 1;
        let delta = x - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = x - self.mean;
        self.m2 += delta * delta2;
        if x < self.min {
            self.min = x;
        }
        if x > self.max {
            self.max = x;
        }
    }

    /// Population variance.
    pub fn variance(&self) -> f64 {
        if self.count > 1 {
            self.m2 / self.count as f64
        } else {
            0.0
        }
    }

    /// Population standard deviation.
    pub fn stddev(&self) -> f64 {
        self.variance().sqrt()
    }

    /// 3-sigma upper threshold (mean + 3*stddev).
    pub fn anomaly_threshold(&self) -> f64 {
        self.mean + 3.0 * self.stddev()
    }

    /// Check if a value is anomalous (exceeds 3σ threshold).
    pub fn is_anomalous(&self, value: f64) -> bool {
        if self.count < 3 {
            return false; // not enough data for reliable detection
        }
        value > self.anomaly_threshold()
    }
}

/// Thread-safe latency histograms with 3-sigma anomaly detection.
///
/// Each RPC method has its own streaming statistics. The overhead per observation
/// is O(1) with negligible memory (<100 bytes per tracked method).
pub struct LatencyHistograms {
    methods: Mutex<HashMap<String, MethodStats>>,
}

impl LatencyHistograms {
    /// Create a new empty histogram set.
    pub fn new() -> Self {
        Self {
            methods: Mutex::new(HashMap::new()),
        }
    }

    /// Observe a latency measurement for a method.
    ///
    /// Returns `true` if the measurement exceeds the 3σ anomaly threshold.
    pub fn observe(&self, method: &str, latency_us: f64) -> bool {
        let mut methods = self.methods.lock().unwrap();
        let anomalous = methods
            .get(method)
            .map(|stats| stats.is_anomalous(latency_us))
            .unwrap_or(false);

        match methods.get_mut(method) {
            Some(stats) => stats.update(latency_us),
            None => {
                methods.insert(method.to_string(), MethodStats::new(latency_us));
            }
        }

        anomalous
    }

    /// Get statistics for a method, if tracked.
    pub fn stats(&self, method: &str) -> Option<MethodStats> {
        self.methods.lock().ok()?.get(method).cloned()
    }

    /// Current mean latency for a method (0 if not tracked).
    pub fn mean(&self, method: &str) -> f64 {
        self.stats(method).map(|s| s.mean).unwrap_or(0.0)
    }

    /// Current standard deviation for a method (0 if not tracked).
    pub fn stddev(&self, method: &str) -> f64 {
        self.stats(method).map(|s| s.stddev()).unwrap_or(0.0)
    }

    /// Number of tracked methods.
    pub fn method_count(&self) -> usize {
        self.methods.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Reset all histograms.
    pub fn reset(&self) {
        if let Ok(mut methods) = self.methods.lock() {
            methods.clear();
        }
    }
}

impl Default for LatencyHistograms {
    fn default() -> Self {
        Self::new()
    }
}
