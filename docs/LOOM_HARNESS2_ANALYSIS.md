# Harness² (google-research/harness2) → Loom: category analysis, Rust-native design, implementation proposal

> Status: analysis + proposal, 2026-10-20. Sourced from `github.com/google-research/harness2` v0.1.0 (Apache-2.0, Google LLC) at commit `1cfe254`, compared against the Loom tree at v0.9.6 (31 crates, MIT).
> Constraint applied throughout: **Rust only — no Python runtime, no `harness2` dependency.**
> Related Loom docs: [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) (pillar 5 — the *owner* of the lesson store this document defers to), [plan/LOOM_HARNESS_COMPARISON_ENHANCEMENT.md](plan/LOOM_HARNESS_COMPARISON_ENHANCEMENT.md) (orchestration-layer competitors — a *different* category from Harness²), [competitive-analysis.md](competitive-analysis.md) ("What NOT to Build" — Harness² is listed there as off-path as a dependency), [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md) (the one slice this document does recommend building).

---

## 1. Executive summary

Harness² is **not** a competitor to Loom and **not** a dependency candidate. It is a *method* for optimizing the scaffolding around a model, sitting one level below both:

| | Harness² | Loom |
|---|---|---|
| Category | method (a research loop) | infrastructure layer (a product) |
| Unit of work | one harness (prompt/tool scaffolding around a model) | one tool ecosystem (skills, MCP, sessions, auth, hooks, workflows) |
| Loop | Propose → Probe → Compose, recursively | none — Loom *drives* tools, it does not improve their scaffolding |
| Runtime | Python 3.12 + uv, Linux, Vertex AI | Rust, macOS/Linux, any provider |
| Surfaces | 2 substrates (OpenCode, Codex) | 12 preset adapters + generic TOML adapter |

**Loom already owns most of what Harness² edits** — skills, plugins, hooks, subagents, memory — but it never *edits* them, and it has no surface for the two prompt-shaped components (system prompt, tool guidance) at all. Loom distributes shared resources across tools; it does not optimize a single tool's context for a task and prove the optimization worked.

Three things Harness² has and Loom does not:

1. **A declarative surface schema** (`SurfaceSpec`) — per-tool description of which files are editable, their char caps, and their validation rules. Loom's adapter trait knows only *where* skills and MCP config live (`crates/loom-adapters/src/lib.rs:36`), never *what may be edited*.
2. **A contrastive probe loop** — anonymized candidate selection + behavioral comparison between executions, with no ground-truth grader. Loom has no analogue (`grep` for `probe`/`contrast`/`anonym` in `crates/`: no hits outside an unrelated config probe).
3. **An experience library** — per-domain store of *adopted* harness trees plus `cue`/`lesson`/`evidence` records. Loom has the substrate (`loom-hindsight`, `loom-knowledge`) but not the shape, and critically not the rule that the **full adopted tree is carried, never a summary**.

**Recommendation: adopt mechanisms, not the system — and adopt them in the order that fixes a real bug.** Do not port Harness² wholesale.

The review that follows §6 changed the ordering. The obvious plan ("schema first, then library, then probe loop") builds a **store with no producer and a schema with no author**: upstream's experience library has exactly one writer, and it is the selection loop (§2.4), so the library is Phase-C output, not Phase-B input; and Loom has no always-on prompt surface to describe, so `SurfaceSpec` describes nothing today.

What is actually worth doing is smaller and more urgent:

1. **A skill inventory that works, plus a validator and a trigger generator** (3–4 days, `loom-skills`). Two separate defects, found by a later review (§12). *A0 — the inventory is off:* `loom skills list` returns `[]`, `loom skills show` finds nothing, and `loom lock --generate` writes zero entries, while 11 skills are installed on this machine — the registry (`~/.loom/skills/skills.db`) is **empty**, because the only code that writes it is the MCP import path, not the skills path. *A1 — the validator:* Loom can install a skill with no `description:` frontmatter, sync it into every native tool directory, and report success — while the agent silently ignores it. Upstream treats exactly this as a hard error. A0 is what a user notices; A1 is what prevents the next silent drop.
2. **A pre-registered contrast experiment** (2–3 days, $10–20) that measures whether editing a surface changes a test outcome at all. It gates the expensive part; if the signal is zero, the probe loop does not ship and the cost was $20.
3. **Only then**, the probe loop, the experience library, and the surface schema — the last as a Phase-D option with three named preconditions.

The re-scoped plan is §7; the cut list is §7.1; the Rust-native design sketch is §6; the strategic direction (a self-evolving *shared* harness, and the one decision upstream does not make) is §11; the first-principles review that reshaped Phase A — and the live defects it found — is §12.

---

## 2. What Harness² actually is

Read from source (`harness2/core/*.py`, `harness2/substrate/*.py` — **6,680 lines**; the whole `harness2` package is 12,747 lines excluding `third_party/`, of which 229 lines are tests and 4,170 lines are benchmark adapters).

### 2.1 The eight editable components

Harness² exposes eight editable components around a model: **system prompt, guardrails, memory, tool guidance, subagents, skills, scripts, plugins**. They control context construction, the tool interface, delegation, and behaviour inside the tool loop.

Each is expressed as files in a *harness tree*, materialised into a substrate-native layout:

| Component | OpenCode (`OPENCODE_SURFACE`) | Codex |
|---|---|---|
| system prompt | `systemprompt.md` | `AGENTS.md` (merged with c3/c4/c6) |
| guardrails | `guardrails.md` | `AGENTS.md` |
| memory | `LongTermMEMORY.md` | `AGENTS.md` |
| tool guidance | `tool_descriptions/tool_guidance.md` | `AGENTS.md` |
| skills | `skills/<name>/SKILL.md` | `.agents/skills/<name>/SKILL.md` |
| subagents | `sub_agents/<name>/AGENT.md` | *not staged* (Codex deployments forbid subagents — see note) |
| scripts | `scripts/<name>.py` | `scripts/<name>.py` (+ `.agents/skills/<name>/scripts/*` carried with a skill) |
| plugins | `plugin/<name>.ts` | `.codex/hooks.json` (lifecycle hooks) |

Codex collapses four always-on components into one file (`AGENTS.md`), so the "always-on" budget is a single file's cap — the same file, once per surface. Component ids in the upstream tables run `c1, c3, c4, c5, c6, c7, c8` (there is no `c2`).

**Subagents are OpenCode-only in practice.** The Codex staging root is `("AGENTS.md", ".agents", "scripts", ".codex")` (`harness2/substrate/codex.py:106`) — `sub_agents/` is not staged — and the benchmark adapters disable subagents for every non-OpenCode substrate: `allow_subagents=(substrate == "oc")` (`harness2/benchmarks/jobbench.py:519`, `harness2/benchmarks/workbuddy.py:664`), `allow_subagents=False` (`harness2/benchmarks/lab.py:491`). The validator enforces it as a hard error (`harness2/core/tree.py:306-307`). So upstream's c5 component is, in the reported results, an OpenCode feature only.

Always-on budget differs per substrate: OpenCode caps always-on text at **24,000 chars** ("every token here competes with the agent's own output budget"); Codex at **8,000 chars** with `reject_empty_always_on=True`.

### 2.2 `SurfaceSpec` — the schema

`harness2/core/tree.py:40`:

```python
@dataclass(frozen=True)
class SurfaceSpec:
    name: str
    always_on: tuple[str, ...]
    always_on_max: int
    always_on_cap_reason: str
    skill_dir: str
    skill_name_rule: str
    trigger_hint: str
    autotrigger_file: str
    trigger_line: str
    subagent_requires_mode: bool
    plugin_dir: str | None
    hooks_file: str | None
    hook_events: frozenset[str] = frozenset()
    compile_skill_scripts: bool = False
    reject_empty_always_on: bool = False
    stubs: tuple[tuple[str, str], ...] = ()
```

It is a *declaration*, not code: it says which paths exist, how big always-on text may be, which validation rules apply (skill name must match directory, subagent frontmatter requires `mode: subagent`, hooks must be valid JSON, scripts must compile), and it enforces a path-escape guard (`normalize_rel` rejects `..` traversal; `is_allowed` is the single allow-list predicate). A tree that violates the surface is *dropped before its work is used*, however good it is.

### 2.3 Propose–Probe–Compose (task-level recursion)

`harness2/core/recursion.py`:

1. **Propose** — a proposer model, given the task, the base harness tree, and the editable-surface table, emits candidate edits. Each candidate becomes a full harness tree.
2. **Probe** — candidates are executed on *the same task* (base run + 2k candidate runs + k final runs; `R = 4k` rollouts, `ledger.py:26`).
3. **Compose** — a refined harness is composed from the *behavioural contrast* between executions. No ground-truth grader is required.

