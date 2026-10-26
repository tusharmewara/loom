//! `loom-admin` — vendor/admin utilities. NOT part of the customer-facing
//! `loom` CLI: license minting is done with the vendor's private key
//! (`docs/decisions/0004-license-mint-custody.md`). Key dir resolves via
//! `LOOM_ADMIN_DIR`, else `~/.loom-admin/` — deliberately separate from `~/.loom/`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "loom-admin",
    version,
    about = "Loom vendor/admin utilities (offline license minting)"
)]
struct AdminCli {
    #[command(subcommand)]
    command: AdminCommand,
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Generate the offline license signing private key (0600; refuses to
    /// overwrite an existing key without --force)
    Keygen {
        /// Overwrite an existing license.key
        #[arg(long)]
        force: bool,
    },
    /// Mint a signed offline license key and print (or write) it
    #[command(name = "licensemint")]
    LicenseMint {
        /// Organization id the license is issued to
        #[arg(long)]
        org: String,
        /// License tier (e.g. team)
        #[arg(long)]
        tier: String,
        /// Soft seat count (purchasing term; enforced only CP-side)
        #[arg(long)]
        seats: u32,
        /// Expiry as RFC3339, at most one year out
        #[arg(long)]
        expires: String,
        /// Feature flag to include (repeatable)
        #[arg(long)]
        feature: Vec<String>,
        /// Write the license key to this path instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// Admin dir: `LOOM_ADMIN_DIR` override, else `~/.loom-admin/`.
fn admin_dir() -> PathBuf {
    match std::env::var("LOOM_ADMIN_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(loom_core::home_dir()).join(".loom-admin"),
    }
}

fn run(command: AdminCommand, dir: &Path) -> Result<String> {
    match command {
        AdminCommand::Keygen { force } => {
            let public_hex = loom_core::license::write_license_key_file(dir, force)?;
            Ok(format!(
                "private key written to {} (never printed, never leaves this machine)\npublic key (embed in loomd at phase 2): {}",
                loom_core::license::license_key_path(dir).display(),
                public_hex
            ))
        }
        AdminCommand::LicenseMint {
            org,
            tier,
            seats,
            expires,
            feature,
            out,
        } => {
            let private_hex = loom_core::license::read_license_private_key(dir)?;
            let payload = loom_core::license::LicensePayload {
                org_id: org,
                tier,
                seats,
                expires_at: expires,
                features: feature,
            };
            let token = loom_core::license::mint_license(&payload, &private_hex)?;
            match out {
                Some(path) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, &token)
                        .with_context(|| format!("writing {}", path.display()))?;
                    Ok(format!("license key written to {}", path.display()))
                }
                None => Ok(token),
            }
        }
    }
}

fn main() {
    if let Err(err) = try_main() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn try_main() -> Result<()> {
    let cli = AdminCli::parse();
    let output = run(cli.command, &admin_dir())?;
    println!("{output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::tempdir;

    fn ends() -> String {
        (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339()
    }

    #[test]
    fn wrong_usage_is_refused() {
        assert!(AdminCli::try_parse_from(["loom-admin"]).is_err());
        // missing all mint flags
        assert!(AdminCli::try_parse_from(["loom-admin", "licensemint"]).is_err());
        // missing one required flag
        assert!(AdminCli::try_parse_from(
            ["loom-admin", "licensemint", "--org", "acme", "--tier", "team", "--seats", "10"]
        )
        .is_err());
    }

    #[test]
    fn keygen_writes_0600_refuses_without_force() {
        let dir = tempdir().unwrap();
        let keygen = || run(AdminCommand::Keygen { force: false }, dir.path());
        keygen().unwrap();
        let path = loom_core::license::license_key_path(dir.path());
        if cfg!(unix) {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(keygen().unwrap_err().to_string().contains("refusing"));
        run(AdminCommand::Keygen { force: true }, dir.path()).unwrap();
    }

    #[test]
    fn mint_refuses_without_key_file() {
        let dir = tempdir().unwrap();
        let err = run(
            AdminCommand::LicenseMint {
                org: "acme".into(),
                tier: "team".into(),
                seats: 10,
                expires: ends(),
                feature: vec![],
                out: None,
            },
            dir.path(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("loom-admin keygen"));
    }

    #[test]
    fn mint_refuses_bad_expiry() {
        let dir = tempdir().unwrap();
        run(AdminCommand::Keygen { force: false }, dir.path()).unwrap();
        for expires in ["not-a-date".to_string(), (chrono::Utc::now() + chrono::Duration::days(400)).to_rfc3339()] {
            let err = run(
                AdminCommand::LicenseMint {
                    org: "acme".into(),
                    tier: "team".into(),
                    seats: 10,
                    expires,
                    feature: vec![],
                    out: None,
                },
                dir.path(),
            )
            .unwrap_err();
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn mint_flow_verifies_against_lib_helper() {
        let dir = tempdir().unwrap();
        let out = tempdir().unwrap();
        let keygen_out = run(AdminCommand::Keygen { force: false }, dir.path()).unwrap();
        let public_hex = keygen_out
            .lines()
            .find_map(|l| l.strip_prefix("public key (embed in loomd at phase 2): "))
            .unwrap()
            .to_string();

        run(
            AdminCommand::LicenseMint {
                org: "acme".into(),
                tier: "team".into(),
                seats: 10,
                expires: ends(),
                feature: vec!["teams.sync".into()],
                out: Some(out.path().join("license.txt")),
            },
            dir.path(),
        )
        .unwrap();

        let token = std::fs::read_to_string(out.path().join("license.txt")).unwrap();
        let verified = loom_core::license::verify_license_key(&token, &public_hex).unwrap();
        assert_eq!(verified.org_id, "acme");
        assert_eq!(verified.seats, 10);
        assert_eq!(verified.features, vec!["teams.sync".to_string()]);
    }
}
