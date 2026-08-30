pub mod vault;
pub mod acl;
pub mod audit;
pub mod providers;
pub mod import;

use anyhow::Result;
use loom_core::{AuthProviderTrait, EncryptedCredential, HealthCheck, HealthStatus, ProviderEntry};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

/// Orchestrator for authentication, authorization, and audit.
///
/// Owns and coordinates:
/// - `CredentialVault` — encrypted credential storage
/// - `AclEngine` — per-tool access control lists
/// - `AuditLogger` — structured audit logging with rotation
/// - `ProviderRegistry` — provider lifecycle and status tracking
/// - `AutoImporter` — automatic import from tool configs
pub struct AuthManager {
    vault: vault::CredentialVault,
    acl: acl::AclEngine,
    audit: audit::AuditLogger,
    providers: providers::ProviderRegistry,
    importer: import::AutoImporter,
}

impl AuthManager {
    /// Create a new `AuthManager` for the given agents directory.
    ///
    /// All storage is relative to `loom_dir/auth/`.
    pub fn new(loom_dir: &str) -> Self {
        // Ensure auth directory exists
        let auth_dir = Path::new(loom_dir).join("auth");
        if let Err(e) = std::fs::create_dir_all(&auth_dir) {
            warn!(path = ?auth_dir, error = %e, "failed to create auth dir");
        }

        let vault = vault::CredentialVault::new(loom_dir);
        let acl = acl::AclEngine::new();
        let audit = audit::AuditLogger::new(loom_dir);
        let providers = providers::ProviderRegistry::new(loom_dir);
        let importer = import::AutoImporter::new(loom_dir);

        Self {
            vault,
            acl,
            audit,
            providers,
            importer,
        }
    }

    // ─── Vault Operations ──────────────────────────────────────────

    /// Reference to the credential vault.
    pub fn vault(&self) -> &vault::CredentialVault {
        &self.vault
    }

    /// Mutable reference to the credential vault (for unlock/lock/change_password).
    pub fn vault_mut(&mut self) -> &mut vault::CredentialVault {
        &mut self.vault
    }

    /// Unlock the vault with a master password.
    pub fn unlock_vault(&mut self, password: &str) -> Result<()> {
        self.vault.unlock(password)
    }

    /// Lock the vault.
    pub fn lock_vault(&mut self) {
        self.vault.lock();
    }

    /// Check if the vault is unlocked.
    pub fn vault_is_locked(&self) -> bool {
        self.vault.is_locked()
    }

    // ─── ACL Operations ────────────────────────────────────────────

    /// Reference to the ACL engine.
    pub fn acl(&self) -> &acl::AclEngine {
        &self.acl
    }

    /// Check if a requesting tool has access to a target tool/provider.
    ///
    /// Delegates to `AclEngine::check_access` and logs the result via `AuditLogger`.
    pub fn check_access(&self, tool: &str, requesting_tool: &str) -> bool {
        let allowed = self.acl.check_access(tool, requesting_tool);
        if allowed {
            if let Err(e) = self.audit.log_read(requesting_tool, tool, true) {
                warn!(error = %e, "failed to audit access grant");
            }
        } else {
            self.audit
                .log_denied_access(requesting_tool, tool, "acl denied");
        }
        allowed
    }

    // ─── Audit Operations ──────────────────────────────────────────

    /// Reference to the audit logger.
    pub fn audit(&self) -> &audit::AuditLogger {
        &self.audit
    }

    // ─── Provider Operations ───────────────────────────────────────

    /// Reference to the provider registry.
    pub fn providers(&self) -> &providers::ProviderRegistry {
        &self.providers
    }

    /// Register a provider entry.
    pub fn register_provider(&self, entry: ProviderEntry) -> Result<bool> {
        let is_new = self.providers.register(entry)?;
        if let Err(e) = self.providers.save() {
            warn!(error = %e, "failed to persist provider registry");
        }
        Ok(is_new)
    }

    /// Unregister a provider.
    pub fn unregister_provider(&self, provider_id: &str) -> bool {
        let removed = self.providers.unregister(provider_id);
        if removed {
            if let Err(e) = self.providers.save() {
                warn!(error = %e, "failed to persist provider registry after unregister");
            }
        }
        removed
    }

    // ─── Import Operations ─────────────────────────────────────────

    /// Run auto-import from tool config TOML files.
    ///
    /// Scans `{loom_dir}/tools/*.toml` and imports any discovered auth providers.
    /// If the vault is unlocked, credentials will be imported into the vault.
    pub fn import_from_tools(&mut self) -> Result<Vec<ProviderEntry>> {
        let vault = if !self.vault.is_locked() {
            Some(&self.vault)
        } else {
            None
        };
        self.importer.import_all(&self.providers, vault)
    }

    /// Import from a specific auth_store path.
    pub fn import_from_auth_store(
        &mut self,
        auth_store_path: &str,
    ) -> Result<Vec<ProviderEntry>> {
        let vault = if !self.vault.is_locked() {
            Some(&self.vault)
        } else {
            None
        };
        self.importer
            .import_from_auth_store(auth_store_path, &self.providers, vault)
    }

    // ─── Convenience ───────────────────────────────────────────────

    /// List all registered providers.
    pub fn list_providers(&self) -> Vec<ProviderEntry> {
        self.providers.list()
    }

    /// List all stored credentials (provider IDs).
    pub fn list_credentials(&self) -> Result<Vec<String>> {
        self.vault.list_credentials()
    }

