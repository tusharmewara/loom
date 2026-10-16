pub mod catalog;
pub mod classifier;
pub mod decision;
pub mod metrics;
pub mod router;
pub mod scorer;

use loom_core::HealthCheck;

pub use catalog::CatalogManager;
pub use classifier::Classifier;
pub use decision::DecisionEngine;
pub use metrics::RoutingMetrics;
pub use router::ModelRouter;
pub use scorer::{
    score_dynamic, score_dynamic_signals, score_static, score_static_signals, DynamicSignals,
    StaticSignals,
};

use std::sync::Arc;

pub struct RoutingEngine {
    classifier: Classifier,
    decision: DecisionEngine,
    metrics: Arc<RoutingMetrics>,
}

impl RoutingEngine {
    pub fn new(_loom_dir: &str) -> Self {
        let decision = DecisionEngine::new();
        Self {
            classifier: Classifier::new(),
            decision,
            metrics: Arc::new(RoutingMetrics::new()),
        }
    }

    pub fn with_metrics(metrics: Arc<RoutingMetrics>) -> Self {
        let decision = DecisionEngine::new();
        Self {
            classifier: Classifier::new(),
            decision,
            metrics,
        }
    }

    pub fn classify(&self, prompt: &str) -> loom_core::ComplexityScore {
        self.metrics.record_classification();
        self.metrics.time(|| self.classifier.classify(prompt))
    }

    pub fn decide(&self, score: loom_core::ComplexityScore) -> loom_core::RoutingDecision {
        self.metrics.record_classification();
        let decision = self.metrics.time(|| self.decision.decide(score));
        let tier = format!("{:?}", decision.recommended_tier);
        self.metrics.record_decision(&tier);
        decision
    }

    pub fn metrics(&self) -> &Arc<RoutingMetrics> {
        &self.metrics
    }
}

impl HealthCheck for RoutingEngine {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-routing",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: self.metrics.snapshot(),
        })
    }
}
