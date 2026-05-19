use crate::router::ModelRouter;
use anyhow::Result;
use loom_core::{ComplexityScore, ComplexityThresholds, RoutingConfig, RoutingDecision};

pub struct DecisionEngine {
    router: ModelRouter,
}

impl DecisionEngine {
    pub fn new() -> Self {
        Self {
            router: ModelRouter::with_default_catalog(None),
        }
    }

    pub fn with_agents_dir(agents_dir: &str, config: Option<RoutingConfig>) -> Result<Self> {
        Ok(Self {
            router: ModelRouter::new(agents_dir, config)?,
        })
    }

    pub fn decide(&self, complexity: ComplexityScore) -> RoutingDecision {
        self.router.resolve(complexity)
    }

    /// Get complexity thresholds from routing config, if any.
    pub fn thresholds(&self) -> Option<&ComplexityThresholds> {
        self.router.config().map(|c| &c.complexity_thresholds)
    }
}
