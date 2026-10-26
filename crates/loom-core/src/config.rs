use crate::types::LoomConfig;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone, Default)]
struct AcpConfig {
    registry_url: Option<String>,
}

/// Options controlling how `ConfigReader::load` resolves the config tree.
///
/// These give callers (CLI, tests) explicit control over which merge steps
/// to apply. All flags default to `false` — when `false`, the corresponding
/// step is applied.
#[derive(Debug, Clone, Default)]
pub struct ConfigOptions {
    /// If true, skip the `extends = [...]` chain from `config.toml` and
    /// any transitive `extends` in the chain itself. Useful for debugging
    /// a base config without the local override, or for the
    /// `--no-extend` CLI flag.
    pub skip_extends: bool,
    /// If true, skip applying the cached `remote_config_url` content
    /// (the file at `~/.loom/.cache/remote_config/<hash>.toml`).
    pub skip_remote: bool,
}

impl ConfigOptions {
    /// Build options from environment variables, on top of the caller's
    /// explicit overrides. Recognized env vars:
    /// - `LOOM_NO_EXTEND=1` → `skip_extends = true`
    /// - `LOOM_NO_REMOTE=1` → `skip_remote = true`
    pub fn from_env(mut self) -> Self {
        if std::env::var("LOOM_NO_EXTEND").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            self.skip_extends = true;
        }
        if std::env::var("LOOM_NO_REMOTE").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        {
            self.skip_remote = true;
        }
        self
    }
}

/// A single step in the config resolution chain — what file (or layer)
/// contributed, in order. Used by `loom config show --extends` to make
/// the precedence visible.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSourceStep {
    /// Short label, e.g. "defaults", "extends", "local", "overlay:routing.toml", "remote".
    pub label: String,
    /// Resolved filesystem path, if any (None for "defaults" and "local").
    pub path: Option<PathBuf>,
    /// The keys this layer contributed, as a list of dotted paths.
    pub contributed_keys: Vec<String>,
}

/// The result of a `load_with_diagnostics` call: the resolved config plus
/// the chain of merge steps, in the order they were applied.
#[derive(Debug, Clone)]
pub struct ConfigDiagnostics {
    pub config: LoomConfig,
    pub steps: Vec<ConfigSourceStep>,
}

/// Load and merge config from `~/.loom/config.toml`
/// with layer-specific overlays from `~/.loom/config/{routing,mcp,hooks,security}.toml`,
/// `extends = ["path"]` inheritance chains, and a cached `remote_config_url`.
///
/// Precedence (low → high):
///   1. Built-in defaults
///   2. Extends chain (in order, left-to-right; first extends is the
///      lowest-priority base, each subsequent one overrides the previous)
///   3. Local `config.toml` (deep-merged on top of the extends chain)
///   4. Layer overlays: `config/routing.toml`, `config/mcp.toml`,
///      `config/hooks.toml`, `config/acp.toml`, `config/security.toml`
///      (each wrapped under its own section key)
///   5. Cached remote config from `remote_config_url` (deep-merged on
///      top of the local+overlays tree; same precedence as the layer
///      overlays — useful for team-wide settings that should be
///      overridable per-machine)
///
/// Skipping layers: see [`ConfigOptions`] (or env vars `LOOM_NO_EXTEND=1`,
/// `LOOM_NO_REMOTE=1`).
pub struct ConfigReader {
    loom_dir: String,
}

impl ConfigReader {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Load the config with default options (applies all layers).
    /// Honors `LOOM_NO_EXTEND=1` / `LOOM_NO_REMOTE=1` env vars.
    pub fn load(&self) -> Result<LoomConfig> {
        self.load_with_options(ConfigOptions::default().from_env())
    }

    /// Load the config with explicit options.
    pub fn load_with_options(&self, options: ConfigOptions) -> Result<LoomConfig> {
        self.load_inner(options, None)
    }

    /// Load the config and capture the resolved source chain (each layer
    /// that contributed, in order). Used by `loom config show --extends`.
    pub fn load_with_diagnostics(&self, options: ConfigOptions) -> Result<ConfigDiagnostics> {
        let mut steps: Vec<ConfigSourceStep> = Vec::new();
        let config = self.load_inner(options, Some(&mut steps))?;
        Ok(ConfigDiagnostics { config, steps })
    }

