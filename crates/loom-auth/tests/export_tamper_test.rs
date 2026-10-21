//! Tamper-evidence tests for the audit export.
//!
//! These are the review's own attacks (docs/plan/LOOM_PILLAR2_REVIEW.md,
//! finding F1/F2), written as real tests:
//!
//! 1. a field tamper fails verification,
//! 2. a whole-document forgery — a self-consistent chain built from scratch
//!    with no audit log behind it — fails against a pinned key,
//! 3. a re-hashed tamper (the attacker recomputes the digest) fails,
//! 4. a correct export verifies as INTACT against its own machine's pinned
//!    public key,
//! 5. a correct export with no pinned key reports UNANCHORED, never INTACT.

use loom_auth::audit::{chain_hash, AuditLogger, CHAIN_SCHEME_OUTCOME};
use loom_auth::export::{
    build_export, compute_export_digest, sign_export, verify_export, AuditExport, ExportVerdict,
};
use loom_core::AuditEntry;
use tempfile::TempDir;

fn seed_chain(loom_dir: &std::path::Path) {
    let logger = AuditLogger::new(loom_dir.to_str().unwrap());
    logger.log_read("tool-a", "provider-1", true).unwrap();
    logger.log_write("tool-b", "provider-2", true).unwrap();
    logger.log_denied_access("tool-c", "provider-3", "acl denied");
}

/// Build a signed export of a seeded chain, plus the public key to pin.
fn signed_export(loom_dir: &std::path::Path) -> (AuditExport, String) {
    let logger = AuditLogger::new(loom_dir.to_str().unwrap());
    let mut doc = build_export(&logger, 365).unwrap();
    let key = loom_auth::export::load_or_create_signing_key(loom_dir).unwrap();
    let pubkey = loom_auth::export::public_key_hex(&key);
    sign_export(&mut doc, &key);
    (doc, pubkey)
}

/// A forged export: a complete document with a self-consistent chain, built
/// from scratch. There is no audit log behind it and it was never signed by
/// the machine.
fn forged_export() -> AuditExport {
    let mut entries = Vec::new();
    let mut prev: Option<String> = None;
    for i in 0..3 {
        let timestamp = format!("2026-01-0{}T00:00:00Z", i + 1);
        let outcome = "granted";
        let entry_hash = chain_hash(
            prev.as_deref(),
            &timestamp,
            "forged-tool",
            "forged-provider",
            "read",
            outcome,
        );
        entries.push(loom_auth::export::ExportEntry {
            timestamp,
            requesting_tool: "forged-tool".to_string(),
            provider_id: "forged-provider".to_string(),
            access_type: "read".to_string(),
            outcome: outcome.to_string(),
            redacted: false,
            prev_hash: prev.clone(),
            entry_hash: Some(entry_hash.clone()),
            chain_scheme: Some(CHAIN_SCHEME_OUTCOME),
        });
        prev = Some(entry_hash);
    }

    let mut doc = AuditExport {
        schema_version: 1,
        kind: "loom.audit.export".to_string(),
        exported_at: "2026-01-04T00:00:00Z".to_string(),
        source: loom_auth::export::ExportSource {
            log_path: "/nonexistent/auth/audit.log".to_string(),
            chain_total: 3,
            chain_chained: 3,
            chain_intact: true,
            chain_tampered_at: None,
            chain_head: prev.clone(),
        },
        retention: loom_auth::export::ExportRetention {
            window_days: 365,
            policy: "drop".to_string(),
            dropped_entries: 0,
            oldest_included: Some("2026-01-01T00:00:00Z".to_string()),
            newest_included: Some("2026-01-03T00:00:00Z".to_string()),
        },
        redaction: loom_auth::export::ExportRedaction {
            policy: "result-normalised".to_string(),
            redacted_entries: 0,
        },
        entries,
        export_digest: String::new(),
        signature: None,
    };
    doc.export_digest = compute_export_digest(&doc);
    doc
}

