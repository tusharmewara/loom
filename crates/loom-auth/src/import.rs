use anyhow::{Context, Result};
use loom_core::{AuthMethod, ProviderEntry, ProviderStatus};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{debug, info, warn};

/// Describes an auth provider found in a tool's TOML config.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolAuthConfig {
    /// The provider identifier (e.g., "openai", "anthropic")
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Provider name (human-readable)
    #[serde(default)]
    pub name: Option<String>,
    /// Authentication method
    #[serde(default)]
    pub auth_method: Option<String>,
    /// API base URL
    #[serde(default)]
    pub api_base: Option<String>,
    /// Models endpoint
    #[serde(default)]
    pub models_endpoint: Option<String>,
    /// Credential value (API key, token, etc.)
    #[serde(default)]
    pub credential: Option<String>,
    /// Path to credential file
    #[serde(default)]
    pub credential_file: Option<String>,
    /// Path to auth store for lookup
    #[serde(default)]
    pub auth_store: Option<String>,
    /// Supported tools
    #[serde(default)]
    pub supported_tools: Vec<String>,
}

/// Outer structure for tool config TOML files
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolConfig {
    #[serde(default)]
    pub provider: Option<ToolAuthConfig>,
    #[serde(default)]
    pub auth: Option<ToolAuthConfig>,
    /// Top-level tool name
    #[serde(default)]
    pub name: Option<String>,
}

/// Scans tool TOML configuration files and auto-imports auth provider entries.
///
/// The import process:
/// 1. Scans all `.toml` files in `{loom_dir}/tools/`
/// 2. Parses `auth` and `provider` sections for credential info
/// 3. Imports credentials into the vault if an unlocked vault is provided
/// 4. Registers providers in the registry
///
/// Also supports importing from explicit `auth_store` paths referenced in tool configs.
pub struct AutoImporter {
    loom_dir: String,
}

impl AutoImporter {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Scan all tool TOML files and discover auth provider configurations.
    pub fn scan_tools(&self) -> Result<Vec<ScannedProvider>> {
        let tools_dir = Path::new(&self.loom_dir).join("tools");
        if !tools_dir.exists() {
            debug!("tools dir does not exist: {:?}", tools_dir);
            return Ok(Vec::new());
        }

        let mut providers = Vec::new();
        let mut errors = Vec::new();

        for entry in fs::read_dir(&tools_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                match self.scan_tool_file(&path) {
                    Ok(mut discovered) => {
                        let tool_name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        for p in &mut discovered {
                            if p.tool_source.is_empty() {
                                p.tool_source = tool_name.clone();
                            }
                        }
                        providers.extend(discovered);
                    }
                    Err(e) => {
                        errors.push(format!("{}: {e}", path.display()));
                        warn!(path = ?path, error = %e, "failed to scan tool config");
                    }
                }
            }
        }

        if !errors.is_empty() {
            warn!("scan completed with {} errors", errors.len());
        }

