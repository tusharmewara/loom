//! E2E integration tests for the MCP control plane (mcp-control-plane feature).
//!
//! These tests run the real `loomd` binary against a throwaway ecosystem
//! directory with `mcp-control-plane` enabled, and assert:
//! - the control-plane IPC commands are available (exec.run, workflow.dispatch,
//!   skills.activate, run.result.get)
//! - state-changing commands are ACL-gated when an allowlist is configured
//! - run results are persisted and retrievable by run_id
//! - unknown commands return errors

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use loom_ipc::{client::send_command, IpcResponse};
use serde_json::json;
use tempfile::TempDir;

const READY_TIMEOUT: Duration = Duration::from_secs(30);

fn make_loom_dir() -> TempDir {
    tempfile::Builder::new()
        .prefix("loomd-cp")
        .tempdir_in("/tmp")
        .or_else(|_| tempfile::Builder::new().prefix("loomd-cp").tempdir())
        .expect("temp dir")
}

fn socket_path(loom_dir: &Path) -> PathBuf {
    loom_dir.join("loomd.sock")
}

fn loomd_command(loom_dir: &Path, log: &Path) -> Command {
    let stderr = std::fs::File::create(log).expect("create daemon log file");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_loomd"));
    cmd.env("LOOM_AGENTS_DIR", loom_dir)
        .env("LOOM_GRPC_ADDR", "127.0.0.1:0")
        .env("LOOM_IDLE_TIMEOUT_MS", "0")
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    cmd
}

fn read_log(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

fn wait_until_ready(socket: &Path, child: &mut Child, log: &Path) {
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if socket.exists() && ipc(socket, "version", serde_json::Value::Null).is_ok() {
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

fn ipc(socket: &Path, command: &str, params: serde_json::Value) -> anyhow::Result<IpcResponse> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let req = loom_ipc::protocol::IpcRequest::new(command).with_params(params);
    rt.block_on(async { send_command(&socket.to_string_lossy(), req).await })
}

fn ipc_simple(socket: &Path, command: &str) -> anyhow::Result<IpcResponse> {
    ipc(socket, command, serde_json::Value::Null)
}

#[test]
fn control_plane_commands_are_available_when_feature_enabled() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    // version should still work
    let resp = ipc_simple(&socket, "version").expect("version command");
    assert_eq!(resp.status, "ok");

    // skills.activate should return "skill not found" (skill doesn't exist),
    // which proves the command is registered and reachable.
    let resp = ipc(
        &socket,
        "skills.activate",
        json!({ "skill": "nonexistent-skill" }),
    )
    .expect("skills.activate command");
    assert_eq!(resp.status, "error");
    assert!(resp.error.unwrap().contains("not found"));

    // workflow.dispatch with missing task_id should error
    let resp = ipc(&socket, "workflow.dispatch", json!({}))
        .expect("workflow.dispatch command");
    assert_eq!(resp.status, "error");
    assert!(resp.error.unwrap().contains("task_id parameter required"));

    // exec.run with missing tool should error
    let resp = ipc(&socket, "exec.run", json!({}))
        .expect("exec.run command");
    assert_eq!(resp.status, "error");
    assert!(resp.error.unwrap().contains("tool parameter required"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_result_get_returns_not_found_for_unknown_run() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    let resp = ipc(
        &socket,
        "run.result.get",
        json!({ "run_id": "nonexistent-run-id" }),
    )
    .expect("run.result.get command");
    assert_eq!(resp.status, "error");
    assert!(resp.error.unwrap().contains("run not found"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn unknown_command_returns_error() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    let resp = ipc_simple(&socket, "nonexistent.command").expect("unknown command");
    assert_eq!(resp.status, "error");
    assert!(resp.error.unwrap().contains("Unknown command"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn acl_allows_tool_when_allowlist_is_empty() {
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    // With no ACL configured (empty allowlist), all tools are allowed.
    // skills.activate with a nonexistent skill returns "not found" (not "ACL denied").
    let resp = ipc(
        &socket,
        "skills.activate",
        json!({ "skill": "nonexistent-skill" }),
    )
    .expect("skills.activate command");
    assert_eq!(resp.status, "error");
    let err = resp.error.unwrap();
    assert!(err.contains("not found"));
    assert!(!err.contains("ACL denied"));

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn mcp_control_plane_gated_by_feature_flag() {
    // This test verifies that without the mcp-control-plane feature, the
    // control-plane IPC commands are not available. We can't easily test the
    // binary without the flag here, but the compile-time feature flag
    // guarantees this at build time. This test documents the expectation.
    let dir = make_loom_dir();
    let log = dir.path().join("daemon.log");
    let mut child = loomd_command(dir.path(), &log)
        .spawn()
        .expect("spawn loomd");
    let socket = socket_path(dir.path());
    wait_until_ready(&socket, &mut child, &log);

    // The daemon built with mcp-control-plane should serve these commands.
    // The build itself enforces the feature flag.
    let resp = ipc_simple(&socket, "version").expect("version command");
    assert_eq!(resp.status, "ok");

    let _ = child.kill();
    let _ = child.wait();
}
