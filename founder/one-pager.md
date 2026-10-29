# One-pager: Loom

## The problem

Teams running multiple AI coding agents (Claude Code, Codex, Cursor, Goose, Aider, Crush, Gemini CLI, Kiro, GitHub Copilot) lose context every time they switch between them. Context is locked inside whichever tool started it — skills, sessions, MCP servers, hooks, workflows, and credentials don't travel. An agent that knows your codebase in one tool is a stranger in the next.

## The customer

Engineering leads at teams running 3+ AI coding agents — AI-native startups, mid-market software teams, and enterprise dev-ops/platform teams — who approve per-seat tooling and need SSO, audit logging, and control over agent access.

## The offer and price

**"Loom Team" — $30/seat/month, first 5 humans free.**

One local daemon (`loomd`) that every AI coding agent connects to through a Unix socket. Shared skills, sessions, MCP servers, hooks, workflows, and credentials across all agents. Start a task in one agent, finish it in another — no lost context.

**Included in the stack:**
- Local-first architecture — nothing leaves your machine unless you choose to sync
- First five humans free — no cost to try
- Plain-file export of every skill, session, MCP config, and credential — context is never locked in
- 30-day money-back guarantee — if a team doesn't recover one engineer-hour per week of context-switching time, full refund

**Enterprise add-on (upsell, not in core):** SSO (SAML/OIDC), audit logging, FIPS mode, config encryption.

## The numbers table

| Number | Value |
|---|---|
| Price | $30/seat/month |
| Variable cost per seat | $2.00/month |
| Contribution per seat | $27.00/month (90% margin) |
| Fixed costs | $20,000/month |
| Startup spend | $50,000 |
| Break-even | ~25 seats/day (715/month) |
| Plan volume | 30 seats/day |
| Year 1 operating profit | -$14,820 |
| Cash needed before it pays for itself | $59,170 |
| Break-even reached | after month 12 |

## The evidence (panel + competitors)

**Buyer panel (simulated, 98 buyers):** 11% buy rate — below the 25% validation bar.
- **Trust (40 buyers):** won't hand code/session data to an unknown vendor with no audit or track record
- **Price (23 buyers):** $30/seat adds up for solo devs and teams on free tiers
- **Habit (12 buyers):** already pay for Cursor/Copilot Enterprise; reluctant to add another tool
- **Need (11 buyers):** editor already shares context inside one tool; cross-agent value not yet felt
- **What would flip a no:** third-party security audit, self-host option, SSO/FIPS/audit, guaranteed export path, trusted peer vouching

**Competitors:** Cursor ($20/$60/$200), Windsurf/Codeium ($20/$200), GitHub Copilot ($10/$19/$39), Devin ($20/$200), LangSmith ($0/$39/seat + per-trace), CrewAI (free/$25/Enterprise), Augment Code ($20/$100/Enterprise). All are either single-tool assistants (context doesn't travel), orchestration platforms (no shared sessions/skills/vault), or enterprise-only ($40-100+/seat or $16K/yr).

**The gap:** nobody sells a shared, agent-agnostic agentic ecosystem at a per-seat price. Loom owns "the shared layer between AI coding agents" — vendor-neutral breadth (12 adapters + generic TOML adapter) plus SSO/FIPS/audit control plane.

## What the money is for

$59,170 cash needed before the business pays for itself (cumulative trough in month 4). Covers:
- Cloud infrastructure, engineering salaries, SaaS tools, insurance, legal/accounting ($20,000/month fixed)
- Startup: Delaware C-Corp, legal, trademark, launch marketing, contingency ($50,000)
- Recovered once cumulative contribution covers it (roughly month 4-5 at plan ramp)

## The ask

**3 paid pilot teams** at the real price ($30/seat/month) — proof of demand before spending the $50k startup budget. Success line: 50 founding pre-orders and 3 pilots in 2 weeks ($1,500/month committed MRR).

If real buyers fall short of that line, trust the buyers, not the panel — return to the offer or pricing before spending.

_The panel is simulated buyers and the numbers are projections from your inputs. Confirm demand with real customers and costs with real quotes before you spend. Not financial, legal or tax advice._
