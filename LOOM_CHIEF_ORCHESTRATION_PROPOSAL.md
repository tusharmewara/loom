# Loom Chief Orchestration — Design Proposal

**Status:** Proposal (for review)
**Date:** 2026-09-13
**Related:** [`LOOM_HARNESS_COMPARISON_ENHANCEMENT.md`](./LOOM_HARNESS_COMPARISON_ENHANCEMENT.md)
**Feature area:** `loom-workflow`, `loom-parallel`, `loom-routing`, `loom-adapters`, `loom-cli`

---

## 1. Vision

> **"Loom asks a Chief agent to decompose an ask into small, self-contained tasks,
> then distributes those tasks across the remaining agents/harnesses, each running
> in its own isolated git worktree."**

Concretely, one invocation drives the whole loop:

```
loom orchestrate "add dark mode to the settings pane" --worktrees
```

1. **Chief phase** — Loom selects one harness (defaulting to the best planner
   available, e.g. `codex` or `claude-code`) and starts it in *planner-only* mode.
   Its sole output is a structured **plan** (JSON contract below): a goal plus a
   list of small tasks with explicit dependency edges, an agent-tier hint, and
   per-task acceptance criteria.
2. **Import phase** — the plan is validated and imported into `loom-workflow`
   machinery (one `loom_core::Task` per plan task, phases driven by the DAG).
3. **Fan-out phase** — ready tasks (all dependencies complete) execute
   concurrently: Loom creates a dedicated worktree per task (base ref pinned),
   assigns the next available agent/harness, injects context (plan summary +
   prior results + knowledge graph), and runs it with a completion gate.
4. **Verify + merge phase** — per-task acceptance gates run (hooks/composer rules),
   conflicts between worktrees are detected with the existing conflict resolution
   protocol, and completed work is merged back to the base branch (or opened as a PR).

---

## 2. Existing foundations (already ~70% there)

| Capability | Where in Loom today |
|---|---|
| Worktree per agent, base-ref pinned, cleanup | `WorktreeManager::create_worktree(agent)` — `crates/loom-parallel/src/worktree.rs:63` |
| Parallel agent execution with concurrency cap | `AgentPool::new(max_concurrent)` + `execute_all(tasks) → Vec<AgentResult>` — `crates/loom-parallel/src/pool.rs:87` |
| Task model, phases, manifests, provider assignment | `loom_core::Task`, `TaskPhase {Plan, Implement, Verify, Finish}`, `TaskManager` — `crates/loom-workflow/src/` |
| Per-phase sub-agent mapping (Continuations/harness choice) | `loom_core::SubAgentConfig { plan, implement, verify, finish }` — `crates/loom-core/src/types.rs:635` |
| Model-tier routing (Slm/Mid/Flagship → model) | `loom-routing` (`catalog.rs`, `router.rs`, complexity scoring) |
| Ask intake + dispatch CLI | `loom workflow create`, `loom workflow dispatch <id> --agent` — `crates/loom-cli/src/commands/workflow.rs` |
| Cross-tool context (prior phase result) | `LOOM_PRIOR_PHASE_RESULT` injection, `context_injector.rs`, sessions, knowledge graph |
| Completion gates | `loom-hooks` (`PostPhase` fires with `LOOM_EXIT_CODE`), `loom-rules`/composer |
| Cross-worktree conflict detection | Conflict resolution protocol (scan/resolve/bulk) — v0.7.0 |

## 3. Target pipeline

```
ask
 │  loom orchestrate --worktrees
 ▼
[1 Chief]   select planner harness → spawn planner-only
 │            │  structured output (plan.json)
 ▼            ▼
[2 Import]  validate plan → build TaskDag → import into loom-workflow
 │              \
 ▼               └→ assistant TaskDag (depends_on edges)
[3 Schedule] level-order batches ● + ● ready → fan out
 │
 ▼
[4 Fan-out]  for each ready task:
 │             WorktreeManager.create_worktree(task)
 │             harness ← HarnessSelector (worker pool, by tier)
 │             AgentPool.execute_all(AgentTask{work_dir: worktree, env: routing})
 │             gate on acceptance criteria (hooks/rules)
 ▼
[5 Merge]    conflict scan across worktrees → resolve → merge or PR
```

## 4. New components (the thin layer)

### 4.1 Plan contract (JSON, schema-validated)

```json
{
  "goal": "Add dark mode to settings pane",
  "tasks": [
    {
      "id": "t1",
      "title": "Add theme design tokens",
      "description": "Define --bg/--fg CSS vars; do not touch UI code.",
      "depends_on": [],
      "agent": "goose",
      "acceptance": ["cargo test -p loom-ui tokens"]
    },
    {
      "id": "t2",
      "title": "Wire toggle into settings",
      "description": "Use tokens from t1; add SettingsDarkModeToggle component.",
      "depends_on": ["t1"],
      "agent": "jcode",
      "acceptance": ["cargo build"]
    }
  ]
}
```