The selection step (`core/selection.py`) is the clever part and the part most worth stealing:

- Candidates are **anonymized** (`A..Z, AA..AZ, BA..`) so the selector cannot reason about provenance or file ordering.
- Assignment is **seeded and reproducible**: `assign_ids` sorts labels, then shuffles with `random.Random(sha1(f"{task_id}|{k}|{mode}")[:12])` — stable across reruns.
- The reply is parsed from a fenced `` ```choice `` block; `<why>`, `<experience>` and `<edits>` are extracted from the same reply.
- The **selection pool is exactly the k finals of that mode** (parallel: `agg1..aggk`, sequential: `c1..ck`) — the reference executions are *not* in the pool, so the selector cannot abstain by construction.
- `order_graded_first` moves candidates that a judge could grade to the front, but **grading happens after selection** (`core/judge.py`): `Scored.graded` means *the judge succeeded*, never *the run scored 0*; ungraded runs are excluded from the denominator rather than counted as zero.

### 2.4 Harnessing-level recursion (the experience library)

`harness2/core/library.py`:

```
<root>/<run-tag>/<mode>/<domain>/
├── INDEX.md                              # regenerated from disk, never hand-written
└── harnesses/<task>/
    ├── harness/<rel>                     # the FULL adopted tree — never a summary
    └── EXPERIENCE.md                     # cue / lesson / evidence (+ confidence, edits)
```

Rules that carry the value:

- `has_experience` requires **all three** of `cue`, `lesson`, `evidence` to be non-empty strings — an experience without evidence is not stored.
- The **full tree** is copied into the library. A summary loses the exact wording that made it work.
- Retrieval is **by construction**: a domain directory only ever contains entries for that domain, so cross-domain contamination is impossible.
- The library directory is handed to the improver session as context; if the domain is empty, the improver gets no library context at all (`context_dir` returns `None`).
- Grader verdicts are asserted absent from the stored tree (`assert_tree_has_no_grader_verdicts`) so no benchmark answer leaks into a reusable artifact.

**Upstream has exactly one writer for this library, and it is the selection loop.** `harness2/core/schedule.py:555`:

```python
if mode_lib and sel.ok and library_store.has_experience(sel.experience):
    mode_lib.add(...)
```

`sel.experience` is distilled from the **selector's** reply (`harness2/core/selection.py:454-505`), not from a human and not from a separate writer. The dependency direction upstream is therefore **selection loop → library**: any plan that builds the library before the selection loop builds a store with no producer.

### 2.5 Evidence-as-files

`harness2/core/context.py` builds a directory tree — `context/task.md`, `context/rollouts/<label>.md` (full trajectory, *no prompt truncation*), `context/deliverables/`, `context/harness/`, `context/library/INDEX.md`, `context/pool/` — and hands the proposer a **short pointer** (`MISSION.md`) rather than an inlined prompt. Full trajectories on disk; small prompt budget.

### 2.6 Cost model (the reason this is not a free feature)

`ledger.py`: `R = 4k` rollouts (k base + 2k candidates + k finals) and `P = 3k` improver sessions (k invariant + k domain + k composition). `BudgetError` is raised on identity mismatch. The proposer isolates each improver session in its own workspace (`SYSTEM_ARG_MAX = 120_000`), runs under a concurrency semaphore, drops `ANTHROPIC_*` env vars, and tracks cost.

**Upstream has no cost cap.** `Entry(kind, label, usd, cached, attempts)` (`ledger.py:40-46`) records spend, and `assert_identity` (`ledger.py:89-113`) checks only `R == 4k`, `P == 3k`, and duplicate labels — it never refuses a run for being too expensive. The hard per-run budget cap proposed in §7 is **Loom's own contribution**, not an upstream mechanism, and it belongs with pillar-4 metering (`ROADMAP.md:54`) regardless of Harness².

Reported results: a single Harness² step improves GLM-5 and Gemini 3.5 Flash over their base harnesses, with gains up to **11.5 points** on WorkBuddy-Bench, **4.5 points** on JobBench, and **11.8 pp** on LAB partial pass rate; more task-level budget → more gains.

**These numbers are benchmark-specific and Vertex-AI-specific.** They do not transfer to Loom's user base or budget.

---

## 3. Coverage map: Harness² → Loom

Legend: ✅ present and equivalent · 🟡 partial · ❌ absent.

| Harness² mechanism | Loom equivalent | Status | Evidence |
|---|---|---|---|
| Shared skills (c6) | `loom-skills` registry + `sync_skills_to_native` | ✅ | `crates/loom-skills/src/registry.rs:10`, `crates/loom-adapters/src/lib.rs:227` |
| Plugins (c8, code) | `PluginRegistry` — TOML manifests, Ed25519 | 🟡 | `crates/loom-core/src/plugin_registry.rs:72` — manifest registry, **not** in-loop plugins |
| Lifecycle hooks (c8) | `loom-hooks` — real execution, exit-code blocking, signing | ✅ | `crates/loom-hooks/src/lib.rs:64`, `crates/loom-hooks/src/executor.rs:87` |
| Subagents (c5) | `SubAgentConfig { plan, implement, verify, finish }` | 🟡 | `crates/loom-core/src/types.rs:769` — phase→agent mapping, no per-task subagent *files* |
| Memory (c3) | `loom-hindsight` (typed facts) + `loom-knowledge` (graph) | 🟡 | `crates/loom-hindsight/src/lib.rs:64,136,167,221` — a fact store, not an always-on memory file injected into every turn |
| Tool guidance (c4) | — | ❌ | no prompt-injection surface exists: `grep -rn "system_prompt\|AGENTS.md" crates/` → 0 hits |
| System prompt (c1) | — | ❌ | same |
| Guardrails (c6-always-on) | `loom-rules` event rules (different trigger: events, not tokens) | 🟡 | `crates/loom-rules/src/lib.rs:234,515` |
| Scripts (c7) | `packages/routing-hook/` | 🟡 | one Python script — the "Rust only" constraint of this document applies to *new* Loom code, so this row is 🟡 at best as a model for a Loom-native scripts surface. `packages/hooks/` holds only a `Cargo.toml` with no `src/` and is **not a workspace member** |
| `SurfaceSpec` | — | ❌ | `crates/loom-adapters/src/lib.rs:36` declares skills dir + MCP path only |
| Propose–Probe–Compose | — | ❌ | no `probe`/`contrast`/`anonym` analogue in `crates/` |
| Selection (anonymized, seeded) | — | ❌ | `crates/loom-routing` scores prompts; it does not select between candidates |
| Outcome signal | `RoutingOutcomeStore` — decision/completion/override records | 🟡 | `crates/loom-routing/src/outcomes.rs:35,101,137,155` — records outcomes, no rubric judge |
| Experience library (cue/lesson/evidence + full tree) | — | ❌ | `loom-hindsight` retains free-text facts; no cue/lesson/evidence contract, no tree retention |
| Evidence-as-files context | session exports + `loom sessions recall` | 🟡 | `crates/loom-sessions/src/exporter.rs:17`, `crates/loom-cli/src/commands/resume.rs:33` |
| Probe isolation | `WorktreeManager` + `AgentPool` | ✅ | `crates/loom-parallel/src/worktree.rs:63`, `crates/loom-parallel/src/pool.rs:87,102` |
| Budget accounting | `loom-telemetry` counters | 🟡 | `crates/loom-telemetry/src/lib.rs:73` — counters, no identity/rollout ledger |
| Local-first, own keys | yes (stronger) | ✅ | vault, ACL, hash-chained audit; harness2 needs Vertex AI |

**Also note:** `loom-workflow/src/chief/harness.rs` uses "harness" to mean *the agent binary* (`codex`, `claude-code`). Harness² uses "harness" to mean *the scaffolding around a model*. The two meanings collide; §6.1 resolves it.

---

## 4. What is genuinely missing, and why it is not a wrapper away

A first instinct is: "Loom already has skills, plugins, hooks — just run Harness² as a tool and capture its output." That does not work:

1. **Harness²'s substrates are OpenCode and Codex only.** Loom has 12 adapters. A Loom-native surface must describe *every* adapter, or the method only works for 2 of 12.
2. **Its loop is intrinsic, not a batch job.** The improvement is in *the harness tree*, and the tree is what Loom would have to own to observe the improvement. A black-box Harness² run produces logs; it produces no editable surface for Loom to ship.
3. **Its evidence is benchmark-graded.** Harness²'s numbers come with JobBench/LAB/WorkBuddy graders. Loom has no benchmark corpus and no rubric judge. The *method* is portable; the *measurement* is not.
4. **Cost.** `R = 4k`, `P = 3k` per task, per `k`. Loom's usage metering is a **pillar 4** (post-1.0) item. There is no metering layer to bound this.

The three missing pieces therefore need to be built *in* Loom, in Rust, on Loom's own substrate — which is exactly what §6 specifies.

---

## 5. Existence check (does each new surface need to exist?)

| Proposed surface | Existing owner / reuse candidate | Why the existing surface is insufficient | Creation proof | Decision |
|---|---|---|---|---|
| **Skill inventory + validator + trigger generator** | `loom-skills` (`loader.rs`, `registry.rs`) | `loader.rs:44-104` indexes flat `*.md` by size+mtime and has **zero callers**; `registry.rs` is the tree's declared source of truth (`crates/loom-discovery/src/lib.rs:128`) but is **empty** — the only writer is the MCP import path (`crates/loom-adapters/src/lib.rs:356`); `neutral_renderer.rs` is the tree's only frontmatter parser and it **does not compile** (§6.2.1) | three live defects: `loom skills list` → `[]` with 11 skills installed, `loom skills install <dir>` fails, and a missing `description:` is a silent drop (§6.2.1) | **build now** (Phase A) — one new module in `loom-skills`, plus deleting one dead file; no new crate, no new CLI group |
| Per-tool editable-surface schema | `loom-adapters` (`LoomToolAdapter`) | the trait carries *paths*, not *editable rules* — but no adapter needs editable rules yet, because Loom injects nothing into prompts | the schema is a per-tool declaration and the adapter trait is the per-tool declaration point — true, but a declaration with no author | **defer** (Phase D) with three named preconditions (§7) |
| Probe/composition engine | none | — | — | **defer** behind the Phase B contrast experiment, then gated on pillars 1 and 4 |
| Experience library (cue/lesson/evidence) | `loom-hindsight` (typed facts), `loom-knowledge` (graph) | hindsight stores flat facts with a free-text body — no cue/lesson/evidence contract, no per-domain scoping | pillar 5 already promises "outcome-gated lessons via loom-knowledge + loom-hindsight" — and owns the only lesson store ([plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) §3.6) — but it is **written by the selection loop**, so it is Phase C output, not a pre-loop store | **defer to Phase C**, and land in `loom-hindsight` as one `lesson` entity — not a second owner beside pillar 5 |
| New crate `loom-harness` | — | — | the engine spans adapters + sessions + parallel + hindsight; a crate boundary is justified only if it survives Phase B | **defer** — a crate is created only if the engine grows out of `loom-adapters` + `loom-hindsight` + `loom-parallel` hosting |

**Net: one new module in `loom-skills` (the walk + validator + trigger generator), one dead file deleted, no new crate, no new CLI group, and no LLM spend in the recommended path.** Everything else is conditional on a measurement that has not been made yet.

---

## 6. Rust-native design sketch

### 6.1 Terminology (resolved before any code)

- Loom's existing "harness" = **agent tool** (`codex`, `claude-code`). Keep it.
- Harness²'s "harness" = **harness surface** — the editable scaffolding of one tool. Use "harness surface" everywhere in Loom. Never "harness" for the surface, never "surface" for the tool.

### 6.2 `SurfaceSpec` (Rust) — Phase D, deferred

Lives in `loom-adapters` next to `LoomToolAdapter`. **This section is a reference design, not a recommendation to build it now** — see the three preconditions in §7 Phase D.

```rust
// crates/loom-adapters/src/surface.rs
/// Built once per adapter and handed out as a reference.
/// (The `manifest.toml` deserialisation that the `Deserialize` derive was
/// for is **cut** — §6.3/§7.1. If it is ever revived, add
/// `serde = { workspace = true }` to `crates/loom-adapters/Cargo.toml`;
/// that crate has `serde_json` + `toml` but no `serde` today.)
///
/// This is a *reinterpretation* of upstream's `SurfaceSpec`, not a mirror:
/// upstream also carries `skill_name_rule`, `trigger_hint`, `autotrigger_file`,
/// `trigger_line`, `hook_events`, `compile_skill_scripts`,
/// `reject_empty_always_on` and `stubs` (`harness2/core/tree.py:40-60`).
/// The dropped fields are exactly the ones with a concrete user-visible
/// failure mode today — see §6.2.1.
#[derive(Clone, Debug)]
pub struct SurfaceSpec {
    pub tool: String,
    /// Files injected into every turn, in order. Their concatenated length is capped.
    pub always_on: Vec<String>,
    pub always_on_max: usize,
    pub always_on_cap_reason: String,
    pub skill_dir: String,               // relative to the tool's native root
    pub plugin_dir: Option<String>,
    pub hooks_file: Option<String>,
    pub subagent_requires_mode: bool,
    /// Reject a surface whose always-on tree is empty (Codex does; OpenCode does not).
    pub reject_empty_always_on: bool,
    /// Validation rules, one predicate per path class.
    pub rules: Vec<SurfaceRule>,
}

