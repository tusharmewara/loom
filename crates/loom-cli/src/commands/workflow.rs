use crate::cli::{TemplateAction, WorkflowAction};
use loom_core::WorkflowTemplate;
use loom_trackers::IssueTracker;
use loom_workflow::WorkflowEngine;
use std::collections::HashMap;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &WorkflowAction) -> anyhow::Result<()> {
    let loom_dir_str = loom_dir.to_str().unwrap();
    let engine = WorkflowEngine::new(loom_dir_str);
    let orchestrator = engine.orchestrator();

    match action {
        WorkflowAction::List { json } => {
            let tasks = orchestrator.list_tasks()?;
            if *json {
                let task_summaries: Vec<_> = tasks.iter().map(|t| {
                    serde_json::json!({
                        "task_id": t.task_id,
                        "title": t.title,
                        "phase": format!("{:?}", t.phase),
                        "status": format!("{:?}", t.status),
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "workflow.list",
                    "count": task_summaries.len(),
                    "tasks": task_summaries
                }))?);
            } else {
                if tasks.is_empty() {
                    println!("No workflow tasks.");
                } else {
                    for task in tasks {
                        let sub_info = task.sub_agents.as_ref().map(|sa| {
                            let parts: Vec<&str> = [&sa.plan, &sa.implement, &sa.verify, &sa.finish]
                                .iter()
                                .filter_map(|o| o.as_deref())
                                .collect();
                            if parts.is_empty() { String::new() } else { format!(" [sub: {}]", parts.join("|")) }
                        }).unwrap_or_default();
                        println!(
                            "{}  phase={:?}  status={:?}  title={}{}{}",
                            task.task_id,
                            task.phase,
                            task.status,
                            task.title,
                            task.linked_issue
                                .as_ref()
                                .map(|i| format!("  🔗 {i}"))
                                .unwrap_or_default(),
                            sub_info,
                        );
                    }
                }
            }
        }
        WorkflowAction::Show { task_id, json } => {
            let task = orchestrator
                .get_task(task_id)?
                .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "workflow.show",
                    "task": task
                }))?);
            } else {
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
        }
        WorkflowAction::Create {
            title,
            description,
            from_issue,
            repo,
            token,
            template,
        } => {
            // If --template is given, create from template
            if let Some(template_name) = template {
                let tmgr = loom_workflow::TemplateManager::new(loom_dir_str);
                let tpl = tmgr
                    .get(template_name)
                    .ok_or_else(|| anyhow::anyhow!("Template '{template_name}' not found"))?;
                let t_title = tpl.default_title.as_deref().unwrap_or("Untitled task");
                let t_description = tpl.default_description.as_deref().unwrap_or("Created from template");
                let task = orchestrator.create_task(t_title, t_description)?;
                // Apply template overrides
                if let Some(agent) = &tpl.default_agent {
                    orchestrator.set_last_agent(&task.task_id, agent)?;
                }
                if let Some(ref sa) = tpl.sub_agents {
                    orchestrator.set_sub_agents(&task.task_id, sa.clone())?;
                }
                if let Some(provider) = &tpl.default_provider {
                    if let Some(model) = &tpl.default_model {
                        orchestrator.assign_routing_with_overrides(
                            &task.task_id,
                            Some(provider),
                            Some(model),
                        )?;
                    }
                }
                let task = orchestrator.get_task(&task.task_id)?.unwrap();
                println!("Created task {} from template '{template_name}'", task.task_id);
                println!("{}", serde_json::to_string_pretty(&task)?);
            } else if let Some(issue_ref) = from_issue {
                let tracker = loom_trackers::github::GitHubTracker::new(token.clone())?;
                let issue_id = if issue_ref.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
                    format!("{}#{}", repo, issue_ref)
                } else {
                    issue_ref.to_string()
                };
                let issue = tracker.get_issue(&issue_id).await?;
                let t_title = issue.title.clone();
                let t_description = issue.body.clone().unwrap_or_default();
                let task = orchestrator.create_task(&t_title, &t_description)?;
                orchestrator.set_linked_issue(&task.task_id, &issue_id)?;
                let task = orchestrator.get_task(&task.task_id)?.unwrap();
                println!(
                    "Created task {} from issue {} 🔗",
                    task.task_id, issue_id
                );
                println!("{}", serde_json::to_string_pretty(&task)?);
            } else {
                let t_title = title
                    .clone()
                    .unwrap_or_else(|| "Untitled task".to_string());
                let t_description = description
                    .clone()
                    .unwrap_or_else(|| "New workflow task".to_string());
                let task = orchestrator.create_task(&t_title, &t_description)?;
                println!("Created task {}", task.task_id);
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
        }
        WorkflowAction::Advance {
            task_id,
            update_issue,
            comment,
        } => {
            let task = orchestrator.advance_phase(task_id)?;
            println!("Advanced task {} to phase {:?}", task.task_id, task.phase);
            // Post comment to linked issue if requested
            if *update_issue {
                if let Some(ref linked) = task.linked_issue {
                    let tracker = loom_trackers::github::GitHubTracker::new(None)?;
                    let msg = comment.clone().unwrap_or_else(|| {
                        format!(
                            "🔄 Loom task advanced to **{:?}** (task: `{}`)",
                            task.phase,
                            task.title
                        )
                    });
                    // Use the GitHub API to comment on the issue
                    tracker.add_comment(linked, &msg).await?;
                    println!("  💬 Posted comment on {linked}");
                } else {
                    eprintln!("  ⚠ Task has no linked issue (use --link-issue on dispatch)");
                }
            }
            println!("{}", serde_json::to_string_pretty(&task)?);
        }
        WorkflowAction::Assign {
            task_id,
            provider,
            model,
            agent,
        } => {
            let task = orchestrator.assign_routing_with_overrides(
                task_id,
                provider.as_deref(),
                model.as_deref(),
            )?;
            if let Some(a) = agent {
                let task = orchestrator.set_last_agent(task_id, a)?;
                println!("Set agent={a} for task {}", task.task_id);
            }
            println!("Assigned routing to task {}", task.task_id);
            if let Some(rd) = &task.routing_decision {
                println!(
                    "  provider={}  model={}  tier={:?}",
                    rd.recommended_provider, rd.recommended_model, rd.recommended_tier
                );
            }
            if let Some(rp) = &task.routed_provider {
                println!("  routed_provider={rp}");
            }
            if let Some(la) = &task.last_agent {
                println!("  last_agent={la}");
            }
        }
        WorkflowAction::Dispatch {
            task_id,
            agent,
            link_issue,
            create_branch,
            mode,
            tool_args,
            dry_run,
            auto_advance,
        } => {
            // Link issue if provided
            if let Some(issue_ref) = link_issue {
                orchestrator.set_linked_issue(task_id, issue_ref)?;
                if let Some(t) = orchestrator.get_task(task_id)? {
                    println!("🔗 Linked issue: {:?}", t.linked_issue);
                }
            }

            let task = orchestrator
                .get_task(task_id)?
                .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))?;

            // Create git branch from linked issue if requested
            if *create_branch {
                if let Some(ref linked) = task.linked_issue {
                    let tracker = loom_trackers::github::GitHubTracker::new(None)?;
                    let issue = tracker.get_issue(linked).await?;
                    let branch_name = tracker
                        .suggest_branch_name(&issue)
                        .ok_or_else(|| anyhow::anyhow!("Could not generate branch name"))?;
                    let output = std::process::Command::new("git")
                        .args(["checkout", "-b", &branch_name])
                        .output()
                        .map_err(|e| anyhow::anyhow!("Failed to run git: {e}"))?;
                    if output.status.success() {
                        println!("🌿 Created branch: {branch_name}");
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        // Branch may already exist, that's ok
                        if !stderr.contains("already exists") {
                            eprintln!("  ⚠ Failed to create branch: {}", stderr.trim());
                        } else {
                            println!("  (branch already exists)");
                        }
                    }
                } else {
                    eprintln!("  ⚠ No linked issue to create branch from (use --link-issue)");
                }
            }

            // Resolve agent
            let resolved_agent = match agent {
                Some(a) => a.clone(),
                None => {
                    if mode == "sub-agents" {
                        orchestrator.resolve_agent(task_id)?
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No sub-agent configured for phase {:?} and no fallback. \
                                     Use `loom workflow set-sub-agents {task_id} --plan <tool> ...` \
                                     or `--agent <tool>`",
                                    task.phase
                                )
                            })?
                    } else {
                        task.last_agent
                            .as_ref()
                            .cloned()
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "No agent specified and task has no last_agent. \
                                     Use `loom workflow dispatch {task_id} --agent <tool>`, \
                                     or set one with `loom workflow assign {task_id} --agent <tool>`"
                                )
                            })?
                    }
                }
            };

            let phase_label = if mode == "sub-agents" {
                format!(" (phase: {:?})", task.phase)
            } else {
                String::new()
            };
            println!("🚀 Dispatching '{resolved_agent}' for task {task_id}{phase_label}...");

            let mut current_task = task.clone();
            let mut current_agent = resolved_agent.clone();
            let mut current_tool_args: &[String] = tool_args;

            // Auto-advance loop: dispatch → capture result → advance → re-dispatch
            // until the task is in Finish phase (or an agent exits non-zero,
            // or the user only requested a single dispatch).
            loop {
                launch_agent_with_task_context(
                    loom_dir_str,
                    &current_task,
                    &current_agent,
                    current_tool_args,
                    *dry_run,
                )
                .await?;
                if *dry_run {
                    break;
                }
                orchestrator.set_last_agent(task_id, &current_agent)?;

                let exit_code = last_exit_code();
                let phase_name = current_task.phase.name().to_string();

                // Fire PostPhase hooks (best-effort, never blocks the chain).
                fire_post_phase_hooks(loom_dir_str, task_id, &phase_name, &current_agent, exit_code)
                    .await;

                // Capture any phase-result file the agent wrote.
                if let Some(result) =
                    capture_phase_result(loom_dir_str, task_id, &phase_name, &current_agent)?
                {
                    orchestrator.set_phase_result(task_id, result.clone())?;
                    println!(
                        "📝 Captured result for phase '{}' ({} bytes summary)",
                        phase_name,
                        result.summary.len()
                    );
                } else {
                    println!(
                        "ℹ  No phase result file found for phase '{}' (agent did not write one)",
                        phase_name
                    );
                }

                if !*auto_advance || mode != "sub-agents" {
                    // Single-shot dispatch: preserve old fail-fast behavior.
                    if exit_code != 0 {
                        anyhow::bail!(
                            "Agent tool '{current_agent}' exited with status code {exit_code}"
                        );
                    }
                    break;
                }

                // Auto-advance: stop the chain if the agent failed, unless
                // the agent still wrote a result (operator may want to keep
                // going with a partial result). Default to stopping on
                // non-zero exit.
                if exit_code != 0 {
                    eprintln!(
                        "🛑 Agent '{current_agent}' exited with {exit_code}; stopping auto-advance chain"
                    );
                    break;
                }

                // Advance to the next phase. If we're already in Finish, stop.
                use loom_core::TaskPhase;
                if matches!(current_task.phase, TaskPhase::Finish) {
                    println!("✅ Task {task_id} reached Finish phase — chain complete");
                    break;
                }
                let advanced = orchestrator.advance_phase(task_id)?;
                println!("➡️  Advanced task to phase {:?}", advanced.phase);

                // Re-resolve the agent for the new phase (sub-agents mode).
                let next_agent = orchestrator
                    .resolve_agent(task_id)?
                    .unwrap_or_else(|| current_agent.clone());
                println!("🚀 Dispatching '{next_agent}' for task {task_id} (phase: {:?})...", advanced.phase);

                current_task = advanced;
                current_agent = next_agent;
                // Subsequent phases get the default prompt (no extra args).
                current_tool_args = &[];
            }
        }
        WorkflowAction::SetSubAgents {
            task_id,
            plan,
            implement,
            verify,
            finish,
        } => {
            let config = loom_core::SubAgentConfig {
                plan: plan.clone(),
                implement: implement.clone(),
                verify: verify.clone(),
                finish: finish.clone(),
            };
            let task = orchestrator.set_sub_agents(task_id, config)?;
            println!("✓ Sub-agent configuration set for task {task_id}:");
            if let Some(ref sa) = task.sub_agents {
                if let Some(ref a) = sa.plan { println!("  Plan:       {a}"); }
                if let Some(ref a) = sa.implement { println!("  Implement:  {a}"); }
                if let Some(ref a) = sa.verify { println!("  Verify:     {a}"); }
                if let Some(ref a) = sa.finish { println!("  Finish:     {a}"); }
            }
        }
        WorkflowAction::Template { action } => {
            let tmgr = loom_workflow::TemplateManager::new(loom_dir_str);
            match action {
                TemplateAction::List => {
                    let templates = tmgr.list();
                    if templates.is_empty() {
                        println!("No workflow templates.");
                        println!();
                        println!("  Create one with: loom workflow template create <name> --description ...");
                    } else {
                        println!("Workflow templates:");
                        for tpl in &templates {
                            let tpl_title = tpl.default_title.as_deref().unwrap_or("(no default title)");
                            println!("  {}  {}  default_agent={}", tpl.name, tpl_title, tpl.default_agent.as_deref().unwrap_or("-"));
                        }
                    }
                }
                TemplateAction::Show { name } => {
                    match tmgr.get(name) {
                        Some(tpl) => {
                            println!("Template: {}", tpl.name);
                            println!("  Description: {}", tpl.description);
                            if let Some(ref t) = tpl.default_title {
                                println!("  Default title: {t}");
                            }
                            if let Some(ref d) = tpl.default_description {
                                println!("  Default description: {d}");
                            }
                            if let Some(ref a) = tpl.default_agent {
                                println!("  Default agent: {a}");
                            }
                            if let Some(ref p) = tpl.default_provider {
                                println!("  Default provider: {p}");
                            }
                            if let Some(ref m) = tpl.default_model {
                                println!("  Default model: {m}");
                            }
                            if let Some(ref sa) = tpl.sub_agents {
                                println!("  Sub-agents:");
                                if let Some(ref a) = sa.plan { println!("    Plan:       {a}"); }
                                if let Some(ref a) = sa.implement { println!("    Implement:  {a}"); }
                                if let Some(ref a) = sa.verify { println!("    Verify:     {a}"); }
                                if let Some(ref a) = sa.finish { println!("    Finish:     {a}"); }
                            }
                        }
                        None => {
                            eprintln!("Template '{name}' not found.");
                        }
                    }
                }
                TemplateAction::Create {
                    name,
                    description,
                    default_title,
                    default_description,
                    default_agent,
                    default_provider,
                    default_model,
                    sub_plan,
                    sub_implement,
                    sub_verify,
                    sub_finish,
                } => {
                    let sub_agents = if sub_plan.is_some()
                        || sub_implement.is_some()
                        || sub_verify.is_some()
                        || sub_finish.is_some()
                    {
                        Some(loom_core::SubAgentConfig {
                            plan: sub_plan.clone(),
                            implement: sub_implement.clone(),
                            verify: sub_verify.clone(),
                            finish: sub_finish.clone(),
                        })
                    } else {
                        None
                    };
                    let template = WorkflowTemplate {
                        name: name.clone(),
                        description: description.clone(),
                        default_title: default_title.clone(),
                        default_description: default_description.clone(),
                        default_agent: default_agent.clone(),
                        sub_agents,
                        default_provider: default_provider.clone(),
                        default_model: default_model.clone(),
                    };
                    tmgr.save(&template)?;
                    println!("✓ Template '{name}' saved.");
                }
                TemplateAction::Remove { name } => {
                    if tmgr.delete(name)? {
                        println!("✓ Template '{name}' removed.");
                    } else {
                        eprintln!("Template '{name}' not found.");
                    }
                }
            }
        }
    }
    Ok(())
}

