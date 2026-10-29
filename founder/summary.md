# Summary

**What Loom is and who it's for, in two sentences.**

Loom is a unified agentic ecosystem: one local daemon (`loomd`) that every AI coding agent — Claude Code, Codex, Cursor, Goose, Aider, Crush, Gemini CLI, Kiro, GitHub Copilot — connects to through a fast Unix socket, sharing skills, sessions, MCP servers, hooks, workflows, and credentials so a task started in one agent can be continued in another without losing context. It's for engineering leads whose teams run three or more AI coding agents and are tired of context getting locked inside whichever tool started it.

**The verdict: Not yet.**

The numbers behind it:

| Number | Value | What it means |
|---|---|---|
| Contribution margin | 90% ($27/seat at $30) | Each seat leaves $27 after its own direct costs — strong unit economics. |
| Break-even | 25 seats/day | Below that you lose money every month; steady-state plan is 30/day. |
| Year 1 operating profit | -$14,820 | Operations lose money in year one; startup spend ($14,000) isn't earned back until after month 12. |
| Cash needed before it pays for itself | $59,170 | Cumulative trough happens in month 4 at ~$59K. |
| Buyer panel buy rate | 11% (11 of 98) | Well below the 25% bar that would validate demand. |

**What the buyer panel and the board agreed on, and where they disagreed.**

Agreed: Loom solves a real daily pain — context loss when switching between AI agents — for teams running 3+ agents. The local-daemon architecture (nothing leaves the machine) is genuinely differentiating and earns trust from at least some buyers. The first-five-humans-free tier removes the price barrier for small teams.

Disagreed: The board voted **fund with conditions** (average 5/10) while the buyer panel voted **no buy** (11% vs the 25% bar). The board saw the Offers lens at 6/10 ("fund if"); the Product lens at 4/10 ("pass"). The gap is willingness-to-pay evidence: buyers say the pain is real but won't pay $30/seat for an unproven vendor with no track record, no audit, and no trusted peer vouching for it yet.

**The biggest risk and what will be done about it.**

Biggest risk: **trust**. 40 of 98 simulated buyers passed explicitly because they won't hand their code and session data to an unknown vendor with no track record, no audit, and no one they trust vouching for it. Compliance buyers (SSO/FIPS/audit) are blocked entirely until those features exist.

What will be done about it — in order:
1. Get a third-party security audit of the daemon (local-first, nothing leaves the machine).
2. Ship SSO (SAML/OIDC), audit logging, and FIPS as an enterprise add-on — these are compliance gates, not nice-to-haves.
3. Guarantee plain-file export of every skill, session, MCP config, and credential — context is never locked in if Loom shuts down.
4. Get real production reviews from engineering leads running 3+ agents before spending the $50k startup budget.

**What the founder needs to start.**

From the CFO: **$59,170** cash before it pays for itself (cumulative trough in month 4). From launch: **3 paid pilot teams** as proof of demand before spending the startup budget. From the buyer panel: real production reviews from teams running 3+ agents — or don't spend until they exist.

**The one thing to do this week.**

Run a founding-member pre-sale at the real price ($30/seat/month) with 3 paid pilot teams. Success line: 50 pre-orders and 3 pilots in 2 weeks ($1,500/month committed). If real buyers fall short, trust the buyers, not the panel — return to the offer or pricing before spending the $50k startup budget.

_The panel is simulated buyers and the numbers are projections from your inputs. Confirm demand with real customers and costs with real quotes before you spend. Not financial, legal or tax advice._
