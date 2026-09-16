//! Tracked ACP dispatch: headless agent launch resolution plus
//! post-completion session capture.
//!
//! The daemon spawns agent processes without a controlling terminal,
//! so tracked dispatches redirect stdout/stderr to per-run log files
//! under `{loom_dir}/runs/`. When the process exits, [`capture_run_session`]
//! persists the run as a Loom session and indexes it into
//! cross-session memory and the knowledge graph — the same visibility
//! the CLI `workflow dispatch` path provides.

use loom_core::{Message, MessageRole, SessionManifest};
use loom_sessions::store::SessionStore;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Bytes kept from the end of the stdout log for the session capture.
pub const STDOUT_TAIL_BYTES: usize = 64 * 1024;
/// Bytes kept from the end of the stderr log for the session capture.
pub const STDERR_TAIL_BYTES: usize = 16 * 1024;

/// Input for capturing a completed ACP run as a Loom session.
#[derive(Debug)]
pub struct RunCaptureInput {
    pub loom_dir: String,
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub topic: String,
    pub prompt: String,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub exit_code: i32,
}

/// A resolved headless agent launch: program + args + environment.
#[derive(Debug)]
pub struct AgentLaunch {
    pub program: String,
    pub args: Vec<String>,
    pub envs: HashMap<String, String>,
    pub description: String,
}

/// Canonical per-run log file paths. The parent directory is created
/// on demand by the spawner, not here (pure function — easy to test).
pub fn run_log_paths(loom_dir: &str, run_id: &str) -> (PathBuf, PathBuf) {
    let dir = Path::new(loom_dir).join("runs");
    (
        dir.join(format!("{run_id}.stdout.log")),
        dir.join(format!("{run_id}.stderr.log")),
    )
}

/// Return up to `max_bytes` from the end of a log file as a UTF-8
/// string. Missing or unreadable files yield an empty string.
pub fn read_log_tail(path: &Path, max_bytes: usize) -> String {
    let Ok(raw) = std::fs::read(path) else {
        return String::new();
    };
    let start = raw.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&raw[start..]).to_string()
}

fn base_envs(loom_dir: &str, task_id: &str) -> HashMap<String, String> {
    let mut envs = HashMap::new();
    envs.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    envs.insert("LOOM_TASK_ID".to_string(), task_id.to_string());
    envs.insert("LOOM_ENABLED".to_string(), "1".to_string());
    envs
}

/// Environment for a tracked agent spawn: task identity plus optional
/// prior-session context (mirroring the CLI workflow dispatch contract:
/// `LOOM_PRIOR_SESSIONS` as a comma-separated list and
/// `LOOM_PRIOR_SESSIONS_CONTEXT_PATH` pointing at a JSON file with
/// truncated assistant outputs). Pure so the contract is unit-testable.
pub fn tracked_envs(
    loom_dir: &str,
    task_id: &str,
    run_id: &str,
    title: &str,
    prompt: &str,
    prior_sessions: &[String],
    context_path: Option<&Path>,
) -> HashMap<String, String> {
    let mut envs = base_envs(loom_dir, task_id);
    envs.insert("LOOM_RUN_ID".to_string(), run_id.to_string());
    envs.insert("LOOM_TASK_TITLE".to_string(), title.to_string());
    envs.insert("LOOM_TASK_DESCRIPTION".to_string(), prompt.to_string());
    let priors: Vec<String> = prior_sessions
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if !priors.is_empty() {
        envs.insert("LOOM_PRIOR_SESSIONS".to_string(), priors.join(","));
    }
    if let Some(path) = context_path {
        envs.insert(
            "LOOM_PRIOR_SESSIONS_CONTEXT_PATH".to_string(),
            path.to_string_lossy().to_string(),
        );
    }
    envs
}

