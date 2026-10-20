//! Daemon-lifecycle integration tests.
//!
//! These pin the invariants that keep `loomd` a single instance:
//!
//! * the single-instance lock is exclusive, and the kernel releases it when
//!   the holder dies — a crash never leaves a permanent lock behind,
//! * a leftover socket file never blocks a fresh daemon,
//! * a second daemon refuses to start instead of stealing the live socket,
//! * `version` and `health.check` report the same daemon version, so a client
//!   can detect version skew.
//!
//! The daemon under test runs in a **child process** — a re-exec of this test
//! binary — because that is the production shape of the problem: the lock is
//! contended across processes, a crash can be simulated with SIGKILL, and
//! every test gets a pristine daemon (the daemon installs a global tracing
//! subscriber on startup, so two daemons cannot share one process).

// loom-ipc speaks Unix domain sockets only, so the whole daemon lifecycle
// exists on Unix only.
#![cfg(unix)]

use loom_core::lock::{self, LockError};
use loom_ipc::{IpcRequest, IpcResponse, IPC_PROTOCOL_VERSION};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

/// Environment marker that turns this test binary into a daemon child.
const CHILD_ROLE_ENV: &str = "LOOM_TEST_CHILD_ROLE";
/// Ecosystem directory the daemon child has to serve.
const CHILD_DIR_ENV: &str = "LOOM_TEST_CHILD_DIR";
/// Role value meaning "run a daemon and serve this directory".
const ROLE_DAEMON: &str = "daemon";
/// Name of the test the parent re-execs to get a daemon.
const CHILD_TEST: &str = "daemon_child_process";
/// Exit code used by a child that refuses to start because a daemon is live.
const EXIT_ALREADY_RUNNING: i32 = 3;
/// Exit code used when the daemon fails to initialise.
const EXIT_INIT_ERROR: i32 = 5;
/// Exit code used when the daemon fails while running.
const EXIT_RUN_ERROR: i32 = 6;
/// Marker the child prints when it refuses to start.
const REFUSED_MARKER: &str = "LOOMD_REFUSED_ALREADY_RUNNING";
/// How long a daemon gets to come up and answer `health.check`.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a refused second daemon gets to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Deadline for a single IPC round trip.
const IPC_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a lock re-acquire may retry while a forked child still holds a
/// copy of the lock descriptor.
const LOCK_RETRY_TIMEOUT: Duration = Duration::from_secs(10);
/// Pause between lock re-acquire attempts.
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(20);

// ─── Lock invariants (no daemon needed) ───────────────────────────────────

/// The single-instance lock admits exactly one holder.
#[test]
fn a_second_lock_acquire_is_rejected_while_the_first_is_held() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first = lock::acquire(dir.path()).expect("first acquire succeeds");
    assert_eq!(
        first.holder_pid(),
        Some(std::process::id()),
        "the holder records its own pid"
    );

    let err = lock::acquire(dir.path()).expect_err("second acquire must fail");
    match &err {
        LockError::Held { path, pid } => {
            assert_eq!(path, &lock::lock_path(dir.path()));
            assert_eq!(*pid, Some(std::process::id()), "the error names the holder");
        }
        other => panic!("expected LockError::Held, got {other:?}"),
    }

    // The CLI probes the same lock before spawning a daemon: it must see the
    // identical "already running" state.
    assert!(
        matches!(lock::probe(dir.path()), Err(LockError::Held { .. })),
        "a probe of a held lock must report Held"
    );
}

/// Dropping the guard releases the lock, so a clean restart can take it.
#[test]
fn the_lock_is_released_when_the_guard_is_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first = lock::acquire(dir.path()).expect("first acquire succeeds");
    drop(first);

    let second = acquire_lock_eventually(dir.path()).expect("acquire succeeds after the drop");
    assert_eq!(second.path(), lock::lock_path(dir.path()).as_path());
}

/// Acquire the daemon lock, retrying briefly while it is still held.
///
/// The lock is an `flock` on an open file descriptor. A `fork()` in another
/// thread — anything that spawns a process, including a concurrent test — copies
/// that descriptor into the child, and the kernel keeps the lock alive until
/// the child `exec`s and closes it. That window is short but real, so an
/// immediate re-acquire can fail with `Held` even though the guard was
/// dropped. Retrying for a moment distinguishes that transient from a lock
/// that is genuinely stuck.
fn acquire_lock_eventually(loom_dir: &Path) -> Result<loom_core::lock::DaemonLock, LockError> {
    let deadline = std::time::Instant::now() + LOCK_RETRY_TIMEOUT;
    loop {
        match lock::acquire(loom_dir) {
            Ok(guard) => return Ok(guard),
            // Only a "held" lock is worth waiting for; an I/O error is final.
            Err(LockError::Held { .. }) if std::time::Instant::now() < deadline => {
                std::thread::sleep(LOCK_RETRY_INTERVAL);
            }
            Err(err) => return Err(err),
        }
    }
}

// ─── Daemon lifecycle ──────────────────────────────────────────────────────