    /// Wrap in Arc for sharing.
    pub fn as_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl HealthCheck for AuthManager {
    fn health(&self) -> Result<HealthStatus> {
        let mut counters = std::collections::HashMap::new();
        counters.insert("vault_locked".to_string(), if self.vault.is_locked() { 1 } else { 0 });
        counters.insert("acl_rules".to_string(), self.acl.len() as u64);
        counters.insert("providers_registered".to_string(), self.providers.len() as u64);
        counters.insert("audit_entries".to_string(), self.audit.list().unwrap_or_default().len() as u64);

        Ok(HealthStatus {
            crate_name: "loom-auth",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: counters,
        })
    }
}

impl AuthProviderTrait for AuthManager {
    fn discover_providers(&self) -> Result<Vec<ProviderEntry>> {
        Ok(self.providers.list())
    }

    fn export_credential(&self, provider_id: &str) -> Result<EncryptedCredential> {
        if self.vault.is_locked() {
            anyhow::bail!("vault is locked");
        }
        let plaintext = self
            .vault
            .get_credential(provider_id)?
            .ok_or_else(|| anyhow::anyhow!("no credential found for provider: {provider_id}"))?;

        // Re-encrypt in memory with a fresh random nonce for export.
        // No temporary file is written to disk, so plaintext never touches storage.
        let (encrypted_data, nonce) = self.vault.encrypt_for_export(&plaintext)?;

        Ok(EncryptedCredential {
            provider_id: provider_id.to_string(),
            encrypted_data,
            nonce,
            created_at: iso_now(),
        })
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
    use loom_core::{AuthMethod, ProviderStatus};

    fn test_provider(id: &str) -> ProviderEntry {
        ProviderEntry {
            provider_id: id.to_string(),
            name: format!("Provider {id}"),
            auth_method: AuthMethod::ApiKey,
            api_base_url: format!("https://api.{id}.com"),
            models_endpoint: format!("https://api.{id}.com/v1/models"),
            credential_ref: format!("{id}.enc"),
            status: ProviderStatus::Active,
            last_validated: String::new(),
            supported_tools: vec![],
        }
    }

    #[test]
    fn test_auth_manager_creation() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());
        assert!(manager.vault_is_locked());
        assert!(manager.providers().is_empty());
        assert!(manager.acl().is_empty());
    }

    #[test]
    fn test_vault_lifecycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());

        assert!(manager.vault_is_locked());
        manager.unlock_vault("test-password").unwrap();
        assert!(!manager.vault_is_locked());

        manager.vault().set_credential("openai", "sk-abc").unwrap();
        assert_eq!(
            manager.vault().get_credential("openai").unwrap(),
            Some("sk-abc".to_string())
        );

        manager.lock_vault();
        assert!(manager.vault_is_locked());
    }

    #[test]
    fn test_acl_check_access_logging() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());

        // Default: allow all
        assert!(manager.check_access("any-tool", "any-requester"));

        // Add a deny rule
        use loom_core::AclRule;
        manager
            .acl()
            .add_rule(AclRule {
                tool: "restricted".to_string(),
                allow: vec![],
                deny: vec!["bad-actor".to_string()],
            });

        assert!(!manager.check_access("restricted", "bad-actor"));
        assert!(manager.check_access("restricted", "good-actor"));

        // Check audit log entries
        let entries = manager.audit().list().unwrap();
        assert!(entries.len() >= 2);
    }

    #[test]
    fn test_provider_registration() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());

        let entry = test_provider("openai");
        manager.register_provider(entry).unwrap();

        assert_eq!(manager.list_providers().len(), 1);
        assert_eq!(manager.list_credentials().unwrap().len(), 0);
    }

    #[test]
    fn test_import_from_tools() {
        let dir = tempfile::TempDir::new().unwrap();
        let tools_dir = dir.path().join("tools");
        std::fs::create_dir_all(&tools_dir).unwrap();

        std::fs::write(
            tools_dir.join("my-tool.toml"),
            r#"
[auth]
provider_id = "imported-provider"
name = "Imported"
credential = "sk-imported"
"#,
        )
        .unwrap();

        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master").unwrap();

        manager.import_from_tools().unwrap();

        let providers = manager.list_providers();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider_id, "imported-provider");

        // Credential should be in vault
        let cred = manager.vault().get_credential("imported-provider").unwrap();
        assert_eq!(cred, Some("sk-imported".to_string()));
    }

    #[test]
    fn test_health_check() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());
        let health = manager.health().unwrap();
        assert_eq!(health.crate_name, "loom-auth");
        assert!(health.performance_counters.contains_key("vault_locked"));
        assert!(health.performance_counters.contains_key("acl_rules"));
    }

    #[test]
    fn test_export_credential_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager
            .vault()
            .set_credential("openai", "sk-secret-key")
            .unwrap();

        let exported = manager.export_credential("openai").unwrap();
        assert_eq!(exported.provider_id, "openai");
        assert!(!exported.encrypted_data.is_empty());
        assert!(!exported.nonce.is_empty());
        assert!(!exported.created_at.is_empty());

        // Data must not be plaintext and no temp export file should remain
        assert!(!exported.encrypted_data.contains("sk-secret-key"));
        let export_path = dir.path().join("auth").join("openai-export.enc");
        assert!(!export_path.exists());

        // No leftover temporary credential files in the vault dir
        let remaining = manager.vault().list_credentials().unwrap();
        assert_eq!(remaining, vec!["openai"]);
    }

    #[test]
    fn test_export_credential_missing_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();

        let result = manager.export_credential("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no credential found"));
    }

    #[test]
    fn test_export_credential_locked_vault() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());
        // Vault starts locked
        let result = manager.export_credential("openai");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("vault is locked"));
    }
}
