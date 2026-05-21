use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// `loom exec <tool1> [tool2 ...] [-- tool-args...]`
///
/// Enables ecosystem integration for all specified tools,
/// then launches the last tool with the configured environment.
///
/// Examples:
///   loom exec opencode
///   loom exec opencode -- "fix the bug"
///   loom exec opencode goose -- opencode --message "hello"
///   loom exec opencode --dry-run
pub async fn run(
    tools: &[String],
    tool_args: &[String],
    dry_run: bool,
    loom_dir: &str,
) -> Result<()> {
    if tools.is_empty() {
        anyhow::bail!("At least one tool name is required");
    }

    let home = std::env::var("HOME").context("HOME not set")?;
    let launch_tool = tools.last().unwrap();
    let ecosystem_tools = &tools[..tools.len() - 1];

    // Check for project-level .loom file
    let project_config = load_project_config()?;
    if let Some(ref config) = project_config {
        println!("✓ Project config loaded: {}", config.project.name);
    }

    // Collect env vars from all ecosystem tools
    let mut env_vars = HashMap::new();
    env_vars.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    env_vars.insert("LOOM_ENABLED".to_string(), "1".to_string());

    if let Some(ref config) = project_config {
        env_vars.insert("LOOM_PROJECT_NAME".to_string(), config.project.name.clone());
        if let Some(ref agents) = config.project.loom_dir {
            env_vars.insert("LOOM_AGENTS_DIR".to_string(), agents.clone());
        }
    }

    // Set up ecosystem for all tools except the last one
    for tool_name in ecosystem_tools {
        if let Some(adapter) = loom_adapters::adapter_for(tool_name) {
            let toml = adapter.default_toml(&home);
            if let Some(ref skills) = toml.skills {
                if let Some(ref native_dir) = skills.native_dir {
                    env_vars.insert(
                        format!("{}_SKILLS_DIR", tool_name.to_uppercase().replace('-', "_")),
                        native_dir.clone(),
                    );
                }
            }
            if let Some(ref mcp) = toml.mcp {
                if let Some(ref config_path) = mcp.config_path {
                    env_vars.insert(
                        format!("{}_MCP_CONFIG", tool_name.to_uppercase().replace('-', "_")),
                        config_path.clone(),
                    );
                }
            }
            println!("✓ Ecosystem enabled for '{tool_name}'");
        } else {
            println!("⚠ No preset adapter for '{tool_name}', using generic config");
        }
    }

    // Set up the launch tool
    let mut native_mcp_config = None;
    let mut native_skills_dir = None;
    let mut native_sessions_dir = None;

    let tool_binary = if let Some(adapter) = loom_adapters::adapter_for(launch_tool) {
        let toml = adapter.default_toml(&home);

        native_mcp_config = toml.mcp.as_ref().and_then(|m| m.config_path.clone());
        native_skills_dir = toml.skills.as_ref().and_then(|s| s.native_dir.clone());
        native_sessions_dir = toml.sessions.as_ref().and_then(|s| s.store_path.clone());

        // Skills
        if let Some(ref skills) = toml.skills {
            if let Some(ref native_dir) = skills.native_dir {
                env_vars.insert(
                    "LOOM_SKILLS_DIR".to_string(),
                    format!("{loom_dir}/skills"),
                );
                env_vars.insert(
                    format!("{}_SKILLS_DIR", launch_tool.to_uppercase().replace('-', "_")),
                    native_dir.clone(),
                );
            }
        }

        // MCP
        if let Some(ref mcp) = toml.mcp {
            if let Some(ref config_path) = mcp.config_path {
                env_vars.insert(
                    "LOOM_MCP_DIR".to_string(),
                    format!("{loom_dir}/mcp"),
                );
                env_vars.insert(
                    format!("{}_MCP_CONFIG", launch_tool.to_uppercase().replace('-', "_")),
                    config_path.clone(),
                );
            }
        }

        // Sessions
        if let Some(ref sessions) = toml.sessions {
            if let Some(ref store_path) = sessions.store_path {
                env_vars.insert(
                    "LOOM_SESSIONS_DIR".to_string(),
                    format!("{loom_dir}/sessions"),
                );
                env_vars.insert(
                    format!("{}_SESSIONS_DIR", launch_tool.to_uppercase().replace('-', "_")),
                    store_path.clone(),
                );
            }
        }

        // Routing
        if let Some(ref routing) = toml.routing {
            env_vars.insert(
                "LOOM_ROUTING_TIER".to_string(),
                routing.default_tier.clone(),
            );
            env_vars.insert(
                "LOOM_ROUTING_STRATEGY".to_string(),
                routing.strategy.clone(),
            );
        }

        // Apply routing decision if LOOM_ROUTING_PROVIDER is set
        if let Ok(provider) = std::env::var("LOOM_ROUTING_PROVIDER") {
            env_vars.insert(
                format!("{}_MODEL", launch_tool.to_uppercase().replace('-', "_")),
                provider.clone(),
            );
        }

        launch_tool.to_string()
    } else {
        println!("⚠ No preset adapter for '{launch_tool}', using generic config");
        launch_tool.to_string()
    };

    // Find the binary
    let binary_path = find_binary(&tool_binary)?;

    if dry_run {
        println!("\n=== Dry Run ===");
        println!("Binary: {}", binary_path.display());
        println!("Args: {:?}", tool_args);
        println!("\nEnvironment variables:");
        for (key, value) in &env_vars {
            println!("  {}={}", key, value);
        }
        return Ok(());
    }

    // Create a session manifest in the shared sessions directory
    let session_id = uuid::Uuid::new_v4().to_string();
    let session_store = loom_sessions::store::SessionStore::new(loom_dir);
    let manifest = loom_core::SessionManifest {
        session_id: session_id.clone(),
        source_tool: launch_tool.to_string(),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        topic: format!("loom exec {}", tools.join(" ")),
        message_count: 0,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        model_provider: std::env::var("LOOM_ROUTING_PROVIDER").unwrap_or_default(),
        files_modified: vec![],
        compatibility_note: None,
    };
    session_store.save_manifest(&manifest)?;
    env_vars.insert("LOOM_SESSION_ID".to_string(), session_id);

    // Inject loom's shared ecosystem resources into the tool's native config
    if let Some(ref mcp_path) = native_mcp_config {
        if let Err(e) = inject_shared_mcp(loom_dir, mcp_path) {
            eprintln!("⚠ Failed to inject shared MCP servers: {e}");
        }
    }
    if let Some(ref skills_dir) = native_skills_dir {
        if let Err(e) = inject_shared_skills(loom_dir, skills_dir) {
            eprintln!("⚠ Failed to inject shared skills: {e}");
        }
    }

    // Build and execute command
    let mut cmd = Command::new(&binary_path);
    cmd.args(tool_args);

    // Inherit current env and overlay loom vars
    cmd.envs(&env_vars);

    println!("🚀 Launching '{}' with loom ecosystem...", launch_tool);
    if !tool_args.is_empty() {
        println!("   Args: {:?}", tool_args);
    }

    let status = cmd
        .spawn()
        .context(format!("Failed to spawn '{}'", tool_binary))?
        .wait()
        .context(format!("Failed to wait for '{}'", tool_binary))?;

    if !status.success() {
        anyhow::bail!("'{}' exited with status: {}", launch_tool, status);
    }

    // Extract sessions from the tool's native session directory into loom
    if let Some(ref sessions_dir) = native_sessions_dir {
        if let Err(e) = extract_tool_sessions(loom_dir, sessions_dir) {
            eprintln!("⚠ Failed to extract tool sessions: {e}");
        }
    }

    Ok(())
}

