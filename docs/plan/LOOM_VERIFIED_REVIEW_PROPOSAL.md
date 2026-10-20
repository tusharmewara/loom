# Loom Verified Review Engine — Parallax comparison → build proposal

> Status: proposal, 2026-10-19. Sourced from the Conifer Parallax docs (conifer.build/docs/parallax/) compared against the Loom tree at v0.9.4.
> Feeds: [ROADMAP.md](../../ROADMAP.md) Pillar 5; competitive context in [competitive-analysis.md](../competitive-analysis.md) §11.
> Shared substrate: the outcome-gated lesson store below (§3.6) is the **one owner** of lessons in Loom. [LOOM_HARNESS2_ANALYSIS.md](../LOOM_HARNESS2_ANALYSIS.md) §6.4 references it rather than proposing a second store — the Harness² analysis proposes a *contract* (cue/lesson/evidence, evidence-gated) and a *writer* (a selection loop), not a new store. If that loop ever ships, it writes here.

## Executive summary

Parallax: when a coding-agent session ends, models from **other labs** re-do the important part **blind**; a sandbox labels every finding with what it established; one fix PR carries only verified patches; a lesson is written from every proven finding and injected into the next session touching that file; a scoreboard tracks which model is right on which task, at what cost. Hosted reviews, per-episode credits, sessions uploaded to Conifer's cloud.

Loom already owns the substrate of every stage — capture (`loom-sessions`), seating (`loom-routing` + `outcomes.rs`), blind headless dispatch (`acp_protocol.rs`), isolation (`loom-parallel` worktrees), execution (`loom-runtime`), PR creation (`loom-trackers`), lessons (`loom-knowledge` + `loom-hindsight`), telemetry (`loom-telemetry`). Two pieces do not exist: the **review orchestration itself**, and the **CI-tested container sandbox** (roadmap pillar 2).

The wedge — why this is a Loom feature, not a Parallax clone:

1. **Local-first.** The session never leaves the machine; the review runs on your daemon.
2. **Own keys.** Reviewer seats run through the vault + routing catalog — no per-episode rent, full cost visibility.
3. **Any-agent reviewers.** Any registered CLI (claude-code, codex, opencode, crush, …) — not only "other labs".
4. **MIT core, multi-forge delivery** (`loom-trackers`: github.rs, gitlab.rs).

## 1. What Parallax got right — copy these mechanics exactly

### 1.1 Evidence states (the honesty core)

| state | exact rule | × |
|---|---|---|
| `proven` | A repro test failed on the code as it was and passed with the fix. Not contested by an adversary seat. Confidence floor 0.90 regardless of stated value. | 2.0 |
| `suite_green` | Suite is green with the patch applied, but no fail-then-pass repro was supplied. | 1.0 |
| `unverified` | Nothing was established: prose judgement, no test command, contested by an adversary seat, or a patch that breaks the suite without a repro (unsafe-patch warning). | 0.8 |
| `could_not_run` | The proof could not finish and refuted nothing: missing toolchain, repo prep failure, a step that timed out, a patch that did not apply. The reason is recorded on the line. | 0.8 |
| `refuted` | The proof ran and did not hold: the test passes without the fix, or the fix fails to make it pass. | 0.2 |

Invariants to steal verbatim:

- **Verification is evidence, not a gate.** Every finding with a patch is delivered; only its label changes.
- **A cut or timed-out step is `could_not_run`, never `refuted`.**
- **Suite result is a separate note** (`green` / `not run` / `regressed`). A proven finding stays proven even if the suite later fails; a patch that regresses the suite never gets a one-click suggestion and never enters the fix PR.
- **Perf claims are measured**: a faster rewrite counts only if the benchmark shows it and the suite still passes. Dead code counts only if removal leaves the suite green.

### 1.2 Mechanism dedup (findings are mechanisms, not copies)

Two findings are one mechanism when any of these holds:

- same patch: same file, same replaced text, same replacement (whitespace aside);
- patches replace the same range: same file, shared non-trivial line (≥12 chars trimmed);
- same root cause in prose (symbol overlap ≥0.6, or title+reason Jaccard ≥0.5, over ≥6 distinct words) **and** share at least one file.

