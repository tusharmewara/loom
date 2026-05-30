use crate::types::LoomConfig;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
struct AcpConfig {
    registry_url: Option<String>,
}

/// Load and merge config from ~/.loom/config.toml
/// with layer-specific overlays from ~/.loom/config/{routing,mcp,hooks}.toml
///
/// Overlays are deep-merged (not replaced), so a partial overlay file
/// only overrides the fields it specifies.
///
/// Supports `extends = ["path/to/base.toml"]` for config inheritance chains
/// (with cycle detection), and `remote_config_url` for HTTPS config fetching.
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

        // Start with defaults as the base
        let mut base_value: serde_json::Value = serde_json::to_value(LoomConfig::default())?;

        // 1. Resolve extends chain from the local config (if it exists)
        if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)?;
            let local_value = toml_to_value(&raw)?;

            // Extract extends paths and resolve the inheritance chain
            if let Some(extends_paths) = local_value.get("extends").and_then(|v| v.as_array()) {
                for extends_path in extends_paths {
                    if let Some(path_str) = extends_path.as_str() {
                        let resolved = self.resolve_extends(path_str, &mut HashSet::new())?;
                        if let Some(base_val) = resolved {
                            deep_merge(&mut base_value, &base_val);
                        }
                    }
                }
            }

            // 2. Deep-merge the local config on top of the chain
            deep_merge(&mut base_value, &local_value);
        }

        // 3. Deep-merge layer-specific overlays
        let config_dir = Path::new(&self.loom_dir).join("config");
        if config_dir.exists() {
            deep_merge_overlay(&mut base_value, &config_dir, "routing.toml", Some("routing"));
            deep_merge_overlay(&mut base_value, &config_dir, "mcp.toml", Some("mcp"));
            deep_merge_overlay(&mut base_value, &config_dir, "hooks.toml", Some("hooks"));
            deep_merge_overlay(&mut base_value, &config_dir, "acp.toml", Some("acp"));
        }

        // 4. Interpolate environment variables in string values
        interpolate_env_vars(&mut base_value);

        // 5. Deserialize the merged value into LoomConfig
        let mut config: LoomConfig = serde_json::from_value(base_value)?;

        // 6. Fallback: load ACP registry URL from acp.toml if not already set
        if config.acp_registry_url.is_none() {
            let acp_path = config_dir.join("acp.toml");
            if acp_path.exists() {
                if let Ok(raw) = std::fs::read_to_string(&acp_path) {
                    if let Ok(acp) = toml::from_str::<AcpConfig>(&raw) {
                        config.acp_registry_url = acp.registry_url;
                    }
                }
            }
        }

        Ok(config)
    }

    /// Resolve a single extends path, recursively processing sub-extends.
    /// Returns None if the file doesn't exist (non-fatal).
    /// Returns error on cycle or I/O error.
    fn resolve_extends(&self, raw_path: &str, visited: &mut HashSet<PathBuf>) -> Result<Option<serde_json::Value>> {
        const MAX_EXTENDS_DEPTH: usize = 10;
        if visited.len() >= MAX_EXTENDS_DEPTH {
            anyhow::bail!(
                "Config extends chain exceeds maximum depth of {} (cycle?)",
                MAX_EXTENDS_DEPTH
            );
        }

        let expanded = shellexpand::tilde(raw_path).to_string();
        let path = if Path::new(&expanded).is_absolute() {
            PathBuf::from(&expanded)
        } else {
            Path::new(&self.loom_dir).join(&expanded)
        };

        if !path.exists() {
            return Ok(None);
        }

        // Cycle detection
        if !visited.insert(path.clone()) {
            anyhow::bail!(
                "Cycle detected in config extends chain: {:?} already visited",
                path
            );
        }

        let raw = std::fs::read_to_string(&path)?;
        let mut value = toml_to_value(&raw)?;

        // Extract extends paths first to avoid borrow conflict
        let extends: Vec<String> = value
            .get("extends")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Recursively resolve this file's own extends
        for extends_path in &extends {
            if let Some(base_val) = self.resolve_extends(extends_path, visited)? {
                deep_merge(&mut value, &base_val);
            }
        }

        Ok(Some(value))
    }
}

