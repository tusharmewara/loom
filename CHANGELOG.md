# Changelog

All notable changes to the Loom project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- docs(harness): the Harness² Phase B contrast experiment ran and **closed negative** — the probe loop, the per-tool surface schema and the experience library are **not built** on this evidence. 5 planted defects, 10 runs, graded by each crate's own test command: the candidate suite was identical to the base suite in **5 of 5** tasks (the pre-registered rule needed ≥3), at a real spend of **$0.00 / ~41 min** against the $10–20 cap. The second finding is that Loom has **no always-on surface to perturb** — the surface was *found* by the agent, not injected, and finding it did not change the outcome. The analysis is updated to the measured outcome and the negative result is recorded in [`docs/plan/LOOM_PHASE_B_RESULT.md`](docs/plan/LOOM_PHASE_B_RESULT.md). The skill inventory + validation + trigger slice (Phase A) is unaffected and stays

### Added

- feat(auth): `loom auth export` writes the hash-chained audit log to a portable, signed JSON document (`<loom-dir>/auth/audit-export.json` by default). The document is **self-verifying**: every entry hash is re-derivable from the exported fields, and the whole document is signed with a per-machine Ed25519 key (`<loom-dir>/auth/export-signing.key`, mode `0600`, separate from the vault key — producing an export never unlocks the vault). `loom auth verify [path]` now reports ONE verdict — `intact` (chain re-derives **and** the signature verifies against a pinned public key), `tampered` (a named check failed), or `unanchored` (the chain re-derives but no public key was pinned, so the document is only self-consistent, not proven to come from this machine). `loom auth export --print-public-key` prints the public key to pin out of band; `loom auth verify <file> --public-key <hex|file>` verifies against it. The free-text `result` field is never exported: it is normalised to `granted` / `denied` / `other` and the entry is flagged `redacted`. The chain hash now covers the normalised outcome (`chain_scheme: 2`) so a consumer can re-derive it without the raw text; entries written by older Loom (`chain_scheme: 1`, hash over the raw result) still verify locally, and are covered by the signature in an export
- feat(auth): audit retention is one config knob — `[security] audit_retention_days` (default 365, `0` keeps everything) in the existing merged config. The oldest out-of-window prefix is dropped from exports, so the exported chain stays contiguous and its hash linkage stays verifiable; the on-disk chain is never rewritten
- feat(auth): `loom auth grant` issues scoped, revocable, expiring per-run credentials — `issue` (derived token for one provider + tool + run, never the secret), `verify` (scope, expiry, revocation), `revoke` (immediate, audited), `list`. The grant registry (`<loom-dir>/auth/grants.json`) is written `0600`. There is deliberately no CLI `redeem`: redemption returns plaintext and belongs to a trusted broker. Trust model: `docs/architecture/grant-trust-model.md`
- feat(runtime): the per-run container network posture is a policy enum (`none` / `restricted` / `unrestricted`) instead of a free-form network name, so the absence of a value can no longer mean "engine default network" (unrestricted egress). A container run defaults to `none`; `unrestricted` is the explicit escape hatch; unknown values are rejected. Container isolation stays opt-in (the default runtime is still `process`). A new `container` CI job runs the end-to-end container smoke test with Docker
- test(auth): the audit export's tamper-evidence is proven by tests, not asserted — a field tamper, a whole-document forgery against a pinned key, and a re-hashed tamper all fail verification, a correct export with no pinned key reports `unanchored` (never `intact`), and a log written by the older chain scheme still verifies
- docs(protocol): versioned **Loom Public Protocol** spec v1.0.0 — the adapter contract (tool TOML shape, `LoomToolAdapter` trait) and the ACP dispatch protocol, every normative statement anchored to a source line: [`docs/protocol/adapter-and-acp-protocol.md`](docs/protocol/adapter-and-acp-protocol.md)
- docs(protocol): the spec now also covers the **ACP dispatch server** (the TCP/Ed25519/JSON-line agent-to-agent dispatch surface in `loom-sessions` + `loomd`, §2.9), the `apply_routing_decision` environment-variable convention and its read-back rule (§1.1), the `adapter_for` name aliases, and the `acp_registry` gitlink caveat
- test(loom-generic-adapter): the tool TOML example published in the protocol spec is parsed by a test, so the document and the schema cannot drift apart
- feat(telemetry): `loom telemetry usage` — a metered usage ledger (per-run, per-tool, per-token) at `{loom_dir}/telemetry/usage.db`, readable with `loom telemetry usage [--json] [--limit N]`. Every row carries its provenance: a measured count, a documented `chars/4` forecast, or `unavailable` when the source reported nothing. An unreported count stays unreported — the ledger never presents a missing number as a zero. Covered paths today: `resume` (provider usage), `acp` (protocol usage), `knowledge-extract` (provider usage); `workflow` is recorded as unavailable rather than estimated. Not yet metered: `loom exec`, the TUI dispatch path, the TUI Cost tab, and multi-turn session cost

