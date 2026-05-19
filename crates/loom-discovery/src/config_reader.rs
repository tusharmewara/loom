use std::path::Path;
use std::collections::HashMap;
use std::sync::Mutex;

/// Layer-specific config reader with lazy loading.
///
/// Loads ~/.agents/config.toml first, then overlays layer-specific configs
/// from ~/.agents/config/routing.toml, mcp.toml, and hooks.toml.
/// Configs are loaded on first access and cached in memory.
pub struct ConfigReader {
    agents_dir: String,
    cache: Mutex<HashMap<String, serde_json::Value>>,
}

impl ConfigReader {
    pub fn new(agents_dir: &str) -> Self {
        Self {
            agents_dir: agents_dir.to_string(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Load the global config.toml with layer overlays.
    pub fn load_global(&self) -> anyhow::Result<Option<loom_core::LoomConfig>> {
        let path = Path::new(&self.agents_dir).join("config.toml");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut config: loom_core::LoomConfig = toml::from_str(&content)?;

        // Overlay layer-specific configs
        if let Some(routing) = self.load_overlay::<loom_core::RoutingConfig>("routing.toml")? {
            config.routing = Some(routing);
        }
        if let Some(mcp) = self.load_overlay::<loom_core::McpGlobalConfig>("mcp.toml")? {
            config.mcp = Some(mcp);
        }
        if let Some(hooks) = self.load_overlay::<loom_core::HooksGlobalConfig>("hooks.toml")? {
            config.hooks = Some(hooks);
        }

        // Cache as JSON
        let json = serde_json::to_value(&config)?;
        self.cache.lock().unwrap().insert("global".into(), json);

        Ok(Some(config))
    }

    /// Load an individual layer config file from ~/.agents/config/{name}.
    fn load_overlay<T: serde::de::DeserializeOwned>(&self, filename: &str) -> anyhow::Result<Option<T>> {
        let path = Path::new(&self.agents_dir).join("config").join(filename);
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let value: T = toml::from_str(&content)?;
        Ok(Some(value))
    }

    /// Get a global config value from cache.
    pub fn get_cached(&self, key: &str) -> Option<serde_json::Value> {
        self.cache.lock().ok()?.get(key).cloned()
    }

    /// Invalidate cache (e.g., after config file change detected).
    pub fn invalidate(&self) {
        self.cache.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_reader_empty_dir() {
        let dir = std::env::temp_dir().join("loom-test-config-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let reader = ConfigReader::new(dir.to_str().unwrap());
        let result = reader.load_global().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_config_reader_with_basic_config() {
        let dir = std::env::temp_dir().join("loom-test-config-basic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config_content = r#"
[ecosystem]
version = "2.0.0"
enabled = true
"#;
        std::fs::write(dir.join("config.toml"), config_content).unwrap();

        let reader = ConfigReader::new(dir.to_str().unwrap());
        let config = reader.load_global().unwrap().unwrap();
        assert_eq!(config.ecosystem.version, "2.0.0");
        assert!(config.ecosystem.enabled);
    }

    #[test]
    fn test_config_reader_with_layer_overlays() {
        let dir = std::env::temp_dir().join("loom-test-config-overlay");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();

        let config_content = r#"
[ecosystem]
version = "2.0.0"
enabled = true
"#;
        std::fs::write(dir.join("config.toml"), config_content).unwrap();

        let routing_content = r#"
strategy = "CostOptimize"
"#;
        std::fs::write(dir.join("config").join("routing.toml"), routing_content).unwrap();

        let reader = ConfigReader::new(dir.to_str().unwrap());
        let config = reader.load_global().unwrap().unwrap();
        assert!(config.routing.is_some());
        assert!(matches!(config.routing.unwrap().strategy, loom_core::RoutingStrategy::CostOptimize));
    }
}
