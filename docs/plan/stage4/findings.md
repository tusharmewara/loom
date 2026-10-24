- tag: spec-gap
  at: crates/loom-core/src/types.rs:985
  Task struct is missing the `converged: Option<bool>` field with `#[serde(default)]` required by acceptance criterion #1.

- tag: spec-gap
  at: crates/loom-cli/src/cli.rs:958
  WorkflowAction enum lacks a `Converge` variant for `loom workflow converge <task-id>` command per acceptance criterion #2.

- tag: spec-gap
  at: crates/loom-cli/src/commands/workflow.rs:54
  The `run` function's match on `WorkflowAction` has no handler for `Converge` (variant doesn't exist yet); needs new arm calling converge logic.

- tag: spec-gap
  at: crates/loom-workflow/src/task_manager.rs:356
  TaskManager has `advance_phase` but no `converge_task` method; needs new public method to load task, read spec.md/findings.md, compute verdict, update `converged` field, and persist.

- tag: spec-gap
  at: crates/loom-workflow/src/orchestrator.rs:32
  TaskOrchestrator exposes `advance_phase` but no `converge_task` wrapper; needs pass-through to TaskManager.

- tag: already-exists
  at: crates/loom-cli/Cargo.toml:32
  loom-cli already depends on loom-hindsight, so `Hindsight::retain()` can be called directly from CLI converge handler for lesson storage (acceptance #3).

- tag: risk
  at: crates/loom-workflow/Cargo.toml:6
  loom-workflow does NOT depend on loom-hindsight; any lesson retention in workflow crate must use fallback file write (`~/.loom/workflow/tasks/<task_id>.lesson.md`) per spec Decision "Lesson retain" fallback.

- tag: already-exists
  at: crates/loom-workflow/src/task_manager.rs:56
  Task files stored at `~/.loom/workflow/tasks/<task_id>.json` via `task_path()`; spec.md at `<task_id>.spec.md` (created in create_task line 349); findings.md expected at `<task_id>.findings.md` per spec.

- tag: needs-clarification
  at: crates/loom-workflow/src/task_manager.rs:328
  `spec_path` and `findings_path` fields exist on Task but findings_path is always None on create; converge needs to read both files from tasks_dir using task_id convention, not from Task fields.

- tag: spec-gap
  at: crates/loom-hindsight/src/lib.rs:136
  `Hindsight::retain(content, fact_type, source, importance)` returns `Result<String>` (fact ID); converge handler must call with `fact_type="lesson"`, source="workflow:<task_id>", importance based on verdict.

- tag: spec-gap
  at: crates/loom-core/src/types.rs:1522
  Existing round-trip test `new_task_phases_round_trip` constructs Task without `converged` field; will need updating once field is added (acceptance #1 grading).

- tag: spec-gap
  at: crates/loom-workflow/tests/workflow_test.rs:6
  Integration tests cover create/advance but no converge test; need new test `test_workflow_converge_verdicts` in loom-cli (acceptance #2) and unit test in loom-workflow for converge path (acceptance #3, #4).

- tag: infeasible
  at: crates/loom-workflow/src/task_manager.rs:235
  `read_task_file` uses `serde_json::from_str` — adding `converged: Option<bool>` with `#[serde(default)]` on Task struct will make old JSON load correctly (None default), satisfying backward compatibility (acceptance #1, constraint #31).

- tag: needs-clarification
  at: docs/plan/stage4/spec.md:37
  Verdict logic: "findings.md has no un-closed `spec-gap`/`risk` findings" — need to define what "un-closed" means in findings.md format (e.g., lines starting with `- [ ]` vs `- [x]`, or tag presence without resolution marker).

SIMULATE DONE: 14 findings.