#[derive(Clone, Debug)]
pub enum SurfaceRule {
    SkillNameMatchesDir,
    SkillFrontmatterRequired(Vec<String>),
    SubagentFrontmatterMode,
    HooksJsonValid,
    PathEscapeRejected,
}
```

Contract mirrors Harness²'s *invariants* (the invariants are what make the method work), but not its field list — see §6.2.1:

- `always_on` concatenated length ≤ `always_on_max`, else the surface is rejected (Harness²: 24k OpenCode / 8k Codex).
- Every editable path must pass a single allow-list predicate (`is_allowed`) and a path-escape guard (`normalize_rel` rejects `..` and absolute paths).
- A tree that violates any rule is **dropped before use**, not repaired.
- Stubs: per-surface starter files so an empty surface is still a valid surface.

`LoomToolAdapter` gains `fn surface(&self) -> Option<SurfaceSpec> { None }` — a default of `None` keeps all 12 existing adapters compiling and makes "this tool has no editable surface" an explicit, visible statement rather than a silent gap.

Each adapter builds a built-in default `SurfaceSpec`. *(Sketch note: a `~/.loom/surfaces/<tool>/manifest.toml` override was in the original sketch and is **cut** — it is a second source of truth beside `~/.loom/tools/<tool>.toml`. See §7.1.)* The adapter constants are the only source of truth unless a user-facing reason for an override appears.

*Build note:* `loom-adapters` currently depends on `serde_json` + `toml` but not `serde` with `derive`. If the rules are plain functions (as §7.1 recommends) no dependency change is needed at all; only the serde-deserialisable variant would need `serde = { workspace = true }` added to `crates/loom-adapters/Cargo.toml`. Phase D is **deferred** — see §7.

#### 6.2.1 The eight upstream fields this sketch drops — and why that is the actual finding

Upstream's `SurfaceSpec` (`harness2/core/tree.py:40-60`) carries eight fields the Rust sketch above omits. Two of them are not "config" — they encode **silent failure modes**, and Loom is in every one of them today:

| Upstream field | Upstream behaviour | Loom today (verified) |
|---|---|---|
| `skill_name_rule` (`must_match` / `overrides_dir`) | a `name:` that does not match the directory is **"dropped SILENTLY at load"**; an overriding `name:` makes every trigger line stop matching (`tree.py:277-291`) | **no frontmatter parsing at all** — `crates/loom-cli/src/commands/skills.rs:161-176` copies the file and never parses it; `crates/loom-skills/src/loader.rs:44-104` indexes flat `*.md` by size+mtime |
| `trigger_hint` / `autotrigger_file` / `trigger_line` | `autotrigger` appends one trigger line per skill into the always-on file, skipping any line that would push the body past the cap (`tree.py:192-213`) | **no trigger mechanism** — `crates/loom-skills/src/adapter.rs` only does string replacement |
| `reject_empty_always_on` | Codex rejects an empty always-on tree; OpenCode does not (`codex.py:39`, `opencode.py:42`) | n/a — no always-on tree exists |
| `stubs` | per-surface starter files | n/a |
| `hook_events`, `compile_skill_scripts` | hook schema and script compilation rules | Loom's `Hook` is script-path based with a free-form string event (`crates/loom-hooks/src/lib.rs:18-27`) |

The consequence is the actual bug worth fixing, and it is *not* the schema:

> A Loom user can install a skill with no `description:` frontmatter. `sync_skills_to_native` (`crates/loom-adapters/src/lib.rs:227-273`) symlinks it into every native tool directory. The agent then **silently ignores it** — upstream calls this "dropped SILENTLY at load" (`tree.py:272-276`). Loom reports success throughout.

That is why §7 recommends a **walk + validator + trigger generator** (3–4 days, owner `loom-skills`) rather than the schema-as-data surface (8–12 days, owner `loom-adapters`). The schema is the artefact; the validator is the value. See [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md).

**A live pillar-1 violation in the same crate:** `crates/loom-skills/src/neutral_renderer.rs` is the tree's only YAML-frontmatter parser (`parse_frontmatter:74`, `serde_yaml::from_str:97`), but it is **not declared** in `crates/loom-skills/src/lib.rs:1-7`, `loom-skills` has no `serde_yaml` dependency, and **nothing in the tree calls `NeutralRenderer`** (`grep -rn "NeutralRenderer" crates/ packages/` matches only the file itself). It is also **broken**: copied out and compiled standalone, it fails with *"character literal may only contain one codepoint"* (`neutral_renderer.rs:198`) and *"mismatched closing delimiter: `}`"* (`:354`). So it is not merely unwired — it does not compile, and `cargo check -p loom-skills` passes only because the module is never declared. `ROADMAP.md:23` requires ship-or-remove; §7 therefore **deletes** it (373 lines, zero callers) rather than reviving it, and the new validator owns the frontmatter parse.

### 6.3 Surface storage (Phase D only — deferred)

```
~/.loom/surfaces/<tool>/          # upstream shape — REFERENCE ONLY, not a recommendation
├── manifest.toml          # SurfaceSpec as data
├── always_on/             # systemprompt.md, guardrails.md, memory.md, tool_guidance.md
├── skills/<name>/SKILL.md
├── subagents/<name>/AGENT.md
├── scripts/<name>.py
├── plugins/<name>.ts
└── hooks.json
```

The tree above is the *improvement surface* — the tree that would ship to the tool's native root, reusing the existing `sync_skills_to_native` / `sync_mcp_to_native` pattern (`crates/loom-adapters/src/lib.rs:169,227`): additive merge into the tool's native config, never clobbering.

**This is deferred, and the cheapest form of it is not the one above.** A per-tool surface directory is a *second source of truth* beside the existing `~/.loom/tools/<tool>.toml` (`crates/loom-generic-adapter/src/config.rs:34`), and the `sync_surface_to_native` method it implies is cut along with it (§7.1). Phase A needs no such directory: it validates skills *where they already live* (`~/.loom/skills/`) and *where they are about to be synced* (the native tool dir). If Phase D ever happens, prefer per-adapter constants plus the Phase A validator function over a manifest-driven rules engine.

### 6.4 Experience library (Phase C only — deferred, and owned by pillar 5)

Upstream's store is a directory of adopted trees. **Loom should not copy that shape**: upstream retains the full tree because the tree *is* the artefact; Loom's artefact is the lesson, and pillar 5 already specifies where lessons live — a `lesson` entity in `loom-knowledge` plus facts in `loom-hindsight` ([plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) §3.6). A second store would be a duplicate owner.

What is worth carrying over is the **contract**, not the storage:

```
~/.loom/experience/<domain>/          # upstream shape — reference only
├── INDEX.md
└── tasks/<task-slug>/
    ├── surface/<rel>      # the FULL adopted tree — never a summary
    └── EXPERIENCE.md      # cue / lesson / evidence [+ confidence, edits]
