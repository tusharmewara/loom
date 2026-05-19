# Loom Status

## Overview

Loom is a multi-agent orchestration framework built in Rust. It provides a unified interface for skills, MCP servers, and cross-tool adapters.

## Crates

- `loom-core`: Types, config, constants, errors, traits
- `loom-cli`: CLI binary with 20+ subcommands
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
- `loom-telemetry`: Metrics, histograms, anomaly detection
- `loom-ffi`: Foreign function interface (minimal)

## Packages

- `jcode`: JCode tool package
- `crush`: Crush tool package
- `claude-code`: Claude Code package
- `opencode`: OpenCode package
- `goose`: Goose tool package
- `hooks`: Hooks package
- `routing-hook`: Routing hook scripts

## Features

- Two-tier architecture: CLI (standalone or daemon-connected) and daemon
- IPC over Unix domain sockets
- Event bus with in-process and cross-process (ZeroMQ) support
- Skill registry with hot-reload
- MCP server registry and health checks
- Credential vault with encryption
- Session export/import with compression
- Hook system for pre/post task execution
- Workflow engine for task orchestration
- Routing for model selection
- Telemetry and anomaly detection

## Build & Test

```bash
cargo build --workspace
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
cargo install loom-cli
```

### From Source

```bash
git clone https://github.com/loom/loom.git
cd loom
cargo build --release
```

## Usage

```bash
loom --help
```

## Configuration

Main config: `~/.agents/config.toml`

## Known Issues

- Hook scripts are placeholders (do not actually execute)
- Auth vault export returns plaintext (should re-encrypt)
- MCP registry creates new instance per request (state not shared across restarts)

## Roadmap

- Implement real hook script execution
- Add proper credential export encryption
- Persist MCP registry state across restarts
- Improve telemetry and anomaly detection
- Add more cross-tool adapters

## Contributors

See README for contribution guidelines.