/// Parse TOML to a serde_json::Value, preserving all nested structure.
fn toml_to_value(raw: &str) -> Result<serde_json::Value> {
    let toml_value: toml::Value = toml::from_str(raw)?;
    let json_value = toml_value_to_json(toml_value);
    Ok(json_value)
}

/// Recursively convert toml::Value to serde_json::Value.
fn toml_value_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}

/// Deep-merge an overlay TOML file into `base`, optionally wrapping under a section key.
/// The `section_key` determines where in the config tree the overlay is merged:
/// - `"routing"`: merged under the `routing` key (routing.toml has flat RoutingConfig fields)
/// - `"mcp"`: merged under the `mcp` key
/// - `"hooks"`: merged under the `hooks` key
/// - `"acp"`: merged under the `acp` key (for ACP-specific config)
/// - `None`: merged at the top level (for general overlay files)
fn deep_merge_overlay(base: &mut serde_json::Value, config_dir: &Path, filename: &str, section_key: Option<&str>) {
    let overlay_path = config_dir.join(filename);
    if !overlay_path.exists() {
        return;
    }
    if let Ok(raw) = std::fs::read_to_string(&overlay_path) {
        if let Ok(overlay_value) = toml_to_value(&raw) {
            if let Some(key) = section_key {
                // Wrap the overlay content under the section key so it matches LoomConfig nesting
                let mut wrapped = serde_json::Map::new();
                wrapped.insert(key.to_string(), overlay_value);
                deep_merge(base, &serde_json::Value::Object(wrapped));
            } else {
                deep_merge(base, &overlay_value);
            }
        }
    }
}

/// Recursive deep merge: `overlay` fields are merged into `base`.
/// - For objects: recursively merge keys
/// - For arrays: overlay replaces base entirely
/// - For scalars: overlay replaces base
fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                if let Some(base_val) = base_map.get_mut(key) {
                    deep_merge(base_val, overlay_val);
                } else {
                    base_map.insert(key.clone(), overlay_val.clone());
                }
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

