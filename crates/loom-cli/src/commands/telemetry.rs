//! `loom telemetry` — View and export telemetry metrics.

use anyhow::Result;
use loom_core::config::ConfigReader;

/// Telemetry actions.
#[derive(clap::Subcommand, Debug)]
pub enum TelemetryAction {
    /// Show telemetry metrics snapshot
    Show {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Reset all telemetry counters
    Reset,
}

pub async fn run(loom_dir: &str, action: &TelemetryAction) -> Result<()> {
    let config_reader = ConfigReader::new(loom_dir);
    let _config = config_reader.load()?;

    match action {
        TelemetryAction::Show { json } => {
            // Try to get metrics from daemon first
            let metrics = get_metrics_from_daemon(loom_dir).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&metrics)?);
            } else {
                println!("Telemetry Metrics:");
                println!("==================");
                if let Some(metrics_obj) = metrics.get("metrics").and_then(|v| v.as_object()) {
                    for (key, value) in metrics_obj {
                        println!("  {}: {}", key, value);
                    }
                } else {
                    println!("  (no metrics available)");
                }
            }
        }
        TelemetryAction::Reset => {
            // Reset via daemon
            reset_metrics_in_daemon(loom_dir).await?;
            println!("Telemetry counters reset");
        }
    }
    Ok(())
}

async fn get_metrics_from_daemon(loom_dir: &str) -> Result<serde_json::Value> {
    use loom_ipc::client::send_command;
    use loom_ipc::IpcRequest;
    use std::path::Path;

    let socket_path = Path::new(loom_dir).join("loomd.sock");
    if !socket_path.exists() {
        // Daemon not running, return empty
        return Ok(serde_json::json!({}));
    }

    let req = IpcRequest::new("telemetry.metrics").with_params(serde_json::json!({}));
    let resp = send_command(socket_path.to_str().unwrap(), req).await?;

    if resp.status == "ok" {
        Ok(resp.data.unwrap_or(serde_json::json!({})))
    } else {
        Ok(serde_json::json!({}))
    }
}

async fn reset_metrics_in_daemon(loom_dir: &str) -> Result<()> {
    use loom_ipc::client::send_command;
    use loom_ipc::IpcRequest;
    use std::path::Path;

    let socket_path = Path::new(loom_dir).join("loomd.sock");
    if !socket_path.exists() {
        anyhow::bail!("Daemon not running");
    }

    let req = IpcRequest::new("telemetry.reset").with_params(serde_json::json!({}));
    let resp = send_command(socket_path.to_str().unwrap(), req).await?;

    if resp.status != "ok" {
        anyhow::bail!("Failed to reset: {}", resp.error.unwrap_or_default());
    }
    Ok(())
}
