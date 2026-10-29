# Loom

- What it is: A unified agentic ecosystem that lets multiple AI coding agents (claude-code, codex, opencode, goose, aider, cursor, gemini-cli, kiro, juno, crush, github-copilot) share skills, sessions, MCP servers, hooks, workflows, and credentials through a central loomd daemon, with a CLI, TUI, and MCP server as clients.
- Who it is for: Software teams running more than one AI coding agent who need context to survive a tool switch; B2B, per-seat.
- What it sells, at what price: Seat subscription at $30/seat/month; enterprise tier with SSO, FIPS, audit log, vault RBAC, org registry (commercialization phases 1-8 complete).
- Where and how: Online; installed via Homebrew, crates.io, or source; daemon runs locally on macOS/Linux/WSL2 (no native Windows).
- Budget and constraints: v0.12.0 just released; Rust workspace of 32 crates plus 8 tool-adaptation packages; license gate, control plane MVP, vault RBAC, org registry, Stripe billing, analytics, and enterprise SSO/FIPS/audit all built.
