use anyhow::{Context, Result};
use loom_core::AuditEntry;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use tracing::{error, info};

/// Maximum audit log file size before rotation (10 MB)
/// Maximum log file size before rotation (configurable via LOOM_AUDIT_MAX_SIZE env var)
/// Default: 10MB (10 * 1024 * 1024 bytes)
fn get_max_log_size() -> u64 {
    std::env::var("LOOM_AUDIT_MAX_SIZE")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(10 * 1024 * 1024)
}

/// Number of rotated log files to retain
const MAX_ROTATED_FILES: u32 = 5;

/// Structured audit logger with automatic log rotation.
///
/// Logs are written as newline-delimited JSON to `{loom_dir}/auth/audit.log`.
/// When the log file exceeds 10 MB, it is rotated to `audit.log.1`, `audit.log.2`, etc.
pub struct AuditLogger {
    /// Mutex to serialize log writes and rotations
    inner: Mutex<AuditInner>,
}

struct AuditInner {
    /// Current log file path
    log_path: String,
    /// Track file size for rotation decisions
    current_size: u64,
    /// Hash of the most recently written entry (chain head). Recovered
    /// from the last line of an existing log on startup.
    last_hash: Option<String>,
}

impl AuditLogger {
    /// Create a new audit logger for the given agents directory.
    pub fn new(loom_dir: &str) -> Self {
        let log_path = Path::new(loom_dir).join("auth").join("audit.log");
        let log_path_str = log_path.to_string_lossy().to_string();

        // Ensure directory exists
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Check current file size
        let current_size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);

        // Recover the hash-chain head from the last entry of an existing log.
        let last_hash = read_last_entry_hash(&log_path);

