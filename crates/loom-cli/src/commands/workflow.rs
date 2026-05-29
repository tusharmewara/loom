use crate::cli::WorkflowAction;
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
        } => {
            // If --from-issue is given, fetch from GitHub
            if let Some(issue_ref) = from_issue {
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

            launch_agent_with_task_context(loom_dir_str, &task, &resolved_agent, tool_args, *dry_run)
                .await?;
            if !dry_run {
                orchestrator.set_last_agent(task_id, &resolved_agent)?;
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
    }
    Ok(())
}

async fn launch_agent_with_task_context(
    loom_dir: &str,
    task: &loom_core::Task,
    agent: &str,
    tool_args: &[String],
    dry_run: bool,
) -> anyhow::Result<()> {
    let home = std::env::var("HOME").expect("HOME not set");

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    env_vars.insert("LOOM_ENABLED".to_string(), "1".to_string());
    env_vars.insert("LOOM_TASK_ID".to_string(), task.task_id.clone());
    env_vars.insert("LOOM_TASK_PHASE".to_string(), format!("{:?}", task.phase));
    env_vars.insert("LOOM_TASK_TITLE".to_string(), task.title.clone());

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

    if dry_run {
        println!("\n=== Dry Run: dispatch {} with {} ===", task.task_id, agent);
        println!("Binary: {}", binary_path.display());
        println!("Args: {:?}", tool_args);
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
        return Ok(());
    }

    // Spawn the agent tool
    let mut cmd = std::process::Command::new(&binary_path);
    cmd.args(tool_args);
    cmd.envs(&env_vars);
    cmd.stdin(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Agent tool '{agent}' exited with status: {status}");
    }
    Ok(())
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
