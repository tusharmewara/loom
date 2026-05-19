use loom_auth::acl::AclEngine;
use loom_auth::audit::AuditLogger;
use loom_auth::import::AutoImporter;
use loom_auth::providers::ProviderRegistry;
use loom_auth::vault::CredentialVault;
use loom_auth::AuthManager;
use loom_core::{AclRule, AuthMethod, HealthCheck, ProviderEntry, ProviderStatus};
use std::io::Write;

// ─── Vault Integration Test ─────────────────────────────────────

#[test]
fn test_vault_full_lifecycle() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut vault = CredentialVault::new(dir.path().to_str().unwrap());

    // Starts locked
    assert!(vault.is_locked());

    // Unlock with password
    vault.unlock("my-strong-password").unwrap();
    assert!(!vault.is_locked());

    // Store multiple credentials
    vault
        .set_credential("openai", "sk-openai-key-12345")
        .unwrap();
    vault
        .set_credential("anthropic", "sk-ant-key-67890")
        .unwrap();
    vault.set_credential("groq", "gsk-groq-token").unwrap();

    // Retrieve them
    assert_eq!(
        vault.get_credential("openai").unwrap(),
        Some("sk-openai-key-12345".to_string())
    );
    assert_eq!(
        vault.get_credential("anthropic").unwrap(),
        Some("sk-ant-key-67890".to_string())
    );
    assert_eq!(
        vault.get_credential("groq").unwrap(),
        Some("gsk-groq-token".to_string())
    );

    // List providers
    let list = vault.list_credentials().unwrap();
    assert_eq!(list.len(), 3);
    assert!(list.contains(&"openai".to_string()));
    assert!(list.contains(&"anthropic".to_string()));
    assert!(list.contains(&"groq".to_string()));

    // Delete one
    assert!(vault.delete_credential("groq").unwrap());
    assert_eq!(vault.list_credentials().unwrap().len(), 2);

    // Lock
    vault.lock();
    assert!(vault.is_locked());

    // Can't read when locked
    assert!(vault.get_credential("openai").is_err());

    // Re-unlock
    vault.unlock("my-strong-password").unwrap();
    assert_eq!(
        vault.get_credential("openai").unwrap(),
        Some("sk-openai-key-12345".to_string())
    );
}

#[test]
fn test_vault_change_password() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut vault = CredentialVault::new(dir.path().to_str().unwrap());

    vault.unlock("old-password").unwrap();
    vault
        .set_credential("my-provider", "super-secret-value")
        .unwrap();

    // Change password — re-encrypts credential
    vault
        .change_password("old-password", "new-password")
        .unwrap();

    // Verify still accessible
    assert_eq!(
        vault.get_credential("my-provider").unwrap(),
        Some("super-secret-value".to_string())
    );

    // Lock and verify new password works
    vault.lock();
    vault.unlock("new-password").unwrap();
    assert_eq!(
        vault.get_credential("my-provider").unwrap(),
        Some("super-secret-value".to_string())
    );
}

#[test]
fn test_vault_nonexistent_credential() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
    vault.unlock("test").unwrap();
    assert_eq!(vault.get_credential("nonexistent").unwrap(), None);
}

#[test]
fn test_vault_delete_nonexistent() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
    vault.unlock("test").unwrap();
    assert!(!vault.delete_credential("nonexistent").unwrap());
}

// ─── ACL Integration Tests ────────────────────────────────────

#[test]
fn test_acl_persistence_and_reload() {
    let dir = tempfile::TempDir::new().unwrap();
    let acl_path = dir.path().join("acl.json");
    let acl_path_str = acl_path.to_str().unwrap();

    // Create and save ACL
    let acl1 = AclEngine::new();
    acl1.add_rule(AclRule {
        tool: "premium".to_string(),
        allow: vec!["vip-client".to_string(), "admin".to_string()],
        deny: vec![],
    });
    acl1.add_rule(AclRule {
        tool: "internal-api".to_string(),
        allow: vec![],
        deny: vec!["external-client".to_string()],
    });
    acl1.save_to_path(acl_path_str).unwrap();

    // Load into new engine
    let acl2 = AclEngine::load(acl_path_str).unwrap();
    assert_eq!(acl2.len(), 2);

    // Verify VIP access
    assert!(acl2.check_access("premium", "vip-client"));
    assert!(acl2.check_access("premium", "admin"));
    assert!(!acl2.check_access("premium", "free-user"));

    // Verify internal API access
    assert!(acl2.check_access("internal-api", "internal-service"));
    assert!(!acl2.check_access("internal-api", "external-client"));

    // Default allow for unconfigured tools
    assert!(acl2.check_access("unconfigured", "anyone"));
}

