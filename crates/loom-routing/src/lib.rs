pub mod catalog;
pub mod classifier;
pub mod decision;
pub mod router;
pub mod scorer;
mod util;

use loom_core::HealthCheck;

pub use catalog::CatalogManager;
pub use classifier::Classifier;
pub use decision::DecisionEngine;
pub use router::ModelRouter;
pub use scorer::{score_dynamic, score_static, score_dynamic_signals, score_static_signals, DynamicSignals, StaticSignals};

pub struct RoutingEngine {
    classifier: Classifier,
    decision: DecisionEngine,
}

impl RoutingEngine {
    pub fn new(_agents_dir: &str) -> Self {
        let decision = DecisionEngine::new();
        Self {
            classifier: Classifier::new(),
            decision,
        }
    }

    pub fn classify(&self, prompt: &str) -> loom_core::ComplexityScore {
        self.classifier.classify(prompt)
    }

    pub fn decide(&self, score: loom_core::ComplexityScore) -> loom_core::RoutingDecision {
        self.decision.decide(score)
    }
}

impl HealthCheck for RoutingEngine {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-routing",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: std::collections::HashMap::new(),
        })
    }
}
