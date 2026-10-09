//! Tracked ACP dispatch via the canonical ACP protocol client.
//!
//! The daemon's `acp.dispatch` IPC path runs the full initialize →
//! session/new → session/prompt flow against a headless agent and
//! captures the transcript as a Loom session, indexed into
//! cross-session memory and the knowledge graph — the same visibility
//! the CLI `workflow dispatch` path provides.

use loom_core::{Message, MessageRole, SessionManifest};
use loom_generic_adapter::GenericAdapter;
use loom_sessions::acp_protocol::{AcpClientConfig, AcpProtocolClient};
use loom_sessions::store::SessionStore;
use std::path::{Path, PathBuf};

/// A resolved headless agent launch: program + args.
#[derive(Debug)]
pub struct AgentLaunch {
    pub program: String,
    pub args: Vec<String>,
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
/// `search_dirs`, or an npm/pip runner from `{loom_dir}/config/agents.toml`.
/// Unlike the interactive dispatch path this never opens a GUI terminal,
/// so the caller can pipe stdio to log files and wait for completion.
pub fn resolve_agent_launch(
    loom_dir: &str,
    agent_id: &str,
    _task_id: &str,
    search_dirs: &[PathBuf],
) -> anyhow::Result<AgentLaunch> {
    for dir in search_dirs {
        let candidate = dir.join(agent_id);
        if candidate.exists() && is_executable(&candidate) {
            return Ok(AgentLaunch {
                program: candidate.to_string_lossy().to_string(),
                args: Vec::new(),
            });
        }
    }

    let agents_path = Path::new(loom_dir).join("config").join("agents.toml");
    if agents_path.exists() {
        let raw = std::fs::read_to_string(&agents_path)?;
        let table: toml::Table = toml::from_str(&raw)?;
        if let Some(entry) = table.get(agent_id) {
            let runner = entry.get("runner").and_then(|v| v.as_str()).unwrap_or("");
            let package = entry
                .get("package")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args: Vec<String> = entry
                .get("args")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                .unwrap_or_default();
            if runner == "npm" && !package.is_empty() {
                let mut full_args = vec![package.to_string()];
                full_args.extend(args);
                return Ok(AgentLaunch {
                    program: "npx".to_string(),
                    args: full_args,
                });
            }
            if runner == "pip" && !package.is_empty() {
                let mut full_args = vec!["run".to_string(), package.to_string()];
                full_args.extend(args);
                return Ok(AgentLaunch {
                    program: "pipx".to_string(),
                    args: full_args,
                });
            }
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
    (
        task_id.to_string(),
        format!("ACP task {task_id}"),
    )
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
        (prompt.chars().take(80).collect::<String>(), prompt.to_string())
    };
    
    // Record run in registry
    if let Ok(mut registry) = manager.lock() {
        registry.record_external_run(&run_id, task_id, agent_id);
    }
    
    // Resolve agent launch: try GenericAdapter first (uses tool TOML launch config),
    // then fall back to binary lookup (PATH or config/agents.toml).
    let launch = resolve_agent_launch_with_adapter(loom_dir, agent_id, task_id)?;
    
    // Build AcpClientConfig - inject --acp flag for cline
    let mut args = launch.args;
    if launch.program == "cline" || launch.program.ends_with("/cline") {
        args.insert(0, "--acp".to_string());
    }
    let config = AcpClientConfig {
        program: launch.program,
        args,
        cwd: None,
        timeout_secs,
    };
    
    // Run ACP prompt
    let transcript = AcpProtocolClient::run_prompt(&config, &prompt)?;
    
    // Build session using same logic as CLI
    let agent_name = std::path::Path::new(&config.program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(agent_id)
        .to_string();
    
    let session_id = format!(
        "acp-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let now = chrono::Utc::now().to_rfc3339();
    
    let mut assistant = if transcript.agent_text.trim().is_empty() {
        "(agent produced no text output)".to_string()
    } else {
        transcript.agent_text.trim().to_string()
    };
    if !transcript.tool_calls.is_empty() {
        assistant.push_str("\n\nTools:\n");
        for tool in &transcript.tool_calls {
            assistant.push_str(&format!("- {tool}\n"));
        }
    }
    assistant.push_str(&format!("\n[stop reason: {}]", transcript.stop_reason));
    
    let messages = vec![
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: format!(
                "ACP protocol run\n  agent: {}\n  agent_session: {}\n  stop_reason: {}",
                agent_id, transcript.session_id, transcript.stop_reason,
            ),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: prompt.to_string(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: assistant,
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
    ];
    
    let tags = vec![
        "acp".to_string(),
        "acp-protocol".to_string(),
        format!("agent:{}", agent_name),
    ];
    let manifest = SessionManifest {
        session_id: session_id.clone(),
        source_tool: format!("loom-acp/{agent_name}"),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now,
        topic,
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default(),
        model_provider: agent_name,
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Canonical ACP run against '{}' (stop={})",
            agent_id, transcript.stop_reason
        )),
        tags: tags.clone(),
        summarized_at: None,
        summarized_message_count: None,
    };
    
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
            tracing::warn!("ACP dispatch: memory index failed for '{}': {}", manifest.session_id, e);
        }
    }
    
    // Extract into knowledge graph
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(loom_dir_str) {
        if let Err(e) = kg.extract_from_messages(&messages, &manifest.session_id) {
            tracing::warn!("ACP dispatch: knowledge extraction failed for '{}': {}", manifest.session_id, e);
        }
    }
    
    // Update run registry with completion
    if let Ok(mut registry) = manager.lock() {
        registry.complete_run(&run_id, 0, Some(session_id.clone()));
    }
    
    Ok(serde_json::json!({
        "dispatched": true,
        "session_id": manifest.session_id,
        "agent_session_id": transcript.session_id,
        "stop_reason": transcript.stop_reason,
        "tool_calls": transcript.tool_calls,
        "run_id": run_id,
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

        let launch =
            resolve_agent_launch(dir.path().to_str().unwrap(), "fake-agent", "t1", &[
                dir.path().to_path_buf()
            ])
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

        let launch =
            resolve_agent_launch(loom_str, "my-agent", "t1", &[]).unwrap();
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
