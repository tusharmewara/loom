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

- Daemon lifecycle: ~~single-instance lock (not socket-file existence)~~, ~~stale-socket recovery~~, ~~CLI↔daemon version handshake~~ — all landed in v0.9.6. Also landed: ~~SIGTERM handling~~ (loomd now handles SIGTERM/SIGHUP and cleans up its socket) and ~~the service unit owning the lifecycle~~ (idle timeout configurable via `LOOM_IDLE_TIMEOUT_MS`, `0` disables it; the unit sets it and restarts only on a crash). Still open: `loom daemon install/uninstall` owning its launchd/systemd unit, so no competing agent keeps a duplicate daemon alive.
- IPC protocol: ~~versioned envelope~~, ~~request timeouts~~ — landed in v0.9.6. Panic isolation is already implemented (`crates/loom-ipc/src/server.rs`).
- Finish-or-delete sweep: ~~`loomd/src/snapshot_builder.rs` orphan (never declared in lib.rs)~~ — deleted in v0.9.6. Still open: end-to-end runtime verification of the embedded-pane dispatch path.
- Release automation: ~~one-command release (version → changelog → tag → CI → bottles dispatch with version input)~~ — landed in v0.9.6 as `scripts/release.sh <version>`; the bottles workflow now validates the dispatched version against `Cargo.toml`.
- Daemon-lifecycle integration tests: restart races, socket contention, version skew.
- Docs coherence: ~~README/STATUS/VISION drift (subcommand counts, clone URLs, stale sections) reconciled into one truth~~ — done in v0.9.6.

### Pillar 3 — Ecosystem & Protocol

- IDE extensions (community): VS Code / JetBrains consuming Loom via `loom-mcp-server` or the public protocol — carried over from the June P3 list.

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

### Code debt — routing (2026-10 review; details in [routing-debt.md](routing-debt.md))

- Six verified routing debts to close: level-vs-tier threshold governance (duplicated derivation), Supplement spec divergences, missing tests (routing_hook.py / user_override / keyword-gaming), CJK byte-vs-char scoring, install-from-URL plain HTTP + optional signatures, outcome telemetry. Includes the decision-model review record (Jev/Laya/Microsoft-Decision-1/Prism: NO for core, flip conditions listed).

## Known quirks — deliberate, not bugs

- Vault lockout on repeated failed unlocks — security feature, keep.
- TUI keybindings documented rather than changed.
- Adapter binaries resolved via PATH lookup — as-is.
