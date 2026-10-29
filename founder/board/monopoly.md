# Monopoly lens

Score: 5/10 | Vote: FUND IF

## What is strong
- Proprietary technology: the local daemon, JSON-line IPC, session format, and TOML-driven adapters are a real integration layer across 11 agent CLIs that is not trivial to replicate.
- Last-mover position: AI coding tooling is still fragmented; no incumbent owns "the layer between agents." Loom can be that layer if it gets there first.
- Niche to start: teams running 3+ different agents are a small, definable beachhead. Dominate them, then expand to single-agent teams and enterprise.

## What would kill it (top 3, most dangerous first)
1. Weak network effects: Loom is infrastructure, not a marketplace. One team using Loom does not make another team's Loom more valuable, so the classic monopoly engine is missing.
2. Incumbents can build it: Cursor, Codex, Copilot could add cross-tool session sharing natively, commoditizing Loom's core. MCP standardization also erodes the "share MCP servers" moat.
3. No visible economies of scale: supporting local daemons across macOS/Linux/WSL2 (no Windows) is a cost, not a scale advantage.

## The questions I would ask the founder
- What proprietary thing does Loom own that Cursor/Codex cannot ship in a quarter? (/founder-competitors)
- Is the niche (multi-agent teams) large enough to be worth monopolizing, or is it a rounding error?

## If FUND IF: the conditions
- Own one narrow wedge first: teams using >=3 agents. Do not expand to single-agent teams or enterprise until that wedge is dominated.
- Build a proprietary data asset (cross-agent knowledge graph / session corpus) that incumbents cannot replicate from their own siloed data.
