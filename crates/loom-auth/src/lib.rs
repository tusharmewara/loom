pub mod acl;
pub mod audit;
pub mod export;
pub mod grant;
pub mod import;
pub mod providers;
pub mod role_store;
pub mod vault;

pub use audit::AuditLogger;
pub use export::{
    build_export, load_or_create_signing_key, parse_public_key, public_key_hex, sign_export,
    verify_export, AuditExport, ExportChecks, ExportVerdict, ExportVerification,
};

use anyhow::Result;
use loom_core::{
    AuthProviderTrait, AuditEntry, EncryptedCredential, HealthCheck, HealthStatus, ProviderEntry,
};
use std::path::Path;
use std::sync::Arc;
use tracing::warn;

use grant::{GrantRecord, IssuedGrant};

/// Orchestrator for authentication, authorization, and audit.
///
/// Owns and coordinates:
/// - `CredentialVault` — encrypted credential storage
/// - `AclEngine` — per-tool access control lists
/// - `AuditLogger` — structured audit logging with rotation
/// - `ProviderRegistry` — provider lifecycle and status tracking
/// - `AutoImporter` — automatic import from tool configs
/// - `GrantStore` — scoped credential grants (issue/verify/revoke)
pub struct AuthManager {
    vault: vault::CredentialVault,
    acl: acl::AclEngine,
    audit: audit::AuditLogger,
    providers: providers::ProviderRegistry,
    importer: import::AutoImporter,
    grants: grant::GrantStore,
    role_store: role_store::RoleStore,
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
        let grants = grant::GrantStore::new(loom_dir);
        let role_store = role_store::RoleStore::new(loom_dir);

