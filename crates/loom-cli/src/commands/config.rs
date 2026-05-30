//! `loom config` subcommands — inspect and validate merged config.

use crate::cli::ConfigAction;
use loom_core::config::ConfigReader;

pub async fn run(loom_dir: &str, action: &ConfigAction) -> anyhow::Result<()> {
    match action {
        ConfigAction::Show { json } => cmd_show(loom_dir, *json).await,
        ConfigAction::Validate => cmd_validate(loom_dir).await,
    }
}

async fn cmd_show(loom_dir: &str, json: bool) -> anyhow::Result<()> {
    let reader = ConfigReader::new(loom_dir);
    let config = reader.load()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&config)?);
    } else {
        println!("=== Merged Loom Config ===");
        println!("Ecosystem:");
        println!("  Version:         {}", config.ecosystem.version);
        println!("  Enabled:         {}", config.ecosystem.enabled);
        println!(
            "  Conflict:        {}",
            config.ecosystem.conflict_resolution
        );
        println!("  Layers:");
        println!("    Skills:  {}", config.ecosystem.layers.skills);
        println!("    MCP:     {}", config.ecosystem.layers.mcp);
        println!("    Sessions: {}", config.ecosystem.layers.sessions);
        println!("    Auth:    {}", config.ecosystem.layers.auth);
        println!("    Hooks:   {}", config.ecosystem.layers.hooks);
        println!("    Workflow: {}", config.ecosystem.layers.workflow);

        if let Some(acp) = &config.acp_registry_url {
            println!("ACP Registry URL: {}", acp);
        }

        if let Some(routing) = &config.routing {
            println!("Routing:");
            println!("  Strategy:    {:?}", routing.strategy);
            println!("  Low max:     {}", routing.complexity_thresholds.low_max);
            println!("  High min:    {}", routing.complexity_thresholds.high_min);
            if let Some(tier) = &routing.default_tier {
                println!("  Default tier: {:?}", tier);
            }
            if let Some(provider) = &routing.default_provider {
                println!("  Default provider: {provider}");
            }
            if let Some(model) = &routing.default_model {
                println!("  Default model: {model}");
            }
        } else {
            println!("Routing: (defaults)");
        }
    }
    Ok(())
}

async fn cmd_validate(loom_dir: &str) -> anyhow::Result<()> {
    let reader = ConfigReader::new(loom_dir);
    match reader.load() {
        Ok(config) => {
            println!("✓ Config is valid");

            // Warn if ecosystem is disabled
            if !config.ecosystem.enabled {
                println!("  ⚠ Ecosystem is disabled (ecosystem.enabled = false)");
            }

            // Check for common issues
            if let Some(routing) = &config.routing {
                if routing.complexity_thresholds.low_max >= routing.complexity_thresholds.high_min
                {
                    println!(
                        "  ⚠ Routing thresholds: low_max ({}) >= high_min ({}); all traffic will route to one tier",
                        routing.complexity_thresholds.low_max,
                        routing.complexity_thresholds.high_min
                    );
                }
            }

            // Check ACP URL is valid if set
            if let Some(url) = &config.acp_registry_url {
                if !url.starts_with("http") {
                    println!("  ⚠ ACP registry URL doesn't start with http: {url}");
                }
            }

            println!();
            println!("To view the full merged config: loom config show");
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("Config validation failed: {e}");
        }
    }
}
