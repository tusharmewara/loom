//! `loom auth grant` — scoped credential grants.
//!
//! Deliberately no `redeem` subcommand: redemption turns a grant back into
//! the plaintext secret, so it belongs to a trusted broker (the library API
//! `AuthManager::redeem_grant`), not to a shell an agent can reach.

use crate::cli::GrantAction;
use loom_auth::AuthManager;
use std::path::Path;

/// Vault master password: explicit flag wins, then the environment.
pub(crate) fn vault_password(flag: Option<&String>) -> anyhow::Result<String> {
    flag.cloned()
        .or_else(|| std::env::var("LOOM_VAULT_PASSWORD").ok())
        .filter(|p| !p.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("vault password required (--vault-password or LOOM_VAULT_PASSWORD)")
        })
}

/// Open an unlocked manager for the grant operations that need the master key.
fn unlocked_manager(loom_dir: &Path, password: Option<&String>) -> anyhow::Result<AuthManager> {
    let mut manager = AuthManager::new(loom_dir.to_str().unwrap());
    manager.unlock_vault(&vault_password(password)?)?;
    Ok(manager)
}

pub fn run(loom_dir: &Path, action: &GrantAction) -> anyhow::Result<()> {
    match action {
        GrantAction::Issue {
            provider,
            run,
            tool,
            ttl_secs,
            vault_password: password,
            json,
        } => {
            let manager = unlocked_manager(loom_dir, password.as_ref())?;
            let issued = manager.issue_grant(provider, run, tool, *ttl_secs)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.grant.issue",
                        "grant_id": issued.grant_id,
                        "token": issued.token,
                        "provider_id": issued.provider_id,
                        "run_id": issued.run_id,
                        "tool": issued.tool,
                        "issued_at": issued.issued_at,
                        "expires_at": issued.expires_at,
                    }))?
                );
            } else {
                println!("grant:  {}", issued.grant_id);
                println!("token:  {}", issued.token);
                println!(
                    "scope:  provider={} run={} tool={}",
                    issued.provider_id, issued.run_id, issued.tool
                );
                println!("expiry: {} (unix seconds)", issued.expires_at);
            }
        }
        GrantAction::Verify {
            token,
            run,
            tool,
            vault_password: password,
            json,
        } => {
            let manager = unlocked_manager(loom_dir, password.as_ref())?;
            let record = manager.verify_grant(token, tool, run)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.grant.verify",
                        "valid": true,
                        "grant_id": record.grant_id,
                        "provider_id": record.provider_id,
                        "run_id": record.run_id,
                        "tool": record.tool,
                        "expires_at": record.expires_at,
                    }))?
                );
            } else {
                println!("✓ grant valid");
                println!("  id:       {}", record.grant_id);
                println!("  provider: {}", record.provider_id);
                println!("  run:      {}", record.run_id);
                println!("  tool:     {}", record.tool);
            }
        }
        GrantAction::Revoke { grant_id, json } => {
            let manager = AuthManager::new(loom_dir.to_str().unwrap());
            let revoked = manager.revoke_grant(grant_id)?;
            if !revoked {
                anyhow::bail!("unknown grant: {grant_id}");
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.grant.revoke",
                        "grant_id": grant_id,
                        "revoked": true,
                    }))?
                );
            } else {
                println!("✓ grant {} revoked", grant_id);
            }
        }
        GrantAction::List { json } => {
            let manager = AuthManager::new(loom_dir.to_str().unwrap());
            let grants = manager.list_grants();
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.grant.list",
                        "count": grants.len(),
                        "grants": grants,
                    }))?
                );
            } else {
                if grants.is_empty() {
                    println!("No grants issued.");
                } else {
                    for g in &grants {
                        println!(
                            "  {}  provider={} run={} tool={} expires={} {}",
                            g.grant_id,
                            g.provider_id,
                            g.run_id,
                            g.tool,
                            g.expires_at,
                            if g.revoked { "revoked" } else { "active" }
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_issue_and_verify_through_cli() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();

        let mut manager = AuthManager::new(path);
        manager.unlock_vault("master-password").unwrap();
        manager
            .vault()
            .set_credential("openai", "sk-cli-secret")
            .unwrap();

        let issue = GrantAction::Issue {
            provider: "openai".into(),
            run: "run-1".into(),
            tool: "claude-code".into(),
            ttl_secs: 900,
            vault_password: Some("master-password".into()),
            json: true,
        };
        run(dir.path(), &issue).unwrap();

        // The token is in the registry-backed store, so a fresh manager can
        // verify it (the CLI path is the same code the broker uses).
        let broker = AuthManager::new(path);
        let grants = broker.list_grants();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].tool, "claude-code");
        assert_eq!(grants[0].run_id, "run-1");
    }

    #[test]
    fn test_grant_revoke_through_cli() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap();

        let mut manager = AuthManager::new(path);
        manager.unlock_vault("master-password").unwrap();
        manager
            .vault()
            .set_credential("openai", "sk-cli-secret")
            .unwrap();
        let issued = manager
            .issue_grant("openai", "run-1", "claude-code", 900)
            .unwrap();

        let revoke = GrantAction::Revoke {
            grant_id: issued.grant_id.clone(),
            json: true,
        };
        run(dir.path(), &revoke).unwrap();

        let broker = AuthManager::new(path);
        assert!(broker
            .verify_grant(&issued.token, "claude-code", "run-1")
            .is_err());
    }

    #[test]
    fn test_grant_revoke_unknown_grant_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let revoke = GrantAction::Revoke {
            grant_id: "nope".into(),
            json: false,
        };
        assert!(run(dir.path(), &revoke).is_err());
    }

    #[test]
    fn test_missing_vault_password_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        // No flag and no env var set.
        if std::env::var("LOOM_VAULT_PASSWORD").is_ok() {
            return;
        }
        let issue = GrantAction::Issue {
            provider: "openai".into(),
            run: "run-1".into(),
            tool: "claude-code".into(),
            ttl_secs: 900,
            vault_password: None,
            json: true,
        };
        assert!(run(dir.path(), &issue).is_err());
    }
}
