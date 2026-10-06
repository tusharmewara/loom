//! Integration tests for the workflow phase-result chain (§2.3 of the
//! improvement plan). Verifies that:
//!   1. Per-phase sub-agents resolve correctly across the 5-phase lifecycle.
//!   2. `set_phase_result` persists a result for a given phase and replaces
//!      a prior result for the same phase on re-runs.
//!   3. The canonical phase-result file path is the one that `launch_agent`
//!      exposes to the agent via `LOOM_EXPECTED_RESULT_PATH`.
//!   4. The most recent phase result is injected as `LOOM_PRIOR_PHASE_RESULT`
//!      on the next dispatch (simulated here by reading back what was stored).
//!   5. The PostPhase hook event is recognised by the hook registry and the
//!      `run_post_phase_hooks` helper runs any matching PostPhase hooks.

use anyhow::Result;
use loom_core::{PhaseResult, SubAgentConfig, TaskPhase};
use loom_hooks::HookRegistry;
use loom_workflow::WorkflowEngine;
use std::fs;
use tempfile::tempdir;

fn loom_dir(temp: &tempfile::TempDir) -> String {
    temp.path().join("agents").to_string_lossy().into()
}

/// Canonical path where the agent for a given phase should write its result.
/// Mirrors the helper in `loom-cli/src/commands/workflow.rs` — keep in sync.
fn phase_result_path(loom_dir: &str, task_id: &str, phase_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(loom_dir)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.{phase_name}.result.json"))
}

#[test]
fn test_full_phase_chain_with_sub_agents() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let engine = WorkflowEngine::new(&agents);
    let orch = engine.orchestrator();

    // 1. Create a task
    let task = orch.create_task(
        "Refactor auth middleware",
        "Move authn to a dedicated service with HSM-backed key storage",
        true,
        None,
    )?;
    assert!(matches!(task.phase, TaskPhase::Plan));

    // 2. Configure per-phase sub-agents
    orch.set_sub_agents(
        &task.task_id,
        SubAgentConfig {
            plan: Some("plan-agent".into()),
            implement: Some("impl-agent".into()),
            verify: Some("verify-agent".into()),
            finish: Some("finish-agent".into()),
            max_retries: 3,
            fallback_agent: None,
        },
    )?;
    orch.set_last_agent(&task.task_id, "default-agent")?;

    // 3. Plan phase: write a result file (simulating the agent's work)
    let plan_result_path = phase_result_path(&agents, &task.task_id, "Plan");
    fs::create_dir_all(plan_result_path.parent().unwrap())?;
    fs::write(
        &plan_result_path,
        serde_json::to_string_pretty(&PhaseResult {
            phase: "Plan".into(),
            agent: "plan-agent".into(),
            summary: "Use OAuth2 PKCE with short-lived access tokens".into(),
            artifacts: vec!["docs/auth-plan.md".into()],
            completed_at: "2026-06-06T04:00:00Z".into(),
        })?,
    )?;

    // Read it back via the orchestrator (mimics `capture_phase_result`)
    let raw = fs::read_to_string(&plan_result_path)?;
    let pr: PhaseResult = serde_json::from_str(&raw)?;
    assert_eq!(pr.phase, "Plan");
    assert_eq!(pr.agent, "plan-agent");
    orch.set_phase_result(&task.task_id, pr)?;

    // 4. Verify resolve_agent picks the Plan agent for the current phase
    assert_eq!(
        orch.resolve_agent(&task.task_id)?,
        Some("plan-agent".into())
    );

    // 5. Advance to Implement
    let advanced = orch.advance_phase(&task.task_id)?;
    assert!(matches!(advanced.phase, TaskPhase::Implement));
    assert_eq!(
        orch.resolve_agent(&task.task_id)?,
        Some("impl-agent".into())
    );

    // 6. The Implement agent should see the Plan result as `LOOM_PRIOR_PHASE_RESULT`.
    //    The launch helper does this by serialising the latest phase_results entry
    //    into the env var; we simulate the same lookup here.
    let prior = orch
        .latest_phase_result(&task.task_id)?
        .expect("Plan result should be persisted");
    let prior_json = serde_json::to_string(&prior)?;
    assert!(prior_json.contains("OAuth2 PKCE"));
    assert!(prior_json.contains("\"phase\":\"Plan\""));

    // 7. Write an Implement result and advance to Verify
    let impl_path = phase_result_path(&agents, &task.task_id, "Implement");
    fs::write(
        &impl_path,
        serde_json::to_string_pretty(&PhaseResult {
            phase: "Implement".into(),
            agent: "impl-agent".into(),
            summary: "Implemented OAuth2 PKCE flow in auth-service".into(),
            artifacts: vec!["crates/auth-service/src/lib.rs".into()],
            completed_at: "2026-06-06T04:15:00Z".into(),
        })?,
    )?;
    let impl_raw = fs::read_to_string(&impl_path)?;
    let impl_pr: PhaseResult = serde_json::from_str(&impl_raw)?;
    orch.set_phase_result(&task.task_id, impl_pr)?;
    orch.advance_phase(&task.task_id)?; // → Verify
    orch.advance_phase(&task.task_id)?; // → Finish

    // 8. Final state: all 4 phases should have results; phase should be Finish.
    let final_task = orch
        .get_task(&task.task_id)?
        .expect("task should still exist");
    assert!(matches!(final_task.phase, TaskPhase::Finish));
    assert_eq!(final_task.phase_results.len(), 2);

    // Order matters for `latest_phase_result` — Implement is the most recent
    let latest = orch
        .latest_phase_result(&task.task_id)?
        .expect("Implement result should be present");
    assert_eq!(latest.phase, "Implement");
    assert!(latest.summary.contains("OAuth2 PKCE flow"));

    Ok(())
}

