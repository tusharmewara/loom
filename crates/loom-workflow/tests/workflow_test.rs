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
