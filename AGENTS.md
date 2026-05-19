# Loom — Agent Guide

## Quick Reference

```bash
# Build everything
cargo build --workspace

# Build release
cargo build --workspace --release

# Run all tests (crate-scoped)
cargo test -p loom-mcp
cargo test -p loom-auth
# ... etc for each crate

# Run integration test
bash run_integration_test.sh        # runs: cargo test --test cross_tool_workflow -- --nocapture

# Check compilation only (no tests)
cargo check --workspace

# Lint (Rust conventions, no dedicated config)
cargo clippy --workspace
```

No Makefile, no CI workflow files (`.github/workflows/`), no Dockerfile.

## Architecture

### Crate Dependency Hierarchy

```
loom-core  ←  everything else (types, constants, errors, traits)
    ↓
loom-ipc          — Unix socket IPC protocol (JSON-line)
loom-event-bus    — in-process (tokio broadcast) + cross-process (ZeroMQ ipc://)
    ↓
loom-{discovery,skills,mcp,auth,sessions,hooks,workflow,routing,telemetry}
    ↓                ↓
loom-generic-adapter + loom-adapters  — tool integration
    tool integration layer
    ↓
loom-cli     — CLI frontend (binary + lib)
loomd        — daemon (aggregates all subsystems)
loom-tui     — TUI (ratatui + crossterm)
```

### Two-Tier Architecture

- **loom-cli** (`crates/loom-cli/`): CLI binary with 20+ subcommands. Connects to daemon via Unix socket when available, or runs standalone (slower, runs Phase 2 discovery inline).
- **loomd** (`crates/loomd/`): Background daemon.agents_dir/`): Background daemon. Aggregates all subsystem crates, provides IPC over Unix domain socket (`~/.loom/loomd.sock`). Auto-shutdown after 5 min idle.

### IPC Protocol

JSON-line protocol over Unix domain socket (`loom-ipc` crate):

```json
{"request_id":"uuid","command":"skills.list","params":{}}
{"request_id":"uuid","status":"ok","data":"ok","data":{...}}
```

Commands: `health.check`, `discovery.scan`, `skills.list`, `skills.show`, `mcp.list`, `mcp.health`, `sessions.list`, `auth.list`, `hooks.list`, `version`, `register_tool`, `touch`.

### Event Bus

- **In-process**: tokio broadcast channel (cap 4096)
- **Cross-process**: ZeroMQ PUB/SUB via `ipc://~/.loom/.loom-events`
- Cross-process is behind `#[cfg(feature = "cross-process")]`
- Events: `SkillsAdded`, `McpHealthChanged`, `SessionExported`, etc. (all in `LoomEvent` enum)

### Filesystem Layout

Everything lives in `~/.loom/` (AGENTS_DIR constant in `loom-core`):

```
~/.loom/
├── config.toml              # Main config
├── config/
│   ├── routing.toml         # Overlays merged on top of config.toml
│   ├── mcp.toml
│   └── hooks.toml
├── skills/                   # Shared skills (skills.db SQLite)
├── mcp/                      # MCP server configs
├── sessions/                 # Session exports (JSONL)
├── auth/                     # Encrypted vault + audit logs
├── hooks/                    # Hook TOML configs and scripts
├── tools/                    # Tool TOML registration files
├── mcp_registry.db           # SQLite MCP server registry
├── loomd.sock                # Daemon IPC socket
└── .loom-events              # ZeroMQ cross-process event socket
```

### Common configuration schema: `config.toml + config/{routing|mcp|hooks}.toml` overlays
  Session: JSON → JSONL migration (compress after 20 messages, full fidelity for first 10)