/// A junk file at the socket path does not stop a fresh daemon.
#[tokio::test]
async fn a_stale_socket_file_does_not_block_a_fresh_daemon() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("loomd.sock");
    std::fs::write(&socket, b"this is not a socket").expect("write junk socket file");

    let daemon = Daemon::start(dir.path(), "stale").await;
    let health = daemon.wait_ready().await;
    assert_eq!(health.status, "ok");

    // The junk file was replaced by a real Unix socket.
    assert!(
        is_socket(&socket),
        "loomd.sock must be a Unix socket, not a regular file"
    );
}

/// The daemon reports one version, and a client can detect a skew.
#[tokio::test]
async fn the_daemon_reports_one_version_and_version_skew_is_detected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let daemon = Daemon::start(dir.path(), "version").await;
    daemon.wait_ready().await;

    let version_resp = daemon.request("version").await;
    assert_eq!(version_resp.status, "ok");
    assert_eq!(
        version_resp.version, IPC_PROTOCOL_VERSION,
        "responses carry the IPC protocol version"
    );
    let version = version_resp
        .data
        .as_ref()
        .and_then(|d| d.get("version"))
        .and_then(|v| v.as_str())
        .expect("version command reports a version string")
        .to_string();
    assert!(!version.is_empty(), "the daemon must report a version");

    let health = daemon.request("health.check").await;
    let health_version = health
        .data
        .as_ref()
        .and_then(|d| d.get("version"))
        .and_then(|v| v.as_str())
        .expect("health.check reports a version");
    assert_eq!(
        health_version, version,
        "version and health.check must report the same version"
    );

    // The CLI's skew rule, mirrored locally: loom-cli keeps `version_skew`
    // `pub(crate)`, so it cannot be imported from an integration test.
    assert!(
        version_skew(Some(&version), &version).is_none(),
        "matching versions must not report a skew"
    );
    let skew = version_skew(Some(&version), "0.0.0-skewed").expect("a skew must be reported");
    assert!(skew.contains(&version), "the skew names the daemon: {skew}");
    assert!(
        skew.contains("0.0.0-skewed"),
        "the skew names the client: {skew}"
    );
    assert!(
        version_skew(None, &version)
            .expect("a missing daemon version must be reported")
            .to_lowercase()
            .contains("unknown"),
        "a daemon that reports no version is unknown, not a match"
    );
}

/// A second daemon on the same directory refuses to start.
#[tokio::test]
async fn a_second_daemon_refuses_to_start_and_the_first_keeps_serving() {
    let dir = tempfile::tempdir().expect("tempdir");

    let first = Daemon::start(dir.path(), "first").await;
    first.wait_ready().await;
    let socket_inode = inode(&first.socket());

    // A second daemon on the same directory must refuse to start.
    let mut second = Daemon::start(dir.path(), "second").await;
    let status = second.wait_exit().await;
    assert_eq!(
        status.code(),
        Some(EXIT_ALREADY_RUNNING),
        "the second daemon must refuse to start; log:\n{}",
        second.log_text()
    );
    assert!(
        second.log_text().contains(REFUSED_MARKER),
        "the second daemon must say why it refused; log:\n{}",
        second.log_text()
    );

    // The live daemon still owns the socket and keeps serving.
    assert_eq!(
        inode(&first.socket()),
        socket_inode,
        "the live socket must not be replaced"
    );
    let health = first.request("health.check").await;
    assert_eq!(health.status, "ok");
}

/// A crashed daemon leaves no permanent lock and its stale socket is taken over.
#[tokio::test]
async fn a_killed_daemon_releases_the_lock_and_a_restart_takes_over_the_socket() {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut first = Daemon::start(dir.path(), "first").await;
    first.wait_ready().await;
    let socket = first.socket();

    // Simulate a crash: SIGKILL, no cleanup at all.
    first.kill().await;
    assert!(
        socket.exists(),
        "a crashed daemon leaves its socket file behind"
    );

    // The kernel reclaims the lock of a dead process, so no stale-lock
    // recovery is needed and a restart can take the lock.
    let lock = acquire_lock_eventually(dir.path()).expect("the lock of a dead daemon is free");
    drop(lock);

    // A fresh daemon replaces the stale socket and serves again.
    let second = Daemon::start(dir.path(), "second").await;
    let health = second.wait_ready().await;
    assert_eq!(health.status, "ok");
    assert!(
        is_socket(&second.socket()),
        "loomd.sock must be a Unix socket"
    );
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Mirror of the CLI's version-skew rule.
///
/// `loom-cli` keeps its `version_skew` helper `pub(crate)`, so the rule is
/// reproduced here instead of imported: a mismatch must be surfaced, and a
/// daemon that reports no version is "unknown" rather than a match.
fn version_skew(daemon_version: Option<&str>, cli_version: &str) -> Option<String> {
    let cli = cli_version.trim();
    match daemon_version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(daemon) if daemon == cli => None,
        Some(daemon) => Some(format!(
            "Version mismatch: CLI {cli}, daemon {daemon} — restart the daemon: loom daemon restart"
        )),
        None => Some(format!(
            "Daemon version unknown (CLI {cli}) — daemon reported no version, it may be an older build."
        )),
    }
}

