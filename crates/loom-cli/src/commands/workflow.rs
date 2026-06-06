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
            no_capture_session,
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

            // Track sessions captured for prior phases of this task so we
            // can pass them to the next phase as LOOM_PRIOR_SESSIONS.
            let mut prior_sessions: Vec<String> = Vec::new();
            if !*no_capture_session {
                prior_sessions = prior_session_ids_for_task(loom_dir_str, task_id);
                if !prior_sessions.is_empty() {
                    println!(
                        "📚 Found {} prior session(s) for task {}: {}",
                        prior_sessions.len(),
                        task_id,
                        prior_sessions.join(", ")
                    );
                }
            }

            // Auto-advance loop: dispatch → capture result → advance → re-dispatch
            // until the task is in Finish phase (or an agent exits non-zero,
            // or the user only requested a single dispatch).
            loop {
                // Build the prompt that will actually be passed to the agent,
                // so the session-capture step can include it in the user
                // message. This mirrors the logic in
                // `launch_agent_with_task_context` exactly.
                let prompt_for_capture: String = if current_tool_args.is_empty() {
                    if current_task.description.is_empty() {
                        current_task.title.clone()
                    } else {
                        format!("{} — {}", current_task.title, current_task.description)
                    }
                } else {
                    current_tool_args.join(" ")
                };

                launch_agent_with_task_context(
                    loom_dir_str,
                    &current_task,
                    &current_agent,
                    current_tool_args,
                    *dry_run,
                    &prior_sessions,
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

                // Capture this dispatch as a session, unless the user opted
                // out. This is what makes the dispatch loop *visible* to
                // cross-session memory, the knowledge graph, and (via
                // LOOM_PRIOR_SESSIONS) the next phase.
                if !*no_capture_session {
                    match capture_dispatch_session(
                        loom_dir_str,
                        &current_task,
                        &current_agent,
                        &phase_name,
                        &prompt_for_capture,
                        exit_code,
                    ) {
                        Ok(sid) => {
                            println!("💾 Captured session {sid} for phase '{phase_name}'");
                            prior_sessions.push(sid);
                        }
                        Err(e) => eprintln!(
                            "⚠  Could not capture session for phase '{phase_name}': {e}"
                        ),
                    }
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
    prior_sessions: &[String],
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

    // Inject prior sessions captured for prior phases of this task. The
    // env var is a comma-separated list of session IDs; the agent can
    // `loom sessions show <id>` to see them. For agents that prefer a
    // single file, we also write a small context file with the prior
    // assistant message bodies (truncated) so they don't have to
    // round-trip through the database.
    if !prior_sessions.is_empty() {
        env_vars.insert(
            "LOOM_PRIOR_SESSIONS".to_string(),
            prior_sessions.join(","),
        );
        if let Some(ctx_path) = build_prior_sessions_context(
            loom_dir,
            &task.task_id,
            prior_sessions,
        ) {
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

// ─── Dispatch output capture (session tee) ───────────────────────────

/// Canonical path of the per-dispatch log file. Used as both the tee
/// destination and the source for `capture_dispatch_session` to build a
/// `SessionManifest` from.
///
/// Layout: `~/.loom/workflow/tasks/{task_id}.{phase}.{stream}.log`
fn capture_log_path(loom_dir: &str, task_id: &str, phase_name: &str, stream: &str) -> PathBuf {
    PathBuf::from(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.{phase_name}.{stream}.log"))
}

/// Read a child process's pipe in 8KB chunks, writing each chunk to both
/// the originating terminal FD (1 for stdout, 2 for stderr) and a log file.
/// Returns when the pipe closes (EOF). Best-effort: log-file write errors
/// are swallowed so a transient disk hiccup never crashes the dispatch.
fn tee_pipe<R: std::io::Read + Send + 'static>(
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
fn write_terminal(is_stderr: bool, chunk: &[u8]) {
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
fn capture_dispatch_session(
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

    let now = rfc3339_now_local();
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
            last.phase, last.agent, last.summary.len(),
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
            m.insert("phase".into(), serde_json::Value::String(phase_name.to_string()));
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
fn read_log_tail(path: &std::path::Path, max_bytes: usize) -> String {
    let Ok(raw) = std::fs::read(path) else {
        return String::new();
    };
    let start = raw.len().saturating_sub(max_bytes);
    String::from_utf8_lossy(&raw[start..]).to_string()
}

/// Find sessions previously captured for prior phases of the same task.
/// Returns session_ids in the order they should be presented as context
/// (oldest phase first, so the agent reads them as a chronological log).
fn prior_session_ids_for_task(loom_dir: &str, task_id: &str) -> Vec<String> {
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
fn build_prior_sessions_context(
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