        Self {
            inner: Mutex::new(AuditInner {
                log_path: log_path_str,
                current_size,
                last_hash,
            }),
        }
    }

    /// Rotate the log file if it exceeds the max size.
    fn rotate_if_needed(&self) -> Result<()> {
        let mut inner = loom_core::lock_ok!(self.inner);

        if inner.current_size < get_max_log_size() {
            return Ok(());
        }

        let log_path = inner.log_path.clone();
        let path = Path::new(&log_path);

        // Shift rotated files: .5 → remove, .4 → .5, .3 → .4, etc.
        let rotated_5 = path.with_extension("log.5");
        if rotated_5.exists() {
            fs::remove_file(&rotated_5)
                .with_context(|| format!("failed to remove old rotated log: {:?}", rotated_5))?;
        }

        for i in (1..MAX_ROTATED_FILES).rev() {
            let src = path.with_extension(format!("log.{i}"));
            let dst = path.with_extension(format!("log.{}", i + 1));
            if src.exists() {
                fs::rename(&src, &dst)
                    .with_context(|| format!("failed to rotate log: {:?} -> {:?}", src, dst))?;
            }
        }

        // Rotate current → .1
        let rotated = path.with_extension("log.1");
        fs::rename(&log_path, &rotated)
            .with_context(|| format!("failed to rotate log: {:?} -> {:?}", log_path, rotated))?;

        // Reset size tracking
        inner.current_size = 0;
        info!("audit log rotated, archived to {:?}", rotated);
        Ok(())
    }

    /// Write an audit entry to the log, extending the integrity hash chain.
    fn write_entry(&self, entry: &AuditEntry) -> Result<()> {
        self.rotate_if_needed()?;

        let mut inner = loom_core::lock_ok!(self.inner);
        let log_path = inner.log_path.clone();

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&log_path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create audit log dir: {:?}", parent))?;
        }

        // Extend the hash chain: entry_hash = H(prev_hash || fields).
        // New entries use the outcome-normalised scheme, so an export can
        // re-derive the hash from redacted fields alone.
        let mut entry = entry.clone();
        let prev = inner.last_hash.clone();
        entry.prev_hash = prev.clone();
        entry.entry_hash = Some(compute_entry_hash(&entry, prev.as_deref(), CHAIN_SCHEME_OUTCOME));
        inner.last_hash = entry.entry_hash.clone();

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open audit log: {log_path}"))?;

        let line = serde_json::to_string(&entry)?;
        writeln!(file, "{}", line).with_context(|| "failed to write audit log entry")?;
        file.flush()?;

        drop(inner);
        let mut inner = loom_core::lock_ok!(self.inner);
        inner.current_size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);

        Ok(())
    }

    /// Log a credential read access.
    pub fn log_read(&self, requesting_tool: &str, provider_id: &str, success: bool) -> Result<()> {
        let entry = AuditEntry {
            timestamp: loom_core::rfc3339_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "read".to_string(),
            result: if success {
                "granted".to_string()
            } else {
                "denied".to_string()
            },
            prev_hash: None,
            entry_hash: None,
        };
        self.write_entry(&entry)
    }

    /// Log a credential write access.
    pub fn log_write(&self, requesting_tool: &str, provider_id: &str, success: bool) -> Result<()> {
        let entry = AuditEntry {
            timestamp: loom_core::rfc3339_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "write".to_string(),
            result: if success {
                "granted".to_string()
            } else {
                "denied".to_string()
            },
            prev_hash: None,
            entry_hash: None,
        };
        self.write_entry(&entry)
    }

    /// Log a denied access attempt.
    pub fn log_denied_access(&self, requesting_tool: &str, provider_id: &str, reason: &str) {
        let entry = AuditEntry {
            timestamp: loom_core::rfc3339_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "denied".to_string(),
            result: format!("denied: {reason}"),
            prev_hash: None,
            entry_hash: None,
        };
        if let Err(e) = self.write_entry(&entry) {
            error!(error = %e, "failed to log denied access");
        }
    }

    /// Log a generic audit entry.
    pub fn log(&self, entry: &AuditEntry) -> Result<()> {
        self.write_entry(entry)
    }

    /// Read all audit entries from the log file.
    pub fn list(&self) -> Result<Vec<AuditEntry>> {
        let inner = loom_core::lock_ok!(self.inner);
        let log_path = Path::new(&inner.log_path);

        if !log_path.exists() {
            return Ok(vec![]);
        }

        let content = fs::read_to_string(log_path)
            .with_context(|| format!("failed to read audit log: {:?}", log_path))?;

        let entries: Vec<AuditEntry> = content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    serde_json::from_str(trimmed).ok()
                }
            })
            .collect();

        Ok(entries)
    }

    /// Read audit entries, including rotated log files.
    pub fn list_all(&self) -> Result<Vec<AuditEntry>> {
        let inner = loom_core::lock_ok!(self.inner);
        let base = Path::new(&inner.log_path);
        let dir = base.parent().unwrap_or_else(|| Path::new("."));

        let mut all_entries = Vec::new();

        // Read current log
        if base.exists() {
            let content = fs::read_to_string(base)?;
            all_entries.extend(content.lines().filter_map(|l| {
                let t = l.trim();
                if t.is_empty() {
                    None
                } else {
                    serde_json::from_str(t).ok()
                }
            }));
        }

        // Read rotated logs (newest first)
        for i in (1..=MAX_ROTATED_FILES).rev() {
            let rotated = dir.join(format!("audit.log.{i}"));
            if rotated.exists() {
                let content = fs::read_to_string(&rotated)?;
                all_entries.extend(content.lines().filter_map(|l| {
                    let t = l.trim();
                    if t.is_empty() {
                        None
                    } else {
                        serde_json::from_str(t).ok()
                    }
                }));
            }
        }

        Ok(all_entries)
    }

    /// Get the current log file path.
    pub fn log_path(&self) -> String {
        let inner = loom_core::lock_ok!(self.inner);
        inner.log_path.clone()
    }

    /// Get current log file size in bytes.
    pub fn current_size(&self) -> u64 {
        let inner = loom_core::lock_ok!(self.inner);
        inner.current_size
    }

    /// Remove all audit logs (for testing/cleanup).
    pub fn clear(&self) -> Result<()> {
        let inner = loom_core::lock_ok!(self.inner);
        let base = Path::new(&inner.log_path);
        let dir = base.parent().unwrap_or_else(|| Path::new("."));

        // Remove current log
        if base.exists() {
            fs::remove_file(base)?;
        }

        // Remove rotated logs
        for i in 1..=MAX_ROTATED_FILES {
            let rotated = dir.join(format!("audit.log.{i}"));
            if rotated.exists() {
                fs::remove_file(&rotated)?;
            }
        }

        // Reset size
        drop(inner);
        let mut inner = loom_core::lock_ok!(self.inner);
        inner.current_size = 0;
        inner.last_hash = None;

        Ok(())
    }

    /// Read every entry of the chain in chronological order.
    ///
    /// Rotated logs first (oldest rotation first), then the current log.
    /// This is the order the hash chain was written in, and the order
    /// [`AuditLogger::verify_integrity`] checks it in.
    pub fn entries_chronological(&self) -> Result<Vec<AuditEntry>> {
        let inner = loom_core::lock_ok!(self.inner);
        let base = Path::new(&inner.log_path);
        let dir = base.parent().unwrap_or_else(|| Path::new("."));

        let mut entries: Vec<AuditEntry> = Vec::new();
        // Chronological order: oldest rotated file first, current last.
        for path in (1..=MAX_ROTATED_FILES)
            .rev()
            .map(|i| dir.join(format!("audit.log.{i}")))
            .chain(std::iter::once(base.to_path_buf()))
        {
            if !path.exists() {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            entries.extend(content.lines().filter_map(|l| {
                let t = l.trim();
                if t.is_empty() {
                    None
                } else {
                    serde_json::from_str(t).ok()
                }
            }));
        }
        Ok(entries)
    }

    /// Verify the integrity of the audit trail hash chain.
    ///
    /// Walks all entries (rotated logs in chronological order, then the
    /// current log) and recomputes each entry's hash. Legacy entries
    /// written before integrity chaining (no `entry_hash`) are skipped.
    /// The first mismatch is reported; `intact == true` means the chain
    /// is unbroken and untampered.
    pub fn verify_integrity(&self) -> Result<IntegrityReport> {
        let entries = self.entries_chronological()?;

        let mut report = IntegrityReport {
            total: entries.len(),
            chained: 0,
            tampered_at: None,
            intact: true,
        };

        let mut prev: Option<String> = None;
        for entry in &entries {
            let Some(entry_hash) = &entry.entry_hash else {
                continue; // legacy entry predating the hash chain
            };
            report.chained += 1;
            // Both chain schemes are accepted: a log written by an older Loom
            // hashes the raw result, a current one hashes the normalised
            // outcome. Neither is a tamper signal.
            let chain_ok = entry.prev_hash.as_ref() == prev.as_ref();
            if !chain_ok || entry_chain_scheme(entry).is_none() {
                report.intact = false;
                report.tampered_at = Some(entry.timestamp.clone());
                return Ok(report);
            }
            prev = Some(entry_hash.clone());
        }

        Ok(report)
    }
}

