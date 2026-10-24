# Stage 4: Converge + lesson capture — closing the spec-kit loop

## Problem

Stage 3 wired the CLI entry point (`loom workflow create --spec`). But the loop can't close: there's no **converge** step (spec-kit's "repeat implement and converge until Converged") and no way to feed implementation-time defects back as **lessons** into loom-hindsight (pillar 5's `loom memory retain --type lesson`). After S4 implement, a task has no convergence verdict and no lesson is recorded — so the spec-kit loop is one-shot, not a loop.

## Acceptance

1. `Task` gains `converged: Option<bool>` with `#[serde(default)]` — backward compatible with old task JSON.
   Grading: `cargo test -p loom-core --all-features` (existing round-trip test still passes).
2. `loom workflow converge <task-id>` command added: reads the task's spec.md (at `<tasks_dir>/<task_id>.spec.md`) and findings.md, checks whether the implementation matches the spec's acceptance, and reports a verdict — `verified` / `partial` / `failed` (only `verified` confirms convergence).
   Grading: `cargo test -p loom-cli` (new test `test_workflow_converge_verdicts`).
3. On `converge → verified`, a lesson is retained to loom-hindsight via `loom memory retain --type lesson` (or the loom-workflow equivalent); on `partial`/`failed`, no lesson is written but findings are preserved.
   Grading: check that converge calls the lesson-retain path (unit test in loom-workflow).
4. `Task.converged` set to `true` on verified, `false` on partial/failed.
   Grading: unit test asserts converged flag after converge.
5. No new crate, no new dependency. Confined to `crates/loom-cli`, `crates/loom-workflow`, `crates/loom-core` + tests.
   Grading: `git diff main..HEAD -- '**/Cargo.toml'` empty; `cargo check --workspace --exclude loom-tui` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean.

## Out of scope

- Auto-running the implement stage after converge (manual trigger is fine).
- TUI changes.
- ACP dispatch for converge.
- Re-converge loops inside the CLI (the agent/user repeats converge manually).

## Constraints

- Existing crates only: loom-cli, loom-workflow, loom-core.
- No new dependency.
- Old task JSON must keep loading (serde default on converged).
- The converge verdict is based on spec.md + findings.md contents, not on re-running tests.

## Decisions

- **Command:** `loom workflow converge <task-id>` (new WorkflowAction::Converge variant).
- **Verdict logic:** if spec.md exists and is non-empty AND findings.md has no un-closed `spec-gap`/`risk` findings → `verified`; if spec.md exists but there are open findings → `partial`; if no spec.md → `failed`.
- **Lesson retain:** call into loom-hindsight's lesson store; if unavailable (no loom-hindsight dep), fall back to writing a lesson file under `~/.loom/workflow/tasks/<task_id>.lesson.md`. (Decide in S2 after checking loom-hindsight API.)
- **converged field:** `Option<bool>` — None = not yet converged, Some(true) = verified, Some(false) = partial/failed.

## Closure notes (S3 harden) — applied to all 14 findings

### spec-gap (7 findings — closed into spec amendments)

1. types.rs:985 — Task missing `converged: Option<bool>` → AMEND: add `converged: Option<bool>` with `#[serde(default)]` to Task struct.
2. cli.rs:958 — WorkflowAction lacks Converge variant → AMEND: add `Converge { task_id: String }` variant.
3. workflow.rs:54 — no Converge arm in run() match → AMEND: add arm calling `run_converge(loom_dir_str, orchestrator, task_id)`.
4. task_manager.rs:356 — no `converge_task` method → AMEND: add `pub fn converge_task(&self, task_id: &str) -> Result<ConvergeVerdict>` that loads task, reads `<tasks_dir>/<task_id>.spec.md` + `.findings.md`, computes verdict, sets `converged`, persists.
5. orchestrator.rs:32 — no converge wrapper → AMEND: add `pub fn converge_task(&self, task_id: &str) -> Result<ConvergeVerdict>` pass-through.
6. loom-hindsight lib.rs:136 — `retain(content, fact_type, source, importance)` → AMEND: CLI converge calls `retain(lesson_text, "lesson", format!("workflow:{}", task_id), importance)` where importance = 0.8 (verified) / 0.5 (partial) / 0.0 (failed, no lesson).
7. types.rs:1522 — round-trip test needs converged field → AMEND: add `converged: None` to the test Task literal (backward-compat test continues to pass).

### already-exists (3 findings — closed, no code change)

8. loom-cli/Cargo.toml:32 — loom-cli already depends on loom-hindsight → closed; lesson storage via CLI.
9. task_manager.rs:56 — task_path convention confirmed → closed; spec.md/findings.md live at tasks_dir/<id>.spec.md + .findings.md.
10. loom-hindsight retain returns Result<String> → closed; used for lesson ID.

### risk (2 findings — mitigated)

11. loom-workflow/Cargo.toml:6 — no hindsight dep → MITIGATION: lesson retention happens in CLI (which has hindsight); workflow crate returns verdict only, no lesson logic.
12. task_manager.rs:235 — serde default handles old JSON → MITIGATION: `#[serde(default)]` on converged makes old files load as None. Confirmed infeasible=actually-safe finding.

### infeasible (1 finding — resolved as non-issue)

13. read_task_file serde from_str → RESOLVED: `#[serde(default)]` makes old JSON backward compatible; not infeasible, it's already-exists-safe.

### needs-clarification (2 findings — closed by decision)

14. findings_path always None / task_id convention → DECISION: converge reads files from tasks_dir using task_id convention (`<id>.spec.md`, `<id>.findings.md`), NOT from Task fields. findings_path field reserved for future use.
15. "un-closed" findings definition → DECISION: a finding line is "open" if it starts with `- tag:` and does NOT have a `closed: true` marker; converge treats any spec-gap or risk tag without `closed: true` as open → verdict partial. "closed" markers are added by S3 harden notes in findings.md.

HARDEN DONE: all 14 findings closed; none deleted.
