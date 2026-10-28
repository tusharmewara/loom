//! `loom org` — connect to a control plane and inspect membership/license.

use anyhow::{Context, Result};
use chrono::Utc;
use loom_core::license::{License, LicenseState};
use loom_core::org::{read_membership, OrgClient};
use std::path::Path;

pub async fn run(loom_dir: &str, action: OrgAction) -> Result<()> {
    match action {
        OrgAction::Connect { url, org } => connect(loom_dir, &url, &org).await,
        OrgAction::Status => status(loom_dir).await,
    }
}

#[derive(Debug, clap::Subcommand)]
pub enum OrgAction {
    /// Enroll this machine with a control plane (challenge-response proof of possession)
    Connect {
        /// Control plane URL, e.g. http://127.0.0.1:7333
        url: String,
        /// Organization id to enroll into
        #[arg(long)]
        org: String,
    },
    /// Show org membership and license state
    Status,
}

async fn connect(loom_dir: &str, url: &str, org: &str) -> Result<()> {
    // Blocking ureq calls inside OrgClient::connect are short (one round trip).
    let client = OrgClient::new(url, org);
    let issued = tokio::task::spawn_blocking(move || client.connect())
        .await
        .context("enroll task panicked")??;

    let org_id = issued
        .get("org_id")
        .and_then(|v| v.as_str())
        .unwrap_or(org);
    let machine_pub = issued
        .get("machine_pub")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    println!("Connected to org {org_id} as machine {machine_pub}");
    println!("Bearer token and membership cached in {}/org/", loom_dir);
    Ok(())
}

async fn status(loom_dir: &str) -> Result<()> {
    let output = status_output(loom_dir, None).await?;
    println!("{}", output);
    Ok(())
}

/// Build the `loom org status` output as a String (testable without capturing stdout).
/// `public_key_hex` overrides the embedded verification key (used in tests; production passes None).
async fn status_output(loom_dir: &str, public_key_hex: Option<&str>) -> Result<String> {
    let dir = Path::new(loom_dir);

    // License state (phase-2 gate).
    let license = match public_key_hex {
        Some(pk) => License::load_with_key(dir, pk),
        None => License::load(dir),
    };
    let state = license.state_at(Utc::now());

    // Cached org membership.
    let membership = read_membership(dir).context(
        "no cached membership — run `loom org connect` first, or enrollment failed",
    )?;
    let membership_obj = membership
        .as_ref()
        .and_then(|v| v.as_object())
        .context("cached membership is not an object")?;
    let org = membership_obj
        .get("org")
        .and_then(|v| v.as_object())
        .context("cached membership has no org")?;
    let org_id = org
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let org_name = org
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let tier = org
        .get("tier")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let humans = membership_obj
        .get("humans")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len());
    let machines = membership_obj
        .get("machines")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len());

    let mut output = String::new();
    output.push_str(&format!("Organization: {org_name} ({org_id})\n"));
    output.push_str(&format!("Tier: {tier}\n"));
    output.push_str(&format!("Humans: {humans} | Machines: {machines}\n"));
    match state {
        LicenseState::Unlicensed => output.push_str("License: unlicensed\n"),
        LicenseState::Invalid => {
            output.push_str(&format!(
                "License: invalid — {}\n",
                license.invalid_reason().unwrap_or("unknown")
            ));
        }
        LicenseState::Active => {
            let p = license.payload().unwrap();
            output.push_str(&format!("License: active ({}, {} seats)\n", p.tier, p.seats));
        }
        LicenseState::Grace => {
            let p = license.payload().unwrap();
            output.push_str(&format!("License: grace (read-only) — {}\n", p.tier));
        }
        LicenseState::Expired => {
            let p = license.payload().unwrap();
            output.push_str(&format!("License: expired — {}\n", p.tier));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use loom_core::license::{generate_keypair, mint_license_for_test, write_license_store, LicensePayload};
    use loom_core::org::store_membership;
    use serde_json::json;
    use tempfile::tempdir;

    /// Write a license signed by `keypair.0`, return the verifying-key hex (for load_with_key).
    /// Uses `mint_license_for_test` to allow past expiries (grace/expired scenarios).
    fn write_test_license(dir: &Path, days_from_now: i64) -> Result<String> {
        let keypair = generate_keypair().unwrap();
        let pk = keypair.1.clone();
        let payload = LicensePayload {
            org_id: "test-org".into(),
            tier: "team".into(),
            seats: 5,
            expires_at: (Utc::now() + chrono::Duration::days(days_from_now)).to_rfc3339(),
            features: vec![],
        };
        let token = mint_license_for_test(&payload, &keypair.0).unwrap();
        write_license_store(dir, &token)?;
        Ok(pk)
    }

    fn write_test_membership(dir: &Path) -> Result<()> {
        let membership = json!({
            "org": {"id": "test-org", "name": "Test Org", "tier": "free"},
            "humans": [{"email": "alice@example.com", "role": "admin"}, {"email": "bob@example.com", "role": "member"}],
            "machines": ["machine-pub-1", "machine-pub-2"],
        });
        store_membership(dir, &membership)
    }

    #[tokio::test]
    async fn status_active_license() {
        let dir = tempdir().unwrap();
        let pk = write_test_license(dir.path(), 30).unwrap();
        write_test_membership(dir.path()).unwrap();
        let output = status_output(dir.path().to_str().unwrap(), Some(&pk)).await.unwrap();
        assert!(output.contains("Organization: Test Org (test-org)"));
        assert!(output.contains("Tier: free"));
        assert!(output.contains("Humans: 2 | Machines: 2"));
        assert!(output.contains("License: active (team, 5 seats)"));
    }

    #[tokio::test]
    async fn status_unlicensed() {
        let dir = tempdir().unwrap();
        write_test_membership(dir.path()).unwrap();
        let output = status_output(dir.path().to_str().unwrap(), None).await.unwrap();
        assert!(output.contains("License: unlicensed"));
    }

    #[tokio::test]
    async fn status_grace_within_30_days() {
        let dir = tempdir().unwrap();
        let pk = write_test_license(dir.path(), -10).unwrap();
        write_test_membership(dir.path()).unwrap();
        let output = status_output(dir.path().to_str().unwrap(), Some(&pk)).await.unwrap();
        assert!(output.contains("License: grace"));
    }

    #[tokio::test]
    async fn status_expired_beyond_30_days() {
        let dir = tempdir().unwrap();
        let pk = write_test_license(dir.path(), -40).unwrap();
        write_test_membership(dir.path()).unwrap();
        let output = status_output(dir.path().to_str().unwrap(), Some(&pk)).await.unwrap();
        assert!(output.contains("License: expired"));
    }

    #[tokio::test]
    async fn status_invalid_license() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("license.json"), "not.a.valid.token").unwrap();
        write_test_membership(dir.path()).unwrap();
        let output = status_output(dir.path().to_str().unwrap(), None).await.unwrap();
        assert!(output.contains("License: invalid"));
    }
}