    /// Save the config to the local config.toml file.
    /// This writes only the local layer (not extends or overlays).
    pub fn save(&self, config: &LoomConfig) -> Result<()> {
        let config_path = Path::new(&self.loom_dir).join("config.toml");
        let toml_str = toml::to_string_pretty(config)?;
        std::fs::write(config_path, toml_str)?;
        Ok(())
    }

    fn load_inner(
        &self,
        options: ConfigOptions,
        mut steps: Option<&mut Vec<ConfigSourceStep>>,
    ) -> Result<LoomConfig> {
        let config_path = Path::new(&self.loom_dir).join("config.toml");

        // Start with defaults as the base
        let mut base_value: serde_json::Value = serde_json::to_value(LoomConfig::default())?;
        if let Some(steps) = steps.as_deref_mut() {
            steps.push(ConfigSourceStep {
                label: "defaults".to_string(),
                path: None,
                contributed_keys: default_keys(),
            });
        }

        // 1. Resolve extends chain from the local config (if it exists)
        if config_path.exists() && !options.skip_extends {
            let raw = std::fs::read_to_string(&config_path)?;
            let local_value = toml_to_value(&raw)?;

            if let Some(extends_paths) = local_value.get("extends").and_then(|v| v.as_array()) {
                for extends_path in extends_paths {
                    if let Some(path_str) = extends_path.as_str() {
                        if let Some((base_val, step)) =
                            self.resolve_extends_with_step(path_str, &mut HashSet::new())?
                        {
                            if let Some(steps) = steps.as_deref_mut() {
                                steps.push(step);
                            }
                            deep_merge(&mut base_value, &base_val);
                        }
                    }
                }
            }

            // 2. Deep-merge the local config on top of the chain
            let keys = collect_keys(&local_value);
            deep_merge(&mut base_value, &local_value);
            if let Some(steps) = steps.as_deref_mut() {
                steps.push(ConfigSourceStep {
                    label: "local".to_string(),
                    path: Some(config_path.clone()),
                    contributed_keys: keys,
                });
            }
        } else if config_path.exists() {
            // skip_extends is set: still apply the local file, just skip
            // the extends chain
            let raw = std::fs::read_to_string(&config_path)?;
            let local_value = toml_to_value(&raw)?;
            let keys = collect_keys(&local_value);
            deep_merge(&mut base_value, &local_value);
            if let Some(steps) = steps.as_deref_mut() {
                steps.push(ConfigSourceStep {
                    label: "local (extends skipped)".to_string(),
                    path: Some(config_path.clone()),
                    contributed_keys: keys,
                });
            }
        }

        // 3. Deep-merge layer-specific overlays
        let config_dir = Path::new(&self.loom_dir).join("config");
        if config_dir.exists() {
            for (filename, section_key) in [
                ("routing.toml", Some("routing")),
                ("mcp.toml", Some("mcp")),
                ("hooks.toml", Some("hooks")),
                ("acp.toml", Some("acp")),
                ("security.toml", Some("security")),
            ] {
                let overlay_path = config_dir.join(filename);
                if !overlay_path.exists() {
                    continue;
                }
                // Capture contributed keys for the diagnostics step
                // before merging (merging consumes the overlay value).
                let contributed_keys = std::fs::read_to_string(&overlay_path)
                    .ok()
                    .and_then(|raw| toml_to_value(&raw).ok())
                    .map(|v| collect_keys(&v))
                    .unwrap_or_default();
                deep_merge_overlay(&mut base_value, &config_dir, filename, section_key);
                if let Some(steps) = steps.as_deref_mut() {
                    let section = section_key.unwrap_or("(top-level)");
                    steps.push(ConfigSourceStep {
                        label: format!("overlay:{filename} → [{section}]"),
                        path: Some(overlay_path),
                        contributed_keys: contributed_keys
                            .iter()
                            .map(|k| format!("{section}.{k}"))
                            .collect(),
                    });
                }
            }
        }

        // 4. Apply the cached remote_config_url content (if any)
        if !options.skip_remote {
            self.apply_remote_config(&mut base_value, steps)?;
        }

        // 5. Interpolate environment variables in string values
        interpolate_env_vars(&mut base_value);

        // 6. Deserialize the merged value into LoomConfig
        let mut config: LoomConfig = serde_json::from_value(base_value)?;

        // 7. Fallback: load ACP registry URL from acp.toml if not already set
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

    /// Apply the cached remote config (if `remote_config_url` is set in
    /// the merged tree and a cached file exists) by deep-merging its
    /// content on top of `base_value`.
    fn apply_remote_config(
        &self,
        base_value: &mut serde_json::Value,
        steps: Option<&mut Vec<ConfigSourceStep>>,
    ) -> Result<()> {
        // Read remote URL from the merged tree
        let remote_url = base_value
            .get("remote_config_url")
            .and_then(|v| v.as_str())
            .map(String::from);

        let Some(remote_url) = remote_url else {
            return Ok(());
        };

        if !remote_url.starts_with("https://") {
            // Same restriction as `loom config fetch`; silently skip
            // non-HTTPS URLs at merge time (validate-time should catch
            // this in `loom config validate`).
            return Ok(());
        }

        // Look up the URL → hash mapping in the cache index
        let cache_dir = Path::new(&self.loom_dir)
            .join(".cache")
            .join("remote_config");
        let index_path = cache_dir.join("index.json");
        if !index_path.exists() {
            // No fetch has happened yet. Don't fail — just skip.
            return Ok(());
        }

        let raw_index = std::fs::read_to_string(&index_path).unwrap_or_default();
        let index: std::collections::HashMap<String, String> =
            serde_json::from_str(&raw_index).unwrap_or_default();
        let url_key = remote_url.trim_end_matches('/').to_string();
        let Some(hash) = index.get(&url_key) else {
            return Ok(());
        };

        let cache_path = cache_dir.join(format!("{hash}.toml"));
        if !cache_path.exists() {
            return Ok(());
        }

        let raw = std::fs::read_to_string(&cache_path)?;
        let remote_value = toml_to_value(&raw)?;
        let keys = collect_keys(&remote_value);
        deep_merge(base_value, &remote_value);

        if let Some(steps) = steps {
            steps.push(ConfigSourceStep {
                label: format!("remote:{remote_url} (sha256:{hash})"),
                path: Some(cache_path),
                contributed_keys: keys,
            });
        }

        Ok(())
    }

    /// Resolve a single extends path, recursively processing sub-extends.
    /// Returns None if the file doesn't exist (non-fatal).
    /// Returns error on cycle or I/O error.
    /// Also returns a `ConfigSourceStep` describing the resolved file's
    /// contributions (for use in [`Self::load_with_diagnostics`]).
    fn resolve_extends_with_step(
        &self,
        raw_path: &str,
        visited: &mut HashSet<PathBuf>,
    ) -> Result<Option<(serde_json::Value, ConfigSourceStep)>> {
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
            if let Some((base_val, _)) = self.resolve_extends_with_step(extends_path, visited)? {
                deep_merge(&mut value, &base_val);
            }
        }

        let keys = collect_keys(&value);
        let step = ConfigSourceStep {
            label: format!("extends:{raw_path}"),
            path: Some(path),
            contributed_keys: keys,
        };
        Ok(Some((value, step)))
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
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
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
fn deep_merge_overlay(
    base: &mut serde_json::Value,
    config_dir: &Path,
    filename: &str,
    section_key: Option<&str>,
) {
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

/// Recursively collect all keys in a JSON value as dotted paths
/// (e.g. `["routing.strategy", "routing.complexity_thresholds.low_max"]`).
fn collect_keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys = Vec::new();
    collect_keys_inner(value, "", &mut keys);
    keys
}

fn collect_keys_inner(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    if let serde_json::Value::Object(map) = value {
        for (k, v) in map {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            if let serde_json::Value::Object(_) = v {
                collect_keys_inner(v, &path, out);
            } else {
                out.push(path);
            }
        }
    }
}

fn default_keys() -> Vec<String> {
    // Surface the top-level fields that `LoomConfig::default()` exposes,
    // so the diagnostics step for "defaults" is meaningful.
    vec![
        "ecosystem.version".to_string(),
        "ecosystem.enabled".to_string(),
        "ecosystem.conflict_resolution".to_string(),
        "ecosystem.layers".to_string(),
        "ecosystem.tools".to_string(),
    ]
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
            workflow: None,
            security: crate::types::SecurityConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_require_signature_defaults_on() {
        let tmp = tempfile::tempdir().unwrap();
        let loom_dir = tmp.path().to_str().unwrap();

        // No config at all → verify-by-default (ADR 0005).
        let empty = ConfigReader::new(loom_dir).load().unwrap();
        assert!(empty.security.require_signature);

        // [security] require_signature = true → flag on.
        std::fs::write(
            tmp.path().join("config.toml"),
            "[security]\nrequire_signature = true\n",
        )
        .unwrap();
        let on = ConfigReader::new(loom_dir).load().unwrap();
        assert!(on.security.require_signature);
    }

    #[test]
    fn security_require_signature_explicit_opt_out() {
        let tmp = tempfile::tempdir().unwrap();
        let loom_dir = tmp.path().to_str().unwrap();

        // Explicit opt-out wins over the default (config layers beat
        // built-in defaults — ConfigReader deep-merges, file wins).
        std::fs::write(
            tmp.path().join("config.toml"),
            "[security]\nrequire_signature = false\n",
        )
        .unwrap();
        let off = ConfigReader::new(loom_dir).load().unwrap();
        assert!(!off.security.require_signature);
    }

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
        assert_eq!(base["routing"]["complexity_thresholds"]["low_max"], 0.5);
        assert_eq!(base["routing"]["complexity_thresholds"]["high_min"], 0.7);
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
            format!(
                "extends = [\"{}\"]\n[ecosystem]\nversion = \"1.0.0\"\n",
                b_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        std::fs::write(
            &b_path,
            format!(
                "extends = [\"{}\"]\n[ecosystem]\nversion = \"2.0.0\"\n",
                a_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        // Main config extends a
        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                "extends = [\"{}\"]\n",
                a_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let result = reader.load();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("cycle") || err.contains("depth") || err.contains("already visited"),
            "Error should mention cycle/depth: {err}"
        );
    }

    #[test]
    fn test_extends_with_skip_extends_option() {
        // When skip_extends is set, extends chain is bypassed.
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

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

        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                r#"
extends = ["{}"]

[ecosystem]
version = "0.2.0"
"#,
                base_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        // Without skip: base extends applied (default_tier inherited)
        let reader = ConfigReader::new(&loom_dir);
        let config_normal = reader.load().unwrap();
        assert_eq!(
            config_normal.routing.as_ref().and_then(|r| r.default_tier),
            Some(crate::ModelTier::Mid),
            "Without skip_extends, base.toml's default_tier is inherited"
        );

        // With skip: extends chain ignored
        let config_no_extend = reader
            .load_with_options(ConfigOptions {
                skip_extends: true,
                skip_remote: true,
            })
            .unwrap();
        assert_eq!(config_no_extend.ecosystem.version, "0.2.0");
        // Base TOML's routing field should NOT be inherited when extends
        // is skipped (only config.toml is read).
        assert!(
            config_no_extend.routing.is_none()
                || config_no_extend
                    .routing
                    .as_ref()
                    .and_then(|r| r.default_tier)
                    .is_none(),
            "With skip_extends, base.toml's routing is NOT inherited"
        );
    }

    #[test]
    fn test_remote_config_applied_when_cached() {
        // When remote_config_url is set in config.toml and a cached file
        // exists in .cache/remote_config/<hash>.toml, the cached content
        // is deep-merged on top of the local+overlays tree.
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        // Local config with remote_config_url
        std::fs::write(
            dir.path().join("config.toml"),
            r#"
remote_config_url = "https://example.com/team-base.toml"

[ecosystem]
version = "0.3.9"
"#,
        )
        .unwrap();

        // Pre-populate the cache with a known remote config
        let cache_dir = dir.path().join(".cache").join("remote_config");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let remote_body = r#"
[ecosystem]
version = "team-base-1.0.0"
enabled = false

[routing]
strategy = "CostOptimize"
"#;
        let remote_hash = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(remote_body.as_bytes()))
        };
        std::fs::write(cache_dir.join(format!("{remote_hash}.toml")), remote_body).unwrap();
        std::fs::write(
            cache_dir.join("index.json"),
            serde_json::json!({
                "https://example.com/team-base.toml": remote_hash
            })
            .to_string(),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let config = reader.load().unwrap();

        // Local version "0.3.9" should be overridden by remote "team-base-1.0.0"
        // because the remote merge happens after the local merge.
        assert_eq!(config.ecosystem.version, "team-base-1.0.0");
        assert!(!config.ecosystem.enabled, "remote sets enabled=false");
        // Remote routing.strategy is applied (routing was None locally).
        assert!(config.routing.is_some());
    }

    #[test]
    fn test_remote_config_not_applied_when_skip_remote() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        std::fs::write(
            dir.path().join("config.toml"),
            r#"
remote_config_url = "https://example.com/team-base.toml"

[ecosystem]
version = "0.3.9"
"#,
        )
        .unwrap();

        let cache_dir = dir.path().join(".cache").join("remote_config");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let remote_body = r#"[ecosystem]
version = "team-base-1.0.0"
"#;
        let remote_hash = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(remote_body.as_bytes()))
        };
        std::fs::write(cache_dir.join(format!("{remote_hash}.toml")), remote_body).unwrap();
        std::fs::write(
            cache_dir.join("index.json"),
            serde_json::json!({
                "https://example.com/team-base.toml": remote_hash
            })
            .to_string(),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let config = reader
            .load_with_options(ConfigOptions {
                skip_extends: false,
                skip_remote: true,
            })
            .unwrap();
        // Local version preserved (remote was skipped).
        assert_eq!(config.ecosystem.version, "0.3.9");
    }

