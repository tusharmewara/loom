# Loom Vision

*Generated from commit history (18 commits, all pushed to `main`, tagged `v2.0.0`)*

## What Loom Is

Loom is a **shared agentic ecosystem** — a unifying layer that connects multiple AI coding tools (jcode, crush, claude-code, opencode, goose) so they can share:

- **Skills** — write once, use from any tool
- **MCP servers** — register once, available everywhere
- **Sessions** — export from one tool, import into another
- **Auth** — encrypted credential vault with ACLs
- **Hooks** — intercept and modify tool behavior
- **Workflow** — cross-tool task orchestration

It's written in Rust as a Cargo workspace of 22 crates plus 6 tool-adaptation packages.

## What's Been Built (pushed to git)

The codebase is substantial — ~28k lines across 184 files — but represents a single development day (May 19, 2026). The 18 commits tell the story:

### Phase 1 — TUI Foundation (commits 1-10)
The earliest commits built out the **terminal UI** (`loom-tui` crate):
- `Dispatcher` trait for launching tools from the TUI (TerminalLaunch, ACPDispatch, EmbeddedPane)
- Keyboard navigation: dispatch menu (`a`), daemon action palette (`D`), help overlay (`?`)
- Tab-based layout with auto-refresh (Dashboard, Daemon, Discovery, MCP, Skills, Sessions, Logs)
- Task selection and dispatch flow

### Phase 2 — CLI & Ecosystem (commits 11-16)
- `loom init` now supports registering tools during initialization
- Homebrew bottle support and CI/CD pipeline
- Binary renamed from `loom-cli` to `loom` for consistent entry point
- Formula generation for macOS/Linux distribution

### Phase 3 — Polish & Release (commits 17-18)
- Race condition fixes in CI/CD
- Package naming corrections

## Architecture At A Glance

```
┌─────────────────────────────────────┐
│  CLI (loom)    TUI    Daemon (loomd) │
│                (ratatui)             │
├─────────────────────────────────────┤
│  Core subsystems (discovery, mcp,   │
│  skills, sessions, auth, hooks,     │
│  workflow, routing, telemetry)      │
├─────────────────────────────────────┤
│  Adapters (generic + preset per     │
│  tool: jcode, crush, claude-code,   │
│  opencode, goose)                   │
├─────────────────────────────────────┤
│  IPC (Unix sockets) + Event Bus     │
│  (in-process + ZeroMQ cross-process)│
└─────────────────────────────────────┘
```

## Design Principles

1. **Two-tier performance**: CLI works standalone (slower) or via daemon (fast, cached). Daemon auto-shuts down after 5 min idle.
2. **TOML-driven tool adapters**: The `GenericAdapter` derives all 6 Loom traits from a single `{tool}.toml` file. Missing sections = no-op, never errors.
3. **Non-blocking**: Even partial implementations are functional — `loom-hooks` has placeholder script execution, `ACPDispatch` and `EmbeddedPane` gracefully report "not yet wired".
4. **Security-conscious**: Encrypted credential vault (ChaCha20Poly1305 + Argon2), hook signing (SHA-256), ACL-based access control, command injection hardening in dispatch layer.
5. **Observable**: Every crate implements a `HealthCheck` trait; daemon aggregates and reports all on startup. Telemetry crate provides 3-sigma anomaly detection for latency regressions.

## What's Still Missing (clear gaps)

- **No CI workflow files** in `.github/workflows/` — Homebrew release workflow was built but not committed
- **No comprehensive test suite** — unit tests exist per crate but no CI runner
- **Hook execution is placeholder** — `execute_hook_script()` returns hardcoded "success"
- **ACP dispatch not wired** — reports "requires daemon IPC" but no implementation
- **EmbeddedPane dispatch** — explicit "not yet implemented"
- **Auth vault export returns plaintext** — noted as "proper export would re-encrypt"
- **Multiple protocol registries** in the `acp_registry/` dir — notably this is a *separate* repository (has its own README, license, AGENTS.md, gitignore) that was mounted into this workspace for sharing ACP agent schemas

## The Big Picture Vision

Loom is positioned to become the **cross-cutting layer** that makes AI coding tooling composable rather than siloed. Instead of each tool (jcode, crush, claude-code, opencode, goose) maintaining its own skills, MCP configs, sessions, and auth, Loom provides one shared ecosystem. It's an **interoperability fabric** — not a new tool itself, but the thing that makes existing tools work together.