/// Build a JSON context file with the truncated assistant message
/// bodies of the given prior sessions (latest first per session is
/// unnecessary — chronological file order is preserved). Missing or
/// message-less sessions are skipped. Returns the file path, or `None`
/// when nothing valid was found.
pub fn build_prior_sessions_context(
    loom_dir: &str,
    run_id: &str,
    session_ids: &[String],
) -> Option<PathBuf> {
    const BODY_CHARS: usize = 4096;
    let store = SessionStore::new(loom_dir);
    let mut entries = Vec::new();
    for session_id in session_ids {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            continue;
        }
        let Ok(messages) = store.load_messages(session_id) else {
            continue;
        };
        let mut bodies = Vec::new();
        for message in &messages {
            if matches!(message.role, loom_core::MessageRole::Assistant) {
                let truncated: String = message.content.chars().take(BODY_CHARS).collect();
                bodies.push(serde_json::json!({ "content": truncated }));
            }
        }
        if bodies.is_empty() {
            continue;
        }
        entries.push(serde_json::json!({
            "session_id": session_id,
            "messages": bodies,
        }));
    }
    if entries.is_empty() {
        return None;
    }
    let dir = Path::new(loom_dir).join("runs");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join(format!("{run_id}.prior_sessions.json"));
    let payload = serde_json::json!({
        "run_id": run_id,
        "prior_sessions": entries,
    });
    match serde_json::to_string_pretty(&payload) {
        Ok(text) => std::fs::write(&path, text).ok().map(|()| path),
        Err(_) => None,
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
/// `search_dirs`, or an npm/pip runner from `{loom_dir}/config/agents.toml`.
/// Unlike the interactive dispatch path this never opens a GUI terminal,
/// so the caller can pipe stdio to log files and wait for completion.
pub fn resolve_agent_launch(
    loom_dir: &str,
    agent_id: &str,
    task_id: &str,
    search_dirs: &[PathBuf],
) -> anyhow::Result<AgentLaunch> {
    for dir in search_dirs {
        let candidate = dir.join(agent_id);
        if candidate.exists() && is_executable(&candidate) {
            return Ok(AgentLaunch {
                program: candidate.to_string_lossy().to_string(),
                args: Vec::new(),
                envs: base_envs(loom_dir, task_id),
                description: format!("native binary {}", candidate.display()),
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
            if runner == "npm" && !package.is_empty() {
                return Ok(AgentLaunch {
                    program: "npx".to_string(),
                    args: vec![package.to_string()],
                    envs: base_envs(loom_dir, task_id),
                    description: format!("npx {package}"),
                });
            }
            if runner == "pip" && !package.is_empty() {
                return Ok(AgentLaunch {
                    program: "pipx".to_string(),
                    args: vec!["run".to_string(), package.to_string()],
                    envs: base_envs(loom_dir, task_id),
                    description: format!("pipx run {package}"),
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

/// Persist a completed run as a Loom session and index it into
/// cross-session memory and the knowledge graph. Returns the session id.
///
/// Storage writes propagate errors; memory/knowledge indexing is
/// best-effort (a corrupt index must not lose the session itself),
/// mirroring the CLI dispatch-capture policy.
pub fn capture_run_session(input: &RunCaptureInput) -> anyhow::Result<String> {
    // Distinct runs must not overwrite each other: scope the session id
    // to the run, keeping it filesystem-safe.
    let short: String = input
        .run_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let session_id = format!("{}.{short}", input.task_id);
    let now = chrono::Utc::now().to_rfc3339();

    let mut messages = Vec::new();
    messages.push(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::System,
        content: format!(
            "ACP run dispatch\n  run_id: {}\n  task_id: {}\n  topic: {}\n  agent: {}\n  exit_code: {}",
            input.run_id, input.task_id, input.topic, input.agent_id, input.exit_code,
        ),
        tool_calls: None,
        timestamp: now.clone(),
        metadata: Default::default(),
    });
    messages.push(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::User,
        content: if input.prompt.trim().is_empty() {
            "(no prompt recorded)".to_string()
        } else {
            input.prompt.clone()
        },
        tool_calls: None,
        timestamp: now.clone(),
        metadata: Default::default(),
    });

    let stdout_text = input.stdout_tail.trim();
    let stderr_text = input.stderr_tail.trim();
    let mut assistant_content = if stdout_text.is_empty() {
        "(agent produced no captured output)".to_string()
    } else {
        stdout_text.to_string()
    };
    if !stderr_text.is_empty() {
        assistant_content.push_str("\n\n--- stderr ---\n");
        assistant_content.push_str(stderr_text);
    }
    if input.exit_code != 0 {
        assistant_content.push_str(&format!(
            "\n\n[agent exited with status {}]",
            input.exit_code
        ));
    }
    let mut assistant_metadata = HashMap::new();
    assistant_metadata.insert(
        "agent".to_string(),
        serde_json::Value::String(input.agent_id.clone()),
    );
    assistant_metadata.insert(
        "exit_code".to_string(),
        serde_json::Value::from(input.exit_code),
    );
    assistant_metadata.insert(
        "run_id".to_string(),
        serde_json::Value::String(input.run_id.clone()),
    );
    messages.push(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content: assistant_content,
        tool_calls: None,
        timestamp: now.clone(),
        metadata: assistant_metadata,
    });

    let mut tags = vec![
        "acp".to_string(),
        format!("agent:{}", input.agent_id),
        format!("task:{}", input.task_id),
        if input.exit_code == 0 {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
    ];
    tags.dedup();

    let manifest = SessionManifest {
        session_id: session_id.clone(),
        source_tool: format!("loom-acp/{}", input.agent_id),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now,
        topic: input.topic.clone(),
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default(),
        model_provider: input.agent_id.clone(),
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Captured from ACP run '{}' (agent={}, exit={})",
            input.run_id, input.agent_id, input.exit_code,
        )),
        tags: tags.clone(),
        summarized_at: None,
        summarized_message_count: None,
    };

    let store = SessionStore::new(&input.loom_dir);
    store.save_manifest(&manifest)?;
    store.replace_messages(&session_id, &messages)?;

    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(&input.loom_dir) {
        if let Err(e) =
            memory.index_session_with_tags(&session_id, &messages, &input.topic, &tags)
        {
            tracing::warn!("ACP capture: memory index failed for '{session_id}': {e}");
        }
    }
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(&input.loom_dir) {
        if let Err(e) = kg.extract_from_messages(&messages, &session_id) {
            tracing::warn!("ACP capture: knowledge extraction failed for '{session_id}': {e}");
        }
    }

    Ok(session_id)
}

/// A spawned tracked agent process plus its log files.
pub struct TrackedChild {
    pub child: std::process::Child,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
}

/// Context a background waiter thread needs to finish a run.
#[derive(Debug, Clone)]
pub struct TrackedRun {
    pub loom_dir: String,
    pub run_id: String,
    pub task_id: String,
    pub agent_id: String,
    pub topic: String,
    pub prompt: String,
}

/// Extract prior-session ids from a dispatch params/payload object.
/// Non-string and blank entries are dropped; missing or non-array
/// input yields an empty list. Shared by the IPC and TCP paths.
pub fn prior_sessions_from_params(params: &serde_json::Value) -> Vec<String> {
    params
        .get("prior_sessions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The single tracked-dispatch flow used by every daemon entry point:
/// validate → mint/reuse run id → resolve + spawn headlessly → record
/// the run → background thread waits, captures a session, and records
/// completion. Returns the response data JSON (with `run_id`).
pub fn dispatch_tracked(
    loom_dir: &str,
    manager: &std::sync::Arc<std::sync::Mutex<crate::run_manager::RunManager>>,
    task_id: &str,
    agent_id: &str,
    requested_run_id: Option<&str>,
    prior_sessions: Vec<String>,
) -> anyhow::Result<serde_json::Value> {
    if task_id.is_empty() || agent_id.is_empty() {
        anyhow::bail!("task_id and agent_id required");
    }
    let run_id = match requested_run_id {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => uuid::Uuid::new_v4().to_string(),
    };
    let (topic, prompt) = load_task_brief(loom_dir, task_id);
    let search_dirs: Vec<PathBuf> = std::env::var("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let tracked = spawn_tracked_agent(
        loom_dir,
        agent_id,
        task_id,
        &run_id,
        &topic,
        &prompt,
        &prior_sessions,
        &search_dirs,
    )?;
    // Record only after a successful spawn so the registry reflects
    // real dispatches.
    if manager
        .lock()
        .map(|mut registry| {
            registry.record_external_run(&run_id, task_id, agent_id);
        })
        .is_err()
    {
        anyhow::bail!("run registry unavailable");
    }
    let waiter_manager = manager.clone();
    let run = TrackedRun {
        loom_dir: loom_dir.to_string(),
        run_id: run_id.clone(),
        task_id: task_id.to_string(),
        agent_id: agent_id.to_string(),
        topic,
        prompt,
    };
    std::thread::spawn(move || wait_and_capture(tracked, &run, &waiter_manager));
    Ok(serde_json::json!({
        "dispatched": true,
        "message": format!(
            "Dispatched task {task_id} to {agent_id} (run {run_id}; logs in runs/{run_id}.{{stdout,stderr}}.log)"
        ),
        "run_id": run_id,
    }))
}
pub fn spawn_tracked_agent(
    loom_dir: &str,
    agent_id: &str,
    task_id: &str,
    run_id: &str,
    topic: &str,
    prompt: &str,
    prior_sessions: &[String],
    search_dirs: &[PathBuf],
) -> anyhow::Result<TrackedChild> {
    let launch = resolve_agent_launch(loom_dir, agent_id, task_id, search_dirs)?;
    let (stdout_log, stderr_log) = run_log_paths(loom_dir, run_id);
    if let Some(parent) = stdout_log.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stdout_file = std::fs::File::create(&stdout_log)?;
    let stderr_file = std::fs::File::create(&stderr_log)?;

    let context_path = build_prior_sessions_context(loom_dir, run_id, prior_sessions);
    let mut envs = launch.envs;
    envs.extend(tracked_envs(
        loom_dir,
        task_id,
        run_id,
        topic,
        prompt,
        prior_sessions,
        context_path.as_deref(),
    ));

    let mut cmd = std::process::Command::new(&launch.program);
    cmd.args(&launch.args);
    if !prompt.trim().is_empty() {
        cmd.arg(prompt);
    }
    cmd.envs(&envs);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::from(stdout_file));
    cmd.stderr(std::process::Stdio::from(stderr_file));
    let child = cmd.spawn().map_err(|e| {
        anyhow::anyhow!("failed to spawn '{}': {e}", launch.description)
    })?;
    Ok(TrackedChild {
        child,
        stdout_log,
        stderr_log,
    })
}

/// Block until the agent process exits, then capture the run as a
/// Loom session and record the outcome in the run registry. Runs on a
/// dedicated thread — never on the IPC handler path.
pub fn wait_and_capture(
    mut tracked: TrackedChild,
    run: &TrackedRun,
    manager: &std::sync::Mutex<crate::run_manager::RunManager>,
) {
    let exit_code = match tracked.child.wait() {
        Ok(status) => status.code().unwrap_or(-1),
        Err(e) => {
            tracing::warn!("ACP run '{}': waiting for agent failed: {e}", run.run_id);
            -1
        }
    };
    let input = RunCaptureInput {
        loom_dir: run.loom_dir.clone(),
        run_id: run.run_id.clone(),
        task_id: run.task_id.clone(),
        agent_id: run.agent_id.clone(),
        topic: run.topic.clone(),
        prompt: run.prompt.clone(),
        stdout_tail: read_log_tail(&tracked.stdout_log, STDOUT_TAIL_BYTES),
        stderr_tail: read_log_tail(&tracked.stderr_log, STDERR_TAIL_BYTES),
        exit_code,
    };
    match capture_run_session(&input) {
        Ok(session_id) => {
            tracing::info!(
                "ACP run '{}' (task {}) exited with {exit_code}; captured session '{session_id}'",
                run.run_id,
                run.task_id
            );
            if let Ok(mut registry) = manager.lock() {
                registry.complete_run(&run.run_id, exit_code, Some(session_id));
            }
        }
        Err(e) => {
            tracing::warn!(
                "ACP run '{}': session capture failed: {e}",
                run.run_id
            );
            if let Ok(mut registry) = manager.lock() {
                registry.complete_run(&run.run_id, exit_code, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capture_run_session, load_task_brief, resolve_agent_launch, run_log_paths,
        RunCaptureInput,
    };
    use loom_sessions::store::SessionStore;

    fn test_input(loom_dir: &str, run_id: &str, marker: &str) -> RunCaptureInput {
        RunCaptureInput {
            loom_dir: loom_dir.to_string(),
            run_id: run_id.to_string(),
            task_id: "task-abc".to_string(),
            agent_id: "opencode".to_string(),
            topic: "Add dark mode".to_string(),
            prompt: "Add dark mode to settings".to_string(),
            stdout_tail: format!("done: {marker}"),
            stderr_tail: String::new(),
            exit_code: 0,
        }
    }

    #[test]
    fn run_log_paths_are_scoped_per_run_under_loom_runs_dir() {
        let (stdout, stderr) = run_log_paths("/tmp/loom-test", "run-1");
        assert_eq!(
            stdout,
            std::path::PathBuf::from("/tmp/loom-test/runs/run-1.stdout.log")
        );
        assert_eq!(
            stderr,
            std::path::PathBuf::from("/tmp/loom-test/runs/run-1.stderr.log")
        );
    }

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
        assert_eq!(
            launch.envs.get("LOOM_TASK_ID").map(String::as_str),
            Some("t1")
        );
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

    #[test]
    fn capture_run_session_persists_manifest_and_messages() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        let session_id =
            capture_run_session(&test_input(loom_str, "run-11111111", "zephyrmelon"))
                .unwrap();

        let store = SessionStore::new(loom_str);
        let manifest = store.get(&session_id).expect("manifest must exist");
        assert_eq!(manifest.topic, "Add dark mode");
        assert!(manifest.tags.iter().any(|t| t == "acp"));

        let messages = store.load_messages(&session_id).unwrap();
        assert_eq!(messages.len(), 3);
        let roles: Vec<String> =
            messages.iter().map(|m| format!("{:?}", m.role)).collect();
        assert_eq!(roles, vec!["System", "User", "Assistant"]);
        assert!(messages[2].content.contains("zephyrmelon"));
    }

    #[test]
    fn capture_run_session_indexes_memory() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        capture_run_session(&test_input(loom_str, "run-22222222", "quetzalplum"))
            .unwrap();

        let memory =
            loom_sessions::memory::CrossSessionMemory::new(loom_str).unwrap();
        let hits = memory.search("quetzalplum", 5).unwrap();
        assert!(!hits.is_empty(), "captured output must be searchable");
    }

    #[test]
    fn capture_run_session_records_failure_note_and_tag() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        let mut input = test_input(loom_str, "run-33333333", "wombatfig");
        input.exit_code = 2;
        let session_id = capture_run_session(&input).unwrap();

        let store = SessionStore::new(loom_str);
        let manifest = store.get(&session_id).expect("manifest must exist");
        assert!(manifest.tags.iter().any(|t| t == "failed"));
        let messages = store.load_messages(&session_id).unwrap();
        assert!(messages[2].content.contains('2'));
    }

    #[test]
    fn capture_run_session_returns_distinct_ids_per_run() {
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        let first =
            capture_run_session(&test_input(loom_str, "run-aaaa1111", "m1")).unwrap();
        let second =
            capture_run_session(&test_input(loom_str, "run-bbbb2222", "m2")).unwrap();
        assert_ne!(first, second);
        assert!(first.contains("task-abc"));
    }

    #[test]
    fn build_prior_sessions_context_collects_assistant_bodies() {
        use super::build_prior_sessions_context;
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        let seeded =
            capture_run_session(&test_input(loom_str, "run-ctx-1111", "contextmarker"))
                .unwrap();

        let path = build_prior_sessions_context(
            loom_str,
            "run-ctx-9999",
            &["ghost-session".to_string(), seeded.clone()],
        )
        .expect("one valid session must produce a context file");
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(&seeded), "got: {raw}");
        assert!(raw.contains("contextmarker"), "got: {raw}");
    }

    #[test]
    fn build_prior_sessions_context_returns_none_without_valid_sessions() {
        use super::build_prior_sessions_context;
        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        assert!(build_prior_sessions_context(loom_str, "run-x", &[]).is_none());
        assert!(
            build_prior_sessions_context(loom_str, "run-x", &["ghost".to_string()])
                .is_none()
        );
    }

    #[test]
    fn tracked_envs_carry_task_and_prior_context() {        use super::tracked_envs;
        use std::path::Path;
        let envs = tracked_envs(
            "loomdir",
            "t1",
            "run1",
            "Title",
            "Prompt",
            &["s1".to_string(), "s2".to_string()],
            Some(Path::new("/tmp/ctx.json")),
        );
        assert_eq!(envs.get("LOOM_TASK_ID").map(String::as_str), Some("t1"));
        assert_eq!(envs.get("LOOM_RUN_ID").map(String::as_str), Some("run1"));
        assert_eq!(
            envs.get("LOOM_TASK_TITLE").map(String::as_str),
            Some("Title")
        );
        assert_eq!(
            envs.get("LOOM_PRIOR_SESSIONS").map(String::as_str),
            Some("s1,s2")
        );
        assert_eq!(
            envs
                .get("LOOM_PRIOR_SESSIONS_CONTEXT_PATH")
                .map(String::as_str),
            Some("/tmp/ctx.json")
        );

        let bare = tracked_envs("loomdir", "t1", "run1", "Title", "Prompt", &[], None);
        assert!(!bare.contains_key("LOOM_PRIOR_SESSIONS"));
        assert!(!bare.contains_key("LOOM_PRIOR_SESSIONS_CONTEXT_PATH"));
    }

    /// End-to-end through a real process: spawn, wait, capture, and
    /// record completion. Uses `/bin/echo` so the test stays hermetic
    /// (unix-only: process launch semantics differ on Windows).
    #[cfg(unix)]
    #[test]
    fn wait_and_capture_records_completion_and_session() {        use super::{spawn_tracked_agent, wait_and_capture, TrackedRun};
        use crate::run_manager::{RunManager, RunStatus};

        let loom = tempfile::tempdir().unwrap();
        let loom_str = loom.path().to_str().unwrap();
        let manager = std::sync::Mutex::new(RunManager::new());
        let run_id = manager
            .lock()
            .unwrap()
            .record_external_run("run-e2e-0001", "task-e2e", "echo");

        let tracked = spawn_tracked_agent(
            loom_str,
            "echo",
            "task-e2e",
            &run_id,
            "e2e topic",
            "e2ecompletionmarker",
            &[],
            &[std::path::PathBuf::from("/bin")],
        )
        .unwrap();
        wait_and_capture(
            tracked,
            &TrackedRun {
                loom_dir: loom_str.to_string(),
                run_id: run_id.clone(),
                task_id: "task-e2e".to_string(),
                agent_id: "echo".to_string(),
                topic: "e2e topic".to_string(),
                prompt: "e2ecompletionmarker".to_string(),
            },
            &manager,
        );

        let run = manager.lock().unwrap().get_run(&run_id).unwrap();
        assert_eq!(run.status, RunStatus::Succeeded);
        assert_eq!(run.exit_code, Some(0));
        let session_id = run.session_id.expect("session must be linked");

        let store = SessionStore::new(loom_str);
        let messages = store.load_messages(&session_id).unwrap();
        assert!(messages[2].content.contains("e2ecompletionmarker"));
        let memory =
            loom_sessions::memory::CrossSessionMemory::new(loom_str).unwrap();
        assert!(!memory.search("e2ecompletionmarker", 5).unwrap().is_empty());
    }

    #[test]
    fn prior_sessions_from_params_filters_blanks_and_non_strings() {
        use super::prior_sessions_from_params;
        let params = serde_json::json!({"prior_sessions": ["s1", 7, "  ", "s2"]});
        assert_eq!(
            prior_sessions_from_params(&params),
            vec!["s1".to_string(), "s2".to_string()]
        );
        assert!(prior_sessions_from_params(&serde_json::json!({})).is_empty());
        assert!(prior_sessions_from_params(&serde_json::json!({"prior_sessions": "nope"})).is_empty());
    }

    /// Full unified dispatch through a real process: record, spawn,
    /// background completion, session capture. Unix-only (`/bin/echo`).
    #[cfg(unix)]
    #[test]
    fn dispatch_tracked_records_run_and_completes() {
        use super::dispatch_tracked;
        use crate::run_manager::{RunManager, RunStatus};
        let loom = tempfile::tempdir().unwrap();
        let manager = std::sync::Arc::new(std::sync::Mutex::new(RunManager::new()));
        let data = dispatch_tracked(
            loom.path().to_str().unwrap(),
            &manager,
            "task-unified",
            "echo",
            None,
            vec![],
        )
        .unwrap();
        let run_id = data["run_id"].as_str().expect("response carries run_id").to_string();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            {
                let registry = manager.lock().unwrap();
                if let Some(run) = registry.get_run(&run_id) {
                    if matches!(run.status, RunStatus::Succeeded) {
                        assert_eq!(run.exit_code, Some(0));
                        assert!(run.session_id.is_some());
                        return;
                    }
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "unified dispatch did not complete in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
