use crate::catalog::CatalogManager;
use anyhow::Result;
use loom_core::{
    ComplexityScore, CostEstimate, ModelTier, RoutingConfig, RoutingDecision, RoutingStrategy,
};

pub struct ModelRouter {
    catalog: CatalogManager,
    config: Option<RoutingConfig>,
}

impl ModelRouter {
    pub fn new(loom_dir: &str, config: Option<RoutingConfig>) -> Result<Self> {
        Ok(Self {
            catalog: CatalogManager::load(loom_dir)?,
            config,
        })
    }

    pub fn with_default_catalog(config: Option<RoutingConfig>) -> Self {
        Self {
            catalog: CatalogManager::from_catalog(CatalogManager::default_catalog()),
            config,
        }
    }

    pub fn resolve(&self, score: ComplexityScore) -> RoutingDecision {
        let tier = if let Some(cfg) = &self.config {
            // If an explicit default_tier is set, use it directly.
            if let Some(forced) = cfg.default_tier {
                forced
            } else {
                // Determine tier from complexity thresholds.
                let base_tier = {
                    let thresh = &cfg.complexity_thresholds;
                    if score.numeric < thresh.low_max {
                        ModelTier::Slm
                    } else if score.numeric < thresh.high_min {
                        ModelTier::Mid
                    } else {
                        ModelTier::Flagship
                    }
                };
                // Apply strategy adjustments to base_tier.
                match cfg.strategy {
                    RoutingStrategy::QualityOptimize => ModelTier::Flagship,
                    RoutingStrategy::CostOptimize => base_tier,
                    _ => base_tier,
                }
            }
        } else {
            score.model_tier
        };

        let model = self
            .catalog
            .best_for_tier(&tier)
            .cloned()
            .unwrap_or_else(|| {
                CatalogManager::default_catalog()
                    .models
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| {
                        // Absolute fallback: hardcoded default
                        tracing::warn!("default catalog empty, using hardcoded model");
                        loom_core::ModelEntry {
                            model_id: "claude-sonnet-4".into(),
                            provider_id: "anthropic".into(),
                            tier: loom_core::ModelTier::Mid,
                            context_window: 200_000,
                            input_cost_per_1k: 0.003,
                            output_cost_per_1k: 0.015,
                            speed_tier: loom_core::SpeedTier::Medium,
                            capabilities: vec![],
                            available: true,
                        }
                    })
            });

        let input_tokens = 1000u32;
        let output_tokens = 500u32;
        let cost = (input_tokens as f64 / 1000.0) * model.input_cost_per_1k
            + (output_tokens as f64 / 1000.0) * model.output_cost_per_1k;

        RoutingDecision {
            recommended_tier: tier,
            recommended_provider: model.provider_id.clone(),
            recommended_model: model.model_id.clone(),
            reason: format!(
                "complexity={:?} numeric={:.2} strategy={:?}",
                score.level,
                score.numeric,
                self.config
                    .as_ref()
                    .map(|c| &c.strategy)
                    .unwrap_or(&RoutingStrategy::Balanced)
            ),
            overrides_role_default: false,
            decided_at: crate::util::rfc3339_now(),
            cost_estimate: CostEstimate {
                input_tokens_estimated: input_tokens,
                output_tokens_estimated: output_tokens,
                estimated_cost_usd: cost,
                confidence: "medium".into(),
            },
        }
    }

    /// Get reference to the routing config, if any.
    pub fn config(&self) -> Option<&RoutingConfig> {
        self.config.as_ref()
    }

    /// Access the underlying model catalog.
    pub fn catalog(&self) -> &loom_core::ModelCatalog {
        self.catalog.catalog()
    }
}
