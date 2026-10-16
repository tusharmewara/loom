use crate::cli::{TemplateAction, WorkflowAction};
use loom_core::WorkflowTemplate;
use loom_trackers::IssueTracker;
use loom_workflow::WorkflowEngine;
use loom_workflow::dispatch::{
    capture_dispatch_session, capture_phase_result, fire_post_phase_hooks,
    launch_agent_with_task_context, last_exit_code, prior_session_ids_for_task,
};
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
            from_ticket,
            repo,
            token,
            template,
            from_plan,
            worktree,
            no_worktree,
        } => {
        run_create(loom_dir_str, orchestrator, title, description, from_issue, from_ticket, repo, token, template, from_plan, worktree, no_worktree).await?;
        }
        WorkflowAction::Remove { task_id } => {
            orchestrator.remove_task(task_id)?;
            println!("Removed task {task_id}");
        }
        WorkflowAction::DraftPlan {
            title,
            description,
            create,
            json,
        } => {
            let goal = title.clone().unwrap_or_else(|| "Untitled goal".to_string());
            let details = description.clone().unwrap_or_default();
            let plan = loom_workflow::chief::draft_plan(&goal, &details)?;
            if *create {
                let tasks =
                    loom_workflow::chief::import_plan(orchestrator.manager(), &plan)?;
                if *json {
                    let ids: Vec<&str> =
                        tasks.iter().map(|t| t.task_id.as_str()).collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "created": ids,
                            "count": ids.len(),
                        }))?
                    );
                } else {
                    for task in &tasks {
                        println!("Created task {} [draft]", task.task_id);
                    }
                    println!(
                        "Imported {} drafted task(s) for goal '{}'",
                        tasks.len(),
                        plan.goal
                    );
                }
                return Ok(());
            }
            if *json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
                return Ok(());
            }
            println!("Draft plan for '{}':", plan.goal);
            for task in &plan.tasks {
                println!("  {}  {}", task.id, task.title);
            }
            println!(
                "{} task(s). Re-run with --create to import.",
                plan.tasks.len()
            );
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
            runtime,
            tool_args,
            dry_run,
            auto_advance,
            no_capture_session,
        } => {
        run_dispatch(loom_dir_str, orchestrator, task_id, agent, link_issue, create_branch, mode, runtime, tool_args, dry_run, auto_advance, no_capture_session).await?;
        }
        WorkflowAction::SetSubAgents {
            task_id,
            plan,
            implement,
            verify,
            finish,
            max_retries,
            fallback_agent,
        } => {
            let config = loom_core::SubAgentConfig {
                plan: plan.clone(),
                implement: implement.clone(),
                verify: verify.clone(),
                finish: finish.clone(),
                max_retries: *max_retries,
                fallback_agent: fallback_agent.clone(),
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
        run_template(loom_dir_str, action).await?;
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn run_create(
    loom_dir_str: &str,
    orchestrator: &loom_workflow::TaskOrchestrator,
    title: &Option<String>,
    description: &Option<String>,
    from_issue: &Option<String>,
    from_ticket: &Option<String>,
    repo: &str,
    token: &Option<String>,
    template: &Option<String>,
    from_plan: &Option<String>,
    worktree: &bool,
    no_worktree: &bool,
) -> anyhow::Result<()> {
            let use_worktree = *worktree && !*no_worktree;
            // If --from-ticket is given, fetch from any supported tracker.
            if let Some(ticket_ref) = from_ticket {
                let parsed = parse_ticket_ref(ticket_ref).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unrecognized ticket reference '{ticket_ref}'. \
                         Use github:owner/repo#123, linear:TEAM-1, \
                         gitlab:owner/repo#1, or jira:PROJ-1"
                    )
                })?;
                let (t_title, t_description, linked) =
                    fetch_ticket(&parsed, repo, token).await?;
                let task = orchestrator.create_task(&t_title, &t_description, use_worktree, None)?;
                orchestrator.set_linked_issue(&task.task_id, &linked)?;
                let task = orchestrator.get_task(&task.task_id)?.unwrap();
                println!(
                    "Created task {} from ticket {} 🔗",
                    task.task_id, linked
                );
                println!("{}", serde_json::to_string_pretty(&task)?);
                return Ok(());
            }
            // If --from-plan is given, import a chief plan as tasks.
            if let Some(plan_path) = from_plan {
                let raw = std::fs::read_to_string(plan_path).map_err(|e| {
                    anyhow::anyhow!("Could not read plan file: {e}")
                })?;
                let plan = loom_workflow::chief::PlanParser::parse(&raw)?;
                let tasks = loom_workflow::chief::import_plan(
                    orchestrator.manager(),
                    &plan,
                )?;
                for task in &tasks {
                    let plan_tag = task
                        .tags
                        .iter()
                        .find(|t| t.starts_with("plan-task:"))
                        .cloned()
                        .unwrap_or_default();
                    println!("Created task {} [{plan_tag}]", task.task_id);
                }
                println!(
                    "Imported {} task(s) from plan '{}'",
                    tasks.len(),
                    plan.goal
                );
                return Ok(());
            }
            // If --template is given, create from template
            if let Some(template_name) = template {
                let tmgr = loom_workflow::TemplateManager::new(loom_dir_str);
                let tpl = tmgr
                    .get(template_name)
                    .ok_or_else(|| anyhow::anyhow!("Template '{template_name}' not found"))?;
                let t_title = tpl.default_title.as_deref().unwrap_or("Untitled task");
                let t_description = tpl.default_description.as_deref().unwrap_or("Created from template");
                let task = orchestrator.create_task(t_title, t_description, use_worktree, None)?;
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
                let task = orchestrator.create_task(&t_title, &t_description, use_worktree, None)?;
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
                let task = orchestrator.create_task(&t_title, &t_description, use_worktree, None)?;
                println!("Created task {}", task.task_id);
                println!("{}", serde_json::to_string_pretty(&task)?);
            }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_dispatch(
    loom_dir_str: &str,
    orchestrator: &loom_workflow::TaskOrchestrator,
    task_id: &str,
    agent: &Option<String>,
    link_issue: &Option<String>,
    create_branch: &bool,
    mode: &str,
    runtime: &Option<String>,
    tool_args: &[String],
    dry_run: &bool,
    auto_advance: &bool,
    no_capture_session: &bool,
) -> anyhow::Result<()> {
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
            // until the task is in Finish phase (or retries exhausted,
            // or the user only requested a single dispatch).
            let mut retry_count: HashMap<String, u8> = HashMap::new();
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
                    runtime.clone(),
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

                // Auto-advance: retry on failure up to max_retries, then
                // try fallback agent before giving up.
                if exit_code != 0 {
                    let config = task.sub_agents.as_ref();
                    let retries_used = retry_count.entry(current_agent.clone()).or_insert(0u8);
                    let max_retries = config.map(|c| c.max_retries).unwrap_or(3);

                    if *retries_used < max_retries {
                        *retries_used += 1;
                        eprintln!(
                            "🔄 Agent '{current_agent}' exited with {exit_code}; retry {}/{}",
                            *retries_used, max_retries
                        );
                        continue;
                    }

                    // Try fallback agent if configured
                    if let Some(fallback) = config.and_then(|c| c.fallback_agent.as_ref()) {
                        if &current_agent != fallback {
                            eprintln!(
                                "🔄 Retries exhausted for '{current_agent}'; trying fallback '{fallback}'"
                            );
                            current_agent = fallback.clone();
                            retry_count.insert(current_agent.clone(), 0);
                            continue;
                        }
                    }

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
    Ok(())
}

async fn run_template(
    loom_dir_str: &str,
    action: &TemplateAction,
) -> anyhow::Result<()> {
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
                            max_retries: 3,
                            fallback_agent: None,
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
    Ok(())
}


/// A ticket reference for `--from-ticket`: an explicit tracker prefix
/// (`github:`/`linear:`/`gitlab:`/`jira:`) or a bare `owner/repo#123`
/// GitHub form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketRef {
    GitHub(String),
    Linear(String),
    GitLab(String),
    Jira(String),
}

