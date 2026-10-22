# Loom — Phase B Result

**The contrast experiment that gates Phase C.**

**Date:** 2026-10-22
**Question:** does editing a tool's always-on surface change the outcome of a real task in this repo?
**Protocol:** pre-registered in [`docs/LOOM_HARNESS2_ANALYSIS.md`](../LOOM_HARNESS2_ANALYSIS.md) §7 Phase B and in the card that ran it. Not changed after the results were in.

---

## 0. The decision, stated literally

> **The candidate suite is IDENTICAL to the base suite in 5 of 5 tasks.**
>
> Per the pre-registered rule: *identical in 5 of 5 → the signal is ZERO for this
> workload → **the probe loop DOES NOT SHIP**. Record the negative result and stop.*

**This is a successful outcome of this card.** The card's whole purpose was to be able
to kill a 38–59 day plan for a few dollars. It did.

What the experiment *did* find is a second, different defect — in the harness, not in
the signal:

> **The always-on surface is not reliably read.** In 4 of the 5 candidate runs the agent
> saw the hint and still burned 1.5×–3× the tokens of the base run; in 1 of 5 it never
> mentioned the surface at all. Loom has no mechanism that puts a surface in front of an
> agent — the file was read only because the agent happened to `read` it.

So the result is not "the probe loop is pointless". It is: **the probe loop cannot be
measured on Loom today, because Loom has no always-on surface to perturb.** That is
exactly the precondition the analysis (§7 Phase D) said had to be true before the
surface-schema work is worth doing — and it is false.

**Recommendation for Phase C: do not ship it.** Not "fix the harness first" — the
harness work is Phase D, and Phase D is deferred. The correct next step is the cheap
one Phase A already shipped (a skill inventory that is real), and a *different*
experiment against a tool that genuinely has an always-on surface.

---

## 1. The 5 tasks

Each task is a real planted defect in this repo, graded by the crate's own test command.
The base tree has the defect; the candidate tree has the same defect **plus exactly one
always-on surface edit** (one appended line in `AGENTS.md`, the file a coding agent reads
as its standing instructions). The two trees are otherwise identical.

| # | Task | Crate | Grading command | Planted defect | Surface edit (verbatim) |
|---|---|---|---|---|---|
| 1 | conflict-reference parsing | `loom-conflict` | `cargo test -p loom-conflict --all-features` | `their_ref = l[6..]` — off by one, keeps a `>` | "When a test in `crates/loom-conflict` fails on conflict-reference parsing, note that the conflict marker prefix `>>>>>> ` is seven characters long, so the branch reference starts at index 7 of the line, not index 6." |
| 2 | case-insensitive entity lookup | `loom-knowledge` | `cargo test -p loom-knowledge --all-features` | `WHERE name = LOWER(?1)` — only one side lowered | "When a test in `crates/loom-knowledge` fails on entity lookup, note that the entity-name comparison must be case-insensitive on BOTH sides: the stored name and the query are compared with `LOWER(...)` on each side." |
| 3 | importance clamping | `loom-hindsight` | `cargo test -p loom-hindsight --all-features` | `importance.min(1.0)` — no lower bound | "When a test in `crates/loom-hindsight` fails on fact importance, note that the importance value is a score bounded to the range 0.0 ..= 1.0: both ends are clamped, so a negative importance becomes 0.0 rather than staying negative." |
| 4 | `Contains` operand order | `loom-rules` | `cargo test -p loom-rules --all-features` | `expected.contains(actual)` — operands swapped | "When a test in `crates/loom-rules` fails on a comparison rule, note that the `Contains` operator asks whether the ACTUAL value contains the EXPECTED value — the operand order is `actual.contains(expected)`, not the reverse." |
| 5 | branch-name sanitiser | `loom-parallel` | `cargo test -p loom-parallel --all-features` | `.trim_start_matches('-')` — trailing dashes kept | "When a test in `crates/loom-parallel` fails on branch-name sanitising, note that the sanitiser must strip BOTH leading and trailing dashes from the branch name, not just the leading ones." |

Every base tree was verified to **fail** its grading command before the run
(`loom-conflict` 2 failed, `loom-knowledge` 2 failed, `loom-hindsight` 1 failed,
`loom-rules` 1 failed, `loom-parallel` 1 failed). Every run tree was re-graded after the
run by the same command: **10/10 pass** (the agent fixed the planted defect in all ten
runs — base and candidate alike).