### Fixed

- fix(runtime): the TUI dispatch path now calls `Runtime::cleanup`, so a container run dispatched from the TUI no longer leaks the container. The engine's own auto-remove stays **off** — the runtime reads the container's logs after the process exits, and a daemon-side auto-remove would delete them
- fix(runtime): a `restricted` run now logs the posture it actually got (`container network posture downgraded: restricted (posture=none, allowed_hosts not enforced yet: …)`) instead of silently discarding the allowlist
- fix(runtime): the Kubernetes `NetworkPolicy` is now created **before** the Job — a policy selects Pods by label, not by the Pod existing
- fix(auth): the grant registry is written `0600` (it was written with the default umask), and the unused `GrantRecord::is_usable` helper is gone
- fix(telemetry): the usage ledger no longer reports "Measured tokens: 0 in / 0 out" for a run that reported no token count. A run that reported only a context-window size and a cost (the stable ACP v1 `usage_update` shape) was counted as measured, and the aggregate turned the absent count into a zero that read like a measurement; the token sums are now nullable end to end and the ledger says "unavailable (no run reported a token count)"
- fix(acp): the ACP usage method label now names what upstream actually defines — `acp:session-usage` and `acp:session-context-cost` instead of `acp:turn-usage`. ACP defines those counts as cumulative across the session, not per turn
- fix(daemon): `acp.dispatch` now returns the metered `usage` object in its reply, matching the CLI path — the TUI no longer has to read the ledger to see what a dispatch cost
- fix(cli): `loom telemetry reset` now says it resets the daemon's in-memory counters only; the usage ledger is untouched
- fix(cli): `loom sessions resume` hands the adapter a real cost estimate instead of a default one, so the adapter sees what the user is shown

## [0.9.6] - 2026-10-20

### Added

- feat(acp): `loom acp registry [--available] [--json]` lists all 42 agents in the public ACP registry, marking locally registered ones with `*`
- feat(acp): ACP agents can now be launched with an `env` map (registry `env` blocks are honored, e.g. `FAST_AGENT_MODEL` for `fast-agent`)
- feat(daemon): single-instance lock — loomd takes an exclusive `File::try_lock` on `<loom-dir>/.cache/loomd.lock` for its whole lifetime, so a second daemon can no longer steal the running one's Unix socket. `loom daemon start` reports the holder's pid, and a leftover socket is treated as stale
- feat(cli): CLI↔daemon version handshake — `loom doctor` fails on a version mismatch, `loom status` warns
- feat(ipc): the IPC envelope carries a protocol version, and requests time out (30s default) instead of hanging forever
- feat(daemon): loomd handles SIGTERM and SIGHUP and takes the same clean-shutdown path as Ctrl+C (socket removed, serve/gRPC handles aborted) — a service stop no longer leaves a stale socket
- feat(daemon): the idle-shutdown timeout is configurable via `LOOM_IDLE_TIMEOUT_MS` (`0` disables it)
- feat(cli): the service unit now owns the daemon's lifecycle from the CLI side — `loom daemon start`/`stop` go through the installed launchd/systemd unit instead of spawning a second, competing daemon. `loom daemon install-service` stops a manually started daemon first (otherwise it keeps the single-instance lock and the unit's daemon refuses to start) and upgrades an already-installed unit, and `loom daemon status` reports the supervisor and its state
- feat(scripts): `scripts/release.sh <version>` performs a release in one command — version guard (semver, strictly greater, no leading zeros, no existing tag, clean tree), workspace version bump, changelog section from `RELEASE_NOTES.md`, `Cargo.lock` refresh + verification, commit, annotated tag, and the exact CI dispatch command. `--dry-run` prints the plan, `--push` pushes and dispatches
- test(loomd): daemon-lifecycle integration tests run the real `loomd` binary against a throwaway ecosystem dir — IPC version handshake, a second daemon refusing to start instead of stealing the live socket, SIGTERM stopping the daemon and releasing the lock, and a stale socket left by a crashed daemon not blocking a new one

