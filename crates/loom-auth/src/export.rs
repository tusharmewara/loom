//! Portable, verifiable export of the audit hash chain.
//!
//! The export is the compliance artifact: a JSON document carrying the chain
//! entries (access metadata only — never credential material), the chain's
//! own integrity report, a digest over the exported document, and an Ed25519
//! signature over it.
//!
//! # Document shape
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "kind": "loom.audit.export",
//!   "exported_at": "2026-10-21T04:40:00Z",
//!   "source": {
//!     "log_path": "/home/u/.loom/auth/audit.log",
//!     "chain_total": 12,
//!     "chain_chained": 12,
//!     "chain_intact": true,
//!     "chain_tampered_at": null,
//!     "chain_head": "9f2c…"
//!   },
//!   "retention": {
//!     "window_days": 365,
//!     "policy": "drop",
//!     "dropped_entries": 3,
//!     "oldest_included": "2026-01-04T00:00:00Z",
//!     "newest_included": "2026-10-21T04:39:00Z"
//!   },
//!   "redaction": { "policy": "result-normalised", "redacted_entries": 1 },
//!   "entries": [
//!     {
//!       "timestamp": "2026-10-21T04:39:00Z",
//!       "requesting_tool": "claude-code",
//!       "provider_id": "anthropic",
//!       "access_type": "read",
//!       "outcome": "granted",
//!       "redacted": false,
//!       "prev_hash": "1a8b…",
//!       "entry_hash": "9f2c…",
//!       "chain_scheme": 2
//!     }
//!   ],
//!   "export_digest": "sha256 hex",
//!   "signature": {
//!     "algorithm": "ed25519",
//!     "public_key": "hex",
//!     "signature": "hex"
//!   }
//! }
//! ```
//!
//! # Trust model
//!
//! The export proves two different things, and the verification reports them
//! separately.
//!
//! 1. **Internal consistency — the recomputable chain.** Every entry carries
//!    the fields its chain hash was computed over, so a consumer can
//!    re-derive every `entry_hash` and detect a modified field, a reordered
//!    entry or a dropped entry. The chain hash is computed over the
//!    *normalised outcome* (`granted` / `denied` / `other`), never over the
//!    raw free-text `result`. That is what lets the export carry a chain a
//!    consumer can re-derive while keeping the raw text off the machine:
//!    redaction and verification are designed together, not independently.
//!    Entries written before this scheme (whose hash covers the raw result)
//!    carry `chain_scheme: 1`; they cannot be re-derived from the export and
//!    are covered by the signature instead.
//!
//! 2. **Provenance — the Ed25519 signature.** The document is signed with a
//!    per-machine key. A consumer holding the operator's pinned public key
//!    can prove the document came from that machine. Without a pinned key the
//!    document is only *self-consistent* and the verdict is `unanchored` —
//!    never `intact`.
//!
//! The `export_digest` is an unkeyed checksum. It detects accidental
//! corruption; an attacker who recomputes it gains nothing, because the
//! chain is re-derived and the signature verified independently of it.
//!
//! # Retention
//!
//! The retention window bounds where the exported chain *starts*: the
//! longest prefix of out-of-window entries is dropped and the rest is
//! exported. The drop is a prefix, never a scattered subset — the chain is
//! linear, so dropping an entry from the middle would break the linkage
//! [`verify_export`] checks. See [`build_export`].

use crate::audit::{
    chain_hash, entry_chain_scheme, normalize_outcome, AuditLogger, CHAIN_SCHEME_RAW,
};
use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Schema version of the export document.
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

/// Document discriminator, so a consumer can recognise the file.
pub const EXPORT_KIND: &str = "loom.audit.export";

/// Signature algorithm identifier carried in the export.
pub const SIGNATURE_ALGORITHM: &str = "ed25519";

/// File name of the per-machine export signing key, under `<loom-dir>/auth/`.
pub const SIGNING_KEY_FILE: &str = "export-signing.key";

