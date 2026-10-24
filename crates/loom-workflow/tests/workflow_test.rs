use loom_core::{PhaseResult, SubAgentConfig, TaskPhase};
use loom_workflow::WorkflowEngine;
use tempfile::tempdir;

#[test]
fn test_create_and_advance_task() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task(
            "Auth refactor",
            "Refactor distributed security middleware",
            false,
            None,
            None,
        )
        .unwrap();
    assert!(task.complexity_score.is_some());
    assert!(task.routing_decision.is_some());

    let advanced = orch.advance_phase(&task.task_id).unwrap();
    assert!(matches!(advanced.phase, loom_core::TaskPhase::Implement));
}

#[test]
fn test_sub_agents_resolution() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task(
            "Sub-agent test",
            "Test per-phase agent resolution",
            false,
            None,
            None,
        )
        .unwrap();

    // Set sub-agents
    let config = SubAgentConfig {
        plan: Some("plan-agent".to_string()),
        implement: Some("impl-agent".to_string()),
        verify: Some("verify-agent".to_string()),
        finish: Some("finish-agent".to_string()),
        max_retries: 3,
        fallback_agent: None,
    };
    orch.set_sub_agents(&task.task_id, config).unwrap();

    // Set last_agent so fallback works if sub-agent missing
    orch.set_last_agent(&task.task_id, "default-agent").unwrap();

    // resolve_agent uses current task phase — should pick plan-agent for new tasks
    let resolved = orch.resolve_agent(&task.task_id).unwrap();
    assert_eq!(
        resolved,
        Some("plan-agent".to_string()),
        "new tasks start in Plan phase, should use plan-agent"
    );

    // Advance to Implement — should resolve to impl-agent
    let adv = orch.advance_phase(&task.task_id).unwrap();
    assert!(matches!(adv.phase, TaskPhase::Implement));
    let resolved = orch.resolve_agent(&task.task_id).unwrap();
    assert_eq!(resolved, Some("impl-agent".to_string()));

    // Advance to Verify
    orch.advance_phase(&task.task_id).unwrap();
    let resolved = orch.resolve_agent(&task.task_id).unwrap();
    assert_eq!(resolved, Some("verify-agent".to_string()));

    // Advance to Finish
    orch.advance_phase(&task.task_id).unwrap();
    let resolved = orch.resolve_agent(&task.task_id).unwrap();
    assert_eq!(resolved, Some("finish-agent".to_string()));
}

#[test]
fn test_phase_result_capture_and_replace() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task(
            "Phase result test",
            "Capture and replace per phase",
            false,
            None,
            None,
        )
        .unwrap();

    // No results yet
    assert!(orch.latest_phase_result(&task.task_id).unwrap().is_none());

    // Append a Plan-phase result
    let plan = PhaseResult {
        phase: "Plan".into(),
        agent: "opencode".into(),
        summary: "Plan: use JWT + refresh tokens".into(),
        artifacts: vec!["plan.md".into()],
        completed_at: "2026-06-06T04:00:00Z".into(),
    };
    orch.set_phase_result(&task.task_id, plan.clone()).unwrap();

    let latest = orch.latest_phase_result(&task.task_id).unwrap().unwrap();
    assert_eq!(latest.phase, "Plan");
    assert_eq!(latest.agent, "opencode");
    assert_eq!(latest.summary, "Plan: use JWT + refresh tokens");

    // Advance to Implement and append a different-phase result
    orch.advance_phase(&task.task_id).unwrap();
    let impl_result = PhaseResult {
        phase: "Implement".into(),
        agent: "goose".into(),
        summary: "Implemented auth middleware".into(),
        artifacts: vec![],
        completed_at: "2026-06-06T04:05:00Z".into(),
    };
    orch.set_phase_result(&task.task_id, impl_result.clone())
        .unwrap();

    let latest = orch.latest_phase_result(&task.task_id).unwrap().unwrap();
    assert_eq!(latest.phase, "Implement");
    assert_eq!(latest.agent, "goose");

    // Re-running the Plan phase replaces the prior Plan result (doesn't add a second)
    let plan_v2 = PhaseResult {
        phase: "Plan".into(),
        agent: "opencode".into(),
        summary: "Plan: revised to use OAuth2 PKCE".into(),
        artifacts: vec!["plan-v2.md".into()],
        completed_at: "2026-06-06T04:10:00Z".into(),
    };
    orch.set_phase_result(&task.task_id, plan_v2).unwrap();

    let reloaded = orch.get_task(&task.task_id).unwrap().unwrap();
    assert_eq!(
        reloaded.phase_results.len(),
        2,
        "duplicate Plan result should replace, not append"
    );
    let plan_in_store = reloaded
        .phase_results
        .iter()
        .find(|r| r.phase == "Plan")
        .unwrap();
    assert_eq!(plan_in_store.summary, "Plan: revised to use OAuth2 PKCE");
}

