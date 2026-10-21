//! Tracked ACP dispatch via the canonical ACP protocol client.
//!
//! The daemon's `acp.dispatch` IPC path runs the full initialize →
//! session/new → session/prompt flow against a headless agent and
//! captures the transcript as a Loom session, indexed into
//! cross-session memory and the knowledge graph — the same visibility
//! the CLI `workflow dispatch` path provides.

use loom_generic_adapter::GenericAdapter;
use loom_sessions::acp_launch::{from_agents_toml, AcpLaunch};
use loom_sessions::acp_protocol::{build_acp_session, AcpClientConfig, AcpProtocolClient};
use loom_sessions::store::SessionStore;
use std::path::{Path, PathBuf};

/// A resolved headless agent launch: program + args.
#[derive(Debug)]
pub struct AgentLaunch {
    pub program: String,
    pub args: Vec<String>,
    /// Extra environment variables the agent needs.
    pub env: Vec<(String, String)>,
}

impl From<AcpLaunch> for AgentLaunch {
    fn from(launch: AcpLaunch) -> Self {
        Self {
            program: launch.program,
            args: launch.args,
            env: launch.env,
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

/// Resolve how to launch an agent headlessly: a native binary found in
/// `search_dirs`, or a runner from `{loom_dir}/config/agents.toml`.
/// Unlike the interactive dispatch path this never opens a GUI terminal,
/// so the caller can pipe stdio to log files and wait for completion.
pub fn resolve_agent_launch(
    loom_dir: &str,
    agent_id: &str,
    _task_id: &str,
    search_dirs: &[PathBuf],
) -> anyhow::Result<AgentLaunch> {
    // A registered entry wins over a bare PATH binary: it carries the
    // agent's protocol mode flag (`acp`, `--acp`), without which the
    // agent starts in its normal interactive mode and never answers.
    if let Some(launch) = from_agents_toml(loom_dir, agent_id) {
        if launch.program != agent_id || !launch.args.is_empty() {
            return Ok(launch.into());
        }
    }

    for dir in search_dirs {
        let candidate = dir.join(agent_id);
        if candidate.exists() && is_executable(&candidate) {
            return Ok(AgentLaunch {
                program: candidate.to_string_lossy().to_string(),
                args: Vec::new(),
                env: Vec::new(),
            });
        }
    }

    anyhow::bail!(
        "Agent '{agent_id}' not found in search dirs or in config/agents.toml. \
         Register it first with: loom add-acp-agent <agent_id>, \
         or ensure the binary is on PATH"
    )
}

/// Resolve agent launch using GenericAdapter (tool TOML) first, then fall back
/// to binary lookup via `resolve_agent_launch`.
/// This enables using the tool's configured runtime (process, docker, podman, etc.)
/// for ACP dispatch, while maintaining backward compatibility.
pub fn resolve_agent_launch_with_adapter(
    loom_dir: &str,
    agent_id: &str,
    _task_id: &str,
) -> anyhow::Result<AgentLaunch> {
    // Try GenericAdapter first - reads tool TOML for launch config
    if let Ok(adapter) = GenericAdapter::new(loom_dir, agent_id) {
        if let Some(launch_config) = adapter.launch_config() {
            return Ok(AgentLaunch {
                program: launch_config.command,
                args: launch_config.args,
                env: launch_config
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            });
        }
    }

    // Fall back to binary lookup (PATH or config/agents.toml)
    let search_dirs: Vec<PathBuf> = std::env::var("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    resolve_agent_launch(loom_dir, agent_id, _task_id, &search_dirs)
}

/// Best-effort task title/description lookup from the workflow task
/// file. Falls back to the raw task id so capture never fails for
/// tasks created outside the workflow system.
pub fn load_task_brief(loom_dir: &str, task_id: &str) -> (String, String) {
    let path = Path::new(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.json"));
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let description = value
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if !title.is_empty() {
                let prompt = if description.is_empty() {
                    title.to_string()
                } else {
                    format!("{title} — {description}")
                };
                return (title.to_string(), prompt);
            }
        }
    }
    (task_id.to_string(), format!("ACP task {task_id}"))
}

/// Dispatch an ACP agent using the canonical ACP protocol client.
/// This runs the initialize → session/new → session/prompt flow
/// and captures the transcript as a Loom session.
pub fn dispatch_acp_protocol(
    loom_dir: &str,
    manager: &std::sync::Arc<std::sync::Mutex<crate::run_manager::RunManager>>,
    agent_id: &str,
    task_id: &str,
    prompt: &str,
    timeout_secs: u64,
) -> anyhow::Result<serde_json::Value> {
    if task_id.is_empty() || agent_id.is_empty() {
        anyhow::bail!("task_id and agent_id required");
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    // Fall back to the workflow task brief (title — description, or the
    // "ACP task {id}" placeholder) when no explicit prompt was supplied.
    // Callers like the TUI dispatch menu only send task_id/agent_id.
    let (topic, prompt) = if prompt.trim().is_empty() {
        let (topic, prompt) = load_task_brief(loom_dir, task_id);
        (topic, prompt)
    } else {
        (
            prompt.chars().take(80).collect::<String>(),
            prompt.to_string(),
        )
    };

    // Record run in registry
    if let Ok(mut registry) = manager.lock() {
        registry.record_external_run(&run_id, task_id, agent_id);
    }

    // Resolve agent launch: try GenericAdapter first (uses tool TOML launch config),
    // then fall back to binary lookup (PATH or config/agents.toml).
    let launch = resolve_agent_launch_with_adapter(loom_dir, agent_id, task_id)?;

    // Build AcpClientConfig from the resolved launch (args already carry
    // the agent's protocol mode flag).
    let config = AcpClientConfig {
        program: launch.program,
        args: launch.args,
        cwd: None,
        timeout_secs,
        env: launch.env,
    };

    // Run ACP prompt
    let transcript = AcpProtocolClient::run_prompt(&config, &prompt)?;

    // Build session using the shared ACP shaper (same as the CLI path);
    // topic was resolved above (task-brief title or explicit prompt).
    let (manifest, messages) = build_acp_session(&transcript, &prompt, agent_id, Some(topic));

    // Save session
    let loom_dir_str = loom_dir;
    let store = SessionStore::new(loom_dir_str);
    store.save_manifest(&manifest)?;
    store.replace_messages(&manifest.session_id, &messages)?;

    // Index into memory
    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir_str) {
        if let Err(e) = memory.index_session_with_tags(
            &manifest.session_id,
            &messages,
            &manifest.topic,
            &manifest.tags,
        ) {
            tracing::warn!(
                "ACP dispatch: memory index failed for '{}': {}",
                manifest.session_id,
                e
            );
        }
    }

    // Extract into knowledge graph
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(loom_dir_str) {
        if let Err(e) = kg.extract_from_messages(&messages, &manifest.session_id) {
            tracing::warn!(
                "ACP dispatch: knowledge extraction failed for '{}': {}",
                manifest.session_id,
                e
            );
        }
    }

    // Update run registry with completion
    if let Ok(mut registry) = manager.lock() {
        registry.complete_run(&run_id, 0, Some(manifest.session_id.clone()));
    }

    // Meter the run. The agent's own report is the only source of truth;
    // an agent that reports no usage is recorded as unavailable.
    // No catalog prices here: loomd does not depend on loom-routing, and the
    // ACP agent reports its own cost anyway.
    let mut usage_record = None;
    if let Ok(meter) = loom_telemetry::UsageMeter::new(loom_dir_str) {
        match meter.record_run(
            &manifest.session_id,
            "acp",
            agent_id,
            transcript.usage,
            &transcript.usage_method,
            prompt.chars().count(),
            None,
        ) {
            Ok(record) => usage_record = Some(record),
            Err(e) => tracing::warn!("ACP dispatch: usage not recorded: {}", e),
        }
    }

    Ok(serde_json::json!({
        "dispatched": true,
        "session_id": manifest.session_id,
        "agent_session_id": transcript.session_id,
        "stop_reason": transcript.stop_reason,
        "tool_calls": transcript.tool_calls,
        "run_id": run_id,
        // Same surface as the CLI path (`loom acp run --json`): the caller
        // sees the metered usage instead of having to read the ledger.
        "usage": usage_record,
    }))
}

#[cfg(test)]
mod tests {
    use super::{load_task_brief, resolve_agent_launch};

    #[test]
    fn resolve_agent_launch_finds_executable_in_search_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("fake-agent");
        std::fs::write(&bin, "#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&bin).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&bin, perms).unwrap();
        }

        let launch = resolve_agent_launch(
            dir.path().to_str().unwrap(),
            "fake-agent",
            "t1",
            &[dir.path().to_path_buf()],
        )
        .unwrap();
        assert_eq!(launch.program, bin.to_str().unwrap());
    }

    #[test]
    fn resolve_agent_launch_errors_for_unknown_agent() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_agent_launch(
            dir.path().to_str().unwrap(),
            "no-such-agent",
            "t1",
            &[dir.path().to_path_buf()],
        )
        .unwrap_err();
        assert!(err.to_string().contains("no-such-agent"));
    }

    #[test]
    fn resolve_agent_launch_reads_npm_entry_from_agents_toml() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        std::fs::create_dir_all(loom.path().join("config")).unwrap();
        std::fs::write(
            loom.path().join("config").join("agents.toml"),
            "[my-agent]\nrunner = \"npm\"\npackage = \"my-agent-pkg\"\n",
        )
        .unwrap();

        let launch = resolve_agent_launch(loom_str, "my-agent", "t1", &[]).unwrap();
        assert_eq!(launch.program, "npx");
        assert!(launch.args.contains(&"my-agent-pkg".to_string()));
    }

    #[test]
    fn load_task_brief_prefers_title_and_description() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        std::fs::create_dir_all(loom.path().join("workflow").join("tasks")).unwrap();
        std::fs::write(
            loom.path()
                .join("workflow")
                .join("tasks")
                .join("task-1.json"),
            r#"{"task_id":"task-1","title":"Fix login","description":"Repair the OAuth flow"}"#,
        )
        .unwrap();

        let (topic, prompt) = load_task_brief(loom_str, "task-1");
        assert_eq!(topic, "Fix login");
        assert!(prompt.contains("Fix login"));
        assert!(prompt.contains("Repair the OAuth flow"));
    }

    #[test]
    fn load_task_brief_falls_back_to_task_id() {
        let loom = tempfile::tempdir().unwrap();
        let (topic, prompt) = load_task_brief(loom.path().to_str().unwrap(), "ghost");
        assert_eq!(topic, "ghost");
        assert!(prompt.contains("ghost"));
    }
}
