use anyhow::{Context, Result};
use loom_core::{ModelCatalog, ModelEntry, ModelTier, SpeedTier};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

// ─── Incoming JSON shapes from provider model-list endpoints ─────────

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProviderFile {
    version: u32,
    providers: Vec<ProviderRecord>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProviderRecord {
    provider_id: String,
    name: String,
    auth_method: String,
    api_base_url: String,
    #[serde(default)]
    models_endpoint: String,
    status: String,
    last_validated: String,
    #[serde(default)]
    supported_tools: Vec<String>,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelRecord>,
}

#[derive(Deserialize)]
struct ModelRecord {
    id: String,
}

// ─── Default catalog (fallback) ─────────────────────────────────────

const DEFAULT_CATALOG: &[(&str, &str, ModelTier, f64, f64, u32)] = &[
    ("gpt-4o-mini", "openai", ModelTier::Slm, 0.00015, 0.0006, 128_000),
    ("gpt-4o", "openai", ModelTier::Mid, 0.0025, 0.01, 128_000),
    ("gpt-4", "openai", ModelTier::Mid, 0.03, 0.06, 8_192),
    ("o1", "openai", ModelTier::Flagship, 0.015, 0.06, 200_000),
    ("o3-mini", "openai", ModelTier::Flagship, 0.0011, 0.0044, 200_000),
    ("claude-sonnet-4", "anthropic", ModelTier::Mid, 0.003, 0.015, 200_000),
    ("claude-3-5-sonnet-latest", "anthropic", ModelTier::Mid, 0.003, 0.015, 200_000),
    ("claude-3-5-haiku-latest", "anthropic", ModelTier::Slm, 0.0008, 0.004, 200_000),
    ("claude-opus-4", "anthropic", ModelTier::Flagship, 0.015, 0.075, 200_000),
    ("claude-3-opus-latest", "anthropic", ModelTier::Flagship, 0.015, 0.075, 200_000),
    ("gemini-2.0-flash", "google", ModelTier::Slm, 0.0001, 0.0004, 1_048_576),
    ("gemini-2.0-flash-lite", "google", ModelTier::Slm, 0.000075, 0.0003, 1_048_576),
    ("gemini-2.5-pro-exp-03-25", "google", ModelTier::Flagship, 0.00125, 0.01, 1_048_576),
    ("deepseek-chat", "deepseek", ModelTier::Mid, 0.00027, 0.0011, 128_000),
    ("deepseek-reasoner", "deepseek", ModelTier::Flagship, 0.00055, 0.00219, 128_000),
];

// ─── CatalogManager ─────────────────────────────────────────────────

pub struct CatalogManager {
    catalog: ModelCatalog,
}

impl CatalogManager {
    pub fn from_catalog(catalog: ModelCatalog) -> Self {
        Self { catalog }
    }

    pub fn load(loom_dir: &str) -> Result<Self> {
        let path = Path::new(loom_dir).join("auth").join("model-catalog.json");
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
                .map(|(id, provider, tier, in_cost, out_cost, ctx)| ModelEntry {
                    model_id: (*id).into(),
                    provider_id: (*provider).into(),
                    tier: *tier,
                    context_window: *ctx,
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

    /// Refresh the catalog by querying each registered provider's models
    /// endpoint, then merging discovered models with the built-in defaults.
    /// Skips providers with no `models_endpoint` configured.
    pub fn refresh_from_providers(loom_dir: &str) -> Result<()> {
        let providers = load_providers(loom_dir)?;
        if providers.is_empty() {
            // No providers registered — write defaults
            write_catalog(loom_dir, &Self::default_catalog())?;
            return Ok(());
        }

        let mut discovered: HashMap<String, Vec<ModelEntry>> = HashMap::new();

        let rt = tokio::runtime::Runtime::new()?;
        for provider in &providers {
            if provider.models_endpoint.is_empty() {
                continue;
            }
            let api_key = resolve_api_key(&provider.provider_id);
            let Some(key) = api_key else {
                continue;
            };

            match rt.block_on(fetch_models(&provider.models_endpoint, &key)) {
                Ok(models) => {
                    let entries = models
                        .into_iter()
                        .filter(|id| !id.is_empty())
                        .map(|id| {
                            let tier = infer_tier(&id);
                            let ctx = infer_context_window(&id, &provider.provider_id);
                            let (in_cost, out_cost) = infer_cost(&id, &tier);
                            ModelEntry {
                                model_id: id,
                                provider_id: provider.provider_id.clone(),
                                tier,
                                context_window: ctx,
                                input_cost_per_1k: in_cost,
                                output_cost_per_1k: out_cost,
                                speed_tier: SpeedTier::Medium,
                                capabilities: vec![],
                                available: true,
                            }
                        })
                        .collect();
                    discovered.insert(provider.provider_id.clone(), entries);
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to fetch models from {}: {}",
                        provider.provider_id,
                        e
                    );
                }
            }
        }

        // Merge: start with defaults, overlay discovered models
        let mut merged = Self::default_catalog();
        for models in discovered.values() {
            // Replace default entries with same model_id (keep default pricing)
            // and append truly new models
            for m in models {
                if !merged.models.iter().any(|existing| existing.model_id == m.model_id) {
                    merged.models.push(m.clone());
                }
            }
        }

        // Remove unavailable entries
        merged.models.retain(|m| m.available);
        merged.models.sort_by(|a, b| a.provider_id.cmp(&b.provider_id).then(a.model_id.cmp(&b.model_id)));

        write_catalog(loom_dir, &merged)?;
        Ok(())
    }
}

// ─── Provider loading ───────────────────────────────────────────────

fn load_providers(loom_dir: &str) -> Result<Vec<ProviderRecord>> {
    let path = Path::new(loom_dir).join("auth").join("providers.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&path)?;
    let file: ProviderFile = serde_json::from_str(&raw)?;
    Ok(file.providers)
}

// ─── API key resolution ─────────────────────────────────────────────

fn resolve_api_key(provider_id: &str) -> Option<String> {
    match provider_id {
        "anthropic" | "claude" => std::env::var("ANTHROPIC_API_KEY").ok(),
        "openai" | "gpt" => std::env::var("OPENAI_API_KEY").ok(),
        "google" | "gemini" => std::env::var("GOOGLE_API_KEY").ok(),
        "deepseek" => std::env::var("DEEPSEEK_API_KEY").ok()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok()),
        _ => std::env::var("OPENAI_API_KEY").ok()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok()),
    }
}

// ─── HTTP model discovery ───────────────────────────────────────────

async fn fetch_models(endpoint: &str, api_key: &str) -> Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp: ModelsResponse = client
        .get(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("User-Agent", "loom/0.1")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp.data.into_iter().map(|r| r.id).collect())
}

// ─── Model inference helpers ────────────────────────────────────────

fn infer_tier(model_id: &str) -> ModelTier {
    let lower = model_id.to_lowercase();
    if lower.contains("mini")
        || lower.contains("small")
        || lower.contains("light")
        || lower.contains("flash-lite")
        || lower.contains("haiku")
        || lower.contains("nano")
    {
        ModelTier::Slm
    } else if lower.contains("flagship")
        || lower.contains("opus")
        || lower.contains("ultra")
        || lower.contains("large")
        || lower.contains("o1")
        || lower.contains("o3")
        || lower.contains("2.5")
        || lower.contains("max")
    {
        ModelTier::Flagship
    } else {
        ModelTier::Mid
    }
}

fn infer_context_window(model_id: &str, provider_id: &str) -> u32 {
    let lower = model_id.to_lowercase();
    match provider_id {
        "anthropic" | "claude" => 200_000,
        "google" | "gemini" => 1_048_576,
        "deepseek" => 128_000,
        _ => {
            if lower.contains("gpt-3.5") || lower.contains("gpt-4") && !lower.contains("turbo") && !lower.contains("o1") && !lower.contains("o3") {
                8_192
            } else if lower.contains("o1") || lower.contains("o3") {
                200_000
            } else {
                128_000
            }
        }
    }
}

fn infer_cost(_model_id: &str, tier: &ModelTier) -> (f64, f64) {
    // Best-effort defaults when pricing is unknown
    match tier {
        ModelTier::Slm => (0.0002, 0.0008),
        ModelTier::Mid => (0.003, 0.015),
        ModelTier::Flagship => (0.015, 0.075),
    }
}

// ─── Persistence ────────────────────────────────────────────────────

fn write_catalog(loom_dir: &str, catalog: &ModelCatalog) -> Result<()> {
    let path = Path::new(loom_dir).join("auth").join("model-catalog.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(catalog)?;
    std::fs::write(path, json)?;
    Ok(())
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
pub fn filter_by_status(catalog: &ModelCatalog, _status: loom_core::ProviderStatus) -> ModelCatalog {
    catalog.clone()
}