async fn launch_agent_with_task_context(
    loom_dir: &str,
    task: &loom_core::Task,
    agent: &str,
    tool_args: &[String],
    dry_run: bool,
) -> anyhow::Result<i32> {
    let home = std::env::var("HOME").expect("HOME not set");

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
        env_vars.insert("LOOM_ROUTING_MODEL".to_string(), rd.recommended_model.clone());
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
                env_vars.insert("LOOM_SESSIONS_DIR".to_string(), format!("{loom_dir}/sessions"));
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
    let binary_path = find_binary(agent)?;

    // Compute the canonical phase-result path so the agent knows where to
    // write a result file (consumed by the next dispatch in --auto-advance).
    let phase_name = task.phase.name();
    let result_path = phase_result_path(loom_dir, &task.task_id, phase_name);
    env_vars.insert("LOOM_EXPECTED_RESULT_PATH".to_string(), result_path.display().to_string());

    // Inject the most recent phase result (if any) so the next phase's agent
    // can see what the prior agent produced.
    if let Some(last) = task.phase_results.last() {
        env_vars.insert(
            "LOOM_PRIOR_PHASE_RESULT".to_string(),
            serde_json::to_string(last).unwrap_or_default(),
        );
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
            prompt.push_str("…");
        }
        vec![prompt]
    } else {
        tool_args.to_vec()
    };

    if dry_run {
        println!("\n=== Dry Run: dispatch {} with {} ===", task.task_id, agent);
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
    let mut cmd = std::process::Command::new(&binary_path);
    cmd.args(&final_args);
    cmd.envs(&env_vars);
    cmd.stdin(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status()?;
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

fn find_binary(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let path = std::path::Path::new(name);
    if path.is_absolute() && path.exists() {
        return Ok(path.to_path_buf());
    }
    let path_env = std::env::var("PATH").map_err(|_| anyhow::anyhow!("PATH not set"))?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if candidate.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&candidate) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Ok(candidate);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("Binary '{name}' not found in PATH")
}

// ─── Phase-result capture and PostPhase hooks ──────────────────────────

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    /// Exit code from the most recent agent dispatch. Read by the
    /// auto-advance loop and PostPhase hooks.
    static LAST_EXIT_CODE: RefCell<i32> = const { RefCell::new(0) };
}

