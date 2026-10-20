use crate::cli::DaemonAction;
use crate::service::{self, Ownership, Supervisor};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PID_FILE: &str = "loomd.pid";
const DAEMON_SOCKET: &str = "loomd.sock";
#[cfg(target_os = "macos")]
const SERVICE_LABEL: &str = "com.loom.loomd";

/// How long to wait for a daemon to come up or go down, in milliseconds.
const LIFECYCLE_WAIT_MS: u64 = 5_000;
const LIFECYCLE_POLL_MS: u64 = 50;

/// Whether something is listening on the daemon socket.
///
/// A socket *file* can outlive the daemon that bound it (a crash), so the
/// file's presence says nothing about liveness — only a successful connect
/// does.
#[cfg(unix)]
pub(crate) fn socket_live(path: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(path).is_ok()
}

#[cfg(not(unix))]
pub(crate) fn socket_live(path: &Path) -> bool {
    path.exists()
}

/// Whether a daemon is currently serving this ecosystem directory.
///
/// The lock is the authoritative signal for a current daemon, but a daemon
/// from an older build (before the lock existed) still answers on the
/// socket — so a live socket counts too.
fn daemon_running(loom_dir: &Path) -> bool {
    let holds_lock = matches!(
        loom_core::lock::probe(loom_dir),
        Err(loom_core::lock::LockError::Held { .. })
    );
    holds_lock || socket_live(&loom_dir.join(DAEMON_SOCKET))
}

pub fn start_daemon(loom_dir: &Path) -> anyhow::Result<()> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let pid_path = loom_dir.join(".cache").join(PID_FILE);

    std::fs::create_dir_all(loom_dir.join(".cache"))?;

    // A service unit owns the daemon when one is installed for this
    // directory: starting a second, competing daemon is exactly what the
    // unit exists to prevent.
    match service::ownership(loom_dir) {
        Ownership::Unit { supervisor } => return start_via_supervisor(loom_dir, supervisor),
        Ownership::ForeignUnit { supervisor, dir } => {
            // The unit serves a *different* ecosystem dir, so it is not the
            // owner of this one. The manual daemon below is legitimate, but
            // both daemons will contend for the default gRPC port.
            eprintln!(
                "  ⚠ The installed {} service serves {} — starting a separate loomd for {}",
                supervisor.unit_name(),
                dir.display(),
                loom_dir.display()
            );
        }
        Ownership::None => {}
    }

    // Probe the single-instance lock: if another loomd holds it, we are done.
    // The probe lock is dropped before spawning so the fresh daemon can take
    // it — the daemon itself holds the lock for its whole lifetime.
    match loom_core::lock::probe(loom_dir) {
        Ok(probe) => drop(probe),
        Err(loom_core::lock::LockError::Held { pid, .. }) => {
            let pid = pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            println!("loomd already running (pid {pid})");
            return Ok(());
        }
        Err(e) => {
            // The lock file is unusable (permissions, read-only fs). The
            // daemon cannot take the lock either, so it would refuse to run —
            // fail here instead of reporting a daemon that never came up.
            anyhow::bail!("Cannot use the loomd single-instance lock: {e}");
        }
    }

    // No daemon holds the lock, so any socket lying around is stale.
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let child = Command::new("loomd")
        .env("LOOM_AGENTS_DIR", loom_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match child {
        Ok(proc) => {
            std::fs::write(&pid_path, proc.id().to_string())?;
            println!("Started loomd (pid {})", proc.id());
            Ok(())
        }
        Err(e) => anyhow::bail!(
            "Failed to start loomd: {e}. Ensure `loomd` is on PATH (cargo install or build)."
        ),
    }
}

/// Start the daemon through its service unit and wait for it to serve.
fn start_via_supervisor(loom_dir: &Path, supervisor: Supervisor) -> anyhow::Result<()> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);

    if daemon_running(loom_dir) {
        println!(
            "loomd already running (owned by the {} service)",
            supervisor.unit_name()
        );
        return Ok(());
    }

    service::start(supervisor)?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(LIFECYCLE_WAIT_MS);
    while std::time::Instant::now() < deadline {
        if socket_live(&socket_path) {
            println!("Started loomd via the {} service", supervisor.unit_name());
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(LIFECYCLE_POLL_MS));
    }

    anyhow::bail!(
        "The {} service did not bring loomd up within {}ms",
        supervisor.unit_name(),
        LIFECYCLE_WAIT_MS
    )
}