One mechanism carries every seat/lens that named it and the **strongest** evidence any copy earned; it is suggested once. The headline counts mechanisms, never copies.

### 1.3 Confidence and seat calibration

```
confidence = min(1, stated × seat_calibration × state_multiplier)
if proven:  confidence = max(confidence, 0.90)   # floor
seat_calibration (per agent×lens, from your own outcomes, default 0.5):
  score = (proven − contested + settled + 2·merged + 1)
        / (proven + settled + unsettled + rejected + 2·merged + closed + 2)
```

A merged fix counts twice in both places; a closed one counts against. A seat overtakes the routing pick on a lens only after **3 decided findings** there and a **+0.15** edge — calibration moves slowly, only in the direction of your own merges and closes.

### 1.4 Fix-PR policy

At most one follow-up per source PR (ours: branch `loom/review/<number>`), refreshed as the source head changes. While the source PR is open in the same repo, the fix PR targets its head branch; after merge (or from a fork) the base branch; a source PR closed unmerged gets none. One commit per admitted mechanism. Router rank picks among competing patches for a mechanism; evidence then confidence break ties. Refuted and suite-regressing patches are never chosen. Never reopen a closed fix PR, never overwrite a maintainer's commit, never merge it for the user. Optional `deliver = "proven-only"`.

### 1.5 Lessons are outcome-gated

Only these create a lesson: a sandbox-proven finding; a merged fix PR; a suggestion the maintainer applied; a recurrence across sessions (2× the same corrected tool call, 3× the same passing check). Reviewer consensus, rejected findings, and single uncorrected events never do. A lesson is 1–2 imperative sentences naming the file or function; repo-scoped, widened to the account only when a second repo confirms it; **expires 30 days after its last confirmation**, which a later proven finding of the same code extends instead of duplicating. A maintainer dismissal suppresses it permanently, with the reason. Every injection writes a receipt.

### 1.6 Repo config surface (`parallax.yml` → our `review.toml`)

Keys: `test`, `setup`, `check` (list), `bench`, `lenses`, `severity_floor`, `ignore` (globs), `deliver`, `comment`, `review` (off switch). The test command is **guessed from root markers when absent** — `package.json` → `npm test`, `Cargo.toml` → `cargo test`, `pyproject.toml`/`pytest.ini` → `pytest -q`, `go.mod` → `go test ./...` — and the report says `"guessed"`. `bench` is never guessed. Review rules: free text, else the `## Code Review Rules` section of `AGENTS.md`.

## 2. Stage map — Parallax stage → Loom crates

| Stage | Parallax | Loom today | New work |
|---|---|---|---|
| Capture | agent hooks, upload at session end | `loom-sessions` (`messages.jsonl` via `acp.dispatch`, `--capture-session`, export/import); event `SessionExported` | trigger, no capture work |
| Review plan | intent + diff from transcript | — | `review_plan_from_session()`: user intent + diff at session head; **never the agent's reasoning** (blind) |
| Reviewer seating | router, other labs, by proven scoreboard | `loom-routing` (complexity tiers, `outcomes.rs` telemetry) | cross-family rule; per-agent×lens calibration |
| Blind re-do | seats × lenses × shards | `loom acp run` headless dispatch; `loom-parallel` worktrees | lens prompts + structured findings contract (JSON out) |
| Lenses | bugs+security / perf / hygiene / drift + mechanical dead-code scan | `loom-ast` (tree-sitter) | dead-code scan on `loom-ast`; perf lens gated on `bench` |
| Verification | sandbox, throwaway repo copy | `loom-parallel` worktrees; `loom-runtime` process→container | evidence runner (§3.4) |
| Dedup + confidence | verdict cards | — | port rules §1.2–1.3 |
| Delivery | fix PR + line comments + check summary | `loom issues create-pr/diff/ci-status` | fix-PR policy §1.4; per-line suggestions |
| Lessons | recall store, session-start brief | `loom-knowledge` + `loom-hindsight` (`loom memory retain/recall`, `--inject-knowledge`) | outcome-gated `lesson` entities + receipts + suppression |
| Scoreboard | per seat×lens calibration | `loom routing outcomes` (tier-level) | per-agent×lens scores; feeds seating |
| Control | `parallax.yml` + env switches | 5-layer config, `config/*.toml` overlays | repo-root `review.toml` reader |
| Surfaces | GitHub checks, console | TUI Review tab, CLI | `loom review` command group; Review tab shows findings |

