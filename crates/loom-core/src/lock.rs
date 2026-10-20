//! Single-instance guard for the loomd daemon.
//!
//! The daemon binds `loomd.sock`; a second instance would otherwise steal the
//! live socket from the first one. An exclusive OS lock on a file in
//! `.cache/loomd.lock` prevents that: the lock is released by the kernel when
//! the holding process dies (even on SIGKILL), so a leftover lock file is
//! harmless and needs no stale-lock recovery.

use crate::constants::CACHE_DIR;
use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Name of the lock file inside the loom directory's cache folder.
pub const LOCK_FILE_NAME: &str = "loomd.lock";

/// Path of the daemon lock file for `loom_dir`.
pub fn lock_path(loom_dir: &Path) -> PathBuf {
    loom_dir.join(CACHE_DIR).join(LOCK_FILE_NAME)
}

/// Why the daemon lock could not be taken.
#[derive(Debug)]
pub enum LockError {
    /// Another process already holds the lock.
    Held { path: PathBuf, pid: Option<u32> },
    /// The lock file could not be opened or locked (permissions, read-only fs).
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockError::Held { path, pid } => write!(
                f,
                "Another loomd instance already holds the lock {} (pid {})",
                path.display(),
                pid.map(|p| p.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            LockError::Io { path, source } => {
                write!(f, "Failed to lock {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LockError::Held { .. } => None,
            LockError::Io { source, .. } => Some(source),
        }
    }
}

/// Guard holding the exclusive lock. Keep it alive for as long as the daemon
/// runs — dropping it releases the lock.
#[derive(Debug)]
pub struct DaemonLock {
    file: File,
    path: PathBuf,
}

impl DaemonLock {
    /// Path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Pid of the process holding the lock, as recorded in the lock file.
    pub fn holder_pid(&self) -> Option<u32> {
        read_pid(&self.path)
    }

    /// Record our pid in the lock file so humans and tools can see the holder.
    fn write_pid(&mut self) {
        let pid = std::process::id().to_string();
        let write = self
            .file
            .set_len(0)
            .and_then(|_| self.file.seek(SeekFrom::Start(0)))
            .and_then(|_| self.file.write_all(pid.as_bytes()))
            .and_then(|_| self.file.flush());
        if let Err(e) = write {
            tracing::warn!("Could not record pid in {}: {e}", self.path.display());
        }
    }
}

/// Acquire the daemon lock for `loom_dir`.
///
/// Returns [`LockError::Held`] when another loomd instance already holds it.
pub fn acquire(loom_dir: &Path) -> Result<DaemonLock, LockError> {
    let mut lock = try_acquire(loom_dir)?;
    lock.write_pid();
    Ok(lock)
}

/// Probe the lock without touching the lock file's contents.
///
/// Used by the CLI to test "is a daemon running?" without overwriting the
/// holder's pid. The returned guard releases the lock when dropped.
pub fn probe(loom_dir: &Path) -> Result<DaemonLock, LockError> {
    try_acquire(loom_dir)
}

fn try_acquire(loom_dir: &Path) -> Result<DaemonLock, LockError> {
    let cache_dir = loom_dir.join(CACHE_DIR);
    std::fs::create_dir_all(&cache_dir).map_err(|source| LockError::Io {
        path: cache_dir.clone(),
        source,
    })?;
    let path = cache_dir.join(LOCK_FILE_NAME);

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        // The pid is rewritten after the lock is taken; truncating here would
        // wipe the current holder's pid before we know whether we won.
        .truncate(false)
        .open(&path)
        .map_err(|source| LockError::Io {
            path: path.clone(),
            source,
        })?;

    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(LockError::Held {
                path: path.clone(),
                pid: holder_pid(loom_dir),
            });
        }
        Err(TryLockError::Error(source)) => {
            return Err(LockError::Io { path, source });
        }
    }

    Ok(DaemonLock { file, path })
}

/// Pid recorded in the lock file, if any.
pub fn holder_pid(loom_dir: &Path) -> Option<u32> {
    read_pid(&lock_path(loom_dir))
}

/// Best-effort read of the pid recorded in a lock file.
fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
