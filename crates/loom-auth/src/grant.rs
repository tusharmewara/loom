//! Scoped credential grants.
//!
//! A grant is a derived, revocable, time-boxed capability that authorizes
//! exactly one tool, in exactly one run, to use exactly one stored
//! credential — while the agent holds a value that is *not* the stored
//! secret and is useless against the upstream provider.
//!
//! Construction (no new dependency, no new cipher):
//!
//! ```text
//! grant_key = SHA-256("loom-grant-key-v1" ‖ master_key ‖ len‖secret ‖ len‖provider)
//! token     = "loom-grant-v1" "." grant_id "." hex(nonce) "." hex(ciphertext)
//! ciphertext = ChaCha20Poly1305(grant_key).encrypt(
//!                nonce = random(12),
//!                msg   = "loom-grant-v1",
//!                aad   = scope_aad(grant_id, provider, run, tool, issued_at, expires_at))
//! ```
//!
//! - The agent holds the AEAD ciphertext of a fixed marker. Recovering the
//!   stored secret would require the master key, which never leaves the
//!   vault.
//! - The AEAD tag authenticates the token; the AAD binds the scope, so
//!   editing the registry record (tool, run, expiry) breaks decryption.
//! - `grant_key` mixes in the master key *and* the stored secret, so
//!   rotating either one (password change, credential re-set) invalidates
//!   every outstanding grant.
//!
//! Trust model and residual risks: `docs/architecture/grant-trust-model.md`.

use anyhow::{Context, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tracing::{debug, warn};

use crate::vault::hex;

/// Token format prefix. Bump when the derivation changes.
pub const GRANT_TOKEN_PREFIX: &str = "loom-grant-v1";

/// Default grant lifetime in seconds (15 minutes).
pub const DEFAULT_GRANT_TTL_SECS: u64 = 900;

/// AEAD plaintext marker — proves the token was minted by the vault.
const GRANT_MARKER: &[u8] = b"loom-grant-v1";

const GRANT_ID_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;

/// Current unix timestamp in seconds.
pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A grant as recorded in the registry.
///
/// The registry never stores the token or the secret — only the scope and
/// lifecycle. The token lives with the agent that asked for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantRecord {
    /// Unique grant identifier.
    pub grant_id: String,
    /// Provider whose credential the grant authorizes.
    pub provider_id: String,
    /// Run identifier the grant is scoped to.
    pub run_id: String,
    /// Tool the grant is scoped to.
    pub tool: String,
    /// Issue time (unix seconds).
    pub issued_at: u64,
    /// Expiry time (unix seconds), enforced at use time.
    pub expires_at: u64,
    /// Whether the grant has been revoked.
    #[serde(default)]
    pub revoked: bool,
    /// When the grant was revoked (unix seconds).
    #[serde(default)]
    pub revoked_at: Option<u64>,
}

impl GrantRecord {
    /// True when the grant is past its expiry at `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
}

/// A freshly issued grant handed back to the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedGrant {
    /// Grant identifier.
    pub grant_id: String,
    /// The opaque token the agent holds. Never the stored secret.
    pub token: String,
    /// Provider the grant authorizes.
    pub provider_id: String,
    /// Run the grant is scoped to.
    pub run_id: String,
    /// Tool the grant is scoped to.
    pub tool: String,
    /// Issue time (unix seconds).
    pub issued_at: u64,
    /// Expiry time (unix seconds).
    pub expires_at: u64,
}

/// Registry file format on disk.
#[derive(Debug, Serialize, Deserialize)]
struct GrantFile {
    version: u32,
    grants: Vec<GrantRecord>,
}

impl Default for GrantFile {
    fn default() -> Self {
        Self {
            version: 1,
            grants: Vec::new(),
        }
    }
}

/// Persistent registry of granted scopes and their lifecycle.
///
/// Thread-safe via `RwLock`. Reloads from disk when the file changed
/// underneath us, so a revocation performed by another process is picked up
/// immediately instead of at the next restart.
pub struct GrantStore {
    inner: RwLock<GrantStoreInner>,
    loom_dir: String,
}

struct GrantStoreInner {
    grants: HashMap<String, GrantRecord>,
    /// Content hash of the file the in-memory state was loaded from.
    stamp: Option<String>,
}

impl GrantStore {
    /// Create a store for `loom_dir`, loading any existing grants.
    pub fn new(loom_dir: &str) -> Self {
        let store = Self {
            inner: RwLock::new(GrantStoreInner {
                grants: HashMap::new(),
                stamp: None,
            }),
            loom_dir: loom_dir.to_string(),
        };
        if let Err(e) = store.load() {
            warn!(error = %e, "failed to load grant registry");
        }
        store
    }

    /// Path of the grant registry file.
    pub fn path(&self) -> PathBuf {
        Path::new(&self.loom_dir).join("auth").join("grants.json")
    }