        Ok(providers)
    }

    /// Scan a single tool TOML file for auth configurations.
    fn scan_tool_file(&self, path: &Path) -> Result<Vec<ScannedProvider>> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read tool config: {:?}", path))?;
        let config: ToolConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse tool config: {:?}", path))?;

        let mut providers = Vec::new();

        // Check for auth section
        if let Some(auth) = &config.auth {
            if let Some(provider_id) = &auth.provider_id {
                providers.push(ScannedProvider {
                    provider_id: provider_id.clone(),
                    name: auth.name.clone().unwrap_or_else(|| provider_id.clone()),
                    auth_method: parse_auth_method(auth.auth_method.as_deref()),
                    api_base_url: auth.api_base.clone().unwrap_or_default(),
                    models_endpoint: auth.models_endpoint.clone().unwrap_or_default(),
                    credential: auth.credential.clone(),
                    credential_file: auth.credential_file.clone(),
                    auth_store: auth.auth_store.clone(),
                    supported_tools: auth.supported_tools.clone(),
                    tool_source: String::new(),
                    source_path: path.to_string_lossy().to_string(),
                });
            }
        }

        // Check for provider section
        if let Some(provider) = &config.provider {
            if let Some(provider_id) = &provider.provider_id {
                providers.push(ScannedProvider {
                    provider_id: provider_id.clone(),
                    name: provider.name.clone().unwrap_or_else(|| provider_id.clone()),
                    auth_method: parse_auth_method(provider.auth_method.as_deref()),
                    api_base_url: provider.api_base.clone().unwrap_or_default(),
                    models_endpoint: provider.models_endpoint.clone().unwrap_or_default(),
                    credential: provider.credential.clone(),
                    credential_file: provider.credential_file.clone(),
                    auth_store: provider.auth_store.clone(),
                    supported_tools: provider.supported_tools.clone(),
                    tool_source: String::new(),
                    source_path: path.to_string_lossy().to_string(),
                });
            }
        }

        Ok(providers)
    }

    /// Import a scanned provider into the registry and vault.
    ///
    /// If `vault` is provided and unlocked, credentials will be imported.
    /// The `save_registry` flag controls whether the registry is persisted after import.
    pub fn import_provider(
        &self,
        scanned: &ScannedProvider,
        registry: &crate::providers::ProviderRegistry,
        vault: Option<&crate::vault::CredentialVault>,
        save_registry: bool,
    ) -> Result<ProviderEntry> {
        // Resolve credential if available
        let credential_value = if let Some(cred) = &scanned.credential {
            Some(cred.clone())
        } else if let Some(cred_file) = &scanned.credential_file {
            let cred_path = Path::new(cred_file);
            if cred_path.is_absolute() {
                Some(
                    fs::read_to_string(cred_path)
                        .with_context(|| format!("failed to read credential file: {cred_file}"))?
                        .trim()
                        .to_string(),
                )
            } else {
                // Resolve relative to loom_dir
                let abs_path = Path::new(&self.loom_dir).join(cred_file);
                Some(
                    fs::read_to_string(&abs_path)
                        .with_context(|| {
                            format!("failed to read credential file: {:?}", abs_path)
                        })?
                        .trim()
                        .to_string(),
                )
            }
        } else {
            None
        };

        let is_active = credential_value.is_some();

        // Store credential in vault if available and vault is provided
        if let (Some(cred), Some(vault)) = (&credential_value, vault) {
            if !vault.is_locked() {
                vault.set_credential(&scanned.provider_id, cred)?;
                debug!(
                    provider_id = &scanned.provider_id,
                    "credential imported into vault"
                );
            } else {
                warn!(
                    provider_id = &scanned.provider_id,
                    "vault is locked, credential not stored"
                );
            }
        }

        // Build provider entry
        let entry = ProviderEntry {
            provider_id: scanned.provider_id.clone(),
            name: scanned.name.clone(),
            auth_method: scanned.auth_method.clone(),
            api_base_url: scanned.api_base_url.clone(),
            models_endpoint: scanned.models_endpoint.clone(),
            credential_ref: format!("{}.enc", scanned.provider_id),
            status: if is_active {
                ProviderStatus::Active
            } else {
                ProviderStatus::Unconfigured
            },
            last_validated: iso_now(),
            supported_tools: scanned.supported_tools.clone(),
        };

        // Register
        registry.register(entry.clone())?;

        if save_registry {
            registry.save()?;
        }

        info!(
            provider_id = &entry.provider_id,
            status = ?entry.status,
            "provider imported from tool config"
        );

        Ok(entry)
    }

    /// Run a full import scan: scan tools, import all discovered providers.
    pub fn import_all(
        &self,
        registry: &crate::providers::ProviderRegistry,
        vault: Option<&crate::vault::CredentialVault>,
    ) -> Result<Vec<ProviderEntry>> {
        let scanned = self.scan_tools()?;
        let mut imported = Vec::new();

        for provider in &scanned {
            match self.import_provider(provider, registry, vault, false) {
                Ok(entry) => imported.push(entry),
                Err(e) => {
                    warn!(
                        provider_id = &provider.provider_id,
                        error = %e,
                        "failed to import provider"
                    );
                }
            }
        }

        // Save registry once at the end
        registry.save()?;

        info!(
            total = scanned.len(),
            imported = imported.len(),
            "auto-import completed"
        );

        Ok(imported)
    }

    /// Import providers from a specific auth_store TOML file.
    ///
    /// This supports the case where a tool config has an `auth_store` field
    /// pointing to a separate TOML file with provider configurations.
    pub fn import_from_auth_store(
        &self,
        auth_store_path: &str,
        registry: &crate::providers::ProviderRegistry,
        vault: Option<&crate::vault::CredentialVault>,
    ) -> Result<Vec<ProviderEntry>> {
        let path = Path::new(auth_store_path);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            Path::new(&self.loom_dir).join(auth_store_path)
        };

        let content = fs::read_to_string(&resolved)
            .with_context(|| format!("failed to read auth store: {:?}", resolved))?;

        // Try to parse as a list of providers
        #[derive(Debug, Deserialize)]
        struct AuthStore {
            #[serde(default)]
            provider: Option<ToolAuthConfig>,
            #[serde(default)]
            providers: Option<Vec<ToolAuthConfig>>,
        }

        let store: AuthStore = toml::from_str(&content)
            .with_context(|| format!("failed to parse auth store: {:?}", resolved))?;

        let configs: Vec<ToolAuthConfig> = if let Some(providers) = store.providers {
            providers
        } else if let Some(provider) = store.provider {
            vec![provider]
        } else {
            Vec::new()
        };

        let mut imported = Vec::new();
        for cfg in configs {
            if let Some(provider_id) = &cfg.provider_id {
                let scanned = ScannedProvider {
                    provider_id: provider_id.clone(),
                    name: cfg.name.clone().unwrap_or_else(|| provider_id.clone()),
                    auth_method: parse_auth_method(cfg.auth_method.as_deref()),
                    api_base_url: cfg.api_base.clone().unwrap_or_default(),
                    models_endpoint: cfg.models_endpoint.clone().unwrap_or_default(),
                    credential: cfg.credential.clone(),
                    credential_file: cfg.credential_file.clone(),
                    auth_store: None,
                    supported_tools: cfg.supported_tools.clone(),
                    tool_source: auth_store_path.to_string(),
                    source_path: resolved.to_string_lossy().to_string(),
                };
                match self.import_provider(&scanned, registry, vault, false) {
                    Ok(entry) => imported.push(entry),
                    Err(e) => warn!(
                        provider_id,
                        error = %e,
                        "failed to import from auth_store"
                    ),
                }
            }
        }

        registry.save()?;
        info!(
            source = auth_store_path,
            count = imported.len(),
            "imported from auth_store"
        );

        Ok(imported)
    }
}