/// A complete audit export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditExport {
    /// Format version (see [`EXPORT_SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// Always `loom.audit.export`.
    pub kind: String,
    /// When the export was produced (RFC3339, UTC).
    pub exported_at: String,
    /// Provenance of the source chain.
    pub source: ExportSource,
    /// Retention window applied while building the export.
    pub retention: ExportRetention,
    /// What was redacted from the payload.
    pub redaction: ExportRedaction,
    /// The exported entries, oldest first.
    pub entries: Vec<ExportEntry>,
    /// Digest over the canonical export document.
    pub export_digest: String,
    /// Ed25519 signature over the document, if it was signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<ExportSignature>,
}

/// The Ed25519 signature over an export document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSignature {
    /// Signature algorithm (`ed25519`).
    pub algorithm: String,
    /// Hex of the public key that produced the signature.
    pub public_key: String,
    /// Hex of the signature.
    pub signature: String,
}

/// Provenance and integrity report of the source chain.
///
/// `chain_intact` is a **self-report by the exporter** about the chain it
/// read at export time — it is not an attestation the document carries. The
/// authoritative checks are the re-derived chain and the signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSource {
    /// Path of the audit log the chain was read from.
    pub log_path: String,
    /// Entries in the source chain.
    pub chain_total: usize,
    /// Entries that are part of the hash chain.
    pub chain_chained: usize,
    /// Whether the source chain was intact at export time (self-reported).
    pub chain_intact: bool,
    /// Timestamp of the first entry that failed verification, if any.
    pub chain_tampered_at: Option<String>,
    /// Hash of the newest entry — the chain head at export time.
    pub chain_head: Option<String>,
}

/// The retention rule applied to the export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRetention {
    /// Configured window in days (`0` = keep everything).
    pub window_days: u32,
    /// What happens to entries past the window: `drop` (the oldest
    /// out-of-window prefix is left out of this export; the on-disk chain
    /// is never rewritten).
    pub policy: String,
    /// Entries left out because they fell outside the window.
    pub dropped_entries: usize,
    /// Oldest exported entry.
    pub oldest_included: Option<String>,
    /// Newest exported entry.
    pub newest_included: Option<String>,
}

/// What was redacted from the payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRedaction {
    /// Redaction rule identifier.
    pub policy: String,
    /// Entries whose free-text result was replaced by a normalised outcome.
    pub redacted_entries: usize,
}

/// One exported audit entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportEntry {
    /// When the access happened (RFC3339, UTC).
    pub timestamp: String,
    /// The tool that asked for access.
    pub requesting_tool: String,
    /// The provider whose credential was accessed.
    pub provider_id: String,
    /// `read`, `write` or `denied`.
    pub access_type: String,
    /// Normalised outcome: `granted`, `denied` or `other`.
    pub outcome: String,
    /// True when the source entry carried free text that was not exported.
    pub redacted: bool,
    /// Chain link to the previous entry.
    pub prev_hash: Option<String>,
    /// Chain hash of this entry.
    pub entry_hash: Option<String>,
    /// Chain hash scheme this entry was written with: `2` = the hash covers
    /// the normalised outcome (re-derivable from the export), `1` = the
    /// legacy scheme whose hash covers the raw result (not re-derivable from
    /// the export; covered by the signature). Absent on documents written
    /// before the scheme was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_scheme: Option<u8>,
}

/// The verdict of verifying an export document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportVerdict {
    /// The chain re-derives from the exported fields **and** the signature
    /// verified against a pinned public key.
    Intact,
    /// A check failed. [`ExportVerification::problems`] names which one.
    Tampered,
    /// The document is internally consistent, but no public key was pinned:
    /// it is only *self-consistent*, not proven to come from this machine.
    Unanchored,
}

