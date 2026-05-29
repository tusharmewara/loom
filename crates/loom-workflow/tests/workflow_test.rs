use loom_core::{SubAgentConfig, TaskPhase};
use loom_workflow::WorkflowEngine;
use tempfile::tempdir;

#[test]
fn test_create_and_advance_task() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task("Auth refactor", "Refactor distributed security middleware")
        .unwrap();
    assert!(task.complexity_score.is_some());
    assert!(task.routing_decision.is_some());

    let advanced = orch.advance_phase(&task.task_id).unwrap();
    assert!(matches!(
        advanced.phase,
        loom_core::TaskPhase::Implement
    ));
}

#[test]
fn test_sub_agents_resolution() {
    let dir = tempdir().unwrap();
    let engine = WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();

    let task = orch
        .create_task("Sub-agent test", "Test per-phase agent resolution")
        .unwrap();

    // Set sub-agents
    let config = SubAgentConfig {
        plan: Some("plan-agent".to_string()),
        implement: Some("impl-agent".to_string()),
        verify: Some("verify-agent".to_string()),
        finish: Some("finish-agent".to_string()),
    };
    orch.set_sub_agents(&task.task_id, config).unwrap();

    // Set last_agent so fallback works if sub-agent missing
    orch.set_last_agent(&task.task_id, "default-agent").unwrap();

    // resolve_agent uses current task phase — should pick plan-agent for new tasks
    let resolved = orch.resolve_agent(&task.task_id).unwrap();
    assert_eq!(resolved, Some("plan-agent".to_string()), "new tasks start in Plan phase, should use plan-agent");

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
