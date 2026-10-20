use anyhow::{Context, Result};
use loom_discovery::initializer::SnapshotBuilder;
use loom_discovery::registry::SnapshotDb;
use loom_sessions::SessionManager;
use std::path::Path;

/// `loom init [--import] [--recommended]`
///
/// Creates the `~/.loom/` directory tree (skills, mcp, sessions, auth,
/// hooks, tools, config, .cache), a minimal default `config.toml`, and
/// initializes the SQLite snapshot schema.
///
/// When `--import` is passed, also walks the filesystem to build the
/// initial snapshot from existing resources.
///
/// When `--recommended` is passed, also installs recommended MCP servers
/// (filesystem, github, playwright, agentmemory) into the registry.
pub async fn run(
    _import: bool,
    recommended: bool,
    source: Option<&str>,
    loom_dir: &str,
) -> Result<()> {
    // 1. Create the full directory tree
    let builder = SnapshotBuilder::new(loom_dir);
    builder
        .create_agents_tree()
        .context("Failed to create ~/.loom/ directory tree")?;
    println!("✓ Initialized ecosystem at {loom_dir}/");

    // 2. Initialize the SQLite snapshot schema (idempotent)
    let db = SnapshotDb::new(loom_dir);
    db.initialize_schema().with_context(|| {
        format!(
            "Failed to initialize snapshot database schema at {}/.cache/snapshot.db",
            loom_dir
        )
    })?;

    // 3. Write a default config.toml if one doesn't exist
    let config_path = Path::new(loom_dir).join("config.toml");
    if !config_path.exists() {
        let mut config = loom_core::LoomConfig::default();
        if let Some(url) = source {
            if !url.starts_with("https://") {
                anyhow::bail!(
                    "--source URL must use https:// scheme (got {}); refusing to write insecure remote_config_url",
                    url
                );
            }
            config.remote_config_url = Some(url.to_string());
        }
        let toml_str =
            toml::to_string_pretty(&config).context("Failed to serialize default config")?;
        std::fs::write(&config_path, toml_str)
            .with_context(|| format!("Failed to write {config_path:?}"))?;
        if source.is_some() {
            println!("✓ Created config.toml with remote_config_url = {source:?}");
        } else {
            println!("✓ Created default config.toml");
        }
    } else {
        println!("  config.toml already exists (not overwritten)");
        if let Some(url) = source {
            merge_remote_config_url(&config_path, url)?;
        }
    }

    // 3b. Write example overlay files in config/ if they don't exist
    write_overlay_templates(loom_dir)?;

    // 4. Migrate legacy session JSON files to JSONL layout
    let sessions = SessionManager::new(loom_dir);
    let migrated = sessions.migrate().unwrap_or(0);
    if migrated > 0 {
        println!("✓ Migrated {migrated} legacy session(s) to JSONL layout");
    }

    install_example_hooks(loom_dir)?;

    // 5. Build initial snapshot from existing resources (auto-import)
    builder
        .build_initial()
        .context("Failed to build initial snapshot from existing resources")?;
    println!("✓ Imported existing resources into snapshot");

    // 6. Optionally install recommended MCP servers
    if recommended {
        let n_recommended = loom_mcp::known::recommended().len();
        for server in loom_mcp::known::recommended() {
            let loom_dir_str = loom_dir.to_string();
            let registry = loom_mcp::McpRegistry::new(&loom_dir_str)?;
            if registry.get(server.id).ok().flatten().is_none() {
                let entry = server.to_entry();
                if let Err(e) = registry.upsert(&entry) {
                    eprintln!("  ⚠ Failed to register '{}': {e}", server.id);
                } else {
                    println!("  ✓ Registered MCP server: {} ({})", server.id, server.name);
                }
            }
        }
        println!("✓ Installed {n_recommended} recommended MCP server(s)");
    }

    // 7. If --source was given, fetch the remote config now so the
    //    cache is populated before the user runs any other command.
    if let Some(url) = source {
        println!();
        println!("Fetching remote config from {url}...");
        match crate::commands::config::run(
            loom_dir,
            &crate::cli::ConfigAction::Fetch {
                url: Some(url.to_string()),
            },
        )
        .await
        {
            Ok(()) => {
                println!("✓ Remote config cached. Run `loom config show --extends` to verify.");
            }
            Err(e) => {
                eprintln!(
                    "  ⚠ Failed to fetch remote config: {e}\n  (The URL is saved; retry with `loom config fetch`.)"
                );
            }
        }
    }

    Ok(())
}

