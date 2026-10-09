//! `loom exec` — Launch agent tool(s) with full ecosystem integration.
//!
//! Sequential mode (default): enables ecosystem for all tools, launches the last.
//! Parallel mode (--parallel): runs ALL tools concurrently with worktree isolation.

use anyhow::{Context, Result};
use chrono;
use loom_core::Task;
use loom_runtime::{LaunchConfig, ProcessRuntime, RuntimeRegistry, RunResult};
#[cfg(feature = "docker")]
use loom_runtime::ContainerRuntime;
#[cfg(feature = "podman")]
use loom_runtime::ContainerRuntime;
#[cfg(feature = "kubernetes")]
use loom_runtime::ContainerRuntime;
use loom_parallel::{AgentPool, AgentTask, WorktreeManager};
use loom_generic_adapter::GenericAdapter;
use std::collections::HashMap;
use std::path::Path;
use uuid;

pub async fn run(
    tools: &[String],
    tool_args: &[String],
    dry_run: bool,
    parallel: bool,
    pool_size: usize,
    no_isolation: bool,
    loom_dir: &str,
) -> Result<()> {
    if tools.is_empty() {
        anyhow::bail!("At least one tool name is required");
    }

    if parallel {
        run_parallel(tools, tool_args, dry_run, pool_size, no_isolation, loom_dir).await
    } else {
        run_sequential(tools, tool_args, dry_run, loom_dir).await
    }
}

// ─── Sequential Mode ──────────────────────────────────────────────────────────

async fn run_sequential(
    tools: &[String],
    tool_args: &[String],
    dry_run: bool,
    loom_dir: &str,
) -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let launch_tool = tools.last().unwrap();
    let ecosystem_tools = &tools[..tools.len() - 1];

    let project_config = load_project_config()?;
    if let Some(ref config) = project_config {
        println!("✓ Project config loaded: {}", config.project.name);
    }

    let mut env_vars = build_base_env(loom_dir, &project_config);

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
    let (tool_binary, native_mcp_config, native_skills_dir, native_sessions_dir) =
        setup_launch_tool(launch_tool, &home, &mut env_vars, loom_dir)?;

    // Find the binary
    let binary_path = find_binary(&tool_binary)?;

    if dry_run {
        print_dry_run(&binary_path, tool_args, &env_vars);
        return Ok(());
    }

    // Create session manifest
    let _session_id = create_session_manifest(loom_dir, launch_tool, tools, &mut env_vars);

    // Inject shared ecosystem resources
    inject_shared_resources(loom_dir, &native_mcp_config, &native_skills_dir);

    // Execute via Runtime abstraction
    let result = execute_via_runtime(
        loom_dir,
        launch_tool,
        tool_args,
        &env_vars,
        dry_run,
    ).await?;

    if !result.success {
        anyhow::bail!("Runtime execution failed: {}", result.stderr);
    }

    // Extract sessions after completion
    if let Some(ref sessions_dir) = native_sessions_dir {
        if let Err(e) = extract_tool_sessions(loom_dir, sessions_dir) {
            eprintln!("⚠ Failed to extract tool sessions: {e}");
        }
    }

    Ok(())
}

// ─── Parallel Mode ────────────────────────────────────────────────────────────