#[test]
fn test_acl_sighup_reload() {
    let dir = tempfile::TempDir::new().unwrap();
    let acl_path = dir.path().join("acl.json");
    let acl_path_str = acl_path.to_str().unwrap();

    // Save initial rules
    let acl_init = AclEngine::new();
    acl_init.add_rule(AclRule {
        tool: "v1".to_string(),
        allow: vec!["admin".to_string()],
        deny: vec![],
    });
    acl_init.save_to_path(acl_path_str).unwrap();

    // Load ACL engine with SIGHUP support
    let engine = AclEngine::load(acl_path_str).unwrap();
    assert_eq!(engine.len(), 1);

    // Modify rules externally
    let acl_new = AclEngine::new();
    acl_new.add_rule(AclRule {
        tool: "v2".to_string(),
        allow: vec![],
        deny: vec!["bad".to_string()],
    });
    acl_new.save_to_path(acl_path_str).unwrap();

    // Engine doesn't see changes until reload
    assert!(engine.get_rule("v2").is_none());

    // Simulate SIGHUP
    engine.request_reload();
    assert!(engine.reload_if_requested().unwrap());

    // Now sees new rules
    assert!(engine.get_rule("v2").is_some());
    assert_eq!(engine.len(), 1);
}

#[test]
fn test_acl_deny_overrides_allow() {
    let acl = AclEngine::new();
    acl.add_rule(AclRule {
        tool: "api".to_string(),
        allow: vec!["user1".to_string(), "user2".to_string()],
        deny: vec!["user1".to_string()], // user1 in both — deny should win
    });

    assert!(!acl.check_access("api", "user1"));
    assert!(acl.check_access("api", "user2"));
}

// ─── Audit Integration Tests ────────────────────────────────────

#[test]
fn test_audit_structured_read_write_denied() {
    let dir = tempfile::TempDir::new().unwrap();
    // Reset threshold to a large value to avoid rotation in this test
    std::env::set_var("LOOM_AUDIT_MAX_SIZE", "104857600"); // 100MB
    let logger = AuditLogger::new(dir.path().to_str().unwrap());

    logger.log_read("tool-a", "provider-1", true).unwrap();
    logger.log_write("tool-a", "provider-2", false).unwrap();
    logger.log_denied_access("tool-b", "provider-3", "rate limit exceeded");

    let entries = logger.list().unwrap();
    assert_eq!(entries.len(), 3);

    // Read
    assert_eq!(entries[0].access_type, "read");
    assert_eq!(entries[0].requesting_tool, "tool-a");
    assert_eq!(entries[0].provider_id, "provider-1");
    assert_eq!(entries[0].result, "granted");

    // Write (failed)
    assert_eq!(entries[1].access_type, "write");
    assert_eq!(entries[1].result, "denied");

    // Denied
    assert_eq!(entries[2].access_type, "denied");
    assert!(entries[2].result.contains("rate limit exceeded"));
}

#[test]
fn test_audit_rotation_and_list_all() {
    let dir = tempfile::TempDir::new().unwrap();
    // Set a small rotation threshold for testing (100 bytes)
    std::env::set_var("LOOM_AUDIT_MAX_SIZE", "100");
    let log_dir = dir.path().join("auth");
    std::fs::create_dir_all(&log_dir).unwrap();
    let log_path = log_dir.join("audit.log");

    // Seed a large log file (~12MB)
    let mut file = std::fs::File::create(&log_path).unwrap();
    // Create valid JSON audit entries instead of just "x" repeated
    let valid_entry = serde_json::json!({
        "timestamp": "2024-01-01T00:00:00Z",
        "requesting_tool": "test-tool",
        "provider_id": "test-provider",
        "access_type": "read",
        "result": "granted"
    });
    let line = valid_entry.to_string();
    // Write enough entries to trigger rotation but not too many
    for _ in 0..1000 {
        writeln!(file, "{}", line).unwrap();
    }
    file.flush().unwrap();
    drop(file);

    // Debug: check seeded file size
    let metadata = std::fs::metadata(&log_path).unwrap();
    eprintln!("Seeded file size: {} bytes", metadata.len());

    let logger = AuditLogger::new(dir.path().to_str().unwrap());

    // This write should trigger rotation
    logger.log_read("new-tool", "new-provider", true).unwrap();

    // The rotated file should exist
    eprintln!(
        "DEBUG: Rotated file exists: {}",
        log_dir.join("audit.log.1").exists()
    );
    eprintln!(
        "DEBUG: Current file exists: {}",
        log_dir.join("audit.log").exists()
    );
    assert!(log_dir.join("audit.log.1").exists());

    // list_all should see entries from both current and rotated
    let all = logger.list_all().unwrap();
    let current = logger.list().unwrap();

    // Current log should just have the new entry
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].requesting_tool, "new-tool");

    // All logs combined should include the seeded ones plus new one
    eprintln!("DEBUG: all.len() = {}", all.len());
    eprintln!("DEBUG: current.len() = {}", current.len());
    assert!(all.len() > 1);
}