    #[test]
    fn test_remote_config_silently_skipped_when_not_fetched() {
        // If remote_config_url is set but no cache file exists,
        // loading should succeed without error (skip silently).
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        std::fs::write(
            dir.path().join("config.toml"),
            r#"
remote_config_url = "https://example.com/missing.toml"

[ecosystem]
version = "0.3.9"
"#,
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let config = reader.load().unwrap();
        // No cache file, so local config is unchanged.
        assert_eq!(config.ecosystem.version, "0.3.9");
    }

    #[test]
    fn test_diagnostics_returns_resolved_chain() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        let base_path = dir.path().join("base.toml");
        std::fs::write(
            &base_path,
            r#"[ecosystem]
version = "0.1.0"
enabled = true
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                r#"extends = ["{}"]

[ecosystem]
version = "0.2.0"
"#,
                base_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let diag = reader
            .load_with_diagnostics(ConfigOptions::default())
            .unwrap();

        // The steps should include "defaults", "extends:...", "local".
        let labels: Vec<&str> = diag.steps.iter().map(|s| s.label.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.starts_with("defaults")),
            "steps: {labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.starts_with("extends:")),
            "steps: {labels:?}"
        );
        assert!(labels.contains(&"local"), "steps: {labels:?}");

        // The extends step should report a path.
        let extends_step = diag
            .steps
            .iter()
            .find(|s| s.label.starts_with("extends:"))
            .unwrap();
        assert!(extends_step.path.is_some());
    }