/// Merge a `remote_config_url` line into an existing config.toml,
/// preserving all other content.
fn merge_remote_config_url(config_path: &Path, url: &str) -> Result<()> {
    let raw = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {config_path:?}"))?;
    if raw
        .lines()
        .any(|l| l.trim_start().starts_with("remote_config_url"))
    {
        let updated: String = raw
            .lines()
            .map(|l| {
                if l.trim_start().starts_with("remote_config_url") {
                    format!("remote_config_url = \"{url}\"")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(config_path, format!("{updated}\n"))
            .with_context(|| format!("Failed to write {config_path:?}"))?;
        println!("  ↻ Updated remote_config_url in existing config.toml");
    } else {
        let updated = format!("remote_config_url = \"{url}\"\n\n{raw}");
        std::fs::write(config_path, updated)
            .with_context(|| format!("Failed to write {config_path:?}"))?;
        println!("  ↻ Inserted remote_config_url into existing config.toml");
    }
    Ok(())
}

fn install_example_hooks(loom_dir: &str) -> Result<()> {
    let examples_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hooks");
    if !examples_dir.exists() {
        return Ok(());
    }
    let hooks_dir = Path::new(loom_dir).join("hooks");
    std::fs::create_dir_all(&hooks_dir)?;
    let mut installed = 0;
    for entry in std::fs::read_dir(&examples_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sh") {
            continue;
        }
        let dest = hooks_dir.join(path.file_name().unwrap());
        if dest.exists() {
            continue;
        }
        std::fs::copy(&path, &dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&dest, perms)?;
        }
        installed += 1;
    }
    if installed > 0 {
        println!("✓ Installed {installed} example hook script(s) in hooks/");
    }
    Ok(())
}

/// Write commented-out example overlay files into config/ for discoverability.
fn write_overlay_templates(loom_dir: &str) -> Result<()> {
    let config_dir = Path::new(loom_dir).join("config");
    let overlays: Vec<(&str, &str)> = vec![
        (
            "routing.toml",
            r#"# Loom Routing Overlay
# Override routing strategy and complexity thresholds.
# These values deep-merge on top of config.toml.
#
# strategy = "cost_optimized"   # Options: cost_optimized, performance, balanced
# default_tier = "Mid"          # Default model tier: Slm, Mid, Flagship
# default_provider = "anthropic"
# default_model = "claude-sonnet-4-20250514"
#
# [complexity_thresholds]
# low_max = 0.35    # Max score for Low complexity (default: 0.35)
# high_min = 0.7    # Min score for High complexity (default: 0.7)
"#,
        ),
        (
            "mcp.toml",
            r#"# Loom MCP Overlay
# Override MCP global configuration settings.
# These values deep-merge on top of config.toml.
#
# health_interval_ms = 300000   # Health check interval (default: 5 min)
# max_connections = 50           # Max concurrent MCP connections
"#,
        ),
        (
            "hooks.toml",
            r#"# Loom Hooks Overlay
# Override hooks global configuration settings.
# These values deep-merge on top of config.toml.
#
# timeout_ms = 30000            # Default hook execution timeout (default: 30s)
# max_concurrent = 10            # Max concurrent hook executions
"#,
        ),
        (
            "acp.toml",
            r#"# Loom ACP Registry Overlay
# Configure the ACP (Agent Client Protocol) registry URL.
#
# registry_url = "https://cdn.agentclientprotocol.com"
"#,
        ),
    ];

    for (filename, content) in &overlays {
        let path = config_dir.join(filename);
        if !path.exists() {
            std::fs::write(&path, content).with_context(|| format!("Failed to write {path:?}"))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_creates_tree() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        run(false, false, None, loom_dir).await.unwrap();

        // Verify key directories exist
        assert!(dir.path().join("config").exists());
        assert!(dir.path().join("skills").exists());
        assert!(dir.path().join("mcp").exists());
        assert!(dir.path().join("sessions").exists());
        assert!(dir.path().join("auth").exists());
        assert!(dir.path().join("hooks").exists());
        assert!(dir.path().join("tools").exists());
        assert!(dir.path().join(".cache").exists());

        // Verify config.toml and VERSION
        assert!(dir.path().join("config.toml").exists());
        assert!(dir.path().join("VERSION").exists());

        // Verify overlay template files
        assert!(dir.path().join("config").join("routing.toml").exists());
        assert!(dir.path().join("config").join("mcp.toml").exists());
        assert!(dir.path().join("config").join("hooks.toml").exists());
        assert!(dir.path().join("config").join("acp.toml").exists());

        // Verify SQLite snapshot exists
        assert!(dir.path().join(".cache").join("snapshot.db").exists());
    }

    #[tokio::test]
    async fn test_init_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Run twice
        run(false, false, None, loom_dir).await.unwrap();
        run(false, false, None, loom_dir).await.unwrap();
    }

    #[tokio::test]
    async fn test_init_with_import() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Pre-populate a skill and tool TOML
        let skill_dir = dir.path().join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# Test Skill\n\nDo something useful.\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("tools")).unwrap();
        std::fs::write(
            dir.path().join("tools").join("cursor.toml"),
            "[tool]\nname = \"cursor\"\nversion = \"0.45.0\"\n",
        )
        .unwrap();

        // Run with --import
        run(true, false, None, loom_dir).await.unwrap();

        // Snapshot should have entries
        let db = SnapshotDb::new(loom_dir);
        let counts = db.count_resources().unwrap();
        assert_eq!(*counts.get("skills").unwrap_or(&0), 1);
        assert_eq!(*counts.get("tools").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_init_with_recommended_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        run(false, true, None, loom_dir).await.unwrap();

        // Registry should contain the recommended MCP servers
        let registry = loom_mcp::McpRegistry::new(loom_dir).unwrap();
        let n_recommended = loom_mcp::known::recommended().len();
        assert!(
            n_recommended >= 4,
            "expected at least 4 recommended servers"
        ); // filesystem, github, playwright, agentmemory
        assert_eq!(registry.count().unwrap(), n_recommended);
    }
}
