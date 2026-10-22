# Stage 1 — Spec-Kit Loop Trials (pre-registered)

Measurement stage of [LOOM_SPECKIT_HARNESS_PROPOSAL.md](LOOM_SPECKIT_HARNESS_PROPOSAL.md)
§4 Stage 1. This document is written and committed BEFORE any trial run
starts. Nothing in it changes after results arrive.

## 1. The five problems (verbatim, gate-scored)

All five are real repo work, selected before any run, gated by the repo's
own scorer (`loom routing score`, ComplexityThresholds band Mid ≥ 0.35):

| id | score | problem (verbatim prompt text) |
|----|-------|-------------------------------|
| T1 | Medium 0.453 | Pillar 5 verified review engine MVP slice: create the loom-review crate with session-to-review-plan conversion reading loom-sessions exports, the proven/suite_green/unverified/could_not_run/refuted evidence-state model, and unit tests; wire the new crate into the workspace cross-module. No ACP dispatch, no review.toml, no fix PR in this slice. |
| T2 | Medium 0.463 | The Performance Budgets table in AGENTS.md documents twelve millisecond budgets (daemon startup, skill search, MCP acquire, hot-reload, session resume, routing decision, hook execution, context injection, event delivery, vault decrypt, knowledge query, knowledge build) but only loom-event-bus has a bench and no budget is verified anywhere. Add performance regression tests across modules for the budgets that are meaningfully measurable in CI without becoming flaky. |
| T3 | Medium 0.403 | The ACP dispatch lifecycle (acp.dispatch spawning an agent headlessly, run tracking, run-status reporting, session persistence) is only tested live behind LOOM_ACP_LIVE=1 and is therefore untested in CI. Add a hermetic in-process ACP lifecycle test across multiple services (the dispatch server, the tracked-run store, and session persistence) that runs without any live network. |
| T4 | Medium 0.400 | loom sessions resume supports --inject-knowledge and --inject-memory which query the loom-knowledge graph and the loom-hindsight memory store and inject a system message before resuming, but no test exercises this path. Add an integration test across modules (loom-sessions, loom-knowledge, loom-hindsight) proving the injection end to end. |
| T5 | Medium 0.505 | AGENTS.md claims every crate implements loom_core::HealthCheck and the daemon aggregates all of them on startup, but only 15 of 31 crates have an impl HealthCheck today. Audit the gap across modules and either implement the trait where the daemon genuinely needs it or correct the claim; the fix must make the documented architecture true. |

Naming contracts for mechanical grading (both arms get the same text, so
neither is advantaged):
- T2: every new test is named with the `perf_budget` prefix.
- T3: the new integration test file in loom-sessions is `tests/acp_lifecycle.rs`.
- T4: the new integration test in loom-integration-tests is named with
  the `resume_inject` prefix.
- T5: the corrected AGENTS.md sentence states the count numerically
  ("N of 31 crates implement loom_core::HealthCheck").

## 2. Two-arm design

- Base tree for BOTH arms: commit `1fc96f4` (speckit/proposal tip; main +
  the four skills + this trial plan). One shared git worktree at
  `/tmp/stage1/tree`, reset --hard to the base between arms; files changed
  by the previous run are `touch`ed after reset so cargo never reuses a
  stale artefact (Phase B lesson, LOOM_PHASE_B_RESULT.md §9).
- Shared `CARGO_TARGET_DIR=/tmp/stage1/target`, mtime-normalized as above.
- Same model for both arms (free tier via the existing dsh headless
  profile; model recorded in the results from the run logs).
- **DIRECT arm** = one run: the problem statement + grading command +
  rules (minimal, do not weaken tests, do not commit, confirm the grade
  passes). This is what happens today without the loop.
- **LOOP arm** = four sequential runs, artifacts persisting in the tree:
  specify (read `skills/speckit-specify.md`, write spec.md) → simulate
  (read `skills/speckit-simulate.md`, findings.md, budget ~20 min) →
  harden (read `skills/speckit-harden.md`) → implement (read
  `skills/speckit-implement.md`; told the worktree already exists so it
  does not create one).
- Time caps per run: direct 25 min; specify 10; simulate 25; harden 10;
  implement 35. Enforced with gtimeout; a capped run records its partial
  state and the arm is graded on it.

## 3. Measurement (fixed before runs)

- **Arm verdict** (per problem, per arm): the problem's grading command
  exits 0 on the arm's final tree.
  - T1: `cargo test -p loom-review --all-features` (crate exists,
    compiles, tests pass) AND `cargo check --workspace --exclude loom-tui`.
  - T2: `cargo test --workspace --exclude loom-tui --all-features --release perf_budget` exits 0 with ≥3 tests run, executed twice, both green.
  - T3: `cargo test -p loom-sessions --all-features --test acp_lifecycle` green with no `LOOM_ACP_LIVE` in the environment.
  - T4: `cargo test -p loom-integration-tests --all-features resume_inject` green.
  - T5: the numeric count in the corrected AGENTS.md sentence equals
    `grep -r "impl HealthCheck for" crates --include=*.rs | wc -l`, and
    `cargo test -p loomd --all-features` green.
- **Wider grade** (both arms, evidence for what a direct run misses):
  `cargo test --workspace --exclude loom-tui --all-features` and
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  on the arm's final tree.
- **BLOCKING-CAUGHT** (the signal the kill rule counts) requires ALL of:
  (a) a finding in the loop's findings.md tagged risk / already-exists /
  infeasible / spec-gap; (b) harden closed it into a spec change or a kill;
  (c) the DIRECT arm's final tree demonstrably trips it — the wider grade
  fails on direct's tree, direct's diff reimplements something that
  already exists, or direct violates the constraint the finding named.
- **Cost**: sum of input+output tokens over all runs of an arm (from
  NDJSON step_end usage). Cache-read tokens are reported but excluded
  from the cost ratio.

## 4. The kill rule (verbatim from the proposal §3)

> over ≥5 real multi-step problems, loop must catch ≥1 blocking finding
> that direct misses at ≤2× token cost; zero blocking finds on ≥3 problems
> → does not ship, lesson recorded

- SHIP (Stage 2) iff ≥1 blocking-caught AND aggregate loop tokens ≤ 2×
  aggregate direct tokens.
- ZERO blocking-catches across ≥3 problems → the loop does not ship; the
  lesson goes to the pillar-5 lesson store (`loom memory retain --type
  lesson`).
- Fewer than 5 problems completable → partial result, reported as such.

## 5. Honesty notes

- Problem selection and this protocol are fixed before any run; gate
  scores above were computed with the repo's own scorer on the verbatim
  prompt text. T2/T3 descriptions were rephrased once, before locking, to
  use accurate terms ("across modules", "across multiple services") the
  scorer's literal patterns recognize; the rephrased text is still a
  true description of the work.
- The DIRECT arm sees the same tree, including `skills/speckit-*.md`. If
  a direct run spontaneously finds and follows a skill, that is itself
  recorded data about the process.
- The loop arm's stages are graded as a chain: a kill at harden ends the
  arm with verdict "killed" and the implement run is skipped.
- Tokens are free-tier ($0 API cost); the cost comparison is in tokens,
  not dollars.