/// Inode of a path, used to tell "socket replaced" from "socket untouched".
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).expect("metadata").ino()
}

/// True when `path` is a Unix domain socket, not a regular file.
fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    std::fs::metadata(path)
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false)
}

/// A daemon running in a child process of this test binary.
struct Daemon {
    child: tokio::process::Child,
    dir: PathBuf,
    log: PathBuf,
}

impl Daemon {
    /// Spawn a daemon child serving `dir`.
    async fn start(dir: &Path, tag: &str) -> Daemon {
        let exe = std::env::current_exe().expect("current_exe of the test binary");
        let log = dir.join(format!("daemon-{tag}.log"));
        let log_file = std::fs::File::create(&log).expect("create daemon log");

        // An empty PATH keeps the daemon from spawning the agentmemory memory
        // server (`npx`): the daemon logs and continues, and the test leaves
        // no external process behind.
        let empty_bin = dir.join("empty-bin");
        std::fs::create_dir_all(&empty_bin).expect("create empty bin dir");

        let child = tokio::process::Command::new(exe)
            .arg(CHILD_TEST)
            .arg("--exact")
            .arg("--ignored")
            .arg("--nocapture")
            .env(CHILD_ROLE_ENV, ROLE_DAEMON)
            .env(CHILD_DIR_ENV, dir)
            .env("LOOM_AGENTS_DIR", dir)
            .env("LOOM_GRPC_ADDR", "127.0.0.1:0")
            .env("HOME", dir)
            .env("PATH", &empty_bin)
            .stdout(Stdio::from(log_file.try_clone().expect("clone log file")))
            .stderr(Stdio::from(log_file))
            .kill_on_drop(true)
            .spawn()
            .expect("spawn daemon child");

        Daemon {
            child,
            dir: dir.to_path_buf(),
            log,
        }
    }

    /// Path of the daemon's IPC socket.
    fn socket(&self) -> PathBuf {
        self.dir.join("loomd.sock")
    }

    /// The daemon's IPC socket as a string.
    fn socket_str(&self) -> String {
        self.socket()
            .to_str()
            .expect("socket path is utf-8")
            .to_string()
    }

    /// Send one IPC command and return the response.
    async fn request(&self, command: &str) -> IpcResponse {
        loom_ipc::send_command_with_timeout(
            &self.socket_str(),
            IpcRequest::new(command),
            IPC_TIMEOUT,
        )
        .await
        .unwrap_or_else(|e| {
            panic!(
                "IPC {command} failed: {e}\ndaemon log:\n{}",
                self.log_text()
            )
        })
    }

    /// Wait until the daemon answers `health.check`.
    async fn wait_ready(&self) -> IpcResponse {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            let attempt = loom_ipc::send_command_with_timeout(
                &self.socket_str(),
                IpcRequest::new("health.check"),
                Duration::from_secs(2),
            )
            .await;
            if let Ok(resp) = attempt {
                if resp.status == "ok" {
                    return resp;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "daemon did not become ready within {READY_TIMEOUT:?}\ndaemon log:\n{}",
                    self.log_text()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Wait for the daemon child to exit on its own.
    async fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = tokio::time::Instant::now() + EXIT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait on daemon child") {
                return status;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "the daemon child did not exit within {EXIT_TIMEOUT:?}\ndaemon log:\n{}",
                    self.log_text()
                );
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Kill the daemon child (SIGKILL — no cleanup, like a crash).
    async fn kill(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }

    /// The daemon child's captured output.
    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

/// The daemon child: re-exec'd by the parent, never run by `cargo test`.
///
/// With `LOOM_TEST_CHILD_ROLE=daemon` it serves `LOOM_TEST_CHILD_DIR` until
/// it is killed; without it, it does nothing.
#[test]
#[ignore = "re-exec'd as a daemon child by the daemon-lifecycle tests"]
fn daemon_child_process() {
    if std::env::var(CHILD_ROLE_ENV).as_deref() != Ok(ROLE_DAEMON) {
        return;
    }
    let dir = PathBuf::from(std::env::var(CHILD_DIR_ENV).expect("child dir env"));

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let code = runtime.block_on(async {
        let daemon = match loomd::Loomd::new(dir.to_str().expect("utf-8 dir")) {
            Ok(daemon) => daemon,
            Err(e) => {
                eprintln!("LOOMD_INIT_ERROR {e}");
                return EXIT_INIT_ERROR;
            }
        };
        match daemon.run().await {
            Ok(()) => 0,
            // The same guard `loomd::run()` uses: a live daemon holds the lock,
            // so a second daemon must refuse to start instead of stealing its
            // socket.
            Err(e) => match e.downcast_ref::<LockError>() {
                Some(LockError::Held { pid, .. }) => {
                    println!("{REFUSED_MARKER} pid={pid:?}");
                    EXIT_ALREADY_RUNNING
                }
                _ => {
                    eprintln!("LOOMD_RUN_ERROR {e}");
                    EXIT_RUN_ERROR
                }
            },
        }
    });
    std::process::exit(code);
}