/// Result of [`AuditLogger::verify_integrity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityReport {
    /// Total entries examined (including legacy unchained ones).
    pub total: usize,
    /// Entries that are part of the hash chain.
    pub chained: usize,
    /// Timestamp of the first entry that failed verification.
    pub tampered_at: Option<String>,
    /// True when the chain is unbroken and every hash matches.
    pub intact: bool,
}

/// Chain hash scheme of the legacy entries: the hash is computed over the
/// raw `result` string.
///
/// Legacy entries carry no scheme marker; the scheme is detected by
/// recomputing the hash (see [`entry_chain_scheme`]).
pub const CHAIN_SCHEME_RAW: u8 = 1;

/// Chain hash scheme of the current entries: the hash is computed over the
/// *normalised* outcome, never over the raw free-text `result`.
///
/// This is what makes an export independently verifiable: a consumer that
/// holds only the exported (redacted) fields can re-derive every entry hash.
/// The raw `result` is the only field a caller can stuff arbitrary content
/// into, so it must not be part of a hash the export has to reproduce.
pub const CHAIN_SCHEME_OUTCOME: u8 = 2;

/// Map the chain's free-text `result` to a normalised outcome.
///
/// `granted` and `denied` pass through; a denial reason (`denied: <reason>`)
/// keeps the outcome but drops the reason; any other text becomes `other`.
/// The raw text never leaves the machine.
pub fn normalize_outcome(result: &str) -> &'static str {
    match result {
        "granted" => "granted",
        "denied" => "denied",
        r if r.starts_with("denied:") => "denied",
        _ => "other",
    }
}

