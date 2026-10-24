# Stage 3: CLI entry into the spec-kit loop — `loom workflow create --spec`

## Problem

Stage 2 put Spec/Simulate phases, `spec_path`/`findings_path`, and the
ComplexityScore gate into the engine (merge `66994be`). But entering the
loop still requires hand-editing a task JSON to set `"phase": "Spec"` —
the decision from Stage 2 (§5 Decisions) was a placeholder, not the
user-facing entry point. A task created with `loom workflow create` always
starts at `TaskPhase::Plan`, so the loop never starts without manual JSON
surgery.

## Acceptance

1. `loom workflow create` gains a `--spec` flag: the task is created in
   `TaskPhase::Spec` (not `Plan`), and `spec_path` defaults to
   `<task_id>.spec.md` next to the task file. `--spec` without
   `--worktree` is fine; `--spec` with `--worktree` works too.
   Grading: `cargo test -p loom-cli` (new test:
   `test_create_with_spec_flag_starts_in_spec_phase` plus a round-trip
   test that re-deserializes the written task JSON).
2. The Stage-2 ComplexityScore gate still applies: advancing from Spec
   with below-Mid score jumps straight to Plan (single-step problems
   stay out of the loop); Mid+ proceeds to Simulate. Unchanged from
   Stage 2 — graded by `cargo test -p loom-workflow --all-features`
   (existing gate tests still pass, no behavior change).
3. No new crate, no new dependency in any Cargo.toml.
   Grading: `git diff main..HEAD -- '**/Cargo.toml'` is empty.
4. The change is confined to `crates/loom-cli/src/cli.rs` and
   `crates/loom-cli/src/commands/workflow.rs` plus tests in
   `crates/loom-workflow/tests/workflow_test.rs` (unit tests may live in
   either crate as long as they pass). No CLI surface beyond one flag.
   Grading: `cargo check --workspace --exclude loom-tui` and
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   both pass.
5. Back-compatible: `loom workflow create` without `--spec` behaves
   exactly as before (phase Plan, spec_path None).
   Grading: the existing `test_create_and_advance_task` test still
   passes unchanged.

## Out of scope

- Auto-writing spec.md content (the agent writes it during the simulate
  stage; `spec_path` just records where).
- TUI changes.
- Any dispatch/prompt injection of spec contents.
- loom-rules, event-bus, migrations.

## Constraints

- Existing crates only: `loom-cli`, `loom-workflow` (tests).
- No new dependency.
- Old task JSON must keep loading: `spec_path` remains `#[serde(default)]`.
- The flag is additive: `--spec` coexists with `--from-issue`,
  `--from-ticket`, `--template`, `--from-plan` (each sets phase Spec if
  given; the JSON/plain paths go through `run_create`).

## Decisions

- **Flag name:** `--spec` (short `-s` risks clashing with nothing
  existing; check against current flags — none use `-s`).
- **Default spec_path:** `<task_id>.spec.md` in the tasks dir, written
  by `run_create` when `--spec` is given. The file itself is created
  empty (0 bytes) — content comes from the agent's simulate run.
- **`--spec` + `--from-*` combos:** allowed; phase Spec set after task
  creation regardless of source. Keep it simple: one code path.
- **Gate placement:** unchanged from Stage 2 — inside `advance_phase`
  via `spec_gate_next_phase`; the CLI flag only chooses the starting
  phase.

## Closure notes (S3 harden) — applied to all 10 findings

### already-exists (4 findings — closed, no code change)

1. TaskPhase::Spec variant already exists at types.rs:1081 → closed; no code change needed.
2. Task.spec_path already Option<String> with #[serde(default)] at types.rs:1012-1013 → closed; backward-compatible.
3. spec_gate_next_phase gate already exists at task_manager.rs:13-26 → closed; Stage 2 gate unchanged.
4. advance_phase already branches on Spec at task_manager.rs:347-348 → closed; wired.

### spec-gap (5 findings — closed into spec amendments)

5. cli.rs:972-1005 missing --spec flag → AMEND §Acceptance #1: add `#[arg(long)] spec: bool` to WorkflowAction::Create in cli.rs.
6. workflow.rs:333-442 run_create doesn't handle spec flag → AMEND: thread `spec: &bool` through run_create; pass to create_task as new parameter `initial_phase: Option<TaskPhase>`.
7. task_manager.rs:308 hardcodes phase=Plan → AMEND: create_task gains `initial_phase: Option<TaskPhase>` param; default stays Plan when None.
8. task_manager.rs:325 spec_path=None → AMEND: when initial_phase == Some(Spec), set spec_path = Some(format!("{task_id}.spec.md")) and create the empty file.
9. task_manager.rs:246-339 signature lacks param → AMEND: add `initial_phase: Option<TaskPhase>` to create_task signature; update all call sites (lines 358, 394, 427, 437).

### needs-clarification (1 finding — closed by decision)

10. run_create 11 params / four branches → DECISION: add `spec: &bool` as a single parameter to run_create (after `no_worktree`); every branch passes it through. One code path confirmed: all branches call create_task.

### risk (1 finding — mitigated)

11. Existing test_create_and_advance_task expects Plan start → MITIGATION: existing test unchanged (still creates without --spec → Plan). Add new test `test_create_with_spec_flag_starts_in_spec_phase` and a spec_path round-trip test. No breakage.

HARDEN DONE: all 10 findings closed; none deleted.
