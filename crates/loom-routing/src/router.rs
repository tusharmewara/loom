use crate::catalog::CatalogManager;
use anyhow::Result;
use loom_core::{
    derive_level, derive_tier, ComplexityScore, CostEstimate, ModelTier, RoutingConfig,
    RoutingDecision, RoutingStrategy,
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
                // Determine tier from complexity thresholds — the same
                // derivation that produces the level (routing-debt Step 3).
                let base_tier =
                    derive_tier(derive_level(score.numeric, &cfg.complexity_thresholds));
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

        // The cost estimate is a FORECAST made before the run exists, so it
        // is derived from the prompt size actually scored
        // (`description_length` is the prompt's character count) rather than
        // a fixed token count. `CostEstimate::estimated` labels it
        // `Estimated` with its method, so a reader can never mistake it for
        // a measured cost.
        let cost_estimate = CostEstimate::estimated(
            score.signals.description_length,
            model.input_cost_per_1k,
            model.output_cost_per_1k,
            "medium",
        );

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
            decided_at: loom_core::rfc3339_now(),
            cost_estimate,
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
