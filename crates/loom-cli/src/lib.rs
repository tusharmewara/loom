mod cli;
mod commands;
mod ipc_client;
mod service;
mod version_check;

use clap::Parser;
use std::path::Path;
use std::time::SystemTime;

pub use cli::{
    AcpAction, AstAction, AuthAction, Cli, Command, DaemonAction, HooksAction, IssuesAction,
    McpAction, MemoryAction, RoutingAction, RulesAction, SessionsAction, SkillsAction,
    WorkflowAction,
};
pub use commands::telemetry::TelemetryAction;

/// Resolve the default ecosystem directory (~/.loom/).
pub fn loom_dir() -> String {
    loom_core::loom_dir()
}

/// Entry point for the loom CLI binary.
pub async fn run() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();

    let cli = Cli::parse();

    if !cli.no_update {
        check_version_background(&loom_dir());
    }

    cli.execute().await
}

/// Check for a newer version at most once per 24 hours, printing a notice
/// to stderr if one is available. Runs synchronously but is lightweight
/// when the cache is fresh.
fn check_version_background(loom_dir: &str) {
    let cache_dir = Path::new(loom_dir).join(".cache");
    let marker = cache_dir.join("version_check");

    // Read cached timestamp
    let should_check = match std::fs::read_to_string(&marker) {
        Ok(ts) => {
            let epoch: u64 = ts.trim().parse().unwrap_or(0);
            let elapsed = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            elapsed.saturating_sub(epoch) > 86400
        }
        Err(_) => true,
    };

    if !should_check {
        return;
    }

    // Write timestamp first to avoid re-checking on every failed attempt
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = std::fs::write(&marker, now.to_string());

    // Quick GitHub API check
    let url = "https://api.github.com/repos/tmewara/loom/releases/latest";
    match ureq::get(url)
        .set("User-Agent", "loom/0.1")
        .set("Accept", "application/vnd.github.v3+json")
        .call()
    {
        Ok(resp) => {
            if let Ok(body) = resp.into_string() {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                    if let Some(tag) = json["tag_name"].as_str() {
                        let latest = tag.trim_start_matches('v');
                        let current = env!("CARGO_PKG_VERSION");
                        if latest != current {
                            eprintln!(
                                "  ⚠ New loom version available: v{latest} (current: v{current}). Run `loom self-update` to upgrade."
                            );
                        }
                    }
                }
            }
        }
        Err(_) => {
            // Silently ignore network failures on startup check
        }
    }
}
