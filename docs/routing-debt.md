# Routing debt & decision-model review record — handoff for a fresh session

*Carries the outcome of a full Technical Review Team + Software Architect review
of Loom's task-complexity module (loom-routing) so a new session can close the
debts below without re-deriving anything. The diagnosis and the file/line refs
in it are kept **as written at review time (2026-10-17)** — where the code has
since moved, each step's **Landed:** paragraph gives the current location.*

> **CLOSED 2026-10-21 — this is a historical record, not live work.**
>
> **What closed it:** all six routing debts below landed on `main` (Step 0 is the
> trivial pre-step that tracked `ROADMAP.md`; Steps 1–6 are the six debts). Every
> step landed as a `debt/step*` branch merged through a pull-request merge, and
> each branch head was a single commit:
>
> | Step | Branch (`debt/`) | Branch head | Merged on `main` |
> |---|---|---|---|
> | 0 | `step0-roadmap` | `868cf09` | `02f936c` (PR #1) |
> | 1 | `step1-char-count` | `f4a5f61` | `faa99da` (PR #2) |
> | 2 | `step2-characterization-tests` | `fc62ae6` | `6991e91` (PR #3) |
> | 3 | `step3-unify-derivation` | `eb1338e` | `11da566` (PR #4) |
> | 4 | `step4-doc-sync` | `ea03d0e` | `0ee6d58` (PR #5) |
> | 5 | `step5-https-and-signatures` | `7dba77c` | `e152553` (PR #6) |
> | 6 | `step6-outcome-telemetry` | `cafb014` | `4d8c9c8` (PR #7) |
>
> Step 4 additionally re-synced the Supplement to the shipped constants — that
> re-sync lives in the working tree only, because the Supplement sits under the
> gitignored `planning/` path. Each step keeps its original diagnosis and now
> records what landed and where.
>
> The **decision record** (NO on Jev/Laya/Microsoft-Decision-1/Prism) and its
> **flip conditions** are unchanged and still binding — closing the debts does
> not reopen the decision-model question. Open routing work now lives in
> [loom-improvements.md](loom-improvements.md).

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
| Prism (hyper.charm.land) | Hosted general chat LLM, $3.27/$16.33 per M tokens | no | Category error for decision support; at most a one-line entry in `crates/loom-routing/src/catalog.rs` |

Structural facts behind the verdict:

- Prompts never leave the machine today (pure local text scoring, no network).
  Hosted decision models break this red line; the hooks/plugins path (arbitrary
  script execution, signatures optional) is a blocking way to add models.
- Routing decision budget is <10ms (~5ms measured). Every candidate misses it.
- No outcome telemetry exists — `crates/loom-routing/src/metrics.rs` is
  in-process AtomicU64 counters only, nothing persisted — so any model
  comparison is unfalsifiable. The premise (current heuristic misroutes) is
  unproven. *(As of the review. Outcome telemetry landed 2026-10-21 — Step 6;
  the persisted store is `crates/loom-routing/src/outcomes.rs`. The premise of
  the verdict is therefore now measurable, but the NO itself still stands: the
  hosted candidates break the local-only red line and miss the <10ms budget.)*
- `ComplexityScore` (`crates/loom-core/src/types.rs:533-540`) is the seam:
  any future model emitting {level, numeric, model_tier, signals} slots into
  `crates/loom-routing/src/scorer.rs` alone; the 7 consumers stay untouched.
  *(As of the review that was 7 files. Step 6 added two more consumers —
  `crates/loom-routing/src/outcomes.rs` and
  `crates/loom-workflow/src/task_manager.rs` — so it is 9 today; the seam is
  unchanged, both consume the struct rather than adding fields to it.)*
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
`packages/routing-hook` pattern (process-isolated, advisory, config-gated,
reversible). Never a trait/registry/dynamic loading in core.

## Debt list — all six closed 2026-10-21 (originally verified valid 2026-10)

The diagnosis below is kept as written when the review was made; each step now
records what actually landed and where. The ordering constraint ("close in this
order") is preserved for provenance only — the work is done.

Test conventions per `AGENTS.md`: unit tests inline
`#[cfg(test)]`, `tempfile::tempdir()` never real fs paths, integration tests in
`crates/loom-integration-tests/tests/`, run `cargo test -p loom-routing -p
loom-integration-tests` (CI: `.github/workflows/ci.yml` runs
`cargo test --workspace --exclude loom-tui` on push and PRs — this line was
stale when the record was written), routing decision must stay <10ms.

### Step 0 — commit ROADMAP.md (trivial, do first) — DONE

`git status` shows `?? ROADMAP.md` — untracked; one careless `git clean` and
the telemetry plan (`ROADMAP.md:54`) is gone. `git add ROADMAP.md` + commit.

**Landed:** `ROADMAP.md` is tracked on `main` (commit `868cf09`, merged via
`debt/step0-roadmap`); `git ls-files ROADMAP.md` resolves.

### Step 1 — scorer.rs:79 counts bytes, not chars — DONE

`let description_length = prompt.len();` — `str::len()` is BYTES, so CJK
prompts inflate the length signal ~3x. Fix: `prompt.chars().count()`. This
deliberately shifts CJK scores down (correct length semantics).
Acceptance: unit test — equal visual-length ASCII and CJK prompts yield equal
length signals.

**Landed:** `crates/loom-routing/src/scorer.rs:83` —
`let description_length = prompt.chars().count();` (the refactor moved the
line from 79 to 83). Parity test: `fn description_length_counts_chars_not_bytes`
at `crates/loom-routing/src/scorer.rs:291`.

### Step 2 — missing tests (pin CURRENT behavior before changing anything) — DONE

- **routing_hook.py** (`packages/routing-hook/scripts/routing_hook.py`) has
  zero tests (only file in that package). Contract tests, plain asserts, no
  harness: empty stdin → `{}`; malformed TOML rules file → `{}` no-op
  (:47-49); rule hit → `{adjust_tier, reason}`; last-match-wins (:104-110).
  Use `tempfile` for the rules file, never `~/.loom/config` for real.
- **user_override** (`crates/loom-routing/src/scorer.rs:205-210`) untested.
  Assert `Some(High)` → level High regardless of numeric, override visible in
  `ComplexityScore.signals`.
- **Adversarial keyword gaming** untested. Pin the reachable ceiling:
  keyword_score = 0.5 base +0.15/high −0.1/low, clamped [0,1], weight 0.35 →
  max static contribution 0.35; High needs ≥0.7. Verify keywords alone cannot
  force Flagship on the static path, and document what the knowledge-graph
  boost (+0.0–0.25, `crates/loom-cli/src/commands/routing.rs:59`) and
  dynamic signals can add. Test what is actually true, not what we assumed.

**Landed:** 7 contract tests in
`packages/routing-hook/scripts/test_routing_hook.py` (empty stdin, malformed
rules file, rule hit, last-match-wins, and three more).
`fn user_override_forces_level_regardless_of_numeric` at
`crates/loom-routing/src/scorer.rs:322`;
`fn keywords_alone_cannot_force_flagship` at
`crates/loom-routing/src/scorer.rs:344` (body :345-363 — asserts Medium/Mid and
`score.numeric >= 0.35 && score.numeric < 0.7`, pinning the 0.35 keyword
ceiling against the 0.0–0.25 knowledge boost).

### Step 3 — level-vs-tier governance (root cause: derivation duplicated ≥4x) — DONE

numeric→level uses hardcoded cutoffs at `crates/loom-routing/src/scorer.rs:205-210`
(`< 0.35` Low, `< 0.7` Medium); numeric→tier is re-derived from configurable
`complexity_thresholds` at `crates/loom-routing/src/router.rs:37`,
ignoring score.level; the same derivation is duplicated in
`crates/loom-workflow/src/task_manager.rs:246` and `:405` and
`crates/loom-cli/src/commands/routing.rs:179` and `:230`.
Live contradiction today: config `low_max=0.3` + numeric=0.32 → level=Low but
tier=Mid.

Fix once, at the root: one `derive_level(numeric, thresholds)` +
`derive_tier(level)` pair (loom-core or loom-routing, exported), all four sites
route through it; config moves level cutoffs too — which also makes the
Supplement's "configurable" claim true (Step 4).
Acceptance: the contradiction case scores consistently everywhere; threshold-
config test added (today only the 0.5 boundary is tested);
`cargo test -p loom-routing -p loom-integration-tests` green.

**Landed:** `pub fn derive_level(numeric: f64, thresholds: &ComplexityThresholds)
-> ComplexityLevel` at `crates/loom-core/src/types.rs:175` and
`pub fn derive_tier(level: ComplexityLevel) -> ModelTier` at
`crates/loom-core/src/types.rs:188`. All four former duplicate sites route
through them: `crates/loom-routing/src/router.rs:37`,
`crates/loom-workflow/src/task_manager.rs:246,248` and `:405,407`,
`crates/loom-cli/src/commands/routing.rs:179-180` and `:230-231`. The
contradiction case is now pinned by
`fn custom_thresholds_resolve_contradiction_case` at
`crates/loom-core/src/types.rs:1109`; threshold defaults
(`low_max` 0.35, `high_min` 0.7) at `crates/loom-core/src/types.rs:162,166`.

### Step 4 — Supplement spec divergences (doc sync, after Step 3) — DONE

The Supplement's keyword weight was 0.25, code uses 0.35 (`scorer.rs:157`).
*(The review cited `:60` for this divergence, but the weights table at `:60`
already said 0.35 — the stale 0.25 was in the pseudocode, at `:566` before the
re-sync and at `:621` after it.)* The low band was 0.3, code cuts at 0.35 —
again not in the `:71` prose the review cited, which already read 0.35/0.7, but
in the config example (`:367` before the re-sync, `:378` after it) and in the
pseudocode (`:578` before, `:633` after). :71 claims "These thresholds are
configurable in config.toml" — true for tier thresholds only, until Step 3
lands. Fix the two numbers to match shipped code (code is the behavioral
truth); the configurability claim becomes true via Step 3 or gets reworded.
Acceptance: no known Supplement-vs-code contradiction remains.

**Landed:** the Supplement is re-synced to the shipped constants — the
description-length divisor 2000 (`:619`, was 500), the keyword weight 0.35 in
the pseudocode (`:621`, was 0.25), the low band 0.35 in the config example
(`:378`, was 0.3) and in the pseudocode (`:633`, was 0.3), the 0.35/0.7 level
bands (`:71`), and the real config key path
`[routing.complexity_thresholds]` with PascalCase enum values
(`:354-382`, with the shipped-config note at `:384-396`).
The "configurable" claim is now true, because Step 3 made the level cutoffs
config-driven. *(The Supplement is under the gitignored `planning/` path and
was never tracked, so the re-sync lives in the working tree, not in this
commit.)*

### Step 5 — security P1 — DONE

- `crates/loom-core/src/plugin_registry.rs` `ureq::get(url)` accepts
  plain http:// (remote-config loading at `crates/loom-core/src/config.rs:270`
  forces HTTPS — inconsistent). Fix: reject non-https in `install_from_url`.
  Acceptance: http:// URL errors.
- Signatures optional: `crates/loom-hooks/src/executor.rs:123` verifies only
  `if hook.signature.is_some()`; `plugin_registry.rs` `(None, None) => Ok(())`
  allows unsigned. Product decision needed: require signatures (breaking) vs a
  config flag `require_signature` (default off, back-compat) + document the
  trust model. Lazy default: the flag. **Superseded 2026-10-26 by ADR 0005:**
  the flag defaults on (verify-by-default for remote plugin installs, pins
  written on signed installs); see `docs/decisions/0005-plugin-verify-by-default.md`.

**Landed:** `fn validate_https_url(url: &str) -> Result<()>` at
`crates/loom-core/src/plugin_registry.rs:9`, called by `install_from_url` (:145)
and `update` (:262); tests `fn plugin_install_urls_must_be_https` (:345) and
`fn plugin_update_urls_must_be_https` (:357). The lazy default was taken: the
`require_signature` config flag at `crates/loom-core/src/types.rs:43`, honoured
by `crates/loom-cli/src/commands/plugin.rs:112`
(`let verify = *verify || require_sig;`), by
`crates/loom-core/src/plugin_registry.rs:57-65` (unsigned is refused when the
flag is on, allowed by default) and by `crates/loom-hooks/src/lib.rs:190`
(`if require_signature && hook.signature.is_none()` — the unsigned hook is
skipped).

### Step 6 — outcome telemetry (the big one; likely its own initiative) — DONE

No persisted (signals → decision → outcome) record exists anywhere.
`ROADMAP.md:54` plans usage metering (per-run/per-tool/per-token accounting)
— that covers token usage, NOT the outcome leg of Supplement §8's
(complexity, token usage, outcome) triple. Without outcomes, no scorer —
current heuristic or future model — can be judged; this is also flip-condition
#3 for any decision-model reconsideration.

Scope: persisted decision+outcome record. Outcome definition candidates: task
completed without manual tier override or re-dispatch; token overrun vs
estimate. SQLite pattern already exists in-repo (loom-knowledge graph.db,
mcp_registry.db). Design first, then implement — do not bolt onto metrics.rs.

**Landed:** `pub struct RoutingOutcomeStore` (SQLite) at
`crates/loom-routing/src/outcomes.rs:35`, wired at
`crates/loom-workflow/src/task_manager.rs:53,63` and
`crates/loom-cli/src/commands/routing.rs:363,386` (record an override; view
outcomes). `crates/loom-routing/src/metrics.rs` is unchanged — it remains
in-process counters only, and the persisted store sits beside it rather than
inside it.

## Housekeeping notes

- At the time of the review, the recent commits were housekeeping only
  (9e118fa cargo fmt, 358df30 cross-crate dedupe, 2b85cce clippy, 4b40f7a ci
  protoc) — no routing-logic changes since. *(Since then the six debt steps
  themselves changed routing logic, so the diagnosis refs above are review-time
  refs; each **Landed:** paragraph carries the current location.)*
- STATUS.md and docs/loom-improvements.md have uncommitted modifications in the
  working tree; check `git status` before starting. *(Obsolete as of
  2026-10-21: the tree was clean when this record was closed.)*
- Repo memory (Hindsight) API is broken in the reviewing session (401: no
  apiToken configured; search GET also failing). This file is the durable
  record until that config is fixed.