### Fixed

- fix(daemon): the installed launchd/systemd unit no longer resurrects a deliberate clean exit — the unit sets `LOOM_IDLE_TIMEOUT_MS=0` and restarts only on a crash (`KeepAlive{SuccessfulExit:false}` / `Restart=on-failure`). The old combination of `KeepAlive=true` / `Restart=always` with the 5-minute idle shutdown produced an endless respawn loop (~397 restarts in 24h on the dev machine)
- fix(daemon): the lock file distinguishes "held by another daemon" from "unusable lock file" — an unusable lock file (permissions, read-only fs) is now reported as such instead of being misread as "a daemon is already running", and loomd refuses to start unlocked
- fix(cli,daemon): the ecosystem directory is resolved once, from `LOOM_AGENTS_DIR` (falling back to `~/.loom`), for the CLI, the daemon and every TUI-spawned CLI — the CLI used to hardcode `~/.loom` while the daemon honored the env var, so the two could talk to different directories
- fix(tui): the Kanban and Orchestrator tabs no longer trap tab navigation — both tab-specific key handlers ran before the global handler and swallowed `Tab`/`Shift-Tab`/`←`/`→`, so the Orchestrator tab could not be left at all and the Kanban tab could not be switched away from. Kanban now uses `h`/`l` for columns and `j`/`k` for rows; the Orchestrator uses `↑`/`↓` for field focus and `←`/`→` for tool selection
- fix(acp): `loom acp run <agent>` now resolves the agent's real launch command — explicit path → local `config/agents.toml` entry → public ACP registry → `PATH` binary — instead of spawning a bare binary with no arguments. Agents were previously started outside ACP mode (e.g. `opencode` without its `acp` flag) and timed out on `initialize`
- fix(acp): `loom add-acp-agent` now picks a usable distribution deterministically (`npx` > `uvx` > `binary`, resolved for the current platform) instead of an arbitrary first key of the registry distribution map, so agents like `kilo` and `sigit` keep their package runner and their `acp` mode flag
- fix(acp): `uvx` agents are resolvable — the daemon's agent launcher previously understood only `npm` and `pip` runners
- fix(acp): a JSON-RPC error returned by the agent during `session/prompt` is now reported as `ACP session/prompt failed: <error>` instead of being masked as `ACP session/prompt timed out`

### Changed

