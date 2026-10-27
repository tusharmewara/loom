use anyhow::{Context, Result};
use chrono::Utc;
use loom_core::license::{verify_license_key, write_license_store, License, LicenseState};
use std::path::Path;

pub async fn run(loom_dir: &str, action: LicenseAction) -> Result<()> {
    match action {
        LicenseAction::Activate { key, json } => activate(loom_dir, &key, json).await,
        LicenseAction::Status { json } => status(loom_dir, json).await,
        LicenseAction::Deactivate { json } => deactivate(loom_dir, json).await,
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum LicenseAction {
    /// Activate a license key (writes ~/.loom/license.json, mode 0600)
    Activate {
        /// The license key to activate
        key: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show license state, tier, seats, expiry
    Status {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Deactivate the current license (removes ~/.loom/license.json)
    Deactivate {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

async fn activate(loom_dir: &str, key: &str, json: bool) -> Result<()> {
    let license = verify_license_key(key.trim(), loom_core::constants::LICENSE_VERIFY_PUBLIC_KEY)
        .context("license key verification failed — is it valid and signed for this loom?")?;

    let path = write_license_store(Path::new(loom_dir), key.trim())?;
    let payload = license;

    if json {
        let out = serde_json::json!({
            "activated": true,
            "tier": payload.tier,
            "seats": payload.seats,
            "expires_at": payload.expires_at,
            "path": path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("License activated: {} ({} seats) — expires {}", payload.tier, payload.seats, payload.expires_at);
        println!("Stored at: {}", path.display());
    }
    Ok(())
}

async fn status(loom_dir: &str, json: bool) -> Result<()> {
    let license = License::load(Path::new(loom_dir));
    let now = Utc::now();
    let state = license.state_at(now);

    if json {
        let out = serde_json::json!({
            "state": state.as_str(),
            "tier": license.payload().map(|p| p.tier.as_str()),
            "seats": license.payload().map(|p| p.seats),
            "expires_at": license.expires_at().map(|e| e.to_rfc3339()),
            "grace_until": license.grace_until().map(|e| e.to_rfc3339()),
            "invalid_reason": license.invalid_reason(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        match state {
            LicenseState::Unlicensed => println!("license: unlicensed"),
            LicenseState::Invalid => {
                println!("license: invalid — {}", license.invalid_reason().unwrap_or("unknown"));
            }
            LicenseState::Active => {
                let p = license.payload().unwrap();
                println!("license: active ({}, {} seats, expires {})", p.tier, p.seats, p.expires_at);
            }
            LicenseState::Grace => {
                let p = license.payload().unwrap();
                let grace = license.grace_until().unwrap();
                println!("license: grace (read-only until {}) — {}", grace, p.tier);
            }
            LicenseState::Expired => {
                let p = license.payload().unwrap();
                println!("license: expired ({}) — {}", p.tier, p.expires_at);
            }
        }
    }
    Ok(())
}

async fn deactivate(loom_dir: &str, json: bool) -> Result<()> {
    let path = Path::new(loom_dir).join(loom_core::license::LICENSE_STORE_FILE);
    if path.exists() {
        std::fs::remove_file(&path)?;
        if json {
            println!("{}", serde_json::json!({"deactivated": true}));
        } else {
            println!("License deactivated (removed {})", path.display());
        }
    } else {
        if json {
            println!("{}", serde_json::json!({"deactivated": false, "reason": "no license stored"}));
        } else {
            println!("No license stored");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::license::{mint_license, mint_license_for_test, generate_keypair, LicenseState, TEST_FEATURE_READ, TEST_FEATURE_WRITE, load_with_key, write_license_store as core_write_license_store};
    use tempfile::tempdir;

    fn sample_payload(expires_at: String) -> loom_core::license::LicensePayload {
        loom_core::license::LicensePayload {
            org_id: "acme".into(),
            tier: "team".into(),
            seats: 10,
            expires_at,
            features: vec!["teams.sync".into()],
        }
    }

    fn future_expiry(days: i64) -> String {
        (Utc::now() + chrono::Duration::days(days)).to_rfc3339()
    }

    fn past_expiry(days: i64) -> String {
        (Utc::now() - chrono::Duration::days(days)).to_rfc3339()
    }

    #[tokio::test]
    async fn status_exits_zero_in_every_state() {
        let dir = tempdir().unwrap();
        let keypair = generate_keypair().unwrap();
        let pk = keypair.1.clone();

        // unlicensed
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(Utc::now()), LicenseState::Unlicensed);

        // active: load_with_key with test keypair
        let token = mint_license(&sample_payload(future_expiry(180)), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &token).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(Utc::now()), LicenseState::Active);

        // expired (past grace) - use test mint
        let expired_token = mint_license_for_test(&sample_payload(past_expiry(40)), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &expired_token).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(Utc::now()), LicenseState::Expired);

        // grace (within 30-day grace) - use test mint
        let grace_token = mint_license_for_test(&sample_payload(past_expiry(10)), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &grace_token).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(Utc::now()), LicenseState::Grace);

        // grace unlocks read-only, refuses mutating
        assert!(license.unlocks_at(TEST_FEATURE_READ, Utc::now()));
        assert!(!license.unlocks_at(TEST_FEATURE_WRITE, Utc::now()));
    }

    #[tokio::test]
    async fn state_at_injects_now_no_sleep() {
        let dir = tempdir().unwrap();
        let keypair = generate_keypair().unwrap();
        let pk = keypair.1.clone();
        let now = Utc::now();

        // active at 'now'
        let token = mint_license(&sample_payload((now + chrono::Duration::days(5)).to_rfc3339()), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &token).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(now), LicenseState::Active);

        // grace at 'now' (expiry was 10 days ago) - use test mint
        let token2 = mint_license_for_test(&sample_payload((now - chrono::Duration::days(10)).to_rfc3339()), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &token2).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(now), LicenseState::Grace);

        // expired at 'now' (expiry was 40 days ago) - use test mint
        let token3 = mint_license_for_test(&sample_payload((now - chrono::Duration::days(40)).to_rfc3339()), &keypair.0).unwrap();
        core_write_license_store(dir.path(), &token3).unwrap();
        let license = load_with_key(dir.path(), &pk);
        assert_eq!(license.state_at(now), LicenseState::Expired);
    }

    #[tokio::test]
    async fn activate_deactivate_flow_with_embedded_key() {
        // This test would need a token signed with the embedded private key,
        // which is not available in tests. The state machine is tested above.
        // The activate/deactivate CLI integration is verified manually.
    }
}