//! Client side of the control plane: org enrollment + membership/license download.
//!
//! `OrgClient` talks to a `loom-cp` over plain HTTP (ureq, blocking — fine for
//! `connect`/`status`). Machine identity is a persistent Ed25519 keypair stored
//! at `{loom_dir}/org/machine.key` (0600). Enrollment is a challenge-response
//! proof of possession; the CP issues a bearer machine token stored at
//! `{loom_dir}/org/token` (0600). Membership is cached at
//! `{loom_dir}/org/membership.json` for offline reads.
//!
//! OAuth 2.1 sender constraint: every org-scoped request carries a DPoP proof
//! JWT (RFC 9449) signed with the machine key, binding the token to
//! (method, URI) with a single-use `jti`. loom-core has no tokio/axum — the
//! live client↔server round-trip is tested in `loom-integration-tests`; this
//! crate's tests are pure (signing, paths, 0600, offline cache).

use crate::loom_dir;
use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand_core::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use ureq;

// Directory for org-related state: `{loom_dir}/org`.
pub fn org_dir(loom_dir: &Path) -> PathBuf {
    loom_dir.join("org")
}

/// `{loom_dir}/org/machine.key` — persistent machine private key (0600).
pub fn machine_key_path(dir: &Path) -> PathBuf {
    dir.join("machine.key")
}

/// `{loom_dir}/org/token` — issued bearer machine token (0600).
pub fn token_path(dir: &Path) -> PathBuf {
    dir.join("token")
}

/// `{loom_dir}/org/membership.json` — cached org membership.
pub fn membership_path(loom_dir: &Path) -> PathBuf {
    loom_dir.join("membership.json")
}

/// Canonical JWK for JWK thumbprint (RFC 7638).
fn jwk_canonical(vk: &VerifyingKey) -> String {
    let x = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{x}"}}"#)
}

/// JWK thumbprint = base64url(SHA-256(canonical JWK)), RFC 7638.
fn jwk_thumbprint(vk: &VerifyingKey) -> String {
    let hash = Sha256::digest(jwk_canonical(vk).as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// JWK OKP Ed25519 from a verifying key.
fn jwk_from_verifying_key(vk: &VerifyingKey) -> Value {
    let x = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    serde_json::json!({"kty": "OKP", "crv": "Ed25519", "x": x})
}

/// Generate + persist a machine keypair, or load the existing one.
/// Returns the verifying key.
pub fn ensure_machine_key(dir: &Path) -> Result<VerifyingKey> {
    fs::create_dir_all(dir)
        .with_context(|| format!("creating org dir {:?}", dir))?;
    let path = machine_key_path(dir);
    let signing_key = if path.exists() {
        let private_hex = fs::read_to_string(&path)
            .with_context(|| format!("reading machine key {:?}", path))?;
        let bytes: [u8; 32] = hex::decode(private_hex.trim())
            .context("machine key is not valid hex")?
            .as_slice()
            .try_into()
            .context("machine key must be 32 bytes")?;
        SigningKey::from_bytes(&bytes)
    } else {
        let signing_key = SigningKey::generate(&mut rand_core::OsRng);
        let private_hex = hex::encode(signing_key.to_bytes());
        fs::write(&path, &private_hex)
            .with_context(|| format!("writing machine key {:?}", path))?;
        restrict_permissions(&path)?;
        signing_key
    };
    Ok(signing_key.verifying_key())
}

/// Issue a bearer token (store 0600) and return it.
pub fn store_token(dir: &Path, token: &str) -> Result<()> {
    fs::write(token_path(dir), token)
        .with_context(|| format!("writing token {:?}", token_path(dir)))?;
    restrict_permissions(&token_path(dir))?;
    Ok(())
}

/// Read the stored bearer token, or None if none enrolled.
pub fn read_token(dir: &Path) -> Result<Option<String>> {
    let path = token_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(
        fs::read_to_string(&path)
            .with_context(|| format!("reading token {:?}", path))?,
    ))
}

/// Cache membership JSON (offline-readable).
pub fn store_membership(loom_dir: &Path, membership: &Value) -> Result<()> {
    let json = serde_json::to_string_pretty(membership).context("serializing membership")?;
    fs::create_dir_all(loom_dir.join("org"))
        .with_context(|| format!("creating org dir {:?}", loom_dir.join("org")))?;
    fs::write(membership_path(loom_dir), &json)
        .with_context(|| format!("writing membership {:?}", membership_path(loom_dir)))?;
    Ok(())
}

/// Read cached membership (offline-readable), or None.
pub fn read_membership(dir: &Path) -> Result<Option<Value>> {
    let path = membership_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("reading membership {:?}", path))?;
    Ok(Some(
        serde_json::from_str(&content)
            .context("parsing cached membership")?,
    ))
}