pub fn parse_ticket_ref(input: &str) -> Option<TicketRef> {
    let text = input.trim();
    if text.is_empty() {
        return None;
    }
    let prefixed = |prefix: &str| {
        text.strip_prefix(prefix)
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string)
    };
    if let Some(rest) = prefixed("github:") {
        return Some(TicketRef::GitHub(rest));
    }
    if let Some(rest) = prefixed("linear:") {
        return Some(TicketRef::Linear(rest));
    }
    if let Some(rest) = prefixed("gitlab:") {
        return Some(TicketRef::GitLab(rest));
    }
    if let Some(rest) = prefixed("jira:") {
        return Some(TicketRef::Jira(rest));
    }
    if text.contains('#') {
        return Some(TicketRef::GitHub(text.to_string()));
    }
    None
}

/// Fetch a ticket's title/body plus its canonical linked reference.
/// Tracker credentials come from `--token` or the trackers' env vars
/// (`GITHUB_TOKEN`, `LINEAR_API_KEY`, `GITLAB_TOKEN`, `JIRA_*`).
async fn fetch_ticket(
    ticket: &TicketRef,
    repo: &str,
    token: &Option<String>,
) -> anyhow::Result<(String, String, String)> {
    use loom_trackers::IssueTracker;
    match ticket {
        TicketRef::GitHub(id) => {
            let issue_id = if id.chars().all(|c| c.is_ascii_digit()) && !repo.is_empty() {
                format!("{repo}#{id}")
            } else {
                id.clone()
            };
            let tracker = loom_trackers::github::GitHubTracker::new(token.clone())?;
            let issue = tracker.get_issue(&issue_id).await?;
            Ok((
                issue.title.clone(),
                issue.body.clone().unwrap_or_default(),
                issue_id,
            ))
        }
        TicketRef::Linear(id) => {
            let tracker = loom_trackers::linear::LinearTracker::new(token.clone())?;
            let issue = tracker.get_issue(id).await?;
            Ok((
                issue.title.clone(),
                issue.body.clone().unwrap_or_default(),
                format!("linear:{id}"),
            ))
        }
        TicketRef::GitLab(id) => {
            let tracker = loom_trackers::gitlab::GitLabTracker::new(None, token.clone())?;
            let issue = tracker.get_issue(id).await?;
            Ok((
                issue.title.clone(),
                issue.body.clone().unwrap_or_default(),
                format!("gitlab:{id}"),
            ))
        }
        TicketRef::Jira(id) => {
            let tracker =
                loom_trackers::jira::JiraTracker::new(None, None, token.clone())?;
            let issue = tracker.get_issue(id).await?;
            Ok((
                issue.title.clone(),
                issue.body.clone().unwrap_or_default(),
                format!("jira:{id}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_ticket_ref, TicketRef};

    #[test]
    fn parses_explicit_tracker_prefixes() {
        assert!(matches!(
            parse_ticket_ref("github:owner/repo#123"),
            Some(TicketRef::GitHub(id)) if id == "owner/repo#123"
        ));
        assert!(matches!(
            parse_ticket_ref("linear:ENG-42"),
            Some(TicketRef::Linear(id)) if id == "ENG-42"
        ));
        assert!(matches!(
            parse_ticket_ref("gitlab:owner/repo#7"),
            Some(TicketRef::GitLab(id)) if id == "owner/repo#7"
        ));
        assert!(matches!(
            parse_ticket_ref("jira:PROJ-9"),
            Some(TicketRef::Jira(id)) if id == "PROJ-9"
        ));
    }

    #[test]
    fn parses_bare_github_forms() {
        assert!(matches!(
            parse_ticket_ref("owner/repo#123"),
            Some(TicketRef::GitHub(_))
        ));
    }

    #[test]
    fn rejects_blank_and_unknown_refs() {
        assert!(parse_ticket_ref("").is_none());
        assert!(parse_ticket_ref("   ").is_none());
        assert!(parse_ticket_ref("github:").is_none());
        assert!(parse_ticket_ref("linear:").is_none());
        assert!(parse_ticket_ref("not a ticket").is_none());
        assert!(parse_ticket_ref("unknown:XYZ-1").is_none());
    }
}