/// A provider discovered by scanning tool configs.
#[derive(Debug, Clone)]
pub struct ScannedProvider {
    pub provider_id: String,
    pub name: String,
    pub auth_method: AuthMethod,
    pub api_base_url: String,
    pub models_endpoint: String,
    pub credential: Option<String>,
    pub credential_file: Option<String>,
    pub auth_store: Option<String>,
    pub supported_tools: Vec<String>,
    pub tool_source: String,
    pub source_path: String,
}

fn parse_auth_method(s: Option<&str>) -> AuthMethod {
    match s {
        Some("api_key" | "ApiKey" | "apikey") => AuthMethod::ApiKey,
        Some("oauth2" | "OAuth2" | "oauth") => AuthMethod::OAuth2,
        Some("device_code" | "DeviceCode" | "device") => AuthMethod::DeviceCode,
        Some("custom" | "Custom") => AuthMethod::Custom,
        _ => AuthMethod::ApiKey, // default
    }
}

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::CredentialVault;
    use crate::providers::ProviderRegistry;

    #[test]
    fn test_scan_tool_config_with_auth() {
        let dir = tempfile::TempDir::new().unwrap();
        let tools_dir = dir.path().join("tools");
        fs::create_dir_all(&tools_dir).unwrap();

        let config = r#"
name = "my-tool"
version = "1.0.0"

[auth]
provider_id = "openai"
name = "OpenAI"
auth_method = "ApiKey"
api_base = "https://api.openai.com"
credential = "sk-test123"
supported_tools = ["chat", "completion"]
"#;
        fs::write(tools_dir.join("my-tool.toml"), config).unwrap();

        let importer = AutoImporter::new(dir.path().to_str().unwrap());
        let scanned = importer.scan_tools().unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].provider_id, "openai");
        assert_eq!(scanned[0].credential.as_deref(), Some("sk-test123"));
    }

    #[test]
    fn test_scan_tool_config_with_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let tools_dir = dir.path().join("tools");
        fs::create_dir_all(&tools_dir).unwrap();

        let config = r#"