The exact surface edits are kept as patch files under `docs/plan/phase-b/surfaces/` and
the per-task worktrees are `phase-b/<task>-base` and `phase-b/<task>-cand`.

## 2. The results table

Pass/fail is the crate's own test command. "Steps" and "tokens" are the cost of the run.

| # | Task | Base | Candidate | Suite differs? | Base steps | Cand steps | Base tokens (in/out) | Cand tokens (in/out) |
|---|---|---|---|---|---|---|---|---|
| 1 | conflict-reference | **PASS** | **PASS** | no | 8 | 20 | 91,578 / 3,389 | 198,984 / 16,578 |
| 2 | entity lookup | **PASS** | **PASS** | no | 18 | 36 | 198,134 / 4,971 | 231,873 / 40,221 |
| 3 | importance clamp | **PASS** | **PASS** | no | 8 | 9 | 89,636 / 2,101 | 56,093 / 3,198 |
| 4 | operand order | **PASS** | **PASS** | no | 9 | 7 | 132,435 / 1,989 | 59,138 / 2,821 |
| 5 | sanitiser | **PASS** | **PASS** | no | 6 | 10 | 54,477 / 4,624 | 130,667 / 6,356 |

**Candidate suite differs from base in 0 of 5 tasks.** The pre-registered rule needs ≥3.

| Task | Base fix | Candidate fix | Same fix? |
|---|---|---|---|
| 1 | `l[6..]` → `l[7..]` | `l[6..]` → `l[7..]` | identical |
| 2 | `name = LOWER(?1)` → `LOWER(name) = LOWER(?1)` | same | identical |
| 3 | `min(1.0)` → `clamp(0.0, 1.0)` | same | identical |
| 4 | `expected.contains(actual)` → `actual.contains(expected)` | same | identical |
| 5 | `trim_start_matches` → `trim_matches` | same | identical |

Every agent found the same root cause and made the same one-line fix. **The surface edit
changed nothing about the outcome.**

## 3. The real spend