async fn run_parallel(
    tools: &[String],
    tool_args: &[String],
    dry_run: bool,
    pool_size: usize,
    no_isolation: bool,
    loom_dir: &str,
) -> Result<()> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let project_config = load_project_config()?;

    // Set up worktree isolation
    let mut worktree_mgr = if !no_isolation {
        let cwd = std::env::current_dir().context("Failed to get current directory")?;
        if WorktreeManager::is_git_repo(&cwd) {
            if let Some(repo_root) = WorktreeManager::find_repo_root(&cwd) {
                Some(WorktreeManager::new(&repo_root))
            } else {
                println!("⚠ Not a git repository, skipping worktree isolation");
                None
            }
        } else {
            println!("⚠ Not a git repository, skipping worktree isolation");
            None
        }
    } else {
        None
    };

    let pool = AgentPool::new(pool_size);

    let mut tasks = Vec::with_capacity(tools.len());
    for tool_name in tools {
        let mut env_vars = build_base_env(loom_dir, &project_config);
        let (tool_binary, _native_mcp, _native_skills, _native_sessions) =
            setup_launch_tool(tool_name, &home, &mut env_vars, loom_dir)?;

        // Create session manifest for this tool
        create_session_manifest(loom_dir, tool_name, tools, &mut env_vars);

        let binary_path = if dry_run {
            tool_binary.clone()
        } else {
            match find_binary(&tool_binary) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(e) => {
                    eprintln!("⚠ Skipping '{}': {e}", tool_name);
                    continue;
                }
            }
        };

        let mut task = AgentTask::new(tool_name, &binary_path)
            .args(tool_args.to_vec())
            .envs(&env_vars);

        // Create isolated worktree
        if let Some(ref mut mgr) = worktree_mgr {
            match mgr.create_worktree(tool_name) {
                Ok(path) => {
                    let path_str = path.display().to_string();
                    task = task.work_dir(path);
                    println!("  📂 Isolated worktree for '{tool_name}': {path_str}");
                }
                Err(e) => {
                    eprintln!("  ⚠ Worktree creation failed for '{tool_name}': {e}");
                }
            }
        }

        tasks.push(task);
    }

    if tasks.is_empty() {
        anyhow::bail!("No tools available to execute");
    }

    if dry_run {
        println!("\n=== Dry Run (Parallel) ===");
        println!("Pool size: {}", pool_size);
        println!("Tools: {}", tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "));
        if !no_isolation {
            println!("Worktree isolation: enabled");
        }
        for task in &tasks {
            println!("\n  --- {} ---", task.name);
            println!("  Binary: {}", task.binary_path.display());
            if let Some(ref wd) = task.work_dir {
                println!("  Work dir: {}", wd.display());
            }
        }
        return Ok(());
    }

    println!(
        "🚀 Launching {} tool(s) in parallel (pool={})...",
        tasks.len(),
        pool_size
    );

    let results = pool.execute_all(tasks).await;

    // Print summary
    println!("\n═══════════════════════════════════════");
    println!("          Parallel Execution Results     ");
    println!("═══════════════════════════════════════");
    let mut ok_count = 0;
    let mut fail_count = 0;
    for r in &results {
        let icon = if r.success() { "✓" } else { "✗" };
        let dur = format_duration(r.duration);
        let err = r
            .error
            .as_ref()
            .map(|e| format!(" — {e}"))
            .unwrap_or_default();
        println!("  {icon} {} ({dur}){}", r.name, err);
        if r.success() {
            ok_count += 1;
        } else {
            fail_count += 1;
        }
    }
    println!("───────────────────────────────────────");
    println!(
        "  {ok_count} succeeded, {fail_count} failed in {}",
        format_duration(
            results
                .iter()
                .map(|r| r.duration)
                .max()
                .unwrap_or_default()
        )
    );

    if fail_count > 0 {
        anyhow::bail!("{fail_count} tool(s) failed");
    }

    Ok(())
}

// ─── Shared Helpers ───────────────────────────────────────────────────────────

fn build_base_env(
    loom_dir: &str,
    project_config: &Option<ProjectConfig>,
) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    env_vars.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    env_vars.insert("LOOM_ENABLED".to_string(), "1".to_string());

    if let Some(ref config) = project_config {
        env_vars.insert("LOOM_PROJECT_NAME".to_string(), config.project.name.clone());
        if let Some(ref agents) = config.project.loom_dir {
            env_vars.insert("LOOM_AGENTS_DIR".to_string(), agents.clone());
        }
    }

    env_vars
}