## 3. Design

### 3.1 New crate: `loom-review`

Library crate owning pipeline types + orchestration; composes existing crates, owns no new infrastructure. State under `~/.loom/review/runs/<id>/`: `plan.json`, `findings.jsonl`, `verify.jsonl`, `report.md`. Daemon IPC commands: `review.run`, `review.status`, `review.findings`.

### 3.2 CLI surface

```
loom review run <session-id> [--seats N] [--lenses ...] [--dry-run]
loom review status [run-id]
loom review findings [run-id] [--state proven|suite_green|unverified|could_not_run|refuted]
loom review fix-pr <run-id>          # MVP: human-triggered
loom review scoreboard [--lens ...]
loom review lesson list|confirm|dismiss  # MVP: manual write-back
```

### 3.3 Config

Repo-root `review.toml` (checked into the repo, per-PR overridable), falling back to the `~/.loom/config/review.toml` overlay — the existing 5-layer merge already deep-merges it:

```toml
test = "cargo test"                       # or guessed (§1.6); report says "guessed"
setup = ""                                # once, before test (npm ci, pip install …)
check = ["cargo clippy --workspace -- -D warnings"]   # precheck before reviewers read
bench = ""                                # never guessed; gates the perf lens
lenses = ["bugs_security", "perf", "hygiene", "drift", "dead_code"]
severity_floor = "low"                    # below → held in report, never suggested
ignore = ["target/**", "dist/**"]
deliver = "labeled"                       # "labeled" | "proven-only"
comment = true                            # per-line suggestions on the PR
review = true                             # false opts this repo out
```

### 3.4 Evidence runner (the honest core)

Per finding patch, in a fresh worktree at the reviewed commit:

1. apply patch — fail → `could_not_run "patch did not apply"`;
2. reviewer's repro: fails without fix + passes with fix → `proven`;
3. suite → note `green` / `not run` / `regressed` (regressed → excluded from fix PR, no suggestion);
4. dead-code claim: remove the symbol → suite green → `proven`;
5. perf claim: `bench` before/after — measurably faster + suite green → `proven`.

Clocks: per-seat **inactivity clock only** (no wall clock locally); a cut step is `could_not_run`, never `refuted`. Every reason lands in `verify.jsonl`. ProcessRuntime first; ContainerRuntime becomes the default verification path once pillar 2 hardens it (per-run network policy).

### 3.5 Seating + scoreboard (`loom-routing` extension)

Seat candidates = registered adapters whose family ≠ the session's agent family (cross-family rule; Parallax's "never the lab that did the work"). Rank by complexity tier, then per-agent×lens calibration (§1.3), cost from the routing catalog. `loom routing outcomes` gains per-agent×lens columns — the same data is pillar-4 usage-metering input. This is also the moat: per-agent×lens calibration **on your repositories** is switching-cost data a hosted reviewer cannot replicate.

### 3.6 Lessons (`loom-knowledge` + `loom-hindsight`)

New `lesson` entity type: 1–2 imperative sentences, file/symbol refs, receipts. Sources, expiry, suppression, and account-widening per §1.5. Injection: existing `--inject-knowledge` on resume + session-start brief; a lesson naming a file is injected only when the session touches that file. Write-back: manual in MVP, automated from PR head/close events via `loom-trackers` in v1.

