use crate::types::LoomConfig;
use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone, Default)]
struct AcpConfig {
    registry_url: Option<String>,
}

/// Load and merge config from ~/.loom/config.toml
/// with layer-specific overlays from ~/.loom/config/{routing,mcp,hooks}.toml
pub struct ConfigReader {
    loom_dir: String,
}

impl ConfigReader {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    pub fn load(&self) -> Result<LoomConfig> {
        let config_path = Path::new(&self.loom_dir).join("config.toml");
        let mut config: LoomConfig = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)?;
            toml::from_str(&raw)?
        } else {
            LoomConfig::default()
        };

        // Overlay layer-specific configs
        let config_dir = Path::new(&self.loom_dir).join("config");
        if config_dir.exists() {
            if let Ok(raw) = std::fs::read_to_string(config_dir.join("routing.toml")) {
                if let Ok(routing) = toml::from_str(&raw) {
                    config.routing = Some(routing);
                }
            }
            if let Ok(raw) = std::fs::read_to_string(config_dir.join("mcp.toml")) {
                if let Ok(mcp) = toml::from_str(&raw) {
                    config.mcp = Some(mcp);
                }
            }
            if let Ok(raw) = std::fs::read_to_string(config_dir.join("hooks.toml")) {
                if let Ok(hooks) = toml::from_str(&raw) {
                    config.hooks = Some(hooks);
                }
            }
        }

        // Load ACP registry URL if present
        if config.acp_registry_url.is_some() {
            // Already set from config.toml
        } else if let Ok(raw) = std::fs::read_to_string(config_dir.join("acp.toml")) {
            if let Ok(acp) = toml::from_str::<AcpConfig>(&raw) {
                config.acp_registry_url = acp.registry_url.clone();
            }
        }

        Ok(config)
    }
}

impl Default for LoomConfig {
    fn default() -> Self {
        Self {
            ecosystem: crate::types::EcosystemConfig {
                version: env!("CARGO_PKG_VERSION").to_string(),
                enabled: true,
                conflict_resolution: crate::types::ConflictResolution::NativeFirst,
                layers: crate::types::LayerFlags::default(),
                tools: crate::types::ToolAccess::default(),
            },
            acp_registry_url: None,
            routing: None,
            mcp: None,
            hooks: None,
        }
    }
}