[provider]
provider_id = "anthropic"
name = "Anthropic"
auth_method = "ApiKey"
api_base = "https://api.anthropic.com"
credential_file = "anthropic_key.txt"
"#;
        fs::write(tools_dir.join("anthropic.toml"), config).unwrap();

        let importer = AutoImporter::new(dir.path().to_str().unwrap());
        let scanned = importer.scan_tools().unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].provider_id, "anthropic");
    }

    #[test]
    fn test_import_provider_into_registry() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = ProviderRegistry::new(dir.path().to_str().unwrap());
        let importer = AutoImporter::new(dir.path().to_str().unwrap());

        let scanned = ScannedProvider {
            provider_id: "test-provider".to_string(),
            name: "Test".to_string(),
            auth_method: AuthMethod::ApiKey,
            api_base_url: "https://api.test.com".to_string(),
            models_endpoint: String::new(),
            credential: Some("sk-test".to_string()),
            credential_file: None,
            auth_store: None,
            supported_tools: vec![],
            tool_source: "test".to_string(),
            source_path: "test.toml".to_string(),
        };

        let entry = importer
            .import_provider(&scanned, &registry, None, true)
            .unwrap();
        assert_eq!(entry.provider_id, "test-provider");
        assert!(matches!(entry.status, ProviderStatus::Active));
    }

    #[test]
    fn test_import_with_vault() {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = ProviderRegistry::new(dir.path().to_str().unwrap());
        let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
        vault.unlock("master-password").unwrap();

        let importer = AutoImporter::new(dir.path().to_str().unwrap());
        let scanned = ScannedProvider {
            provider_id: "vault-test".to_string(),
            name: "Vault Test".to_string(),
            auth_method: AuthMethod::ApiKey,
            api_base_url: "https://api.test.com".to_string(),
            models_endpoint: String::new(),
            credential: Some("sk-vault-secret".to_string()),
            credential_file: None,
            auth_store: None,
            supported_tools: vec![],
            tool_source: "test".to_string(),
            source_path: "test.toml".to_string(),
        };

        importer
            .import_provider(&scanned, &registry, Some(&vault), true)
            .unwrap();

        // Verify credential is in vault
        let cred = vault.get_credential("vault-test").unwrap();
        assert_eq!(cred, Some("sk-vault-secret".to_string()));
    }

    #[test]
    fn test_import_all_integration() {
        let dir = tempfile::TempDir::new().unwrap();
        let tools_dir = dir.path().join("tools");
        fs::create_dir_all(&tools_dir).unwrap();

        // Create two tool configs
        fs::write(
            tools_dir.join("tool-a.toml"),
            r#"
[auth]
provider_id = "provider-a"
name = "Provider A"
credential = "key-a"
"#,
        )
        .unwrap();

        fs::write(
            tools_dir.join("tool-b.toml"),
            r#"
[provider]
provider_id = "provider-b"
name = "Provider B"
credential = "key-b"
"#,
        )
        .unwrap();

        let registry = ProviderRegistry::new(dir.path().to_str().unwrap());
        let importer = AutoImporter::new(dir.path().to_str().unwrap());

        let imported = importer.import_all(&registry, None).unwrap();
        assert_eq!(imported.len(), 2);

        let providers = registry.list();
        assert_eq!(providers.len(), 2);
    }
}