// ─── Provider Registry Integration Tests ──────────────────────

#[test]
fn test_provider_registry_persistence() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();

    // Create and populate
    let reg1 = ProviderRegistry::new(path);
    reg1.register(ProviderEntry {
        provider_id: "p1".to_string(),
        name: "Provider One".to_string(),
        auth_method: AuthMethod::ApiKey,
        api_base_url: "https://api.p1.com".to_string(),
        models_endpoint: "https://api.p1.com/v1/models".to_string(),
        credential_ref: "p1.enc".to_string(),
        status: ProviderStatus::Active,
        last_validated: String::new(),
        supported_tools: vec!["chat".to_string()],
    })
    .unwrap();
    reg1.register(ProviderEntry {
        provider_id: "p2".to_string(),
        name: "Provider Two".to_string(),
        auth_method: AuthMethod::OAuth2,
        api_base_url: "https://api.p2.com".to_string(),
        models_endpoint: "https://api.p2.com/v1/models".to_string(),
        credential_ref: "p2.enc".to_string(),
        status: ProviderStatus::Expired,
        last_validated: String::new(),
        supported_tools: vec![],
    })
    .unwrap();
    reg1.save().unwrap();

    // Reload in new instance
    let reg2 = ProviderRegistry::load(path).unwrap();
    assert_eq!(reg2.len(), 2);

    let p1 = reg2.get("p1").unwrap();
    assert!(matches!(p1.status, ProviderStatus::Active));
    assert_eq!(p1.supported_tools, vec!["chat"]);

    let p2 = reg2.get("p2").unwrap();
    assert!(matches!(p2.status, ProviderStatus::Expired));
}

#[test]
fn test_provider_status_transitions() {
    let dir = tempfile::TempDir::new().unwrap();
    let reg = ProviderRegistry::new(dir.path().to_str().unwrap());

    reg.register(ProviderEntry {
        provider_id: "test".to_string(),
        name: "Test".to_string(),
        auth_method: AuthMethod::ApiKey,
        api_base_url: "https://test.com".to_string(),
        models_endpoint: "https://test.com/v1/models".to_string(),
        credential_ref: "test.enc".to_string(),
        status: ProviderStatus::Unconfigured,
        last_validated: String::new(),
        supported_tools: vec![],
    })
    .unwrap();

    assert!(!reg.is_active("test"));

    reg.update_status("test", ProviderStatus::Active).unwrap();
    assert!(reg.is_active("test"));

    reg.update_status("test", ProviderStatus::Expired).unwrap();
    assert!(!reg.is_active("test"));

    // Status counts
    let counts = reg.status_counts();
    assert_eq!(counts.get("Expired"), Some(&1));
}

// ─── Auto-Importer Integration Tests ──────────────────────────

