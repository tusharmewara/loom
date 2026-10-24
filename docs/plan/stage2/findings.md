- tag: spec-gap
  at: crates/loom-core/src/types.rs:1051
  The `TaskPhase` enum lacks `Spec` and `Simulate` variants required by the spec; currently only has `Plan`, `Implement`, `Verify`, `Finish`.

- tag: spec-gap
  at: crates/loom-core/src/types.rs:1058
  `TaskPhase::name()` method does not handle `Spec` and `Simulate` variants; will panic or return incorrect names if new variants are added without updating this match.

- tag: spec-gap
  at: crates/loom-workflow/src/phases.rs:14
  `next_phase` function walks `Plan → Implement → Verify → Finish → None` but spec requires `Spec → Simulate → Plan → Implement → Verify → Finish → None`.

- tag: spec-gap
  at: crates/loom-workflow/src/phases.rs:4
  `default_model_tier` maps only `Plan→Flagship`, `Implement→Mid`, `Verify→Mid`, `Finish→Slm`; spec requires `Spec→Flagship` and `Simulate→Flagship` mappings.

- tag: spec-gap
  at: crates/loom-core/src/types.rs:985
  `Task` struct lacks `spec_path: Option<String>` and `findings_path: Option<String>` fields with `#[serde(default)]` for backward compatibility with old task JSON files.

- tag: spec-gap
  at: crates/loom-workflow/src/task_manager.rs:321
  `advance_phase` lacks the ComplexityScore gate when current phase is `Spec`; spec requires checking `task.complexity_score` (or classifying `task.description`) and skipping to `Plan` if tier < Mid, else advancing to `Simulate` normally.

- tag: already-exists
  at: crates/loom-routing/src/classifier.rs:18
  `Classifier::classify` and `classify_with_role` already exist for computing complexity scores from task descriptions; spec correctly says to reuse these rather than re-implement scoring.

- tag: already-exists
  at: crates/loom-core/src/types.rs:564
  `ComplexityScore` struct already has `model_tier: ModelTier` field; the gate can check `score.model_tier >= ModelTier::Mid` without new constants.

- tag: already-exists
  at: crates/loom-core/src/types.rs:197
  `derive_level` and `derive_tier` functions in loom-core already implement the routing classification thresholds (Low<0.35=Slm, Medium=Mid, High=Flagship); spec constraint "Mid threshold is existing routing classification" is satisfied by reusing these.

- tag: risk
  at: crates/loom-core/src/types.rs:1130
  `SubAgentConfig::agent_for` matches on `TaskPhase` with only `Plan/Implement/Verify/Finish` arms; adding `Spec`/`Simulate` will cause non-exhaustive match warnings/errors unless a catch-all or explicit arms are added.

- tag: risk
  at: crates/loom-workflow/src/task_manager.rs:391
  `advance_phase` determines `agent_role` via match on `TaskPhase` with only `Plan/Implement/Verify/Finish` arms; `Spec` and `Simulate` phases need role mappings (likely `trellis-brainstorm` or similar for both).

- tag: risk
  at: crates/loom-cli/src/commands/workflow.rs:695
  CLI auto-advance chain checks `matches!(current_task.phase, TaskPhase::Finish)` to stop; with new phases, the chain will naturally stop at `Finish` but the loop entry at `Spec` needs no CLI change per spec (manual manifest edit).

- tag: risk
  at: crates/loom-tui/src/dispatch.rs:649
  TUI embedded-pane dispatch creates a minimal `Task` with hardcoded `phase: TaskPhase::Plan`; if a task is in `Spec` or `Simulate` phase, this would incorrectly reset phase for the runtime task.

- tag: risk
  at: crates/loom-integration-tests/tests/workflow_session.rs:39
  Integration test `all_phases()` helper returns only `[Plan, Implement, Verify, Finish]`; tests using this helper will not cover new phases unless updated.

- tag: risk
  at: crates/loom-event-bus/src/lib.rs:36
  `LoomEvent::TaskPhaseChanged` carries `TaskPhase`; adding variants is serde-compatible but cross-process consumers (daemon, TUI, other tools) must handle new variants or they'll fail to deserialize.

- tag: risk
  at: crates/loom-rules/src/lib.rs:495
  `loom-rules` matches on `LoomEvent::TaskPhaseChanged { task_id, phase }` and may need to handle `Spec`/`Simulate` phases in rule conditions; currently out of scope per spec but a real risk for rules users.

- tag: risk
  at: crates/loom-workflow/src/task_manager.rs:336
  `phase_history` entry uses `format!("{next:?}")` which relies on `Debug` derive; new variants will serialize as `"Spec"` and `"Simulate"` which matches `TaskPhase::name()` output — this is consistent but worth verifying.

- tag: needs-clarification
  at: crates/loom-workflow/src/phases.rs:14
  Spec says gate placement is "inside `advance_phase` calling a small helper in `phases.rs` so the pure `next_phase` chain stays exhaustive and testable." This implies `next_phase(Spec) = Some(Simulate)` and `next_phase(Simulate) = Some(Plan)` in `phases.rs`, while `advance_phase` overrides to `Plan` when gate triggers — but the helper's signature and location are not specified.

