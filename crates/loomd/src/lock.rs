//! Daemon-facing view of the single-instance lock.
//!
//! The implementation lives in `loom-core` so the CLI can probe the same lock
//! file before spawning a daemon; see [`loom_core::lock`].

pub use loom_core::lock::{acquire, holder_pid, lock_path, probe, DaemonLock, LOCK_FILE_NAME};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_until_guard_is_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");

        let first = acquire(dir.path()).expect("first acquire succeeds");
        assert_eq!(first.holder_pid(), Some(std::process::id()));

        let second = acquire(dir.path());
        assert!(
            second.is_err(),
            "second acquire must fail while the lock is held"
        );
        let err = second.unwrap_err().to_string();
        assert!(err.contains("loomd.lock"), "error must name the lock: {err}");
        assert!(
            err.contains(&std::process::id().to_string()),
            "error must name the holder pid: {err}"
        );

        drop(first);

        let third = acquire(dir.path()).expect("acquire succeeds after drop");
        assert_eq!(third.path().file_name().unwrap(), "loomd.lock");
    }
}
