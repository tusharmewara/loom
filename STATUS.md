# Loom Status

## Overview

Loom is a multi-agent orchestration framework built in Rust. It provides a unified interface for skills, MCP servers, and cross-tool adapters.

## Crates

31 crates in the workspace (`cargo metadata` is the source of truth):

- `loom-core`: Types, config, constants, errors, traits, single-instance lock
- `loom`: CLI binary with 20+ subcommands
- `loomd`: Background daemon aggregating all subsystems
- `loom-tui`: Terminal UI (ratatui + crossterm)
- `loom-ipc`: JSON-line protocol over Unix sockets
- `loom-event-bus`: In-process (tokio broadcast) + cross-process (ZeroMQ ipc://)
- `loom-discovery`: Two-phase discovery engine
- `loom-skills`: Skill registry, loader, adapter, hot-reload
- `loom-mcp`: MCP server registry, pool, health checks
- `loom-auth`: Credential vault, ACL, audit
- `loom-sessions`: Session export/import, storage
- `loom-hooks`: Hook registry, execution, signing
- `loom-workflow`: Task orchestration and phases
- `loom-routing`: Complexity scoring, model routing
- `loom-generic-adapter`: TOML-driven tool adapter
- `loom-adapters`: Preset adapters (jcode, crush, etc.)
- `loom-ast`: AST structural editing with tree-sitter
- `loom-telemetry`: Metrics, histograms, anomaly detection
- `loom-ffi`: Foreign function interface (minimal)
- `loom-conflict`: Conflict resolution protocol (detect/resolve merge conflicts)
- `loom-debug`: Debug Adapter Protocol (DAP) client integration
- `loom-hindsight`: Hindsight memory — cross-session typed fact storage
- `loom-knowledge`: Knowledge graph (SQLite entities/relationships, FTS5)
- `loom-lsp`: Language Server Protocol (LSP) client integration
- `loom-mcp-server`: MCP server exposing Loom's shared resources
- `loom-parallel`: Concurrent agent execution, git-worktree isolation
- `loom-remote`: SSH remote development
- `loom-rules`: Stream rules engine for LoomEvent processing
- `loom-runtime`: Pluggable runtime backends (process, Docker, Podman, Kubernetes)
- `loom-trackers`: GitHub/Linear issue and PR trackers
- `loom-integration-tests`: Cross-crate integration tests

## Packages

- `jcode`: JCode tool package
- `crush`: Crush tool package
- `claude-code`: Claude Code package
- `opencode`: OpenCode package
- `goose`: Goose tool package
- `hooks`: Hooks package
- `routing-hook`: Routing hook scripts
- `homebrew`: Homebrew formula and bottles

## Features

- Two-tier architecture: CLI (standalone or daemon-connected) and daemon
- IPC over Unix domain sockets, versioned envelope, 30s request timeout
- Daemon single-instance lock (`.cache/loomd.lock`, `File::try_lock`) with stale-socket recovery
- Daemon handles SIGTERM/SIGHUP and cleans up its socket; the idle timeout is configurable via `LOOM_IDLE_TIMEOUT_MS` (`0` = disabled, which the installed service unit sets)
- Service units restart only on a crash (`KeepAlive{SuccessfulExit:false}` / `Restart=on-failure`), so a deliberate clean exit is not resurrected
- CLI↔daemon version handshake: `loom doctor` fails on skew, `loom status` warns
- One-command release: `scripts/release.sh <version>` (version guard → bump → changelog → lock → commit → tag → CI dispatch command)
- Daemon-lifecycle integration tests (`crates/loomd/tests/daemon_lifecycle.rs`): IPC version handshake, duplicate-daemon refusal, SIGTERM shutdown, stale-socket recovery
- Event bus with in-process and cross-process (ZeroMQ) support
- Skill registry with hot-reload
- MCP server registry and health checks
- Credential vault with encryption
- Scoped, revocable, expiring per-run grants (`loom auth grant issue|verify|revoke|list`); the agent never holds the stored secret, and there is deliberately no CLI `redeem`
- Tamper-evident, signed audit export (`loom auth export` / `loom auth verify`): a recomputable chain over the normalised outcome plus an Ed25519 anchor; one verdict — `intact` / `tampered` / `unanchored`
- Container network posture as a policy enum (`none` / `restricted` / `unrestricted`); a container run defaults to no egress, and `restricted` is fail-closed until the egress proxy lands
- Session export/import with compression
- Hook system for pre/post task execution
- Workflow engine for task orchestration
- Routing for model selection
- Telemetry and anomaly detection

## Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo test -p loom-mcp
cargo test -p loom-auth
# ... etc for each crate

# Run integration test
bash run_integration_test.sh

# Check compilation only
cargo check --workspace

# Lint
cargo clippy --workspace
```

## Installation

### Homebrew (macOS/Linux)

```bash
brew install loom
```

### Cargo

```bash
cargo install loom
```

### From Source

```bash
git clone https://github.com/tusharmewara/loom.git
cd loom
cargo build --release
```

## Usage

```bash
loom --help
```

## Configuration

Main config: `~/.loom/config.toml`

`LOOM_AGENTS_DIR` moves the whole ecosystem directory (config, cache, sockets,
sessions) for both the CLI and the daemon — the two always agree on which
directory they are talking to:

```bash
LOOM_AGENTS_DIR=/tmp/loom-sandbox loom init
```

Audit retention is configured in the `[security]` section of the merged
config (one knob, same 5-layer merge as everything else):

```toml
[security]
audit_retention_days = 365   # default; 0 = keep everything
```

Entries older than the window are dropped from an audit export
(`loom auth export`); the drop is a prefix, so the exported chain stays
contiguous and its hash linkage stays verifiable. The on-disk chain is
never rewritten.

The export is tamper-evident: the chain hash covers the normalised outcome
(so a consumer can re-derive it without the raw free-text `result`, which
never leaves the machine) and the whole document is signed with a per-machine
Ed25519 key. `loom auth verify <file>` returns one verdict — `intact`,
`tampered`, or `unanchored` (self-consistent only, because no public key was
pinned). Pin the key with `loom auth export --print-public-key`.

`loom auth verify` (an audit **export**) and `loom auth grant verify` (a
**grant**) are different commands and check different things.

## Known Issues

- Hook scripts execute properly (implementation complete)
- ~~Auth vault export requires encryption enforcement~~ — **Fixed**: `export_credential()` re-encrypts in-memory with fresh nonce (no plaintext persistence)
- ~~Cross-pool MCP process reuse~~ — **Fixed**: all pools sharing a registry now share a single live process map (`RegistryShared::processes`), so a process spawned by one pool is reused by any other pool referencing the same registry, and a pool can only kill a process when the shared refcount reaches zero (closes the "terminated" orphan bug). `shutdown_all` clears the shared refcounts once after killing, per the documented daemon-restart reset semantics.
- **Seeding scope**: `McpRegistry::new()` does not auto-seed defaults; built-in defaults (context-mode, rtk, loom, agentmemory) are seeded by the daemon-facing `RegistryShared` path only when the shared table is empty, so explicit setup (`loom init`, `loom init --recommended`, user registration) controls the registry contents without defaults being forced in.
- ~~**Service unit ownership**: endless respawn loop from `KeepAlive=true` / `Restart=always` + the 5-minute idle shutdown~~ — **Fixed**: the unit now sets `LOOM_IDLE_TIMEOUT_MS=0` (no idle self-exit) and restarts only on a crash (`KeepAlive{SuccessfulExit:false}` / `Restart=on-failure`). A manually started daemon keeps the 5-minute idle default. An existing installed unit from an older version must be reinstalled (`loom daemon install-service`) to pick this up.
- ~~**SIGTERM**: loomd handled only SIGINT, so a service stop left a stale socket~~ — **Fixed**: loomd now handles SIGTERM and SIGHUP and takes the same clean-shutdown path as Ctrl+C (socket removed, serve/gRPC handles aborted).
- ~~**Service unit ownership (remaining)**: the unit files are still written by `loom daemon install-service`; the supervisor does not yet prevent a *second*, unrelated agent from keeping its own duplicate daemon alive~~ — **Fixed**: `loom daemon start`/`stop` now route through the installed launchd/systemd unit instead of spawning a competing daemon (`crates/loom-cli/src/service.rs`), `loom daemon install-service` stops a manually started daemon first so the unit can take the single-instance lock, and `loom daemon status` reports the supervisor and its state. A unit installed by an older version must be reinstalled to pick this up.

## Roadmap

Direction and pillars: [ROADMAP.md](ROADMAP.md). Open backlog: [docs/loom-improvements.md](docs/loom-improvements.md).

## Review Records

The following independent review reports are on `main` and document the findings, evidence, and resolutions for each major merge:

- [Phase A — Skill Inventory, Validation & Trigger Generation](docs/plan/LOOM_PHASE_A_REVIEW.md) — merge `fa29056`
- [Pillar 1 — Harden the Core](docs/plan/LOOM_PILLAR1_REVIEW.md) — merge `7457c72`
- [Pillar 2 — Security & Trust](docs/plan/LOOM_PILLAR2_REVIEW.md) — merge `e0edb50`
- [Pillars 3+4 — Protocol Spec & Usage Metering](docs/plan/LOOM_PILLAR34_REVIEW.md) — merges `d8eb9db`, `a31740d`

Each report carries a RESOLVED banner mapping every finding to its resolution (code change, test addition, or documentation fix).

## Contributors

See README for contribution guidelines.