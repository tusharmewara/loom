//! Scoped credential grants — integration tests.
//!
//! Covers the security properties a grant must have: tool/run scope,
//! expiry at use time, immediate revocation recorded in the existing
//! hash-chained audit log, invalidation on credential/password rotation,
//! and that the stored secret never appears in a grant.

use loom_auth::AuthManager;
use loom_core::{AclRule, AuditEntry};

fn manager_with_credential(dir: &str, provider: &str, secret: &str) -> AuthManager {
    let mut manager = AuthManager::new(dir);
    manager.unlock_vault("master-password").unwrap();
    manager
        .vault()
        .set_credential(provider, secret)
        .unwrap();
    manager
}

fn audit_entries(manager: &AuthManager) -> Vec<AuditEntry> {
    manager.audit().list().unwrap()
}

#[test]
fn test_grant_works_for_its_tool_and_run() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();
    assert_eq!(issued.provider_id, "openai");
    assert_eq!(issued.run_id, "run-1");
    assert_eq!(issued.tool, "claude-code");

    let record = manager
        .verify_grant(&issued.token, "claude-code", "run-1")
        .unwrap();
    assert_eq!(record.grant_id, issued.grant_id);
}

#[test]
fn test_grant_fails_for_another_tool() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    let err = manager
        .verify_grant(&issued.token, "other-tool", "run-1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("scoped to tool"), "unexpected error: {err}");
}

#[test]
fn test_grant_fails_for_another_run() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    let err = manager
        .verify_grant(&issued.token, "claude-code", "run-2")
        .unwrap_err()
        .to_string();
    assert!(err.contains("scoped to run"), "unexpected error: {err}");
}

#[test]
fn test_grant_fails_after_expiry() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    // One-second lifetime, so expiry is enforced on the use path.
    let issued = manager.issue_grant("openai", "run-1", "claude-code", 1).unwrap();
    assert!(manager.verify_grant(&issued.token, "claude-code", "run-1").is_ok());

    std::thread::sleep(std::time::Duration::from_millis(1_200));

    let err = manager
        .verify_grant(&issued.token, "claude-code", "run-1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("expired"), "unexpected error: {err}");
}

#[test]
fn test_grant_fails_after_revocation() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();
    assert!(manager.verify_grant(&issued.token, "claude-code", "run-1").is_ok());

    assert!(manager.revoke_grant(&issued.grant_id).unwrap());

    let err = manager
        .verify_grant(&issued.token, "claude-code", "run-1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("revoked"), "unexpected error: {err}");
}

#[test]
fn test_revocation_is_recorded_in_the_audit_chain() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();
    manager.revoke_grant(&issued.grant_id).unwrap();

    let entries = audit_entries(&manager);
    let revoke = entries
        .iter()
        .find(|e| e.access_type == "grant.revoke")
        .expect("revocation not present in the audit log");
    assert!(revoke.result.contains(&issued.grant_id));

    // Same channel, same chain: the chain still verifies.
    let report = manager.audit().verify_integrity().unwrap();
    assert!(report.intact, "audit chain broken: {report:?}");
    assert!(report.chained >= 2);
}

#[test]
fn test_revocation_takes_effect_in_another_manager_instance() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let issuer = manager_with_credential(path, "openai", "sk-secret-value");
    let issued = issuer
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    // A second process/manager over the same directory.
    let mut broker = AuthManager::new(path);
    broker.unlock_vault("master-password").unwrap();
    assert!(broker
        .verify_grant(&issued.token, "claude-code", "run-1")
        .is_ok());

    issuer.revoke_grant(&issued.grant_id).unwrap();

    let err = broker
        .verify_grant(&issued.token, "claude-code", "run-1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("revoked"), "unexpected error: {err}");
}

#[test]
fn test_rotated_master_password_invalidates_grants() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let mut manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();
    assert!(manager.verify_grant(&issued.token, "claude-code", "run-1").is_ok());

    manager
        .vault_mut()
        .change_password("master-password", "new-master-password")
        .unwrap();

    assert!(
        manager.verify_grant(&issued.token, "claude-code", "run-1").is_err(),
        "grant survived a master password rotation"
    );
}

#[test]
fn test_rotated_credential_invalidates_grants() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();
    assert!(manager.verify_grant(&issued.token, "claude-code", "run-1").is_ok());

    // Rotation: the same provider now holds a different secret.
    manager
        .vault()
        .set_credential("openai", "sk-rotated-value")
        .unwrap();

    assert!(
        manager.verify_grant(&issued.token, "claude-code", "run-1").is_err(),
        "grant survived a credential rotation"
    );
}

#[test]
fn test_stored_secret_never_appears_in_a_grant() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let secret = "sk-super-secret-do-not-leak";
    let manager = manager_with_credential(path, "openai", secret);

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    assert!(!issued.token.contains(secret));
    assert!(!issued.token.contains("super-secret"));

    // The registry on disk must not carry the secret either.
    let registry = std::fs::read_to_string(dir.path().join("auth").join("grants.json")).unwrap();
    assert!(!registry.contains(secret));
    assert!(!registry.contains(&issued.token));
}

#[test]
fn test_acl_denial_blocks_grant_issuance() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    manager.acl().add_rule(AclRule {
        tool: "openai".to_string(),
        allow: vec![],
        deny: vec!["untrusted-plugin".to_string()],
    });

    let err = manager
        .issue_grant("openai", "run-1", "untrusted-plugin", 900)
        .unwrap_err()
        .to_string();
    assert!(err.contains("access denied"), "unexpected error: {err}");

    // The denial is in the existing audit chain.
    let entries = audit_entries(&manager);
    assert!(entries
        .iter()
        .any(|e| e.access_type == "denied" && e.result.contains("grant issuance")));
}

#[test]
fn test_redeem_returns_credential_and_is_audited() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    let secret = manager
        .redeem_grant(&issued.token, "claude-code", "run-1")
        .unwrap();
    assert_eq!(secret, "sk-secret-value");

    // A grant for another run cannot be redeemed.
    assert!(manager
        .redeem_grant(&issued.token, "claude-code", "run-2")
        .is_err());

    let entries = audit_entries(&manager);
    assert!(entries.iter().any(|e| e.access_type == "grant.redeem"));
}

#[test]
fn test_acl_denial_after_issuance_blocks_redemption() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    // A deny rule appears after the grant was issued.
    manager.acl().add_rule(AclRule {
        tool: "openai".to_string(),
        allow: vec![],
        deny: vec!["claude-code".to_string()],
    });

    let err = manager
        .redeem_grant(&issued.token, "claude-code", "run-1")
        .unwrap_err()
        .to_string();
    assert!(err.contains("access denied"), "unexpected error: {err}");
}

#[test]
fn test_locked_vault_blocks_grants() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");
    let issued = manager
        .issue_grant("openai", "run-1", "claude-code", 900)
        .unwrap();

    let locked = AuthManager::new(path);
    assert!(locked.vault_is_locked());
    assert!(locked.issue_grant("openai", "run-1", "claude-code", 900).is_err());
    assert!(locked.verify_grant(&issued.token, "claude-code", "run-1").is_err());
}

#[test]
fn test_grant_for_unknown_provider_fails() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().to_str().unwrap();
    let manager = manager_with_credential(path, "openai", "sk-secret-value");

    let err = manager
        .issue_grant("nope", "run-1", "claude-code", 900)
        .unwrap_err()
        .to_string();
    assert!(err.contains("no credential found"), "unexpected error: {err}");
}