// ─── Ecosystem Integration Helpers ────────────────────────────────────

/// Inject loom's shared MCP servers into the tool's native MCP config.
///
/// Reads all registered MCP servers from `mcp_registry.db` and merges them
/// into the tool's native `mcpServers` JSON config. Native servers take
/// precedence — shared servers are only added if not already present.
fn inject_shared_mcp(loom_dir: &str, native_config_path: &str) -> Result<()> {
    let path = Path::new(native_config_path);
    if path.is_dir() {
        return Ok(());
    }

    let registry = match loom_mcp::McpRegistry::new(loom_dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let shared_servers = registry.filter_shared(true).unwrap_or_default();
    if shared_servers.is_empty() {
        return Ok(());
    }

    // Read existing native config (standard MCP JSON format)
    let raw = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    let mut config: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or(serde_json::json!({"mcpServers": {}}));

    if !config.is_object() {
        config = serde_json::json!({"mcpServers": {}});
    }
    let config_obj = config.as_object_mut().unwrap();
    if !config_obj.contains_key("mcpServers") {
        config_obj.insert("mcpServers".into(), serde_json::json!({}));
    }
    let Some(servers) = config_obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) else {
        return Ok(());
    };

    let mut count = 0;
    for entry in &shared_servers {
        if servers.contains_key(&entry.id) {
            continue;
        }
        let mut sv = serde_json::json!({});
        if let Some(cmd) = &entry.transport.command {
            sv["command"] = serde_json::json!(cmd);
        }
        if !entry.transport.args.is_empty() {
            sv["args"] = serde_json::json!(entry.transport.args);
        }
        if !entry.transport.env.is_empty() {
            sv["env"] = serde_json::json!(entry.transport.env);
        }
        servers.insert(entry.id.clone(), sv);
        count += 1;
    }

    if count > 0 {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, serde_json::to_string_pretty(&config)?)?;
        println!("✓ Injected {count} shared MCP server(s) into {native_config_path}");
    }

    Ok(())
}

