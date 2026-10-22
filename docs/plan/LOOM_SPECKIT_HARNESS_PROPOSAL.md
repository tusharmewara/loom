# Loom Spec-Kit-Driven Harness — Proposal

Status: proposed, Stage 0 ready. Validated against github/spec-kit 1.0 (MIT).
Branch: `speckit/proposal`. Not merged.

## 0. The question and the verdict

The ask: a loop where a **generic spec** for a given problem →
**simulated implementation** → **hardened specification** →
**actual implementation**, solving problems given via Loom. Validate the
approach first; scaffold it if probable; build it using the same approach.

Verdict: **probable — worth the cheap version.** Process before code,
measurement pre-registered, productization only on signal. github/spec-kit
1.0 externally validates the family (structured processes + documented
outcomes for coding agents); our S2 simulate step and Loom-native
substrate are the only novel bets.

This is NOT the probe loop Phase B killed. Phase B measured whether
editing *always-on standing context* changes single-line-bugfix outcomes
(zero signal, 5/5). This loop changes the **workflow** — staged artifacts
with gates between them — and targets multi-step problems. Different
mechanism, different workload. We still pre-registered its own kill rule
rather than assume signal.

## 1. What spec-kit actually is (facts from the 1.0 README)

- Python 3.11+/uv CLI (`specify-cli`), MIT, backed by GitHub. Skills run
  in the agent's chat one at a time, reviewed between steps.
- Three **independent processes**, not mandatory phases:
  - **SDD** (core): `/speckit-constitution` (once per project) →
    `/speckit-specify` → `/speckit-plan` (tech constraints) →
    `/speckit-tasks` → `/speckit-implement` → `/speckit-converge`;
    repeat implement+converge until `Converged`.
  - **Bug fixing** (extension): assess → fix → test; final verdict
    `verified | partial | failed`; **only `verified` confirms**.
  - **Idea assessment** (extension): intake → research → define → shape →
    decide; decision `go | needs-clarification | kill`; "stopping with a
    documented reason is also a useful outcome"; hand off `decision.md`
    to `/speckit-specify` when `go`.
- House rules worth stealing: resolve unknowns by **editing existing
    Markdown artifacts, not rerunning stages**; dogfood the process on
    itself.
- Machinery: integrations, extensions, presets, workflows, bundles,
    catalogs, auth, air-gap. None of it is needed below.

## 2. Mapping: spec-kit → the Loom loop

| spec-kit element | Loom loop element | disposition |
|---|---|---|
| constitution (once per project) | `AGENTS.md` already is Loom's project constitution | **skip** — do not create a second always-on doc |
| specify + plan | S1 `spec.md`, four sections: problem · acceptance · out-of-scope · constraints | **borrow** (fused into one file, not two) |
| tasks | Loom workflow tasks already exist | **skip** for v0 |
| implement + converge | S4 implement graded by the repo's own command; explicit verdict | **borrow** — verdicts `verified \| partial \| failed`, only `verified` closes |
| — | S2 simulate pass (repo walk, no code, findings with file:line) | **novel** — spec-kit has no pre-implementation equivalent; this is the measured bet |
| — | S3 harden (findings closed into the spec BEFORE implementing) | **novel** — spec-kit converges after; we harden before |
| bug-assess | same S1–S4 loop, reproduce-first acceptance | borrow later, same scaffold |
| assess `go/needs-clarification/kill` | pre-registered kill rule + `needs-clarification` state | **borrow** |
| specify-cli, extensions, presets, bundles, catalogs, auth | Loom already has skills registry, worktrees, routing, lesson store | **do not port** — duplicate machinery, YAGNI |

## 3. The loop (pre-registered — do not change after seeing results)

- **S1 · spec.** `spec.md`, ≤1 page: the problem; acceptance criteria that
  are **machine-checkable and graded by the repo's own commands**
  (`cargo test`/`cargo clippy`/`cargo check`); out-of-scope; constraints
  (existing crates only, no new deps unless stated).