```

## Key Patterns

### HealthCheck Trait

Every crate implements `loom_core::HealthCheck` returning `HealthStatus { crate_name, version, uptime_seconds, last_error, performance_counters }`. The daemon aggregates all on startup.

### as_arc() / into_arc() Pattern

Every major struct has a convenience method: `as_arc()` or `into_arc()` (they're the same — wrapping `self` into `Arc<Self>`). Use these when sharing across subsystems.

### GenericAdapter

Auto-derives 6 Loom traits (`SkillProvider`, `McpConsumer`, `AuthProviderTrait`, `HookExecutor`, `RoutingTarget`, `SessionExportable`) from a single TOML file at `~/.loom/tools/{tool_name}.toml`. Missing TOML sections produce no-op implementations (never errors on absent sections).

### Feature Flags

- `integration-acp` — enables ACP agent discovery (in `loom-cli` and `loom-sessions`)
- `cross-process` — enables ZeroMQ cross-process event bus

## Testing

- **Unit tests**: inline in each crate under `#[cfg(test)] mod tests { ... }`
- **All tests use `tempfile::tempdir()`** — never write to real filesystem paths
- **Integration tests**: `tests/integration/cross_tool_workflow.rs` (run via `bash run_integration_test.sh`)
- **loom-integration-tests**: `crates/loom-integration-tests/tests/` — tests/` (ecosystem.rs, routing_integration.rs)
- No dedicated test binary or test harness, no CI workflows

## CLI Commands

## Known Gotchas

1. **Hook scripts are placeholder**: `execute_hook_script()` in `loom-hooks` returns a success message without actually executing anything. If you need real hook execution, implement the script runner.
2. **loom-tui has `#[allow(dead_code)]` on `mod dispatch`** — dispatch module is used but compiler flags dead code due to conditional compilation patterns.
3. **Auth vault `export_credential()` returns plaintext** — the comment notes "this is the plaintext; proper export would re-encrypt".
4. **`mcp_registry.db` is SQLite** — created automatically in `~/.loom/` by `McpRegistry`.
5. **ConfigReader overlays layer-specific TOML files** — if `routing.toml` exists in `config/`, it completely replaces (not deep-merges) the `routing` field from `config.toml`.
6. **Session compression**: After `SESSION_COMPRESSION_THRESHOLD` (20) messages, session data uses compression. First `SESSION_FULL_FIDELITY_COUNT` (10) messages are stored uncompressed.
7. **Vault lockout**: max `VAULT_MAX_UNLOCK_ATTEMPTS` (5) within `VAULT_MAX_UNLOCK_ATTEMPTS within `VAULT_UNLOCK_WINDOW_MS` (60s).
8. **Daemon auto-shutdown**: after `DAEMON_DEFAULT_IDLE_TIMEOUT_MS` (300s = 5min) of no IPC activity.
9. **TUI keybindings**: `D` opens action palette (daemon controls), `a` opens dispatch meny only on task tabs (5=Skills, 6=Sessions), `?` for help. Tab indices: 0=Dashboard, 1=Daemon, 2=Discovery, 3=MCP, 4=Skills, 5=Sessions, 6=Logs.
10. **Adapter binaries for TerminalLaunch**: checks PATH for `opencode`, `claude-code`, `jcode`, `crush` (in order).
11. **loom-tui currently has a `#[allow(dead_code)]` annotation on the dispatch module**.
12. **`loom-ipc` uses Unix sockets only** — no TCP or Windows named pipe support.
13. **`loomd` error recovery**: `MCP server add/remove.upsert()` in `McpRegistry` returns Result — but the IPC handler (`ipc_handler.rs`) creates a **new** `McpRegistry` per request instead of using the shared instance, which means in-memory pools (`McpPool`, `McpClientPool`) are not persisted across restarts and health check state is not shared.
14. **Config `DocType` is v2.0.0 for all crates** (workspace version).

## TUI Architecture

- `ratatui` + `crossterm` backend
- 7 tabs, 1s auto-refresh
- Action palette for daemon operations (Start/Stop/Restart/InstallService/UninstallService)
- Dispatch meny on task tabs: TerminalLaunch (opens new terminal window with agent), ACPDispatch (not wired), EmbeddedPane (not implemented)
- Keybinding cheat sheet accessible via `?`

## Directory Structure

```
crates/
├── loom-core/        — Types, config, constants, errors, traits
├── loom-cli/         — CLI binary + lib
├── loomd/            — Daemon binary
├── loom-tui/         — TUI (ratatui)
├── loom-ipc/         — IPC protocol
├── loom-event-bus/   — Event bus
├── loom-discovery/   — Two-phase discovery engine
├── loom-skills/      — Skill registry, loader, adapter, hot-reload
├── loom-mcp/         — MCP server registry, pool, health checks
├── loom-auth/        — Credential vault, ACL, audit
├── loom-sessions/    — Session export/import, storage
├── loom-hooks/       — Hook registry, execution, signing
├── loom-workflow/    — Task orchestration and phases
├── loom-routing/     — Complexity scoring, model routing
├── loom-generic-adapter/  — TOML-driven tool adapter
├── loom-adapters/    — Preset adapters (jcode, crush, etc.)
├── loom-telemetry/   — Metrics, histograms, anomaly detection
├── loom-ffi/         — Foreign function interface (minimal)
└── loom-integration-tests/

packages/
├── sdk/              — SDK
├── jcode/            — jcode tool package
├── crush/            — Crush tool package
├── goose/            — Goose tool package
├── claude-code/      — Claude Code package
├── hooks/            — Hooks package
├── routing-hook/     — Routing hook scripts
└── opencode/         — OpenCode packaget/

docs/
└── ~/.loom/
    └── ...
```
