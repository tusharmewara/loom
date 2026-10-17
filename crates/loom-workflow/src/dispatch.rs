//! Agent dispatch and capture machinery for workflow tasks: launch an
//! agent with task context, tee its output to per-dispatch logs, capture
//! phase results, fire PostPhase hooks, and persist dispatches as sessions
//! (indexed into cross-session memory + the knowledge graph).
//!
//! Shared between the CLI and the daemon so both dispatch paths behave
//! identically.

use std::collections::HashMap;

/// This function is designed for `loom workflow dispatch` to execute tasks
/// using the Runtime abstraction (process, docker, podman, kubernetes)
/// while capturing stdout/stderr to log files.
///
/// Returns the exit code for the auto-advance loop.
pub async fn execute_workflow_task(
    loom_dir: &str,
    agent_id: &str,
    launch_config: Option<loom_runtime::LaunchConfig>,
    task: &loom_core::Task,
    stdout_log: &std::path::Path,
    stderr_log: &std::path::Path,
) -> anyhow::Result<i32> {
    use std::fs::File;
    use std::io::Write;

    // Get launch config from tool's TOML if not provided
    let config = if let Some(cfg) = launch_config {
        cfg
    } else {
        loom_generic_adapter::GenericAdapter::new(loom_dir, agent_id)
            .ok()
            .and_then(|adapter| adapter.launch_config())
            .unwrap_or_else(|| loom_runtime::LaunchConfig {
                runtime: "process".to_string(),
                command: agent_id.to_string(),
                args: vec![],
                env: HashMap::new(),
                working_dir: None,
                image: None,
                network: None,
                auto_remove: true,
            })
    };

    // Create runtime registry with all available runtimes
    let mut registry = loom_runtime::RuntimeRegistry::new();
    let _ = registry.register(Box::new(loom_runtime::ProcessRuntime::new()));
    #[cfg(feature = "docker")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::docker()));
    }
    #[cfg(feature = "podman")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::podman()));
    }
    #[cfg(feature = "kubernetes")]
    {
        let _ = registry.register(Box::new(loom_runtime::ContainerRuntime::kubernetes()));
    }

    // Get the runtime
    let runtime = registry
        .get(&config.runtime)
        .ok_or_else(|| anyhow::anyhow!("Runtime '{}' not available", config.runtime))?;

    // Prepare, run, cleanup
    runtime.prepare(&config).await?;
    let result = runtime.run(&config, task).await?;
    runtime.cleanup(&result).await?;

    // Write stdout/stderr to log files
    if let Ok(mut f) = File::create(stdout_log) {
        let _ = f.write_all(result.stdout.as_bytes());
    }
    if let Ok(mut f) = File::create(stderr_log) {
        let _ = f.write_all(result.stderr.as_bytes());
    }

    Ok(result.exit_code.unwrap_or(-1))
}

