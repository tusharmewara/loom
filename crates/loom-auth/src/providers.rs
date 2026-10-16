use anyhow::{Context, Result};
use loom_core::{AuthMethod, ProviderEntry, ProviderStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// Serializable provider metadata stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderMeta {
    pub provider_id: String,
    pub name: String,
    pub auth_method: String, // serialized as string for file format stability
    pub api_base_url: String,
    pub models_endpoint: String,
    pub status: String,
    pub last_validated: String,
    pub supported_tools: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ProviderEntry> for ProviderMeta {
    fn from(e: ProviderEntry) -> Self {
        Self {
            provider_id: e.provider_id,
            name: e.name,
            auth_method: format!("{:?}", e.auth_method),
            api_base_url: e.api_base_url,
            models_endpoint: e.models_endpoint,
            status: format!("{:?}", e.status),
            last_validated: e.last_validated,
            supported_tools: e.supported_tools,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

impl TryInto<ProviderEntry> for ProviderMeta {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<ProviderEntry> {
        let auth_method = match self.auth_method.as_str() {
            "ApiKey" => AuthMethod::ApiKey,
            "OAuth2" => AuthMethod::OAuth2,
            "DeviceCode" => AuthMethod::DeviceCode,
            "Custom" => AuthMethod::Custom,
            _ => AuthMethod::Custom,
        };
        let status = match self.status.as_str() {
            "Active" => ProviderStatus::Active,
            "Expired" => ProviderStatus::Expired,
            _ => ProviderStatus::Unconfigured,
        };
        let provider_id = self.provider_id;
        Ok(ProviderEntry {
            credential_ref: format!("{}.enc", provider_id),
            provider_id,
            name: self.name,
            auth_method,
            api_base_url: self.api_base_url,
            models_endpoint: self.models_endpoint,
            status,
            last_validated: self.last_validated,
            supported_tools: self.supported_tools,
        })
    }
}

/// Provider registry file on disk
#[derive(Debug, Serialize, Deserialize)]
struct ProviderFile {
    version: u32,
    providers: Vec<ProviderMeta>,
}

impl Default for ProviderFile {
    fn default() -> Self {
        Self {
            version: 1,
            providers: Vec::new(),
        }
    }
}

/// Tracks provider authentication status and metadata.
///
/// Thread-safe via `RwLock`. Persists to disk for daemon restarts.
pub struct ProviderRegistry {
    inner: RwLock<RegistryInner>,
    loom_dir: String,
}

struct RegistryInner {
    providers: HashMap<String, ProviderEntry>,
    dirty: bool,
}

impl ProviderRegistry {
    /// Create a new empty provider registry.
    pub fn new(loom_dir: &str) -> Self {
        Self {
            inner: RwLock::new(RegistryInner {
                providers: HashMap::new(),
                dirty: false,
            }),
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Load provider registry from disk.
    pub fn load(loom_dir: &str) -> Result<Self> {
        let registry = Self::new(loom_dir);
        registry.reload()?;
        Ok(registry)
    }

    /// Register a new provider or update an existing one.
    /// Returns `true` if this was a new registration.
    pub fn register(&self, entry: ProviderEntry) -> Result<bool> {
        let mut inner = self.inner.write().expect("registry lock poisoned");
        let pid = entry.provider_id.clone();
        let is_new = !inner.providers.contains_key(&pid);
        inner.providers.insert(pid.clone(), entry);
        inner.dirty = true;
        if is_new {
            debug!(provider_id = %pid, "provider registered");
        } else {
            debug!(provider_id = %pid, "provider updated");
        }
        Ok(is_new)
    }

    /// Unregister a provider by ID.
    pub fn unregister(&self, provider_id: &str) -> bool {
        let mut inner = self.inner.write().expect("registry lock poisoned");
        let removed = inner.providers.remove(provider_id).is_some();
        if removed {
            inner.dirty = true;
            info!(provider_id, "provider unregistered");
        }
        removed
    }

    /// Get a provider entry by ID.
    pub fn get(&self, provider_id: &str) -> Option<ProviderEntry> {
        let inner = self.inner.read().expect("registry lock poisoned");
        inner.providers.get(provider_id).cloned()
    }

    /// List all registered providers.
    pub fn list(&self) -> Vec<ProviderEntry> {
        let inner = self.inner.read().expect("registry lock poisoned");
        let mut entries: Vec<_> = inner.providers.values().cloned().collect();
        entries.sort_by(|a, b| a.provider_id.cmp(&b.provider_id));
        entries
    }

    /// List providers filtered by status.
    pub fn list_by_status(&self, status: ProviderStatus) -> Vec<ProviderEntry> {
        let inner = self.inner.read().expect("registry lock poisoned");
        let mut entries: Vec<_> = inner
            .providers
            .values()
            .filter(|e| std::mem::discriminant(&e.status) == std::mem::discriminant(&status))
            .cloned()
            .collect();
        entries.sort_by(|a, b| a.provider_id.cmp(&b.provider_id));
        entries
    }

    /// Update the status of a provider.
    pub fn update_status(&self, provider_id: &str, status: ProviderStatus) -> Result<bool> {
        let mut inner = self.inner.write().expect("registry lock poisoned");
        let Some(entry) = inner.providers.get_mut(provider_id) else {
            return Ok(false);
        };
        entry.status = status.clone();
        entry.last_validated = loom_core::rfc3339_now();
        inner.dirty = true;
        debug!(provider_id, status = ?status, "provider status updated");
        Ok(true)
    }

    /// Check if a provider is active.
    pub fn is_active(&self, provider_id: &str) -> bool {
        let inner = self.inner.read().expect("registry lock poisoned");
        inner
            .providers
            .get(provider_id)
            .map(|e| matches!(e.status, ProviderStatus::Active))
            .unwrap_or(false)
    }

    /// Count providers by status.
    pub fn status_counts(&self) -> HashMap<String, usize> {
        let inner = self.inner.read().expect("registry lock poisoned");
        let mut counts = HashMap::new();
        for entry in inner.providers.values() {
            let key = format!("{:?}", entry.status);
            *counts.entry(key).or_insert(0) += 1;
        }
        counts
    }

    /// Total number of registered providers.
    pub fn len(&self) -> usize {
        let inner = self.inner.read().expect("registry lock poisoned");
        inner.providers.len()
    }

    /// Check if registry is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ─── Persistence ───────────────────────────────────────────────

    fn registry_path(&self) -> std::path::PathBuf {
        Path::new(&self.loom_dir)
            .join("auth")
            .join("providers.json")
    }

    /// Save provider registry to disk.
    pub fn save(&self) -> Result<()> {
        let path = self.registry_path();
        let inner = self.inner.read().expect("registry lock poisoned");

        let metas: Vec<ProviderMeta> = inner
            .providers
            .values()
            .cloned()
            .map(ProviderMeta::from)
            .collect();

        let file = ProviderFile {
            version: 1,
            providers: metas,
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create providers dir: {:?}", parent))?;
        }

        let json = serde_json::to_string_pretty(&file)?;
        fs::write(&path, &json)
            .with_context(|| format!("failed to write providers file: {:?}", path))?;

        drop(inner);
        let mut inner = self.inner.write().expect("registry lock poisoned");
        inner.dirty = false;

        debug!(path = ?path, "provider registry saved");
        Ok(())
    }

    /// Reload provider registry from disk.
    pub fn reload(&self) -> Result<()> {
        let path = self.registry_path();
        if !path.exists() {
            debug!("provider registry file not found, starting empty");
            return Ok(());
        }

        let json = fs::read_to_string(&path)
            .with_context(|| format!("failed to read providers file: {:?}", path))?;
        let file: ProviderFile = serde_json::from_str(&json)
            .with_context(|| format!("failed to parse providers file: {:?}", path,))?;

        let mut inner = self.inner.write().expect("registry lock poisoned");
        inner.providers.clear();
        for meta in file.providers {
            match <ProviderMeta as TryInto<ProviderEntry>>::try_into(meta) {
                Ok(entry) => {
                    inner.providers.insert(entry.provider_id.clone(), entry);
                }
                Err(e) => {
                    warn!(error = %e, "skipping invalid provider entry");
                }
            }
        }
        inner.dirty = false;
        info!(count = inner.providers.len(), "provider registry reloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_entry(id: &str, method: AuthMethod, status: ProviderStatus) -> ProviderEntry {
        ProviderEntry {
            provider_id: id.to_string(),
            name: format!("Provider {}", id),
            auth_method: method,
            api_base_url: format!("https://api.{}.com", id),
            models_endpoint: format!("https://api.{}.com/v1/models", id),
            credential_ref: format!("{}.enc", id),
            status,
            last_validated: String::new(),
            supported_tools: vec![],
        }
    }

    #[test]
    fn test_register_and_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let reg = ProviderRegistry::new(dir.path().to_str().unwrap());
        assert!(reg.is_empty());

        let entry = test_entry("openai", AuthMethod::ApiKey, ProviderStatus::Active);
        assert!(reg.register(entry).unwrap());

        assert_eq!(reg.len(), 1);
        let list = reg.list();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_get_and_unregister() {
        let dir = tempfile::TempDir::new().unwrap();
        let reg = ProviderRegistry::new(dir.path().to_str().unwrap());
        reg.register(test_entry(
            "test-provider",
            AuthMethod::OAuth2,
            ProviderStatus::Active,
        ))
        .unwrap();

        let entry = reg.get("test-provider");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "Provider test-provider");

        assert!(reg.unregister("test-provider"));
        assert!(reg.get("test-provider").is_none());
    }

    #[test]
    fn test_status_filtering() {
        let dir = tempfile::TempDir::new().unwrap();
        let reg = ProviderRegistry::new(dir.path().to_str().unwrap());
        reg.register(test_entry(
            "active1",
            AuthMethod::ApiKey,
            ProviderStatus::Active,
        ))
        .unwrap();
        reg.register(test_entry(
            "active2",
            AuthMethod::ApiKey,
            ProviderStatus::Active,
        ))
        .unwrap();
        reg.register(test_entry(
            "expired1",
            AuthMethod::OAuth2,
            ProviderStatus::Expired,
        ))
        .unwrap();
        reg.register(test_entry(
            "unconfig1",
            AuthMethod::Custom,
            ProviderStatus::Unconfigured,
        ))
        .unwrap();

        let active = reg.list_by_status(ProviderStatus::Active);
        assert_eq!(active.len(), 2);

        let expired = reg.list_by_status(ProviderStatus::Expired);
        assert_eq!(expired.len(), 1);

        let counts = reg.status_counts();
        assert_eq!(counts.get("Active"), Some(&2));
        assert_eq!(counts.get("Expired"), Some(&1));
    }

    #[test]
    fn test_update_status() {
        let dir = tempfile::TempDir::new().unwrap();
        let reg = ProviderRegistry::new(dir.path().to_str().unwrap());
        reg.register(test_entry(
            "provider",
            AuthMethod::ApiKey,
            ProviderStatus::Active,
        ))
        .unwrap();

        assert!(reg.is_active("provider"));
        reg.update_status("provider", ProviderStatus::Expired)
            .unwrap();
        assert!(!reg.is_active("provider"));
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();

        // Create registry, add providers, save
        let reg1 = ProviderRegistry::new(path);
        reg1.register(test_entry("p1", AuthMethod::ApiKey, ProviderStatus::Active))
            .unwrap();
        reg1.register(test_entry(
            "p2",
            AuthMethod::OAuth2,
            ProviderStatus::Expired,
        ))
        .unwrap();
        reg1.save().unwrap();

        // Load from disk
        let reg2 = ProviderRegistry::load(path).unwrap();
        assert_eq!(reg2.len(), 2);
        assert!(reg2.is_active("p1"));
        assert!(!reg2.is_active("p2"));
    }
}