/// The individual checks behind an [`ExportVerdict`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportChecks {
    /// The recomputed digest matched `export_digest`.
    pub digest_ok: bool,
    /// Every entry linked to the previous one.
    pub chain_linked: bool,
    /// Every entry's chain hash was re-derived from the exported fields and
    /// matched.
    pub chain_ok: bool,
    /// The last entry matched the recorded chain head.
    pub chain_head_ok: bool,
    /// The source chain was intact when the export was taken (self-reported).
    pub source_chain_intact: bool,
    /// A public key was pinned by the verifier.
    pub anchored: bool,
    /// The signature verified against the pinned public key.
    pub signature_ok: bool,
    /// Entries whose chain hash cannot be re-derived from the export (legacy
    /// entries whose raw result carried free text).
    pub entries_unverifiable: usize,
}

/// Result of verifying an exported document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportVerification {
    /// The single verdict.
    pub verdict: ExportVerdict,
    /// Number of entries in the document.
    pub entries: usize,
    /// The individual checks.
    pub checks: ExportChecks,
    /// Human-readable description of every problem found.
    pub problems: Vec<String>,
    /// Caveats that are not failures (e.g. entries that cannot be
    /// re-derived from the export).
    pub notes: Vec<String>,
}

/// Build an export of the audit chain, applying the retention window.
///
/// `retention_days` is the configured window. The window defines where the
/// exported chain *starts*: the longest prefix of entries that are all older
/// than the window is **dropped**, and everything from the first in-window
/// entry onwards is exported verbatim.
///
/// The drop is a prefix, never a scattered subset. The hash chain is a
/// linear sequence, so dropping an entry from the middle would break the
/// linkage [`AuditLogger::verify_integrity`] checks, and dropping the tail
/// would break the link to the chain head. Keeping the exported entries
/// contiguous is what makes the export verifiable. An entry that is out of
/// the window but sits *after* the cut is therefore kept (a clock skew or a
/// late-written entry) — the window bounds the start of the chain, not each
/// entry individually.
///
/// The on-disk chain is never rewritten. Disk is bounded instead by the
/// logger's existing size-based rotation.
///
/// The document is returned unsigned; call [`sign_export`] to sign it.
pub fn build_export(logger: &AuditLogger, retention_days: u32) -> Result<AuditExport> {
    let report = logger.verify_integrity()?;
    let all = logger.entries_chronological()?;

    let chain_head = all.iter().rev().find_map(|e| e.entry_hash.clone());
    // `0` means "keep everything" — a zero-day window would otherwise drop
    // every dated entry.
    let cutoff = if retention_days == 0 {
        None
    } else {
        Some(loom_core::rfc3339_days_ago(retention_days))
    };

    // The longest prefix of out-of-window entries is dropped.
    let dropped_entries = cutoff
        .as_ref()
        .map(|cutoff| {
            all.iter()
                .take_while(|e| is_dated(&e.timestamp) && e.timestamp.as_str() < cutoff.as_str())
                .count()
        })
        .unwrap_or(0);

    let mut entries = Vec::new();
    let mut redacted_entries = 0usize;

    for entry in &all[dropped_entries..] {
        let outcome = normalize_outcome(&entry.result);
        let redacted = is_redacted(&entry.result);
        if redacted {
            redacted_entries += 1;
        }
        entries.push(ExportEntry {
            timestamp: entry.timestamp.clone(),
            requesting_tool: entry.requesting_tool.clone(),
            provider_id: entry.provider_id.clone(),
            access_type: entry.access_type.clone(),
            outcome: outcome.to_string(),
            redacted,
            prev_hash: entry.prev_hash.clone(),
            entry_hash: entry.entry_hash.clone(),
            // The scheme is detected by recomputation, so a log written by
            // an older Loom is exported with the scheme it actually uses.
            chain_scheme: entry_chain_scheme(entry).or(Some(CHAIN_SCHEME_RAW)),
        });
    }

    let oldest_included = entries.first().map(|e| e.timestamp.clone());
    let newest_included = entries.last().map(|e| e.timestamp.clone());

    let mut doc = AuditExport {
        schema_version: EXPORT_SCHEMA_VERSION,
        kind: EXPORT_KIND.to_string(),
        exported_at: loom_core::rfc3339_now(),
        source: ExportSource {
            log_path: logger.log_path(),
            chain_total: report.total,
            chain_chained: report.chained,
            chain_intact: report.intact,
            chain_tampered_at: report.tampered_at,
            chain_head,
        },
        retention: ExportRetention {
            window_days: retention_days,
            policy: "drop".to_string(),
            dropped_entries,
            oldest_included,
            newest_included,
        },
        redaction: ExportRedaction {
            policy: "result-normalised".to_string(),
            redacted_entries,
        },
        entries,
        export_digest: String::new(),
        signature: None,
    };
    doc.export_digest = compute_export_digest(&doc);
    Ok(doc)
}