pub async fn launch_agent_with_task_context(
    loom_dir: &str,
    task: &loom_core::Task,
    agent: &str,
    tool_args: &[String],
    dry_run: bool,
    prior_sessions: &[String],
    runtime: Option<String>,
) -> anyhow::Result<i32> {
    // If runtime is specified, use the Runtime abstraction
    if let Some(ref rt) = runtime {
        let phase_name = task.phase.name();
        let stdout_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stdout");
        let stderr_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stderr");

        // Build launch config from tool's TOML and CLI args
        let launch_config = if runtime.is_some() {
            Some(loom_runtime::LaunchConfig {
                runtime: rt.clone(),
                command: agent.to_string(),
                args: tool_args.to_vec(),
                env: HashMap::new(),
                working_dir: None,
                image: None,
                network: None,
                auto_remove: true,
            })
        } else {
            None
        };

        return execute_workflow_task(
            loom_dir,
            agent,
            launch_config,
            task,
            &stdout_log,
            &stderr_log,
        )
        .await;
    }

    let home = loom_core::home_dir();

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    env_vars.insert("LOOM_ENABLED".to_string(), "1".to_string());
    env_vars.insert("LOOM_TASK_ID".to_string(), task.task_id.clone());
    env_vars.insert("LOOM_TASK_PHASE".to_string(), format!("{:?}", task.phase));
    env_vars.insert("LOOM_TASK_TITLE".to_string(), task.title.clone());
    env_vars.insert(
        "LOOM_TASK_DESCRIPTION".to_string(),
        task.description.clone(),
    );

    // Inject routing decision from the task as env vars
    if let Some(rd) = &task.routing_decision {
        env_vars.insert(
            "LOOM_ROUTING_PROVIDER".to_string(),
            rd.recommended_provider.clone(),
        );
        env_vars.insert(
            "LOOM_ROUTING_MODEL".to_string(),
            rd.recommended_model.clone(),
        );
        env_vars.insert(
            "LOOM_ROUTING_TIER".to_string(),
            format!("{:?}", rd.recommended_tier),
        );
    }
    if let Some(rp) = &task.routed_provider {
        env_vars.insert("LOOM_ROUTED_PROVIDER".to_string(), rp.clone());
    }

    // Inject linked issue if present
    if let Some(issue) = &task.linked_issue {
        env_vars.insert("LOOM_LINKED_ISSUE".to_string(), issue.clone());
    }

    // Set up the agent tool adapter and apply routing
    if let Some(adapter) = loom_adapters::adapter_for(agent) {
        let toml = adapter.default_toml(&home);

        // Skills
        if let Some(ref skills) = toml.skills {
            if let Some(ref native_dir) = skills.native_dir {
                env_vars.insert("LOOM_SKILLS_DIR".to_string(), format!("{loom_dir}/skills"));
                env_vars.insert(
                    format!("{}_SKILLS_DIR", agent.to_uppercase().replace('-', "_")),
                    native_dir.clone(),
                );
            }
        }

        // MCP
        if let Some(ref mcp) = toml.mcp {
            if let Some(ref config_path) = mcp.config_path {
                env_vars.insert("LOOM_MCP_DIR".to_string(), format!("{loom_dir}/mcp"));
                env_vars.insert(
                    format!("{}_MCP_CONFIG", agent.to_uppercase().replace('-', "_")),
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
                    format!("{}_SESSIONS_DIR", agent.to_uppercase().replace('-', "_")),
                    store_path.clone(),
                );
            }
        }

        // Apply the routing decision through the adapter (sets {TOOL}_MODEL etc.)
        if let Some(rd) = &task.routing_decision {
            adapter.apply_routing_decision(rd)?;
            // Read back the tool-native model env var so we capture it in dry-run
            let tool_key = format!("{}_MODEL", agent.to_uppercase().replace('-', "_"));
            if let Ok(val) = std::env::var(&tool_key) {
                env_vars.insert(tool_key, val);
            }
            let provider_key = "LOOM_ROUTING_PROVIDER".to_string();
            if let Ok(val) = std::env::var(&provider_key) {
                env_vars.insert(provider_key, val);
            }
        }
    } else {
        // No adapter — set generic routing env vars directly
        if let Some(rd) = &task.routing_decision {
            env_vars.insert(
                format!("{}_MODEL", agent.to_uppercase().replace('-', "_")),
                rd.recommended_model.clone(),
            );
        }
    }

    // Find the binary
    let binary_path = loom_core::find_binary(agent)?;

    // Compute the canonical phase-result path so the agent knows where to
    // write a result file (consumed by the next dispatch in --auto-advance).
    let phase_name = task.phase.name();
    let result_path = phase_result_path(loom_dir, &task.task_id, phase_name);
    env_vars.insert(
        "LOOM_EXPECTED_RESULT_PATH".to_string(),
        result_path.display().to_string(),
    );

    // Inject the most recent phase result (if any) so the next phase's agent
    // can see what the prior agent produced.
    if let Some(last) = task.phase_results.last() {
        env_vars.insert(
            "LOOM_PRIOR_PHASE_RESULT".to_string(),
            serde_json::to_string(last).unwrap_or_default(),
        );
    }

    // Inject prior sessions captured for prior phases of this task. The
    // env var is a comma-separated list of session IDs; the agent can
    // `loom sessions show <id>` to see them. For agents that prefer a
    // single file, we also write a small context file with the prior
    // assistant message bodies (truncated) so they don't have to
    // round-trip through the database.
    if !prior_sessions.is_empty() {
        env_vars.insert("LOOM_PRIOR_SESSIONS".to_string(), prior_sessions.join(","));
        if let Some(ctx_path) =
            build_prior_sessions_context(loom_dir, &task.task_id, prior_sessions)
        {
            env_vars.insert(
                "LOOM_PRIOR_SESSIONS_CONTEXT_PATH".to_string(),
                ctx_path.display().to_string(),
            );
        }
    }

    // Build the prompt: if no explicit tool_args, use task title + description
    // as the prompt passed to the agent.
    let final_args: Vec<String> = if tool_args.is_empty() {
        let mut prompt = if task.description.is_empty() {
            task.title.clone()
        } else {
            format!("{} — {}", task.title, task.description)
        };

        // Truncate to a reasonable length (most agents have an arg limit)
        const MAX_PROMPT_LEN: usize = 4096;
        if prompt.len() > MAX_PROMPT_LEN {
            prompt.truncate(MAX_PROMPT_LEN);
            prompt.push('…');
        }
        vec![prompt]
    } else {
        tool_args.to_vec()
    };

    if dry_run {
        println!(
            "\n=== Dry Run: dispatch {} with {} ===",
            task.task_id, agent
        );
        println!("Binary: {}", binary_path.display());
        println!("Args: {:?}", final_args);
        println!("\nTask context:");
        println!("  phase={:?}  title={}", task.phase, task.title);
        if let Some(rd) = &task.routing_decision {
            println!(
                "  routing: provider={}  model={}  tier={:?}",
                rd.recommended_provider, rd.recommended_model, rd.recommended_tier
            );
        }
        println!("\nEnvironment variables:");
        for (key, value) in &env_vars {
            println!("  {key}={value}");
        }
        return Ok(0);
    }

    // Spawn the agent tool. We do NOT bail on non-zero exit here so the
    // --auto-advance loop can decide whether to stop or continue.
    //
    // We pipe stdout/stderr through tee-style threads: a copy goes to the
    // terminal (so the user still sees agent output interactively) and a
    // copy goes to a per-dispatch log file (so the session-capture step
    // can index the agent's actual output into cross-session memory).
    let stdout_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stdout");
    let stderr_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stderr");

    let mut cmd = std::process::Command::new(&binary_path);
    cmd.args(&final_args);
    cmd.envs(&env_vars);
    cmd.stdin(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();

    // Spawn tee threads: each reads half of the pipe and writes to both the
    // original terminal FD and the log file.
    let stdout_thread = child_stdout.map(|pipe| {
        let path = stdout_log.clone();
        std::thread::spawn(move || tee_pipe(pipe, &path))
    });
    let stderr_thread = child_stderr.map(|pipe| {
        let path = stderr_log.clone();
        std::thread::spawn(move || tee_pipe(pipe, &path))
    });

    let status = child.wait()?;
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }

    // Stash the exit code for `last_exit_code()` consumers (auto-advance loop,
    // PostPhase hooks). This is a process-local slot; safe because the
    // dispatch is synchronous within a single CLI invocation.
    LAST_EXIT_CODE.with(|cell| {
        *cell.borrow_mut() = status.code().unwrap_or(-1);
    });
    if !status.success() {
        eprintln!(
            "⚠  Agent tool '{agent}' exited with status: {status} (continuing per dispatch policy)"
        );
    }
    Ok(status.code().unwrap_or(-1))
}

