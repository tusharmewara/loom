use anyhow::{Context, Result};
use comfy_table::Table;
use loom_discovery::registry::SnapshotDb;
use loom_ipc::{client::send_command, IpcRequest};
use std::path::{Path, PathBuf};
use std::process::Command;

const DAEMON_SOCKET: &str = "loomd.sock";

#[derive(Clone, Copy)]
enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

impl CheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

struct CheckRow {
    name: String,
    status: CheckStatus,
    detail: String,
}

impl CheckRow {
    fn new(name: impl Into<String>, status: CheckStatus, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            detail: detail.into(),
        }
    }
}

pub async fn run(loom_dir: &str, json: bool, fix: bool) -> Result<()> {
    let mut rows: Vec<CheckRow> = Vec::new();
    let agents_path = Path::new(loom_dir);

    if !agents_path.exists() && fix {
        // Auto-initialize missing agents directory
        crate::commands::init::run(false, false, loom_dir).await?;
        rows.push(CheckRow::new(
            "Agents root",
            CheckStatus::Pass,
            format!("Created {}", agents_path.display()),
        ));
    } else if !agents_path.exists() {
        rows.push(CheckRow::new(
            "Agents root",
            CheckStatus::Fail,
            format!(
                "Missing {}. Run `loom init --import` first, or use `--fix`.",
                agents_path.display()
            ),
        ));
        if json {
            print_json_report(loom_dir, &rows);
        } else {
            print_report(&rows);
        }
        anyhow::bail!("doctor found 1 failing check");
    }

    rows.push(CheckRow::new(
        "Agents root",
        CheckStatus::Pass,
        format!("Found {}", agents_path.display()),
    ));

    // Core structure checks
    for dir_name in [
        "config", "skills", "mcp", "sessions", "auth", "hooks", "tools", ".cache",
    ] {
        let p = agents_path.join(dir_name);
        if p.is_dir() {
            rows.push(CheckRow::new(
                format!("Directory: {dir_name}"),
                CheckStatus::Pass,
                p.display().to_string(),
            ));
        } else if fix {
            std::fs::create_dir_all(&p)?;
            rows.push(CheckRow::new(
                format!("Directory: {dir_name}"),
                CheckStatus::Pass,
                format!("Created {}", p.display()),
            ));
        } else {
            rows.push(CheckRow::new(
                format!("Directory: {dir_name}"),
                CheckStatus::Fail,
                format!("Missing {}", p.display()),
            ));
        }
    }

    for file_name in ["config.toml", "VERSION"] {
        let p = agents_path.join(file_name);
        if p.is_file() {
            rows.push(CheckRow::new(
                format!("File: {file_name}"),
                CheckStatus::Pass,
                p.display().to_string(),
            ));
        } else if fix {
            crate::commands::init::run(false, false, loom_dir).await?;
            rows.push(CheckRow::new(
                format!("File: {file_name}"),
                CheckStatus::Pass,
                format!("Restored {}", p.display()),
            ));
        } else {
            rows.push(CheckRow::new(
                format!("File: {file_name}"),
                CheckStatus::Fail,
                format!("Missing {}", p.display()),
            ));
        }
    }

    // Persistence write checks
    for writable_dir in ["", ".cache", "sessions", "auth", "tools"] {
        let p = if writable_dir.is_empty() {
            agents_path.to_path_buf()
        } else {
            agents_path.join(writable_dir)
        };
        let check_name = if writable_dir.is_empty() {
            "Write test: agents root".to_string()
        } else {
            format!("Write test: {writable_dir}")
        };
        match write_probe(&p) {
            Ok(()) => rows.push(CheckRow::new(check_name, CheckStatus::Pass, p.display().to_string())),
            Err(e) => rows.push(CheckRow::new(check_name, CheckStatus::Fail, format!("{e}"))),
        }
    }

    // Snapshot health
    let snapshot_db = SnapshotDb::new(loom_dir);
    match snapshot_db
        .initialize_schema()
        .context("initialize_schema failed")
        .and_then(|_| snapshot_db.count_resources().context("count_resources failed"))
    {
        Ok(counts) => rows.push(CheckRow::new(
            "Snapshot DB",
            CheckStatus::Pass,
            format!("OK ({} resource buckets)", counts.len()),
        )),
        Err(e) => rows.push(CheckRow::new(
            "Snapshot DB",
            CheckStatus::Fail,
            format!("SQLite health check failed: {e}"),
        )),
    }

    // Daemon socket + IPC health
    let socket_path = agents_path.join(DAEMON_SOCKET);
    if socket_path.exists() {
        let socket = socket_path.display().to_string();
        match send_command(&socket, IpcRequest::new("health.check")).await {
            Ok(resp) if resp.status == "ok" => rows.push(CheckRow::new(
                "Daemon health",
                CheckStatus::Pass,
                format!("Daemon reachable at {socket}"),
            )),
            Ok(resp) => rows.push(CheckRow::new(
                "Daemon health",
                CheckStatus::Fail,
                format!(
                    "Daemon responded with status={} error={}",
                    resp.status,
                    resp.error.unwrap_or_else(|| "none".to_string())
                ),
            )),
            Err(e) => rows.push(CheckRow::new(
                "Daemon health",
                CheckStatus::Fail,
                format!("Socket exists but IPC failed: {e}"),
            )),
        }
    } else {
        rows.push(CheckRow::new(
            "Daemon health",
            CheckStatus::Warn,
            format!(
                "{} is not present. Run `loom daemon start` or `loom daemon install-service`.",
                socket_path.display()
            ),
        ));
    }

    // Service registration check
    let (service_status, service_detail) = service_status_check();
    rows.push(CheckRow::new(
        "Daemon service",
        service_status,
        service_detail,
    ));

    if json {
        print_json_report(loom_dir, &rows);
    } else {
        print_report(&rows);
    }

    let failures = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Fail))
        .count();
    if failures > 0 {
        anyhow::bail!("doctor found {failures} failing checks");
    }
    Ok(())
}

