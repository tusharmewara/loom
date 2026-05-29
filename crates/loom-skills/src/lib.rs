pub mod adapter;
pub mod hot_reload;
pub mod loader;
pub mod lockfile;
pub mod manager;
pub mod registry;

pub use adapter::{adapt_skill, ToolAdapterConfig};
pub use hot_reload::SkillWatcher;
pub use loader::SkillLoader;

use loom_core::HealthCheck;
use std::sync::Arc;

pub struct SkillsEngine {
    #[allow(dead_code)]
    loom_dir: String,
    registry: registry::SkillRegistry,
    loader: SkillLoader,
}

impl SkillsEngine {
    pub fn new(loom_dir: &str) -> anyhow::Result<Self> {
        Ok(Self {
            loom_dir: loom_dir.to_string(),
            registry: registry::SkillRegistry::new(loom_dir)?,
            loader: SkillLoader::new(loom_dir),
        })
    }

    pub fn registry(&self) -> &registry::SkillRegistry {
        &self.registry
    }

    pub fn loader(&self) -> &SkillLoader {
        &self.loader
    }

    pub fn as_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl HealthCheck for SkillsEngine {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-skills",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: std::collections::HashMap::new(),
        })
    }
}