    /// Read the registry from disk, replacing the in-memory state.
    pub fn load(&self) -> Result<()> {
        let path = self.path();
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if !path.exists() {
            inner.grants.clear();
            inner.stamp = None;
            return Ok(());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read grant registry: {:?}", path))?;
        let file: GrantFile =
            serde_json::from_str(&raw).context("failed to parse grant registry")?;
        inner.grants = file
            .grants
            .into_iter()
            .map(|g| (g.grant_id.clone(), g))
            .collect();
        inner.stamp = file_stamp(&path);
        Ok(())
    }

    /// Reload only if the file changed since the last load.
    pub fn reload_if_changed(&self) {
        let path = self.path();
        let current = file_stamp(&path);
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if current != inner.stamp {
            drop(inner);
            if let Err(e) = self.load() {
                warn!(error = %e, "failed to reload grant registry");
            }
        }
    }

    /// Persist the registry to disk.
    pub fn save(&self) -> Result<()> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create auth dir: {:?}", parent))?;
        }
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let file = GrantFile {
            version: 1,
            grants: {
                let mut v: Vec<GrantRecord> = inner.grants.values().cloned().collect();
                v.sort_by(|a, b| {
                    a.issued_at
                        .cmp(&b.issued_at)
                        .then(a.grant_id.cmp(&b.grant_id))
                });
                v
            },
        };
        let json = serde_json::to_string_pretty(&file)?;
        drop(inner);

        // Write-then-rename so a crash cannot leave a half-written registry.
        // The registry is the revocation source of truth, so it is written
        // owner-only (0600) — see docs/architecture/grant-trust-model.md.
        let tmp = path.with_extension("json.tmp");
        crate::vault::write_private(&tmp, json.as_bytes())
            .with_context(|| format!("failed to write grant registry: {:?}", tmp))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("failed to replace grant registry: {:?}", path))?;
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.stamp = file_stamp(&path);
        Ok(())
    }

    /// Insert (or replace) a grant record.
    pub fn insert(&self, record: GrantRecord) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.grants.insert(record.grant_id.clone(), record);
    }

    /// Look up a grant by id.
    pub fn get(&self, grant_id: &str) -> Option<GrantRecord> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.grants.get(grant_id).cloned()
    }

    /// All known grants.
    pub fn list(&self) -> Vec<GrantRecord> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<GrantRecord> = inner.grants.values().cloned().collect();
        v.sort_by(|a, b| {
            a.issued_at
                .cmp(&b.issued_at)
                .then(a.grant_id.cmp(&b.grant_id))
        });
        v
    }

    /// Mark a grant revoked. Returns false when the grant is unknown.
    pub fn revoke(&self, grant_id: &str, now: u64) -> bool {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        match inner.grants.get_mut(grant_id) {
            Some(record) => {
                record.revoked = true;
                record.revoked_at = Some(now);
                true
            }
            None => false,
        }
    }
}

/// Content hash of the registry file — the change detector.
///
/// The file is small (one record per grant) and a content hash avoids the
/// coarse timestamp granularity of the filesystem.
fn file_stamp(path: &Path) -> Option<String> {
    let raw = std::fs::read(path).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&raw);
    Some(hex::encode(&hasher.finalize()))
}

// ─── Token construction ────────────────────────────────────────────

/// Canonical, unambiguous encoding of a grant's scope.
///
/// Length-prefixed fields keep `provider="a|b"` from colliding with
/// `provider="a", run="b"`.
fn scope_aad(record: &GrantRecord) -> Vec<u8> {
    let mut aad = Vec::new();
    aad.extend_from_slice(GRANT_TOKEN_PREFIX.as_bytes());
    for field in [
        &record.grant_id,
        &record.provider_id,
        &record.run_id,
        &record.tool,
    ] {
        aad.extend_from_slice(&(field.len() as u64).to_le_bytes());
        aad.extend_from_slice(field.as_bytes());
    }
    aad.extend_from_slice(&record.issued_at.to_le_bytes());
    aad.extend_from_slice(&record.expires_at.to_le_bytes());
    aad
}

/// Generate a random grant identifier.
pub fn new_grant_id() -> String {
    let mut bytes = [0u8; GRANT_ID_BYTES];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(&bytes)
}

/// Mint a token for `record` under `grant_key`.
pub fn mint_token(grant_key: &[u8; 32], record: &GrantRecord) -> Result<String> {
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = ChaCha20Poly1305::new(Key::from_slice(grant_key));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: GRANT_MARKER,
                aad: &scope_aad(record),
            },
        )
        .map_err(|e| anyhow::anyhow!("grant token encryption failed: {e}"))?;

    Ok(format!(
        "{}.{}.{}.{}",
        GRANT_TOKEN_PREFIX,
        record.grant_id,
        hex::encode(&nonce_bytes),
        hex::encode(&ciphertext)
    ))
}

