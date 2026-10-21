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
    /// Reset the DAEMON's in-memory telemetry counters
    ///
    /// This does not touch the usage ledger at {loom_dir}/telemetry/usage.db —
    /// the metered runs stay. Delete that file to clear the ledger.
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
            println!(
                "Telemetry counters reset (daemon counters only — the usage ledger at \
                 {loom_dir}/telemetry/usage.db is untouched)"
            );
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

    print!("{}", render_usage(&payload));
    Ok(())
}

/// Render the ledger for humans. Pure: takes the JSON payload, returns the
/// text — so the rendering can be asserted on without capturing stdout.
fn render_usage(payload: &serde_json::Value) -> String {
    let mut out = String::new();
    let totals = &payload["totals"];
    out.push_str("Usage Ledger\n");
    out.push_str("============\n");
    out.push_str(&format!(
        "  Runs: {} ({} reported, {} with token counts, {} unavailable)\n",
        totals["runs"],
        totals["reported_runs"],
        totals["measured_runs"],
        totals["unavailable_runs"]
    ));
    // The token sum is absent when NO run reported a token count. That is
    // true even when runs were reported (a run that reported only a cost
    // is a reported run, not a token measurement) — so never print a
    // zero that reads like a measurement.
    out.push_str(&format!(
        "  Measured tokens: {}\n",
        token_pair(
            totals["input_tokens"].as_u64(),
            totals["output_tokens"].as_u64(),
        )
    ));
    out.push_str(&format!(
        "  Measured cost:   {}\n",
        money(totals["cost_usd"].as_f64())
    ));
    let est_input = totals["est_input_tokens"].as_u64().unwrap_or(0);
    let est_output = totals["est_output_tokens"].as_u64().unwrap_or(0);
    if est_input == 0 {
        out.push_str("  Estimated cost:  unavailable (no forecast recorded)\n");
    } else {
        out.push_str(&format!(
            "  Estimated tokens: {est_input} in / {est_output} out (chars/4 forecast)\n"
        ));
        match totals["est_cost_usd"].as_f64() {
            Some(cost) => out.push_str(&format!(
                "  Estimated cost:  ${cost:.4} (chars/4 forecast — not a measurement)\n"
            )),
            None => {
                out.push_str("  Estimated cost:  unavailable (catalog does not price this model)\n")
            }
        }
    }

    if let Some(by_tool) = payload["by_tool"].as_array() {
        if !by_tool.is_empty() {
            out.push('\n');
            out.push_str("Per tool:\n");
            for tool in by_tool {
                let est = match tool["est_cost_usd"].as_f64() {
                    Some(cost) => format!("${cost:.4}"),
                    None => "n/a".to_string(),
                };
                out.push_str(&format!(
                    "  {:<12} runs={} reported={} with_tokens={} unavailable={} tokens={} measured={} est={}\n",
                    tool["tool"].as_str().unwrap_or("?"),
                    tool["runs"],
                    tool["reported_runs"],
                    tool["measured_runs"],
                    tool["unavailable_runs"],
                    // Same rule as the totals line: a tool whose runs all
                    // reported no token count reads "unavailable", not "0/0".
                    token_pair(
                        tool["input_tokens"].as_u64(),
                        tool["output_tokens"].as_u64(),
                    ),
                    money(tool["cost_usd"].as_f64()),
                    est,
                ));
            }
        }
    }

    if let Some(recent) = payload["recent"].as_array() {
        if !recent.is_empty() {
            out.push('\n');
            out.push_str("Recent runs:\n");
            for run in recent {
                let tokens = match (run["input_tokens"].as_u64(), run["output_tokens"].as_u64()) {
                    (Some(input), Some(output)) => format!("{input} in / {output} out"),
                    _ => "no token count reported".to_string(),
                };
                let cost = match run["cost_usd"].as_f64() {
                    Some(cost) => format!("${cost:.4}"),
                    None => "cost unavailable".to_string(),
                };
                out.push_str(&format!(
                    "  {} [{}] {} {} — {} ({})\n",
                    run["recorded_at"].as_str().unwrap_or("?"),
                    run["source"].as_str().unwrap_or("?"),
                    run["tool"].as_str().unwrap_or("?"),
                    run["model"].as_str().unwrap_or("?"),
                    tokens,
                    cost
                ));
            }
        }
    }

    out
}