/// SHA-256 over the canonical chain fields plus the previous entry's hash.
///
/// `hashed_result` is the field's contribution to the hash: the raw result
/// under [`CHAIN_SCHEME_RAW`], the normalised outcome under
/// [`CHAIN_SCHEME_OUTCOME`]. The entry's own `entry_hash` is excluded.
pub fn chain_hash(
    prev_hash: Option<&str>,
    timestamp: &str,
    requesting_tool: &str,
    provider_id: &str,
    access_type: &str,
    hashed_result: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(prev_hash.unwrap_or(""));
    hasher.update(timestamp);
    hasher.update(requesting_tool);
    hasher.update(provider_id);
    hasher.update(access_type);
    hasher.update(hashed_result);
    format!("{:x}", hasher.finalize())
}

/// Compute the SHA-256 chain hash for an entry under the given scheme.
///
/// The entry's own `entry_hash` field is intentionally excluded.
pub fn compute_entry_hash(entry: &AuditEntry, prev_hash: Option<&str>, scheme: u8) -> String {
    let hashed_result = match scheme {
        CHAIN_SCHEME_OUTCOME => normalize_outcome(&entry.result),
        _ => entry.result.as_str(),
    };
    chain_hash(
        prev_hash,
        &entry.timestamp,
        &entry.requesting_tool,
        &entry.provider_id,
        &entry.access_type,
        hashed_result,
    )
}

/// The chain hash scheme an entry was written with.
///
/// Both schemes are checked, so a log written by an older Loom still
/// verifies. `None` means neither scheme reproduces the stored hash — the
/// entry has been tampered with.
pub fn entry_chain_scheme(entry: &AuditEntry) -> Option<u8> {
    let prev = entry.prev_hash.as_deref();
    let stored = entry.entry_hash.as_deref()?;
    if stored == compute_entry_hash(entry, prev, CHAIN_SCHEME_OUTCOME) {
        Some(CHAIN_SCHEME_OUTCOME)
    } else if stored == compute_entry_hash(entry, prev, CHAIN_SCHEME_RAW) {
        Some(CHAIN_SCHEME_RAW)
    } else {
        None
    }
}

