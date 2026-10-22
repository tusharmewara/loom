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

The surface edit was genuinely in play. The harness (`dsh` headless) auto-loads the
workspace `AGENTS.md` into the agent's standing context (verified separately: a child
agent answered a token planted in `AGENTS.md` without any file read), and in **all five**
candidate runs the agent quoted or grepped the appended hint line. The edit was read
in 5 of 5 — and it still changed no outcome.

What the experiment *did* surface is a defect in the measurement harness, not in the
signal:

> **A shared `CARGO_TARGET_DIR` across run trees is untrustworthy.** Stale build
> artefacts made planted failures look like passes mid-run (the t1, t2 and t4
> candidate agents each hit this and worked around it), and later produced a false
> FAIL in the experiment's own re-grade until a forced-recompile re-grade was run.
> Any future A/B on this repo must give each run its own target directory.

**Recommendation for Phase C: do not ship it.** Not "fix the harness first" — all 5
tasks were reproducible (10/10 runs completed and were graded), so the pre-registered
third row does not apply; the result is the second row, zero signal. The correct next
step is the cheap one Phase A already shipped (a skill inventory that is real), and a
*different* experiment against a different workload if the contrast question ever
matters again.

---

## 1. The 5 tasks

Each task is a real planted defect in this repo, graded by the crate's own test command.
The base tree has the defect; the candidate tree has the same defect **plus exactly one
always-on surface edit** (one appended line in `AGENTS.md`, the standing instructions a
coding agent reads in this repo). The two trees are otherwise identical.

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

The exact planted defects are kept as patch files under
[`docs/plan/phase-b/plants/`](phase-b/plants/) and the exact surface edits under
[`docs/plan/phase-b/surfaces/`](phase-b/surfaces/). The per-task worktrees live at
`.worktrees/phase-b-t{1..5}-<crate>-{base,cand}` on branches
`phase-b/t{1..5}-<crate>-{base,cand}`.

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

**Independent verification.** After the runs, all ten trees were re-graded with a
forced recompile from each tree's own source (a first re-grade attempt had run a stale
test binary from the shared target — see §9). Result: **10/10 exit 0** —
`loom-conflict` 15/15, `loom-knowledge` 12/12, `loom-hindsight` 12/12, `loom-rules`
19/19 (+1 ignored doctest), `loom-parallel` 8/8 (+1 doc test), for both variants of
every task.

## 3. The real spend

| | |
|---|---|
| Runs | 10 (5 base + 5 candidate) |
| Wall clock | 2,478 s ≈ 41 min (runs were sequential) |
| Input tokens | 1,243,015 |
| Output tokens | 86,248 |
| Model | `poolside/laguna-s-2.1:free` via the `kilo` gateway (a free-tier model) |
| **Cost against the $10–20 cap** | **$0.00 — the model is free** |

The card's budget was a hard cap of $10–20; the experiment came in at **$0** and ~41
minutes of wall clock. The cap was never the binding constraint. The *information* was.

The harness was `/tmp/phaseb2/run.sh`: a clean rsync of the task worktree (no `.git`, no
`target`), a shared `CARGO_TARGET_DIR`, and one `dsh --profile headless --json` run per
case, graded by the crate's own `cargo test`.

## 4. What the candidate runs actually did

The hint was seen in **all five** candidate runs — the agent's own reasoning quotes it
in four and greps it out of `AGENTS.md` in the fifth. That is the point of this
section: the edit was *not* ignored; it was read, and it still changed nothing.

| Task | Did the agent see the surface edit? | Effect on the run |
|---|---|---|
| 1 | yes — quoted it | It also hit the stale-target artefact (tests looked green on the first run; a clean rebuild showed 2 failures), then used the hint to zero in: **2.5× the steps of base** (20 vs 8). Same fix. |
| 2 | yes — quoted it | It hit the same stale-target artefact and spent **2× the steps** (36 vs 18) and 8× the output tokens working around it. Same fix. |
| 3 | yes | Base 8 steps / candidate 9 steps; the candidate used **fewer** tokens (56k vs 90k). Same fix. |
| 4 | yes — grepped `AGENTS.md` and found the hint after diagnosing the failure | Candidate was **cheaper** than base (7 vs 9 steps, 59k vs 132k tokens). Same fix. |
| 5 | yes — called it "a strong hint about the bug" and went straight in | Same fix. |

Two observations that matter more than the pass/fail table:

1. **The surface edit was injected and read — and still changed nothing.** The harness
   loads `AGENTS.md` into the agent's standing context, so the hint was present from
   the first turn of every candidate run; the agents quote or grep it in 5 of 5. A hint
   that is reliably present, about the exact function under test, in the exact crate,
   with the correct fix implied — and the outcome is identical 5 of 5. On this workload
   the outcome is bounded by the task, not by the surface.
2. **The variance is in the harness, not in the task.** The two expensive candidate
   runs (tasks 1 and 2) were expensive because of a stale build artefact and hint
   interpretation — nothing to do with the defect being easier or harder. The same
   staleness later produced a false FAIL in the experiment's own re-grade (§9).

## 5. What this establishes, and what it does not

**Establishes:**

- **The contrast signal is zero on this workload.** Five single-line planted defects,
  five base runs that pass, five candidate runs with a read, on-target surface hint
  that also pass, with the same fix. If a one-line, answer-adjacent hint cannot move a
  single-line Rust bugfix, the probe loop that Phase C proposed (perturb, contrast,
  select) has nothing to select on here.