/// Render a token pair for humans. A missing leg reads "unavailable" —
/// never "0", which would read as a measurement.
fn token_pair(input: Option<u64>, output: Option<u64>) -> String {
    match (input, output) {
        (Some(input), Some(output)) => format!("{input} in / {output} out"),
        (Some(input), None) => {
            format!("{input} in / unavailable (no run reported an output count)")
        }
        (None, Some(output)) => {
            format!("unavailable (no run reported an input count) / {output} out")
        }
        (None, None) => "unavailable (no run reported a token count)".to_string(),
    }
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
fn usage_json(loom_dir: &str, limit: usize) -> Result<serde_json::Value> {
    let meter = UsageMeter::new(loom_dir)?;
    Ok(serde_json::json!({
        "totals": meter.totals()?,
        "by_tool": meter.by_tool()?,
        "recent": meter.recent(limit)?,
    }))
}

async fn get_metrics_from_daemon(loom_dir: &str) -> Result<serde_json::Value> {
    use loom_ipc::IpcRequest;
    use std::path::Path;

    let socket_path = Path::new(loom_dir).join("loomd.sock");
    if !socket_path.exists() {
        // Daemon not running, return empty
        return Ok(serde_json::json!({}));
    }

    let req = IpcRequest::new("telemetry.metrics").with_params(serde_json::json!({}));
    // A telemetry probe is single-shot (no backoff, no daemon start) but names
    // the cause and the remedy like every other command.
    let resp = crate::ipc_client::probe_daemon(Path::new(loom_dir), req)
        .await
        .map_err(|e| crate::ipc_client::stale_socket_advice(&e.to_string()))?;

    if resp.status == "ok" {
        Ok(resp.data.unwrap_or(serde_json::json!({})))
    } else {
        Ok(serde_json::json!({}))
    }
}

async fn reset_metrics_in_daemon(loom_dir: &str) -> Result<()> {
    use loom_ipc::IpcRequest;
    use std::path::Path;

    let socket_path = Path::new(loom_dir).join("loomd.sock");
    if !socket_path.exists() {
        anyhow::bail!("Daemon not running");
    }

    let req = IpcRequest::new("telemetry.reset").with_params(serde_json::json!({}));
    // A telemetry probe is single-shot (no backoff, no daemon start) but names
    // the cause and the remedy like every other command.
    let resp = crate::ipc_client::probe_daemon(Path::new(loom_dir), req)
        .await
        .map_err(|e| crate::ipc_client::stale_socket_advice(&e.to_string()))?;

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
        assert_eq!(totals["reported_runs"], 1);
        assert_eq!(totals["measured_runs"], 1);
        assert_eq!(totals["unavailable_runs"], 1);
        assert_eq!(totals["input_tokens"], 1200);
        assert_eq!(totals["output_tokens"], 300);
        // The measured cost is computed from the measured count.
        let measured_cost = totals["cost_usd"].as_f64().unwrap();
        assert!((measured_cost - (1.2 * 0.003 + 0.3 * 0.015)).abs() < 1e-9); // The forecast leg is separate and labelled by its own field.
        assert!(totals["est_cost_usd"].as_f64().unwrap() > 0.0);

        let by_tool = json["by_tool"].as_array().unwrap();
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool[0]["tool"], "acp");
        assert_eq!(by_tool[0]["runs"], 2);
        assert_eq!(by_tool[0]["reported_runs"], 1);
        assert_eq!(by_tool[0]["measured_runs"], 1);

        let recent = json["recent"].as_array().unwrap();
        assert_eq!(recent.len(), 2);
        // Newest first: the unavailable run, then the measured one.
        assert_eq!(recent[0]["source"], "unavailable");
        assert!(recent[0]["input_tokens"].is_null());
        assert_eq!(recent[1]["source"], "measured");
        assert_eq!(recent[1]["input_tokens"], 1200);
    }

    /// A run whose source reported ONLY a context size and a cost did not
    /// report a token count. The ledger must not print "0 in / 0 out" for
    /// it — that reads as a measurement of zero tokens when the truth is
    /// "we were never told".
    #[test]
    fn context_and_cost_only_run_does_not_render_zero_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        let meter = UsageMeter::new(loom_dir).unwrap();

        // The stable ACP v1 `usage_update` shape: a context-window size and
        // a cost, but no per-turn token count.
        meter
            .record_run(
                "run-acp",
                "acp",
                "claude-sonnet-4",
                loom_core::UsageCounts {
                    input_tokens: None,
                    output_tokens: None,
                    context_tokens: Some(4321),
                    cost_usd: Some(0.0123),
                },
                "acp:session-context-cost",
                0,
                None,
            )
            .unwrap();

        let json = usage_json(loom_dir, 20).unwrap();
        let totals = &json["totals"];
        // The run was reported (the source told us something)...
        assert_eq!(totals["runs"], 1);
        assert_eq!(totals["reported_runs"], 1);
        // ...but it is not a token measurement.
        assert_eq!(totals["measured_runs"], 0);
        // The token aggregate stays absent, NOT zero.
        assert!(totals["input_tokens"].is_null());
        assert!(totals["output_tokens"].is_null());
        // The cost is real.
        assert!((totals["cost_usd"].as_f64().unwrap() - 0.0123).abs() < 1e-9);

        let rendered = render_usage(&json);
        assert!(
            !rendered.contains("0 in / 0 out"),
            "ledger must not present an unreported count as zero:\n{rendered}"
        );
        assert!(
            rendered.contains("Measured tokens: unavailable (no run reported a token count)"),
            "ledger must say the token count is unavailable:\n{rendered}"
        );
        // The per-tool line must not print a zero pair either.
        assert!(
            !rendered.contains("tokens=0/0"),
            "per-tool line must not present an unreported count as zero:\n{rendered}"
        );
        assert!(
            rendered.contains("tokens=unavailable"),
            "per-tool line must say the token count is unavailable:\n{rendered}"
        );
    }
}