- chore(loomd): removed the unwired `loomd/src/snapshot_builder.rs` orphan (211 lines, never compiled, duplicated `loom-discovery`'s snapshot handling and used a dependency loomd does not have)
- docs: corrected the stale claims in `AGENTS.md`, `ROADMAP.md` and `VISION.md` (CI workflows do exist, the TUI has 15 tabs not 7, the crate list was missing 11 members, the workspace version is 0.9.6)
- fix(homebrew): bottle releases are published under a `vX.Y.Z-bottles` tag in the tap repo. The tap's plain `vX.Y.Z` tag namespace is permanently reserved by GitHub once a tag has been used by an immutable release and can never be recreated, which broke the 0.9.5 publish
- chore: `cargo fmt --all` across the workspace — 8 files that had drifted out of rustfmt compliance (loom-routing `outcomes.rs`/`scorer.rs`, loom-cli `commands/routing.rs`, loom-tui `app.rs`/`ui/tabs/kanban.rs`, loom-workflow `task_manager.rs`, two loom-integration-tests)

## [0.9.5] - 2026-10-19

### Added

- docs: architecture diagrams — static SVG (`docs/diagrams/architecture-export.svg`) and interactive HTML (`docs/diagrams/architecture.html`), embedded in the README intro
- docs: competitive analysis refresh — Orca (stablyai) tool-by-tool entry, Orca column in the feature matrix, shipped-status annotations across the P0/P1/P2 roadmap items (worktrees, containers, PR integration, remote SSH, Kanban/Graph/Cost tabs, ACP registry, cross-session recall all marked shipped)
- docs: man pages rewritten for 0.9.x — `manpages/loom.1` covers all 36 subcommands (incl. cfg-gated `tui`), the 12 adapters, the 15 TUI tabs, corrected hook event types (PreToolUse/PostToolUse/SessionStart/PostPhase) and honest environment-variable docs (`LOOM_AGENTS_DIR`, not the never-honored `LOOM_DIR`); `manpages/loomd.1` documents the gRPC server, TLS env vars, ZeroMQ event socket, and the WSL2/Unix-socket constraint
- docs: `docs/` is no longer gitignored — analysis docs and diagrams are tracked; only the archify JSON intermediates (`docs/diagrams/*.json`) stay ignored

### Changed

- docs: README rewritten with source-verified facts (12 adapters, 15 tabs, 31 crates, CHANGELOG-driven version history, working links) replacing the 0.3.x-era drift
- feat(tui): tab bar shows per-tab shortcut labels (`1`–`9`, `0`, `!`, `@`, `#`) and scrolls to keep the active tab visible on narrow terminals
- feat(tui): Kanban tab renders workflow phases (Plan → Implement → Verify → Finish) as columns with per-task selection; the custom scrollbar widget was removed in favor of native scrolling

### Fixed

- fix(workflow): `git worktree add` now retries with `--orphan` (git ≥ 2.42) for repositories without any commit, and worktree isolation is best-effort — a non-git project directory or missing git no longer blocks task creation (the task runs in the project directory instead)

## [0.9.4] - 2026-10-18

### Added

- feat(routing): persisted decision + outcome telemetry (`crates/loom-routing/src/outcomes.rs`) — SQLite store at `~/.loom/telemetry/routing.db` records one row per task-bound decision (signals JSON, numeric, level, tier, model, token estimates) and closes it with an outcome: `completed` when the task reaches Finish, `overridden` when a manual tier override rejects the heuristic's pick; new `loom routing outcomes [--limit N]` reports recent decisions and outcome counts, giving the decision-model flip condition its evidence surface
- feat(core): `[security] require_signature` config flag (default off) — when enabled, unsigned hooks are skipped and unsigned plugins are rejected at install; documents the trust model explicitly (loom dir contents are user-owned local state)
- feat(config): `config/security.toml` overlay joins the merge chain alongside `routing.toml`/`mcp.toml`/`hooks.toml`/`acp.toml`
- feat(core): `derive_level(numeric, thresholds)` and `derive_tier(level)` — one exported source of truth for complexity level and model tier derivation
- docs: track `ROADMAP.md` (previously untracked, referenced by the routing debt record)

### Changed

- refactor(routing): the level/tier derivation duplicated across `scorer.rs`, `router.rs`, `task_manager.rs` (×2) and the CLI now routes through `derive_level`/`derive_tier`; configurable `complexity_thresholds` govern **level** as well as tier, making the Supplement's "these thresholds are configurable" claim true
- refactor: dedupe cross-crate helpers into `loom-core`, split workflow machinery, dependency hygiene
- chore: workspace `cargo fmt` and clippy 1.99 drift lints (`result_large_err`, `useless_format`, borrow in `println`)
- chore: `__pycache__/` gitignored; release-bottle script untracked (artifact dirs are gitignored)

### Fixed

- fix(routing): `extract_signals` counted **bytes**, not chars, inflating the length signal ~3× for UTF-8 CJK prompts and biasing CJK tasks toward higher tiers
- fix(core): `ComplexityThresholds` derived `Default` was `{0.0, 0.0}` rather than the serde defaults (`0.35`/`0.7`), so any routing config without a `[complexity_thresholds]` section treated every score as High → Flagship
- fix(routing): the level-vs-tier contradiction (config `low_max = 0.3` + `numeric = 0.32` yielded `level = Low` but `tier = Mid`) is resolved by the unified derivation
- fix(cli): `init`'s `routing.toml` template claimed threshold defaults `0.33`/`0.66`; corrected to the shipped `0.35`/`0.7`
- fix(routing-hook): the Python hook imported third-party `toml` (ImportError on any machine without `pip install toml`); now uses stdlib `tomllib` (Python ≥ 3.11), with the no-op-on-error contract unchanged
- ci: install `protoc` for the integration and test-matrix jobs; fail cargo steps properly (`set -o pipefail`); `--all-features` builds no longer break on duplicate cfg-gated imports

### Security

- fix(core): plugin downloads were HTTPS-only for remote config but accepted plain `http://` for plugins — `install_from_url` **and** `update()` now reject non-HTTPS URLs before any network I/O, closing an on-path tool-TOML swap
- feat(security): `require_signature` trusts nothing unsigned when enabled — hooks skip and plugin installs fail rather than running unverified code

### Tests

- test(routing): characterization suite pinning hook and scorer behavior before the refactor — 7 `routing_hook.py` contract tests (plain asserts, `tempfile` rules files, never `~/.loom`), user-override precedence, and the adversarial keyword ceiling (keyword weight 0.35 clamps at max 0.35 contribution, so keywords alone cannot force Flagship on the static path)
- test(routing): boundary and contradiction coverage for `derive_level`/`derive_tier`, including the strict-`<` edges at `low_max`/`high_min` (previously only the 0.5 midpoint was tested)
- test(core): HTTPS rejection on install and update, unsigned-plugin policy follows the flag, half-signed manifests rejected, `security` config parse and default
- test(routing): outcome-telemetry store and full workflow flow (create → advance ×3 → override → Finish = 4 rows: 3 `completed`, 1 `overridden`)

## [0.9.3] - 2026-10-09

### Fixed

- fix(loom-ipc): sync IPC handlers now run on a bare scoped thread instead of inside the async connection task — handlers that drive tokio runtimes internally (`Handle::block_on` / `Runtime::block_on`) panicked with "Cannot start a runtime from within a runtime", breaking `loom acp status/show/stop`, `loom hooks list`, and tool availability checks (the CLI surfaced this as `EOF while parsing a value at line 1 column 0`); a panicking handler now returns an error response instead of dropping the connection

### Changed

- chore: workspace clippy cleanup — `cargo clippy --workspace --all-targets` reports 0 warnings (was ~110): dead code removed (loomd capture.rs never-wired tracked-spawn chain, gRPC shutdown channel, loom-remote `default_key_paths`, `ForgeManager.loom_dir` field), lint fixes (`matches!`, `sort_by_key` + `Reverse`, `while let`, needless pass-by-value, complex type aliases), `#[allow(dead_code)]` on deserialized-but-unread serde response fields, `///!` → `//!` doc-comment headers, integration tests no longer drop `save_manifest` Results
- chore(loomd, loom-tui): container runtime features (docker/podman/kubernetes) forwarded via `[features]` instead of forced on every dependent build

## [0.9.2] - 2026-10-06

### Added

- feat(loom-auth): audit trail integrity verification — SHA-256 hash-chained audit log (`prev_hash`/`entry_hash` on `AuditEntry`) with `verify_integrity()` tamper detection; chain survives restarts and log rotation; legacy unchained entries are tolerated
- test(loom-integration-tests): workflow template sharing suite (2 tests)
- test(loom-integration-tests): hook signing edge cases suite (7 tests)
- test(loom-integration-tests): MCP health monitoring suite (4 tests)
- test(loom-integration-tests): session compression performance suite (5 tests)
- docs: horizontal scaling investigation (`docs/horizontal-scaling-investigation.md`) — verdict: local-first by design, no scaling work warranted

### Changed

- ci: integration test step actually executes now (was gated on `test -f` against a directory, which is always false)
- ci: integration and benchmark runner scripts are path-independent (no hardcoded checkout paths)

### Fixed

- fix(loom-cli): hooks `run` command reads correct `HookRunResult` fields (`blocked`/`stdout` instead of nonexistent `success`/`output`)
- fix(loom-auth): credential export performs ACL authorization check, audit logging, and in-memory re-encryption with a fresh nonce — no plaintext exposure, `requesting_tool` parameter added to `AuthProviderTrait`
- test: hermetic tests — no test writes to the real `~/.loom` anymore (worktree-creating tests use `worktree=false` or a tempdir `worktree_auto=false` config)

## [0.9.0] - 2026-09-16

### Added

- feat(loom-runtime): new crate with pluggable Runtime trait and ProcessRuntime
- feat(loom-runtime): Docker backend via bollard (Docker API)
- feat(loom-runtime): Podman backend via CLI
- feat(loom-runtime): Kubernetes backend via kube-rs 0.95 (batch/v1 Jobs, no kubectl)
- feat(loom-runtime): RuntimeRegistry with feature-gated registration (docker, podman, kubernetes, containers)
- feat(loom-runtime): Feature priority logic: docker > podman > kubernetes
- feat(loom-generic-adapter): launch_config() — reads [launch] TOML section → LaunchConfig
- feat(loom-tui): RuntimeDispatch dispatcher (shortcut 'r') in dispatch menu
- feat(loom-cli): loom exec sequential mode uses Runtime abstraction (prepare/run/cleanup)
- feat(loom-cli): loom exec run_parallel preserved (not yet using Runtime)
- feat: Feature flags docker, podman, kubernetes, containers propagated to loom-cli and loom-tui

### Changed

- fix(loom-runtime): Fixed lib.rs corruption (duplicate modules, format strings, missing braces)
- fix(loom-runtime): Docker client for bollard 0.18 (HostConfig import, StartContainerOptions<String>, LogsOptions wrap)
- fix(loom-runtime): Cleaned up warnings (unused imports, dead code, unused variables)
- fix(loom-runtime): RuntimeRegistry::Default registers kubernetes when feature enabled
- chore: Version bump to 0.9.0

### Fixed

- ContainerClient::is_available trait method removed (no callers)
- execute_via_runtime consolidated (was duplicated in loom-runtime and loom-cli)
- PodmanClient dead code under narrow feature gate

### Security

- Kubernetes uses kube-rs with proper kubeconfig/in-cluster authentication
- No kubectl CLI dependency (eliminates command injection surface)

## [0.8.0] - 2026-09-16

### Added

- feat(loomd): tracked ACP dispatch runs with disk-persisted registry (`run.status`, restart stale-marking, 30-day retention)
- feat(loomd): single tracked dispatch path for IPC and TCP (headless spawn, per-run logs, session capture, prior-session context)
- feat(loomd): canonical ACP v1 protocol client (`initialize` → `session/new` → `session/prompt`, stop reasons, permission policy)
- feat(cli): `loom acp run` runs a canonical ACP prompt turn and persists an indexed session
- feat(cli): `loom acp agents` lists launchable agents (daemon handlers plus agents.toml entries)
- feat(cli): `loom acp run-status` shows a tracked run's status, exit code, and session
- feat(cli): `loom workflow create --from-plan` imports chief plans as tagged tasks
- feat(cli): `loom workflow create --from-ticket` creates tasks from GitHub/Linear/GitLab/Jira tickets
- feat(cli): `loom workflow draft-plan [--create]` drafts task plans from free text with preview-then-confirm
- feat(cli): `loom workflow remove` deletes a task and its per-phase sidecars
- feat(cli): `loom issues create-pr` opens a pull request via `gh`
- feat(cli): `loom remote exec --capture-session` persists remote output as an indexed session
- feat(tui): Orchestrator intake tab (create task, tracked dispatch, run polling)
- feat(tui): Review tab (task list, detail, dispatch/resume/reassign/remove)
- feat(workflow): chief plan import, deterministic draft plans, task tags

### Changed

- fix(cli): allow hyphen-leading `--title`/`--description` values on `workflow create`/`draft-plan`
- chore(tui): dynamic tab indices; `0` selects Orchestrator, Review via Tab cycling and palette
- docs: corrected ACP/EmbeddedPane gotchas and harness comparison for the above

### Fixed

### Security

## [0.7.2] - 2026-09-01

### Added

### Changed

- Fix: isolate environment-variable tests in child process to prevent flakiness
- chore: fill real source SHA for v0.7.1 homebrew formula

### Fixed

### Security

## [0.7.1] - 2026-08-30

### Added

- feat(cli): add `loom edit` and `loom conflict` CLI commands (P2/5.3, P2/5.7)
- feat(acp): add CLI commands and IPC handler for ACP dispatch server (P1/2.5)
- feat(acp): wire ACP dispatch server into daemon startup (P1/2.5)

### Fixed

- test(core): isolate env-var test in child process to fix flakiness

## [0.7.0] - 2026-08-22

### Added

- feat(ast): AST structural editing with tree-sitter (P1/5.5)
- feat(rules): CLI `loom rules list/show/add/remove/eval` commands (P1/5.6)
- feat(rules): stream rule engine with condition matching and evaluation (P1/5.6)
- feat(conflict): conflict resolution protocol with scan/resolve/bulk operations (P1/5.7)
- feat(adapters): add Kiro adapter (P1/1.2)

## [0.6.1] - 2026-08-22

### Changed

- chore: bump version to 0.6.1

## [0.6.0] - 2026-08-22

### Added

- feat(hindsight): cross-session typed fact memory with retain/recall/reflect (P1/5.4)
- feat(skills): hashline edit system with content-hash anchored edits (P1/5.3)
- feat(acp): peer discovery and secure handshake (P1/2.5)
- feat: debug provider trait and adapter integration
- feat: DAP debugging integration (5.2)
- feat: LSP integration with skill adapters (5.1)
