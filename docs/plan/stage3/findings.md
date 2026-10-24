- tag: already-exists
  at: crates/loom-core/src/types.rs:1081
  TaskPhase::Spec enum variant already exists (line 1081), so the target phase is available.

- tag: already-exists
  at: crates/loom-core/src/types.rs:1012-1013
  Task.spec_path is already Option<String> with #[serde(default)], ensuring backward compatibility with old task JSON files.

- tag: already-exists
  at: crates/loom-workflow/src/task_manager.rs:13-26
  spec_gate_next_phase function already implements the ComplexityScore gate: advances from Spec to Simulate when model_tier >= Mid, otherwise jumps to Plan (line 20-24).

- tag: already-exists
  at: crates/loom-workflow/src/task_manager.rs:347-348
  advance_phase already branches on TaskPhase::Spec and calls spec_gate_next_phase (lines 347-348), so the gate logic is wired.

- tag: spec-gap
  at: crates/loom-cli/src/cli.rs:972-1005
  WorkflowAction::Create is missing the --spec flag; no flag definition exists to trigger Spec phase creation.

- tag: spec-gap
  at: crates/loom-cli/src/commands/workflow.rs:333-442
  run_create function does not accept or handle a spec flag; all code paths call create_task with phase hardcoded to Plan (line 358, 394, 427, 437).

- tag: spec-gap
  at: crates/loom-workflow/src/task_manager.rs:308,325
  create_task hardcodes phase=TaskPhase::Plan (line 308) and spec_path=None (line 325); needs a parameter to optionally start in Spec phase and set default spec_path.

- tag: spec-gap
  at: crates/loom-workflow/src/task_manager.rs:246-339
  create_task signature lacks a parameter to control initial phase and spec_path; the default spec_path=<task_id>.spec.md file creation (empty) is not implemented.

- tag: needs-clarification
  at: crates/loom-cli/src/commands/workflow.rs:333
  run_create has 11 parameters; adding spec flag requires threading it through all call sites (from_issue, from_ticket, from_plan, template, plain create) — spec says "one code path" but current code has four distinct branches.

- tag: risk
  at: crates/loom-workflow/tests/workflow_test.rs:6-24
  Existing test_create_and_advance_task expects new tasks to start in Plan phase and advance to Implement; adding --spec must not break this test (acceptance #5), but the test will need a counterpart for --spec behavior.

- tag: spec-gap
  at: crates/loom-workflow/tests/workflow_test.rs
  No test exists for test_create_with_spec_flag_starts_in_spec_phase or round-trip deserialization of spec_path (acceptance #1 grading requirement).

SIMULATE DONE: 10 findings.