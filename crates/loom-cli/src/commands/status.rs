use anyhow::{Context, Result};
use comfy_table::Table;
use loom_discovery::registry::SnapshotDb;
use loom_ipc::{client::send_command, IpcRequest};

/// Default IPC socket path, relative to ~/.agents/.
const DAEMON_SOCKET: &str = "loomd.sock";

/// `loom status`
///
/// Checks daemon health via the IPC Unix socket. Falls back to reading the
/// SQLite snapshot directly when the daemon is not running.
pub async fn run(agents_dir: &str, json: bool) -> Result<()> {
    let agents_path = std::path::Path::new(agents_dir);
    if !agents_path.exists() {
        anyhow::bail!(
            "Ecosystem not initialized at ~/.agents/. Run 'loom init' first."
        );
    }

    // ── Check daemon health via IPC ───────────────────────────────
    let socket_path = format!("{agents_dir}/{DAEMON_SOCKET}");
    let mut daemon_status = "unreachable".to_string();
    let mut daemon_error: Option<String> = None;
    let mut daemon_data: Option<serde_json::Value> = None;
    let daemon_ok = match send_command(&socket_path, IpcRequest::new("health.check")).await {
        Ok(resp) if resp.status == "ok" => {
            daemon_status = "ok".to_string();
            daemon_data = resp.data;
            true
        }
        Ok(resp) => {
            daemon_status = resp.status;
            daemon_error = resp.error;
            false
        }
        Err(e) => {
            daemon_error = Some(e.to_string());
            false
        }
    };

    // ── Read resource snapshot ────────────────────────────────────
    let snapshot = SnapshotDb::new(agents_dir);
    snapshot
        .initialize_schema()
        .context("Failed to initialize snapshot schema")?;
    let counts = snapshot
        .count_resources()
        .context("Failed to read snapshot counts")?;

    let mut total = 0usize;
    for count in counts.values() {
        total += count;
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "status",
                "agents_dir": agents_dir,
                "daemon": {
                    "reachable": daemon_ok,
                    "status": daemon_status,
                    "socket_path": socket_path,
                    "error": daemon_error,
                    "data": daemon_data
                },
                "resources": {
                    "counts": counts,
                    "total": total
                }
            }))?
        );
        return Ok(());
    }

    if daemon_ok {
        println!("✓ Daemon is running (status: {daemon_status})");
        if let Some(ref data) = daemon_data {
            if let Some(uptime) = data.get("uptime_seconds") {
                println!("  Uptime: {uptime}s");
            }
        }
    } else {
        println!("✗ Daemon is not reachable");
        println!("  Socket: {socket_path}");
        if let Some(err) = daemon_error {
            println!("  Reason: {err}");
        } else {
            println!("  Reason: daemon status={daemon_status}");
        }
    }

    let mut table = Table::new();
    table.set_header(vec!["Resource", "Count"]);
    for (resource, count) in &counts {
        table.add_row(vec![resource.as_str(), &count.to_string()]);
    }

    // Always show the key count rows even for empty
    if counts.is_empty() {
        for name in &["skills", "mcp_servers", "sessions", "providers", "tools"] {
            table.add_row(vec![*name, "0"]);
        }
    }

    println!();
    println!("{table}");
    println!("Total ecosystem resources: {total}");

    if !daemon_ok {
        println!();
        println!("Tip: Run 'loomd' to start the daemon for real-time features");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init;
    use loom_discovery::registry::SnapshotDb;

    #[tokio::test]
    async fn test_status_without_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let agents_dir = dir.path().to_str().unwrap();

        // Initialize first
        init::run(false, agents_dir).await.unwrap();

        // Should succeed with daemon-not-reachable + snapshot counts
        run(agents_dir, false).await.unwrap();

        // Snapshot should have schema with empty counts
        let db = SnapshotDb::new(agents_dir);
        let counts = db.count_resources().unwrap();
        assert!(counts.values().all(|c| *c == 0));
    }

    #[tokio::test]
    async fn test_status_proceeds_with_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let agents_dir = dir.path().to_str().unwrap();

        // Should succeed — schema created on the fly
        run(agents_dir, false).await.unwrap();

        // Snapshot should have schema with empty counts
        let db = SnapshotDb::new(agents_dir);
        let counts = db.count_resources().unwrap();
        assert!(counts.values().all(|c| *c == 0));
    }
}