#[test]
fn test_task_phase_name_helper() {
    assert_eq!(TaskPhase::Spec.name(), "Spec");
    assert_eq!(TaskPhase::Simulate.name(), "Simulate");
    assert_eq!(TaskPhase::Plan.name(), "Plan");
    assert_eq!(TaskPhase::Implement.name(), "Implement");
    assert_eq!(TaskPhase::Verify.name(), "Verify");
    assert_eq!(TaskPhase::Finish.name(), "Finish");
}

#[test]
fn test_next_phase_chain() {
    // Test the full chain: Spec -> Simulate -> Plan -> Implement -> Verify -> Finish -> None
    use loom_workflow::phases::next_phase;
    assert_eq!(next_phase(&TaskPhase::Spec), Some(TaskPhase::Simulate));
    assert_eq!(next_phase(&TaskPhase::Simulate), Some(TaskPhase::Plan));
    assert_eq!(next_phase(&TaskPhase::Plan), Some(TaskPhase::Implement));
    assert_eq!(next_phase(&TaskPhase::Implement), Some(TaskPhase::Verify));
    assert_eq!(next_phase(&TaskPhase::Verify), Some(TaskPhase::Finish));
    assert_eq!(next_phase(&TaskPhase::Finish), None);
}

#[test]
fn test_default_model_tier_new_phases() {
    use loom_workflow::phases::default_model_tier;
    use loom_core::ModelTier;
    assert_eq!(default_model_tier(&TaskPhase::Spec), ModelTier::Flagship);
    assert_eq!(default_model_tier(&TaskPhase::Simulate), ModelTier::Flagship);
    assert_eq!(default_model_tier(&TaskPhase::Plan), ModelTier::Flagship);
    assert_eq!(default_model_tier(&TaskPhase::Implement), ModelTier::Mid);
    assert_eq!(default_model_tier(&TaskPhase::Verify), ModelTier::Mid);
    assert_eq!(default_model_tier(&TaskPhase::Finish), ModelTier::Slm);
}

#[test]
fn test_create_with_spec_flag_starts_in_spec_phase() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task(
            "Spec-driven feature",
            "Implement the spec-kit loop entry point",
            false,
            None,
            Some(TaskPhase::Spec),
        )
        .unwrap();

    assert!(matches!(task.phase, TaskPhase::Spec));
    assert_eq!(
        task.spec_path,
        Some(format!("{}.spec.md", task.task_id)),
        "spec_path should default to <task_id>.spec.md when starting in Spec"
    );
    // spec.md placeholder file should exist next to the task manifest
    let spec_path = dir.path().join("workflow").join("tasks").join(format!("{}.spec.md", task.task_id));
    assert!(
        spec_path.exists(),
        "empty spec.md placeholder should be created"
    );
}

#[test]
fn test_create_spec_task_serializes_and_roundtrips() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task(
            "Round-trip test",
            "Old task JSON must keep loading",
            false,
            None,
            Some(TaskPhase::Spec),
        )
        .unwrap();

    let task_id = &task.task_id;
    let reloaded = orch.get_task(task_id).unwrap().unwrap();
    assert_eq!(reloaded.phase, TaskPhase::Spec);
    assert_eq!(reloaded.spec_path, task.spec_path);

    // Back-compat: a task created without --spec still has spec_path None
    let plain = orch
        .create_task("Plain task", "No loop entry", false, None, None)
        .unwrap();
    assert!(matches!(plain.phase, TaskPhase::Plan));
    assert!(plain.spec_path.is_none());
}
