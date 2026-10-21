# Loom Improvement Backlog

*Reconciled 2026-10-10 against [ROADMAP.md](../ROADMAP.md). Direction and sequencing live in the roadmap; this file is the feature-level backlog inventory.*

The original June 2026 planning document (phases 1–5, sourced from the Emdash/Kasetto/Trellis/Oh-My-Pi analyses) was executed to completion — scoring tables, week-by-week milestones, and per-item implementation notes are preserved in git history (last full revision: 2026-06-07).

## Shipped (June plan — complete)

All P0–P3 action items shipped across v0.3 → v0.9.x:

- **Quick wins**: shell completions; Claude Code, Codex, Cursor, Kiro, Juno, GitHub Copilot adapters; GitHub Issues; lockfile; README docs.
- **Core**: parallel execution with worktree isolation (`loom-parallel`); Linear + Jira trackers; SSH remote (`loom-remote`); sub-agent orchestration; ACP dispatch (server + daemon + CLI; handler panic isolation hardened in v0.9.3); embedded pane.
- **Experience**: cross-session memory (FTS5); config inheritance (`extends:`) + remote config URLs; plugin registry (SHA-256 + Ed25519 `--verify`); session branching; auto-update (`loom self-update`); visual PR review in TUI; doctor `--verbose`/`--fix`.
- **Precision (OMP-derived)**: LSP (`loom-lsp`, 6 ops); DAP debugging (`loom-debug`); hindsight memory (`loom memory retain/recall/reflect`); AST structural editing (`loom-ast`, tree-sitter); hashline edits (`loom edit`); time-traveling stream rules (`loom rules`); conflict resolution (`loom conflict`).

Verification (2026-10-10): stub-marker scan of `loom-lsp`/`loom-debug`/`loom-ast`/`loom-hindsight`/`loom-conflict`/`loom-rules`/`loom-knowledge` finds no placeholder code paths.

## Open Items — by roadmap pillar

### Pillar 1 — Harden the Core

- Daemon lifecycle: ~~single-instance lock (not socket-file existence)~~, ~~stale-socket recovery~~, ~~CLI↔daemon version handshake~~ — all landed in v0.9.6. Also landed: ~~SIGTERM handling~~ (loomd now handles SIGTERM/SIGHUP and cleans up its socket) and ~~the service unit owning the lifecycle~~ (idle timeout configurable via `LOOM_IDLE_TIMEOUT_MS`, `0` disables it; the unit sets it and restarts only on a crash). And the ownership is enforced end to end: `loom daemon start`/`stop` route through the installed unit instead of spawning a competing daemon, `install-service` stops a manual daemon first so the unit can take the lock, and `daemon status` reports the supervisor.
- IPC protocol: ~~versioned envelope~~, ~~request timeouts~~ — landed in v0.9.6. Panic isolation is already implemented (`crates/loom-ipc/src/server.rs`).
- Finish-or-delete sweep: ~~`loomd/src/snapshot_builder.rs` orphan (never declared in lib.rs)~~ — deleted in v0.9.6; ~~the fake `EmbeddedPane` dispatcher (it never spawned anything)~~ — deleted, the TUI's `App::dispatch_embedded_pane` is now the single owner of the embedded-pane path, covered by a test that asserts the `loom workflow dispatch <task> --agent <agent>` command is actually spawned. Still open: interactive end-to-end verification of the embedded-pane dispatch path in a live TUI.
- Release automation: ~~one-command release (version → changelog → tag → CI → bottles dispatch with version input)~~ — landed in v0.9.6 as `scripts/release.sh <version>`; the bottles workflow now validates the dispatched version against `Cargo.toml`.
- Daemon-lifecycle integration tests: restart races, socket contention, version skew.
- Docs coherence: ~~README/STATUS/VISION drift (subcommand counts, clone URLs, stale sections) reconciled into one truth~~ — done in v0.9.6.

### Pillar 3 — Ecosystem & Protocol

- IDE extensions (community): VS Code / JetBrains consuming Loom via `loom-mcp-server` or the public protocol — carried over from the June P3 list.

### Pillar 1 (candidate slice) — Skill inventory + validation + trigger generation (spec: [plan/LOOM_SKILL_VALIDATION_PROPOSAL.md](plan/LOOM_SKILL_VALIDATION_PROPOSAL.md))