    #[test]
    fn test_precedence_stack_full() {
        // Verify the full precedence stack:
        // 1. defaults (lowest)
        // 2. extends chain
        // 3. local config.toml
        // 4. config/ overlays (routing.toml, etc.)
        // 5. cached remote (highest)
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        // Base (step 2)
        let base_path = dir.path().join("base.toml");
        std::fs::write(
            &base_path,
            r#"[ecosystem]
version = "0.0.0"
conflict_resolution = "SharedFirst"
"#,
        )
        .unwrap();

        // Local (step 3)
        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                r#"extends = ["{}"]
remote_config_url = "https://example.com/team.toml"

[ecosystem]
version = "0.3.9"
"#,
                base_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        // Overlay (step 4): routing.toml wraps under [routing]
        let config_subdir = dir.path().join("config");
        std::fs::create_dir_all(&config_subdir).unwrap();
        std::fs::write(
            config_subdir.join("routing.toml"),
            r#"
strategy = "OverlayStrategy"
default_tier = "Slm"
"#,
        )
        .unwrap();

        // Remote (step 5)
        let cache_dir = dir.path().join(".cache").join("remote_config");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let remote_body = r#"
[ecosystem]
version = "team-final-1.0.0"

[routing]
strategy = "CostOptimize"
default_tier = "Flagship"
"#;
        let remote_hash = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(remote_body.as_bytes()))
        };
        std::fs::write(cache_dir.join(format!("{remote_hash}.toml")), remote_body).unwrap();
        std::fs::write(
            cache_dir.join("index.json"),
            serde_json::json!({
                "https://example.com/team.toml": remote_hash
            })
            .to_string(),
        )
        .unwrap();

        let reader = ConfigReader::new(&loom_dir);
        let config = reader.load().unwrap();

        // Highest precedence: remote
        assert_eq!(config.ecosystem.version, "team-final-1.0.0");
        assert_eq!(
            config.routing.as_ref().map(|r| format!("{:?}", r.strategy)),
            Some("CostOptimize".to_string())
        );
        assert_eq!(
            config.routing.as_ref().and_then(|r| r.default_tier),
            Some(crate::ModelTier::Flagship)
        );
    }

    #[test]
    fn test_options_from_env_respects_no_extend() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom_dir = dir.path().to_str().unwrap().to_string();

        let base_path = dir.path().join("base.toml");
        std::fs::write(
            &base_path,
            r#"[ecosystem]
version = "0.1.0"
enabled = true
conflict_resolution = "BaseFirst"
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("config.toml"),
            format!(
                r#"extends = ["{}"]

[ecosystem]
version = "0.2.0"
"#,
                base_path.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();

        // Test from_env() directly without polluting global env state.
        // Mutating std::env from a #[test] runs in parallel with the other
        // tests in this module, which call from_env()/load() and read the
        // same env vars — a data race that made test_extends_cycle_detection
        // intermittently see LOOM_NO_EXTEND=1 and silently skip the cycle.
        // To avoid the race entirely we run the env-sensitive assertion in a
        // child process (whose env mutations cannot leak into this process).
        //
        // The child runs the dedicated probe below with the target env vars
        // set via Command::env; its exit status is the assertion result.
        let exe = std::env::current_exe().unwrap();
        let child = std::process::Command::new(&exe)
            .env("LOOM_CHILD_PROBE", "1")
            .env("LOOM_NO_EXTEND", "1")
            .env("LOOM_NO_REMOTE", "1")
            .arg("--exact")
            .arg("config::tests::from_env_probe")
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "from_env probe failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );

        // Test the actual skip behavior using the captured options
        // (NOT a second from_env() call which would re-read the env).
        let reader = ConfigReader::new(&loom_dir);
        let config = reader
            .load_with_options(ConfigOptions {
                skip_extends: true,
                skip_remote: false,
            })
            .unwrap();

        // Local wins because extends is skipped
        assert_eq!(config.ecosystem.version, "0.2.0");
        // Base's `conflict_resolution = "BaseFirst"` was NOT inherited
        // (extends skipped) — falls back to the default `SharedFirst`.
        assert_ne!(
            format!("{:?}", config.ecosystem.conflict_resolution),
            "BaseFirst"
        );
    }

    /// Dedicated child-process probe for [`ConfigOptions::from_env`].
    ///
    /// Only meaningful when run by `test_options_from_env_respects_no_extend`
    /// via `Command::env(...)` with `LOOM_CHILD_PROBE` and the target env vars
    /// set. In a normal (parent) test-suite run this is a harmless no-op, so
    /// the env-sensitive assertions never execute in-process and therefore
    /// cannot race with the other env-reading tests.
    #[test]
    fn from_env_probe() {
        if std::env::var("LOOM_CHILD_PROBE").is_err() {
            return;
        }
        let options = ConfigOptions::default().from_env();
        assert!(
            options.skip_extends,
            "from_env should set skip_extends when LOOM_NO_EXTEND=1"
        );
        assert!(
            options.skip_remote,
            "from_env should set skip_remote when LOOM_NO_REMOTE=1"
        );
    }
}