- tag: needs-clarification
  at: crates/loom-workflow/src/task_manager.rs:321
  When advancing from `Spec`, the gate reads `task.complexity_score` if present, else classifies `task.description`. However, `create_task` already scores with `agent_role="trellis-brainstorm"` for Plan phase; for `Spec` phase the appropriate agent role for initial scoring is unclear (maybe same as Plan?).

- tag: needs-clarification
  at: crates/loom-core/src/types.rs:1051
  Adding enum variants to `TaskPhase` is a breaking change for exhaustive matches in downstream crates (loom-cli, loom-tui, loom-rules, loom-runtime, integration tests). Spec says "existing crates only" but these crates will need updates to compile — unclear if this is expected within the slice or a follow-up.

- tag: infeasible
  at: crates/loom-workflow/src/task_manager.rs:321
  The spec requires "absent score classifies the description" but `advance_phase` already re-scores with dynamic signals at every phase transition. For `Spec→Simulate` transition, there are no dynamic signals yet (no PRD, no files modified), so classification would be static-only — this is feasible but the spec's "computes-or-reads" implies reading existing score if present, which `advance_phase` currently overwrites unconditionally.

- tag: spec-gap
  at: crates/loom-workflow/tests/workflow_test.rs:154
  Unit test `test_task_phase_name_helper` only asserts `Plan/Implement/Verify/Finish` names; needs `Spec` and `Simulate` assertions for the new variants.

- tag: spec-gap
  at: crates/loom-workflow/tests/workflow_test.rs:22
  Unit test `test_create_and_advance_task` asserts advance from `Plan` goes to `Implement`; with new phases, a task created in `Spec` phase (per spec, manual manifest edit) would advance to `Simulate` or `Plan` depending on gate — no test covers this path yet.
---

## Closure notes (S3 harden — resolved by the spec owner; spec.md amended 2026-10-23)

1. spec-gap @ types.rs:1051 — CLOSED into spec acceptance 1 (Spec+Simulate variants). Spec text already required it; confirmed as work item.
2. spec-gap @ types.rs:1058 — CLOSED into spec acceptance 1 (name() arms "Spec"/"Simulate").
3. spec-gap @ phases.rs:14 — CLOSED into spec acceptance 2 (chain Spec→Simulate→Plan→…).
4. spec-gap @ phases.rs:4 — CLOSED into spec acceptance 2 (Spec→Flagship, Simulate→Flagship).
5. spec-gap @ types.rs:985 — CLOSED into spec acceptance 3 (spec_path/findings_path, #[serde(default)]).
6. spec-gap @ task_manager.rs:321 — CLOSED into spec acceptance 4 (gate: stored score else classify; below Mid → Plan).
7. already-exists @ classifier.rs:18 — CONFIRMED; constraint already says reuse; no spec change needed.
8. already-exists @ types.rs:564 — CONFIRMED; gate uses ComplexityScore.model_tier (constraint amended to name it).
9. already-exists @ types.rs:197 — CONFIRMED; no change (gate reads stored score, threshold application stays at creation).
10. risk @ types.rs:1130 — CLOSED into spec Decisions: agent_for maps Spec/Simulate → plan slot → default fallback; no new config fields.
11. risk @ task_manager.rs:391 — CLOSED into spec Decisions: agent_role Spec/Simulate → "trellis-brainstorm".
12. risk @ workflow.rs:695 — ACCEPTED, no action: matches! macro is non-exhaustive, chain stops at Finish naturally; no CLI change.
13. risk @ dispatch.rs:649 — CLOSED into spec Decisions: out of scope, synthetic task stays TaskPhase::Plan; behavior unchanged.
14. risk @ workflow_session.rs:39 — CLOSED into spec Decisions: all_phases() helper not updated this slice; gate tests cover the new path; coverage follow-up recorded.
15. risk @ event-bus lib.rs:36 — CLOSED into spec Decisions: accepted forward-compat cost (old binaries can't read new phase names); no migration this slice.
16. risk @ loom-rules lib.rs:495 — SAME closure as 15: rules matching TaskPhaseChanged carries the enum; same-version workspaces only this slice.
17. risk @ task_manager.rs:336 — ACCEPTED, no action: format!("{next:?}") yields "Spec"/"Simulate", matching name() by construction; gate tests assert it.
18. needs-clarification @ phases.rs:14 — ANSWERED in spec Decisions: helper in phases.rs, pure next_phase stays exhaustive; advance_phase overrides next after the gate check.
19. needs-clarification @ task_manager.rs:321 — ANSWERED in spec Decisions: gate reads the stored creation-time complexity_score BEFORE the unconditional re-score; re-score stays untouched.
20. needs-clarification @ types.rs:1051 — ANSWERED in spec acceptance 5 + Constraints: mechanical match-arm additions in consuming crates are permitted (compilation-forced only, named in the note); no CLI surface change.
21. infeasible @ task_manager.rs:321 — RESOLVED: not infeasible once the gate reads the stored score before the re-score (spec Decisions: gate semantics). The re-score overwrite is fine.
22. spec-gap @ workflow_test.rs:154 — CLOSED into acceptance 1's grading scope: name-helper test gains Spec/Simulate assertions.
23. spec-gap @ workflow_test.rs:22 — CLOSED into acceptance 4: gate tests cover Spec→(Low→Plan) and Spec→(Mid→Simulate) and absent-score classify.

All 23 findings closed. No criteria deleted. Card not killed.
