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
cargo test -p loom-routing
cargo test -p loom-integration-tests
# ... etc for each crate

# Run integration tests
cargo test -p loom-integration-tests

# Check compilation only (no tests)
cargo check --workspace

# Lint (Rust conventions, no dedicated config)
cargo clippy --workspace

# Dependency hygiene (cargo audit also runs in CI; cargo machete is local-only)
cargo machete    # unused deps (loomd/prost is a documented false positive, ignore-listed)
cargo audit      # RustSec advisories (currently 0 errors, unmaintained-class warnings only)
```

No Makefile, no Dockerfile. CI workflows live in `.github/workflows/` (`ci.yml`, `test.yml`, `homebrew-bottles.yml`).

## Architecture

### Crate Dependency Hierarchy

```
loom-core  ←  everything else (types, constants, errors, traits)
    ↓
loom-ipc          — Unix socket IPC protocol (JSON-line)
loom-event-bus    — in-process (tokio broadcast) + cross-process (ZeroMQ ipc://)
loom-knowledge    — Knowledge graph (SQLite entities + relationships, FTS5, recursive CTE traversal)
    ↓
loom-{discovery,skills,mcp,auth,sessions,hooks,workflow,routing,telemetry}
    ↓                ↓
loom-generic-adapter + loom-adapters  — tool integration
    tool integration layer
    ↓
loom-cli     — CLI frontend (binary + lib)
loomd        — daemon (aggregates all subsystems)
loom-tui     — TUI (ratatui + crossterm)
loom-ffi     — Foreign function interface
```

### Two-Tier Architecture

- **loom-cli** (`crates/loom-cli/`): CLI binary with 20+ subcommands. Connects to daemon via Unix socket when available, or runs standalone (slower, runs Phase 2 discovery inline).
- **loomd** (`crates/loomd/`): Background daemon. Aggregates all subsystem crates, provides IPC over Unix domain socket (`~/.loom/loomd.sock`). Auto-shutdown after 5 min idle.
- **loom-tui** (`crates/loom-tui/`): Terminal UI with 15 tabs (Dashboard, Daemon, Discovery, MCP, Skills, Sessions, Logs, Knowledge, PRs, Runtime, Kanban, Graph, Cost, Orchestrator, Review). 1s auto-refresh. Keybindings: D (action palette), a (dispatch menu on task tabs), ? (help).
- **loom-ffi** (`crates/loom-ffi/`): C FFI exports for external tool integration.

### IPC Protocol

JSON-line protocol over Unix domain socket (`loom-ipc` crate):

```json
{"request_id":"uuid","command":"skills.list","params":{}}
{"request_id":"uuid","status":"ok","data":"ok","data":{...}}
```

Commands: `health.check`, `discovery.scan`, `skills.list`, `skills.show`, `mcp.list`, `mcp.health`, `sessions.list`, `auth.list`, `hooks.list`, `version`, `register_tool`, `touch`, `knowledge.query`, `knowledge.search`, `knowledge.stats`, `knowledge.build`.

### Event Bus

- **In-process**: tokio broadcast channel (cap 4096)
- **Cross-process**: ZeroMQ PUB/SUB via `ipc://~/.loom/.loom-events` (behind `#[cfg(feature = "cross-process")]`)
- Events: `SkillsAdded`, `McpHealthChanged`, `SessionExported`, `RoutingUpdated`, `AuthProviderAdded`, `HooksRegistered`, `TaskPhaseChanged`, `TaskCreated`

### Filesystem Layout

Everything lives in `~/.loom/` (AGENTS_DIR constant in `loom-core`):

```
~/.loom/
├── config.toml              # Main config
├── config/
│   ├── routing.toml         # Overlays merged on top of config.toml
│   ├── mcp.toml
│   ├── hooks.toml
│   └── acp.toml             # ACP registry URL
├── skills/                   # Shared skills (skills.db SQLite)
├── mcp/                      # MCP server configs
├── sessions/                 # Session exports (JSONL)
├── auth/                     # Encrypted vault + audit logs
├── hooks/                    # Hook TOML configs and scripts
├── tools/                    # Tool TOML registration files
├── workflow/                 # Task orchestration
│   └── tasks/               # Task manifests
├── knowledge/                # Knowledge graph DB
│   └── graph.db             # SQLite (entities, relationships, FTS5)
├── mcp_registry.db           # SQLite MCP server registry
├── loomd.sock                # Daemon IPC socket
├── .loom-events              # ZeroMQ cross-process event socket
└── .cache/                   # Snapshot cache
```

### Session Compression

After `SESSION_COMPRESSION_THRESHOLD` (20) messages, session data uses compression. First `SESSION_FULL_FIDELITY_COUNT` (10) messages are stored uncompressed. Compression is **automatic** on `append_message` and `replace_message` once a session crosses the threshold, and can also be triggered manually via `loom sessions summarize <id> [--force]`. The compression step folds the older messages into a single `System`-role summary at the head of the message list (id = `loom-summary-v1`), extracts role-prefixed excerpts (200 chars per dropped message) merged on role boundaries, and updates the manifest's `summarized_at` / `summarized_message_count` fields. The summary message is also re-indexed into the cross-session memory store so `sessions search` / `recall` reflect the compressed view.

## Key Patterns

### HealthCheck Trait

Every crate implements `loom_core::HealthCheck` returning `HealthStatus { crate_name, version, uptime_seconds, last_error, performance_counters }`. The daemon aggregates all on startup.

### as_arc() / into_arc() Pattern

Every major struct has a convenience method: `as_arc()` or `into_arc()` (they're the same — wrapping `self` into `Arc<Self>`). Use these when sharing across subsystems.

### GenericAdapter

Auto-derives 6 Loom traits (`SkillProvider`, `McpConsumer`, `AuthProviderTrait`, `HookExecutor`, `RoutingTarget`, `SessionExportable`) from a single TOML file at `~/.loom/tools/{tool_name}.toml`. Missing TOML sections produce no-op implementations (never errors on absent sections).

### Feature Flags

- `cross-process` — enables ZeroMQ cross-process event bus
- `tui` — enables TUI commands in CLI

## Testing

- **Unit tests**: inline in each crate under `#[cfg(test)] mod tests { ... }`
- **All tests use `tempfile::tempdir()`** — never write to real filesystem paths
- **Integration tests**: `crates/loom-integration-tests/tests/` (run via `cargo test -p loom-integration-tests`)
- **No dedicated test binary or test harness; CI runs `cargo test --workspace` (see `.github/workflows/ci.yml`)**

## CLI Commands

### Core
- `loom init [--import] [--recommended]` — Create ~/.loom/ tree, optionally scan and import
- `loom scan [--json]` — Display resource summary
- `loom status [--json]` — Show ecosystem state
- `loom doctor [--json]` — Deep diagnostics
- `loom thread <tool> [--apply-routing <decision>]` — Register/update tool
- `loom exec <tools...> [-- args...]` — Execute tools with ecosystem
- `loom init-shell [--shell bash|zsh|fish]` — Generate shell integration
- `loom add-acp-agent <agent_id>` — Add ACP agent from registry
- `loom tui` — Launch terminal UI (requires tui feature)

### MCP
- `loom mcp list [--json]` — List registered MCP servers
- `loom mcp show <id> [--json]` — Show MCP server config
- `loom mcp add <name>` — Add MCP server stub
- `loom mcp remove <id>` — Remove MCP server
- `loom mcp install <name> [--prewarm]` — Install known MCP server
- `loom mcp install-recommended [--prewarm]` — Install all recommended
- `loom mcp list-known [--json]` — List installable MCP servers

### Skills
- `loom skills list [--json]` — List skills
- `loom skills show <name> [--json]` — Show skill details
- `loom skills install <path>` — Install skill from file
- `loom skills remove <name>` — Remove skill

### Sessions
- `loom sessions list [--json]` — List session manifests
- `loom sessions show <id> [--json]` — Show session manifest
- `loom sessions export <id> [--output path]` — Export session
- `loom sessions import <path>` — Import session
- `loom sessions resume <id> [--provider] [--model] [--agent] [--inject-memory] [--inject-knowledge]` — Resume session

### LSP
- `loom lsp servers [--json]` — List available language servers
- `loom lsp rename --path <file> --line <n> --column <n> <new_name>` — Rename symbol
- `loom lsp find-references --path <file> --symbol <name>` — Find references
- `loom lsp exec <tool> <operation> [--path] [--symbol] [--line] [--column]` — Execute via tool adapter

### Debug
- `loom debug adapters [--json]` — List debug adapters
- `loom debug launch --program <path> --language <lang>` — Launch and debug
- `loom debug attach --pid <n> --language <lang>` — Attach to process
- `loom debug breakpoints --file <path> --line <n>` — Set breakpoints (active session required)
- `loom debug step <over|in|out>` — Step through execution
- `loom debug evaluate <expression>` — Evaluate expression
- `loom debug stack` — Show stack trace

### AST
- `loom ast parse <file> [--language] [--max-depth] [--json]` — Parse and show AST tree
- `loom ast query <file> <query> [--language] [--json]` — Execute tree-sitter query
- `loom ast node-at <file> --line <n> --column <n> [--json]` — Show node at position
- `loom ast move-up/move-down <file> --line <n> --column <n> [--dry-run]` — Move node
- `loom ast swap <file> --line-a <n> --column-a <n> --line-b <n> --column-b <n>` — Swap nodes
- `loom ast extract <file> --line <n> --column <n> <name> [--dry-run]` — Extract to variable
- `loom ast wrap <file> --line <n> --column <n> <wrapper> [--dry-run]` — Wrap in construct
- `loom ast unwrap <file> --line <n> --column <n> [--dry-run]` — Unwrap node
- `loom ast duplicate <file> --line <n> --column <n> [--dry-run]` — Duplicate node
- `loom ast insert <file> --line <n> --column <n> <text> [--dry-run]` — Insert text
- `loom ast replace <file> --start-line/col <n> --end-line/col <n> <text>` — Replace range
- `loom ast kind-at <file> --line <n> --column <n>` — Show node kind
- `loom ast find-kind <file> <kind> [--json]` — Find nodes of kind
- `loom ast format <file> [--dry-run]` — Format source code
- `loom ast stats <file> [--json]` — AST statistics

### Auth
- `loom auth list [--json]` — List auth entries
- `loom auth providers [--json]` — List registered providers
- `loom auth show <provider> [--json]` — Show provider config
- `loom auth validate <provider>` — Validate credential

### Hooks
- `loom hooks list [--json]` — List registered hooks
- `loom hooks show <id> [--json]` — Show hook config
- `loom hooks register <path>` — Register hook from file
- `loom hooks remove <id>` — Remove hook
- `loom hooks run --event <type> --tool <name> [--matcher]` — Run hooks for event
- `loom hooks verify` — Verify hook signatures

### Workflow
- `loom workflow list [--json]` — List tasks
- `loom workflow show <id> [--json]` — Show task
- `loom workflow create [--title] [--description]` — Create task
- `loom workflow advance <id>` — Advance phase
- `loom workflow assign <id> [--provider] [--model] [--last-agent]` — Assign provider
- `loom workflow dispatch <id> [--agent] [--tool-args...] [--dry-run]` — Dispatch task

### Routing
- `loom routing show [--json]` — Show routing config and catalog
- `loom routing evaluate <prompt> [--json]` — Score prompt and get decision
- `loom routing score <prompt> [--json]` — Show complexity score
- `loom routing recommend <task-id> [--json]` — Get recommendation
- `loom routing override <task-id> --tier <tier> [--reason]` — Override decision
- `loom routing catalog [--refresh] [--json]` — List models

### Knowledge
- `loom knowledge build [--json] [--llm]` — Build knowledge graph from indexed sessions (--llm uses LLM extraction)
- `loom knowledge query <entity> [--depth <n>] [--json]` — Query subgraph around entity
- `loom knowledge stats [--json]` — Show graph statistics
- `loom knowledge add <name> [--entity-type <type>] [--canonical <name>]` — Add entity
- `loom knowledge relate <from> <predicate> <to> [--weight <w>]` — Add relationship
- `loom knowledge search <query> [--limit <n>] [--json]` — Search entities
- `loom knowledge clear --force` — Clear the entire graph

### Daemon
- `loom daemon start` — Start loomd
- `loom daemon stop` — Stop loomd
- `loom daemon status [--json]` — Show daemon status
- `loom daemon restart` — Restart loomd
- `loom daemon install-service` — Install as user service
- `loom daemon uninstall-service` — Uninstall user service

### ACP
- `loom acp status [--json]` — Show ACP dispatch server status
- `loom acp start` / `loom acp stop` — Control the ACP dispatch server
- `loom acp show [--json]` — Show ACP dispatch server details
- `loom acp agents [--json]` — List launchable agents (daemon handlers + `config/agents.toml`)
- `loom acp registry [--available] [--json]` — List agents in the public ACP registry (`*` = registered locally)
- `loom acp run <agent> --prompt <text> [--cwd] [--timeout-secs] [--json] [-- args...]` — Run one ACP prompt turn, persisted as a Loom session
- `loom acp run-status <run_id> [--json]` — Show a tracked ACP run's status
- `loom add-acp-agent <agent_id>` — Register an agent from the ACP registry into `config/agents.toml`

Agent launch resolution (`loom-sessions/src/acp_launch.rs`): explicit
path → local `config/agents.toml` entry → public ACP registry → `PATH`
binary. The registry pick is deterministic: `npx` > `uvx` > `binary`
(package runners install on demand). A local entry with no `args` is
completed from the registry, because most agents need a protocol mode
flag (`acp`, `--acp`) before they speak ACP at all.

## Config Precedence (v0.3.10)

`ConfigReader::load()` builds the effective `LoomConfig` by deep-merging 5
layers, lowest → highest precedence (later wins on conflicts):

1. **Built-in defaults** — `LoomConfig::default()`. Always present.
2. **Extends chain** — paths in `config.toml`'s `extends = [...]`. Each
   path is `shellexpand::tilde`-expanded and resolved relative to the loom
   dir. Cycle detection uses a `HashSet<PathBuf>` with `MAX_EXTENDS_DEPTH = 10`.
3. **Local `config.toml`** — deep-merged on top of the extends chain.
4. **Layer overlays** — `config/routing.toml`, `config/mcp.toml`,
   `config/hooks.toml`, `config/acp.toml`. Each is wrapped under its
   section key (e.g. `config/routing.toml` deep-merges into `[routing]`).
5. **Cached remote config** — when `remote_config_url` is set in the
   merged tree and `~/.loom/.cache/remote_config/index.json` has an
   entry for the URL, the cached file is deep-merged on top. Non-HTTPS
   URLs and missing cache entries are silently skipped.

After all merging, `${LOOM_X}` env-var patterns in string values are
interpolated (`ConfigOptions::from_env()` does not control this — it's
unconditional and uses `std::env` lookups, no defaults).

Skip flags:
- `LOOM_NO_EXTEND=1` → skip layer 2 only.
- `LOOM_NO_REMOTE=1` → skip layer 5 only.
- `loom config show --extends [--no-extend] [--no-remote]` and
  `loom config validate [--no-extend] [--no-remote]` pass these through
  per-command.

Inspect the chain: `loom config show --extends` prints each layer with
its label, path, and contributed keys. `loom config show --extends --json`
emits `{ config, extends_chain: [...] }` for tooling.

## Known Gotchas

1. **Auth vault `export_credential()` re-encrypts** — the credential is encrypted in memory with a fresh random nonce (`vault.encrypt_for_export`, ChaCha20Poly1305); plaintext is never written to disk.
2. **`mcp_registry.db` is SQLite** — created automatically in `~/.loom/` by `McpRegistry`.
3. **ConfigReader deep-merges (not replaces) overlay files** — if `config/routing.toml` exists, it is deep-merged under `[routing]` on top of `config.toml` (see "Config Precedence" above). Set `LOOM_NO_EXTEND=1` or use `loom config show --extends` to inspect which keys each layer contributed.
4. **Session compression** (v0.3.9): When a session's message count exceeds `SESSION_COMPRESSION_THRESHOLD` (20), older messages are folded into a single `System`-role summary (id `loom-summary-v1`) at the head of the message list, and the last `SESSION_FULL_FIDELITY_COUNT` (10) messages are kept verbatim. Auto-runs on `append_message`/`replace_message` once a session crosses the threshold; can be triggered manually via `loom sessions summarize <id> [--force]`. The manifest's `summarized_at` / `summarized_message_count` fields record the pass.
5. **Vault lockout**: max `VAULT_MAX_UNLOCK_ATTEMPTS` (5) within `VAULT_MAX_UNLOCK_WINDOW_MS` (60s).
6. **Daemon auto-shutdown**: after `DAEMON_DEFAULT_IDLE_TIMEOUT_MS` (300s = 5min) of no IPC activity.
7. **TUI keybindings**: `D` opens action palette (daemon controls), `a` opens dispatch menu only on task tabs (5=Skills, 6=Sessions), `?` for help. Tab indices: 0=Dashboard, 1=Daemon, 2=Discovery, 3=MCP, 4=Skills, 5=Sessions, 6=Logs, 7=Knowledge, 8=PRs, 9=Runtime, 10=Kanban, 11=Graph, 12=Cost, 13=Orchestrator, 14=Review. Digits `1-9` select tabs 0-8, `0` selects tab 9 (Runtime), `!`/`@`/`#` select tabs 10-12. The command palette offers `tab 1`..`tab 11`, which map to tab indices 0-10 — Orchestrator and Review are reachable only by Tab cycling.
8. **Adapter binaries for TerminalLaunch**: checks PATH for `opencode`, `claude-code`, `jcode`, `crush` (in order).
9. **ACP dispatch is wired and tracked** — `acp.dispatch` spawns headlessly with per-run logs, records completion plus a Loom session, and serves `run.status`. A canonical ACP-spec client also exists (`loom acp run`, `loom-sessions/src/acp_protocol.rs`).
10. **EmbeddedPane dispatch** — opens the embedded pane and streams `workflow dispatch` output for the task.
11. **`loom-ipc` uses Unix sockets only** — no TCP or Windows named pipe support.
12. **All crates share the workspace version** — 0.9.6 (`[workspace.package] version` in the root `Cargo.toml`); the separate `ECOSYSTEM_VERSION` constant is 2.0.0.
13. **Knowledge extraction auto-runs on `loom sessions index` and `loom sessions export`** — entities and relationships are automatically extracted into `~/.loom/knowledge/graph.db` during session indexing.
14. **Knowledge graph boosts routing decisions** — when evaluating a prompt with `loom routing evaluate`, if the prompt mentions known entities in the knowledge graph, the complexity score receives a boost (up to +0.25) based on entity degree and type. Entities found are listed in `knowledge_entities` in the signals.
15. **Knowledge-enhanced session resume** — `loom sessions resume --inject-knowledge` queries the knowledge graph for entities matching the session topic and recent messages, then injects entity definitions and their relationships as a system message before resuming.

## TUI Architecture

- `ratatui` + `crossterm` backend
- 15 tabs, 1s auto-refresh
- Action palette for daemon operations (Start/Stop/Restart/InstallService/UninstallService)
- Dispatch menu on task tabs: TerminalLaunch (opens new terminal window with agent), ACPDispatch (wired — dispatches over daemon IPC), EmbeddedPane (implemented — opens the embedded pane and streams dispatch output)
- Keybinding cheat sheet accessible via `?`

## Directory Structure

```
crates/
├── loom-core/        — Types, config, constants, errors, traits
├── loom-cli/         — CLI binary with 20+ subcommands
├── loomd/            — Daemon binary
├── loom-tui/         — TUI (ratatui + crossterm)
├── loom-ipc/         — IPC protocol
├── loom-event-bus/   — Event bus
├── loom-discovery/   — Two-phase discovery engine
├── loom-skills/      — Skill registry, loader, adapter, hot-reload
├── loom-mcp/         — MCP server registry, pool, health checks
├── loom-auth/        — Credential vault, ACL, audit
├── loom-sessions/    — Session export/import, storage
├── loom-hooks/       — Hook registry, execution, signing (real script execution with exit code checking)
├── loom-workflow/    — Task orchestration and phases
├── loom-routing/     — Complexity scoring, model routing
├── loom-generic-adapter/  — TOML-driven tool adapter
├── loom-adapters/    — Preset adapters (aider, claude-code, codex, crush, cursor, gemini-cli, goose, jcode, juno, kiro, github-copilot, opencode)
├── loom-ast/         — AST structural editing with tree-sitter (P1/5.5)
├── loom-telemetry/   — Metrics, histograms, anomaly detection
├── loom-ffi/         — Foreign function interface
├── loom-conflict/    — Conflict resolution protocol (detect/resolve merge conflicts)
├── loom-debug/       — Debug Adapter Protocol (DAP) client integration
├── loom-hindsight/   — Hindsight memory — cross-session typed fact storage
├── loom-knowledge/   — Knowledge graph (SQLite entities/relationships, FTS5)
├── loom-lsp/         — Language Server Protocol (LSP) client integration
├── loom-mcp-server/  — MCP server exposing Loom's shared resources
├── loom-parallel/    — Concurrent agent execution, git-worktree isolation
├── loom-remote/      — SSH remote development
├── loom-rules/       — Stream rules engine for LoomEvent processing (P1/5.6)
├── loom-runtime/     — Pluggable runtime backends (process, Docker, Podman, Kubernetes)
├── loom-trackers/    — Issue tracker integration (GitHub Issues, Linear, Jira)
└── loom-integration-tests/ — Integration tests

packages/
├── jcode/            — jcode tool package
├── crush/            — Crush tool package
├── goose/            — Goose tool package
├── claude-code/      — Claude Code package
├── hooks/            — Hooks package
├── routing-hook/     — Routing hook scripts
├── opencode/         — OpenCode package
├── homebrew/         — Homebrew formula
```

## Integration Tests

Tests in `crates/loom-integration-tests/tests/`:

- **ecosystem.rs**: Skill portability, MCP pool, session export/import, auth provider import, hook enforcement (real script execution with exit code checking), workflow routing, cost optimization
- **routing_integration.rs**: Complexity scoring, config thresholds, phase transitions, cost optimization, manual overrides
- **cross_tool_workflow.rs**: Session sharing between tools, skill portability, MCP communication

## Constants

Key constants in `loom-core/src/constants.rs`:

- `ECOSYSTEM_VERSION`: "2.0.0"
- `DAEMON_DEFAULT_IDLE_TIMEOUT_MS`: 300000 (5 min)
- `MCP_DEFAULT_HEALTH_INTERVAL_MS`: 300000 (5 min)
- `HOOK_DEFAULT_TIMEOUT_MS`: 30000 (30s)
- `SESSION_COMPRESSION_THRESHOLD`: 20
- `SESSION_FULL_FIDELITY_COUNT`: 10
- `VAULT_MAX_UNLOCK_ATTEMPTS`: 5
- `VAULT_UNLOCK_WINDOW_MS`: 60000 (1 min)
- `AUDIT_LOG_MAX_SIZE_BYTES`: 10 MB
- `JOURNAL_ROTATION_LINES`: 2000

## Model Tiers

- **Slm**: Fast/Cheap (claude-haiku-3, gpt-4o-mini, gemini-2.5-flash)
- **Mid**: Balanced (claude-sonnet-4, gpt-4o, gemini-pro)
- **Flagship**: Slow/Expensive (claude-opus-4, gpt-5, gemini-ultra)

## Performance Budgets

| Operation | Budget | Achieved |
|-----------|--------|---------|
| Startup (daemon) | <50ms | ~20ms |
| Startup (no daemon) | <200ms | ~100ms |
| Skill search | <5ms | ~0.05ms |
| Skill activation | <50ms | ~30ms |
| MCP acquire | <20ms | ~5ms |
| Hot-reload | <60ms | ~15ms |
| Session resume | <100ms | ~50ms |
| Routing decision | <10ms | ~5ms |
| Hook execution | <30ms | ~30ms |
| Context injection | <5ms | ~2ms |
| Event delivery | <100us | ~15us |
| Vault decrypt | <10ms | ~3ms |
| Knowledge query (subgraph, depth 2) | <20ms | ~5ms |
| Knowledge build (100 sessions) | <5s | ~10ms (0 sessions with data) |
