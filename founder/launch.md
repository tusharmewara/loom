# Loom launch plan — general availability, v0.12.0

**Product:** unified agentic ecosystem — loomd daemon, CLI, TUI, MCP server; skills, sessions, MCP servers, auth vault, hooks, workflow shared across claude-code, codex, opencode, goose, cursor, aider, gemini-cli, kiro, juni, crush, jcode, github-copilot.
**Commercial model:** per-seat Team tier at $30/seat/month (numbers.json), enterprise SSO/FIPS/audit add-on. Break-even ≈ 715 seats/month ≈ 24 paid seats/day at full ramp.
**Launch day:** Monday, December 14, 2026 (earliest realistic given a 6-week commercial build: license gate, Stripe billing, entitlements).

---

## Step 1 — test before you spend (subscription → founding-member offer, paid up front)

**Test:** landing page at loom.sh/team with the real price ($30/seat/month) and a "Pre-order founding team" button. Payment captured up front for the first month; founding members lock $30/seat/month for 12 months, get enterprise SSO/FIPS/audit at launch price, and a direct Slack line to the founder. Run concurrently: 3 paid pilot teams at the planned price, onboarded manually through loom init + loomd, invoiced $90/month.

**Success line (written before launch):** 50 founding pre-orders and 3 paid pilots within 2 weeks (by Nov 11) = $1,500/month committed MRR + $270/month pilots.

**If real buyers fall well short:** trust the buyers, not the panel; return to founder/offer or founder/pricing before spending the $50k startup budget.

**Owner:** founder/CEO. **Deadline:** page live Nov 4; test ends Nov 11.

---

## Step 2 — countdown to opening day

Weekly blocks, each task with an owner and a deadline. Critical path marked (CP).

| Week | Dates | Task | Owner | Deadline | CP? |
|---|---|---|---|---|---|
| 1 | Oct 29–Nov 4 | Legal & money: entity, bank account, Stripe Connect account, terms + privacy for paid seats | CEO | Nov 4 | yes |
| 1 | Oct 29–Nov 4 | Pre-sale landing page live; success line posted internally | CEO/Marketing | Nov 4 | yes |
| 2 | Nov 5–Nov 11 | Commercial build: Ed25519 license gate, Stripe checkout + webhooks, seat entitlements in loomd, fail-closed default | Eng lead | Nov 11 | yes |
| 2 | Nov 5–Nov 11 | Pre-sale test closes; decide go/no-go on launch | CEO | Nov 11 | yes |
| 3 | Nov 12–Nov 18 | Brand: pricing page, tier comparison (Team vs Enterprise), launch messaging from founder/brand | Marketing | Nov 18 | no |
| 3 | Nov 12–Nov 18 | 3 paid pilots onboarded and invoiced (manual concierge) | Ops/Support | Nov 18 | no |
| 4 | Nov 19–Nov 25 | Marketing calendar from launch minus 14 days (starts Nov 30): Product Hunt, LinkedIn, X, devtool newsletters, waitlist email sequence | Marketing | Nov 25 | no |
| 4 | Nov 19–Nov 25 | Support SOPs: refund, entitlement restore, license-key reissue runbooks | Ops/Support | Nov 25 | no |
| 5 | Nov 26–Dec 2 | Soft open: friends-and-family day with 5 friendly teams, paid at $30/seat/month; collect breakage | Eng lead/Support | Dec 2 | no |
| 5 | Nov 26–Dec 2 | Enterprise SSO/FIPS/audit SKU priced and contract template ready | CEO | Dec 2 | no |
| 6 | Dec 3–Dec 9 | Release candidate 0.12.0-rc1, full QA of license gate + billing flow, rollback plan documented | Release eng | Dec 9 | yes |
| 6 | Dec 3–Dec 9 | Launch comms pre-written and scheduled; support on-call rota set | Marketing/Support | Dec 9 | yes |
| Launch | Mon Dec 14 | Public GA: v0.12.0, Team tier on sale, enterprise add-on available | All | Dec 14 | yes |

**Critical path:** Stripe billing + license gate (no entitlements = no revenue), commercial release build, pre-sale go/no-go. Everything else slips without moving the date.

---

## Step 3 — launch day run sheet (Monday, December 14, 2026)

| Time | Action | Channel | Owner |
|---|---|---|---|
| 08:00 | On-call eng + support on incident bridge; PagerDuty-ready | — | Eng lead |
| 09:00 | Health check: loomd socket, license gate, Stripe webhook endpoint, entitlement service | internal | Eng lead |
| 10:00 | Launch post: Product Hunt + LinkedIn + X; "Loom v0.12.0 — the shared agentic ecosystem, $30/seat/month" | public | Marketing |
| 11:00 | Email waitlist + founding pre-orders with purchase link | email | Marketing |
| 11:30 | Submit to devtool newsletters/HN | public | Marketing |
| 12:00 | Conversion watch: Stripe checkouts vs target (24 seats/day plan); block on webhook delays | Stripe dashboard | CEO |
| 13:00 | Lunch — eng stays on bridge; no deploys | — | — |
| 14:00 | Respond to early comments/reviews; escalate bugs to eng | public/social | Marketing |
| 15:00 | Day-1 numbers: seats sold, pilot activations, support tickets opened | internal | CEO |
| 16:00 | Breakage response (see below) | — | Eng lead |
| 17:00 | Day-1 report to founder; decide day-2 ad spend | internal | CEO |
| 19:00 | Stand-down; on-call handoff | — | Support |

**If something breaks (from the risk register):**
- License gate down / key validation failing: issue temporary 72-hour license keys manually via `loom auth grant issue`; never fail open — seats stay unentitled rather than free.
- Stripe webhook delayed or missing: grant entitlement manually on proof of payment; reconcile daily against Stripe exports.
- loomd crash: daemon auto-restarts on socket reconnect; clients retry automatically; page on-call if >2 restarts/hour.
- Pre-sale buyers can't pay: direct Stripe payment links sent manually; honor founding price regardless.

---

## Step 4 — first 30 days (Dec 14 – Jan 12)

**Numbers to track weekly (from founder/numbers.json):**
- Paid seats/day vs plan (30/day) and break-even (≈24/day); cumulative vs 715 seats/month
- New seats by channel (Product Hunt, LinkedIn, X, waitlist email, direct) with CAC per channel
- Churn (seats cancelled / total seats), support tickets/day, activation rate (installed → paid within 7 days)

**Change triggers:**
- <50% of plan seats/day for 5 straight days → pause paid channels, revisit offer/pricing (founder/offer, founder/pricing)
- Churn >5%/week → stop marketing spend, fix onboarding/activation first
- >10 support tickets/day → freeze non-critical features, eng works support queue

**Reviews:**
- Day 7 (Dec 21): Did we hit the day-1 seat target? Which channel converted? Any billing entitlement failures? Decide: scale channel or fix funnel.
- Day 14 (Dec 28): Week-2 seats/day vs break-even; churn in first week; pilot feedback (3 pilots still paying?). Decide: enterprise add-on push or Team-only focus.
- Day 30 (Jan 12): Month-1 seats vs 715/month break-even; CAC vs $30×12 LTV; refund rate. Decide: hire support, raise price, or extend founding offer.

**Next:** founder/plan compiles founder/brand.md, founder/offer, founder/pricing, founder/ops.md, founder/marketing.md, founder/numbers.json, and this file into the business plan with a computed verdict (Profitable / Not yet / Incomplete).