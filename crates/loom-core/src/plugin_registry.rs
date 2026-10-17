use crate::PluginManifest;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Plugin downloads are HTTPS-only, mirroring remote-config loading
/// (config.rs). Rejects http://, ftp://, and other schemes before any
/// network I/O (routing-debt.md Step 5).
fn validate_https_url(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "refusing non-HTTPS plugin URL '{url}': plugin downloads are HTTPS-only, like remote-config loading"
        );
    }
    Ok(())
}

/// Verify a plugin tool TOML's Ed25519 signature.
///
/// Trust model (routing-debt.md Step 5): signatures are optional by
/// default — unsigned artifacts are allowed because everything in the
/// loom dir is user-owned local state. When `require_signature` is
/// true (config `[security]`), unsigned manifests are rejected.
///
/// Returns `Ok(())` if `public_key` and `signature` are present and the
/// signature authenticates the content, or if both are absent and
/// `require_signature` is false. Returns an error if one field is
/// present without the other, if the signature is invalid, or if both
/// are absent while `require_signature` is true.
pub fn verify_plugin_signature(
    manifest: &PluginManifest,
    content: &[u8],
    require_signature: bool,
) -> Result<()> {
    match (&manifest.public_key, &manifest.signature) {
        (Some(pk_b64), Some(sig_hex)) => {
            use ed25519_dalek::{Signature, Verifier, VerifyingKey};

            use base64::Engine;
            let pk_bytes = base64::engine::general_purpose::STANDARD
                .decode(pk_b64)
                .context("invalid base64 public_key")?;
            let pk =
                VerifyingKey::from_bytes(&pk_bytes.try_into().map_err(|_| {
                    anyhow::anyhow!("invalid public key length (expected 32 bytes)")
                })?)?;

            let sig_bytes = hex::decode(sig_hex).context("invalid hex signature")?;
            let sig = Signature::from_slice(&sig_bytes)
                .map_err(|e| anyhow::anyhow!("invalid signature: {e}"))?;

            pk.verify(content, &sig)
                .map_err(|e| anyhow::anyhow!("plugin signature verification failed: {e}"))?;

            Ok(())
        }
        (None, None) => {
            if require_signature {
                anyhow::bail!(
                    "plugin '{}' is unsigned and security.require_signature is enabled",
                    manifest.name
                );
            }
            Ok(()) // unsigned — allowed by default
        }
        (Some(_), None) => anyhow::bail!("plugin has public_key but no signature"),
        (None, Some(_)) => anyhow::bail!("plugin has signature but no public_key"),
    }
}

/// Manages the plugin registry stored in `~/.loom/plugins/`.
pub struct PluginRegistry {
    plugins_dir: PathBuf,
}

