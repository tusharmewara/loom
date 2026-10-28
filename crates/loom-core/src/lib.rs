pub mod config;
pub mod constants;
pub mod license;
pub mod lock;
pub mod org;
pub mod plugin_registry;
pub mod types;

pub use config::*;
pub use constants::*;
pub use license::*;
pub use types::*;

use anyhow::Result;

/// The user's home directory, tilde-expanded. Falls back to the passwd entry
/// when `HOME` is unset (works in launchd/systemd service contexts).
pub fn home_dir() -> String {
    shellexpand::tilde("~").to_string()
}

/// The `~/.loom` ecosystem directory (AGENTS_DIR under the user's home).
///
/// `LOOM_AGENTS_DIR` overrides it, so the CLI and the daemon always agree on
/// which directory they are talking to. The env var is the single source of
/// truth: `loom init-shell` exports it, every spawned child inherits it, and
/// the launchd/systemd unit sets it explicitly.
pub fn loom_dir() -> String {
    resolve_loom_dir(std::env::var("LOOM_AGENTS_DIR").ok().as_deref())
}

/// The resolution behind [`loom_dir`], split out so it can be tested without
/// mutating the process environment.
fn resolve_loom_dir(env_value: Option<&str>) -> String {
    match env_value {
        Some(dir) if !dir.is_empty() => dir.to_string(),
        _ => format!("{}/{}", home_dir(), AGENTS_DIR),
    }
}

/// Locate an executable on `PATH` (or return `name` itself if it is an
/// absolute path that exists). Checks the executable bit on unix.
pub fn find_binary(name: &str) -> Result<std::path::PathBuf> {
    let path = std::path::Path::new(name);
    if path.is_absolute() && path.exists() {
        return Ok(path.to_path_buf());
    }
    let path_env = std::env::var("PATH").map_err(|_| anyhow::anyhow!("PATH not set"))?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if candidate.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&candidate) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Ok(candidate);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("Binary '{name}' not found in PATH")
}

/// Current time as an RFC3339 timestamp (UTC, `Z` suffix, second precision).
/// The canonical timestamp format across the ecosystem.
pub fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// The RFC3339 timestamp `days` days before now (UTC, `Z` suffix).
///
/// Used as the retention cutoff for the audit chain: entries whose
/// timestamp is older than this are dropped from an export.
pub fn rfc3339_days_ago(days: u32) -> String {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    cutoff.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// SQLite bootstrap shared by the ecosystem's stores: open a connection at
/// `path` with the standard pragmas (WAL journal, NORMAL synchronous — the
/// durable-but-fast tradeoff all registries want). Foreign keys stay a
/// per-crate policy: enabling them changes insert semantics, so callers that
/// want FKs add `PRAGMA foreign_keys=ON` themselves.
pub mod db {
    /// Open `path` with WAL + synchronous=NORMAL. Creates the file if absent;
    /// parent directories are the caller's job (each store already knows its
    /// own layout).
    pub fn open(path: &std::path::Path) -> crate::Result<rusqlite::Connection> {
        use anyhow::Context;
        let conn = rusqlite::Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }
}

/// Lock a `std::sync::Mutex` without propagating poisoning. These locks guard
/// plain data (SQLite conns, maps, counters) whose state stays usable after a
/// panic in the lock holder; with plain `.lock().unwrap()` one panic poisons
/// the mutex and cascades into daemon-wide lock failures.
#[macro_export]
macro_rules! lock_ok {
    ($mutex:expr) => {
        $mutex.lock().unwrap_or_else(|e| e.into_inner())
    };
}

#[cfg(test)]
mod db_tests {
    #[test]
    fn opens_with_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let conn = crate::db::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t(x)").unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }
}

#[cfg(test)]
mod loom_dir_tests {
    use super::resolve_loom_dir;

    #[test]
    fn env_var_overrides_the_default() {
        assert_eq!(resolve_loom_dir(Some("/tmp/custom")), "/tmp/custom");
    }

    #[test]
    fn an_empty_env_var_falls_back_to_the_default() {
        assert!(resolve_loom_dir(Some("")).ends_with("/.loom"));
        assert!(resolve_loom_dir(None).ends_with("/.loom"));
    }
}

#[cfg(test)]
mod lock_ok_tests {
    #[test]
    fn recovers_after_poisoning() {
        let m = std::sync::Mutex::new(0u8);
        let poisoned = std::sync::Arc::new(m);
        let p2 = poisoned.clone();
        let _ = std::thread::spawn(move || {
            let _g = p2.lock().unwrap();
            panic!("poison me");
        })
        .join();
        let mut g = crate::lock_ok!(poisoned);
        *g = 7;
        assert_eq!(*g, 7);
    }
}

/// Health status reported by all crates, enabling unified monitoring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthStatus {
    pub crate_name: &'static str,
    pub version: &'static str,
    pub uptime_seconds: u64,
    pub last_error: Option<String>,
    pub performance_counters: std::collections::HashMap<String, u64>,
}

/// Every crate implements this trait for daemon health monitoring.
pub trait HealthCheck {
    fn health(&self) -> Result<HealthStatus>;
}