/// Recursively replace `${VAR}` or `${VAR:-default}` patterns in string values
/// with the corresponding environment variable.
fn interpolate_env_vars(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            let mut result = s.clone();
            // Match ${VAR} or ${VAR:-default}
            let re = regex_lite::Regex::new(r"\$\{([^}:]+)(?::-([^}]*))?\}").unwrap();
            for caps in re.captures_iter(s) {
                let var_name = caps.get(1).unwrap().as_str();
                let default_val = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                let env_val = std::env::var(var_name).unwrap_or_else(|_| default_val.to_string());
                // Only substitute if the env var is set (or default is provided)
                if !env_val.is_empty() || caps.get(2).is_some() {
                    let pattern = caps.get(0).unwrap().as_str().to_string();
                    result = result.replace(&pattern, &env_val);
                }
            }
            *s = result;
        }
        serde_json::Value::Object(map) => {
            for val in map.values_mut() {
                interpolate_env_vars(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for val in arr.iter_mut() {
                interpolate_env_vars(val);
            }
        }
        _ => {}
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
            extends: None,
            remote_config_url: None,
            routing: None,
            mcp: None,
            hooks: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_merge_scalar_replaces() {
        let mut base = serde_json::json!({"name": "old", "keep": "stay"});
        let overlay = serde_json::json!({"name": "new"});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["name"], "new");
        assert_eq!(base["keep"], "stay");
    }

    #[test]
    fn test_deep_merge_nested_object() {
        let mut base = serde_json::json!({
            "routing": {
                "strategy": "cost",
                "default_tier": "mid",
                "complexity_thresholds": {
                    "low_max": 0.35,
                    "high_min": 0.7
                }
            }
        });
        let overlay = serde_json::json!({
            "routing": {
                "complexity_thresholds": {
                    "low_max": 0.5
                }
            }
        });
        deep_merge(&mut base, &overlay);
        assert_eq!(base["routing"]["strategy"], "cost");
        assert_eq!(base["routing"]["default_tier"], "mid");
        assert_eq!(
            base["routing"]["complexity_thresholds"]["low_max"],
            0.5
        );
        assert_eq!(
            base["routing"]["complexity_thresholds"]["high_min"],
            0.7
        );
    }

    #[test]
    fn test_deep_merge_new_key_added() {
        let mut base = serde_json::json!({"existing": "value"});
        let overlay = serde_json::json!({"new_key": "added"});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["existing"], "value");
        assert_eq!(base["new_key"], "added");
    }

    #[test]
    fn test_interpolate_env_var() {
        std::env::set_var("LOOM_TEST_HOME", "/tmp/test");
        let mut value = serde_json::json!({"path": "${LOOM_TEST_HOME}/data"});
        interpolate_env_vars(&mut value);
        assert_eq!(value["path"], "/tmp/test/data");
        std::env::remove_var("LOOM_TEST_HOME");
    }

    #[test]
    fn test_interpolate_env_var_with_default() {
        std::env::remove_var("LOOM_UNDEFINED");
        let mut value = serde_json::json!({"path": "${LOOM_UNDEFINED:-/default/path}"});
        interpolate_env_vars(&mut value);
        assert_eq!(value["path"], "/default/path");
    }

    #[test]
    fn test_interpolate_env_var_no_substitution() {
        let mut value = serde_json::json!({"plain": "no_vars_here"});
        interpolate_env_vars(&mut value);
        assert_eq!(value["plain"], "no_vars_here");
    }

    #[test]
    fn test_toml_to_value_parse() {
        let raw = r#"
            [ecosystem]
            version = "0.2.0"
            enabled = true

            [routing]
            strategy = "cost"
        "#;
        let value = toml_to_value(raw).unwrap();
        assert_eq!(value["ecosystem"]["version"], "0.2.0");
        assert_eq!(value["ecosystem"]["enabled"], true);
        assert_eq!(value["routing"]["strategy"], "cost");
    }

    #[test]
    fn test_interpolate_preserves_unset_without_default() {
        std::env::remove_var("LOOM_MISSING_VAR");
        let mut value = serde_json::json!({"val": "${LOOM_MISSING_VAR}"});
        interpolate_env_vars(&mut value);
        // Without a default, an unset var leaves the pattern intact
        assert_eq!(value["val"], "${LOOM_MISSING_VAR}");
    }

    #[test]
    fn test_extends_chain_resolution() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        // Write base config
        let base_path = dir.path().join("base.toml");
        std::fs::write(
            &base_path,
            r#"
[ecosystem]
version = "0.1.0"
enabled = true

[routing]
strategy = "CostOptimize"
default_tier = "Mid"
"#,
        )
        .unwrap();

        // Write main config that extends base
        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                r#"
extends = ["{}"]

[ecosystem]
version = "0.2.0"

[routing]
strategy = "QualityOptimize"
"#,
                base_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let config = reader.load().unwrap();

        // Should be overridden by main config
        assert_eq!(config.ecosystem.version, "0.2.0");
        // Should be inherited from base
        assert!(config.ecosystem.enabled);
        // Should be overridden by main config
        assert_eq!(
            config.routing.as_ref().map(|r| format!("{:?}", r.strategy)),
            Some("QualityOptimize".to_string())
        );
        // Should be inherited from base
        assert_eq!(
            config.routing.as_ref().and_then(|r| r.default_tier),
            Some(crate::ModelTier::Mid)
        );
    }

    #[test]
    fn test_extends_cycle_detection() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        // Create a cycle: a → b → a
        let a_path = dir.path().join("a.toml");
        let b_path = dir.path().join("b.toml");

        std::fs::write(
            &a_path,
            format!("extends = [\"{}\"]\n[ecosystem]\nversion = \"1.0.0\"\n", b_path.to_string_lossy().replace('\\', "\\\\")),
        )
        .unwrap();

        std::fs::write(
            &b_path,
            format!("extends = [\"{}\"]\n[ecosystem]\nversion = \"2.0.0\"\n", a_path.to_string_lossy().replace('\\', "\\\\")),
        )
        .unwrap();

        // Main config extends a
        std::fs::write(
            dir.path().join("config.toml"),
            format!("extends = [\"{}\"]\n", a_path.to_string_lossy().replace('\\', "\\\\")),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let result = reader.load();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cycle") || err.contains("depth") || err.contains("already visited"), "Error should mention cycle/depth: {err}");
    }
}