fn print_report(rows: &[CheckRow]) {
    let mut table = Table::new();
    table.set_header(vec!["Check", "Status", "Details"]);
    for row in rows {
        table.add_row(vec![row.name.as_str(), row.status.as_str(), row.detail.as_str()]);
    }

    let passes = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Pass))
        .count();
    let warns = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Warn))
        .count();
    let fails = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Fail))
        .count();

    println!("{table}");
    println!();
    println!("Doctor summary: {passes} pass | {warns} warn | {fails} fail");
}

fn print_json_report(loom_dir: &str, rows: &[CheckRow]) {
    let passes = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Pass))
        .count();
    let warns = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Warn))
        .count();
    let fails = rows
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Fail))
        .count();
    let checks: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "check": r.name,
                "status": r.status.as_str(),
                "details": r.detail
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "command": "doctor",
            "loom_dir": loom_dir,
            "summary": {
                "pass": passes,
                "warn": warns,
                "fail": fails
            },
            "checks": checks
        }))
        .unwrap_or_else(|_| "{\"error\":\"failed to render doctor json\"}".to_string())
    );
}

fn write_probe(dir: &Path) -> Result<()> {
    if !dir.exists() {
        anyhow::bail!("{} does not exist", dir.display());
    }
    if !dir.is_dir() {
        anyhow::bail!("{} is not a directory", dir.display());
    }

    let probe = dir.join(format!(
        ".loom-doctor-write-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));

    std::fs::write(&probe, b"loom-doctor-write-check")
        .with_context(|| format!("write failed at {}", dir.display()))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_status_check() -> (CheckStatus, String) {
    let home = match home_dir() {
        Some(h) => h,
        None => return (CheckStatus::Warn, "HOME not set; cannot check launchd".to_string()),
    };
    let plist = home
        .join("Library")
        .join("LaunchAgents")
        .join("com.loom.loomd.plist");
    if !plist.exists() {
        return (
            CheckStatus::Warn,
            format!(
                "No launchd service at {}. Run `loom daemon install-service`.",
                plist.display()
            ),
        );
    }

    let uid = match current_uid() {
        Ok(u) => u,
        Err(e) => return (CheckStatus::Warn, format!("Cannot determine UID: {e}")),
    };
    let service_target = format!("gui/{uid}/com.loom.loomd");
    match Command::new("launchctl")
        .arg("print")
        .arg(&service_target)
        .status()
    {
        Ok(s) if s.success() => (
            CheckStatus::Pass,
            format!(
                "launchd service installed and loaded ({})",
                plist.display()
            ),
        ),
        Ok(_) => (
            CheckStatus::Warn,
            "launchd plist exists but service is not loaded. Try `loom daemon install-service`.".to_string(),
        ),
        Err(e) => (
            CheckStatus::Warn,
            format!("Failed to query launchctl service state: {e}"),
        ),
    }
}

#[cfg(target_os = "linux")]
fn service_status_check() -> (CheckStatus, String) {
    let home = match home_dir() {
        Some(h) => h,
        None => return (CheckStatus::Warn, "HOME not set; cannot check systemd".to_string()),
    };
    let unit = home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("loomd.service");
    if !unit.exists() {
        return (
            CheckStatus::Warn,
            format!(
                "No systemd user unit at {}. Run `loom daemon install-service`.",
                unit.display()
            ),
        );
    }

    let enabled = Command::new("systemctl")
        .arg("--user")
        .arg("is-enabled")
        .arg("loomd.service")
        .output();
    let active = Command::new("systemctl")
        .arg("--user")
        .arg("is-active")
        .arg("loomd.service")
        .output();

    match (enabled, active) {
        (Ok(e), Ok(a)) => {
            let enabled_str = String::from_utf8_lossy(&e.stdout).trim().to_string();
            let active_str = String::from_utf8_lossy(&a.stdout).trim().to_string();
            if e.status.success() && a.status.success() {
                (
                    CheckStatus::Pass,
                    format!("systemd user service enabled={enabled_str} active={active_str}"),
                )
            } else {
                (
                    CheckStatus::Warn,
                    format!(
                        "systemd service installed but not healthy (enabled={enabled_str}, active={active_str})."
                    ),
                )
            }
        }
        (Err(e), _) | (_, Err(e)) => (
            CheckStatus::Warn,
            format!("Failed to query systemctl --user: {e}"),
        ),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn service_status_check() -> (CheckStatus, String) {
    (
        CheckStatus::Warn,
        "Service diagnostics unsupported on this OS".to_string(),
    )
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(target_os = "macos")]
fn current_uid() -> Result<String> {
    let out = Command::new("id").arg("-u").output()?;
    if !out.status.success() {
        anyhow::bail!("`id -u` failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init;

    #[tokio::test]
    async fn test_doctor_fails_when_loom_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().join("missing");
        let err = run(loom_dir.to_str().unwrap(), false, false).await.unwrap_err();
        assert!(err.to_string().contains("failing check"));
    }

    #[tokio::test]
    async fn test_doctor_initialized_dir_has_no_failures() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        init::run(false, false, loom_dir).await.unwrap();
        let result = run(loom_dir, false, false).await;
        assert!(result.is_ok(), "doctor should pass, got: {result:?}");
    }

    #[tokio::test]
    async fn test_doctor_fix_creates_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        // Run doctor with --fix on a non-initialized path
        let result = run(loom_dir, false, true).await;
        assert!(result.is_ok(), "doctor --fix should pass, got: {result:?}");
        // Verify the agents root was created
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("skills").is_dir());
        assert!(dir.path().join("mcp").is_dir());
    }
}