        Self {
            vault,
            acl,
            audit,
            providers,
            importer,
            grants,
            role_store,
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

    /// Reference to the role store (admin/member RBAC).
    pub fn role_store(&self) -> &role_store::RoleStore {
        &self.role_store
    }

    /// `true` when `identity` resolves to `Admin`.
    pub fn is_admin(&self, identity: &str) -> bool {
        self.role_store.is_admin(identity)
    }

    /// `true` when `identity` resolves to `Member`.
    pub fn is_member(&self, identity: &str) -> bool {
        self.role_store.is_member(identity)
    }

    /// Require `identity` to be `Admin`; returns an error otherwise.
    pub fn require_admin(&self, identity: &str) -> Result<()> {
        if self.role_store.is_admin(identity) {
            Ok(())
        } else {
            anyhow::bail!(
                "admin required: identity '{}' is not an admin",
                identity
            );
        }
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
    pub fn import_from_auth_store(&mut self, auth_store_path: &str) -> Result<Vec<ProviderEntry>> {
        let vault = if !self.vault.is_locked() {
            Some(&self.vault)
        } else {
            None
        };
        self.importer
            .import_from_auth_store(auth_store_path, &self.providers, vault)
    }

    // ─── Grant Operations ──────────────────────────────────────────

    /// Issue a scoped grant for one credential, one run and one tool.
    ///
    /// The caller receives a derived token — never the stored secret. The
    /// ACL is checked for the same (provider, tool) pair that
    /// `export_credential` uses, and the issuance is recorded in the
    /// existing hash-chained audit log.
    pub fn issue_grant(
        &self,
        provider_id: &str,
        run_id: &str,
        tool: &str,
        ttl_secs: u64,
    ) -> Result<IssuedGrant> {
        if self.vault.is_locked() {
            anyhow::bail!("vault is locked");
        }

        // Same ACL gate as credential export — a grant never widens access.
        if !self.acl.check_access(provider_id, tool) {
            self.audit
                .log_denied_access(tool, provider_id, "grant issuance denied by ACL");
            anyhow::bail!(
                "access denied: tool '{}' not authorized for provider '{}'",
                tool,
                provider_id
            );
        }

        let ttl = if ttl_secs == 0 {
            grant::DEFAULT_GRANT_TTL_SECS
        } else {
            ttl_secs
        };
        let issued_at = grant::now_epoch();
        let record = GrantRecord {
            grant_id: grant::new_grant_id(),
            provider_id: provider_id.to_string(),
            run_id: run_id.to_string(),
            tool: tool.to_string(),
            issued_at,
            expires_at: issued_at.saturating_add(ttl),
            revoked: false,
            revoked_at: None,
        };

        let grant_key = self.vault.derive_grant_key(provider_id)?;
        let token = grant::mint_token(&grant_key, &record)?;

        self.grants.insert(record.clone());
        self.grants.save()?;
        self.audit_grant("grant.issue", tool, provider_id, &record, "granted")?;

        Ok(IssuedGrant {
            grant_id: record.grant_id,
            token,
            provider_id: record.provider_id,
            run_id: record.run_id,
            tool: record.tool,
            issued_at: record.issued_at,
            expires_at: record.expires_at,
        })
    }

    /// Verify a grant token for a use of `tool` in `run_id`.
    ///
    /// Checks, in order: the grant exists, it is not revoked, it has not
    /// expired (at use time), it is scoped to this tool and run, and the
    /// token authenticates under the current grant key. Returns the grant
    /// record on success.
    pub fn verify_grant(&self, token: &str, tool: &str, run_id: &str) -> Result<GrantRecord> {
        let parsed = grant::parse_token(token)?;
        self.grants.reload_if_changed();
        let record = self
            .grants
            .get(parsed.grant_id)
            .ok_or_else(|| anyhow::anyhow!("unknown grant: {}", parsed.grant_id))?;

        if record.revoked {
            self.audit_grant(
                "grant.use",
                tool,
                &record.provider_id,
                &record,
                "denied: revoked",
            )?;
            anyhow::bail!("grant has been revoked");
        }
        let now = grant::now_epoch();
        if record.is_expired(now) {
            self.audit_grant(
                "grant.use",
                tool,
                &record.provider_id,
                &record,
                "denied: expired",
            )?;
            anyhow::bail!("grant has expired");
        }
        if record.tool != tool {
            self.audit_grant(
                "grant.use",
                tool,
                &record.provider_id,
                &record,
                "denied: tool scope mismatch",
            )?;
            anyhow::bail!("grant is scoped to tool '{}', not '{}'", record.tool, tool);
        }
        if record.run_id != run_id {
            self.audit_grant(
                "grant.use",
                tool,
                &record.provider_id,
                &record,
                "denied: run scope mismatch",
            )?;
            anyhow::bail!(
                "grant is scoped to run '{}', not '{}'",
                record.run_id,
                run_id
            );
        }

        // The token must authenticate under the *current* grant key, so a
        // rotated master secret or a re-stored credential invalidates it.
        let grant_key = self.vault.derive_grant_key(&record.provider_id)?;
        grant::open_token(&grant_key, &record, token)?;

        self.audit_grant("grant.use", tool, &record.provider_id, &record, "granted")?;
        Ok(record)
    }

    /// Revoke a grant. Takes effect immediately.
    ///
    /// The revocation is recorded in the existing hash-chained audit log.
    pub fn revoke_grant(&self, grant_id: &str) -> Result<bool> {
        self.grants.reload_if_changed();
        let Some(record) = self.grants.get(grant_id) else {
            return Ok(false);
        };
        let now = grant::now_epoch();
        if !self.grants.revoke(grant_id, now) {
            return Ok(false);
        }
        self.grants.save()?;
        self.audit_grant(
            "grant.revoke",
            &record.tool,
            &record.provider_id,
            &record,
            "revoked",
        )?;
        Ok(true)
    }

    /// Redeem a grant for the stored credential.
    ///
    /// This is the broker side of the proxy: the agent holds a grant, a
    /// trusted broker calls this and gets the plaintext. Every redemption
    /// is audited.
    ///
    /// The ACL is re-checked here: a rule added after the grant was issued
    /// must still block the credential from reaching the tool.
    pub fn redeem_grant(&self, token: &str, tool: &str, run_id: &str) -> Result<String> {
        let record = self.verify_grant(token, tool, run_id)?;
        if !self.acl.check_access(&record.provider_id, tool) {
            self.audit.log_denied_access(
                tool,
                &record.provider_id,
                "grant redemption denied by ACL",
            );
            anyhow::bail!(
                "access denied: tool '{}' not authorized for provider '{}'",
                tool,
                record.provider_id
            );
        }
        let plaintext = self
            .vault
            .get_credential(&record.provider_id)?
            .ok_or_else(|| {
                anyhow::anyhow!("no credential found for provider: {}", record.provider_id)
            })?;
        self.audit_grant(
            "grant.redeem",
            tool,
            &record.provider_id,
            &record,
            "granted",
        )?;
        Ok(plaintext)
    }

    /// List all known grants.
    pub fn list_grants(&self) -> Vec<GrantRecord> {
        self.grants.reload_if_changed();
        self.grants.list()
    }

    /// List grants visible to `identity`.
    ///
    /// Admin → all grants.  Member → only grants scoped to `identity` as the
    /// requesting tool (a member sees their own grants, never another
    /// member's).
    pub fn list_grants_for(&self, identity: &str) -> Vec<GrantRecord> {
        self.grants.reload_if_changed();
        if self.role_store.is_admin(identity) {
            return self.grants.list();
        }
        self.grants
            .list()
            .into_iter()
            .filter(|record| record.tool == identity)
            .collect()
    }

    /// Export a credential with role-based access control.
    ///
    /// Admin → proceeds if the ACL allows.  Member → proceeds only when
    /// `requesting_tool == identity` and the ACL allows; otherwise denied.
    pub fn export_credential_for(
        &self,
        provider_id: &str,
        requesting_tool: &str,
        identity: &str,
    ) -> Result<EncryptedCredential> {
        if self.vault.is_locked() {
            anyhow::bail!("vault is locked");
        }

        // Role gate: members may only export their own credentials.
        if !self.role_store.is_admin(identity) && requesting_tool != identity {
            self.audit
                .log_denied_access(requesting_tool, provider_id, "export denied by role");
            anyhow::bail!(
                "access denied: member '{}' may not export credential for '{}'",
                identity,
                provider_id
            );
        }

        // ACL gate (same as export_credential).
        if !self.acl.check_access(provider_id, requesting_tool) {
            self.audit
                .log_denied_access(requesting_tool, provider_id, "export denied by ACL");
            anyhow::bail!(
                "access denied: tool '{}' not authorized to export credential for '{}'",
                requesting_tool,
                provider_id
            );
        }

        if let Err(e) = self.audit.log_read(requesting_tool, provider_id, true) {
            tracing::warn!(error = %e, "failed to audit credential export");
        }

        let plaintext = self
            .vault
            .get_credential(provider_id)?
            .ok_or_else(|| anyhow::anyhow!("no credential found for provider: {provider_id}"))?;

        let (encrypted_data, nonce) = self.vault.encrypt_for_export(&plaintext)?;

        Ok(EncryptedCredential {
            provider_id: provider_id.to_string(),
            encrypted_data,
            nonce,
            created_at: loom_core::rfc3339_now(),
        })
    }

    /// Redeem a grant with role-based access control.
    ///
    /// Admin → proceeds if `verify_grant` passes.  Member → proceeds only
    /// when `tool == identity` and `verify_grant` passes.
    pub fn redeem_grant_for(
        &self,
        token: &str,
        tool: &str,
        run_id: &str,
        identity: &str,
    ) -> Result<String> {
        let record = self.verify_grant(token, tool, run_id)?;

        // Role gate: members may only redeem their own grants.
        if !self.role_store.is_admin(identity) && tool != identity {
            self.audit.log_denied_access(
                tool,
                &record.provider_id,
                "grant redemption denied by role",
            );
            anyhow::bail!(
                "access denied: member '{}' may not redeem grant for tool '{}'",
                identity,
                tool
            );
        }

        // ACL re-check (same as redeem_grant).
        if !self.acl.check_access(&record.provider_id, tool) {
            self.audit.log_denied_access(
                tool,
                &record.provider_id,
                "grant redemption denied by ACL",
            );
            anyhow::bail!(
                "access denied: tool '{}' not authorized for provider '{}'",
                tool,
                record.provider_id
            );
        }

        let plaintext = self
            .vault
            .get_credential(&record.provider_id)?
            .ok_or_else(|| {
                anyhow::anyhow!("no credential found for provider: {}", record.provider_id)
            })?;
        self.audit_grant(
            "grant.redeem",
            tool,
            &record.provider_id,
            &record,
            "granted",
        )?;
        Ok(plaintext)
    }

    /// List audit entries visible to `identity`.
    ///
    /// Admin → all entries.  Member → only entries where
    /// `requesting_tool == identity` (their own actions).
    pub fn audit_entries_for(&self, identity: &str) -> Vec<AuditEntry> {
        let all = self.audit.list().unwrap_or_default();
        if self.role_store.is_admin(identity) {
            return all;
        }
        all.into_iter()
            .filter(|entry| entry.requesting_tool == identity)
            .collect()
    }

    /// Record a grant lifecycle event in the existing hash-chained audit log.
    fn audit_grant(
        &self,
        access_type: &str,
        tool: &str,
        provider_id: &str,
        record: &GrantRecord,
        result: &str,
    ) -> Result<()> {
        let entry = loom_core::AuditEntry {
            timestamp: loom_core::rfc3339_now(),
            requesting_tool: tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: access_type.to_string(),
            result: format!("{result} grant={} run={}", record.grant_id, record.run_id),
            prev_hash: None,
            entry_hash: None,
        };
        self.audit.log(&entry)
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
        counters.insert(
            "vault_locked".to_string(),
            if self.vault.is_locked() { 1 } else { 0 },
        );
        counters.insert("acl_rules".to_string(), self.acl.len() as u64);
        counters.insert(
            "providers_registered".to_string(),
            self.providers.len() as u64,
        );
        counters.insert(
            "audit_entries".to_string(),
            self.audit.list().unwrap_or_default().len() as u64,
        );

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

    fn export_credential(
        &self,
        provider_id: &str,
        requesting_tool: &str,
    ) -> Result<EncryptedCredential> {
        if self.vault.is_locked() {
            anyhow::bail!("vault is locked");
        }

        // Check ACL before exporting any credential
        if !self.acl.check_access(provider_id, requesting_tool) {
            self.audit
                .log_denied_access(requesting_tool, provider_id, "export denied by ACL");
            anyhow::bail!(
                "access denied: tool '{}' not authorized to export credential for '{}'",
                requesting_tool,
                provider_id
            );
        }

        // Log the export attempt
        if let Err(e) = self.audit.log_read(requesting_tool, provider_id, true) {
            tracing::warn!(error = %e, "failed to audit credential export");
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
            created_at: loom_core::rfc3339_now(),
        })
    }
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
        manager.acl().add_rule(AclRule {
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

        let exported = manager.export_credential("openai", "loom-cli").unwrap();
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

        let result = manager.export_credential("nonexistent", "test-tool");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no credential found"));
    }

    #[test]
    fn test_export_credential_locked_vault() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());
        // Vault starts locked
        let result = manager.export_credential("openai", "test-tool");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("vault is locked"));
    }