fn stop_daemon(loom_dir: &Path) {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let pid_path = loom_dir.join(".cache").join(PID_FILE);

    // A service unit owns the daemon: stopping means telling the supervisor
    // to stop it. The unit stays installed (and enabled), so a later
    // `daemon start` brings the same daemon back.
    if let Ownership::Unit { supervisor } = service::ownership(loom_dir) {
        stop_via_supervisor(loom_dir, supervisor);
        return;
    }

    if !stop_manual_daemon(loom_dir) {
        // Nothing holds the lock and there is no pid file: no daemon.
        let _ = std::fs::remove_file(&pid_path);
        let _ = std::fs::remove_file(&socket_path);
        println!("loomd is not running; removed stale socket if present");
    }
}

/// Stop a daemon that was started directly (not by a service unit).
///
/// Returns whether a daemon was actually stopped.
fn stop_manual_daemon(loom_dir: &Path) -> bool {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let pid_path = loom_dir.join(".cache").join(PID_FILE);

    // The lock is the authoritative "is a daemon running" signal: the daemon
    // holds it for its whole life and records its own pid, so a held lock
    // means the pid is live and belongs to the daemon. The pid file is only a
    // fallback (it is missing for a daemon started by launchd/systemd).
    let holder = match loom_core::lock::probe(loom_dir) {
        Ok(_free) => None,
        Err(loom_core::lock::LockError::Held { pid, .. }) => pid,
        Err(e) => {
            eprintln!("Could not probe the loomd lock: {e}");
            None
        }
    };

    let Some(pid) = holder.or_else(|| {
        std::fs::read_to_string(&pid_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
    }) else {
        return false;
    };

    let killed = Command::new("kill")
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if killed {
        // Wait for the process to actually exit: the daemon holds the
        // single-instance lock until the kernel reclaims it, and
        // `daemon restart` starts a new daemon right after this.
        wait_for_exit(pid, 2_000);
    }
    let _ = std::fs::remove_file(&pid_path);
    if killed {
        let _ = std::fs::remove_file(&socket_path);
        println!("Stopped loomd (pid {pid})");
        true
    } else {
        // Still alive: leave its socket alone.
        println!("Could not stop loomd (pid {pid}); it may already be gone");
        false
    }
}

/// Stop the daemon through its service unit and wait for it to go down.
fn stop_via_supervisor(loom_dir: &Path, supervisor: Supervisor) {
    let socket_path = loom_dir.join(DAEMON_SOCKET);

    if !daemon_running(loom_dir) {
        println!(
            "loomd is not running (the {} service is installed but idle)",
            supervisor.unit_name()
        );
        return;
    }

    // Remember the daemon we are about to stop: a unit that still has a
    // blanket "keep alive" restarts it immediately, and the new daemon gets
    // a different pid.
    let pid_before = loom_core::lock::holder_pid(loom_dir);

    if let Err(e) = service::stop(supervisor) {
        eprintln!("Could not stop the {} service: {e}", supervisor.unit_name());
        return;
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(LIFECYCLE_WAIT_MS);
    while std::time::Instant::now() < deadline {
        if !daemon_running(loom_dir) {
            let _ = std::fs::remove_file(&socket_path);
            println!("Stopped loomd (via the {} service)", supervisor.unit_name());
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(LIFECYCLE_POLL_MS));
    }

    // Still running: either the unit refused to stop it, or it came back.
    let pid_after = loom_core::lock::holder_pid(loom_dir);
    if pid_before.is_some() && pid_after.is_some() && pid_before != pid_after {
        eprintln!(
            "The {} service restarted loomd (pid {} → {}). Its unit file still has a blanket \
             keep-alive; re-run `loom daemon install-service` to update it.",
            supervisor.unit_name(),
            pid_before.unwrap_or(0),
            pid_after.unwrap_or(0)
        );
    } else {
        eprintln!(
            "The {} service did not stop loomd within {}ms",
            supervisor.unit_name(),
            LIFECYCLE_WAIT_MS
        );
    }
}

/// Wait until `pid` is gone, up to `timeout_ms` milliseconds.
fn wait_for_exit(pid: u32, timeout_ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        let alive = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !alive {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(target_os = "macos")]
fn install_service(loom_dir: &Path) -> anyhow::Result<()> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("HOME not set"))?;
    let launch_agents_dir = home.join("Library").join("LaunchAgents");
    std::fs::create_dir_all(&launch_agents_dir)?;

    let plist_path = launch_agents_dir.join(format!("{SERVICE_LABEL}.plist"));
    let loomed_bin = resolve_loomd_binary()?;
    let cache_dir = loom_dir.join(".cache");
    std::fs::create_dir_all(&cache_dir)?;
    let stdout_log = cache_dir.join("loomd.stdout.log");
    let stderr_log = cache_dir.join("loomd.stderr.log");

    // A daemon started outside the unit would keep holding the single-instance
    // lock, and the unit's own daemon would refuse to start. Hand ownership
    // over: stop the manual daemon first.
    if daemon_running(loom_dir) {
        println!("Stopping the manually started loomd so the service can take over");
        stop_manual_daemon(loom_dir);
    }

    // A unit that is already installed is replaced below, but the old job
    // must leave the domain first: `launchctl bootstrap` refuses a label that
    // is still loaded.
    let uid = detect_uid()?;
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{SERVICE_LABEL}");
    let _ = Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let plist = render_launchd_plist(
        SERVICE_LABEL,
        &loomed_bin,
        loom_dir,
        &stdout_log,
        &stderr_log,
    );
    std::fs::write(&plist_path, plist)?;

    // `service::bootstrap` owns the launchctl plumbing.
    service::bootstrap()?;

    let _ = Command::new("launchctl")
        .arg("enable")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    println!(
        "Installed launchd service '{SERVICE_LABEL}' at {}",
        plist_path.display()
    );
    println!("Service started and configured to run at login.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_service(loom_dir: &Path) -> anyhow::Result<()> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("HOME not set"))?;
    let systemd_user_dir = home.join(".config").join("systemd").join("user");
    std::fs::create_dir_all(&systemd_user_dir)?;

    let unit_path = systemd_user_dir.join("loomd.service");
    let loomed_bin = resolve_loomd_binary()?;
    let unit = render_systemd_unit(&loomed_bin, loom_dir);
    std::fs::write(&unit_path, unit)?;

    // A daemon started outside the unit would keep holding the single-instance
    // lock, and the unit's own daemon would refuse to start. Hand ownership
    // over: stop the manual daemon first.
    if daemon_running(loom_dir) {
        println!("Stopping the manually started loomd so the service can take over");
        stop_manual_daemon(loom_dir);
    }

    run_systemctl_user(["daemon-reload"])?;
    // `enable --now` keeps the unit enabled at login and starts it now; an
    // already-running unit is restarted so the new unit file takes effect.
    run_systemctl_user(["enable", "--now", "loomd.service"])?;

    println!(
        "Installed systemd user service 'loomd.service' at {}",
        unit_path.display()
    );
    println!("Service enabled and started.");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install_service(_loom_dir: &Path) -> anyhow::Result<()> {
    anyhow::bail!("Service installation is not supported on this operating system yet.")
}

#[cfg(target_os = "macos")]
fn uninstall_service() -> anyhow::Result<()> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("HOME not set"))?;
    let plist_path = home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist"));
    let uid = detect_uid()?;
    let target = format!("gui/{uid}/{SERVICE_LABEL}");

    let _ = Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("launchctl")
        .arg("disable")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
        println!("Removed {}", plist_path.display());
    } else {
        println!("Service plist not found at {}", plist_path.display());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_service() -> anyhow::Result<()> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("HOME not set"))?;
    let unit_path = home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("loomd.service");

    let _ = run_systemctl_user(["disable", "--now", "loomd.service"]);
    let _ = run_systemctl_user(["daemon-reload"]);

    if unit_path.exists() {
        std::fs::remove_file(&unit_path)?;
        println!("Removed {}", unit_path.display());
    } else {
        println!("Service unit not found at {}", unit_path.display());
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn uninstall_service() -> anyhow::Result<()> {
    anyhow::bail!("Service uninstall is not supported on this operating system yet.")
}

#[cfg(target_os = "linux")]
fn run_systemctl_user<const N: usize>(args: [&str; N]) -> anyhow::Result<()> {
    let status = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .status()?;
    if !status.success() {
        anyhow::bail!("systemctl --user command failed");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn detect_uid() -> anyhow::Result<String> {
    let out = Command::new("id").arg("-u").output()?;
    if !out.status.success() {
        anyhow::bail!("failed to detect user id via `id -u`");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn resolve_loomd_binary() -> anyhow::Result<PathBuf> {
    if let Some(path) = find_executable("loomd") {
        return Ok(path);
    }
    anyhow::bail!("Unable to locate `loomd` on PATH. Build/install loomd first.")
}

fn find_executable(binary: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(binary);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(target_os = "macos")]
fn render_launchd_plist(
    label: &str,
    loomed_bin: &Path,
    loom_dir: &Path,
    stdout_log: &Path,
    stderr_log: &Path,
) -> String {
    // LOOM_IDLE_TIMEOUT_MS=0 disables loomd's self-managed 5-minute idle
    // shutdown: under a supervisor the supervisor owns the lifecycle, and a
    // daemon that exits on its own is immediately restarted (respawn loop).
    // KeepAlive{SuccessfulExit=false} restarts only on a *crash* (non-zero
    // exit); a deliberate clean shutdown (exit 0) is not resurrected.
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>LOOM_AGENTS_DIR</key>
    <string>{}</string>
    <key>LOOM_IDLE_TIMEOUT_MS</key>
    <string>0</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        loomed_bin.display(),
        loom_dir.display(),
        stdout_log.display(),
        stderr_log.display(),
    )
}

#[cfg(target_os = "linux")]
fn render_systemd_unit(loomd_bin: &Path, loom_dir: &Path) -> String {
    // LOOM_IDLE_TIMEOUT_MS=0 disables loomd's self-managed 5-minute idle
    // shutdown: under systemd the unit owns the lifecycle, and a daemon that
    // exits on its own is immediately restarted (respawn loop).
    // Restart=on-failure restarts only on a *crash* (non-zero exit); a
    // deliberate clean shutdown (exit 0) is not resurrected.
    format!(
        r#"[Unit]
Description=Loom Daemon
After=network.target

[Service]
Type=simple
ExecStart="{}"
Restart=on-failure
RestartSec=2
Environment=LOOM_AGENTS_DIR={}
Environment=LOOM_IDLE_TIMEOUT_MS=0

[Install]
WantedBy=default.target
"#,
        loomd_bin.display(),
        loom_dir.display(),
    )
}

pub async fn run(loom_dir: &Path, action: &DaemonAction) -> anyhow::Result<()> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);

    match action {
        DaemonAction::Start => start_daemon(loom_dir)?,
        DaemonAction::Stop => stop_daemon(loom_dir),
        DaemonAction::Status { json } => {
            // The supervisor that owns the daemon, and what it is doing.
            let (supervised, supervisor_state) = match service::ownership(loom_dir) {
                Ownership::Unit { supervisor } => {
                    let state = if service::is_running(supervisor) {
                        "running"
                    } else {
                        "idle"
                    };
                    (
                        Some(supervisor.unit_name().to_string()),
                        Some(state.to_string()),
                    )
                }
                Ownership::ForeignUnit { supervisor, .. } => (
                    Some(supervisor.unit_name().to_string()),
                    Some("serves another directory".to_string()),
                ),
                Ownership::None => (None, None),
            };

            if socket_path.exists() {
                let req = loom_ipc::IpcRequest::new("health.check");
                match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                    Ok(resp) if resp.status == "ok" => {
                        let data = resp.data.unwrap_or(serde_json::Value::Null);
                        if *json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "command": "daemon.status",
                                    "running": true,
                                    "socket_path": socket_path.display().to_string(),
                                    "supervisor": supervised,
                                    "supervisor_state": supervisor_state,
                                    "status": "ok",
                                    "data": data
                                }))?
                            );
                        } else {
                            println!("loomd is running ({})", socket_path.display());
                            if let (Some(unit), Some(state)) = (&supervised, &supervisor_state) {
                                println!("  supervised by the {unit} service ({state})");
                            }
                            println!("{}", serde_json::to_string_pretty(&data)?);
                        }
                    }
                    Ok(resp) => {
                        if *json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "command": "daemon.status",
                                    "running": false,
                                    "socket_path": socket_path.display().to_string(),
                                    "supervisor": supervised,
                                    "supervisor_state": supervisor_state,
                                    "status": resp.status,
                                    "error": resp.error
                                }))?
                            );
                        } else {
                            println!("loomd responded with status: {}", resp.status);
                        }
                    }
                    Err(e) => {
                        if *json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "command": "daemon.status",
                                    "running": false,
                                    "socket_path": socket_path.display().to_string(),
                                    "supervisor": supervised,
                                    "supervisor_state": supervisor_state,
                                    "status": "unreachable",
                                    "error": e.to_string()
                                }))?
                            );
                        } else {
                            println!("Socket exists but daemon unreachable: {e}");
                        }
                    }
                }
            } else {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "daemon.status",
                            "running": false,
                            "socket_path": socket_path.display().to_string(),
                            "supervisor": supervised,
                            "supervisor_state": supervisor_state,
                            "status": "not_running"
                        }))?
                    );
                } else {
                    println!("loomd is not running");
                }
            }
        }
        DaemonAction::Restart => {
            stop_daemon(loom_dir);
            start_daemon(loom_dir)?;
        }
        DaemonAction::InstallService => install_service(loom_dir)?,
        DaemonAction::UninstallService => uninstall_service()?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_executable_missing() {
        assert!(find_executable("definitely-not-a-real-executable").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_render_launchd_plist() {
        let plist = render_launchd_plist(
            "com.loom.test",
            Path::new("/usr/local/bin/loomd"),
            Path::new("/tmp/.loom"),
            Path::new("/tmp/.loom/.cache/out.log"),
            Path::new("/tmp/.loom/.cache/err.log"),
        );
        assert!(plist.contains("com.loom.test"));
        assert!(plist.contains("/usr/local/bin/loomd"));
        assert!(plist.contains("LOOM_AGENTS_DIR"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_render_launchd_plist_disables_idle_shutdown() {
        let plist = render_launchd_plist(
            "com.loom.test",
            Path::new("/usr/local/bin/loomd"),
            Path::new("/tmp/.loom"),
            Path::new("/tmp/.loom/.cache/out.log"),
            Path::new("/tmp/.loom/.cache/err.log"),
        );
        // The service owns the lifecycle: no idle self-exit, and a clean
        // exit is not resurrected (only a crash is).
        assert!(plist.contains("<key>LOOM_IDLE_TIMEOUT_MS</key>"));
        assert!(plist.contains("<key>SuccessfulExit</key>"));
        assert!(!plist.contains("<key>KeepAlive</key>\n  <true/>"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_render_systemd_unit() {
        let unit = render_systemd_unit(Path::new("/usr/local/bin/loomd"), Path::new("/tmp/.loom"));
        assert!(unit.contains("ExecStart=\"/usr/local/bin/loomd\""));
        assert!(unit.contains("LOOM_AGENTS_DIR=/tmp/.loom"));
        // The service owns the lifecycle: no idle self-exit, and a clean
        // exit is not resurrected (only a crash is).
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("LOOM_IDLE_TIMEOUT_MS=0"));
        assert!(!unit.contains("Restart=always"));
    }

    /// A held lock means a daemon owns this directory.
    #[test]
    fn daemon_running_follows_the_lock() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(!daemon_running(dir.path()), "no daemon, no lock");

        let lock = loom_core::lock::acquire(dir.path()).expect("acquire the lock");
        assert!(daemon_running(dir.path()), "the lock holder is a daemon");

        drop(lock);
        assert!(!daemon_running(dir.path()), "the lock is free again");
    }

    /// A socket file without a listener is not a running daemon.
    #[cfg(unix)]
    #[test]
    fn a_stale_socket_file_is_not_a_running_daemon() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join(DAEMON_SOCKET);
        std::fs::write(&socket, b"stale").expect("write a stale socket file");

        assert!(!socket_live(&socket), "a plain file is not a socket");
        assert!(
            !daemon_running(dir.path()),
            "a stale socket is not a daemon"
        );
    }
}