**This is the only lesson store in Loom.** [LOOM_HARNESS2_ANALYSIS.md](../LOOM_HARNESS2_ANALYSIS.md) §6.4 proposes the same location and adds two upstream rules to this contract — every lesson carries a `cue` (when it applies) and non-empty `evidence` (what was observed) besides the `lesson` text, and a lesson with no evidence is not storable. It proposes no second store, no `~/.loom/experience/` directory, and no separate index; a second store would be a duplicate owner.

### 3.7 Fix PR (`loom-trackers` extension)

Policy per §1.4. GitHub first (`gh`), GitLab via `gitlab.rs`. One commit per admitted mechanism, evidence attached to the commit body.

## 4. Sequencing

**MVP** *(gate: pillar 1 complete; no container dependency)*

1. `loom-review` crate: plan extraction → 2 cross-family seats over `loom acp run` → findings contract → dedup → worktree evidence runner (ProcessRuntime) → labels + `report.md`.
2. `loom review run/status/findings`; fix PR via existing `loom issues create-pr`, human-triggered.
3. Lessons: proven findings → `lesson` entities, manual confirm/dismiss, injected via `--inject-knowledge`.

*Not in MVP:* adversary seats, calibration-based seating (tier routing suffices), write-back automation, perf lens (needs `bench`), containers, PR-event triggers, hosted anything.

**V1** *(gate: pillar 2)* — ContainerRuntime as default verification path (per-run network policy); PR-open / `SessionExported` triggers; complete fix-PR policy; automated write-back; per-line suggestions; calibration + scoreboard-driven seating.

**V2** *(aligned with pillar 4)* — team-tier review policies + shared scoreboard; hosted relay (execution stays local — self-host-first posture, roadmap open question 3); the review's own memory (tool paths that passed shared across seats, appended after shard text so the shared header stays a cache prefix); sweep modes (immediate / nightly).

## 5. Commercialization (the point of pillar 5)

- **Demand driver.** A review is a metered multi-seat workload (seats × lenses × shards) through the routing catalog — the flagship consumer of pillar-4 usage metering, and the reason a team adopts the vault/key broker: every reviewer call is a scoped, audited, metered grant.
- **Moat.** The scoreboard (per-agent×lens calibration on your repos, from your merges/closes) lives in `loom-routing` outcomes — not in anyone's cloud.
- **Wedge vs Parallax.** Local-first (no session uploads), own keys (no per-episode credits; cost transparency from catalog pricing), any-agent reviewers, MIT core.
- **Monetization surface (pillar 4).** Team tier: shared review policies, org scoreboard, audit rollup. Hosted relay for teams. Key broker proxies every reviewer seat.
- **Anchors.** Conductor $50–60/mo/user for an orchestration UI; Conifer Parallax per-episode credits on a hosted gateway. Loom sells the substrate the loop runs on — not the loop per seat. Conifer's structure (gateway monetized by review episodes) is itself validation of the pillar-4 key-broker plan.

## 6. What NOT to copy

- Cloud-only review and capture-everything-by-default: Loom reviews run on demand or on opt-in triggers; sessions stay local.
- Per-episode billing.
- GitHub App as the only delivery path (`loom-trackers` is multi-forge).
- The five-state label set, dedup thresholds, and delivery invariants: copy those **exactly** — they encode hard-won honesty rules, and inventing variants is how they break.

## Sources

- Conifer Parallax docs, retrieved 2026-10-19: `/docs/parallax/` (pipeline, pricing), `/docs/parallax/setup/` (capture, allowlist), `/docs/parallax/review/` (evidence states, dedup, confidence, fix PR), `/docs/parallax/memory/` (lessons, write-back, receipts), `/docs/parallax/control/` (`parallax.yml`, switches, retention).
- Loom: `crates/loom-sessions/src/acp_protocol.rs`, `crates/loom-routing/src/{router,outcomes,catalog}.rs`, `crates/loom-parallel/src/worktree.rs`, `crates/loom-runtime/`, `crates/loom-trackers/src/{github,gitlab}.rs`, `crates/loom-knowledge/`, `crates/loom-ast/`, `crates/loom-hindsight/`, [ROADMAP.md](../../ROADMAP.md) (pillars 2 and 4), [competitive-analysis.md](../competitive-analysis.md).