impl PluginRegistry {
    /// Create a new plugin registry rooted under `loom_dir/plugins`.
    pub fn new(loom_dir: &str) -> Self {
        let plugins_dir = Path::new(loom_dir).join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);
        Self { plugins_dir }
    }

    /// Path to a plugin's TOML manifest.
    fn plugin_path(&self, name: &str) -> PathBuf {
        self.plugins_dir.join(format!("{name}.toml"))
    }

    /// List all registered plugins.
    pub fn list(&self) -> Result<Vec<PluginManifest>> {
        let mut plugins = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.plugins_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    let raw = std::fs::read_to_string(&path)
                        .with_context(|| format!("Failed to read {}", path.display()))?;
                    match toml::from_str::<PluginManifest>(&raw) {
                        Ok(p) => plugins.push(p),
                        Err(e) => {
                            tracing::warn!("invalid plugin manifest '{}': {e}", path.display());
                        }
                    }
                }
            }
        }
        plugins.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(plugins)
    }

    /// Get a specific plugin by name.
    pub fn get(&self, name: &str) -> Result<PluginManifest> {
        let path = self.plugin_path(name);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Plugin '{}' not found at {}", name, path.display()))?;
        let manifest: PluginManifest =
            toml::from_str(&raw).with_context(|| format!("Invalid plugin manifest '{}'", name))?;
        Ok(manifest)
    }

    /// Install a plugin from a tool TOML URL, creating a PluginManifest.
    pub fn install_from_url(
        &self,
        name: &str,
        url: &str,
        version: Option<&str>,
    ) -> Result<PluginManifest> {
        // Validate the plugin name (alphanumeric + hyphens)
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!(
                "Invalid plugin name '{}': only alphanumeric, hyphens, and underscores allowed",
                name
            );
        }

        // Check if already installed
        if self.plugin_path(name).exists() {
            anyhow::bail!("Plugin '{}' is already installed", name);
        }

        // Plugin downloads are HTTPS-only (routing-debt.md Step 5)
        validate_https_url(url)?;

        // Attempt to download the tool TOML
        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow::anyhow!("Failed to download tool TOML from '{}': {}", url, e))?;

        let body = response
            .into_string()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        // Validate that it's parseable TOML (must at least resemble a tool config)
        if let Err(e) = toml::from_str::<toml::Value>(&body) {
            anyhow::bail!("Downloaded file is not valid TOML: {e}");
        }

        // Compute checksum
        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Save the tool TOML to the tools directory
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        std::fs::create_dir_all(&tools_dir)?;
        let tool_path = tools_dir.join(format!("{name}.toml"));
        std::fs::write(&tool_path, &body)?;

        // Record the linked tool
        println!("  Downloaded tool TOML to {}", tool_path.display());

        // Create and save the plugin manifest
        let manifest = PluginManifest {
            name: name.to_string(),
            version: version.unwrap_or("0.1.0").to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: Some(url.to_string()),
            installed_at: Some(chrono::Utc::now().to_rfc3339()),
            checksum: Some(checksum),
            public_key: None,
            signature: None,
        };

        self.save(&manifest)?;

        Ok(manifest)
    }

    /// Register a plugin from a local tool TOML file (no download).
    pub fn register_local(&self, name: &str, tool_path: &Path) -> Result<PluginManifest> {
        let raw = std::fs::read_to_string(tool_path)
            .with_context(|| format!("Failed to read {}", tool_path.display()))?;

        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(raw.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Copy into tools directory
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        std::fs::create_dir_all(&tools_dir)?;
        let dest = tools_dir.join(format!("{name}.toml"));
        std::fs::copy(tool_path, &dest)?;

        let manifest = PluginManifest {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: None,
            installed_at: Some(chrono::Utc::now().to_rfc3339()),
            checksum: Some(checksum),
            public_key: None,
            signature: None,
        };

        self.save(&manifest)?;
        Ok(manifest)
    }

    /// Remove a plugin and its associated tool TOML.
    pub fn remove(&self, name: &str) -> Result<()> {
        // Remove plugin manifest
        let plugin_path = self.plugin_path(name);
        if plugin_path.exists() {
            std::fs::remove_file(&plugin_path)?;
        } else {
            anyhow::bail!("Plugin '{}' not found", name);
        }

        // Optionally remove the tool TOML (don't fail if it doesn't exist)
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        let tool_path = tools_dir.join(format!("{name}.toml"));
        if tool_path.exists() {
            std::fs::remove_file(&tool_path)?;
            println!("  Removed tool config: {}", tool_path.display());
        }

        Ok(())
    }

    /// Update a plugin: re-download tool TOML if it has a tool_toml_url.
    pub fn update(&self, name: &str) -> Result<PluginManifest> {
        let existing = self.get(name)?;

        let url = existing.tool_toml_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Plugin '{}' has no tool_toml_url to update from", name)
        })?;

        // Plugin downloads are HTTPS-only (routing-debt.md Step 5)
        validate_https_url(url)?;

        // Download new version
        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow::anyhow!("Failed to download tool TOML: {}", e))?;

        let body = response
            .into_string()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        // Validate TOML
        if let Err(e) = toml::from_str::<toml::Value>(&body) {
            anyhow::bail!("Downloaded file is not valid TOML: {e}");
        }

        // Compute new checksum
        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Save tool TOML
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        let tool_path = tools_dir.join(format!("{name}.toml"));
        std::fs::write(&tool_path, &body)?;

        // Create updated manifest
        let mut updated = existing.clone();
        updated.checksum = Some(checksum);
        updated.installed_at = Some(chrono::Utc::now().to_rfc3339());
        // Bump minor version
        if let Ok(ver) = semver::Version::parse(&updated.version) {
            updated.version = format!("{}.{}.{}", ver.major, ver.minor + 1, ver.patch);
        }

        self.save(&updated)?;
        Ok(updated)
    }

    /// Search installed plugins by name/description.
    pub fn search(&self, query: &str) -> Result<Vec<PluginManifest>> {
        let query_lower = query.to_lowercase();
        let plugins = self.list()?;
        Ok(plugins
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query_lower)
                    || p.description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query_lower)
                    || p.author
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query_lower)
            })
            .collect())
    }

    /// Save a plugin manifest to disk.
    fn save(&self, manifest: &PluginManifest) -> Result<()> {
        std::fs::create_dir_all(&self.plugins_dir)?;
        let path = self.plugin_path(&manifest.name);
        let raw = toml::to_string_pretty(manifest)?;
        std::fs::write(&path, raw)?;
        Ok(())
    }

    /// Count installed plugins.
    pub fn count(&self) -> usize {
        self.list().map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_install_urls_must_be_https() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());
        let err = registry
            .install_from_url("evil", "http://evil.example/x.toml", None)
            .unwrap_err();
        assert!(err.to_string().contains("HTTPS"), "got: {err}");
        // Rejected before any download: nothing was written.
        assert!(!tmp.path().join("plugins").join("evil.toml").exists());
    }

    #[test]
    fn plugin_update_urls_must_be_https() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());
        // Hand-write a manifest pointing at an http:// URL.
        std::fs::create_dir_all(tmp.path().join("plugins")).unwrap();
        std::fs::write(
            tmp.path().join("plugins").join("x.toml"),
            "name = \"x\"\nversion = \"0.1.0\"\ntool_toml_url = \"http://evil.example/x.toml\"\n",
        )
        .unwrap();
        let err = registry.update("x").unwrap_err();
        assert!(err.to_string().contains("HTTPS"), "got: {err}");
    }

    fn unsigned_manifest() -> PluginManifest {
        PluginManifest {
            name: "x".to_string(),
            version: "0.1.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: None,
            installed_at: None,
            checksum: None,
            public_key: None,
            signature: None,
        }
    }

    #[test]
    fn unsigned_plugin_policy_follows_require_signature() {
        let m = unsigned_manifest();
        // Historic default: unsigned is allowed.
        assert!(verify_plugin_signature(&m, b"content", false).is_ok());
        // require_signature = true rejects unsigned manifests.
        let err = verify_plugin_signature(&m, b"content", true).unwrap_err();
        assert!(err.to_string().contains("unsigned"), "got: {err}");
    }

    #[test]
    fn half_signed_plugin_manifests_are_rejected() {
        let mut m = unsigned_manifest();
        m.public_key = Some("AAAA".to_string());
        assert!(verify_plugin_signature(&m, b"c", false).is_err());
        assert!(verify_plugin_signature(&m, b"c", true).is_err());

        let mut m = unsigned_manifest();
        m.signature = Some("deadbeef".to_string());
        assert!(verify_plugin_signature(&m, b"c", false).is_err());
        assert!(verify_plugin_signature(&m, b"c", true).is_err());
    }
}