#[allow(clippy::type_complexity)]
fn setup_launch_tool(
    launch_tool: &str,
    home: &str,
    env_vars: &mut HashMap<String, String>,
    loom_dir: &str,
) -> Result<(String, Option<String>, Option<String>, Option<String>)> {
    #[allow(unused_assignments)]
    let mut native_mcp_config: Option<String> = None;
    #[allow(unused_assignments)]
    let mut native_skills_dir: Option<String> = None;
    #[allow(unused_assignments)]
    let mut native_sessions_dir: Option<String> = None;

    if let Some(adapter) = loom_adapters::adapter_for(launch_tool) {
        let toml = adapter.default_toml(home);

        native_mcp_config = toml.mcp.as_ref().and_then(|m| m.config_path.clone());
        native_skills_dir = toml.skills.as_ref().and_then(|s| s.native_dir.clone());
        native_sessions_dir = toml
            .sessions
            .as_ref()
            .and_then(|s| s.store_path.clone());

        // Skills
        if let Some(ref skills) = toml.skills {
            if let Some(ref native_dir) = skills.native_dir {
                env_vars.insert("LOOM_SKILLS_DIR".to_string(), format!("{loom_dir}/skills"));
                env_vars.insert(
                    format!("{}_SKILLS_DIR", launch_tool.to_uppercase().replace('-', "_")),
                    native_dir.clone(),
                );
            }
        }

        // MCP
        if let Some(ref mcp) = toml.mcp {
            if let Some(ref config_path) = mcp.config_path {
                env_vars.insert("LOOM_MCP_DIR".to_string(), format!("{loom_dir}/mcp"));
                env_vars.insert(
                    format!("{}_MCP_CONFIG", launch_tool.to_uppercase().replace('-', "_")),
                    config_path.clone(),
                );
            }
        }

        // Sessions
        if let Some(ref sessions) = toml.sessions {
            if let Some(ref store_path) = sessions.store_path {
                env_vars.insert("LOOM_SESSIONS_DIR".to_string(), format!("{loom_dir}/sessions"));
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

        // Apply routing decision from env
        if let Ok(provider) = std::env::var("LOOM_ROUTING_PROVIDER") {
            env_vars.insert(
                format!("{}_MODEL", launch_tool.to_uppercase().replace('-', "_")),
                provider,
            );
        }

        Ok((
            launch_tool.to_string(),
            native_mcp_config,
            native_skills_dir,
            native_sessions_dir,
        ))
    } else {
        println!("⚠ No preset adapter for '{launch_tool}', using generic config");
        Ok((
            launch_tool.to_string(),
            None,
            None,
            None,
        ))
    }
}

fn create_session_manifest(
    loom_dir: &str,
    launch_tool: &str,
    tools: &[String],
    env_vars: &mut HashMap<String, String>,
) -> String {
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
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    };
    let _ = session_store.save_manifest(&manifest);
    env_vars.insert("LOOM_SESSION_ID".to_string(), session_id.clone());
    session_id
}

fn inject_shared_resources(loom_dir: &str, mcp_config: &Option<String>, skills_dir: &Option<String>) {
    if let Some(ref mcp_path) = mcp_config {
        if let Err(e) = inject_shared_mcp(loom_dir, mcp_path) {
            eprintln!("⚠ Failed to inject shared MCP servers: {e}");
        }
    }
    if let Some(ref skills_dir) = skills_dir {
        if let Err(e) = inject_shared_skills(loom_dir, skills_dir) {
            eprintln!("⚠ Failed to inject shared skills: {e}");
        }
    }
}

fn print_dry_run(binary_path: &Path, tool_args: &[String], env_vars: &HashMap<String, String>) {
    println!("\n=== Dry Run ===");
    println!("Binary: {}", binary_path.display());
    println!("Args: {:?}", tool_args);
    println!("\nEnvironment variables:");
    for (key, value) in env_vars {
        println!("  {}={}", key, value);
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{:.0}ms", d.as_millis())
    } else if secs < 60.0 {
        format!("{:.1}s", secs)
    } else {
        format!("{}m {:0}s", secs as u64 / 60, secs as u64 % 60)
    }
}

// ─── Ecosystem Integration Helpers (unchanged from original) ──────────────────

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

    let raw = if path.exists() {
        std::fs::read_to_string(path).unwrap_or_default()
    } else {
        String::new()
    };
    let mut config: serde_json::Value =
        serde_json::from_str(&raw).unwrap_or(serde_json::json!({"mcpServers": {}}));

    if !config.is_object() {
        config = serde_json::json!({"mcpServers": {}});
    }
    let config_obj = config.as_object_mut().unwrap();
    if !config_obj.contains_key("mcpServers") {
        config_obj.insert("mcpServers".into(), serde_json::json!({}));
    }
    let Some(servers) = config_obj
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
    else {
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
            std::os::unix::fs::symlink(&path, &target).or_else(|_| std::fs::copy(&path, &target).map(|_| ()))?;
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

// ─── Project Config ───────────────────────────────────────────────────────────

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
    let path = std::path::Path::new(name);
    if path.is_absolute() && path.exists() {
        return Ok(path.to_path_buf());
    }

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
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

/// Execute a task via the Runtime abstraction.
///
/// This function:
/// 1. Creates a GenericAdapter for the tool to get its launch config
/// 2. Creates the appropriate Runtime from the registry
/// 3. Calls Runtime::prepare, Runtime::run, and Runtime::cleanup
async fn execute_via_runtime(
    loom_dir: &str,
    agent_id: &str,
    tool_args: &[String],
    env_vars: &HashMap<String, String>,
    dry_run: bool,
) -> Result<RunResult> {
    // Get launch config from tool's TOML
    let config = GenericAdapter::new(loom_dir, agent_id)
        .ok()
        .and_then(|adapter| adapter.launch_config())
        .unwrap_or_else(|| LaunchConfig {
            runtime: "process".to_string(),
            command: agent_id.to_string(),
            args: tool_args.to_vec(),
            env: env_vars.clone(),
            working_dir: None,
            image: None,
            network: None,
            auto_remove: true,
        });

    if dry_run {
        println!("=== Dry Run: execute via {} runtime ===", config.runtime);
        println!("Command: {}", config.command);
        println!("Args: {:?}", config.args);
        println!("Image: {:?}", config.image);
        println!("Network: {:?}", config.network);
        println!("Auto-remove: {}", config.auto_remove);
        return Ok(RunResult {
            success: true,
            stdout: "".to_string(),
            stderr: "".to_string(),
            exit_code: Some(0),
            container_id: None,
        });
    }

    // Create runtime registry with all available runtimes
    let mut registry = RuntimeRegistry::new();
    let _ = registry.register(Box::new(ProcessRuntime::new()));
    #[cfg(feature = "docker")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::docker()));
    }
    #[cfg(feature = "podman")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::podman()));
    }
    #[cfg(feature = "kubernetes")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::kubernetes()));
    }

    // Get the runtime
    let runtime = registry
        .get(&config.runtime)
        .ok_or_else(|| anyhow::anyhow!("Runtime '{}' not available", config.runtime))?;

    // Create a minimal task for runtime execution
    let task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        title: format!("exec {}", agent_id),
        description: "".to_string(),
        phase: loom_core::TaskPhase::Plan,
        phase_history: vec![],
        prd_path: None,
        context_files: vec![],
        routed_provider: None,
        files_modified: vec![],
        complexity_score: None,
        routing_decision: None,
        last_agent: None,
        linked_issue: None,
        sub_agents: None,
        phase_results: vec![],
        tags: vec![],
        worktree_path: None,
        runtime: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        status: loom_core::TaskStatus::Pending,
    };

    // Prepare, run, cleanup
    runtime.prepare(&config).await?;
    let result = runtime.run(&config, &task).await?;
    runtime.cleanup(&result).await?;
    Ok(result)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

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
            false,
            4,
            true,
            "/tmp",
        )
        .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_exec_requires_at_least_one_tool() {
        let result = run(&[], &[], false, false, 4, true, "/tmp").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_parallel_dry_run() {
        let result = run(
            &[String::from("echo"), String::from("echo")],
            &[String::from("hello")],
            true,
            true,
            4,
            true,
            "/tmp",
        )
        .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_format_duration() {
        assert!(format_duration(std::time::Duration::from_millis(500)).contains("ms"));
        assert!(format_duration(std::time::Duration::from_secs(5)).contains("s"));
    }

    #[test]
    fn test_load_project_config_missing() {
        let result = load_project_config();
        let _ = result; // doesn't panic
    }
}
