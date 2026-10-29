# Brief: Loom

Loom is a unified agentic ecosystem — a dev tool / B2B SaaS that lets multiple AI coding agents (claude-code, codex, opencode, goose, aider, cursor, gemini-cli, kiro, juno, crush, github-copilot) share skills, sessions, MCP servers, hooks, workflows, and credentials. A central loomd daemon owns every shared subsystem; the CLI, TUI, and MCP server are thin clients over a local Unix socket. Task started in one agent can be continued, reviewed, and finished in another without losing context.

Target customer: software teams running more than one AI coding agent, buying per seat.
Product and price: seat subscription at $30/seat/month; enterprise tier adds SSO, FIPS, audit log, vault RBAC, org registry.
Delivery: online, installed via Homebrew / crates.io / source; daemon runs locally on macOS / Linux / WSL2; no native Windows.
Status: v0.12.0 released. Commercialization phases 1-8 complete: license gate, control plane MVP, vault RBAC, org registry, Stripe billing, analytics, enterprise SSO/FIPS/audit. Rust workspace, ~32 crates plus 8 tool-adaptation packages.