#[test]
fn test_importer_scan_and_import_all() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();

    // Create tool configs
    let tools_dir = dir.path().join("tools");
    std::fs::create_dir_all(&tools_dir).unwrap();

    std::fs::write(
        tools_dir.join("tool-a.toml"),
        r#"
[auth]
provider_id = "provider-a"
name = "Provider A"
auth_method = "ApiKey"
credential = "key-a"
"#,
    )
    .unwrap();

    std::fs::write(
        tools_dir.join("tool-b.toml"),
        r#"
[provider]
provider_id = "provider-b"
name = "Provider B"
auth_method = "OAuth2"
api_base = "https://api.b.com"
credential = "key-b"
supported_tools = ["x", "y"]
"#,
    )
    .unwrap();

    // Run import
    let registry = ProviderRegistry::new(path);
    let importer = AutoImporter::new(path);
    let imported = importer.import_all(&registry, None).unwrap();

    assert_eq!(imported.len(), 2);

    // Both providers should be active (both have credentials)
    for entry in &imported {
        assert!(matches!(entry.status, ProviderStatus::Active));
    }

    // Verify persisted
    let registry2 = ProviderRegistry::load(path).unwrap();
    assert_eq!(registry2.len(), 2);
    assert!(registry2.get("provider-b").is_some());
}

#[test]
fn test_importer_with_vault() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();

    // Create a tool config
    let tools_dir = dir.path().join("tools");
    std::fs::create_dir_all(&tools_dir).unwrap();
    std::fs::write(
        tools_dir.join("tool.toml"),
        r#"
[auth]
provider_id = "vault-test"
credential = "sk-vault-secret"
"#,
    )
    .unwrap();

    // Create unlocked vault
    let mut vault = CredentialVault::new(path);
    vault.unlock("master-password").unwrap();

    let registry = ProviderRegistry::new(path);
    let importer = AutoImporter::new(path);
    importer.import_all(&registry, Some(&vault)).unwrap();

    // Credential should be in vault
    let cred = vault.get_credential("vault-test").unwrap();
    assert_eq!(cred, Some("sk-vault-secret".to_string()));
}

// ─── AuthManager Integration Tests ────────────────────────────

#[test]
fn test_auth_manager_full_workflow() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();

    // Create tool config for import
    let tools_dir = dir.path().join("tools");
    std::fs::create_dir_all(&tools_dir).unwrap();
    std::fs::write(
        tools_dir.join("my-ai-tool.toml"),
        r#"
[auth]
provider_id = "openai"
name = "OpenAI"
auth_method = "ApiKey"
credential = "sk-real-key-12345"
supported_tools = ["chat", "completion", "embedding"]
"#,
    )
    .unwrap();

    let mut manager = AuthManager::new(path);

    // 1. Vault starts locked
    assert!(manager.vault_is_locked());

    // 2. Unlock vault
    manager.unlock_vault("master-key").unwrap();
    assert!(!manager.vault_is_locked());

    // 3. Import providers from tools
    let imported = manager.import_from_tools().unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].provider_id, "openai");

    // 4. Credential is in vault
    assert_eq!(
        manager.vault().get_credential("openai").unwrap(),
        Some("sk-real-key-12345".to_string())
    );

    // 5. Register an additional provider
    manager
        .register_provider(ProviderEntry {
            provider_id: "custom-llm".to_string(),
            name: "Custom LLM".to_string(),
            auth_method: AuthMethod::Custom,
            api_base_url: "https://custom.local/api".to_string(),
            models_endpoint: "https://custom.local/api/models".to_string(),
            credential_ref: "custom-llm.enc".to_string(),
            status: ProviderStatus::Active,
            last_validated: String::new(),
            supported_tools: vec!["chat".to_string()],
        })
        .unwrap();

    // 6. Set up ACL rules
    manager.acl().add_rule(AclRule {
        tool: "openai".to_string(),
        allow: vec!["my-ai-tool".to_string(), "admin-tool".to_string()],
        deny: vec!["untrusted-plugin".to_string()],
    });

    // 7. Check access via auth manager
    assert!(manager.check_access("openai", "my-ai-tool"));
    assert!(manager.check_access("openai", "admin-tool"));
    assert!(!manager.check_access("openai", "untrusted-plugin"));

    // 8. Audit log should have entries
    let audit_entries = manager.audit().list().unwrap();
    assert!(!audit_entries.is_empty());

    // 9. Verify health check
    let health = manager.health().unwrap();
    assert_eq!(health.crate_name, "loom-auth");
    assert!(health.performance_counters["providers_registered"] >= 2);

    // 10. List all providers
    let providers = manager.list_providers();
    assert_eq!(providers.len(), 2);

    // 11. Lock vault
    manager.lock_vault();
    assert!(manager.vault_is_locked());

    // 12. Vault operations denied when locked
    assert!(manager.vault().set_credential("test", "x").is_err());
}