#[test]
fn attack_1_field_tamper_fails_verification() {
    let dir = TempDir::new().unwrap();
    seed_chain(dir.path());
    let (doc, pubkey) = signed_export(dir.path());
    let key = loom_auth::export::parse_public_key(&pubkey).unwrap();

    let mut tampered = doc.clone();
    tampered.entries[0].provider_id = "provider-9".to_string();

    let v = verify_export(&tampered, Some(&key));
    assert_eq!(
        v.verdict,
        ExportVerdict::Tampered,
        "field tamper must not verify: {v:?}"
    );
    assert!(!v.checks.digest_ok);
    assert!(!v.checks.chain_ok);
    assert!(
        v.problems
            .iter()
            .any(|p| p.contains("chain re-derivation failed")),
        "the failing check must be named: {v:?}"
    );
}

#[test]
fn attack_2_whole_document_forgery_fails_against_pinned_key() {
    let dir = TempDir::new().unwrap();
    seed_chain(dir.path());
    let (_real, pubkey) = signed_export(dir.path());
    let key = loom_auth::export::parse_public_key(&pubkey).unwrap();

    // The forged document is internally perfect: its chain re-derives and its
    // digest matches. It is still not from this machine.
    let forged = forged_export();
    let v = verify_export(&forged, Some(&key));
    assert_eq!(
        v.verdict,
        ExportVerdict::Tampered,
        "forgery must not verify: {v:?}"
    );
    assert!(v.checks.digest_ok, "the forged digest is consistent");
    assert!(v.checks.chain_ok, "the forged chain is consistent");
    assert!(!v.checks.signature_ok);
    assert!(
        v.problems.iter().any(|p| p.contains("forged")),
        "the failing check must be named: {v:?}"
    );
}

#[test]
fn attack_3_rehashed_tamper_fails_verification() {
    let dir = TempDir::new().unwrap();
    seed_chain(dir.path());
    let (doc, pubkey) = signed_export(dir.path());
    let key = loom_auth::export::parse_public_key(&pubkey).unwrap();

    // The attacker edits a field AND recomputes the digest, exactly as the
    // unkeyed checksum invites them to.
    let mut tampered = doc.clone();
    tampered.entries[1].requesting_tool = "attacker-tool".to_string();
    tampered.export_digest = compute_export_digest(&tampered);
    assert_ne!(
        tampered.export_digest, doc.export_digest,
        "the attacker recomputed the digest over the tampered document"
    );
    assert_eq!(
        tampered.export_digest,
        compute_export_digest(&tampered),
        "the digest must match the tampered document"
    );

    let v = verify_export(&tampered, Some(&key));
    assert_eq!(
        v.verdict,
        ExportVerdict::Tampered,
        "re-hashed tamper must not verify: {v:?}"
    );
    assert!(v.checks.digest_ok, "the attacker recomputed the digest");
    assert!(!v.checks.chain_ok, "but the chain no longer re-derives");
    assert!(!v.checks.signature_ok, "and the signature does not verify");
}

#[test]
fn correct_export_verifies_intact_against_pinned_key() {
    let dir = TempDir::new().unwrap();
    seed_chain(dir.path());
    let (doc, pubkey) = signed_export(dir.path());
    let key = loom_auth::export::parse_public_key(&pubkey).unwrap();

    let v = verify_export(&doc, Some(&key));
    assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
    assert!(v.checks.digest_ok);
    assert!(v.checks.chain_ok);
    assert!(v.checks.chain_linked);
    assert!(v.checks.chain_head_ok);
    assert!(v.checks.signature_ok);
    assert_eq!(v.entries, 3);
    assert!(v.problems.is_empty());
}

#[test]
fn correct_export_without_pinned_key_is_unanchored_not_intact() {
    let dir = TempDir::new().unwrap();
    seed_chain(dir.path());
    let (doc, _pubkey) = signed_export(dir.path());

    let v = verify_export(&doc, None);
    assert_eq!(
        v.verdict,
        ExportVerdict::Unanchored,
        "must not be intact: {v:?}"
    );
    assert!(v.checks.digest_ok);
    assert!(v.checks.chain_ok);
    assert!(!v.checks.anchored);
    assert!(
        v.notes
            .iter()
            .any(|n| n.contains("self-consistent") && n.contains("not proven")),
        "the reason must be stated: {v:?}"
    );
}

