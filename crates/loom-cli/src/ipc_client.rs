use loom_ipc::{IpcRequest, send_command};
use std::path::Path;
use std::time::Duration;

/// Start loomd if the Unix socket is not present.
pub async fn ensure_daemon(loom_dir: &Path) -> anyhow::Result<()> {
    let socket_path = loom_dir.join("loomd.sock");
    if socket_path.exists() {
        return Ok(());
    }
    crate::commands::daemon::start_daemon(loom_dir)?;
    for _ in 0..10 {
        if socket_path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    anyhow::bail!("loomd failed to start (socket not created)");
}

pub async fn send_daemon_command(
    loom_dir: &Path,
    req: IpcRequest,
) -> anyhow::Result<loom_ipc::IpcResponse> {
    ensure_daemon(loom_dir).await?;
    let socket_path = loom_dir.join("loomd.sock");
    send_command(socket_path.to_str().unwrap(), req).await
}

/// Send IPC only when the daemon socket already exists (no auto-start).
pub async fn send_daemon_command_if_running(
    loom_dir: &Path,
    req: IpcRequest,
) -> Option<loom_ipc::IpcResponse> {
    let socket_path = loom_dir.join("loomd.sock");
    if !socket_path.exists() {
        return None;
    }
    send_command(socket_path.to_str().unwrap(), req).await.ok()
}
