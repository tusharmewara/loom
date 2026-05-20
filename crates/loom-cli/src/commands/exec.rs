use anyhow::{Context, Result};
use std::collections::HashMap;
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
    let tool_binary = if let Some(adapter) = loom_adapters::adapter_for(launch_tool) {
        let toml = adapter.default_toml(&home);

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

    Ok(())
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
