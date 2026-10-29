# Loom — founder marketing plan

## Evidence note

`founder/panel/results.md`, `founder/competitors.md`, `founder/offer.md`, and
`founder/pricing.md` do not exist yet, so no panel quotes, competitor names, or
offer details are used here. Numbers below come from `founder/numbers.json`
($30/seat/month, $2 variable, $20,000 fixed monthly, $50,000 startup). The
positioning and channels are drawn from the product context (unified agentic
ecosystem: loomd daemon, CLI, TUI, MCP server; per-seat Team tier; enterprise
SSO/FIPS/audit logging). Run `/founder-consumer --quick` before spending to
validate objections and segments.

## Positioning

Three versions of the statement, then the recommendation.

1) Team lead, unified control plane

For engineering leads whose developers context-switch between a half-dozen AI
coding tools, Loom is the unified agentic ecosystem that puts one daemon, one
CLI, one TUI, and one MCP surface behind every seat, unlike buying point tools
and wiring them together yourself.

2) Security / compliance buyer

For engineering orgs that must centralize agent access with SSO, FIPS, and audit
logging, Loom is the enterprise-ready agent ecosystem that logs and controls
every agent action, unlike open agent tools with no central control plane.

3) Individual contributor who outgrew chat

For developers who have hit the ceiling of single-agent chat, Loom is the
agentic operating system for coding that runs agents across daemon, CLI, and
TUI, unlike a single chat window with no system underneath.

**Recommended: version 1.** Per-seat Team tier is the commercialization model,
so the buyer is the engineering lead who approves seats; version 1 leads with
the gap (one control plane vs. stitched point tools) that matches Loom's
architecture.

Panel evidence: none yet — no `founder/panel/results.md`. Confirm this segment
buys most before launch.

## Channels

Where this buyer actually spends attention, with fit, cost, and signal.

| Channel | Why it fits this buyer | Rough cost | How you know it worked |
|---|---|---|---|
| GitHub presence + dev content (README, repo, release notes, short "how it works" posts) | Devs discover and evaluate coding tools on GitHub; the loomd/CLI/TUI/MCP stack is repo-native | Time + ~$300/mo for small boosts | Repo stars, clone traffic, README-to-install conversion |
| LinkedIn/X engineering-leader posts + direct outreach to team leads | The seat buyer is the engineering lead; per-seat sales start with a conversation | Time + ~$1,500/mo small paid social | Meetings booked, trial signups attributed to outbound |

Start with these two. Not doing:

- Paid search — no category intent yet for a new unified agent ecosystem; expensive for an unproven offer.
- Conference sponsorships, brand advertising — no panel evidence that this audience converts there; too early.
- Aggressive cold email — trust barrier too high for a dev tool; damages deliverability before the list is warm.

Reconsider after 60 days if trials are below plan.

## 30-day campaign

Dated calendar from launch minus 14 to launch plus 16. Day 0 = public launch.

**Ten hooks** — each answers one buyer objection or uses one buyer reason:

1. "One control plane for every AI coding agent your team runs." (answers: "another tool to manage")
2. "Stop paying for six point tools; pay for one seat, run daemon, CLI, TUI, MCP." (answers: cost / seat math)
3. "Your agents, your log. SSO, FIPS, audit logging built in." (answers: security/compliance objection)
4. "It plugs into your repos, not the other way around." (answers: lock-in / migration fear)
5. "The daemon runs in the background; your CLI and TUI just work." (answers: complexity / setup effort)
6. "You already use Copilot. Loom is the layer above it — agents that act, not just suggest." (answers: "we already have AI")
7. "Per-seat pricing. No enterprise minimum. Start with one team." (answers: "too expensive for our size")
8. "One bill, one audit trail, one offboarding point when someone leaves." (answers: admin overhead)
9. "If it doesn't ship code in your repo, it's not doing its job." (answers: "does it actually ship code?")
10. "Founding teams: first 30 days free, then $30/seat/month." (launch offer hook)

**Launch-week offer** (provisional — confirm against `founder/pricing.md`):
First 30 days free for founding teams, then $30/seat/month. Ends Day 14
(launch plus 14). No fake scarcity; the date is real.

**When there is nothing new to say:** behind-the-scenes posts — how loomd starts,
what the MCP surface exposes, a CLI walkthrough, a TUI screenshot, "why we
built a daemon instead of a plugin." One per week, format matched to channel
(short video for X/LinkedIn, repo README update for GitHub).

**Calendar:**

- Day -14 to Day -10: GitHub repo public, README live with hook 1/2/4; one dev
  content post on X/LinkedIn per day (Day -14, -12, -10).
- Day -9 to Day -5: Direct outreach to 5 engineering leads/day; hook 3, 7, 8 in
  messages. One "behind the scenes" post (Day -7).
- Day -4 to Day -1: Launch announcement drafted, hook 5/6/9 prepared; paid
  social small test ($100 total); outreach continues.
- Day 0: Launch post on GitHub + X/LinkedIn with hook 10 + offer; Product Hunt
  style launch thread if the audience is there; email the outbound list.
- Day 1-3: One hook per day (2, 4, 5); outreach follow-ups; behind-the-scenes
  post Day 2.
- Day 4-7: Hooks 6, 7, 8, 9 one per day; collect first trial feedback.
- Day 8-14: Reminder that the launch offer ends Day 14 (Day 10, Day 12, Day 14);
  case study from first paying team if one exists (Day 12).
- Day 15-16: Post-launch review; decide which channel gets more budget.

## Budget and numbers

**Budget split** (two channels to start, ~$2,000/month):

- Dev content + GitHub boosts: $500/month (25%)
- LinkedIn/X posts + small paid social: $1,000/month (50%)
- Direct outreach (time, no media cost): counted as labor
- Contingency: $500/month (25%)

**Most you can pay to win a customer** (from numbers.json):

Contribution per seat per month = $30 − $2 = $28.
If a seat stays 3 months, gross contribution = $28 × 3 = $84.
Break-even CAC = $84 per seat. Target CAC ≤ $40 for a ~1.5-month payback;
anything above $84 loses money on acquisition.

Fixed monthly $20,000 → break-even at $20,000 ÷ $28 ≈ 715 paid seats.
Startup $50,000 → recovered once cumulative contribution covers it (roughly
month 4-5 at plan ramp).

**Three numbers to watch weekly, and the "change something" level:**

1. New paid seats per week vs plan ramp (plan = 30/day ≈ 210/week at steady
   state). Change something if below 70% of plan for two straight weeks.
2. Trial-to-paid conversion. Change something if below 10% for two straight
   weeks.
3. Blended CAC. Change something if above $40; stop spending if above $84.

No fake reviews, follower counts, testimonials, before/afters, or "as seen on."
Paid partnerships disclosed if any are added later.