/// Sign an export document in place with the machine's signing key.
///
/// The signature covers the canonical document (everything but the
/// `signature` field itself, digest included), so any change to the payload
/// invalidates it.
pub fn sign_export(doc: &mut AuditExport, signing_key: &SigningKey) {
    let payload = signature_payload(doc);
    let signature = signing_key.sign(&payload);
    doc.signature = Some(ExportSignature {
        algorithm: SIGNATURE_ALGORITHM.to_string(),
        public_key: public_key_hex(signing_key),
        signature: hex::encode(signature.to_bytes()),
    });
}

/// Verify an exported document.
///
/// Two independent checks, reported separately:
///
/// - the **chain** is re-derived from the exported fields and compared with
///   the exported `entry_hash` values, and
/// - the **signature** is verified against `pinned_key` when one is given.
///
/// Without a pinned key the document can only be *self-consistent*, so the
/// verdict is [`ExportVerdict::Unanchored`] — never `Intact`.
pub fn verify_export(doc: &AuditExport, pinned_key: Option<&VerifyingKey>) -> ExportVerification {
    let mut problems = Vec::new();
    let mut notes = Vec::new();

    let digest_ok = compute_export_digest(doc) == doc.export_digest;
    if !digest_ok {
        problems.push("export_digest mismatch: exported content was modified".to_string());
    }

    // Re-derive every entry hash from the exported fields. The hash covers
    // the normalised outcome, which is exactly what the export carries.
    let mut chain_ok = true;
    let mut entries_unverifiable = 0usize;
    for (i, entry) in doc.entries.iter().enumerate() {
        let Some(stored) = entry.entry_hash.as_deref() else {
            continue; // legacy entry written before the chain existed
        };
        let expected = chain_hash(
            entry.prev_hash.as_deref(),
            &entry.timestamp,
            &entry.requesting_tool,
            &entry.provider_id,
            &entry.access_type,
            &entry.outcome,
        );
        if expected == stored {
            continue;
        }
        // A legacy entry (scheme 1) hashes the raw result, which the export
        // deliberately does not carry. It cannot be re-derived — and it
        // cannot be *proven* tampered either. The signature covers it.
        if entry.chain_scheme.unwrap_or(CHAIN_SCHEME_RAW) == CHAIN_SCHEME_RAW {
            entries_unverifiable += 1;
            continue;
        }
        chain_ok = false;
        problems.push(format!(
            "chain re-derivation failed at entry {i}: the exported fields do not reproduce the entry hash (the entry was modified)"
        ));
        break;
    }
    if entries_unverifiable > 0 {
        notes.push(format!(
            "{entries_unverifiable} entr(ies) use the legacy chain scheme: their hash covers the raw result, so it cannot be re-derived from the export; they are covered by the signature"
        ));
    }

    let mut chain_linked = true;
    for (i, pair) in doc.entries.windows(2).enumerate() {
        // Legacy entries (written before the chain existed) carry no hash
        // and cannot anchor the next one — skip the pair.
        if pair[0].entry_hash.is_none() {
            continue;
        }
        if pair[1].prev_hash.as_deref() != pair[0].entry_hash.as_deref() {
            chain_linked = false;
            problems.push(format!(
                "chain break at entry {}: prev_hash does not match the previous entry hash",
                i + 1
            ));
            break;
        }
    }

    // The head is the newest *hashed* entry — legacy entries (written before
    // the chain existed) carry no hash and cannot be the head. With nothing
    // exported (every entry fell outside the retention window) there is
    // nothing to compare, so the check is vacuous.
    let last_hashed = doc
        .entries
        .iter()
        .rev()
        .find_map(|e| e.entry_hash.as_deref());
    let chain_head_ok = match (last_hashed, &doc.source.chain_head) {
        (Some(last), Some(head)) => last == head.as_str(),
        (None, _) => true,
        (Some(_), None) => false,
    };
    if !chain_head_ok {
        problems
            .push("chain head mismatch: last entry does not match source.chain_head".to_string());
    }

    let source_chain_intact = doc.source.chain_intact;
    if !source_chain_intact {
        problems.push(format!(
            "source chain was already broken at export time{}",
            doc.source
                .chain_tampered_at
                .as_ref()
                .map(|t| format!(" (first bad entry: {t})"))
                .unwrap_or_default()
        ));
    }

    // Signature: only meaningful against a pinned key.
    let anchored = pinned_key.is_some();
    let mut signature_ok = false;
    if let Some(key) = pinned_key {
        match &doc.signature {
            None => problems.push(
                "document carries no signature: it was not produced by a signing Loom (forged)"
                    .to_string(),
            ),
            Some(sig) => {
                if sig.algorithm != SIGNATURE_ALGORITHM {
                    problems.push(format!(
                        "unsupported signature algorithm '{}': expected {SIGNATURE_ALGORITHM}",
                        sig.algorithm
                    ));
                } else if sig.public_key != hex::encode(key.to_bytes()) {
                    problems.push(
                        "signature was made by a different key than the pinned one: the document was not produced by this machine (forged)"
                            .to_string(),
                    );
                } else {
                    match hex::decode(&sig.signature)
                        .ok()
                        .and_then(|b| <[u8; 64]>::try_from(b.as_slice()).ok())
                    {
                        Some(bytes) => {
                            signature_ok = key
                                .verify_strict(&signature_payload(doc), &Signature::from_bytes(&bytes))
                                .is_ok();
                            if !signature_ok {
                                problems.push(
                                    "signature does not verify against the pinned public key: the document was modified or forged"
                                        .to_string(),
                                );
                            }
                        }
                        None => problems
                            .push("signature is not a valid ed25519 signature".to_string()),
                    }
                }
            }
        }
    } else {
        notes.push(
            "no public key was pinned: the document is only self-consistent, not proven to come from this machine"
                .to_string(),
        );
    }

    let tampered = !digest_ok
        || !chain_ok
        || !chain_linked
        || !chain_head_ok
        || !source_chain_intact
        || (anchored && !signature_ok);
    let verdict = if tampered {
        ExportVerdict::Tampered
    } else if !anchored {
        ExportVerdict::Unanchored
    } else {
        ExportVerdict::Intact
    };

    ExportVerification {
        verdict,
        entries: doc.entries.len(),
        checks: ExportChecks {
            digest_ok,
            chain_linked,
            chain_ok,
            chain_head_ok,
            source_chain_intact,
            anchored,
            signature_ok,
            entries_unverifiable,
        },
        problems,
        notes,
    }
}