/// Inject loom's shared skills into the tool's native skills directory.
///
/// Symlinks `.md` skill files from `~/.loom/skills/` into the tool's native
/// skills directory, skipping any that already exist (native takes precedence).
fn inject_shared_skills(loom_dir: &str, native_skills_dir: &str) -> Result<()> {
    let shared_dir = Path::new(loom_dir).join("skills");
    if !shared_dir.is_dir() {
        return Ok(());
    }

    let native_dir = Path::new(native_skills_dir);
    std::fs::create_dir_all(native_dir)?;

    let mut count = 0;
    for entry in std::fs::read_dir(&shared_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = match path.file_name() {
            Some(n) => n.to_os_string(),
            None => continue,
        };
        let target = native_dir.join(&name);
        if target.exists() {
            continue;
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, &target).or_else(|_| {
                std::fs::copy(&path, &target).map(|_| ())
            })?;
        }
        #[cfg(not(unix))]
        {
            std::fs::copy(&path, &target)?;
        }
        count += 1;
    }

    if count > 0 {
        println!("✓ Injected {count} shared skill(s) into {native_skills_dir}");
    }

    Ok(())
}

/// Extract sessions from the tool's native session directory into loom's store.
///
/// Scans the tool's native session dir for exported session manifests and
/// imports any that aren't already in loom's `~/.loom/sessions/` store.
fn extract_tool_sessions(loom_dir: &str, native_session_dir: &str) -> Result<usize> {
    let session_dir = Path::new(native_session_dir);
    if !session_dir.is_dir() {
        return Ok(0);
    }

    let store = loom_sessions::store::SessionStore::new(loom_dir);
    let mut count = 0;

    for entry in std::fs::read_dir(session_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("manifest.json");
        if !manifest_path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let manifest: loom_core::SessionManifest = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if store.get(&manifest.session_id).is_some() {
            continue;
        }
        if let Err(e) = store.save_manifest(&manifest) {
            eprintln!("⚠ Failed to import session '{}': {e}", manifest.session_id);
            continue;
        }
        count += 1;
    }

    if count > 0 {
        println!("✓ Imported {count} session(s) from {native_session_dir}");
    }

    Ok(count)
}

/// Project configuration from .loom file
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct ProjectConfig {
    project: ProjectSection,
    #[serde(default)]
    tools: Option<ToolsSection>,
    #[serde(default)]
    routing: Option<RoutingSection>,
    #[serde(default)]
    mcp: Option<McpSection>,
    #[serde(default)]
    skills: Option<SkillsSection>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProjectSection {
    name: String,
    loom_dir: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct ToolsSection {
    enabled: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct RoutingSection {
    default_tier: String,
    strategy: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct McpSection {
    shared: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct SkillsSection {
    auto_load: bool,
}

fn load_project_config() -> Result<Option<ProjectConfig>> {
    let current_dir = std::env::current_dir()?;
    
    // Check for .loom or .loomrc in current directory
    for filename in &[".loom", ".loomrc"] {
        let config_path = current_dir.join(filename);
        if config_path.exists() && config_path.is_file() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: ProjectConfig = toml::from_str(&content)?;
            return Ok(Some(config));
        }
    }
    
    Ok(None)
}

fn find_binary(name: &str) -> Result<std::path::PathBuf> {
    // Check if it's an absolute path
    let path = std::path::Path::new(name);
    if path.is_absolute() && path.exists() {
        return Ok(path.to_path_buf());
    }

    // Search PATH
    let path_env = std::env::var("PATH").context("PATH not set")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if candidate.exists() && is_executable(&candidate) {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "Binary '{}' not found in PATH. Install it first or provide absolute path.",
        name
    );
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_binary_absolute_path() {
        let result = find_binary("/bin/ls");
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_binary_nonexistent() {
        let result = find_binary("nonexistent-binary-xyz-123");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_exec_dry_run_single_tool() {
        let result = run(
            &[String::from("echo")],
            &[String::from("--dry-run")],
            true,
            "/tmp",
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_exec_requires_at_least_one_tool() {
        let result = run(&[], &[], false, "/tmp").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_load_project_config_missing() {
        // Should return None when no .loom file exists
        let result = load_project_config();
        // This may succeed with None or fail depending on current directory
        // Just verify it doesn't panic
        let _ = result;
    }
}
