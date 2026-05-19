//! Telemetry tests: metrics, histograms, anomaly detection, health check.

use loom_core::HealthCheck;
use loom_telemetry::histograms::LatencyHistograms;
use loom_telemetry::metrics::MetricsCollector;
use loom_telemetry::TelemetryCollector;

// ─── Metrics tests ──────────────────────────────────────────────────

#[test]
fn test_metrics_initial_state() {
    let m = MetricsCollector::new();
    for (name, value) in m.snapshot() {
        assert_eq!(value, 0, "counter {} should start at 0", name);
    }
}

#[test]
fn test_metrics_increment() {
    let m = MetricsCollector::new();
    m.increment("rpc_calls");
    m.increment("rpc_calls");
    assert_eq!(m.get("rpc_calls"), 2);
}

#[test]
fn test_metrics_add() {
    let m = MetricsCollector::new();
    m.add("events_published", 10);
    assert_eq!(m.get("events_published"), 10);
}

#[test]
fn test_metrics_unknown_counter() {
    let m = MetricsCollector::new();
    m.increment("nonexistent");
    assert_eq!(m.get("nonexistent"), 0);
}

// ─── Histogram tests ─────────────────────────────────────────────────

#[test]
fn test_histogram_initial_state() {
    let h = LatencyHistograms::new();
    assert_eq!(h.method_count(), 0);
    assert_eq!(h.mean("test_method"), 0.0);
    assert_eq!(h.stddev("test_method"), 0.0);
}

#[test]
fn test_histogram_single_observation() {
    let h = LatencyHistograms::new();
    let anomalous = h.observe("my_method", 100.0);
    assert!(!anomalous, "single observation should not be anomalous");

    let stats = h.stats("my_method").unwrap();
    assert_eq!(stats.count, 1);
    assert!((stats.mean - 100.0).abs() < 1e-9);
}

#[test]
fn test_histogram_observations() {
    let h = LatencyHistograms::new();
    let values = vec![10.0, 20.0, 30.0, 40.0, 50.0];

    for v in &values {
        h.observe("method_a", *v);
    }

    let stats = h.stats("method_a").unwrap();
    assert_eq!(stats.count, 5);
    assert!((stats.mean - 30.0).abs() < 1e-6);
    assert!((stats.min - 10.0).abs() < 1e-6);
    assert!((stats.max - 50.0).abs() < 1e-6);
    assert!(stats.stddev() > 0.0);
}

#[test]
fn test_histogram_anomaly_detection() {
    let h = LatencyHistograms::new();

    // Feed stable values
    for _ in 0..10 {
        h.observe("stable_method", 100.0);
    }

    // 100 should not be anomalous
    let anomalous = h.observe("stable_method", 100.0);
    assert!(!anomalous, "value at mean should not be anomalous");

    // Very large value should be anomalous (needs enough samples)
    let anomalous = h.observe("stable_method", 10000.0);
    assert!(anomalous, "extreme value should be anomalous");
}

#[test]
fn test_histogram_insufficient_data() {
    let h = LatencyHistograms::new();
    // With only 2 observations, anomaly detection shouldn't trigger
    h.observe("few_data", 100.0);
    let anomalous = h.observe("few_data", 10000.0);
    assert!(!anomalous, "insufficient data should not flag anomaly");
}

#[test]
fn test_histogram_multiple_methods() {
    let h = LatencyHistograms::new();
    h.observe("method_a", 10.0);
    h.observe("method_b", 100.0);

    assert_eq!(h.method_count(), 2);
    assert!((h.mean("method_a") - 10.0).abs() < 1e-6);
    assert!((h.mean("method_b") - 100.0).abs() < 1e-6);
}

#[test]
fn test_histogram_reset() {
    let h = LatencyHistograms::new();
    h.observe("method_x", 50.0);
    assert_eq!(h.method_count(), 1);

    h.reset();
    assert_eq!(h.method_count(), 0);
}

// ─── TelemetryCollector tests ────────────────────────────────────────

#[test]
fn test_telemetry_collector_creation() {
    let t = TelemetryCollector::new("/tmp/.agents");
    assert_eq!(t.metrics().get("rpc_calls"), 0);
}

#[test]
fn test_telemetry_record_latency() {
    let t = TelemetryCollector::new("/tmp/.agents");

    for _ in 0..10 {
        t.record_latency("test_rpc", 100.0);
    }

    let stats = t.histograms().stats("test_rpc").unwrap();
    assert_eq!(stats.count, 10);
    assert!((stats.mean - 100.0).abs() < 1e-6);
}

#[test]
fn test_telemetry_anomaly_flag() {
    let t = TelemetryCollector::new("/tmp/.agents");

    for _ in 0..5 {
        t.record_latency("anomaly_test", 50.0);
    }

    let flagged = t.record_latency("anomaly_test", 5000.0);
    assert!(flagged, "extreme latency should be flagged");
}

#[test]
fn test_telemetry_snapshot() {
    let t = TelemetryCollector::new("/tmp/.agents");
    t.metrics().increment("rpc_calls");
    t.metrics().increment("rpc_calls");

    let snap = t.snapshot();
    assert!(snap.contains_key("rpc_calls"));
    assert_eq!(snap["rpc_calls"], 2);
    assert!(snap.contains_key("method_count"));
}

#[test]
fn test_telemetry_health() {
    let t = TelemetryCollector::new("/tmp/.agents");
    let health = t.health().expect("health should succeed");
    assert_eq!(health.crate_name, "loom-telemetry");
    assert!(!health.version.is_empty());
    assert!(health.performance_counters.contains_key("rpc_calls"));
}