// ─── Phase-result capture and PostPhase hooks ──────────────────────────

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    /// Exit code from the most recent agent dispatch. Read by the
    /// auto-advance loop and PostPhase hooks.
    static LAST_EXIT_CODE: RefCell<i32> = const { RefCell::new(0) };
}

pub fn last_exit_code() -> i32 {
    LAST_EXIT_CODE.with(|cell| *cell.borrow())
}

/// Canonical path where the agent for a given phase should write its result.
/// `~/.loom/workflow/tasks/{task_id}.{phase}.result.json` (or `.md`).
pub fn phase_result_path(loom_dir: &str, task_id: &str, phase_name: &str) -> PathBuf {
    PathBuf::from(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.{phase_name}.result.json"))
}

/// Read the phase result file the agent wrote. Supports either a JSON
/// `PhaseResult` (preferred) or a plain Markdown summary (wrapped into a
/// `PhaseResult` with no artifacts). Returns `Ok(None)` if no file was found.
pub fn capture_phase_result(
    loom_dir: &str,
    task_id: &str,
    phase_name: &str,
    agent: &str,
) -> anyhow::Result<Option<loom_core::PhaseResult>> {
    let json_path = phase_result_path(loom_dir, task_id, phase_name);
    let md_path = json_path.with_extension("md");

    // Preferred: a structured JSON PhaseResult.
    if json_path.exists() {
        let raw = std::fs::read_to_string(&json_path)?;
        // Try strict PhaseResult first; fall back to a thin wrapper.
        if let Ok(pr) = serde_json::from_str::<loom_core::PhaseResult>(&raw) {
            return Ok(Some(pr));
        }
        // Fallback: treat the JSON as `{ "summary": "...", "artifacts": [...] }`.
        #[derive(serde::Deserialize)]
        struct Loose {
            #[serde(default)]
            summary: String,
            #[serde(default)]
            artifacts: Vec<String>,
        }
        if let Ok(loose) = serde_json::from_str::<Loose>(&raw) {
            return Ok(Some(loom_core::PhaseResult {
                phase: phase_name.to_string(),
                agent: agent.to_string(),
                summary: loose.summary,
                artifacts: loose.artifacts,
                completed_at: loom_core::rfc3339_now(),
            }));
        }
        // Last resort: use the raw JSON as the summary.
        return Ok(Some(loom_core::PhaseResult {
            phase: phase_name.to_string(),
            agent: agent.to_string(),
            summary: raw,
            artifacts: vec![],
            completed_at: loom_core::rfc3339_now(),
        }));
    }

    // Fallback: a plain markdown summary.
    if md_path.exists() {
        let summary = std::fs::read_to_string(&md_path)?;
        return Ok(Some(loom_core::PhaseResult {
            phase: phase_name.to_string(),
            agent: agent.to_string(),
            summary,
            artifacts: vec![],
            completed_at: loom_core::rfc3339_now(),
        }));
    }

    Ok(None)
}

/// Fire all `PostPhase` hooks. Errors are logged but never propagated —
/// the chain should keep going even if a hook script fails.
pub async fn fire_post_phase_hooks(
    loom_dir: &str,
    task_id: &str,
    phase_name: &str,
    agent: &str,
    exit_code: i32,
) {
    let registry = loom_hooks::HookRegistry::new();
    // Load any persisted hooks from ~/.loom/hooks/*.toml
    let hooks_dir = std::path::Path::new(loom_dir).join("hooks");
    if hooks_dir.exists() {
        if let Err(e) = registry.load_from_dir(&hooks_dir).await {
            eprintln!("⚠  Failed to load hooks from {}: {e}", hooks_dir.display());
            return;
        }
    } else {
        return;
    }
    match loom_hooks::run_post_phase_hooks(
        &registry,
        task_id,
        phase_name,
        agent,
        exit_code,
        loom_hooks::require_signature(loom_dir),
    )
    .await
    {
        Ok(n) if n > 0 => println!("🔗 Fired {n} PostPhase hook(s)"),
        Ok(_) => {} // no hooks registered — silent
        Err(e) => eprintln!("⚠  PostPhase hook error: {e}"),
    }
}

// ─── Dispatch output capture (session tee) ───────────────────────────

/// Canonical path of the per-dispatch log file. Used as both the tee
/// destination and the source for `capture_dispatch_session` to build a
/// `SessionManifest` from.
///
/// Layout: `~/.loom/workflow/tasks/{task_id}.{phase}.{stream}.log`
pub fn capture_log_path(loom_dir: &str, task_id: &str, phase_name: &str, stream: &str) -> PathBuf {
    PathBuf::from(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.{phase_name}.{stream}.log"))
}

/// Read a child process's pipe in 8KB chunks, writing each chunk to both
/// the originating terminal FD (1 for stdout, 2 for stderr) and a log file.
/// Returns when the pipe closes (EOF). Best-effort: log-file write errors
/// are swallowed so a transient disk hiccup never crashes the dispatch.
pub fn tee_pipe<R: std::io::Read + Send + 'static>(
    mut pipe: R,
    log_path: &PathBuf,
) -> std::io::Result<()> {
    use std::io::Write;

    // Determine which terminal FD to mirror to based on the log suffix.
    // `.stdout.log` -> FD 1, `.stderr.log` -> FD 2. The caller is responsible
    // for the suffix via `capture_log_path`; we parse it back here.
    //
    // We can't stash a `Box<dyn Write + Send>` (Stdout/Stderr locks aren't
    // Send), so we route through a small `is_stderr` flag and acquire the
    // lock on every write.
    let is_stderr = log_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".stderr.log"))
        .unwrap_or(false);

    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut log_file = match std::fs::File::create(log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("⚠  Could not create log file {}: {e}", log_path.display());
            // Still tee to the terminal even if the file is unwritable.
            let mut buf = [0u8; 8192];
            loop {
                let n = pipe.read(&mut buf)?;
                if n == 0 {
                    return Ok(());
                }
                write_terminal(is_stderr, &buf[..n]);
            }
        }
    };

    let mut buf = [0u8; 8192];
    loop {
        let n = match pipe.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(_) => return Ok(()),
        };
        let _ = log_file.write_all(&buf[..n]);
        write_terminal(is_stderr, &buf[..n]);
    }
}