fn last_exit_code() -> i32 {
    LAST_EXIT_CODE.with(|cell| *cell.borrow())
}

/// Canonical path where the agent for a given phase should write its result.
/// `~/.loom/workflow/tasks/{task_id}.{phase}.result.json` (or `.md`).
fn phase_result_path(loom_dir: &str, task_id: &str, phase_name: &str) -> PathBuf {
    PathBuf::from(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.{phase_name}.result.json"))
}

/// Read the phase result file the agent wrote. Supports either a JSON
/// `PhaseResult` (preferred) or a plain Markdown summary (wrapped into a
/// `PhaseResult` with no artifacts). Returns `Ok(None)` if no file was found.
fn capture_phase_result(
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
                completed_at: rfc3339_now_local(),
            }));
        }
        // Last resort: use the raw JSON as the summary.
        return Ok(Some(loom_core::PhaseResult {
            phase: phase_name.to_string(),
            agent: agent.to_string(),
            summary: raw,
            artifacts: vec![],
            completed_at: rfc3339_now_local(),
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
            completed_at: rfc3339_now_local(),
        }));
    }

    Ok(None)
}

/// Fire all `PostPhase` hooks. Errors are logged but never propagated —
/// the chain should keep going even if a hook script fails.
async fn fire_post_phase_hooks(
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
    match loom_hooks::run_post_phase_hooks(&registry, task_id, phase_name, agent, exit_code).await
    {
        Ok(n) if n > 0 => println!("🔗 Fired {n} PostPhase hook(s)"),
        Ok(_) => {} // no hooks registered — silent
        Err(e) => eprintln!("⚠  PostPhase hook error: {e}"),
    }
}

fn rfc3339_now_local() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Delegate to the same RFC3339 helper used by the workflow crate.
    // We replicate it here to avoid a cross-crate dependency.
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}