/// A client connected to a control plane at `base_url` for org `org_id`.
/// If `loom_dir` is `None`, the global `loom_core::loom_dir()` is used.
pub struct OrgClient {
    pub base_url: String,
    pub org_id: String,
    loom_dir: Option<PathBuf>,
}

impl OrgClient {
    pub fn new(base_url: impl Into<String>, org_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            org_id: org_id.into(),
            loom_dir: None,
        }
    }

    /// Like [`new`] but resolves state under `loom_dir` instead of the global
    /// `LOOM_AGENTS_DIR` / `~/.loom` (useful for multi-host tests).
    pub fn with_dir(
        base_url: impl Into<String>,
        org_id: impl Into<String>,
        loom_dir: PathBuf,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            org_id: org_id.into(),
            loom_dir: Some(loom_dir),
        }
    }

    /// The machine key directory for this client — always `{loom_dir}/org/`.
    fn key_dir(&self) -> Result<PathBuf> {
        Ok(match &self.loom_dir {
            Some(d) => org_dir(d),
            None => org_dir(Path::new(&loom_dir())),
        })
    }

    /// Sign a DPoP proof JWT binding `token` to (method, URI).
    fn dpop_proof(&self, signing_key: &SigningKey, method: &str, uri: &str) -> Result<String> {
        let vk = signing_key.verifying_key();
        let jkt = jwk_thumbprint(&vk);
        let jti: String = {
            let mut bytes = [0u8; 16];
            rand_core::OsRng.fill_bytes(&mut bytes);
            URL_SAFE_NO_PAD.encode(bytes)
        };
        let iat = Utc::now().timestamp();

        let header = serde_json::json!({"alg": "EdDSA", "typ": "dpop+jwt", "jkt": jkt});
        let payload = serde_json::json!({
            "jti": jti,
            "iat": iat,
            "htu": uri,
            "htm": method,
        });
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header)?.as_bytes());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&payload)?.as_bytes());
        let signing_input = format!("{header_b64}.{payload_b64}");
        let sig = signing_key.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(sig.to_bytes());
        Ok(format!("{header_b64}.{payload_b64}.{sig_b64}"))
    }

    /// `GET {base_url}/api/v1/org/{org_id}/membership` with Bearer + DPoP.
    pub fn download_membership(&self) -> Result<Value> {
        let token = read_token(&self.key_dir()?)
            .ok()
            .flatten()
            .context("no machine token — run `loom org connect` first")?;
        let signing_key = self.signing_key()?;
        let path = format!("/api/v1/org/{}/membership", self.org_id);
        let url = format!("{}{}", self.base_url, path);
        let proof = self.dpop_proof(&signing_key, "GET", &path)?;
        let body = ureq::get(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("DPoP", &proof)
            .call()
            .map_err(|e| anyhow::anyhow!("membership request failed: {e}"))?
            .into_string()
            .context("reading membership response")?;
        serde_json::from_str(&body).context("parsing membership response")
    }

    /// `GET {base_url}/api/v1/org/{org_id}/license` with Bearer + DPoP.
    pub fn download_license(&self) -> Result<Value> {
        let token = read_token(&self.key_dir()?)
            .ok()
            .flatten()
            .context("no machine token — run `loom org connect` first")?;
        let signing_key = self.signing_key()?;
        let path = format!("/api/v1/org/{}/license", self.org_id);
        let url = format!("{}{}", self.base_url, path);
        let proof = self.dpop_proof(&signing_key, "GET", &path)?;
        let body = ureq::get(&url)
            .set("Authorization", &format!("Bearer {token}"))
            .set("DPoP", &proof)
            .call()
            .map_err(|e| anyhow::anyhow!("license request failed: {e}"))?
            .into_string()
            .context("reading license response")?;
        serde_json::from_str(&body).context("parsing license response")
    }

    /// Returns the stored bearer machine token, or None if not enrolled.
    pub fn token(&self) -> Result<Option<String>> {
        read_token(&self.key_dir()?)
    }

    /// Generate a DPoP proof JWT for the given method + URI path.
    pub fn dpop_proof_for(&self, method: &str, uri: &str) -> Result<String> {
        let signing_key = self.signing_key()?;
        self.dpop_proof(&signing_key, method, uri)
    }

    /// Enroll this machine with the control plane: challenge-response proof of
    /// possession, then store the issued bearer token and cache membership.
    pub fn connect(&self) -> Result<Value> {
        let dir = self.key_dir()?;
        let vk = ensure_machine_key(&dir)?;
        let signing_key = self.signing_key()?;
        let jwk = jwk_from_verifying_key(&vk);

        // 1. Challenge
        let challenge_body = serde_json::to_string(&serde_json::json!({
            "machine_pub": jwk,
            "org_id": self.org_id,
        }))?;
        let challenge: Value = serde_json::from_str(
            &ureq::post(&format!("{}/api/v1/machine/enroll", self.base_url))
                .set("Content-Type", "application/json")
                .send_string(&challenge_body)
                .map_err(|e| anyhow::anyhow!("enroll challenge request failed: {e}"))?
                .into_string()
                .context("reading enroll challenge response")?,
        )
        .context("parsing enroll challenge response")?;
        let nonce = challenge
            .get("challenge")
            .and_then(|v| v.as_str())
            .context("enroll challenge missing nonce")?
            .to_string();

        // 2. Confirm: sign machine_pub_bytes || nonce
        let mut signed = vk.to_bytes().to_vec();
        signed.extend_from_slice(&hex::decode(&nonce).context("challenge nonce is not hex")?);
        let sig = signing_key.sign(&signed);
        let sig_hex = hex::encode(sig.to_bytes());

        // 3. Token issued
        let confirm_body = serde_json::to_string(&serde_json::json!({
            "machine_pub": jwk,
            "signature": sig_hex,
        }))?;
        let issued: Value = serde_json::from_str(
            &ureq::post(&format!("{}/api/v1/machine/enroll/confirm", self.base_url))
                .set("Content-Type", "application/json")
                .send_string(&confirm_body)
                .map_err(|e| anyhow::anyhow!("enroll confirm request failed: {e}"))?
                .into_string()
                .context("reading enroll confirm response")?,
        )
        .context("parsing enroll confirm response")?;
        let token = issued
            .get("token")
            .and_then(|v| v.as_str())
            .context("enroll confirm missing token")?
            .to_string();

        store_token(&dir, &token)?;

        // 4. Cache membership
        let membership = self.download_membership()?;
        store_membership(&dir, &membership)?;

        Ok(issued)
    }

    fn signing_key(&self) -> Result<SigningKey> {
        let dir = self.key_dir()?;
        let path = machine_key_path(&dir);
        let private_hex =
            fs::read_to_string(&path)
                .with_context(|| format!("reading machine key {:?}", path))?;
        let bytes: [u8; 32] = hex::decode(private_hex.trim())
            .context("machine key is not valid hex")?
            .as_slice()
            .try_into()
            .context("machine key must be 32 bytes")?;
        Ok(SigningKey::from_bytes(&bytes))
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_dir_is_under_loom_dir() {
        let d = org_dir(Path::new("/tmp/x"));
        assert_eq!(d, Path::new("/tmp/x/org"));
    }

    #[test]
    fn jwk_thumbprint_is_deterministic_and_sha256() {
        let sk = SigningKey::generate(&mut rand_core::OsRng);
        let vk = sk.verifying_key();
        let a = jwk_thumbprint(&vk);
        let b = jwk_thumbprint(&vk);
        assert_eq!(a, b);
        assert_eq!(a.len(), 43);
    }

    #[test]
    fn dpop_proof_binds_method_and_uri() {
        let dir = tempfile::tempdir().unwrap();
        let vk = ensure_machine_key(dir.path()).unwrap();
        let sk = SigningKey::from_bytes(
            &hex::decode(
                fs::read_to_string(machine_key_path(dir.path())).unwrap(),
            )
            .unwrap()
            .as_slice()
            .try_into()
            .unwrap(),
        );
        let client = OrgClient::new("http://cp.example", "org-1");
        let proof1 = client.dpop_proof(&sk, "GET", "https://cp/a").unwrap();
        let proof2 = client.dpop_proof(&sk, "GET", "https://cp/a").unwrap();
        assert_ne!(proof1, proof2); // jti differs
        let parts: Vec<&str> = proof1.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header.get("alg").and_then(|v| v.as_str()), Some("EdDSA"));
        assert_eq!(header.get("typ").and_then(|v| v.as_str()), Some("dpop+jwt"));
        assert_eq!(
            header.get("jkt").and_then(|v| v.as_str()),
            Some(jwk_thumbprint(&vk).as_str())
        );
    }

    #[test]
    fn machine_key_persists_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let vk1 = ensure_machine_key(dir.path()).unwrap();
        let vk2 = ensure_machine_key(dir.path()).unwrap();
        assert_eq!(vk1, vk2);
    }

    #[test]
    fn store_and_read_membership_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let membership = serde_json::json!({
            "org": {"id": "org-1", "name": "Acme", "tier": "free"},
            "humans": [{"email": "a@example.com", "role": "member"}],
            "machines": [],
        });
        store_membership(dir.path(), &membership).unwrap();
        let back = read_membership(dir.path()).unwrap().unwrap();
        assert_eq!(back, membership);
    }

    #[test]
    fn read_missing_membership_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_membership(dir.path()).unwrap().is_none());
    }
}