/// Write `chunk` to stdout (or stderr if `is_stderr`). Best-effort: errors
/// from the terminal FD are swallowed so a broken pipe to the user's
/// terminal doesn't kill the tee.
pub fn write_terminal(is_stderr: bool, chunk: &[u8]) {
    use std::io::Write;
    let mut lock: Box<dyn Write> = if is_stderr {
        Box::new(std::io::stderr().lock())
    } else {
        Box::new(std::io::stdout().lock())
    };
    let _ = lock.write_all(chunk);
}

// ─── Dispatch → Session capture ──────────────────────────────────────

/// Build a `SessionManifest` + message list from a completed dispatch and
/// persist it under `~/.loom/sessions/{task_id}.{phase}/`. Then index the
/// resulting session into cross-session memory and the knowledge graph so
/// the next phase (or a future session) can see what the agent produced.
///
/// Returns the `session_id` (which doubles as the lookup key in
/// `LOOM_PRIOR_SESSIONS` on subsequent dispatches).
pub fn capture_dispatch_session(
    loom_dir: &str,
    task: &loom_core::Task,
    agent: &str,
    phase_name: &str,
    prompt: &str,
    exit_code: i32,
) -> anyhow::Result<String> {
    use loom_sessions::memory::CrossSessionMemory;
    use loom_sessions::SessionManager;

    let session_id = format!("{}.{}", task.task_id, phase_name);
    let stdout_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stdout");
    let stderr_log = capture_log_path(loom_dir, &task.task_id, phase_name, "stderr");

    // Read the captured agent output. The stdout log is the primary
    // source; stderr is appended as a system note (or a separate Tool
    // message) so debugging context isn't lost.
    let stdout_text = read_log_tail(&stdout_log, 64 * 1024);
    let stderr_text = read_log_tail(&stderr_log, 16 * 1024);

    let now = loom_core::rfc3339_now();
    let provider_model = task
        .routing_decision
        .as_ref()
        .map(|rd| format!("{}/{}", rd.recommended_provider, rd.recommended_model))
        .unwrap_or_else(|| agent.to_string());

    let mut messages = Vec::new();

    // 1. System context: the task itself, plus the LOOM_TASK_* env vars
    //    the agent saw.
    let mut sys_content = format!(
        "Workflow task dispatch\n  task_id: {}\n  task_title: {}\n  task_description: {}\n  phase: {}\n  agent: {}\n  exit_code: {}",
        task.task_id, task.title, task.description, phase_name, agent, exit_code,
    );
    if let Some(rd) = &task.routing_decision {
        sys_content.push_str(&format!(
            "\n  routing: provider={} model={} tier={:?}",
            rd.recommended_provider, rd.recommended_model, rd.recommended_tier,
        ));
    }
    if let Some(last) = task.phase_results.last() {
        sys_content.push_str(&format!(
            "\n  prior_phase_result: phase={} agent={} summary_len={}",
            last.phase,
            last.agent,
            last.summary.len(),
        ));
    }
    messages.push(loom_core::Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: loom_core::MessageRole::System,
        content: sys_content,
        tool_calls: None,
        timestamp: now.clone(),
        metadata: Default::default(),
    });

    // 2. User prompt: the prompt that was passed to the agent.
    messages.push(loom_core::Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: loom_core::MessageRole::User,
        content: prompt.to_string(),
        tool_calls: None,
        timestamp: now.clone(),
        metadata: Default::default(),
    });

    // 3. Assistant response: the captured agent stdout (possibly empty).
    //    If there is also stderr with diagnostic content, fold it in as a
    //    trailing note so the knowledge extractor sees it.
    let mut assistant_content = if stdout_text.is_empty() {
        "(agent produced no stdout)".to_string()
    } else {
        stdout_text
    };
    if !stderr_text.is_empty()
        && !stderr_text.trim_start().starts_with("⚠")
        && !stderr_text.contains("loomd (shadowed by")
    {
        // Only fold in stderr if it looks substantive (not our own warning
        // banner from `find_binary` / `tee_pipe`).
        assistant_content.push_str("\n\n--- stderr ---\n");
        assistant_content.push_str(&stderr_text);
    }
    messages.push(loom_core::Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: loom_core::MessageRole::Assistant,
        content: assistant_content,
        tool_calls: None,
        timestamp: now.clone(),
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("agent".into(), serde_json::Value::String(agent.to_string()));
            m.insert(
                "phase".into(),
                serde_json::Value::String(phase_name.to_string()),
            );
            m.insert("exit_code".into(), serde_json::Value::from(exit_code));
            m
        },
    });

    // Derive a small set of tags from the dispatch context. These are
    // conservative defaults that make the captured session findable via
    // `loom sessions list --tag workflow` and `--tag agent:<name>`
    // without requiring the user to manually tag every dispatch.
    let dispatch_tags: Vec<String> = {
        let mut t = vec![
            "workflow".to_string(),
            format!("agent:{}", agent),
            format!("phase:{}", phase_name),
            format!("task:{}", task.task_id),
        ];
        if let Some(rd) = &task.routing_decision {
            t.push(format!("provider:{}", rd.recommended_provider));
            t.push(format!("model:{}", rd.recommended_model));
        }
        t
    };

    let manifest = loom_core::SessionManifest {
        session_id: session_id.clone(),
        source_tool: format!("loom-workflow/{}", agent),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now.clone(),
        topic: task.title.clone(),
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default(),
        model_provider: provider_model,
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Captured from workflow phase '{}' (agent={}, exit={})",
            phase_name, agent, exit_code,
        )),
        tags: dispatch_tags,
        summarized_at: None,
        summarized_message_count: None,
    };

    // Persist via the existing SessionStore. This makes the session show
    // up in `loom sessions list` / `loom sessions show` without any
    // additional wiring.
    let manager = SessionManager::new(loom_dir);
    let store = manager.store();
    store.save_manifest(&manifest)?;
    store.replace_messages(&session_id, &messages)?;

    // Index into cross-session memory and the knowledge graph. Failures
    // here are non-fatal: a corrupt memory DB shouldn't break dispatch.
    if let Ok(memory) = CrossSessionMemory::new(loom_dir) {
        if let Err(e) = memory.index_session(&session_id, &messages, &task.title) {
            eprintln!("⚠  Cross-session memory index failed: {e}");
        }
    }
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(loom_dir) {
        match kg.extract_from_messages(&messages, &session_id) {
            Ok((entities, rels)) if entities > 0 || rels > 0 => {
                println!(
                    "🧠 Extracted {} entities / {} relationships from phase '{}'",
                    entities, rels, phase_name
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("⚠  Knowledge extraction failed: {e}"),
        }
    }

    Ok(session_id)
}

/// Return up to `max_bytes` from the end of a log file as a UTF-8 string.
/// Returns an empty string if the file doesn't exist or can't be read.
pub fn read_log_tail(path: &std::path::Path, max_bytes: usize) -> String {
    let Ok(raw) = std::fs::read(path) else {
        return String::new();
    };
    let start = raw.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&raw[start..]).to_string()
}

