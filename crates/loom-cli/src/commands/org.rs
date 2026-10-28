//! `loom org` — connect to a control plane and inspect membership/license.

use anyhow::{Context, Result};
use chrono::Utc;
use loom_core::license::{License, LicenseState};
use loom_core::org::{membership_path, read_membership, OrgClient};
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
    let dir = Path::new(loom_dir);

    // License state (phase-2 gate).
    let license = License::load(dir);
    let state = license.state_at(Utc::now());

    // Cached org membership.
    let membership = read_membership(&membership_path(dir)).context(
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
    let humans = org.get("humans").and_then(|v| v.as_array()).map_or(0, |a| a.len());
    let machines = membership_obj
        .get("machines")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len());

    println!("Organization: {org_name} ({org_id})");
    println!("Tier: {tier}");
    println!("Humans: {humans} | Machines: {machines}");
    match state {
        LicenseState::Unlicensed => println!("License: unlicensed"),
        LicenseState::Invalid => {
            println!("License: invalid — {}", license.invalid_reason().unwrap_or("unknown"));
        }
        LicenseState::Active => {
            let p = license.payload().unwrap();
            println!("License: active ({}, {} seats)", p.tier, p.seats);
        }
        LicenseState::Grace => {
            let p = license.payload().unwrap();
            println!("License: grace (read-only) — {}", p.tier);
        }
        LicenseState::Expired => {
            let p = license.payload().unwrap();
            println!("License: expired — {}", p.tier);
        }
    }
    Ok(())
}
