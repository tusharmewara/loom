# Stage 2: Spec-Kit Loop Awareness in the Workflow Engine — Spec

## Problem

The spec-kit loop (specify → simulate → harden → implement) is a validated
process: the Stage-1 trials (docs/plan/LOOM_SPECKIT_STAGE1_TRIALS.md §6)
recorded 4 of 5 blocking-catches at 1.58× token cost, and the
pre-registered kill rule said ship. Today the loop lives only in the four
`skills/speckit-*.md` files and a card convention: the workflow engine's
phase pipeline has no Spec/Simulate phases, the task manifest cannot carry
the loop's artifacts, and nothing enforces the ComplexityScore gate that
keeps single-step problems out of the loop.

## Acceptance

1. `TaskPhase` (crates/loom-core/src/types.rs:1051) gains `Spec` and
   `Simulate` variants. `TaskPhase::name()` returns "Spec" and "Simulate";
   the names of Plan/Implement/Verify/Finish are unchanged.
   Grading: `cargo test -p loom-core --all-features` (serde round-trip:
   a task JSON written before this change, with `"phase": "Plan"` and no
   `spec_path`/`findings_path` keys, still deserializes; new variants
   round-trip).
2. `next_phase` (crates/loom-workflow/src/phases.rs) walks
   Spec → Simulate → Plan → Implement → Verify → Finish → None.
   `default_model_tier` maps Spec → Flagship and Simulate → Flagship;
   Plan/Implement/Verify/Finish mappings unchanged.
   Grading: `cargo test -p loom-workflow --all-features` (unit tests for
   the full chain and the tier mapping).
3. `Task` (crates/loom-core/src/types.rs:985) gains two optional fields
   with `#[serde(default)]`: `spec_path: Option<String>` and
   `findings_path: Option<String>`.
   Grading: covered by the loom-core serde round-trip test above.
4. ComplexityScore gate in `advance_phase`
   (crates/loom-workflow/src/task_manager.rs:321): when the task is in
   `Spec`, advancing computes-or-reads the task's complexity score
   (stored `task.complexity_score`, else classify `task.description` with
   the existing routing engine); a score below the Mid band skips the
   loop — the task advances directly to `Plan` (the same jump a human
   makes when speckit-specify says "this is single-step, stop"); Mid or
   above advances to `Simulate` normally.
   Grading: `cargo test -p loom-workflow --all-features` (gate tests:
   Low score → Plan, Mid score → Simulate, absent score classifies the
   description).
5. No new CLI flag, command, or command group; the entry to the loop is
   setting `"phase": "Spec"` in the task manifest (documented in this
   spec's Decisions). Mechanical compilation fixes forced by the new
   enum variants (adding match arms to existing exhaustive matches on
   `TaskPhase`) are permitted anywhere in the workspace; anything that
   changes CLI surface or behavior is not.
   Grading: `git diff speckit/proposal..HEAD -- crates/loom-cli` shows
   no new flags, commands, or subcommand definitions (mechanical match
   arms excepted, with a note naming them).
6. The workspace still builds and lints clean:
   `cargo check --workspace --exclude loom-tui` and
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   both pass.

## Out of scope

- Any CLI flag (including a `--phase` on `loom workflow create`).
- Injecting spec.md/findings.md contents into dispatch prompts
  (`PhaseResult` already carries phase output forward).
- TUI changes beyond what falls out of `TaskPhase::name()` strings.
- loom-rules integration, event-bus events for the new phases,
  persistence migrations beyond serde defaults.
- New crates, new dependencies, new CLI groups (proposal §4 constraint).

## Constraints

- Existing crates only: `loom-core`, `loom-workflow` (and their tests)
  plus the minimal mechanical match-arm additions in consuming crates
  that compilation forces (acceptance 5). No new crate, no new
  dependency in any crate's Cargo.toml.
- `loom-workflow` already depends on `loom-routing`
  (crates/loom-workflow/src/routing.rs); reuse its scorer — do not
  re-implement scoring.
- Old task JSON files (loom-workflow/tasks/*.json written by earlier
  versions) must load unchanged: every new field is `#[serde(default)]`,
  every existing `TaskPhase` name deserializes as before.
- The Mid threshold for the gate is the existing routing classification
  (the stored `ComplexityScore.model_tier`), not a new constant
  duplicated in loom-workflow.

## Decisions (resolved from needs-clarification and harden)

- **Entering the loop:** manual manifest edit (`"phase": "Spec"`). No CLI
  change in this slice; the workflow JSON is already the task's
  source of truth, and `loom workflow advance` walks the rest.
- **Gate placement:** inside `advance_phase` (it owns the task and the
  phase transition), calling a small helper in `phases.rs` so the pure
  `next_phase` chain stays exhaustive and testable.
- **Simulate tier:** Flagship, same as Spec — both stages read the whole
  tree and reason about it; the trials' simulate runs behaved
  Plan-like, not Implement-like.
- **Gate semantics (resolves the infeasible finding):** the gate reads
  the task's stored `complexity_score` (computed at `create_task`)
  BEFORE `advance_phase`'s unconditional re-score at task_manager.rs:399;
  if absent, it classifies `task.description` with the static
  classifier. The re-score at the end of `advance_phase` stays as is.
- **`agent_for` (loom-core types.rs:1131):** `Spec` and `Simulate` map to
  the existing `plan` slot, then the `default_agent` fallback — the
  sub-agent config gains no new fields.
- **`agent_role` (task_manager.rs:391):** `Spec` and `Simulate` map to
  `"trellis-brainstorm"`, the same reasoning role as Plan.
- **TUI dispatch.rs:649:** out of scope. It constructs a synthetic
  minimal task hardcoded to `TaskPhase::Plan` for a dispatch call, not
  phase management; leave its behavior unchanged.
- **Event-bus / loom-rules forward-compat:** old binaries cannot
  deserialize the new `"Spec"`/`"Simulate"` phase names; accepted —
  same-version workspaces only, no migration in this slice.
- **Integration test `all_phases()` helper (workflow_session.rs:39):**
  not updated in this slice; the new gate tests in loom-workflow cover
  the new path. Coverage of Spec/Simulate in integration tests is a
  follow-up.