/// Find sessions previously captured for prior phases of the same task.
/// Returns session_ids in the order they should be presented as context
/// (oldest phase first, so the agent reads them as a chronological log).
pub fn prior_session_ids_for_task(loom_dir: &str, task_id: &str) -> Vec<String> {
    let manager = loom_sessions::SessionManager::new(loom_dir);
    let store = manager.store();
    let prefix = format!("{task_id}.");
    let mut ids: Vec<String> = store
        .list()
        .into_iter()
        .map(|m| m.session_id)
        .filter(|id| id.starts_with(&prefix))
        .collect();
    // Stable, predictable order: chronological by phase, not alphabetical.
    let order = |id: &str| -> u8 {
        let phase = id.strip_prefix(&prefix).unwrap_or("");
        match phase {
            "Plan" => 0,
            "Implement" => 1,
            "Verify" => 2,
            "Finish" => 3,
            _ => 99,
        }
    };
    ids.sort_by_key(|id| order(id));
    ids
}

/// Write a JSON context file with the assistant message bodies of every
/// session in `prior_sessions`, truncated to keep the dispatch prompt
/// small. Returns the path on success, `None` on any failure (which the
/// caller treats as "skip the env var, the agent will look up sessions
/// via the comma-separated `LOOM_PRIOR_SESSIONS` list instead").
pub fn build_prior_sessions_context(
    loom_dir: &str,
    task_id: &str,
    prior_sessions: &[String],
) -> Option<std::path::PathBuf> {
    let manager = loom_sessions::SessionManager::new(loom_dir);
    let store = manager.store();
    let mut entries: Vec<serde_json::Value> = Vec::new();
    for sid in prior_sessions {
        let messages = match store.load_messages(sid) {
            Ok(m) => m,
            Err(_) => continue,
        };
        // Capture only the assistant (agent output) messages, truncated.
        let mut bodies: Vec<serde_json::Value> = Vec::new();
        for m in &messages {
            if matches!(m.role, loom_core::MessageRole::Assistant) {
                let truncated: String = m.content.chars().take(4096).collect();
                let ellipsis = if m.content.chars().count() > 4096 {
                    "…"
                } else {
                    ""
                };
                bodies.push(serde_json::json!({
                    "phase": m.metadata.get("phase").cloned().unwrap_or(serde_json::Value::Null),
                    "agent":  m.metadata.get("agent").cloned().unwrap_or(serde_json::Value::Null),
                    "content": format!("{truncated}{ellipsis}"),
                }));
            }
        }
        entries.push(serde_json::json!({
            "session_id": sid,
            "messages": bodies,
        }));
    }

    let ctx_dir = std::path::Path::new(loom_dir)
        .join("workflow")
        .join("tasks");
    let ctx_path = ctx_dir.join(format!("{task_id}.prior_sessions.json"));
    if let Err(e) = std::fs::create_dir_all(&ctx_dir) {
        eprintln!("⚠  Could not create {}: {e}", ctx_dir.display());
        return None;
    }
    let payload = serde_json::json!({
        "task_id": task_id,
        "prior_sessions": entries,
    });
    let payload = match serde_json::to_string_pretty(&payload) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("⚠  Could not serialize prior-sessions context: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::write(&ctx_path, payload) {
        eprintln!("⚠  Could not write prior-sessions context: {e}");
        return None;
    }
    Some(ctx_path)
}