Cut from the Harness² analysis after review ([LOOM_HARNESS2_ANALYSIS.md](LOOM_HARNESS2_ANALYSIS.md) §7, §12). Two defects, one slice. *Inventory:* `loom skills list` returns `[]` and `loom lock --generate` writes zero entries while 11 skills are installed — the registry is never populated. *Validation:* a skill with no `description:` frontmatter is installed, synced into every native tool directory, and **silently ignored by the agent** while Loom reports success. The validator is a plain function (upstream's shape), not a schema and not a rules engine.

- `crates/loom-skills/src/scan.rs` — one walk over `~/.loom/skills/` handling both layouts (`<name>/SKILL.md` and flat `<name>.md`); parses frontmatter; `validate_skill_tree(dir) -> Vec<SkillDefect>`; rules: missing `description:` (fatal), `name:` mismatch (fatal in dir layout, warn in flat layout), unparseable frontmatter (fatal), missing referenced script (fatal), duplicate flat/dir pair (warn).
- `crates/loom-skills/src/trigger.rs` — one trigger line per skill, deterministic order, cap enforced by dropping **and reporting** the untriggered skills. The cue is the **parsed** YAML value, not upstream's regex.
- Populate the existing registry from the walk (`crates/loom-skills/src/registry.rs`) — the `description` column and the `loom_core::SkillEntry` row type already exist; no schema change, no new struct.
- Fix `loom skills install` / `remove` to handle the directory layout (`std::fs::copy` on a directory fails today; `remove_file` cannot remove one).
- Delete the dead `crates/loom-skills/src/neutral_renderer.rs` (373 lines, zero callers, **does not compile** — not declared in `lib.rs`, no `serde_yaml` dep, and two syntax errors). Pillar 1 ship-or-remove.
- Wire the validator into `loom skills install` and into both sync paths (`sync_skills_to_native`, `inject_shared_skills`); report in `loom doctor`. No new CLI group, no new crate.
- Cost: 3–4 days, no LLM spend. The inventory half is corrective (`loom skills list` → `[]` today); the validation half is preventive: 1 fatal defect in 17 real skill files on this machine.

### Pillar 5 — Verified Review Engine (spec: [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md))

- New `loom-review` crate: session → review plan (intent + diff, never the agent's reasoning) → cross-family reviewer seats over ACP dispatch → mechanism dedup → worktree evidence runner → evidence-labelled findings → fix PR (`loom-trackers`) → outcome-gated lessons (`loom-knowledge` + `loom-hindsight`).
- Evidence states copied verbatim from Parallax: `proven` / `suite_green` / `unverified` / `could_not_run` / `refuted`; a cut step is `could_not_run`, never `refuted`; suite-regressing patches are never auto-applied and never enter the fix PR.
- `loom-routing` extension: cross-family reviewer seating (tier + outcome telemetry) and per-agent×lens scoreboard/calibration — the same data feeds pillar-4 usage metering.
- Repo-root `review.toml` + `~/.loom/config/review.toml` overlay (existing 5-layer merge): `test/setup/check/bench` commands with root-marker guess table (report says "guessed"), `lenses`, `severity_floor`, `ignore`, `deliver` policy.
- MVP gate: pillar 1 complete, no container dependency — manual `loom review run <session-id>`, 2 seats, worktree verification (ProcessRuntime), human-triggered fix PR, proven-finding lessons. Containers, adversary seats, and write-back automation wait for pillar 2.

### Candidates (unscheduled — decide when pillars 1–3 close)

- Browser automation surface (OMP comparison "future" item; needs an owner and a use case before entering a pillar).
- web_search across multiple providers (OMP "future" item; likely an MCP-server answer, not a core feature).
- Telemetry/anomaly-detection maturation — deepen only when pillar 4 usage metering needs it.
- More preset adapters / tracker backends — demand-driven; `GenericAdapter` TOML + `IssueTracker` trait are the escape hatches.
- **Self-evolving shared harness** — Loom editing the *scaffolding* (skills, tool guidance, trigger sets) rather than only the code, using the Harness² propose→probe→compose loop. Analysis in [LOOM_HARNESS2_ANALYSIS.md](LOOM_HARNESS2_ANALYSIS.md) §11. Not a new pillar: it is pillar 5 with the edit target widened from code to the always-on surface, and pillar 5 supplies the outcome signal it needs (so the probe loop is *downstream* of pillar 5, not parallel). Two decisions are open before it can enter a pillar: **(a) shared layer vs one tool's prompt** — shared is the answer Loom's north star gives, and it is the one upstream cannot give; **(b) the security obligation** — who may write a surface, what is signed (Ed25519, as the plugin registry does), and what the audit log records, because a self-modifying surface is a pillar-2 item by nature. Needs an owner and a use case first; the cheap entry point is the contrast experiment in [LOOM_HARNESS2_ANALYSIS.md](LOOM_HARNESS2_ANALYSIS.md) §7 Phase B.

### Code debt — routing (2026-10 review; details in [routing-debt.md](routing-debt.md))

- Six verified routing debts to close: level-vs-tier threshold governance (duplicated derivation), Supplement spec divergences, missing tests (routing_hook.py / user_override / keyword-gaming), CJK byte-vs-char scoring, install-from-URL plain HTTP + optional signatures, outcome telemetry. Includes the decision-model review record (Jev/Laya/Microsoft-Decision-1/Prism: NO for core, flip conditions listed).

## Known quirks — deliberate, not bugs

- Vault lockout on repeated failed unlocks — security feature, keep.
- TUI keybindings documented rather than changed.
- Adapter binaries resolved via PATH lookup — as-is.
