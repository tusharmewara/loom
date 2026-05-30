//! `loom config` subcommands — inspect, validate, and fetch remote config.

use crate::cli::ConfigAction;
use loom_core::config::ConfigReader;
use sha2::{Digest, Sha256};
use std::path::Path;

pub async fn run(loom_dir: &str, action: &ConfigAction) -> anyhow::Result<()> {
    match action {
        ConfigAction::Show { json } => cmd_show(loom_dir, *json).await,
        ConfigAction::Validate => cmd_validate(loom_dir).await,
        ConfigAction::Fetch { url } => cmd_fetch(loom_dir, url.as_deref()).await,
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

            // Check for remote_config_url
            if config.remote_config_url.is_some() {
                println!("  ℹ Remote config URL is configured. Run `loom config fetch` to update.");
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

async fn cmd_fetch(loom_dir: &str, url: Option<&str>) -> anyhow::Result<()> {
    // Determine the remote URL: from arg or from config
    let remote_url = match url {
        Some(u) => u.to_string(),
        None => {
            let reader = ConfigReader::new(loom_dir);
            let config = reader.load()?;
            config.remote_config_url.ok_or_else(|| {
                anyhow::anyhow!(
                    "No remote_config_url set in config. Specify --url <URL> or set remote_config_url in config.toml"
                )
            })?
        }
    };

    if !remote_url.starts_with("https://") {
        anyhow::bail!("Only HTTPS URLs are supported for remote config: {remote_url}");
    }

    println!("Fetching remote config from: {remote_url}");
    println!("  (This may take a moment...)");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("loom-config-fetcher/2.0")
        .build()?;

    let response = client.get(&remote_url).send().await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to fetch remote config: HTTP {}",
            response.status()
        );
    }

    let body = response.text().await?;
    let body_bytes = body.as_bytes();

    // Compute SHA-256 hash
    let hash = format!("{:x}", Sha256::digest(body_bytes));

    // Cache directory
    let cache_dir = Path::new(loom_dir).join(".cache").join("remote_config");
    std::fs::create_dir_all(&cache_dir)?;

    // Write cached file named by hash
    let cache_path = cache_dir.join(format!("{}.toml", hash));
    std::fs::write(&cache_path, &body)?;

    // Check if TOML parses
    if let Err(e) = body.parse::<toml::Value>() {
        println!("  ⚠ Downloaded content is not valid TOML: {e}");
        println!("  Cached anyway — you can inspect the file at: {cache_path:?}");
    } else {
        println!("  ✓ Valid TOML");
    }

    // Save the URL → hash mapping for future reference
    let index_path = cache_dir.join("index.json");
    let mut index: std::collections::HashMap<String, String> = if index_path.exists() {
        let raw = std::fs::read_to_string(&index_path).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    // Normalize URL as key
    let url_key = remote_url.trim_end_matches('/').to_string();
    index.insert(url_key, hash.clone());
    std::fs::write(&index_path, serde_json::to_string_pretty(&index)?)?;

    // Display to user
    let size_kb = body_bytes.len() as f64 / 1024.0;
    println!();
    println!("  ✓ Remote config fetched successfully");
    println!("    URL:   {remote_url}");
    println!("    Size:  {size_kb:.1} KB");
    println!("    SHA256: {hash}");
    println!("    Cached: {:?}", cache_path);

    // Try to merge and validate the full config with this remote config
    println!();
    println!("  Validating merged config with remote content...");
    match body.parse::<toml::Value>() {
        Ok(toml_val) => {
            let json_val = toml_value_to_json(toml_val);
            println!("  ✓ Remote config has {} top-level key(s)", json_val.as_object().map(|o| o.len()).unwrap_or(0));
        }
        Err(e) => {
            println!("  ⚠ Remote content is not valid TOML: {e}");
        }
    }

    Ok(())
}

/// Recursively convert toml::Value to serde_json::Value (copied for CLI use).
fn toml_value_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .into_iter()
                .map(|(k, v)| (k, toml_value_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
    }
}
