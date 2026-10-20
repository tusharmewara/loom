//! Daemon lifecycle integration tests.
//!
//! These run the real `loomd` binary against a throwaway ecosystem
//! directory and assert the properties the v0.9.6 hardening work promised:
//!
//! - the daemon serves IPC and reports its version (the CLI↔daemon handshake)
//! - a second daemon refuses to start instead of stealing the live socket
//! - SIGTERM stops the daemon cleanly, removes its socket and releases the lock
//! - a stale socket left by a crashed daemon does not block a new one

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use loom_ipc::{client::send_command, IpcRequest, IpcResponse};
use tempfile::TempDir;

/// How long to wait for a daemon to become ready / exit.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const EXIT_TIMEOUT: Duration = Duration::from_secs(20);

/// A short-lived ecosystem directory.
///
/// The daemon's IPC socket lives inside it, and a Unix socket path is limited
/// to ~104 bytes, so the directory is kept short (`/tmp` rather than the
/// per-user temp dir, which is much deeper on macOS).
fn make_loom_dir() -> TempDir {
    tempfile::Builder::new()
        .prefix("loomd")
        .tempdir_in("/tmp")
        .or_else(|_| tempfile::Builder::new().prefix("loomd").tempdir())
        .expect("temp dir")
}

fn socket_path(loom_dir: &Path) -> PathBuf {
    loom_dir.join("loomd.sock")
}

/// Build the command that starts a daemon against `loom_dir`.
///
/// The daemon's stderr goes to `log` so a failing test can show what the
/// daemon said.
fn loomd_command(loom_dir: &Path, log: &Path) -> Command {
    let stderr = std::fs::File::create(log).expect("create daemon log file");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_loomd"));
    cmd.env("LOOM_AGENTS_DIR", loom_dir)
        // Never collide with a real daemon's gRPC port.
        .env("LOOM_GRPC_ADDR", "127.0.0.1:0")
        // The daemon must not shut itself down while the test watches it.
        .env("LOOM_IDLE_TIMEOUT_MS", "0")
        // No `npx` on PATH: the daemon must not spawn the agentmemory server
        // during a test.
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    cmd
}

/// Read the daemon's log file (stderr) — the diagnostic for a failed test.
fn read_log(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

/// Send one IPC command to the daemon and wait for the answer.
fn ipc(socket: &Path, command: &str) -> anyhow::Result<IpcResponse> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async { send_command(&socket.to_string_lossy(), IpcRequest::new(command)).await })
}

/// Wait until the daemon answers on its socket.
fn wait_until_ready(socket: &Path, child: &mut Child, log: &Path) {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if socket.exists() && ipc(socket, "version").is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("try_wait") {
            panic!(
                "loomd exited before becoming ready: {status}\n--- daemon log ---\n{}",
                read_log(log)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "loomd did not become ready within {READY_TIMEOUT:?}\n--- daemon log ---\n{}",
        read_log(log)
    );
}

/// Wait for the child to exit, returning its exit status.
fn wait_for_exit(child: &mut Child, timeout: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("try_wait") {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn daemon_serves_ipc_and_reports_its_version() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    let resp = ipc(&socket, "version").expect("version command");
    assert_eq!(resp.status, "ok");
    assert_eq!(resp.version, loom_ipc::IPC_PROTOCOL_VERSION);

    // The version the CLI compares its own against.
    let version = resp.data.as_ref().unwrap()["version"]
        .as_str()
        .expect("version string")
        .to_string();
    assert_eq!(version, env!("CARGO_PKG_VERSION"));

    // The health check must report the same version, or the CLI's skew
    // detection would compare against a different field than the handshake.
    let health = ipc(&socket, "health.check").expect("health.check");
    assert_eq!(health.data.unwrap()["version"], version);

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn a_second_daemon_refuses_to_start_and_leaves_the_running_one_alone() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let second_log = dir.path().join("daemon2.log");
    let mut first = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut first, &log);

    let mut second = loomd_command(dir.path(), &second_log)
        .spawn()
        .expect("spawn second loomd");
    let status = wait_for_exit(&mut second, EXIT_TIMEOUT).expect("second daemon must exit");
    assert!(
        !status.success(),
        "the second daemon must fail, got {status}"
    );
    let stderr = read_log(&second_log);
    assert!(
        stderr.contains("already holds the lock"),
        "the second daemon must report the held lock, stderr was: {stderr}"
    );

    // The running daemon keeps its socket and keeps serving.
    assert!(socket.exists(), "the live daemon's socket must survive");
    let resp = ipc(&socket, "version").expect("the first daemon still serves");
    assert_eq!(resp.status, "ok");

    let _ = first.kill();
    let _ = first.wait();
}

#[test]
fn sigterm_stops_the_daemon_and_releases_the_lock() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("send SIGTERM")
        .success();
    assert!(killed, "SIGTERM must be delivered");

    let status = wait_for_exit(&mut child, EXIT_TIMEOUT).expect("daemon must exit on SIGTERM");
    assert!(
        status.success(),
        "SIGTERM is a clean shutdown, got {status}\n--- daemon log ---\n{}",
        read_log(&log)
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while socket.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!socket.exists(), "the socket must be removed on shutdown");

    // The lock was released with the process, so a new daemon can take over.
    let next_log = dir.path().join("daemon2.log");
    let mut next = loomd_command(dir.path(), &next_log)
        .spawn()
        .expect("spawn loomd again");
    wait_until_ready(&socket, &mut next, &next_log);
    let _ = next.kill();
    let _ = next.wait();
}

#[cfg(unix)]
#[test]
fn a_stale_socket_left_by_a_crashed_daemon_does_not_block_a_new_one() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let socket = socket_path(dir.path());

    // A leftover socket from a daemon that was killed without cleaning up:
    // the file is still on disk, but nothing is listening on it.
    {
        let stale = std::os::unix::net::UnixListener::bind(&socket).expect("bind stale socket");
        drop(stale);
    }
    assert!(socket.exists(), "the stale socket file must still be there");

    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    wait_until_ready(&socket, &mut child, &log);

    let resp = ipc(&socket, "version").expect("the new daemon serves on the socket");
    assert_eq!(resp.status, "ok");

    let _ = child.kill();
    let _ = child.wait();
}
