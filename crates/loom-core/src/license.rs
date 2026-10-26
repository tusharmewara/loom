//! Offline license-key minting and verification.
//!
//! Ed25519-signed license keys (`base64(payload JSON).base64(sig)`) with
//! claims `org_id`, `tier`, `seats`, `expires_at`, `features`. The private key
//! never leaves the maintainer machine; loomd (phase 2) consumes only the
//! public key. Custody and mint-path decisions: `docs/decisions/0004-license-mint-custody.md`.
//!
//! PR4 scope: this module is the *only* mint path. Phase 6 (control-plane
//! `license_issuance`) must call [`mint_license`] — never a parallel one.
//! Phase 2's `license gate` consumes [`verify_license_key`] as-is.

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Maximum license validity: keys dye with expiry within one year
/// (plan: lapsed subscriptions resolve by re-minting a shorter key).
pub const LICENSE_MAX_VALIDITY_DAYS: i64 = 365;

/// Where the private key lives inside the admin dir (`~/.loom-admin/`).
pub const LICENSE_KEY_FILE: &str = "license.key";

/// The license claims signed into a license key. This is the payload shape
/// phase 2's `letter` / `loom-core` license gate will build its `License`
/// struct on; keep field names stable once phase 2 ships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicensePayload {
    pub org_id: String,
    pub tier: String,
    /// Soft seat count — a purchasing term; hosted CP-side cap only.
    pub seats: u32,
    /// RFC3339 UTC expiry. At most one year out (see [`LICENSE_MAX_VALIDITY_DAYS`]).
    pub expires_at: String,
    pub features: Vec<String>,
}

/// Generate a fresh Ed25519 keypair for license signing.
/// Returns `(private_key_hex, public_key_hex)`.
pub fn generate_keypair() -> Result<(String, String)> {
    let signing_key = SigningKey::generate(&mut OsRng);
    Ok((
        hex::encode(signing_key.to_bytes()),
        hex::encode(signing_key.verifying_key().to_bytes()),
    ))
}

/// Evaluate the payload against the license model's invariants.
fn validate_payload(payload: &LicensePayload) -> Result<()> {
    if payload.org_id.trim().is_empty() {
        bail!("org_id must not be empty");
    }
    if payload.tier.trim().is_empty() {
        bail!("tier must not be empty");
    }
    if payload.seats == 0 {
        bail!("seats must be at least 1");
    }
    let expires = DateTime::parse_from_rfc3339(&payload.expires_at)
        .with_context(|| format!("expires_at must be RFC3339, got '{}'", payload.expires_at))?
        .with_timezone(&Utc);
    if expires <= Utc::now() {
        bail!("expires_at must be in the future");
    }
    if expires > Utc::now() + chrono::Duration::days(LICENSE_MAX_VALIDITY_DAYS) {
        bail!(
            "expires_at must be at most {} days out — the license model dyes keys with expiry within one year",
            LICENSE_MAX_VALIDITY_DAYS
        );
    }
    Ok(())
}

/// Mint a license key: `base64url(payload JSON).base64url(signature)`.
/// Signs the exact payload JSON bytes. The one mint path — control-plane
/// issuance (phase 6) calls this same function.
pub fn mint_license(payload: &LicensePayload, private_key_hex: &str) -> Result<String> {
    validate_payload(payload)?;
    let private = hex::decode(private_key_hex.trim()).context("private key is not valid hex")?;
    let bytes: [u8; 32] = private
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("private key must be 32 bytes (64 hex characters)"))?;
    let signing_key = SigningKey::from_bytes(&bytes);
    let payload_json = serde_json::to_vec(payload)?;
    let signature = signing_key.sign(&payload_json);
    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(&payload_json),
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// Verify a license key against a pinned public key and return its claims.
/// The phase-2 license gate's verify: the pinning invariant is loomd's
/// embedded Ed25519 public key — the only non-self-hostable check.
pub fn verify_license_key(token: &str, public_key_hex: &str) -> Result<LicensePayload> {
    let (payload_b64, sig_b64) = token
        .trim()
        .split_once('.')
        .context("license key must be <b64 payload>.<b64 signature>")?;
    if sig_b64.contains('.') {
        bail!("license key must have exactly one '.' separator");
    }
    let payload_json = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .context("license key payload is not valid base64")?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .context("license key signature is not valid base64")?;
    let pk_bytes: [u8; 32] = hex::decode(public_key_hex.trim())
        .context("public key is not valid hex")?
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes (64 hex characters)"))?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| anyhow::anyhow!("invalid ed25519 public key: {e}"))?;
    let signature: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    verifying_key
        .verify(&payload_json, &Signature::from_bytes(&signature))
        .map_err(|_| anyhow::anyhow!("license signature verification failed"))?;
    let payload: LicensePayload = serde_json::from_slice(&payload_json)
        .context("license payload is not valid license claims")?;
    Ok(payload)
}

