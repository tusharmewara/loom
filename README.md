# Loom – The Shared Agentic Ecosystem

Loom is a unified agentic ecosystem that enables seamless collaboration between multiple AI tools (jcode, crush, claude-code, opencode, goose, etc.) while sharing skills, sessions, MCP servers, and workflows.

## 🚀 Quick Start

### Prerequisites
- Rust 1.70+ (via rustup)
- Unix-like environment (Linux/macOS)

### Installation
```bash
git clone https://github.com/yourusername/loom.git
cd loom
cargo build --workspace --release
export PATH="$PWD/target/release:$PATH"

# Initialize the ecosystem
loom init

# Start the background daemon (optional, for faster CLI startup)
loom daemon start
```

### Using Loom
- **Cross‑tool sessions**: Start a session in jcode, export it, and continue in claude‑code.
- **Shared skills**: Write a skill once, use it from any tool.
- **MCP servers**: Register MCP servers once, share across tools.
- **Sub‑agent orchestration**: Decompose complex tasks across multiple agents.
- **Issue tracking**: Integrate with GitHub Issues and Linear for project management.
- **Workflow templates**: Reusable task templates for common workflows.

---

## 🏗️ Architecture

Loom is built as a Rust workspace with a daemon + CLI + TUI architecture.

### Crate Map

| Crate | Purpose |
|-------|---------|
| `loom-core` | Core types, constants, errors, and traits |
| `loom-cli` | CLI binary with 20+ subcommands (`loom`) |
| `loomd` | Background daemon (Unix socket IPC, auto-shutdown) |
| `loom-tui` | Terminal UI (ratatui + crossterm, 7 tabs) |
| `loom-ipc` | Unix socket JSON-line IPC protocol |
| `loom-event-bus` | In‑process (tokio broadcast) + cross‑process (ZeroMQ) event bus |
| `loom-discovery` | Two‑phase discovery engine for tools and resources |
| `loom-mcp` | MCP server registry, connection pool, and health checks |
| `loom-skills` | Skill registry, loader, adapter, and hot‑reload |
| `loom-sessions` | Session export/import, storage, and compression |
| `loom-auth` | Encrypted credential vault, ACL, and audit logging |
| `loom-hooks` | Hook registry, script execution, and signing |
| `loom-workflow` | Task orchestration, phases, and templates |
| `loom-routing` | Complexity scoring and intelligent model routing |
| `loom-trackers` | Issue tracker integration (GitHub Issues, Linear) |
| `loom-parallel` | Multi‑agent parallel execution engine |
| `loom-telemetry` | Performance metrics, histograms, anomaly detection |
| `loom-generic-adapter` | TOML‑driven tool adapter (auto‑derives 6 traits) |
| `loom-adapters` | Preset tool adapters (jcode, crush, claude‑code, opencode, goose) |
| `loom-ffi` | C FFI exports for external tool integration |
| `loom-mcp-server` | MCP server for Loom ecosystem services |
| `loom-integration-tests` | End‑to‑end integration tests |

Each crate shares the workspace version and depends on `loom-core` for types and traits.

### Two‑Tier Architecture

- **CLI mode** (`loom`): Connects to the daemon via Unix socket when available, or runs standalone.
- **Daemon mode** (`loomd`): Background daemon that aggregates all subsystems. Auto‑shutdown after 5 min idle.
- **TUI mode** (`loom tui`): Terminal UI with dashboard, daemon controls, discovery, MCP, skills, sessions, and logs tabs.

### IPC Protocol

JSON‑line messages over a Unix domain socket (`~/.loom/loomd.sock`):
```json
{"request_id":"uuid","command":"skills.list","params":{}}
{"request_id":"uuid","status":"ok","data":{...}}
```

---

## 🔧 Key Features

### Core Ecosystem
- **Shared Skills**: Write a skill once, use it across all tools.
- **Session Portability**: Export a session from one tool, import into another. Compression after 20 messages.
- **MCP Server Sharing**: Register MCP servers once, share across all tools.
- **Secure Auth Vault**: Store credentials in an encrypted vault (Chacha20Poly1305) with ACL and audit log.
- **Hook System**: Intercept pre/post tool events with custom scripts and exit‑code verification.
- **Workflow Orchestration**: Multi‑phase task management with provider assignment and phase advancement.
- **Sub‑Agent Orchestration**: Decompose tasks across sub-agents with configurable dispatch modes (sequential, parallel, fan-out).

### Issue Tracking
- **GitHub Issues**: List, show, create branches, add comments.
- **Linear**: Full GraphQL integration (list by team, show, add comments, create branches).
- **Extensible trait**: `IssueTracker` trait for adding more backends.

### Intelligent Routing
- **Complexity Scoring**: Score prompts by token/structural complexity.
- **Model Tiers**: SLM (fast/cheap), Mid (balanced), Flagship (powerful/expensive).
- **Configurable thresholds**: Route based on score with manual overrides.

### Workflow Templates
- **TOML‑based**: Store reusable task templates in `~/.loom/workflow/templates/`.
- **Create from template**: `loom workflow create --template <name>`.

### Daemon
- **Auto‑shutdown**: Idle timeout after 5 minutes.
- **Launchd/systemd service**: `loom daemon install-service`.
- **Health checks**: Every subsystem implements `HealthCheck` trait; aggregated on startup.

### Performance Budgets

| Operation | Budget | Achieved |
|-----------|--------|---------|
| Startup (daemon) | <50ms | ~20ms |
| Startup (no daemon) | <200ms | ~100ms |
| Skill search | <5ms | ~0.05ms |
| MCP acquire | <20ms | ~5ms |
| Routing decision | <10ms | ~5ms |
| Hook execution | <30ms | ~30ms |
| Event delivery | <100µs | ~15µs |
| Vault decrypt | <10ms | ~3ms |

---

## 📦 Installation

### From Source
```bash
git clone https://github.com/yourusername/loom.git
cd loom
cargo build --workspace --release
```

### Pre‑built Binaries
Pre‑built binaries for Linux and macOS are available on the [GitHub Releases](https://github.com/yourusername/loom/releases) page.

---

## 🤝 Contributing

Contributions are welcome! Please read the [CONTRIBUTING.md](CONTRIBUTING.md) guide for details on how to set up a development environment, run tests, and submit patches.

---

## 📄 License

Loom is licensed under the MIT License. See the `LICENSE` file for details.

---

## 📦 Version History

- **0.2.0**: Sub‑agent orchestration, GitHub Issues & Linear integration, workflow templates, multi‑agent parallel execution.
- **0.1.3**: CLI modernization, hooks system, routing engine, hot-reload.
- **0.1.2**: Session compression, auth vault, TUI.
- **0.1.1**: MCP pool, skill sharing, event bus.
- **0.1.0**: Initial MVP with basic MCP and skill sharing.