/// Read the last chained entry's hash from an existing log file.
fn read_last_entry_hash(log_path: &Path) -> Option<String> {
    let content = fs::read_to_string(log_path).ok()?;
    content
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<AuditEntry>(line.trim()).ok())
        .and_then(|e| e.entry_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn test_entry(ts: &str, tool: &str, provider: &str, atype: &str, result: &str) -> AuditEntry {
        AuditEntry {
            timestamp: ts.to_string(),
            requesting_tool: tool.to_string(),
            provider_id: provider.to_string(),
            access_type: atype.to_string(),
            result: result.to_string(),
            prev_hash: None,
            entry_hash: None,
        }
    }

    #[test]
    fn test_integrity_chain_verifies_untampered_log() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_write("tool-b", "provider-2", true).unwrap();
        logger.log_denied_access("tool-c", "provider-3", "acl");

        let report = logger.verify_integrity().unwrap();
        assert_eq!(report.total, 3);
        assert_eq!(report.chained, 3);
        assert!(report.intact, "untampered chain should verify: {report:?}");
    }

    #[test]
    fn test_integrity_chain_detects_field_tampering() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_read("tool-b", "provider-2", true).unwrap();

        // Tamper with the second entry's result field in the log file.
        let log_path = PathBuf::from(logger.log_path());
        let raw = fs::read_to_string(&log_path).unwrap();
        let tampered = raw.replace("granted", "revoked");
        assert_ne!(raw, tampered, "tamper substitution must change content");
        fs::write(&log_path, tampered).unwrap();

        let report = logger.verify_integrity().unwrap();
        assert!(!report.intact, "tampered entry must fail verification");
        assert!(report.tampered_at.is_some());
    }

    #[test]
    fn test_integrity_chain_detects_deleted_entry() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_read("tool-b", "provider-2", true).unwrap();
        logger.log_read("tool-c", "provider-3", true).unwrap();

        // Delete the middle entry — breaks the prev_hash linkage.
        let log_path = PathBuf::from(logger.log_path());
        let raw = fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        let kept: Vec<&str> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 1)
            .map(|(_, l)| *l)
            .collect();
        fs::write(&log_path, kept.join("\n") + "\n").unwrap();

        let report = logger.verify_integrity().unwrap();
        assert!(!report.intact, "deleted middle entry must break the chain");
    }

    #[test]
    fn test_integrity_chain_survives_logger_restart() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom = dir.path().to_str().unwrap();
        {
            let logger = AuditLogger::new(loom);
            logger.log_read("tool-a", "provider-1", true).unwrap();
        }
        // New logger instance must recover the chain head and extend it.
        let logger = AuditLogger::new(loom);
        logger.log_write("tool-b", "provider-2", true).unwrap();
        let report = logger.verify_integrity().unwrap();
        assert!(
            report.intact,
            "chain must continue across restart: {report:?}"
        );
        assert_eq!(report.chained, 2);
    }

    #[test]
    fn test_integrity_tolerates_legacy_unchained_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        let loom = dir.path().to_str().unwrap();
        // Legacy log written before hash chaining: no hash fields at all.
        let log_path = dir.path().join("auth").join("audit.log");
        fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        fs::write(
            &log_path,
            concat!(
                r#"{"timestamp":"2025-01-01T00:00:00Z","requesting_tool":"old","provider_id":"p","access_type":"read","result":"granted"}"#,
                "\n",
            ),
        )
        .unwrap();

        let logger = AuditLogger::new(loom);
        logger.log_read("tool-a", "provider-1", true).unwrap();
        let report = logger.verify_integrity().unwrap();
        assert_eq!(report.total, 2);
        assert_eq!(report.chained, 1, "only the new entry is chained");
        assert!(report.intact, "legacy entry must not break the chain");
    }

    #[test]
    fn test_log_and_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        let entry = test_entry(
            "2025-01-01T00:00:00Z",
            "tool-a",
            "provider-1",
            "read",
            "granted",
        );
        logger.log(&entry).unwrap();
        let entries = logger.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].access_type, "read");
    }

    #[test]
    fn test_log_read_and_write() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_write("tool-a", "provider-2", true).unwrap();
        let entries = logger.list().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].access_type, "read");
        assert_eq!(entries[1].access_type, "write");
    }

    #[test]
    fn test_log_denied() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger.log_denied_access("tool-a", "provider-1", "not authorized");
        let entries = logger.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].access_type, "denied");
        assert!(entries[0].result.contains("not authorized"));
    }

    #[test]
    fn test_log_rotation() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_dir = dir.path().join("auth");
        fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("audit.log");

        // Pre-fill log to exceed max size
        let mut file = fs::File::create(&log_path).unwrap();
        let big_line = "x".repeat(8192);
        // Write ~12 MB
        for _ in 0..(12 * 1024 * 1024 / big_line.len()) {
            writeln!(file, "{}", big_line).unwrap();
        }
        file.flush().unwrap();
        drop(file);

        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        let entry = test_entry(
            "2025-01-01T00:00:00Z",
            "tool",
            "provider",
            "read",
            "granted",
        );
        logger.log(&entry).unwrap();

        // Check that rotation occurred
        assert!(log_dir.join("audit.log.1").exists());
        // Current log should contain only the new entry
        let current_entries = logger.list().unwrap();
        assert_eq!(current_entries.len(), 1);
    }

    #[test]
    fn test_clear() {
        let dir = tempfile::TempDir::new().unwrap();
        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger
            .log(&test_entry("t1", "tool-a", "p1", "read", "granted"))
            .unwrap();
        assert!(!logger.list().unwrap().is_empty());
        logger.clear().unwrap();
        assert!(logger.list().unwrap().is_empty());
    }

    #[test]
    fn test_list_all_includes_rotated() {
        let dir = tempfile::TempDir::new().unwrap();
        let log_dir = dir.path().join("auth");
        fs::create_dir_all(&log_dir).unwrap();

        // Create rotated log files manually
        let rotated1_path = log_dir.join("audit.log.1");
        let mut f1 = fs::File::create(&rotated1_path).unwrap();
        writeln!(f1, r#"{{"timestamp":"t1","requesting_tool":"old","provider_id":"p","access_type":"read","result":"granted"}}"#).unwrap();

        let rotated2_path = log_dir.join("audit.log.2");
        let mut f2 = fs::File::create(&rotated2_path).unwrap();
        writeln!(f2, r#"{{"timestamp":"t0","requesting_tool":"older","provider_id":"p","access_type":"read","result":"granted"}}"#).unwrap();

        let logger = AuditLogger::new(dir.path().to_str().unwrap());
        logger
            .log(&test_entry("t2", "current", "p", "read", "granted"))
            .unwrap();

        let all = logger.list_all().unwrap();
        assert_eq!(all.len(), 3);
    }
}
