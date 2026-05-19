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
/// Logs are written as newline-delimited JSON to `{agents_dir}/auth/audit.log`.
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
}

impl AuditLogger {
    /// Create a new audit logger for the given agents directory.
    pub fn new(agents_dir: &str) -> Self {
        let log_path = Path::new(agents_dir).join("auth").join("audit.log");
        let log_path_str = log_path.to_string_lossy().to_string();

        // Ensure directory exists
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Check current file size
        let current_size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);

        Self {
            inner: Mutex::new(AuditInner {
                log_path: log_path_str,
                current_size,
            }),
        }
    }

    /// Rotate the log file if it exceeds the max size.
    fn rotate_if_needed(&self) -> Result<()> {
        let mut inner = self.inner.lock().expect("audit mutex poisoned");

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

    /// Write an audit entry to the log.
    fn write_entry(&self, entry: &AuditEntry) -> Result<()> {
        self.rotate_if_needed()?;

        let inner = self.inner.lock().expect("audit mutex poisoned");
        let log_path = inner.log_path.clone();

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&log_path).parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create audit log dir: {:?}", parent))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .with_context(|| format!("failed to open audit log: {log_path}"))?;

        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line).with_context(|| format!("failed to write audit log entry"))?;
        file.flush()?;

        drop(inner);
        let mut inner = self.inner.lock().expect("audit mutex poisoned");
        inner.current_size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);

        Ok(())
    }

    /// Log a credential read access.
    pub fn log_read(&self, requesting_tool: &str, provider_id: &str, success: bool) -> Result<()> {
        let entry = AuditEntry {
            timestamp: iso_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "read".to_string(),
            result: if success {
                "granted".to_string()
            } else {
                "denied".to_string()
            },
        };
        self.write_entry(&entry)
    }

    /// Log a credential write access.
    pub fn log_write(&self, requesting_tool: &str, provider_id: &str, success: bool) -> Result<()> {
        let entry = AuditEntry {
            timestamp: iso_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "write".to_string(),
            result: if success {
                "granted".to_string()
            } else {
                "denied".to_string()
            },
        };
        self.write_entry(&entry)
    }

    /// Log a denied access attempt.
    pub fn log_denied_access(&self, requesting_tool: &str, provider_id: &str, reason: &str) {
        let entry = AuditEntry {
            timestamp: iso_now(),
            requesting_tool: requesting_tool.to_string(),
            provider_id: provider_id.to_string(),
            access_type: "denied".to_string(),
            result: format!("denied: {reason}"),
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
        let inner = self.inner.lock().expect("audit mutex poisoned");
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
        let inner = self.inner.lock().expect("audit mutex poisoned");
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
        let inner = self.inner.lock().expect("audit mutex poisoned");
        inner.log_path.clone()
    }

    /// Get current log file size in bytes.
    pub fn current_size(&self) -> u64 {
        let inner = self.inner.lock().expect("audit mutex poisoned");
        inner.current_size
    }

    /// Remove all audit logs (for testing/cleanup).
    pub fn clear(&self) -> Result<()> {
        let inner = self.inner.lock().expect("audit mutex poisoned");
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
        let mut inner = self.inner.lock().expect("audit mutex poisoned");
        inner.current_size = 0;

        Ok(())
    }
}

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert Rata Die days to Gregorian date (Howard Hinnant algorithm)
fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_entry(ts: &str, tool: &str, provider: &str, atype: &str, result: &str) -> AuditEntry {
        AuditEntry {
            timestamp: ts.to_string(),
            requesting_tool: tool.to_string(),
            provider_id: provider.to_string(),
            access_type: atype.to_string(),
            result: result.to_string(),
        }
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
