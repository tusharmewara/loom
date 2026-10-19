# Routing debt & decision-model review record — handoff for a fresh session

*Carries the outcome of a full Technical Review Team + Software Architect review
of Loom's task-complexity module (loom-routing) so a new session can close the
debts below without re-deriving anything. All file/line refs re-verified against
the current tree; recent housekeeping commits (fmt/clippy/cross-crate dedupe)
did not shift them.*

## Decision record — NO on decision-model support (for now)

Question (owner): should Loom add Jev, Laya, and similar decision models to the
task-complexity module or anywhere else?

**Verdict: NO — now.** Unanimous (Software Architect + 3-member review team:
architecture, security, QA), then re-tested against the owner's sources —
verdict unchanged.

Model landscape (fetched and read, 2026-10):

| Model | What it is | Local? | Verdict for Loom |
|---|---|---|---|
| Laya (github.com/NandhaKishorM/laya) | System-1 decision engine; choice/score/noul primitives; Apache-2.0 weights (421M English / 322M multilingual); 33ms/decision GPU, ~100ms CPU; preset `router_questions()` matches our use case | yes | Only credible future candidate. Blocked: near-chance zero-shot base (0.362; 0.766 only after fine-tuning on labeled decisions we don't have), ~1.6GB weights + torch/ONNX dep, misses the <10ms budget 3–4x even on GPU |
| Jev (thejevai.com) | Closed API, $0.042/1M tokens, 236–276ms p50; `POST /v1/systemone {model:"jev-latest", state, questions}` | no | Structurally disqualified: hosted → every prompt leaves the machine (red line); per-call network vs <10ms budget |
| Microsoft-Decision-1 (Foundry / OpenRouter) | Hosted decision-scoring model | no | Same disqualification as Jev |
| Prism (hyper.charm.land) | Hosted general chat LLM, $3.27/$16.33 per M tokens | no | Category error for decision support; at most a one-line entry in loom/crates/loom-routing/src/catalog.rs |

Structural facts behind the verdict:

- Prompts never leave the machine today (pure local text scoring, no network).
  Hosted decision models break this red line; the hooks/plugins path (arbitrary
  script execution, signatures optional) is a blocking way to add models.
- Routing decision budget is <10ms (~5ms measured). Every candidate misses it.
- No outcome telemetry exists — loom/crates/loom-routing/src/metrics.rs is
  in-process AtomicU64 counters only, nothing persisted — so any model
  comparison is unfalsifiable. The premise (current heuristic misroutes) is
  unproven.
- `ComplexityScore` (loom/crates/loom-core/src/types.rs:476-483) is the seam:
  any future model emitting {level, numeric, model_tier, signals} slots into
  loom/crates/loom-routing/src/scorer.rs alone; the 7 consumers stay untouched.
  The vendor category is converging on one contract (Jev's
  {state, questions{choice|score|noul}} ≈ Laya's predict()), so target the
  contract, never a vendor.
- Both vendors prescribe eval-set-first (Jev guide: build an eval set of real
  traffic, validate in the playground BEFORE integration; Laya ships a
  `laya-evals` gating harness). External corroboration of our flip conditions.

Flip conditions — ALL required before revisiting:

1. Verifiable model + I/O contract (now exists for Laya/Jev).
2. ≥20-case golden prompt set from real Loom traffic, labeled.
3. Telemetry evidence that the current heuristic actually misroutes.
4. Deterministic in-process decision <10ms. (No candidate meets this today.)

Lazy path if ever flipped: golden set → run stock Laya against it through
`laya-evals run data.jsonl --min-accuracy 0.8 --max-ece 0.05` → only if it
beats the current heuristic, wrap as a Stage-3 hook script in the
loom/packages/routing-hook pattern (process-isolated, advisory, config-gated,
reversible). Never a trait/registry/dynamic loading in core.

## Debt list — all six verified still valid (2026-10)

Close in this order. Test conventions per loom/AGENTS.md: unit tests inline
`#[cfg(test)]`, `tempfile::tempdir()` never real fs paths, integration tests in
loom/crates/loom-integration-tests/tests/, run `cargo test -p loom-routing -p
loom-integration-tests` (no CI exists — run locally), routing decision must
stay <10ms.

### Step 0 — commit ROADMAP.md (trivial, do first)

`git status` shows `?? ROADMAP.md` — untracked; one careless `git clean` and
the telemetry plan (loom/ROADMAP.md:54) is gone. `git add ROADMAP.md` + commit.

### Step 1 — scorer.rs:79 counts bytes, not chars

`let description_length = prompt.len();` — `str::len()` is BYTES, so CJK
prompts inflate the length signal ~3x. Fix: `prompt.chars().count()`. This
deliberately shifts CJK scores down (correct length semantics).
Acceptance: unit test — equal visual-length ASCII and CJK prompts yield equal
length signals.

### Step 2 — missing tests (pin CURRENT behavior before changing anything)

- **routing_hook.py** (loom/packages/routing-hook/scripts/routing_hook.py) has
  zero tests (only file in that package). Contract tests, plain asserts, no
  harness: empty stdin → `{}`; malformed TOML rules file → `{}` no-op
  (:46-48); rule hit → `{adjust_tier, reason}`; last-match-wins (:105-108).
  Use `tempfile` for the rules file, never `~/.loom/config` for real.
- **user_override** (loom/crates/loom-routing/src/scorer.rs:200-202) untested.
  Assert `Some(High)` → level High regardless of numeric, override visible in
  `ComplexityScore.signals`.
- **Adversarial keyword gaming** untested. Pin the reachable ceiling:
  keyword_score = 0.5 base +0.15/high −0.1/low, clamped [0,1], weight 0.35 →
  max static contribution 0.35; High needs ≥0.7. Verify keywords alone cannot
  force Flagship on the static path, and document what the knowledge-graph
  boost (+0.0–0.25, loom/crates/loom-cli/src/commands/routing.rs:168-173) and
  dynamic signals can add. Test what is actually true, not what we assumed.

### Step 3 — level-vs-tier governance (root cause: derivation duplicated ≥4x)

numeric→level uses hardcoded cutoffs at loom/crates/loom-routing/src/scorer.rs:205-211
(`< 0.35` Low, `< 0.7` Medium); numeric→tier is re-derived from configurable
`complexity_thresholds` at loom/crates/loom-routing/src/router.rs:36-42,
ignoring score.level; the same derivation is duplicated in
loom/crates/loom-workflow/src/task_manager.rs:199-213 and :350-364 and
loom/crates/loom-cli/src/commands/routing.rs:174-185 and :230-241.
Live contradiction today: config `low_max=0.3` + numeric=0.32 → level=Low but
tier=Mid.

Fix once, at the root: one `derive_level(numeric, thresholds)` +
`derive_tier(level)` pair (loom-core or loom-routing, exported), all four sites
route through it; config moves level cutoffs too — which also makes the
Supplement's "configurable" claim true (Step 4).
Acceptance: the contradiction case scores consistently everywhere; threshold-
config test added (today only the 0.5 boundary is tested);
`cargo test -p loom-routing -p loom-integration-tests` green.

### Step 4 — Supplement spec divergences (doc sync, after Step 3)

loom/planning/Intelligent_Task_Routing_Supplement.md:60 says keyword weight
0.25, code uses 0.35 (scorer.rs:153). :71 says the low band is 0.0–0.3, code
cuts at 0.35. :71 claims "These thresholds are configurable in config.toml" —
true for tier thresholds only, until Step 3 lands. Fix the two numbers to
match shipped code (code is the behavioral truth); the configurability claim
becomes true via Step 3 or gets reworded.
Acceptance: no known Supplement-vs-code contradiction remains.

### Step 5 — security P1

- loom/crates/loom-core/src/plugin_registry.rs:116 `ureq::get(url)` accepts
  plain http:// (remote-config loading at loom/crates/loom-core/src/config.rs:269
  forces HTTPS — inconsistent). Fix: reject non-https in `install_from_url`.
  Acceptance: http:// URL errors.
- Signatures optional: loom/crates/loom-hooks/src/executor.rs:123 verifies only
  `if hook.signature.is_some()`; plugin_registry.rs:36 `(None, None) => Ok(())`
  allows unsigned. Product decision needed: require signatures (breaking) vs a
  config flag `require_signature` (default off, back-compat) + document the
  trust model. Lazy default: the flag.

### Step 6 — outcome telemetry (the big one; likely its own initiative)

No persisted (signals → decision → outcome) record exists anywhere.
loom/ROADMAP.md:54 plans usage metering (per-run/per-tool/per-token accounting)
— that covers token usage, NOT the outcome leg of Supplement §8's
(complexity, token usage, outcome) triple. Without outcomes, no scorer —
current heuristic or future model — can be judged; this is also flip-condition
#3 for any decision-model reconsideration.

Scope: persisted decision+outcome record. Outcome definition candidates: task
completed without manual tier override or re-dispatch; token overrun vs
estimate. SQLite pattern already exists in-repo (loom-knowledge graph.db,
mcp_registry.db). Design first, then implement — do not bolt onto metrics.rs.

## Housekeeping notes

- Recent commits are housekeeping only (9e118fa cargo fmt, 358df30 cross-crate
  dedupe, 2b85cce clippy, 4b40f7a ci protoc) — no routing-logic changes since
  the review; refs above are current.
- STATUS.md and docs/loom-improvements.md have uncommitted modifications in the
  working tree; check `git status` before starting.
- Repo memory (Hindsight) API is broken in the reviewing session (401: no
  apiToken configured; search GET also failing). This file is the durable
  record until that config is fixed.