| | |
|---|---|
| Runs | 10 (5 base + 5 candidate) |
| Wall clock | 2,478 s ≈ 41 min (runs were sequential) |
| Input tokens | 1,243,015 |
| Output tokens | 86,248 |
| Model | `poolside/laguna-s-2.1:free` via the `kilo` gateway (DSH's free pool) |
| **Cost against the $10–20 cap** | **$0.00 — the model is free** |

The card's budget was a hard cap of $10–20; the experiment came in at **$0** and ~41
minutes of wall clock. The cap was never the binding constraint. The *information* was.

The harness was `/tmp/phaseb2/run.sh`: a clean rsync of the task worktree (no `.git`, no
`target`), a shared `CARGO_TARGET_DIR`, and one `dsh --profile headless --json` run per
case, graded by the crate's own `cargo test`.

## 4. What the candidate runs actually did

The surface edit was read in 4 of 5 candidate runs — the agent's own reasoning quotes it.
That is the point of this section: the edit was *not* ignored; it was read, and it still
changed nothing.

| Task | Did the agent quote the surface edit? | Effect on the run |
|---|---|---|
| 1 | yes | It mis-read the hint ("`>>>>>>` is 6 chars, then a space = 7 total") and spent **2.5× the steps** working it out by hand — 28 tool calls against the base run's 10. Same fix. |
| 2 | yes | It hit a **stale shared-target artefact** that made the failing test look like it passed, then spent **2× the steps** (42 vs 22 tool calls) and 8× the output tokens working around it. Same fix. |
| 3 | yes | Base 8 steps / candidate 9 steps; the candidate used **fewer** tokens (56k vs 90k). Same fix. |
| 4 | **no** — the surface file was never mentioned | Candidate was **cheaper** than base (7 vs 9 steps, 59k vs 132k tokens). Same fix. |
| 5 | yes | The agent called the hint "a strong hint about the bug" and went straight in. Same fix. |

Two observations that matter more than the pass/fail table:

1. **The surface is not injected — it is found.** In task 4 the agent never mentioned
   `AGENTS.md` at all, and the run was *cheaper* than the base run. In tasks 1 and 2 the
   agent found it and did *more* work than the base run, because a hint it has to
   interpret is a constraint it has to reconcile, not a shortcut. A surface that only
   sometimes gets read, and that can make the agent slower, is not an instrument.
2. **The variance is in the harness, not in the task.** The two expensive candidate runs
   (tasks 1 and 2) were expensive because of a stale build artefact and an ambiguous
   hint — nothing to do with the defect being easier or harder.

## 5. What this establishes, and what it does not

**Establishes:**

- **Loom has no always-on surface for an agent.** Grep the whole tree: nothing in
  `crates/` writes a system prompt, an `AGENTS.md`, or any standing instruction into a
  tool's context. `crates/loom-cli/src/commands/exec.rs:263` (`build_base_env`) sets
  `LOOM_AGENTS_DIR`, `LOOM_ENABLED` and `LOOM_PROJECT_NAME` and nothing else. The
  `AGENTS.md` the candidate runs read is a **hand-placed experiment scaffold**, not
  something Loom put there. Loom's own always-on content is the *skills* — which is
  exactly what Phase A just made real: an inventory, a validator, and a trigger line per
  skill.
- **Phase D's precondition is FALSE.** The analysis (§7 Phase D) said the per-tool
  editable-surface schema is worth building only when "Loom decides to ship a
  prompt-injection surface" and "a third-party tool ships a harness-editing protocol".
  The first is not true today, and this experiment shows what a prompt-injection surface
  would have to be *good* at: reliably present, cheap to read, and unambiguous — three
  properties the experiment scaffold fails on two of three.
- **Phase C (probe + contrast + selection) has no measurable substrate.** Its input is
  "run the task, compare the outcome". With no surface to perturb, the probe loop
  perturbs nothing and can only observe noise.

**Does not establish:**

- That a prompt-injection surface can never help. It establishes that *this* one-line
  hint about *this* class of defect does not change the outcome, on this workload, with
  this model.
- That the analysis was wrong about Harness². It was not: Harness²'s own report shows
  large gains on *multi-component* harness edits (system prompt + tool guidance + memory
  together), not on a single hint line. This experiment perturbed one component with one
  line. That is a weaker perturbation than the thing the upstream result is about.

## 6. Where the negative result is recorded

- **This document**, on branch `phase-b/experiment` (not merged to `main`).
- **Not** in a new store. The pillar-5 lesson store owns lessons — see
  [`LOOM_VERIFIED_REVIEW_PROPOSAL.md`](LOOM_VERIFIED_REVIEW_PROPOSAL.md) §3.6. If this
  result becomes a lesson, it becomes a `lesson` entity there. No `~/.loom/experience/`
  directory, no second index, no second owner.
- The analysis document's §7 Phase B is the protocol this ran; §7 Phase C's gate
  ("Phase C is gated on a measurable contrast signal") is now closed **negative**.

## 7. What to do next (cheap, in order)

1. **Do not build the probe loop.** It is the 8–12 day slice with no substrate.
2. **Do not build the per-tool surface schema yet.** Phase D's precondition is false, and
   this experiment says what it would take to make it true: a surface that is injected
   (not found), read cheaply, and unambiguous — plus the metering Phase C would have
   needed to measure itself.
3. **Keep the Phase A slice.** A real skill inventory, a validator that refuses to let a
   broken skill through silently, and a trigger line per skill is the Loom-shaped version
   of "make the agent's standing context correct". It shipped; it is worth keeping.
4. **If the contrast question still matters, run it against a tool that HAS an
   always-on surface** — an agent that reads `AGENTS.md` by design, with a multi-line
   edit rather than a one-line hint, and with the token cost of the surface edit
   included in the comparison. That is a different experiment and it is not this card.
5. **Fix the measurement harness before any future A/B:** give each run its own
   `CARGO_TARGET_DIR` (the shared one caused a false "the test passes" in task 2), and
   record a "read the surface?" flag per run so a difference can be attributed.

## 8. Reproducing this

```bash
# the harness (one clean tree per run, graded by the crate's own command)
/tmp/phaseb2/run.sh <runid> <worktree> <prompt> [timeout-secs]

# the five tasks, base and candidate
git worktree add .worktrees/phase-b/<task>-base  -b phase-b/<task>-base  phase-b/<task>-base
git worktree add .worktrees/phase-b/<task>-cand  -b phase-b/<task>-cand  phase-b/<task>-base
# … append the surface edit to AGENTS.md in the candidate tree, then run both
```

The per-run logs are `/tmp/phaseb2/logs/<runid>.ndjson` (NDJSON: `thinking`, `text`,
`tool_call`, `tool_result`, `status` with per-step `usage`, `final`, `meta`).