Validation rules:
- `id`s must be unique and `[a-zA-Z0-9_-]+`.
- `depends_on` entries must reference existing task ids (else error).
- `depends_on` must be acyclic (cycle → error with the cycle path).
- `agent` may be omitted → assigned from routing tier at dispatch time.

### 4.2 New `chief` module (`crates/loom-workflow/src/chief/`)

Pure-logic core, test-first (see TDD plan):

- `plan.rs` — `PlanTask`, `TaskPlan`, `PlanParser`. Parse/validate/serialize the contract.
- `dag.rs` — `TaskDag::from_plan(&TaskPlan)`; `execution_batches() -> Vec<Vec<String>>`
  level-order batching so a batch runs only after all its dependencies' batches.
  Cycle detection.
- `harness.rs` — `HarnessRole { Chief, Worker }`, `HarnessSelector`. Picks the chief
  harness from a preference list plus the set of installed harnesses; assigns workers.
- `scheduler.rs` — `DispatchEntry { task_id, harness, worktree_path }`;
  `build_dispatch(dag, selection, worktree_for_task) -> Vec<Vec<DispatchEntry>>`.
  Binds ready tasks to harness + worktree into fan-out batches.
- `manifest.rs` — `WorktreeEntry { task_id, branch, worktree_path, base_ref, created_at }`,
  `WorktreeManifest` add/save/load. Persists per-run worktree maps so a restart can
  resume or clean up.

### 4.3 Bridge to execution (task 2 of roadmap, needs `loom-parallel` dep on `loom-workflow`)

- `loom orchestrate <ask> [--chief codex] [--base main] [--merge pr]` CLI command.
- Chief invocation via existing adapter machinery in *planner mode* (env/arg override),
  e.g. `codex exec "produce a plan only; output JSON per schema"` and `claude-code --output-format json`-style flags for harnesses that support headless output.
- Dispatch: per batch, `AgentPool::execute_all` with `AgentTask{ work_dir: worktree }`.
- Completion gates via `PostPhase` hooks (`LOOM_TASK_ID`, `LOOM_AGENT`, `LOOM_EXIT_CODE`).
- Post-run `loom conflict scan` already exists for the merge step.

### 4.4 Harness-level routing (extends `loom-routing`)

Routing today picks a *model* per tier. Add a harness layer:

```
tier(Slm|Mid|Flagship) → preferred harness ranking → installed adapters → harness
```

- Chief defaults to the top-ranked Flagship planner (`codex` → `claude-code` → ...).
- Workers default to the cheapest available (Slm/Mid) harness to control cost.

## 5. Design decisions & tradeoffs

1. **Chief is planner-only.** Same adapter machinery, but invoked with a
   planner/system prompt override and its output parsed against the schema.
   Parse failure ⇒ surface the raw output and allow one re-ask before failing.
2. **Structured output is the contract.** Independence is only as good as the
   decomposition. Per-task `acceptance` + `description` scoping ("do not touch X")
   is what makes worktrees compose safely.
3. **DAG over flat fan-out.** `AgentPool.execute_all` is flat today; the scheduler
   adds `depends_on` batching on top, keeping `AgentPool` untouched.
4. **Worktree manifests are persisted** (`~/.loom/workflow/manifests/`), not
   ephemeral, so interrupted runs can be probed/resumed and stale worktrees cleaned.
5. **Merge is manual-first.** Auto-merge only for clean, gated branches; otherwise
   surface via PR/diff review. Conflict scan runs regardless.

## 6. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Two worktrees claim overlapping files | `depends_on` edges + per-task file contracts in `description` + base-ref pinning + conflict scan/resolve |
| Nondeterministic plan output from LLM harness | Schema validation, one re-ask, fall back to Slm-class chief for cheap retries |
| Cost = chief + N agents | Tier-based harness selection, worker count cap, budget routing |
| Chief "wanders" into implementation | Planner-only prompt override; gate on structured output |
| Restart / crash mid-fan-out | Persisted `WorktreeManifest` + task phase state for resume |

## 7. Roadmap

**P0 — De-risking vertical slice (proves the loop, pure logic first):**
1. `chief` module (plan/dag/harness/scheduler/manifest) with full TDD suite — RED tests land with this proposal.
2. `loom workflow create --from-plan <plan.json>` — imports a validated plan as tasks.
3. `loom orchestrate` end-to-end with worktrees + AgentPool + post-run conflict scan on a 2-task demo.

**P1 — Hardening:**
4. Harness-level routing in `loom-routing` (chief/worker selection by tier).
5. Completion-gate wiring via PostPhase hooks + acceptance command runner.
6. Resume/cleanup of interrupted runs from persisted manifests.

**P2 — Productization:**
7. PR/diff review modal + merge automation.
8. Remote/UHP dispatch (aligns with HarnessRouter `UHP` gap in the comparison doc).

## 8. Non-goals (for now)

- Cross-host/SSH execution (P2+).
- Auto-selecting the chief agent from raw ask content (future; routing could score this).
- Replacing `sub_agents`/`SubAgentConfig` — the chief layer composes *on top* of it.