    // ─── Phase 4: vault broker v1 (RBAC roles) ─────────────────────

    #[test]
    fn test_role_admin_member_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let manager = AuthManager::new(dir.path().to_str().unwrap());

        // No roles.json, no org.toml → everyone is Member by default.
        assert!(manager.is_member("alice"));
        assert!(!manager.is_admin("alice"));
        assert!(manager.is_member("bob"));
        assert!(!manager.is_admin("bob"));

        // require_admin rejects non-admin.
        assert!(manager.require_admin("alice").is_err());
    }

    #[test]
    fn test_role_explicit_roles_json_wins() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("auth")).unwrap();
        std::fs::write(
            dir.path().join("auth").join("roles.json"),
            r#"{"roles": {"alice": "admin", "bob": "member"}}"#,
        )
        .unwrap();

        let manager = AuthManager::new(dir.path().to_str().unwrap());

        assert!(manager.is_admin("alice"));
        assert!(!manager.is_member("alice"));
        assert!(manager.is_member("bob"));
        assert!(!manager.is_admin("bob"));

        // require_admin passes for admin.
        assert!(manager.require_admin("alice").is_ok());
        assert!(manager.require_admin("bob").is_err());
    }

    #[test]
    fn test_role_org_toml_offline_interim() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config").join("org.toml"),
            r#"[org]