```

```rust
pub struct Experience {
    pub cue: String,       // when this applies
    pub lesson: String,    // what to do
    pub evidence: String,  // what was observed — required, non-empty
    pub confidence: Option<f64>,
}

impl Experience {
    /// All three of cue/lesson/evidence must be non-empty. No evidence, no entry.
    pub fn is_storable(&self) -> bool { ... }
}
```

Rules carried over verbatim from Harness²:

- **Full tree, never a summary.** A summary loses the exact wording that made it work.
- **Retrieval by construction.** A domain directory only holds that domain's entries, so cross-domain contamination is structurally impossible.
- **Grader verdicts asserted absent** from any stored tree before it is written (no benchmark answer leaks into a reusable artifact).
- `forget` / `retain` semantics: keep entries preceding the current task in the stream.

Bridging to what Loom already has:

- The lesson is retained in `loom-hindsight` as a typed fact with `fact_type = "lesson"` → cross-domain recall via `recall()` / `reflect()`. (Not `"pattern"` — that is a pre-existing hindsight fact type; pillar 5's contract names the type `lesson`.)
- The surface's entities are added to `loom-knowledge` (`add_entity`, `add_relationship`) → the graph can answer "which tasks touched this file/tool", and the existing +0.25 routing boost stays meaningful.

*One owner.* The lesson store is pillar 5's: a `lesson` entity in `loom-knowledge` plus a fact in `loom-hindsight` ([plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) §3.6), which is confirmed there as the only lesson store in Loom. If Phase C ever writes lessons, it writes *there* — `~/.loom/experience/` is upstream's shape, not Loom's. What Loom adds is the *contract* (cue/lesson/evidence all non-empty, evidence-gated, per-domain) and the *writer* (the selection loop), not a second store.

*Dependency note:* `loom-hindsight` does not depend on `loom-knowledge` (checked: `crates/loom-hindsight/Cargo.toml` has no `loom-knowledge` entry), and it should not — `loom-knowledge` is the heavier SQLite/FTS5 crate. The knowledge-graph bridge therefore lives in the **caller** (the CLI command that writes an experience), which already has access to both. Keep the bridge one-directional and out of `loom-hindsight`.

### 6.5 Probe loop (Phase C — deferred, but specified so it can be judged)

The loop, expressed in Loom's primitives:

| Harness² | Loom primitive |
|---|---|
| base rollout | `loom exec <tool>` / `AcpProtocolClient::run_prompt` (`crates/loom-sessions/src/acp_protocol.rs:206`) — captured as a Loom session |
| candidate probes (2k) | `AgentPool::execute_all` + `WorktreeManager::create_worktree` (`crates/loom-parallel/src/pool.rs:102`, `worktree.rs:63`), one worktree per candidate surface, `AgentTask::env` pointing `LOOM_SURFACE_DIR` at the candidate tree |
| isolation | same worktree mechanism, one worktree per candidate |
| selection (anonymized) | new: assign `A..Z, AA..` ids over a seeded shuffle of candidate labels — SHA-256 over `task\|k\|mode`, not SHA-1 (Harness²'s SHA-1 is a stability choice, not a security one); the pool is exactly the k finals of that mode, so the reference runs are not selectable |
| choice parse | new: fenced `` ```choice `` block; `<why>` / `<experience>` / `<edits>` extraction from the same reply |
| compose | new: merge the chosen candidate's edits into the candidate surface tree, then validate against the surface before it is allowed to ship (where the tree lives is a Phase-D decision — see §6.3) |
| ledger | new: `R = 4k` / `P = 3k` identity check mirroring `ledger.py:26,31`, backed by `loom-telemetry` counters; `BudgetError` on mismatch |
| behavioural contrast | **the open question — measured by Phase B (§7)** |

### 6.6 The behavioural-contrast problem (honest gap)

Harness² composes from "behavioural contrast between executions" without a ground-truth grader. In Loom the contrast signal has to come from somewhere concrete, and there are exactly three candidates:

