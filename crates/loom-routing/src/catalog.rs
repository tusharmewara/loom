use anyhow::{Context, Result};
use loom_core::{ModelCatalog, ModelEntry, ModelTier, ProviderStatus, SpeedTier};
use std::path::Path;

const DEFAULT_CATALOG: &[(&str, &str, ModelTier, f64, f64)] = &[
    ("gpt-4o-mini", "openai", ModelTier::Slm, 0.00015, 0.0006),
    ("claude-sonnet-4", "anthropic", ModelTier::Mid, 0.003, 0.015),
    ("claude-opus-4", "anthropic", ModelTier::Flagship, 0.015, 0.075),
];

pub struct CatalogManager {
    catalog: ModelCatalog,
}

impl CatalogManager {
    pub fn from_catalog(catalog: ModelCatalog) -> Self {
        Self { catalog }
    }

    pub fn load(agents_dir: &str) -> Result<Self> {
        let path = Path::new(agents_dir)
            .join("auth")
            .join("model-catalog.json");
        let catalog = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            serde_json::from_str(&raw)?
        } else {
            Self::default_catalog()
        };
        Ok(Self { catalog })
    }

    pub fn default_catalog() -> ModelCatalog {
        ModelCatalog {
            models: DEFAULT_CATALOG
                .iter()
                .map(|(id, provider, tier, in_cost, out_cost)| ModelEntry {
                    model_id: (*id).into(),
                    provider_id: (*provider).into(),
                    tier: *tier,
                    context_window: 128_000,
                    input_cost_per_1k: *in_cost,
                    output_cost_per_1k: *out_cost,
                    speed_tier: SpeedTier::Medium,
                    capabilities: vec![],
                    available: true,
                })
                .collect(),
        }
    }

    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    pub fn models_for_tier(&self, tier: &ModelTier) -> Vec<&ModelEntry> {
        self.catalog
            .models
            .iter()
            .filter(|m| m.available && &m.tier == tier)
            .collect()
    }

    pub fn best_for_tier(&self, tier: &ModelTier) -> Option<&ModelEntry> {
        self.models_for_tier(tier).into_iter().min_by(|a, b| {
            a.input_cost_per_1k
                .partial_cmp(&b.input_cost_per_1k)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Refresh the catalog from all active providers.
    /// Currently resets to default catalog (placeholder for live provider queries).
    pub fn refresh_from_providers(agents_dir: &str) -> Result<()> {
        // In a full implementation, this would:
        // 1. Load ProviderRegistry
        // 2. For each active provider, read credential and query models endpoint
        // 3. Merge results into a new ModelCatalog
        // For now, rebuild from built-in defaults to ensure the file exists and is valid.
        let catalog = Self::default_catalog();
        let path = Path::new(agents_dir).join("auth").join("model-catalog.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&catalog)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// Filter catalog entries to providers that are active in the registry.
pub fn filter_active_providers(
    catalog: &ModelCatalog,
    active_provider_ids: &[String],
) -> ModelCatalog {
    if active_provider_ids.is_empty() {
        return catalog.clone();
    }
    ModelCatalog {
        models: catalog
            .models
            .iter()
            .filter(|m| active_provider_ids.contains(&m.provider_id))
            .cloned()
            .collect(),
    }
}

/// Placeholder for provider status filtering when no live registry is attached.
pub fn filter_by_status(catalog: &ModelCatalog, _status: ProviderStatus) -> ModelCatalog {
    catalog.clone()
}