- **Loom the product has no always-on surface of its own.** Grep the whole tree: nothing
  in `crates/` writes a system prompt or standing instruction into a tool it
  dispatches. `crates/loom-cli/src/commands/exec.rs:263` (`build_base_env`) sets only
  `LOOM_AGENTS_DIR`, `LOOM_ENABLED` and `LOOM_PROJECT_NAME`; `resume.rs:33,72` are the
  only prompt-adjacent injections. The `AGENTS.md` the candidates read is the repo's
  agent guide, loaded by the *harness* (dsh) — not something Loom-the-product puts in
  front of the tools it dispatches. Loom's own always-on content is the *skills* —
  which is exactly what Phase A just made real: an inventory, a validator, and a
  trigger line per skill.
- **Phase D's precondition is FALSE.** The analysis (§7 Phase D) said the per-tool
  editable-surface schema is worth building only when "Loom decides to ship a
  prompt-injection surface" and "a third-party tool ships a harness-editing protocol".
  Neither is true today.
- **Phase C (probe + contrast + selection) has no measurable substrate on this
  workload.** Its input is "run the task, compare the outcome". With a zero contrast
  signal over five tasks, the probe loop perturbs nothing and can only observe noise.

**Does not establish:**

- That a prompt-injection surface can never help. It establishes that *this* one-line
  hint about *this* class of defect does not change the outcome, on this workload, with
  this free-tier model.
- That the analysis was wrong about Harness². It was not: Harness²'s own report shows
  large gains on *multi-component* harness edits (system prompt + tool guidance + memory
  together), not on a single hint line. This experiment perturbed one component with
  one line. That is a weaker perturbation than the thing the upstream result is about.

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
2. **Do not build the per-tool surface schema yet.** Phase D's precondition is false.
3. **Keep the Phase A slice.** A real skill inventory, a validator that refuses to let a
   broken skill through silently, and a trigger line per skill is the Loom-shaped version
   of "make the agent's standing context correct". It shipped; it is worth keeping.
4. **If the contrast question still matters, run it against a different workload** —
   multi-step tasks where outcome variance is plausible (feature work, refactors,
   multi-crate changes), a multi-line surface edit rather than a one-line hint, a
   stronger model than the free tier, and with the token cost of the surface edit
   included in the comparison. That is a different experiment and it is not this card.
5. **Fix the measurement harness before any future A/B:** give each run its own
   `CARGO_TARGET_DIR` (the shared one caused false "the test passes" mid-run and a
   false FAIL in the re-grade), and record a "read the surface?" flag per run so a
   difference can be attributed.

## 8. Reproducing this

The durable record is the patches; the run trees and logs under `/tmp/phaseb2/` are
ephemeral.

```bash
# Base tree for task N:
git worktree add .worktrees/phase-b-tN-<crate>-base -b phase-b/tN-<crate>-base main
git -C .worktrees/phase-b-tN-<crate>-base apply docs/plan/phase-b/plants/tN-<crate>.patch
git -C .worktrees/phase-b-tN-<crate>-base commit -m "chore: plant defect tN"

# Candidate tree: same, plus the one-line surface edit:
git -C .worktrees/phase-b-tN-<crate>-cand apply docs/plan/phase-b/surfaces/tN-<crate>.md

# Run one case (clean tree per run, per-run CARGO_TARGET_DIR, dsh headless):
rsync -a --exclude .git --exclude target <worktree>/ /tmp/run-tree/
cd /tmp/run-tree && CARGO_TARGET_DIR=/tmp/target-<runid> \
  dsh --profile headless --json - < prompt.txt
# Grade with the crate's own command, in the run tree, same CARGO_TARGET_DIR:
CARGO_TARGET_DIR=/tmp/target-<runid> cargo test -p <crate> --all-features
```

The per-run logs are NDJSON: `session`, `status` (`step_start`/`step_end` with
`usage{inputTokens,outputTokens,totalTokens,cacheReadTokens}`, `turn_end`), `thinking`,
`text`, `tool_call`, `tool_result`, `final`, `meta{rc,secs}`.

## 9. Execution notes (what went wrong, and its resolution)

Recorded so the record is honest about the experiment's own error bar:

1. **An aborted second generation contaminated the repo, and was reverted.** After the
   ten runs above completed and the first draft of this document was written, a second
   experiment generation (a different t1 plant, a generic "process rule" surface line)
   was started and abandoned midway. Its scaffolding leaked: two unpushed commits landed
   on `main` (a plant commit and an `AGENTS.md` append), and three base worktrees got
   uncommitted `AGENTS.md` edits. Nothing was ever pushed. `main` was reset to the last
   legitimate commit (24ec028); all five base worktrees were restored to pristine
   (`AGENTS.md` md5 d2792fba7cc0f788fc5eff0961d32727, byte-identical to 6787722). The ten
   run trees used for the results above were **not** part of the contamination: all ten
   were diffed against `AGENTS.md` at 6787722 — base trees +0 lines, candidate trees
   +1 line (the hint only).
2. **The first re-grade ran a stale test binary.** The trees were graded with the same
   shared `CARGO_TARGET_DIR` the runs used; because `rsync -a` preserves source mtimes,
   cargo considered the tree sources older than previously built artefacts and ran a
   binary compiled from a *different* source state — the clean t1 trees graded as
   "9 passed; 6 failed" (the signature of the second generation's abandoned plant).
   Touching each tree's planted file to force a recompile from the tree's own source
   yielded the true result: 10/10 exit 0 (§2). This is the same staleness the t1/t2/t4
   candidate agents hit mid-run — the lesson is §7.5.
3. **The run trees and logs live under `/tmp/phaseb2/`** (`run.sh`, `prompt-t{1..5}.txt`,
   `candidates/t{1..5}*.md`, `logs/*.ndjson`, `trees/`) and will not survive a reboot.
   The patches under `docs/plan/phase-b/` and the numbers in this document are the
   durable record.