members = ["alice", "bob"]
admins = ["alice"]
"#,
        )
        .unwrap();

        let manager = AuthManager::new(dir.path().to_str().unwrap());

        assert!(manager.is_admin("alice"));
        assert!(manager.is_member("bob"));
        assert!(!manager.is_admin("bob"));
        assert!(manager.is_member("carol")); // default member
    }

    #[test]
    fn test_role_cp_membership_takes_precedence_over_org_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("org")).unwrap();

        // org.toml says carol is a plain member.
        std::fs::write(
            dir.path().join("config").join("org.toml"),
            r#"[org]
members = ["carol"]
admins = []
"#,
        )
        .unwrap();

        // CP membership says carol is an admin.
        std::fs::write(
            dir.path().join("org").join("membership.json"),
            r#"{"org": {"id": "org-1", "name": "Test", "tier": "free", "admins": ["carol"]}}"#,
        )
        .unwrap();

        let manager = AuthManager::new(dir.path().to_str().unwrap());

        assert!(manager.is_admin("carol"));
    }

    #[test]
    fn test_list_grants_for_member_scoped() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();

        // Set credentials so grants can be issued.
        manager.vault().set_credential("openai", "sk-a").unwrap();
        manager.vault().set_credential("anthropic", "sk-b").unwrap();

        // Two members each get a grant.
        manager
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();
        manager
            .issue_grant("anthropic", "run-2", "member-b", 900)
            .unwrap();

        // member-a sees only their own grant.
        let a_grants = manager.list_grants_for("member-a");
        assert_eq!(a_grants.len(), 1);
        assert_eq!(a_grants[0].tool, "member-a");
        assert_eq!(a_grants[0].provider_id, "openai");

        // member-b sees only their own grant.
        let b_grants = manager.list_grants_for("member-b");
        assert_eq!(b_grants.len(), 1);
        assert_eq!(b_grants[0].tool, "member-b");
        assert_eq!(b_grants[0].provider_id, "anthropic");

        // Admin (via roles.json) sees all — separate dir so we don't see
        // grants persisted by `manager` above.
        let dir2 = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir2.path().join("auth")).unwrap();
        std::fs::write(
            dir2.path().join("auth").join("roles.json"),
            r#"{"roles": {"admin-1": "admin"}}"#,
        )
        .unwrap();
        let mut manager2 = AuthManager::new(dir2.path().to_str().unwrap());
        manager2.unlock_vault("master-pass").unwrap();
        manager2.vault().set_credential("openai", "sk-a").unwrap();
        manager2.vault().set_credential("anthropic", "sk-b").unwrap();
        manager2
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();
        manager2
            .issue_grant("anthropic", "run-2", "member-b", 900)
            .unwrap();

        let all = manager2.list_grants_for("admin-1");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_export_credential_for_member_denied_other_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager.vault().set_credential("openai", "sk-secret").unwrap();

        // member-a tries to export openai credential — not their own tool.
        let result = manager.export_credential_for("openai", "loom-cli", "member-a");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("may not export credential"));
    }

    #[test]
    fn test_export_credential_for_admin_allowed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("auth")).unwrap();
        std::fs::write(
            dir.path().join("auth").join("roles.json"),
            r#"{"roles": {"admin-1": "admin"}}"#,
        )
        .unwrap();

        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager.vault().set_credential("openai", "sk-secret").unwrap();

        // Admin can export any credential (ACL allows by default).
        let exported = manager.export_credential_for("openai", "loom-cli", "admin-1");
        assert!(exported.is_ok());
        assert_eq!(exported.unwrap().provider_id, "openai");
    }

    #[test]
    fn test_redeem_grant_for_member_denied_other_tool() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager.vault().set_credential("openai", "sk-secret").unwrap();

        // member-a gets a grant scoped to their own tool.
        let issued = manager
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();

        // member-b tries to redeem member-a's grant — denied by role.
        let result = manager.redeem_grant_for(&issued.token, "member-a", "run-1", "member-b");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("may not redeem grant"));
    }

    #[test]
    fn test_redeem_grant_for_admin_allowed() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("auth")).unwrap();
        std::fs::write(
            dir.path().join("auth").join("roles.json"),
            r#"{"roles": {"admin-1": "admin"}}"#,
        )
        .unwrap();

        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager.vault().set_credential("openai", "sk-secret").unwrap();

        let issued = manager
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();

        // Admin can redeem any grant (ACL allows by default).
        let plaintext = manager
            .redeem_grant_for(&issued.token, "member-a", "run-1", "admin-1")
            .unwrap();
        assert_eq!(plaintext, "sk-secret");
    }

    #[test]
    fn test_audit_entries_for_member_scoped() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut manager = AuthManager::new(dir.path().to_str().unwrap());
        manager.unlock_vault("master-pass").unwrap();
        manager.vault().set_credential("openai", "sk-secret").unwrap();

        // member-a does a grant issue + redeem (audited with requesting_tool).
        let issued = manager
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();
        let _ = manager.redeem_grant(&issued.token, "member-a", "run-1").unwrap();

        // member-b does a grant issue (audited with member-b).
        manager.vault().set_credential("anthropic", "sk-b").unwrap();
        let _ = manager.issue_grant("anthropic", "run-2", "member-b", 900).unwrap();

        // member-a sees only their own entries.
        let a_entries = manager.audit_entries_for("member-a");
        assert!(!a_entries.is_empty());
        assert!(a_entries.iter().all(|e| e.requesting_tool == "member-a"));

        // member-b sees only their own entries.
        let b_entries = manager.audit_entries_for("member-b");
        assert!(!b_entries.is_empty());
        assert!(b_entries.iter().all(|e| e.requesting_tool == "member-b"));

        // Admin (via roles.json) sees all.
        std::fs::write(
            dir.path().join("auth").join("roles.json"),
            r#"{"roles": {"admin-1": "admin"}}"#,
        )
        .unwrap();
        let mut manager2 = AuthManager::new(dir.path().to_str().unwrap());
        manager2.unlock_vault("master-pass").unwrap();
        manager2.vault().set_credential("openai", "sk-secret").unwrap();
        manager2.vault().set_credential("anthropic", "sk-b").unwrap();
        manager2
            .issue_grant("openai", "run-1", "member-a", 900)
            .unwrap();
        manager2
            .issue_grant("anthropic", "run-2", "member-b", 900)
            .unwrap();

        let all = manager2.audit_entries_for("admin-1");
        assert!(all.len() >= 4); // 4 grant issue events
    }
}