/// Parsed token components.
pub struct ParsedToken<'a> {
    /// Grant identifier carried by the token.
    pub grant_id: &'a str,
    /// AEAD nonce.
    pub nonce: Vec<u8>,
    /// AEAD ciphertext.
    pub ciphertext: Vec<u8>,
}

/// Split a token into its components.
pub fn parse_token(token: &str) -> Result<ParsedToken<'_>> {
    let mut parts = token.split('.');
    let prefix = parts.next().unwrap_or_default();
    let grant_id = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("malformed grant token"))?;
    let nonce_hex = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("malformed grant token"))?;
    let ct_hex = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("malformed grant token"))?;
    if parts.next().is_some() {
        anyhow::bail!("malformed grant token");
    }
    if prefix != GRANT_TOKEN_PREFIX {
        anyhow::bail!("unknown grant token format");
    }
    let nonce =
        hex::decode(nonce_hex).map_err(|e| anyhow::anyhow!("malformed grant token: {e}"))?;
    let ciphertext =
        hex::decode(ct_hex).map_err(|e| anyhow::anyhow!("malformed grant token: {e}"))?;
    if nonce.len() != NONCE_BYTES {
        anyhow::bail!("malformed grant token");
    }
    Ok(ParsedToken {
        grant_id,
        nonce,
        ciphertext,
    })
}

/// Verify that `token` was minted for `record` under `grant_key`.
///
/// Fails when the token was minted for a different scope, under a different
/// key (rotation), or was tampered with.
pub fn open_token(grant_key: &[u8; 32], record: &GrantRecord, token: &str) -> Result<()> {
    let parsed = parse_token(token)?;
    if parsed.grant_id != record.grant_id {
        anyhow::bail!("grant token does not match grant id");
    }
    if parsed.nonce.len() != NONCE_BYTES {
        anyhow::bail!("malformed grant token");
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(grant_key));
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&parsed.nonce),
            Payload {
                msg: &parsed.ciphertext,
                aad: &scope_aad(record),
            },
        )
        .map_err(|_| anyhow::anyhow!("grant token is not valid for this grant"))?;
    if plain.as_slice() != GRANT_MARKER {
        anyhow::bail!("grant token is not valid for this grant");
    }
    debug!(grant_id = %record.grant_id, "grant token verified");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> GrantRecord {
        GrantRecord {
            grant_id: new_grant_id(),
            provider_id: "openai".into(),
            run_id: "run-1".into(),
            tool: "claude-code".into(),
            issued_at: 1_000,
            expires_at: 2_000,
            revoked: false,
            revoked_at: None,
        }
    }

    #[test]
    fn test_token_roundtrip() {
        let key = [7u8; 32];
        let rec = record();
        let token = mint_token(&key, &rec).unwrap();
        assert!(token.starts_with("loom-grant-v1."));
        open_token(&key, &rec, &token).unwrap();
    }

    #[test]
    fn test_token_bound_to_scope() {
        let key = [7u8; 32];
        let rec = record();
        let token = mint_token(&key, &rec).unwrap();

        let mut other = rec.clone();
        other.tool = "other-tool".into();
        assert!(open_token(&key, &other, &token).is_err());

        let mut other = rec.clone();
        other.expires_at += 1;
        assert!(open_token(&key, &other, &token).is_err());
    }

    #[test]
    fn test_token_rejects_wrong_key() {
        let rec = record();
        let token = mint_token(&[7u8; 32], &rec).unwrap();
        assert!(open_token(&[9u8; 32], &rec, &token).is_err());
    }

    #[test]
    fn test_malformed_tokens() {
        assert!(parse_token("nope").is_err());
        assert!(parse_token("loom-grant-v1.a.b").is_err());
        assert!(parse_token("other-v1.a.b.c").is_err());
    }

    #[test]
    fn test_scope_aad_unambiguous() {
        let mut a = record();
        a.provider_id = "a|b".into();
        a.run_id = "c".into();
        let mut b = record();
        b.provider_id = "a".into();
        b.run_id = "b|c".into();
        assert_ne!(scope_aad(&a), scope_aad(&b));
    }

    #[test]
    fn test_store_persist_and_revoke() {
        let dir = tempfile::TempDir::new().unwrap();
        let store = GrantStore::new(dir.path().to_str().unwrap());
        let rec = record();
        store.insert(rec.clone());
        store.save().unwrap();

        let reopened = GrantStore::new(dir.path().to_str().unwrap());
        assert!(reopened.get(&rec.grant_id).is_some());
        assert!(reopened.revoke(&rec.grant_id, 1_500));
        reopened.save().unwrap();

        let again = GrantStore::new(dir.path().to_str().unwrap());
        let r = again.get(&rec.grant_id).unwrap();
        assert!(r.revoked);
        assert_eq!(r.revoked_at, Some(1_500));
    }
}
