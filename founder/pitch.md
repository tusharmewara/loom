# Loom pitch

**Loom** is the shared layer between your AI coding agents. Install it once and every tool — Claude Code, Codex, Cursor, Goose, Aider, Crush, Gemini CLI, Kiro, GitHub Copilot — shares the same skills, sessions, MCP servers, hooks, workflows, and credentials. A task started in one agent continues in another without losing context.

One daemon (`loomd`) runs locally and owns every shared subsystem. Your agents connect through it via a fast Unix socket. No per-tool configuration, no context silos.

**Pricing:** $30 per seat per month (Team tier). First 5 humans free. One seat = one human, unlimited agent connections.

**Open offer:** Founding-member pre-sale at the real price — 3 paid pilot teams get priority support and direct input on the roadmap.

**Get it:** `brew tap tusharmewara/loom && brew install loom` or `cargo install --path crates/loom-cli`. Then `loom init` and `loom daemon start`.