/// SQLite-backed [`RunStore`] persisting run results to
/// `<loom_dir>/control_plane/runs.db`.
pub struct SqliteRunStore {
    conn: std::sync::Mutex<rusqlite::Connection>,
}

impl SqliteRunStore {
    pub fn open(loom_dir: &str) -> anyhow::Result<Self> {
        use anyhow::Context;
        let dir = std::path::Path::new(loom_dir).join("control_plane");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create control plane dir at {}", dir.display()))?;
        let path = dir.join("runs.db");
        let conn = crate::db::open(&path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS runs (run_id TEXT PRIMARY KEY, tool TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', output TEXT, error TEXT, exit_code INTEGER, created_at TEXT NOT NULL, completed_at TEXT);",
        )?;
        Ok(Self {
            conn: std::sync::Mutex::new(conn),
        })
    }

    fn save_inner(&self, conn: &rusqlite::Connection, result: &RunResult) -> anyhow::Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO runs (run_id, tool, status, output, error, exit_code, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                result.run_id,
                result.tool,
                format!("{:?}", result.status).to_lowercase(),
                result.output,
                result.error,
                result.exit_code,
                result.created_at,
                result.completed_at,
            ],
        )?;
        Ok(())
    }
}

impl RunStore for SqliteRunStore {
    fn save(&self, result: &RunResult) -> anyhow::Result<()> {
        let conn = lock_ok!(self.conn);
        self.save_inner(&conn, result)
    }

    fn update(&self, _run_id: &str, result: &RunResult) -> anyhow::Result<()> {
        let conn = lock_ok!(self.conn);
        self.save_inner(&conn, result)
    }

    fn get(&self, run_id: &str) -> anyhow::Result<Option<RunResult>> {
        let conn = lock_ok!(self.conn);
        match conn.query_row(
            "SELECT run_id, tool, status, output, error, exit_code, created_at, completed_at FROM runs WHERE run_id = ?1",
            rusqlite::params![run_id],
            |r| {
                let status_str: String = r.get(2)?;
                let status = match status_str.as_str() {
                    "pending" => RunStatus::Pending,
                    "running" => RunStatus::Running,
                    "completed" => RunStatus::Completed,
                    "failed" => RunStatus::Failed,
                    "cancelled" => RunStatus::Cancelled,
                    _ => return Err(rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, format!("invalid status: {}", status_str).into())),
                };
                Ok(RunResult {
                    run_id: r.get(0)?,
                    tool: r.get(1)?,
                    status,
                    output: r.get(3)?,
                    error: r.get(4)?,
                    exit_code: r.get(5)?,
                    created_at: r.get(6)?,
                    completed_at: r.get(7)?,
                })
            },
        ) {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn expire(&self, ttl_seconds: u64) -> anyhow::Result<u64> {
        if ttl_seconds == 0 {
            return Ok(0);
        }
        let conn = lock_ok!(self.conn);
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::seconds(ttl_seconds as i64))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let deleted = conn.execute(
            "DELETE FROM runs WHERE created_at < ?1",
            rusqlite::params![cutoff],
        )?;
        Ok(deleted as u64)
    }
}

#[cfg(test)]
mod run_store_tests {
    use super::*;

    #[test]
    fn save_and_get_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteRunStore::open(dir.path().to_str().unwrap()).unwrap();
        let result = RunResult {
            run_id: "run-1".to_string(),
            tool: "loom_exec".to_string(),
            status: RunStatus::Completed,
            output: Some("ok".to_string()),
            error: None,
            exit_code: Some(0),
            created_at: crate::rfc3339_now(),
            completed_at: Some(crate::rfc3339_now()),
        };
        store.save(&result).unwrap();
        let got = store.get("run-1").unwrap().unwrap();
        assert_eq!(got.run_id, "run-1");
        assert_eq!(got.status, RunStatus::Completed);
        assert_eq!(got.output, Some("ok".to_string()));
        assert_eq!(store.expire(0).unwrap(), 0);
    }

    #[test]
    fn expire_removes_old_runs() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteRunStore::open(dir.path().to_str().unwrap()).unwrap();
        let old = RunResult {
            run_id: "run-old".to_string(),
            tool: "loom_exec".to_string(),
            status: RunStatus::Completed,
            output: None,
            error: None,
            exit_code: None,
            created_at: crate::rfc3339_days_ago(2),
            completed_at: None,
        };
        let fresh = RunResult {
            run_id: "run-fresh".to_string(),
            tool: "loom_exec".to_string(),
            status: RunStatus::Pending,
            output: None,
            error: None,
            exit_code: None,
            created_at: crate::rfc3339_now(),
            completed_at: None,
        };
        store.save(&old).unwrap();
        store.save(&fresh).unwrap();
        // TTL of 1 day should expire the 2-day-old run.
        let deleted = store.expire(86_400).unwrap();
        assert_eq!(deleted, 1);
        assert!(store.get("run-old").unwrap().is_none());
        assert!(store.get("run-fresh").unwrap().is_some());
    }
}