- **S2 · simulate.** One budget-capped agent run. Walks the repo against
  the spec, writes **no code**, returns findings tagged
  `spec-gap | already-exists | risk | infeasible`, each with `file:line`.
  Unknowns that need a human → `needs-clarification` (ask, don't burn
  more runs); resolve by editing the spec artifact, not rerunning.
- **S3 · harden.** Every finding is closed into the spec or the criterion
  is deleted. No finding left open. A spec that can't be hardened is a
  `kill` with the reason recorded.
- **S4 · implement.** Worktree per run. Graded by the repo's own command.
  Verdict `verified | partial | failed` — only `verified` closes the card.
  Implementation-time defects fix the **spec first**, then the code, and
  the lesson goes to the pillar-5 store (`loom-hindsight` lesson entity;
  `loom memory retain --type lesson`). No new store.

**Kill rule (fixed in advance):** over ≥5 real multi-step problems, the
loop must surface ≥1 blocking finding that a direct run misses, at ≤2×
token cost of direct. Zero blocking finds across ≥3 problems → **does not
ship**; record the negative result in the pillar-5 lesson store and stop.
A negative result is a successful outcome.

**Routing gate (mandatory, not optional):** the loop only engages when
`loom-routing` scores the problem multi-step/high complexity
(`ComplexityScore` → `decide`, crates/loom-routing/src/decision.rs:28).
Single-line fixes get direct runs — Phase B showed they need no help,
and the loop multiplies cost 2–3×.

## 4. Scaffold

- **Stage 0 — process only (no code, ~1–2 days).** Four skills in
  loom-skills format (`speckit-specify`, `speckit-simulate`,
  `speckit-harden`, `speckit-implement`) plus a card convention. Rides the
  Phase-A-validated skill substrate (registry, validation, trigger
  lines) exactly the way spec-kit skills ride agent integrations.
- **Stage 1 — measure.** Run Stage 0 on ≥5 real problems; apply the kill
  rule to the trial data.
- **Stage 2 — productize, only on signal.** Smallest delta: spec fields
  in the workflow task manifest; optional Spec/Simulate handling around
  the existing `TaskPhase { Plan, Implement, Verify, Finish }`
  (crates/loom-core/src/types.rs:1051) and `default_model_tier`
  (crates/loom-workflow/src/phases.rs). **No new crate, no new dependency,
  no new CLI group.**
- Honest failure mode: if the card convention alone catches the findings,
  Stage 2 ships nothing — and that is fine.

## 5. Dogfood: this proposal through its own S2

Findings from simulating this proposal against the repo:

- `already-exists` — the whole substrate: skill registry + validation +
  trigger lines (crates/loom-skills), task phases and model tiers
  (crates/loom-workflow/src/phases.rs), complexity gate
  (crates/loom-routing/src/decision.rs:28), worktree isolation
  (crates/loom-parallel), lesson home (crates/loom-hindsight). Stage 2
  must reuse these, not rebuild them.
- `risk` — "machine-checkable acceptance" is not true of every problem;
  problems without a grading command go to `needs-clarification`, not
  into the loop. Recorded above in S3/S4.
- `risk` — 2–3× token cost on small tasks; mitigated by the mandatory
  routing gate, recorded in §3.
- `infeasible` — none found.
- `spec-gap` — none open after this section.

## 6. Next steps

1. Adopt Stage 0 as cards (the four skills + the card convention).
2. Pick ≥5 real multi-step trial problems (ComplexityScore-gated).
3. Apply the kill rule at Stage 1; Stage 2 only on signal.

## 7. Prior art and provenance

- github/spec-kit 1.0 — MIT, Python CLI, processes above
  (github.com/github/spec-kit; docs at github.github.io/spec-kit).
  Borrowed: the specify/plan split, converge verdict discipline,
  assess go/kill states, edit-artifacts-not-rerun-stages,
  dogfooding. Not ported: its CLI, extension/preset/bundle/catalog
  machinery, `.specify/` layout.
- In-repo evidence for the pattern: R1 review caught a 5-branch
  non-composition and 5/23 acceptance failures pre-merge; R2-P2 caught
  F1/F2 audit forgery pre-ship; Phase B demonstrated the value of
  pre-registering the measurement before building.
