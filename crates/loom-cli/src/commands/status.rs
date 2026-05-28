use anyhow::Result;
use comfy_table::Table;
use loom_discovery::DiscoveryEngine;
use loom_ipc::{client::send_command, IpcRequest};

const DAEMON_SOCKET: &str = "loomd.sock";

/// `loom status`
///
/// Checks daemon health via IPC and displays resource counts from live registries
/// (skills, mcp, tools) so counts are accurate immediately — no need to wait for
/// the snapshot to sync.
pub async fn run(loom_dir: &str, json: bool) -> Result<()> {
    let agents_path = std::path::Path::new(loom_dir);
    if !agents_path.exists() {
        anyhow::bail!("Ecosystem not initialized at ~/.loom/. Run 'loom init' first.");
    }

    // ── Check daemon health ─────────────────────────────────────
    let socket_path = format!("{loom_dir}/{DAEMON_SOCKET}");
    let (daemon_ok, daemon_status, daemon_error, daemon_data) =
        match send_command(&socket_path, IpcRequest::new("health.check")).await {
            Ok(resp) if resp.status == "ok" => (true, resp.status, None, resp.data),
            Ok(resp) => (false, resp.status, resp.error, resp.data),
            Err(e) => (false, "unreachable".into(), Some(e.to_string()), None),
        };

    // ── Live resource counts (source of truth, not snapshot) ─────
    let engine = DiscoveryEngine::new(loom_dir);
    let counts = engine.live_counts();
    let total: usize = counts.values().sum();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "status",
                "loom_dir": loom_dir,
                "daemon": {
                    "reachable": daemon_ok,
                    "status": daemon_status,
                    "socket_path": socket_path,
                    "error": daemon_error,
                    "data": daemon_data,
                },
                "resources": {
                    "counts": counts,
                    "total": total,
                },
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
    // Always show the canonical rows even when empty
    for name in &["skills", "mcp_servers", "sessions", "providers", "tools"] {
        if !counts.contains_key(*name) {
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

    #[tokio::test]
    async fn test_status_without_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        init::run(false, false, loom_dir).await.unwrap();
        run(loom_dir, false).await.unwrap();
    }

    #[tokio::test]
    async fn test_status_proceeds_with_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        run(loom_dir, false).await.unwrap();
    }
}