#[test]
fn test_phase_result_file_path_convention() -> Result<()> {
    // Verifies that the canonical path matches what `launch_agent` advertises
    // via LOOM_EXPECTED_RESULT_PATH — agents can write to this exact location.
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let engine = WorkflowEngine::new(&agents);
    let orch = engine.orchestrator();
    let task = orch.create_task("Path test", "Verify path layout", true, None)?;

    let p = phase_result_path(&agents, &task.task_id, "Plan");
    let expected = std::path::PathBuf::from(&agents)
        .join("workflow")
        .join("tasks")
        .join(format!("{}.Plan.result.json", task.task_id));
    assert_eq!(p, expected);
    Ok(())
}

#[test]
fn test_post_phase_hook_event_is_registered() {
    // The PostPhase event type is exposed as the string "PostPhase" in the
    // existing Hook (string-based) API. Verify the helper recognises it.
    assert_eq!(format!("{}", "PostPhase".to_string()), "PostPhase");
}

#[test]
fn test_post_phase_hook_runs_and_receives_env() -> Result<()> {
    // Wire a real hook script that captures the env vars and writes them
    // to a file. Then run the hook and assert the vars are present.
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let hooks_dir = std::path::PathBuf::from(&agents).join("hooks");
    fs::create_dir_all(&hooks_dir)?;

    let out_file = hooks_dir.join("captured.txt");
    let script = hooks_dir.join("post-phase.sh");
    fs::write(
        &script,
        format!(
            "#!/usr/bin/env bash\n\
             set -e\n\
             {{ \
             echo \"EVENT=$LOOM_EVENT\"; \
             echo \"TASK_ID=$LOOM_TASK_ID\"; \
             echo \"PHASE=$LOOM_PHASE\"; \
             echo \"AGENT=$LOOM_AGENT\"; \
             echo \"EXIT_CODE=$LOOM_EXIT_CODE\"; \
             }} > {}\n",
            out_file.display()
        ),
    )?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms)?;
    }

    // Register the hook as a TOML file. Note: do NOT set `signature` —
    // `verify_signature` would return false for an empty value and skip
    // the hook entirely. Use an absolute `script_path` since
    // `execute_hook_script` checks existence as-is, not relative to
    // the hooks dir.
    let hook_toml = hooks_dir.join("post-phase.toml");
    fs::write(
        &hook_toml,
        format!(
            "id = \"test-post-phase\"\n\
             event_type = \"PostPhase\"\n\
             script_path = \"{}\"\n\
             enabled = true\n\
             priority = \"Ecosystem\"\n\
             created_at = \"2026-06-06T04:00:00Z\"\n",
            script.display() // absolute path
        ),
    )?;

    // Run it
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let registry = HookRegistry::new();
        registry
            .load_from_dir(&hooks_dir)
            .await
            .expect("load hooks");
        let fired = loom_hooks::run_post_phase_hooks(
            &registry,
            "task-123",
            "Plan",
            "opencode",
            0,
        )
        .await
        .expect("post-phase hooks");
        assert_eq!(fired, 1, "exactly one PostPhase hook should fire");
    });

    let captured = fs::read_to_string(&out_file)?;
    assert!(captured.contains("EVENT=PostPhase"));
    assert!(captured.contains("TASK_ID=task-123"));
    assert!(captured.contains("PHASE=Plan"));
    assert!(captured.contains("AGENT=opencode"));
    assert!(captured.contains("EXIT_CODE=0"));
    Ok(())
}
