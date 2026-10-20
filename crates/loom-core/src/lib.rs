pub mod config;
pub mod constants;
pub mod lock;
pub mod plugin_registry;
pub mod types;

pub use config::*;
pub use constants::*;
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
