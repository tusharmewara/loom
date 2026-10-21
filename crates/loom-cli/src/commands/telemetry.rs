//! `loom telemetry` — View and export telemetry metrics.

use anyhow::Result;
use loom_core::config::ConfigReader;
use loom_telemetry::UsageMeter;

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
    /// Show metered token usage and cost per run and per tool
    Usage {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Show this many recent runs
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
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
        TelemetryAction::Usage { json, limit } => {
            show_usage(loom_dir, *json, *limit)?;
        }
    }
    Ok(())
}

/// Print the metered usage ledger: totals, per-tool breakdown, recent runs.
fn show_usage(loom_dir: &str, json: bool, limit: usize) -> Result<()> {
    let payload = usage_json(loom_dir, limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&payload)?);
        return Ok(());
    }

    let totals = &payload["totals"];
    println!("Usage Ledger");
    println!("============");
    println!(
        "  Runs: {} ({} measured, {} unavailable)",
        totals["runs"], totals["measured_runs"], totals["unavailable_runs"]
    );
    let measured_runs = totals["measured_runs"].as_u64().unwrap_or(0);
    if measured_runs == 0 {
        println!("  Measured tokens: unavailable (no measured run)");
    } else {
        println!(
            "  Measured tokens: {} in / {} out",
            totals["input_tokens"], totals["output_tokens"]
        );
    }
    println!("  Measured cost:   {}", money(totals["cost_usd"].as_f64()));
    let est_input = totals["est_input_tokens"].as_u64().unwrap_or(0);
    let est_output = totals["est_output_tokens"].as_u64().unwrap_or(0);
    if est_input == 0 {
        println!("  Estimated cost:  unavailable (no forecast recorded)");
    } else {
        println!("  Estimated tokens: {est_input} in / {est_output} out (chars/4 forecast)");
        match totals["est_cost_usd"].as_f64() {
            Some(cost) => println!(
                "  Estimated cost:  ${cost:.4} (chars/4 forecast — not a measurement)"
            ),
            None => println!("  Estimated cost:  unavailable (catalog does not price this model)"),
        }
    }

    if let Some(by_tool) = payload["by_tool"].as_array() {
        if !by_tool.is_empty() {
            println!();
            println!("Per tool:");
            for tool in by_tool {
                let est = match tool["est_cost_usd"].as_f64() {
                    Some(cost) => format!("${cost:.4}"),
                    None => "n/a".to_string(),
                };
                println!(
                    "  {:<12} runs={} measured={} unavailable={} tokens={}/{} measured={} est={}",
                    tool["tool"].as_str().unwrap_or("?"),
                    tool["runs"], tool["measured_runs"], tool["unavailable_runs"],
                    tool["input_tokens"], tool["output_tokens"],
                    money(tool["cost_usd"].as_f64()),
                    est,
                );
            }
        }
    }

    if let Some(recent) = payload["recent"].as_array() {
        if !recent.is_empty() {
            println!();
            println!("Recent runs:");
            for run in recent {
                let tokens = match (run["input_tokens"].as_u64(), run["output_tokens"].as_u64()) {
                    (Some(input), Some(output)) => format!("{input} in / {output} out"),
                    _ => "no token count reported".to_string(),
                };
                let cost = match run["cost_usd"].as_f64() {
                    Some(cost) => format!("${cost:.4}"),
                    None => "cost unavailable".to_string(),
                };
                println!(
                    "  {} [{}] {} {} — {} ({})",
                    run["recorded_at"].as_str().unwrap_or("?"),
                    run["source"].as_str().unwrap_or("?"),
                    run["tool"].as_str().unwrap_or("?"),
                    run["model"].as_str().unwrap_or("?"),
                    tokens,
                    cost
                );
            }
        }
    }

    Ok(())
}

/// Render a cost for humans: a real number, or "unavailable" when the
/// ledger has no cost for that scope. Never "$0.0000" for "we do not know".
fn money(cost: Option<f64>) -> String {
    match cost {
        Some(cost) => format!("${cost:.4}"),
        None => "unavailable".to_string(),
    }
}

/// The ledger as JSON: totals, per-tool breakdown, recent runs.
fn usage_json(loom_dir: &str, limit: usize) -> Result<serde_json::Value> {    let meter = UsageMeter::new(loom_dir)?;
    Ok(serde_json::json!({
        "totals": meter.totals()?,
        "by_tool": meter.by_tool()?,
        "recent": meter.recent(limit)?,
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_telemetry::UsageMeter;

    /// The rendered ledger must say which numbers are measured and which are
    /// a forecast — a reader must never mistake one for the other.
    #[test]
    fn usage_output_distinguishes_measured_from_estimated() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        let meter = UsageMeter::new(loom_dir).unwrap();

        // A run the provider reported, and a run it reported nothing for.
        meter
            .record_run(
                "run-1",
                "acp",
                "claude-sonnet-4",
                loom_core::UsageCounts {
                    input_tokens: Some(1200),
                    output_tokens: Some(300),
                    context_tokens: None,
                    cost_usd: None,
                },
                "provider",
                8000,
                Some((0.003, 0.015)),
            )
            .unwrap();
        meter
            .record_run(
                "run-2",
                "acp",
                "unknown-agent",
                loom_core::UsageCounts::default(),
                "",
                4000,
                None,
            )
            .unwrap();

        let json = usage_json(loom_dir, 20).unwrap();
        let totals = &json["totals"];
        assert_eq!(totals["runs"], 2);
        assert_eq!(totals["measured_runs"], 1);
        assert_eq!(totals["unavailable_runs"], 1);
        assert_eq!(totals["input_tokens"], 1200);
        assert_eq!(totals["output_tokens"], 300);
        // The measured cost is computed from the measured count.
        let measured_cost = totals["cost_usd"].as_f64().unwrap();
        assert!((measured_cost - (1.2 * 0.003 + 0.3 * 0.015)).abs() < 1e-9);        // The forecast leg is separate and labelled by its own field.
        assert!(totals["est_cost_usd"].as_f64().unwrap() > 0.0);

        let by_tool = json["by_tool"].as_array().unwrap();
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool[0]["tool"], "acp");
        assert_eq!(by_tool[0]["runs"], 2);
        assert_eq!(by_tool[0]["measured_runs"], 1);

        let recent = json["recent"].as_array().unwrap();
        assert_eq!(recent.len(), 2);
        // Newest first: the unavailable run, then the measured one.
        assert_eq!(recent[0]["source"], "unavailable");
        assert!(recent[0]["input_tokens"].is_null());
        assert_eq!(recent[1]["source"], "measured");
        assert_eq!(recent[1]["input_tokens"], 1200);
    }
}
