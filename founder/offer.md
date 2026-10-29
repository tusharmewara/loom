# The Loom offer

## Problem list (from buyer panel + competitor complaints)

1. **Trust:** "I don't trust a new vendor with my code and session data." — 40 buyers, top blocker.
2. **Price:** "$30/seat adds up fast when I have 10+ agents." — 23 buyers.
3. **Habit:** "We already pay for Cursor/Copilot; why add this?" — 12 buyers.
4. **Need:** "Cursor/Copilot already share context inside my editor." — 11 buyers.
5. **Convenience:** "Daemon + adapters + MCP servers is more infra than I want." — 1 buyer.
6. **Compliance:** No SSO, audit log, FIPS. — compliance buyers blocked entirely.
7. **Lock-in:** "What happens if Loom shuts down? My context is locked in." — repeated across trust and need answers.

## The stack

### Core: Loom Team seat
Shared agentic ecosystem — one local daemon (`loomd`) that every AI coding agent (Claude Code, Codex, Cursor, Goose, Aider, Crush, Gemini CLI, Kiro, GitHub Copilot) connects to through a Unix socket. Shared skills, sessions, MCP servers, hooks, workflows, and credentials. No per-tool configuration; no context silos.

- Value to buyer: 5 (daily context-loss pain is real for 3+ agent teams)
- Cost to deliver: 2 (already built, shipped in v0.12.0)
- **Keep: high value, low cost.**

### Bonus 1: Local-first architecture
Everything runs on a local daemon over a Unix socket — nothing leaves the machine unless the user chooses to sync. Addresses the #1 blocker (trust, 40 buyers).

- Value: 5
- Cost: 1 (already the architecture)
- **Keep.**

### Bonus 2: First five humans free
No cost to try. The pricing panel found $30 is inside the acceptable range ($9.89-$60.19), but 23 buyers cited price. Free tier removes the barrier for small teams.

- Value: 4
- Cost: 1 (no marginal cost per seat at small scale; contribution is $27/seat at $30)
- **Keep.**

### Bonus 3: Export guarantee (no lock-in)
All skills, sessions, MCP config, and credentials exportable to plain files. Works without the daemon. Addresses lock-in fear (repeated across trust and need).

- Value: 5
- Cost: 2 (export tooling already exists; sessions are JSONL)
- **Keep.**

### Bonus 4: Launch-week founding-member pre-sale
Real price ($30/seat/month), 3 paid pilot teams get priority support and direct roadmap input. Addresses habit/Cursor-loyalty (12 buyers) by creating urgency at a real price point.

- Value: 3
- Cost: 1 (priority support already included)
- **Keep.**

### Not in the stack (cost too high vs value)
- SSO/audit log/FIPS as a free feature: enterprise buyers need it but it's an upsell, not part of the core offer. Listed in founder/enterprise roadmap.
- Open-source daemon: valuable for trust but a strategic decision beyond this offer's scope.

## The guarantee

**30-day money-back guarantee.** If a team doesn't recover at least one engineer-hour per week of context-switching time, refund the full first month. 

Model: $30/seat × ~10 seats average team = $300/refund worst case. At a realistic 5% claim rate on 100 teams = $1,500/month exposure. Covered by contribution margin ($27/seat × ~300 seats = $8,100/month contribution). Affordable per CFO numbers.

## Urgency and scarcity

Launch week (Dec 14, 2026 GA): founding-member pre-sale at the real price, 3 paid pilot teams only. True scarcity — limited pilot capacity, not fake countdowns.

## The name

**"Loom Team"** — one daemon, every agent, no lost context.

## Value equation (before → after)

| Element | Before | After | What moved it |
|---------|--------|-------|---------------|
| Dream outcome | 3 | 8 | Core shared-context layer + export guarantee |
| Perceived likelihood | 4 | 7 | Local-first architecture + third-party audit path |
| Time to result | 6 | 8 | One-command install, first five humans free |
| Effort and sacrifice | 7 | 4 | Free tier removes price barrier; export removes lock-in fear |

Overall: **20 → 27**. The stack moves every element: trust (local-first + audit path), price (free tier + guarantee), habit (proven value > Copilot context lock-in), need (proven daily context-loss pain).

## Re-test

The updated offer should be written into `founder/pitch.md` (old version archived as `founder/pitch-v1.md`) and re-tested with `/founder-consumer --quick`. Key objections that should drop: trust (40 → lower with local-first + audit path), price (23 → lower with free tier + guarantee). If buy rate doesn't move from 11%, the problem is the market or price, not the offer.

## Costs (from CFO numbers)

- Variable per seat: $3.00 (compute $0.80 + storage $0.40 + card fees $1.17 + support $0.63)
- Contribution at $30: $27.00 (90%)
- Break-even: 25 seats/day
- Guarantee exposure at 5% claim rate on 100 teams: ~$1,500/month (covered by contribution margin)
