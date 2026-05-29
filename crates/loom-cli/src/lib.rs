mod cli;
mod commands;
mod ipc_client;

use clap::Parser;

pub use cli::{
    AuthAction, Cli, Command, DaemonAction, HooksAction, McpAction, MemoryAction, RoutingAction,
    SessionsAction, SkillsAction, WorkflowAction,
};

/// Resolve the default ecosystem directory (~/.loom/).
pub fn loom_dir() -> String {
    let home = std::env::var("HOME").expect("HOME environment variable not set");
    format!("{}/.loom", home)
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
    cli.execute().await
}