/// SHA-256 over the canonical export document.
///
/// Canonical form: the document serialised with `export_digest` blanked and
/// the `signature` removed. `serde` serialises struct fields in declaration
/// order, so the bytes are stable for a given document.
pub fn compute_export_digest(doc: &AuditExport) -> String {
    use sha2::{Digest, Sha256};
    let mut blanked = doc.clone();
    blanked.export_digest = String::new();
    blanked.signature = None;
    let bytes = serde_json::to_vec(&blanked).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

/// The bytes the signature is computed over: the document without the
/// `signature` field (the digest is included, so the two are bound
/// together).
fn signature_payload(doc: &AuditExport) -> Vec<u8> {
    let mut unsigned = doc.clone();
    unsigned.signature = None;
    serde_json::to_vec(&unsigned).unwrap_or_default()
}

/// True when the entry's raw result carried text the export did not carry.
fn is_redacted(result: &str) -> bool {
    !matches!(result, "granted" | "denied")
}

/// Path of the per-machine export signing key.
pub fn signing_key_path(loom_dir: &Path) -> PathBuf {
    loom_dir.join("auth").join(SIGNING_KEY_FILE)
}

/// Load the machine's export signing key, generating it on first use.
///
/// The key is a plain Ed25519 seed stored next to the audit log, mode `0600`.
/// It is a *separate* key: it is never derived from the vault master key, and
/// producing an export never unlocks the vault.
pub fn load_or_create_signing_key(loom_dir: &Path) -> Result<SigningKey> {
    let path = signing_key_path(loom_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            let bytes = hex::decode(raw.trim())
                .with_context(|| format!("export signing key at {} is not valid hex", path.display()))?;
            let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
                anyhow::anyhow!(
                    "export signing key at {} must be 32 bytes; restore it or remove it to generate a new one",
                    path.display()
                )
            })?;
            Ok(SigningKey::from_bytes(&seed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = SigningKey::generate(&mut OsRng);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, hex::encode(key.to_bytes()))?;
            restrict_permissions(&path)?;
            Ok(key)
        }
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Hex of the public key belonging to a signing key.
pub fn public_key_hex(signing_key: &SigningKey) -> String {
    hex::encode(signing_key.verifying_key().to_bytes())
}

/// Parse a pinned public key: either 32 bytes of hex, or a path to a file
/// holding that hex.
pub fn parse_public_key(spec: &str) -> Result<VerifyingKey> {
    let trimmed = spec.trim();
    let hex_text = if looks_like_hex_key(trimmed) {
        trimmed.to_string()
    } else {
        let path = Path::new(trimmed);
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read public key file {}", path.display()))?;
        raw.trim().trim_start_matches("0x").to_string()
    };
    let bytes = hex::decode(&hex_text)
        .with_context(|| format!("public key is not valid hex: {hex_text}"))?;
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!("public key must be 32 bytes (64 hex characters), got {}", hex_text.len())
    })?;
    VerifyingKey::from_bytes(&arr).map_err(|e| anyhow::anyhow!("invalid ed25519 public key: {e}"))
}

