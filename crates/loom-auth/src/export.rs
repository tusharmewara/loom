//! Portable, verifiable export of the audit hash chain.
//!
//! The export is the compliance artifact: a JSON document carrying the chain
//! entries (access metadata only — never credential material), the chain's
//! own integrity report, and a digest over the exported document.
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
//!       "entry_hash": "9f2c…"
//!     }
//!   ],
//!   "export_digest": "sha256 hex"
//! }
//! ```
//!
//! # Verification
//!
//! Verifying an exported file is three checks, all of which
//! [`verify_export`] performs:
//!
//! 1. **Digest** — `export_digest` is SHA-256 over the canonical document
//!    (the document with `export_digest` blanked). Any edit to any exported
//!    field breaks it.
//! 2. **Chain linkage** — every entry's `prev_hash` equals the previous
//!    entry's `entry_hash`, and the last entry's `entry_hash` equals the
//!    recorded `source.chain_head`.
//! 3. **Source chain** — `source.chain_intact` records the result of the
//!    chain's own verification at export time.
//!
//! The chain's own hashes are never recomputed here: they are carried
//! verbatim from the chain and bound into the digest, so the digest is
//! produced from the same chain the entries came from.

use crate::audit::AuditLogger;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Schema version of the export document.
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

/// Document discriminator, so a consumer can recognise the file.
pub const EXPORT_KIND: &str = "loom.audit.export";

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
}

/// Provenance and integrity report of the source chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSource {
    /// Path of the audit log the chain was read from.
    pub log_path: String,
    /// Entries in the source chain.
    pub chain_total: usize,
    /// Entries that are part of the hash chain.
    pub chain_chained: usize,
    /// Whether the source chain was intact at export time.
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
    /// What happens to entries past the window: `drop` (they are left out of
    /// this export; the on-disk chain is never rewritten).
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
}

/// Result of verifying an exported document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportVerification {
    /// True when every check passed.
    pub intact: bool,
    /// The recomputed digest matched `export_digest`.
    pub digest_ok: bool,
    /// Every entry linked to the previous one.
    pub chain_linked: bool,
    /// The last entry matched the recorded chain head.
    pub chain_head_ok: bool,
    /// The source chain was intact when the export was taken.
    pub source_chain_intact: bool,
    /// Number of entries in the document.
    pub entries: usize,
    /// Human-readable description of every problem found.
    pub problems: Vec<String>,
}

/// Build an export of the audit chain, applying the retention window.
///
/// `retention_days` is the configured window: entries older than the window
/// are **dropped from the export**. The on-disk chain is never rewritten —
/// dropping entries from the head of the chain would break the hash linkage
/// that [`AuditLogger::verify_integrity`] checks. Disk is bounded instead by
/// the logger's existing size-based rotation.
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

    let mut entries = Vec::new();
    let mut dropped_entries = 0usize;
    let mut redacted_entries = 0usize;

    for entry in &all {
        if let Some(cutoff) = &cutoff {
            if is_dated(&entry.timestamp) && entry.timestamp.as_str() < cutoff.as_str() {
                dropped_entries += 1;
                continue;
            }
        }
        let (outcome, redacted) = normalize_outcome(&entry.result);
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
    };
    doc.export_digest = export_digest(&doc);
    Ok(doc)
}

/// Verify an exported document: digest, chain linkage, and chain head.
///
/// The digest detects any edit to the exported content; the linkage check
/// detects dropped or reordered entries; the head check ties the document
/// to the chain it was taken from.
pub fn verify_export(doc: &AuditExport) -> ExportVerification {
    let mut problems = Vec::new();

    let digest_ok = export_digest(doc) == doc.export_digest;
    if !digest_ok {
        problems.push("export_digest mismatch: exported content was modified".to_string());
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

    ExportVerification {
        intact: digest_ok && chain_linked && chain_head_ok && source_chain_intact,
        digest_ok,
        chain_linked,
        chain_head_ok,
        source_chain_intact,
        entries: doc.entries.len(),
        problems,
    }
}

/// SHA-256 over the canonical export document.
///
/// Canonical form: the document serialised with `export_digest` blanked.
/// `serde` serialises struct fields in declaration order, so the bytes are
/// stable for a given document. The chain's own `entry_hash` values are part
/// of the serialised document, so the digest is produced from the same chain
/// the entries came from.
fn export_digest(doc: &AuditExport) -> String {
    use sha2::{Digest, Sha256};
    let mut blanked = doc.clone();
    blanked.export_digest = String::new();
    let bytes = serde_json::to_vec(&blanked).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

/// Map the chain's free-text `result` to a normalised outcome.
///
/// The raw text is never exported: it is the only field a caller can stuff
/// arbitrary content into. `granted` and `denied` pass through; a denial
/// reason (`denied: <reason>`) keeps the outcome but drops the reason; any
/// other text becomes `other`. The second element of the tuple says whether
/// free text was dropped.
fn normalize_outcome(result: &str) -> (&'static str, bool) {
    match result {
        "granted" => ("granted", false),
        "denied" => ("denied", false),
        r if r.starts_with("denied:") => ("denied", true),
        _ => ("other", true),
    }
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

    #[test]
    fn test_export_writes_file_and_reimport_verifies_intact() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = logger_with(dir.path());

        let doc = build_export(&logger, 365).unwrap();
        let path = dir.path().join("audit-export.json");
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        assert!(path.exists(), "export must be written to disk");

        // Re-import: parse the file back and verify it.
        let raw = fs::read_to_string(&path).unwrap();
        let reimported: AuditExport = serde_json::from_str(&raw).unwrap();
        let v = verify_export(&reimported);
        assert!(v.intact, "re-imported export must verify: {v:?}");
        assert!(v.digest_ok);
        assert!(v.chain_linked);
        assert_eq!(v.entries, 3);
    }

    #[test]
    fn test_tampered_export_fails_verification() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = logger_with(dir.path());
        let doc = build_export(&logger, 365).unwrap();

        let mut tampered = doc.clone();
        tampered.entries[0].timestamp = "1999-01-01T00:00:00Z".to_string();

        let v = verify_export(&tampered);
        assert!(!v.intact, "tampered export must fail verification");
        assert!(!v.digest_ok, "tampering must break the digest");
    }

    #[test]
    fn test_tampered_source_block_fails_verification() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = logger_with(dir.path());
        let mut doc = build_export(&logger, 365).unwrap();
        doc.source.chain_intact = false;

        let v = verify_export(&doc);
        assert!(!v.intact, "tampered source block must fail verification");
        assert!(!v.digest_ok);
    }

    #[test]
    fn test_deleted_entry_breaks_chain_linkage() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = logger_with(dir.path());
        let mut doc = build_export(&logger, 365).unwrap();
        doc.entries.remove(1);

        let v = verify_export(&doc);
        assert!(!v.intact, "dropped entry must break the chain");
        assert!(!v.chain_linked);
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

        let v = verify_export(&doc);
        assert!(v.intact, "empty export must still verify: {v:?}");
        assert_eq!(v.entries, 0);
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
        let v = verify_export(&doc);
        assert!(v.intact, "legacy entry must not break verification: {v:?}");
    }
}