/// Path of the private key file inside an admin dir.
pub fn license_key_path(dir: &Path) -> PathBuf {
    dir.join(LICENSE_KEY_FILE)
}

/// Generate a keypair and write the private key to `<dir>/license.key`
/// (mode `0600` on unix). Refuses to clobber an existing key unless
/// `force`. Returns the public key hex (the only thing worth printing).
pub fn write_license_key_file(dir: &Path, force: bool) -> Result<String> {
    let path = license_key_path(dir);
    if path.exists() && !force {
        bail!(
            "{} already exists — refusing to overwrite (pass --force)",
            path.display()
        );
    }
    let (private_hex, public_hex) = generate_keypair()?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, &private_hex)
        .with_context(|| format!("writing {}", path.display()))?;
    restrict_permissions(&path)?;
    Ok(public_hex)
}

/// Read the private key hex from `<dir>/license.key`.
pub fn read_license_private_key(dir: &Path) -> Result<String> {
    let path = license_key_path(dir);
    std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {} — run `loom-admin keygen` first", path.display()))
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_payload(expires_at: String) -> LicensePayload {
        LicensePayload {
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

    fn sample_token(keypair: &(String, String)) -> String {
        mint_license(&sample_payload(future_expiry(180)), &keypair.0).unwrap()
    }

    #[test]
    fn mint_then_verify_roundtrips_payload() {
        let expiry = future_expiry(180);
        let keypair = generate_keypair().unwrap();
        let token = mint_license(&sample_payload(expiry.clone()), &keypair.0).unwrap();
        let verified = verify_license_key(&token, &keypair.1).unwrap();
        assert_eq!(verified, sample_payload(expiry));
        // deterministic structure: exactly one separator, both parts base64
        assert_eq!(token.matches('.').count(), 1);
    }

    #[test]
    fn wrong_key_never_verifies_pinning() {
        let keypair_a = generate_keypair().unwrap();
        let keypair_b = generate_keypair().unwrap();
        let token = sample_token(&keypair_a);
        let err = verify_license_key(&token, &keypair_b.1).unwrap_err();
        assert!(err.to_string().contains("verification failed"));
    }

    #[test]
    fn tampered_payload_never_verifies() {
        let keypair = generate_keypair().unwrap();
        let token = sample_token(&keypair);
        let (payload, sig) = token.split_once('.').unwrap();
        let mut json = URL_SAFE_NO_PAD.decode(payload).unwrap();
        if json[0] == b'{' {
            json[0] = b'[';
        } else {
            json[0] = b'{';
        }
        let tampered = format!("{}.{}", URL_SAFE_NO_PAD.encode(json), sig);
        assert!(verify_license_key(&tampered, &keypair.1).is_err());
    }

    #[test]
    fn token_never_contains_private_key() {
        let keypair = generate_keypair().unwrap();
        let token = sample_token(&keypair);
        assert!(!token.contains(&keypair.0));
    }

    #[test]
    fn mint_rejects_bad_payloads() {
        let keypair = generate_keypair().unwrap();
        for expires in [future_expiry(-1), future_expiry(366), "not-a-date".into()] {
            let err = mint_license(&sample_payload(expires), &keypair.0).unwrap_err();
            assert!(!err.to_string().is_empty());
        }
        let mut zero = sample_payload(future_expiry(30));
        zero.seats = 0;
        assert!(mint_license(&zero, &keypair.0).is_err());
        let mut no_org = sample_payload(future_expiry(30));
        no_org.org_id = "  ".into();
        assert!(mint_license(&no_org, &keypair.0).is_err());
    }

    #[test]
    fn verify_rejects_garbage_tokens() {
        let keypair = generate_keypair().unwrap();
        assert!(verify_license_key("nodot", &keypair.1).is_err());
        assert!(verify_license_key("a.b.c", &keypair.1).is_err());
        assert!(verify_license_key("!!!.&&&", &keypair.1).is_err());
        assert!(verify_license_key("", &keypair.1).is_err());
    }

    #[test]
    fn keygen_writes_0600_and_refuses_without_force() {
        let dir = tempdir().unwrap();
        let pub1 = write_license_key_file(dir.path(), false).unwrap();
        assert_eq!(pub1.len(), 64);

        let path = license_key_path(dir.path());
        if cfg!(unix) {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let refused = write_license_key_file(dir.path(), false).unwrap_err();
        assert!(refused.to_string().contains("refusing to overwrite"));
        write_license_key_file(dir.path(), true).unwrap(); // --force succeeds
        let pub2 = read_license_private_key(dir.path()).unwrap();
        // overwriting produced a live, different key
        let token = mint_license(&sample_payload(future_expiry(30)), &pub2).unwrap();
        assert!(verify_license_key(&token, &pub1).is_err());
    }
}