| Signal | Loom source | Cost | Verdict |
|---|---|---|---|
| Deliverable diff | `loom-ast` + worktree diffs between base and candidate | free | **viable but weak** — different wording of the same working file is not a behavioural difference |
| Test / suite outcome | `loom-runtime` execution in the candidate worktree | free-ish (needs the repo's own test command) | **viable** — matches Harness²'s `suite_green` state, but needs a repo-local test command (pillar 5's `review.toml` `test`/`check` fields already plan this) |
| Rubric judge (LLM-as-judge) | `loom acp run` with a rubric prompt | one LLM call per candidate | **viable** — closest to Harness²'s `judge.py`; needs an evidence state machine (`proven` / `suite_green` / `unverified` / `could_not_run` / `refuted`, already specified in [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md)) |

**Recommendation:** Phase C uses the *test/suite outcome* signal first (free, deterministic, already plumbed through pillar 5's `review.toml` plan) and adds the rubric judge only if the suite signal proves too coarse. The judge must follow pillar 5's evidence states verbatim — a cut or timed-out step is `could_not_run`, never `refuted` — and grading happens **after** selection, with ungraded candidates excluded from the denominator rather than scored zero (Harness²'s `Scored.graded` rule).

---

## 7. Implementation proposal (re-scoped)

No code is written by this document. This is the plan a follow-up spec would formalise.

**This section was re-scoped after review.** The earlier draft of this document proposed a four-phase build (surface schema → experience library → probe loop → selection) totalling 38–59 engineer-days. That ordering was wrong for two reasons now verified in upstream source:

1. **The experience library has no producer without the selection loop.** Upstream's library has exactly one writer — the selection loop (§2.4). Building the library first builds a store with no producer.
2. **The schema was the artefact, not the value.** The one defect in Harness² worth copying is a *validator* that catches skills the agent will silently ignore (§6.2.1). A full `SurfaceSpec` schema solves a problem Loom does not yet have.

What follows is therefore ordered by *value per day*, and gated by cheap experiments that can kill the expensive part.

### Phase A — Skill inventory + validation + trigger generation (**recommended now**, 3–4 days, owner `loom-skills`) — **SHIPPED 2026-10-21**

> **Status: landed.** Phase A shipped on `main` in merge `37baf41` (branch `phase-a/integrate`): the one walk in `crates/loom-skills/src/scan.rs` with one validator and one real YAML parser, the trigger generator in `crates/loom-skills/src/trigger.rs`, the registry populated from that walk, install/remove fixed for both layouts, `neutral_renderer.rs` deleted, the validator wired into both sync paths through one shared predicate, and the `loom doctor` row. All 23 acceptance criteria of the spec re-verified against the merged tree on a copy of the live corpus. The bullets below are kept as the plan of record — the shipped shape matches them, with two deliberate differences the brief corrected: `missing_script` is a **warning**, not fatal; and a defecting skill is still **listed** with its defect attached, never dropped from the inventory. Phase B is unaffected and remains the gate for everything after it. Spec: [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md) §4; review: [plan/LOOM_PHASE_A_REVIEW.md](plan/LOOM_PHASE_A_REVIEW.md).

Fixes three live defects. *A0 (inventory):* `loom skills list` returns `[]`, `loom skills show` finds nothing, and `loom lock --generate` writes zero entries — while 11 skills are installed on this machine, because the registry is never populated. *A1 (validation):* a skill with no `description:` frontmatter is installed, synced into every native tool directory, and then **silently ignored by the agent** — while Loom reports success everywhere.

- `crates/loom-skills/src/scan.rs` (**new**): one walk over `~/.loom/skills/` that handles **both** layouts (`<name>/SKILL.md` and flat `<name>.md`), parses each frontmatter, and produces the skill list plus the defect list. It is the single place skills are read.
- `crates/loom-skills/src/scan.rs`: `pub fn validate_skill_tree(skills_dir: &Path) -> Vec<SkillDefect>`.
  Checks per skill: the file is found in **either** layout; the frontmatter parses as YAML; it has a non-empty `description:`; `name:` (if present) matches the expected name for that layout; a referenced `scripts/…` file exists; a flat/dir pair of the same name is reported as a duplicate.
- `crates/loom-skills/src/trigger.rs`: `pub fn trigger_lines(skills: &[SkillEntry], cue_of: impl Fn(&SkillEntry) -> &str) -> TriggerSet` — one line per skill, e.g. `When the task mentions <cue>, read skills/<name>/SKILL.md`. Total length is capped; lines that would exceed the cap are dropped and **reported** in `TriggerSet::not_triggered`, not silently omitted (upstream's `autotrigger` behaviour, `harness2/core/tree.py:192-213`). The cue comes from the **parsed** YAML value, not from upstream's regex — see §12.3.
- Populate the registry from the walk (`crates/loom-skills/src/registry.rs`): the schema already has a `description TEXT NOT NULL` column (`registry.rs:35`) and the row type is already `loom_core::SkillEntry` (`crates/loom-core/src/types.rs:282`) — **no schema change, no new struct**. The walk's output *is* a registry row; `SkillEntry` is reused rather than a fourth "what a skill is" type being added (§12.2).
- Fix the two write paths: `crates/loom-cli/src/commands/skills.rs:161` (install) must accept a **directory** and a file — `std::fs::copy` on a directory fails today, so `loom skills install <dir>` cannot install the layout-A form at all; and `:177` (remove) must remove a directory tree, not `remove_file` it.
- Wire the validator into the two places a skill enters a tool: `crates/loom-cli/src/commands/skills.rs:161` (install) and `crates/loom-adapters/src/lib.rs:227` (`sync_skills_to_native`), plus the layout-B path `crates/loom-cli/src/commands/exec.rs:518` (`inject_shared_skills`). A defecting skill is **rejected with a named reason**, not synced.
- Surface the result in `loom doctor` (existing command — no new CLI group) and in `loom skills list`.
- Delete `crates/loom-skills/src/neutral_renderer.rs` (§6.2.1): 373 lines, zero callers, does not compile. Pillar 1 requires ship-or-remove (`ROADMAP.md:23`); the new validator owns the frontmatter parse.

**Acceptance criteria**

1. `loom skills list` returns the installed skills (11 on this machine), and `loom lock --generate` writes one entry per skill.
2. `loom skills install <dir>` and `loom skills install <file.md>` both succeed; `loom skills remove <name>` removes both forms, leaving nothing behind.
3. A skill with no `description:` is rejected at install with the file path and the missing field named — and is never symlinked into a native tool directory.
4. A skill whose `name:` disagrees with its directory is rejected with both values shown; a **quoted** `name:` is not a false mismatch.
5. `validate_skill_tree` returns **every** defect in the tree, not just the first.
6. Trigger lines are generated for every valid skill, and a skill whose line would exceed the cap is named in the result as "not triggered" — never silently dropped. A folded (`description: >`) description yields the parsed text as the cue.
7. `cargo test -p loom-skills` covers: missing `description:`, mismatched `name:` (layout A and B), quoted `name:`, multi-line `description: >`, unterminated fence, empty tree, the cap boundary, a missing referenced script, a duplicate flat/dir pair, and a directory install + remove round-trip.

**Cost:** 3–4 days. **LLM spend:** none. **New crate:** none. **New dependency:** `serde_yaml` in `loom-skills` (the replacement parser; `neutral_renderer.rs` is deleted, not repaired).

Full spec: [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md).

### Phase B — the cheap experiment that gates everything (2–3 days, $10–20)

Run this **before** writing any probe-loop code. It is the whole of Harness²'s core claim, reduced to its minimum.

- 5 tasks from this repo. For each: one **base** run (current `~/.loom` state) and one **candidate** run with a single hand-edited always-on surface change.
- Signal: the repo's own test command, run in the candidate worktree (`loom-runtime` / `loom-parallel`).
- Budget cap: $10–20 total, hard stop.

**Decision rule (pre-registered, so the result cannot be reinterpreted afterwards):**

| Outcome | Meaning | Action |
|---|---|---|
| Candidate suite differs from base in ≥3 of 5 tasks | the contrast signal exists | proceed to Phase C scoping |
| Identical in 5 of 5 | the signal is zero for this workload | **the probe loop does not ship.** Record the negative result in the experience library and stop |
| Fewer than 5 tasks can be run reproducibly | the harness is not yet reliable enough to measure | fix the harness before spending on it — not a green light for Phase C |

**Cost:** 2–3 days, $10–20. **Note:** this experiment spends more than Phase A. That is deliberate — it is the cheapest way to kill a 38–59 day plan.

### Phase C — probe + contrast + selection (conditional on Phase B)

**Only if Phase B shows a real signal.** This is where upstream's mechanisms land, and where the expensive part lives.

- Probe runner over `loom-parallel`: one worktree per candidate surface, one `AgentPool` batch, each probe captured as a Loom session tagged `harness-probe`.
- Outcome signal: repo-local test/suite command, executed by `loom-runtime` in the candidate worktree.
- Ledger: `R`/`P` identity check (`R = 4k`, `P = 3k`), `BudgetError` on mismatch.
- Selection: anonymized seeded ids, `` ```choice `` parsing, `<why>`/`<experience>`/`<edits>` extraction, deterministic fallback on a malformed reply.
- Compose: merge the chosen candidate's edits, re-validate, only then sync to the native root.
- Experience library: written **by** the selection loop (upstream's dependency direction, §2.4), landing in `loom-hindsight` as one `lesson` entity — not a new store, and not a second owner alongside pillar 5.

**Gate:** pillar 4 usage metering must exist first, **or** a hard per-run budget cap must be added to `loom-telemetry`. Upstream has no cost cap (§2.6) — that cap is Loom's contribution and it is non-negotiable here. **Do not ship an unmetered probe loop.**

**Acceptance criteria**

1. A probe batch of *n* candidates produces *n* worktrees, *n* sessions, and zero writes outside the worktrees.
2. A candidate that fails to apply its surface is dropped before execution, never half-applied.
3. The ledger refuses to report a run whose rollout/session counts do not match `R`/`P`.
4. Re-running selection with the same `(task, k, mode)` yields the same id→candidate mapping.
5. The selection pool contains exactly the k finals of the mode — a reference/base run is never selectable.
6. A malformed reply falls back deterministically and is never silently treated as a valid choice.
7. Graded and ungraded candidates are counted separately; ungraded candidates never enter the denominator.
8. The composed surface is re-validated before it is synced; a failing surface is not shipped.

### Phase D — surface schema (`SurfaceSpec`) — **deferred, optional**

The `SurfaceSpec` design in §6.2 is sound but currently has no consumer: Loom has no always-on prompt surface to describe, and no prompt injection anywhere (`exec.rs:263` sets only `LOOM_AGENTS_DIR`, `LOOM_ENABLED`, `LOOM_PROJECT_NAME`; `resume.rs:33,72` are the only prompt-adjacent injections). Building it now is "a schema with no author".

Revisit **only** when one of these is true:

- Phase C is running and needs a place to put a candidate always-on tree; **or**
- Loom decides to ship a prompt-injection surface for a tool (a product decision, not a Harness² consequence); **or**
- a third-party tool ships a harness-editing protocol Loom must speak.

If it happens, the cheap form is: a plain `fn validate_skill_tree(...)` (Phase A already needs one) plus per-adapter constants — **not** a serde-deserialisable rules engine, not a `~/.loom/surfaces/<tool>/manifest.toml` second source of truth beside `~/.loom/tools/*.toml`, and not a new `loom surface` CLI group (36 top-level commands exist already; `loom doctor` and `loom skills` are enough).

### Phase E — integration + reporting (conditional on Phase C)

- A contrast report for a run — base vs candidate table with pillar-5 evidence states. It rides an **existing** command (`loom sessions show <id>` / `loom routing show`), not a new `loom surface report` group (§7.1 cuts that group).
- `loom sessions export` of probe runs so the improvement loop is inspectable and reproducible.
- A `loom-harness` crate is created **iff** the engine has outgrown `loom-adapters` + `loom-hindsight` + `loom-parallel` hosting — and never as a fork of either.

---

## 7.1 What was cut, and why

| Cut | Reason |
|---|---|
| `SurfaceSpec` as serde-deserialisable rules-as-data (§6.2) | a schema with no author today; upstream's own validator is plain functions (`tree.py:267-333`) |
| `~/.loom/surfaces/<tool>/` + `manifest.toml` + `sync_surface_to_native` | a second source of truth beside `~/.loom/tools/*.toml`, for a surface that does not exist yet |
| `loom surface show/validate/report` CLI group | 36 top-level commands already; `loom doctor` + `loom skills` cover the actual need |
| `~/.loom/experience/` + `INDEX.md` + a knowledge bridge | one `lesson` entity in `loom-hindsight` + pillar 5's existing `loom memory retain --type lesson` is enough |
| Full-tree retention + grader-verdict assertion | upstream retains the tree because the tree *is* the artefact; Loom's artefact is the lesson |
| The `loom-harness` crate | `ROADMAP.md` pillar 1: no new crate without proof of duplication |
| The original Phase 0–4 plan (38–59 days total) | replaced by Phase A (3–4 d) + Phase B (2–3 d, $10–20) as the gate |

---

## 8. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **Unmetered LLM spend.** `R=4k`/`P=3k` per task is expensive; Loom's metering is pillar 4. Upstream has no cost cap at all (§2.6). | High | Phase C is gated on metering, plus a hard budget cap from `loom-telemetry` |
| **A prompt-injection surface is new behaviour for every tool.** Loom currently injects nothing into prompts except at `loom sessions resume` (`resume.rs:33`). Adding always-on prompt text changes every agent's behaviour. | High | Phase D only, opt-in per tool, char cap enforced, every change diffable — and Phase D has three named preconditions (§7) |
| **The behavioural-contrast signal may be too weak.** Without a grader, "different output" is not "better output". | High | Phase B measures it for $10–20 *before* any probe code exists; the decision rule is pre-registered so the result cannot be reinterpreted afterwards |
| **Benchmark results do not transfer.** Harness²'s gains are Vertex-AI + LAB/JobBench/WorkBuddy numbers. | Medium | the doc claims no transfer; the local signal (tests) is the only claim made |
| **Terminology collision** ("harness" = tool vs surface) | Medium | §6.1 resolves it before any code |
| **Scope creep into a second orchestrator.** Harness² is close to "another orchestration UI", which is an explicit ROADMAP non-goal. | Medium | Phases A and B add *no* orchestration: no scheduler, no fan-out, no UI, no new CLI group |
| **Duplicate owner.** A `loom-harness` crate could fork `loom-adapters` (surface knowledge) and `loom-hindsight` (memory); the lesson store could fork pillar 5's. | Medium | §5 defers the crate; the experience library lands in `loom-hindsight` as one `lesson` entity, never a parallel store |
| **Building a store before its writer.** The failure mode this document was originally written into. | Medium | the selection-loop → library dependency is now stated in §2.4 and enforced by the phase order in §7 |
| **Building a validator nobody can see.** The same failure mode, one level down: A1 (validation) without A0 (a real inventory) validates skills that `loom skills list` still cannot show. | Medium | A0 comes first inside Phase A; the two halves are one walk (§7 Phase A) |

---

## 9. Non-goals

- Not a Harness² port. No Python, no `uv`, no Vertex AI, no `harness2` subprocess.
- Not a benchmark harness. Loom will not ship JobBench/LAB/WorkBuddy.
- Not an orchestrator. No scheduler, no fan-out UI, no agent marketplace.
- Not a new memory system. The experience library (if it ever ships) extends `loom-hindsight`; it does not replace it, and it does not duplicate pillar 5's lesson store.
- Not a plugin runtime. Loom's plugin registry stays manifest-based.
- Not a prompt-injection surface. Loom does not start writing into agents' prompts because an upstream paper does.

---

## 10. Verdict

**Harness² is a source of four specific mechanisms, and only those.** It is not a dependency, not a competitor, and not a template for Loom's architecture.

| Phase | Value | Cost | Gate |
|---|---|---|---|
| **A — skill inventory + validation + trigger** | **high and immediate** — fixes a live silent-drop bug *and* a live blind inventory (`loom skills list` → `[]` with 11 skills installed); upstream treats the same defect as a hard error | 3–4 d, no LLM, no new crate | none |
| **B — contrast experiment** | high — it is the *only* thing that can prove the rest is worth building, and it costs $20 | 2–3 d, $10–20 | none (this *is* the gate) |
| C — probe + contrast + selection | unknown until B is measured | **high** | B shows a signal **and** pillar 4 metering exists |
| D — surface schema | medium — a real interoperability primitive, but with no author today | 8–12 d, no LLM | one of three named preconditions (§7) |
| E — integration + reporting | medium | low | C running |

**Recommended: approve Phase A now and Phase B next; hold C, D, and E behind the measurements they depend on.**

Phase A is worth doing on its own merits even if the entire rest of this document is never implemented: Loom today installs a skill the agent will silently ignore and reports success, and its own skill registry has never been populated — so three of its commands cannot see a single skill. Phase B is worth doing because it is the cheapest possible test of Harness²'s core claim, and it can only *save* money — if the signal is zero, the probe loop never ships.

The cost of doing nothing is small and should be stated honestly: Loom keeps working, and the two concrete defects Phase A fixes keep costing users one silently-ignored skill and one blind skill list at a time. Nothing in this document is on a critical path.

**Total recommended spend: 5–7 days and $10–20**, against 38–59 days for the plan this document originally proposed.

---

## 11. Direction: a self-evolving shared harness

The phases above are deliberately small, and the deferrals are not a verdict against the direction. The direction is worth moving toward; the phasing only says *measure before you build*.

Upstream's own term is **recursive agent harnessing** — a harness that improves for the next task, driven by an LLM that proposes, probes, and composes edits, with evidence-backed lessons carried forward. Loom's *design* already has most of that machinery; almost none of it is built yet, which is why the phasing in §7 matters.

**Loom's pillar 5 is Harness²'s five mechanisms, with a different edit target.** Pillar 5 ([plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md), post-1.0, gated on pillar 2) specifies the same four stages — but edits *code*, not *scaffolding*:

| Harness² | Loom pillar 5 (verified review) — *specified, not built* |
|---|---|
| propose candidate edits | the review plan + cross-family reviewer seats produce candidate fixes |
| probe the candidate's effect | the worktree evidence runner executes the repo's own test |
| compose the verified change | one fix PR carries only the verified patches |
| reflect → carry forward | an outcome-gated lesson is injected into the next session touching that file |
| **edit the harness itself** | **← the one Loom lacks** — pillar 5 edits *code*, not the *scaffolding* |

So "a self-evolving harness" is not a new pillar. It is pillar 5 with the edit target widened from code to the always-on surface. That also clarifies a phase dependency the earlier drafts blurred: **pillar 5 is the outcome-signal engine** — it is what turns "the probe changed something" into "the probe changed something *for the better*, with evidence." Phase C needs it, and Phase C is therefore *downstream of pillar 5* — not parallel to it. Phase B does not need it (the repo's own test command is enough), which is exactly why Phase B can run now, cheaply, before anything else exists.

### 11.1 The decision upstream does not make: per-tool or shared

Upstream evolves **one tool's** surface — OpenCode's `systemprompt.md` (`harness2/substrate/opencode.py:42`). Loom's thesis is the opposite: one skill, twelve adapters (`crates/loom-adapters/src/lib.rs:334`). If Loom evolves a surface, it has to pick:

| Target | Gain | Cost | Problem |
|---|---|---|---|
| **One tool's prompt** (e.g. Claude Code's) | fast to measure against one agent | high — Loom ships a tool-specialised product it does not own | breaks the shared-layer thesis; 12 adapters, 12 divergent surfaces |
| **The shared layer** (skills, tool guidance, trigger sets) | one improvement benefits every tool | needs the shared layer to be genuinely editable, which it is not today | Loom currently *distributes* shared resources but never *edits* them |

**Shared is the answer Loom's own north star gives** ("the shared agentic ecosystem … skills written once and shared across every tool"). It is also the one upstream structurally cannot give, because upstream's executor is a single tool by construction. If Loom wants a differentiation, this is it — and it is a bigger decision than any of Phases A–D.

### 11.2 The obligation this direction creates

A self-modifying surface is a **security-pillar** item, and it must be designed in from the start rather than retrofitted when the first agent rewrites a prompt. Loom already signs plugins (Ed25519, `crates/loom-core/src/plugin_registry.rs:30`) and hash-chains the vault audit log — but that covers *plugins a human installed*, not *a surface an LLM rewrote*. Three questions must be answered before any proposer can write a surface:

1. **Who may write the surface** — which agent, under which grant. A per-run, per-tool scoped credential (pillar 2) is the existing answer.
2. **What is signed** — the same Ed25519/sha256 machinery the plugin registry uses, applied to a surface tree, so a changed always-on file is detectable.
3. **What the audit log records** — before/after diffs per edit, attributable to the run that made them.

"Self-healing" becomes "self-modifying", and that is a different class of risk from a malformed skill.

### 11.3 What this section does not do

It does not commit Loom to self-evolution, and it does not open a pillar. The phases in §7 stand unchanged: **Phase A now, Phase B next, then pillar 5 supplies the outcome signal, then the proposer.** What is deferred is spending 38–59 days *before* the $20 measurement — not the destination. This section exists so the destination is on paper, and so the per-tool-vs-shared choice and the security obligation are decided deliberately rather than by whoever writes the first proposer.

---

## 12. First-principles review of this document and its proposal

A review pass over this document and [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md) checked every load-bearing claim against the source tree and against a live audit of `~/.loom/skills/`. It changed the plan in four ways: it found a *bigger* defect than the one the plan was written for (§12.4), it removed a type the plan would have added (§12.2), it rejected one of upstream's techniques (§12.3), and it deleted a file the plan would have revived (§12.5). This section records what was found, so the reasoning survives the edit.

**The five-line review:**

- **Claim:** "a skill without a `description:` is silently dropped, and Loom reports success." — **Verdict: true, and smaller than presented.** 1 defect in 17 files; preventive, not corrective.
- **Claim:** "the fix is a validator." — **Verdict: incomplete.** The registry is never populated, so `loom skills list` returns `[]` with 11 skills installed. The slice needs an inventory first (§12.4).
- **Claim:** "upstream's `autotrigger` is the model." — **Verdict: the rule yes, the extraction no.** Upstream's regex yields the literal `>` for a folded description; 13 of 16 live descriptions are folded (§12.3).
- **Claim:** "`neutral_renderer.rs` is the tree's frontmatter parser." — **Verdict: it is dead and it does not compile.** Delete it, do not revive it (§12.5).
- **Claim:** "Phase A is 2–3 days." — **Verdict: 3–4 days, for one extra day of inventory work that the review made mandatory.**

### 12.1 The two documents disagreed

The analysis document specified `pub fn trigger_lines(skills: &[SkillMeta]) -> Vec<String>`; the proposal specified `pub fn trigger_lines(skills: &[SkillMeta], cue_of: impl Fn(&SkillMeta) -> &str) -> TriggerSet`. Same name, different return type, and the proposal's version had a parameter the analysis version did not. Two documents describing one function are two contracts.

**Resolved:** the proposal's signature is canonical — the cap needs the skipped skills reported, so the return cannot be a bare `Vec<String>`, and the cue is a *choice* (upstream uses a folder-specific trigger hint; Loom's shared layer does not), so it is a parameter rather than a hard-coded field. The analysis document now carries the same signature. `validate_skill_tree` differed only in a parameter *name*; that is now identical too.

**The general rule this exposes:** a document that quotes a function signature must quote it identically in every document. A signature is not prose.

### 12.2 The proposal would have added a fourth struct for a job that already has three

Loom already has three types that model "a skill":

| Type | Where | What it carries |
|---|---|---|
| `SkillMetadata` | `crates/loom-skills/src/loader.rs:20` | name, path, content hash, mtime — **no description**; the loader has zero callers |
| `SkillFrontmatter` | `crates/loom-skills/src/neutral_renderer.rs` | name, description, tags, version — but in a file that is not declared, not compiled, and has zero callers |
| `SkillEntry` | `crates/loom-core/src/types.rs:282` | name, **description**, path, hash, tags, ecosystem flags, version, mtime — and it is what the registry row already is (`crates/loom-skills/src/registry.rs:1,90`) |

The proposal's `SkillMeta { name, description, path }` would have been the fourth. **Resolved:** no new struct. The walk produces `loom_core::SkillEntry` directly — the registry's existing row type, which already has a `description` column behind it (`crates/loom-skills/src/registry.rs:35`, `description TEXT NOT NULL`) and already knows how to be written (`registry.upsert`, `:156`). The trigger generator consumes `&[SkillEntry]`. The only thing "missing" was a *writer*, and the writer is the walk.

**The general rule:** before adding a type, check whether the codebase already owns the concept. Loom's problem was never the type; it was that nothing populated the store.

### 12.3 The upstream extraction is broken, and copying it would have copied the bug

Upstream builds its trigger line from a regex over the frontmatter (`harness2/core/tree.py:192-213`):

```python
m = re.search(r"^description:\s*(.+)$", fm, re.M)
desc = m.group(1).strip().rstrip(".")
```

Running that against the live corpus produces `- When >, load the \`ponytail\` skill.` — the regex captures the literal `>` of a folded scalar, the literal `|` of a literal block, and the surrounding quotes of a quoted value. **13 of the 16 parsed Loom descriptions are folded or literal.** Upstream's own trigger text is broken for every YAML style except a plain inline scalar; its *check* (`^description:\s*\S`, `tree.py:265-300`) accepts all three styles, but its *extraction* does not.

**Resolved:** the proposal copies upstream's **rules** and not its extraction. The cue is the **parsed** YAML value, and the parse is a real YAML parse — which the corpus demands anyway: a `^description:` regex would also produce a false `name_mismatch` on `name: "graphflow"` (quoted) and would mis-handle the `>`/`|` styles present in 13 of 16 live skills.

**The general rule:** an upstream method's *checks* and its *transformations* are separate artefacts. Copy the check; verify the transformation against your own corpus.

### 12.4 The real defect was bigger than the one the proposal was written for

The proposal was scoped around one defect: a skill with no `description:` is silently dropped at load. Auditing the live tree for that defect found **1 instance in 17 files** — preventive, not corrective, as the proposal said.

The same audit turned up a defect the proposal did not mention, and it is larger:

| Inventory | Count | Populated by |
|---|---|---|
| `~/.loom/.cache/snapshot.db` `skills` table | **11** | `crates/loom-discovery/src/initializer.rs:56-70` (walks dirs with `SKILL.md`) |
| `~/.loom/skills/skills.db` (the `SkillRegistry`) | **0** | nothing — the only `registry.upsert` call site is `crates/loom-adapters/src/lib.rs:356`, which is the **MCP** import path |
| `~/.loom/skills/skills.lock` | **0** | `crates/loom-cli/src/commands/lock.rs` reads `registry().list()` — the empty registry |

So `loom skills list` returns `[]`, `loom skills show <name>` returns `{"found": false}`, and `loom lock --generate` writes zero entries — **while eleven skills are installed.** The daemon (`crates/loomd/src/ipc_handler.rs:68`) and the MCP server (`crates/loom-mcp-server/src/lib.rs:245`) serve the same empty registry, and `crates/loom-discovery/src/lib.rs:128` even calls the registry *"source of truth, not filesystem"* while counting directories from the filesystem in `live_counts` (`:139-149`).

Two further failures in the same subsystem, both **source-verified** against `std`:

- `loom skills install <dir>` — `std::fs::copy` on a directory fails with *"the source path is neither a regular file nor a symlink to a regular file"*, so **the only install path cannot install the layout-A form at all** (`crates/loom-cli/src/commands/skills.rs:161-176`).
- `loom skills remove <dir-skill>` — `std::fs::remove_file` on a directory fails with *"Operation not permitted (os error 1)"* and leaves the directory behind (`:177-186`).

**Resolved:** Phase A is now two halves, **A0 (inventory)** and **A1 (validation)**, and A0 comes first because the validator's output has to be *visible* to be worth anything. A0 costs roughly one extra day; A0 is corrective, A1 is preventive.

### 12.5 The dead frontmatter parser was worse than "unwired"

`crates/loom-skills/src/neutral_renderer.rs` (373 lines) was described as *the tree's only frontmatter parser, not declared in `lib.rs`, no `serde_yaml` dependency — so it is never compiled*. Three further facts:

- **It has zero callers.** `grep -rn "NeutralRenderer" crates/ packages/` matches only the file itself.
- **It does not compile.** Copied out and compiled standalone, it fails with *"character literal may only contain one codepoint"* (`neutral_renderer.rs:198`) and *"mismatched closing delimiter: `}`"* (`:354`).
- Therefore `cargo check -p loom-skills` passes only because the module is never declared — a green build is not evidence the crate is sound.

**Resolved:** **delete**, do not revive. "Declare it and use it" would have meant fixing two syntax errors in 373 lines of unreferenced code to obtain a parser that the new validator would immediately replace. The validator's own parser is a few lines against `serde_yaml`.

### 12.6 Smaller findings folded into the plan

- **The corpus is bigger than the proposal said, and the counts changed.** `~/.loom/skills/` holds 17 files (8 real skill directories, 3 symlinked directories, 6 flat `.md`), 11 distinct skills. The 6 flat files are **byte-identical** to the 6 corresponding `SKILL.md` files — six duplicate effective names, visible in `~/.opencode/skills/` as a directory and a file side by side. The proposal's "1 fatal defect in 14 real skills" is now "1 fatal, 0 name mismatches, 6 duplicates, 1 missing script" over 17 files. The missing script is real: `~/.loom/skills/open-design/SKILL.md:156-160` references `$SKILL_DIR/scripts/list-design-templates.mjs` and two siblings, and that directory contains only `SKILL.md`.
- **The description-length claim was wrong.** The proposal said the longest description was ~1,400 characters (`ui-ux-pro-max`); the measured value is **729**. That is why the trigger cue needs a *cap*, not a truncation policy on a fixed budget.
- **A quoted `name:` must not read as a mismatch.** `name: "graphflow"` (a real skill in `~/.agents/skills`) yields `"graphflow"` with the quotes under a naive regex. The proposal now has a dedicated acceptance criterion.
- **Unknown frontmatter keys are legal.** The live corpus uses `triggers`, `argument-hint`, and `license` — the parse must tolerate them, not reject them.
- **`SkillManager` and `SkillLoader` are dead.** `crates/loom-skills/src/manager.rs`'s `install`/`remove` are stubs returning `Ok(())` with zero callers, and `loader.rs`'s `refresh_metadata` is never called. The new walk supersedes both; nothing in the plan depends on them.
- **`copy_dir_recursive` is dead code.** `crates/loom-adapters/src/lib.rs:276` is `#[cfg(not(unix))]` — it cannot run on macOS or Linux.
- **One real bug in the copy path:** `inject_shared_skills` (`crates/loom-cli/src/commands/exec.rs:518-560`) handles **flat `.md` only**, while `sync_skills_to_native` (`crates/loom-adapters/src/lib.rs:227-273`) handles **directories only**. The two paths that are supposed to agree about a skill do not even look at the same files. The plan now wires the same validator into both.

### 12.7 What the review did not change

- The direction (adopt mechanisms, not the system) and the phase order.
- The deferral of Phases C, D, and E behind their measurements.
- The one-owner rule for lessons (pillar 5), and the cost figures for Phases B–E.
- The verdict on Harness² itself: a source of mechanisms, not a dependency.

**The one number that moved is the cost of Phase A: 2–3 days → 3–4 days**, and it moved because A0 was added — not because the estimate was padded. The honest summary of the slice is: **A0 is corrective, A1 is preventive, and A0 is what a user notices.**

---

## Sources

**Harness²** — `github.com/google-research/harness2` v0.1.0 @ `1cfe254`, Apache-2.0, Google LLC: `harness2/core/tree.py:40` (`SurfaceSpec`), `:82` (`normalize_rel`), `:92` (`is_allowed`), `:336` (`validate`); `harness2/substrate/opencode.py:42` (`OPENCODE_SURFACE`), `:140` (`compose_prompt`), `:181` (install modes); `harness2/substrate/codex.py:41` (Codex surface); `harness2/core/recursion.py:343,452,569`; `harness2/core/selection.py:33` (choice regex), `:71` (`assign_ids`), `:79` (`_anon_id`), `:41` (`order_graded_first`); `harness2/core/judge.py:33` (`Scored`), `:47` (`graded`); `harness2/core/library.py:43` (`has_experience`), `:106` (`add`), `:79` (`context_dir`), `:154` (`rebuild_index`); `harness2/core/context.py:105` (`build`), `:399` (`assert_no_leak_paths`), `:454` (`assert_no_grader_verdicts`), `:521` (`mission`); `harness2/core/ledger.py:26,31`; `harness2/core/prompts.py:424,475` (component tables); `harness2/core/proposer.py:39` (`SYSTEM_ARG_MAX`), `:47` (dropped `ANTHROPIC_*` env); `harness2/core/runmark.py:41` (`fingerprint`); `harness2/core/spec.py` (`ChannelSpec`, `BenchmarkAdapter`); `docs/SETUP.md`, `docs/BENCHMARKS.md`, `docs/EXPERIMENTS.md`; README (component list, reported gains).

**Loom** (v0.9.6) — `crates/loom-adapters/src/lib.rs:36` (`LoomToolAdapter`), `:169` (`sync_mcp_to_native`), `:227` (`sync_skills_to_native`), `:334` (`supported_tools`); `crates/loom-adapters/src/{opencode,codex}.rs` (`default_toml` native paths); `crates/loom-generic-adapter/src/config.rs:34` (`ToolToml`); `crates/loom-cli/src/commands/exec.rs:263` (`build_base_env`), `:411` (`inject_shared_resources`); `crates/loom-cli/src/cli.rs:19` (`Command`), `:1773` (`AcpAction`); `crates/loom-cli/src/commands/resume.rs:33` (memory injection); `crates/loom-skills/src/{registry,loader,hot_reload,neutral_renderer}.rs`; `crates/loom-hindsight/src/lib.rs:33,64,136,167,221`; `crates/loom-knowledge/src/lib.rs:16,80,150,176,244`; `crates/loom-core/src/types.rs:769,806,859`; `crates/loom-core/src/plugin_registry.rs:72`; `crates/loom-hooks/src/lib.rs:58,64`, `crates/loom-hooks/src/executor.rs:87`; `crates/loom-rules/src/lib.rs:234,515`; `crates/loom-parallel/src/pool.rs:87,102`, `crates/loom-parallel/src/worktree.rs:63`; `crates/loom-runtime/src/lib.rs:61,182,249`; `crates/loom-sessions/src/acp_protocol.rs:24,39,206`, `crates/loom-sessions/src/{store,exporter,memory}.rs`; `crates/loom-routing/src/outcomes.rs:35,101,137,155`; `crates/loom-telemetry/src/lib.rs:17,73`; `crates/loom-workflow/src/chief/{harness,plan,dag,scheduler}.rs`; `ROADMAP.md`; `docs/plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md`; `docs/plan/LOOM_HARNESS_COMPARISON_ENHANCEMENT.md`.
