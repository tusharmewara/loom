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
pub async fn run(import: bool, recommended: bool, loom_dir: &str) -> Result<()> {
    // 1. Create the full directory tree
    let builder = SnapshotBuilder::new(loom_dir);
    builder
        .create_agents_tree()
        .context("Failed to create ~/.loom/ directory tree")?;
    println!("✓ Initialized ecosystem at ~/.loom/");

    // 2. Initialize the SQLite snapshot schema (idempotent)
    let db = SnapshotDb::new(loom_dir);
    db.initialize_schema()
        .with_context(|| {
            format!(
                "Failed to initialize snapshot database schema at {}/.cache/snapshot.db",
                loom_dir
            )
        })?;

    // 3. Write a default config.toml if one doesn't exist
    let config_path = Path::new(loom_dir).join("config.toml");
    if !config_path.exists() {
        let config = loom_core::LoomConfig::default();
        let toml_str = toml::to_string_pretty(&config)
            .context("Failed to serialize default config")?;
        std::fs::write(&config_path, toml_str)
            .with_context(|| format!("Failed to write {config_path:?}"))?;
        println!("✓ Created default config.toml");
    } else {
        println!("  config.toml already exists (not overwritten)");
    }

    // 4. Migrate legacy session JSON files to JSONL layout
    let sessions = SessionManager::new(loom_dir);
    let migrated = sessions.migrate().unwrap_or(0);
    if migrated > 0 {
        println!("✓ Migrated {migrated} legacy session(s) to JSONL layout");
    }

    install_example_hooks(loom_dir)?;

    // 5. Optionally import existing resources
    if import {
        builder
            .build_initial()
            .context("Failed to build initial snapshot from existing resources")?;
        println!("✓ Imported existing resources into snapshot");
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_creates_tree() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        run(false, false, loom_dir).await.unwrap();

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

        // Verify SQLite snapshot exists
        assert!(dir.path().join(".cache").join("snapshot.db").exists());
    }

    #[tokio::test]
    async fn test_init_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Run twice
        run(false, false, loom_dir).await.unwrap();
        run(false, false, loom_dir).await.unwrap();
    }

    #[tokio::test]
    async fn test_init_with_import() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Pre-populate a skill and tool TOML
        let skill_dir = dir.path().join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Test Skill\n\nDo something useful.\n").unwrap();
        std::fs::create_dir_all(dir.path().join("tools")).unwrap();
        std::fs::write(
            dir.path().join("tools").join("cursor.toml"),
            "[tool]\nname = \"cursor\"\nversion = \"0.45.0\"\n",
        ).unwrap();

        // Run with --import
        run(true, false, loom_dir).await.unwrap();

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

        run(false, true, loom_dir).await.unwrap();

        // Registry should contain the recommended MCP servers
        let registry = loom_mcp::McpRegistry::new(loom_dir).unwrap();
        let n_recommended = loom_mcp::known::recommended().len();
        assert!(n_recommended >= 4, "expected at least 4 recommended servers"); // filesystem, github, playwright, agentmemory
        assert_eq!(registry.count().unwrap(), n_recommended);
    }
}
