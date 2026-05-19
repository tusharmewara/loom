use crate::cli::DaemonAction;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PID_FILE: &str = "loomd.pid";
const DAEMON_SOCKET: &str = "loomd.sock";
#[cfg(target_os = "macos")]
const SERVICE_LABEL: &str = "com.loom.loomd";

pub fn start_daemon(loom_dir: &Path) -> anyhow::Result<()> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let pid_path = loom_dir.join(".cache").join(PID_FILE);

    if socket_path.exists() {
        println!("loomd already appears to be running ({})", socket_path.display());
        return Ok(());
    }
    std::fs::create_dir_all(loom_dir.join(".cache"))?;

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

fn stop_daemon(loom_dir: &Path) {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let pid_path = loom_dir.join(".cache").join(PID_FILE);

    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
            let _ = std::fs::remove_file(&pid_path);
            let _ = std::fs::remove_file(&socket_path);
            println!("Stopped loomd (pid {pid})");
            return;
        }
    }
    let _ = std::fs::remove_file(&socket_path);
    println!("No pid file found; removed stale socket if present");
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

    let plist = render_launchd_plist(
        SERVICE_LABEL,
        &loomed_bin,
        loom_dir,
        &stdout_log,
        &stderr_log,
    );
    std::fs::write(&plist_path, plist)?;

    let uid = detect_uid()?;
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{SERVICE_LABEL}");

    let _ = Command::new("launchctl")
        .arg("bootout")
        .arg(&target)
        .status();

    let bootstrap = Command::new("launchctl")
        .arg("bootstrap")
        .arg(&domain)
        .arg(&plist_path)
        .status()?;
    if !bootstrap.success() {
        anyhow::bail!(
            "launchctl bootstrap failed for {}. Try: launchctl bootstrap {} {}",
            SERVICE_LABEL,
            domain,
            plist_path.display()
        );
    }

    let _ = Command::new("launchctl")
        .arg("enable")
        .arg(&target)
        .status();
    let _ = Command::new("launchctl")
        .arg("kickstart")
        .arg("-k")
        .arg(&target)
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

    run_systemctl_user(["daemon-reload"])?;
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

    let _ = Command::new("launchctl").arg("bootout").arg(&target).status();
    let _ = Command::new("launchctl").arg("disable").arg(&target).status();

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
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
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
    format!(
        r#"[Unit]
Description=Loom Daemon
After=network.target

[Service]
Type=simple
ExecStart="{}"
Restart=always
RestartSec=2
Environment=LOOM_AGENTS_DIR={}

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
                                    "status": "ok",
                                    "data": data
                                }))?
                            );
                        } else {
                            println!("loomd is running ({})", socket_path.display());
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

    #[cfg(target_os = "linux")]
    #[test]
    fn test_render_systemd_unit() {
        let unit = render_systemd_unit(
            Path::new("/usr/local/bin/loomd"),
            Path::new("/tmp/.loom"),
        );
        assert!(unit.contains("ExecStart=\"/usr/local/bin/loomd\""));
        assert!(unit.contains("LOOM_AGENTS_DIR=/tmp/.loom"));
        assert!(unit.contains("Restart=always"));
    }
}