/// A log written by the OLD scheme (the hash covers the raw result) must
/// still verify: an upgrading user must not see their chain as tampered.
#[test]
fn legacy_scheme_log_still_verifies_intact() {
    let dir = TempDir::new().unwrap();
    let auth_dir = dir.path().join("auth");
    std::fs::create_dir_all(&auth_dir).unwrap();

    // Write a chain with the legacy hasher: raw result in the preimage.
    let mut lines = Vec::new();
    let mut prev: Option<String> = None;
    for (i, (tool, result)) in [("tool-a", "granted"), ("tool-b", "denied: acl denied")]
        .iter()
        .enumerate()
    {
        let timestamp = format!("2026-02-0{}T00:00:00Z", i + 1);
        let hash = chain_hash(
            prev.as_deref(),
            &timestamp,
            tool,
            "provider-1",
            "read",
            result,
        );
        let entry = AuditEntry {
            timestamp,
            requesting_tool: tool.to_string(),
            provider_id: "provider-1".to_string(),
            access_type: "read".to_string(),
            result: result.to_string(),
            prev_hash: prev.clone(),
            entry_hash: Some(hash.clone()),
        };
        lines.push(serde_json::to_string(&entry).unwrap());
        prev = Some(hash);
    }
    std::fs::write(
        auth_dir.join("audit.log"),
        format!("{}\n", lines.join("\n")),
    )
    .unwrap();

    // The local chain verifies: the old scheme is still accepted.
    let logger = AuditLogger::new(dir.path().to_str().unwrap());
    let report = logger.verify_integrity().unwrap();
    assert!(
        report.intact,
        "a log written by the old scheme must still verify: {report:?}"
    );

    // And the export records the legacy scheme for the entries whose hash
    // covered free text, so a consumer knows they cannot be re-derived from
    // the export. (An entry whose raw result was already a normalised value
    // hashes the same under both schemes — it stays re-derivable.)
    let doc = build_export(&logger, 365).unwrap();
    assert_eq!(doc.entries.len(), 2);
    assert_eq!(
        doc.entries[1].chain_scheme,
        Some(1),
        "the free-text entry must be marked as legacy: {:?}",
        doc.entries
    );

    // Signed against the machine key, the export still verifies INTACT: the
    // signature covers what the chain cannot.
    let mut doc = doc;
    let key = loom_auth::export::load_or_create_signing_key(dir.path()).unwrap();
    let pubkey = loom_auth::export::public_key_hex(&key);
    sign_export(&mut doc, &key);
    let v = verify_export(
        &doc,
        Some(&loom_auth::export::parse_public_key(&pubkey).unwrap()),
    );
    assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
    assert_eq!(
        v.checks.entries_unverifiable, 1,
        "only the free-text entry is unverifiable: {v:?}"
    );
    assert!(
        v.notes.iter().any(|n| n.contains("legacy")),
        "the caveat must be stated: {v:?}"
    );
}

/// The redaction rule and the new chain scheme must hold together: the raw
/// result never appears in the export bytes, and the chain still re-derives.
#[test]
fn raw_result_never_appears_in_export_bytes() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().to_str().unwrap());
    let secret = "sk-live-SUPERSECRET-VALUE";
    logger.log_denied_access("tool-a", "provider-1", secret);
    logger.log_read("tool-b", "provider-2", true).unwrap();

    let mut doc = build_export(&logger, 365).unwrap();
    let raw = serde_json::to_string(&doc).unwrap();
    assert!(
        !raw.contains(secret),
        "the raw result must never reach the export: {raw}"
    );
    assert!(!raw.contains("acl denied") && !raw.contains("denied:"));

    // The chain still re-derives from the redacted fields.
    let key = loom_auth::export::load_or_create_signing_key(dir.path()).unwrap();
    let pubkey = loom_auth::export::public_key_hex(&key);
    sign_export(&mut doc, &key);
    let v = verify_export(
        &doc,
        Some(&loom_auth::export::parse_public_key(&pubkey).unwrap()),
    );
    assert_eq!(v.verdict, ExportVerdict::Intact, "must verify: {v:?}");
    assert!(
        v.checks.chain_ok,
        "the chain must re-derive from redacted fields"
    );
}