/// True when the string looks like a 32-byte hex key rather than a path.
fn looks_like_hex_key(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Restrict a key file to the owner (`0600`).
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

/// True when the timestamp looks like an RFC3339 date-time we can order.
///
/// Entries with an unrecognisable timestamp are never dropped by retention:
/// undated evidence is kept, not silently discarded.
fn is_dated(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    bytes.len() >= 20 && bytes[4] == b'-' && bytes[10] == b'T'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditLogger;
    use loom_core::AuditEntry;
    use std::fs;

    fn entry(ts: &str, tool: &str, provider: &str, result: &str) -> AuditEntry {
        AuditEntry {
            timestamp: ts.to_string(),
            requesting_tool: tool.to_string(),
            provider_id: provider.to_string(),
            access_type: "read".to_string(),
            result: result.to_string(),
            prev_hash: None,
            entry_hash: None,
        }
    }

    fn logger_with(dir: &std::path::Path) -> AuditLogger {
        let logger = AuditLogger::new(dir.to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_write("tool-b", "provider-2", true).unwrap();
        logger.log_denied_access("tool-c", "provider-3", "acl denied");
        logger
    }

    /// Build and sign an export; return the document and the public key to
    /// pin against it.
    fn signed_export(dir: &std::path::Path) -> (AuditExport, VerifyingKey) {
        let logger = logger_with(dir);
        let key = SigningKey::generate(&mut OsRng);
        let mut doc = build_export(&logger, 365).unwrap();
        sign_export(&mut doc, &key);
        (doc, key.verifying_key())
    }

    #[test]
    fn test_export_writes_file_and_reimport_verifies_intact() {
        let dir = tempfile::TempDir::new().unwrap();
        let (doc, key) = signed_export(dir.path());

        let path = dir.path().join("audit-export.json");
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        assert!(path.exists(), "export must be written to disk");

        // Re-import: parse the file back and verify it against the pinned key.
        let raw = fs::read_to_string(&path).unwrap();
        let reimported: AuditExport = serde_json::from_str(&raw).unwrap();
        let v = verify_export(&reimported, Some(&key));
        assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
        assert!(v.checks.digest_ok);
        assert!(v.checks.chain_linked);
        assert!(v.checks.chain_ok);
        assert!(v.checks.signature_ok);
        assert_eq!(v.entries, 3);
    }

    #[test]
    fn test_export_without_pinned_key_is_unanchored_not_intact() {
        let dir = tempfile::TempDir::new().unwrap();
        let (doc, _key) = signed_export(dir.path());

        let v = verify_export(&doc, None);
        assert_eq!(v.verdict, ExportVerdict::Unanchored, "must not be intact: {v:?}");
        assert!(v.checks.digest_ok && v.checks.chain_ok);
        assert!(!v.checks.anchored);
    }

    #[test]
    fn test_tampered_export_fails_verification() {
        let dir = tempfile::TempDir::new().unwrap();
        let (doc, key) = signed_export(dir.path());

        let mut tampered = doc.clone();
        tampered.entries[0].timestamp = "1999-01-01T00:00:00Z".to_string();

        let v = verify_export(&tampered, Some(&key));
        assert_eq!(v.verdict, ExportVerdict::Tampered, "must fail: {v:?}");
        assert!(!v.checks.digest_ok, "tampering must break the digest");
    }

    #[test]
    fn test_tampered_source_block_fails_verification() {
        let dir = tempfile::TempDir::new().unwrap();
        let (doc, key) = signed_export(dir.path());
        let mut doc = doc;
        doc.source.chain_intact = false;

        let v = verify_export(&doc, Some(&key));
        assert_eq!(v.verdict, ExportVerdict::Tampered, "must fail: {v:?}");
        assert!(!v.checks.digest_ok);
    }

    #[test]
    fn test_deleted_entry_breaks_chain_linkage() {
        let dir = tempfile::TempDir::new().unwrap();
        let (doc, key) = signed_export(dir.path());
        let mut doc = doc;
        doc.entries.remove(1);

        let v = verify_export(&doc, Some(&key));
        assert_eq!(v.verdict, ExportVerdict::Tampered, "must fail: {v:?}");
        assert!(!v.checks.chain_linked);
    }

    #[test]
    fn test_export_never_contains_secret_value() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        let secret = "sk-live-SUPERSECRETVALUE";
        logger
            .log(&entry(
                &loom_core::rfc3339_now(),
                "tool-a",
                "provider-1",
                &format!("denied: {secret}"),
            ))
            .unwrap();
        logger.log_read("tool-a", "provider-1", true).unwrap();

        let doc = build_export(&logger, 365).unwrap();
        let raw = serde_json::to_string(&doc).unwrap();
        assert!(
            !raw.contains(secret),
            "secret value must never appear in the export: {raw}"
        );
        assert!(raw.contains("\"outcome\":\"denied\""));
        assert_eq!(doc.redaction.redacted_entries, 1);
        assert!(doc.entries[0].redacted);
    }

    #[test]
    fn test_retention_drops_entries_past_the_window() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        // An old entry and a fresh one.
        logger
            .log(&entry(
                "2020-01-01T00:00:00Z",
                "old-tool",
                "provider-1",
                "granted",
            ))
            .unwrap();
        logger
            .log(&entry(
                &loom_core::rfc3339_now(),
                "new-tool",
                "provider-2",
                "granted",
            ))
            .unwrap();

        let doc = build_export(&logger, 365).unwrap();
        assert_eq!(doc.retention.window_days, 365);
        assert_eq!(doc.retention.policy, "drop");
        assert_eq!(
            doc.retention.dropped_entries, 1,
            "old entry must be dropped"
        );
        assert_eq!(doc.entries.len(), 1);
        assert_eq!(doc.entries[0].requesting_tool, "new-tool");

        // A zero-day window keeps everything.
        let all = build_export(&logger, 0).unwrap();
        assert_eq!(all.retention.dropped_entries, 0);
        assert_eq!(all.entries.len(), 2);
    }

    #[test]
    fn test_export_with_everything_dropped_still_verifies() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger
            .log(&entry(
                "2020-01-01T00:00:00Z",
                "old-tool",
                "provider-1",
                "granted",
            ))
            .unwrap();

        // A one-day window drops the only entry — the export is empty, and an
        // empty export is still a valid, verifiable document.
        let doc = build_export(&logger, 1).unwrap();
        assert!(doc.entries.is_empty());
        assert_eq!(doc.retention.dropped_entries, 1);

        let key = SigningKey::generate(&mut OsRng);
        let mut doc = doc;
        sign_export(&mut doc, &key);
        let v = verify_export(&doc, Some(&key.verifying_key()));
        assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
        assert_eq!(v.entries, 0);
    }

    #[test]
    fn test_out_of_window_entry_in_the_middle_is_kept_and_still_verifies() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        // A fresh entry, then an entry with a skewed (old) timestamp written
        // later — the out-of-window entry sits in the middle of the chain.
        logger
            .log(&entry(
                &loom_core::rfc3339_now(),
                "tool-a",
                "provider-1",
                "granted",
            ))
            .unwrap();
        logger
            .log(&entry(
                "2020-01-01T00:00:00Z",
                "tool-b",
                "provider-2",
                "granted",
            ))
            .unwrap();

        // The window defines where the exported chain starts, so the out-of-
        // window entry is kept and the chain stays contiguous.
        let doc = build_export(&logger, 365).unwrap();
        assert_eq!(doc.retention.dropped_entries, 0);
        assert_eq!(doc.entries.len(), 2);
        let key = SigningKey::generate(&mut OsRng);
        let mut doc = doc;
        sign_export(&mut doc, &key);
        let v = verify_export(&doc, Some(&key.verifying_key()));
        assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
    }

    #[test]
    fn test_legacy_unchained_entry_is_tolerated() {
        let dir = tempfile::TempDir::new().unwrap();
        // A legacy log line, written before the chain existed: no hashes.
        let log_dir = dir.path().join("auth");
        std::fs::create_dir_all(&log_dir).unwrap();
        let legacy = serde_json::json!({
            "timestamp": loom_core::rfc3339_now(),
            "requesting_tool": "legacy-tool",
            "provider_id": "provider-1",
            "access_type": "read",
            "result": "granted",
        });
        std::fs::write(log_dir.join("audit.log"), format!("{legacy}\n")).unwrap();

        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_read("tool-a", "provider-2", true).unwrap();

        let doc = build_export(&logger, 365).unwrap();
        assert_eq!(doc.entries.len(), 2);
        let key = SigningKey::generate(&mut OsRng);
        let mut doc = doc;
        sign_export(&mut doc, &key);
        let v = verify_export(&doc, Some(&key.verifying_key()));
        assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
    }
}
