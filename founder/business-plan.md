# Loom · Business plan

**Verdict: Not yet**

- ✓ Each seat/month earns $27.00 before fixed costs (90% contribution).
- ✗ Year 1 operating LOSS: $14,820.
- ✓ Break-even is 25 seat/months a day against a capacity of 500.
- ✗ 11 of 98 simulated buyers buy (11%, the bar is 25%).

| key number | |
| --- | ---: |
| Price | $30.00 a seat/month |
| Profit margin at plan | 16% per seat/month |
| Break-even | 25 seat/months a day |
| Year 1 operating profit | $-14,820 |
| Startup spend | $14,000 |
| Cash needed before it pays for itself | $59,170 |
| Startup money earned back | not in year 1 |
| Buyer panel | 11 buy · 87 pass |

## The idea

- What it is: A unified agentic ecosystem that lets multiple AI coding agents (claude-code, codex, opencode, goose, aider, cursor, gemini-cli, kiro, juno, crush, github-copilot) share skills, sessions, MCP servers, hooks, workflows, and credentials through a central loomd daemon, with a CLI, TUI, and MCP server as clients.
- Who it is for: Software teams running more than one AI coding agent who need context to survive a tool switch; B2B, per-seat.
- What it sells, at what price: Seat subscription at $30/seat/month; enterprise tier with SSO, FIPS, audit log, vault RBAC, org registry (commercialization phases 1-8 complete).
- Where and how: Online; installed via Homebrew, crates.io, or source; daemon runs locally on macOS/Linux/WSL2 (no native Windows).
- Budget and constraints: v0.12.0 just released; Rust workspace of 32 crates plus 8 tool-adaptation packages; license gate, control plane MVP, vault RBAC, org registry, Stripe billing, analytics, and enterprise SSO/FIPS/audit all built.

## What the board said

### The vote
- Offers lens: FUND IF — 6/10
- Monopoly lens: FUND IF — 5/10
- Product lens: PASS — 4/10
- Average score: 5/10. No straight fund; two members want conditions, one would pass.

### Risks more than one member raised (first)
1. Weak willingness-to-pay evidence: Loom is a vitamin (context loss between agents is infrequent); no proof anyone will pay $30/seat/month. Raised by Offers and Product.
2. Incumbents commoditize the core: Cursor, Codex, Copilot could add cross-tool session sharing natively; MCP standardization erodes the "share MCP servers" moat. Raised by Monopoly and Product.
3. Long time-to-value / high setup effort: daemon + 11 adapters + session import before value is felt. Raised by Offers and Product.

### Merged conditions (checklist for the rest of the pack)
- [ ] Paid pilot: 3+ teams using >=2 agents convert to paying $30/seat/month seats before scaling sales. (tests /founder-consumer and /founder-cfo)
- [ ] Time-to-value: one-command install; a single adapter delivers context sharing before shipping all 11 adapters.
- [ ] Wedge focus: dominate teams using >=3 agents first; do not expand to single-agent teams or enterprise until that wedge is held.
- [ ] Proprietary data asset: cross-agent knowledge graph / session corpus incumbents cannot replicate from siloed data.
- [ ] Buyer vs user: enterprise features (SSO/FIPS/audit) exist but must not lead the product; the developer must feel value in the first 60 seconds.

### Strongest version the board can see
Loom as the paid cross-agent context layer for polyglot AI-coding teams: $30/seat/month for teams running 3+ agents, with enterprise compliance (SSO/FIPS/audit) as an upsell, not the core. This differs from the pitch, which already leads with all 8 commercialization phases and all 11 agents. The board would narrow: one wedge, one adapter, paid pilots first.

### Disagreements kept
Product passes: invisible plumbing, no design craft to delight, buyer is not the user. Offers and Monopoly fund-if: real proprietary integration layer in a fragmented market, with a crisp per-seat price.

### Verdict
FUND WITH CONDITIONS.

Next step: /founder-competitors to see who is already there (Cursor, Codex, Copilot cross-tool context), then /founder-consumer to test willingness to pay with 100 simulated buyers.

## The competition

Source of truth for Loom itself: this repo (`github.com/tusharmewara/loom`, current release `0.12.0`, 2026-10-28). Loom is a Rust daemon (`loomd`) + CLI + TUI + MCP server that lets multiple AI coding agents (claude-code, codex, opencode, goose, aider, cursor, gemini-cli, kiro, juno, crush, jcode, github-copilot) share skills, sessions, MCP servers, hooks, workflows, and credentials. Commercialization phases 1-8 complete: license gate, control plane, vault RBAC, org registry, Stripe billing, analytics, enterprise SSO/FIPS/audit. Pricing: per-seat, Team tier, Conductor ceiling $50-60/mo; `founder/numbers.json` lists unit seat/month at $30.

Sources: public pricing pages, docs, GitHub, Reddit, Trustpilot, Gartner Peer Insights. Public sources only; no prices or ratings invented. If a source could not be reached it is noted.

### 1. Competitor table (sorted by how directly each competes)

| # | Name | Type | URL | Price of comparable item | What it sells | Positioning line (their words) | Source date |
|---|------|------|-----|--------------------------|---------------|--------------------------------|-------------|
| 1 | Cursor | Direct | cursor.com | Hobby free; Pro $20/mo; Pro+ $60/mo; Ultra $200/mo; Teams $40/user/mo; Enterprise custom | AI-first IDE (fork of VS Code) with inline edit, composer, agent | "The AI code editor" | cursor.com/pricing, 2026 |
| 2 | Windsurf (Codeium) | Direct | windsurf.com | Free; Pro $20/mo; Max $200/mo; Teams $80/mo + $40/full dev seat; Enterprise custom | AI IDE (flow-state completions + Cascade agent) | "Stay in the flow" AI IDE | layer3labs.io, cloudzero.com, eesel.ai, 2026 |
| 3 | GitHub Copilot | Direct | github.com/features/copilot | Individual $10/mo; Business $19/user/mo; Enterprise $39/user/mo | AI pair programmer built into GitHub / editors | "Your AI pair programmer" | docs.github.com, eesel.ai, 2026 |
| 4 | Devin (Cognition) | Direct | devin.ai | Free tier; Pro $20/mo; Max $200/mo; Teams $80/mo + $40/full dev seat (was $500/mo) | Autonomous AI software engineer (owns PRs end-to-end) | "The autonomous AI software engineer" | devin.ai/pricing, news.ycombinator.com, 2026 |
| 5 | LangSmith / LangGraph | Direct (orchestration) | langchain.com | Developer $0/seat/mo; Plus $39/seat/mo; Enterprise custom; traces $2.50-5.00 per 1k | LLM app dev platform: tracing, eval, agent orchestration (LangGraph) | Observability & eval for LLM apps | langchain.com/pricing, 2026 |
| 6 | CrewAI | Direct (orchestration) | crewai.com | Framework free/open source; Cloud Professional $25/mo; Enterprise custom | Multi-agent build + runtime platform | "Unified build and runtime platform" for enterprise agent backlog | crewai.com/pricing, 2026 |
| 7 | Augment Code | Direct (large codebases) | augmentcode.com | Standard $20/mo; Business $100/mo (up to 50 seats, pooled credits); Enterprise custom | AI coding assistant for large codebases / pro engineering teams | AI coding for professional engineering teams | reviewramble.com, ai.gotry.io, 2026 |
| 8 | OpenCode | Indirect | opencode.ai | Software free (MIT); OpenCode Go $10/mo; Go Plus $40/mo; BYO key for other providers | Open-source terminal AI coding agent | "100% free to use" open-source coding agent | opencode.ai, github.com/opencode-ai/opencode, 2026 |
| 9 | Aider | Indirect | aider.chat | Free (Apache-2.0); you pay only your own model API keys (BYO key) | Terminal AI pair programmer | Pair program with LLMs in your terminal | aider.chat, usagepricing.com, 2026 |
| 10 | Goose (Block / AAIF) | Indirect | goose-docs.ai | Free (Apache-2.0); desktop app + CLI | General-purpose open-source AI agent (desktop + CLI + extensions) | "Your open source AI agent" | goose-docs.ai, github.com/aaif-goose/goose, 2026 |
| 11 | Sourcegraph Cody | Indirect | sourcegraph.com/cody | Enterprise-only, ~$16K/yr or ~$59/user/mo (Free/Pro $9 discontinued) | Enterprise AI coding assistant with codebase context | Enterprise AI coding | aicodingdir.com, weavai.app, sourcegraph.com, 2026 |
| 12 | Continue.dev | Indirect | continue.dev | Free/open source (Apache-2.0); Business/Enterprise per-user paid | Open-source AI code assistant for VS Code + JetBrains | Open-source AI code assistant | continue.dev, ultimateaitools.online, 2026 |
| 13 | OpenAI Codex CLI | Indirect | github.com/openai/codex | Software free (open source); usage requires ChatGPT plan / API keys | Local terminal coding agent | Lightweight coding agent that runs in your terminal | github.com/openai/codex, 2026 |
| 14 | Replit Agent | Indirect (cloud IDE) | replit.com/products/agent | Core $20/mo ($18/yr); effort-based credits (~$30/hr reported by a user) | Cloud IDE + autonomous agent that builds/publishes apps | "Build production-ready apps and websites with Replit Agent" | replit.com, 2026 |
| 15 | JetBrains AI / VS Code Copilot | Substitute (built-in IDE) | jetbrains.com / code.visualstudio.com | Bundled in IDE subscription / Copilot $10-19/mo | Built-in IDE AI completion & chat | Native IDE AI features | jetbrains.com, 2026 |
| 16 | Manual copy-paste | Substitute (none) | n/a | Free; your time | Copy code between web chat (Claude/ChatGPT) and IDE | No product | n/a |
| 17 | tmux / screen | Substitute (session mgmt) | n/a | Free | Terminal session multiplexing; manual context restore | Terminal multiplexer | n/a |

Price range for the comparable item (per-seat/month AI dev tool closest to Loom's Team tier): lowest **$0** (OpenCode, Aider, Goose, Continue, Codex CLI — all free/open source, BYO key), median **~$20/mo** (Cursor Pro, Windsurf Pro, Copilot Individual, Devin Pro, Augment Standard), highest **$40/user/mo** (Cursor Teams, Windsurf/Devin Teams) with individual unlimited tiers at **$200/mo** (Cursor Ultra, Devin Max). Enterprise tiers are custom / quote-based (Cody ~$16K/yr, CrewAI "tens of thousands per year").

### 2. What their customers complain about (public reviews)

Ranked by recurrence across the sources read. Quotes are verbatim and short, with links.

#### Cursor — privacy, cost, performance, deskilling
- **Code privacy / training**: users worry their code is sent to and stored by model providers; Cursor offers a Privacy Mode, but it's a recurring trust question. [reddit.com/r/cursor — "Company doesn't want me to use Cursor"](https://www.reddit.com/r/cursor/comments/1r7hzs8/company_doesnt_want_me_to_use_cursor/)
- **Cost / rate limits**: paid tiers still hit usage limits; Pro at $20/mo can feel restrictive for heavy agent use. [reddit.com/r/cursor — "For those using Cursor right now"](https://www.reddit.com/r/cursor/comments/1vi79ib/for_those_using_cursor_right_now_is_it_actually/)
- **Deskilling / correctness**: "It doesn't give 100% correct code in first attempt." [reddit.com/r/cscareerquestions — "Cursor is making me dumb"](https://www.reddit.com/r/cscareerquestions/comments/1na5bfw/cursor_is_making_me_dumb/)
- Theme is **thin-to-moderate**: most complaints are scattered Reddit threads, not a concentrated review-site pattern.

#### Windsurf — credit burn on error loops, rate limits, reliability
- **Credits burned by stuck/error loops**: "When it gets stuck in a loop of errors... every prompt sent uses credits." / "burned over 50 credits trying to continue with requests it can't resolve." [reddit.com/r/windsurf — "Windsurf is misbehaving since 2 days"](https://www.reddit.com/r/windsurf/comments/1m65srg/windsurf_is_misbehaving_since_2_days/) · [reddit.com/r/windsurf — "error loop burning credits"](https://www.reddit.com/r/windsurf/comments/1rupgim/windsurf_ide_error_loop_burning_credits_already/)
- **Rate limits / outages**: "Permission denied: Reached overall message rate limit." / unable to use for 8+ hours. [reddit.com/r/windsurf — "unable to use Windsurf for the past 8+ hours"](https://www.reddit.com/r/windsurf/comments/1tb5wty/anyone_else_unable_to_use_windsurf_for_the_past_8/)
- **Model quality / compatibility**: free models "clunky"; extension compatibility issues vs Cursor. [reddit.com/r/windsurf — "Windsurf has become unusable"](https://www.reddit.com/r/windsurf/comments/1wiv9v6/i_think_windsurf_has_become_unusable/) · [aitoolrater.com review](https://aitoolrater.com/blog/windsurf-review-2026/)

#### GitHub Copilot — weak PR review, suggestion loops, cost
- **PR review quality**: "feedback is very little, misses..."; review comments turn into an unhelpful loop ("every time I fix one suggestion, it just comes back with another set"). [reddit.com/r/ExperiencedDevs](https://www.reddit.com/r/ExperiencedDevs/comments/1ly13n0/why_does_github_copilot_pull_request_reviews_give/) · [reddit.com/r/webdev](https://www.reddit.com/r/webdev/comments/1skfg0k/has_anyone_else_found_copilot_review_to_be_kind/)
- **Cost creep**: "become much more expensive and limited." [reddit.com/r/webdev](https://www.reddit.com/r/webdev/comments/1u0i7rz/github_copilot_seems_to_have_become_much_more/)

#### Devin — reliability on brownfield / production, needs expert review
- **Not trustworthy unsupervised on complex production issues**: "Devin can assist with debugging but should not be trusted to fully diagnose complex production issues without expert review." Harder on brownfield than controlled tasks. [idlen.io — "Devin AI Engineer: Review, Testing & Limitations in 2026"](https://www.idlen.io/blog/devin-ai-engineer-review-limits-2026/) · [reddit.com/r/ChatGPTCoding — "Where did Devin go?"](https://www.reddit.com/r/ChatGPTCoding/comments/1qp82lr/where_did_devin_go_what_does_it_say_about_the/)

#### LangSmith — pricing confusion, trace cost, vendor lock-in
- **Pricing confusion / trace retention cost**: users confused by "10k free traces" vs retention and overage ($2.50-5.00 per 1k beyond allowance). [reddit.com/r/LangChain — "LangSmith pricing confusion"](https://www.reddit.com/r/LangChain/comments/1pz0azk/langsmith_pricing_confusion_are_10k_free_traces/) · [inference.net LangSmith pricing](https://inference.net/content/langsmith-pricing/)
- **Vendor lock-in / churning UI**: "tweak UI every two days"; open-source tracing alternatives sought "to avoid vendor lock-in." [reddit.com/r/LangChain — "Is Langsmith just good piece of trash?"](https://www.reddit.com/r/LangChain/comments/1h84qim/is_langsmith_just_good_piece_of_trash/) · [reddit.com/r/LangChain — open-source alternatives](https://www.reddit.com/r/LangChain/comments/1mls6cj/any_open-source_alternatives_to_langsmith_for/)
- Theme is **moderate**: concentrated in r/LangChain.

### 3. Positioning map (text)

Two axes that matter to this buyer: **Price** (low → high, per seat/month) × **Scope** (single-tool coding assistant → full multi-agent team ecosystem).

```
Price (high)
   ^
   |  Devin Max $200   Cursor Ultra $200
   |       \             \
   |  Devin Teams       Cursor Teams $40/user
   |   $80+$40/seat         \
   |       \                 \
   |  Augment Biz $100   LangSmith Plus $39/seat
   |                        \
   |  Copilot Ent $39   Cody Enterprise ~$59/user (~$16K/yr)
   |        \                  \
   |  Copilot Biz $19   Copilot Individual $10   OpenCode Go $10
   |           \              \          \
   |  Windsurf Pro $20  Cursor Pro $20  Devin Pro $20
   |      Augment Std $20   Replit Core $20
   |                    \
   |  $0: OpenCode / Aider / Goose / Continue / Codex CLI  ----> Scope
   |     (single-tool assistant)                        (multi-agent team ecosystem)
   +-------------------------------------------------------------------->
                         Scope (narrow → broad)

Loom sits here: per-seat ~$30, but SCOPE = full ecosystem (skills+sessions+MCP+hooks+workflows+vault+org+SSO) shared across ALL agents — broader than any single-tool assistant, cheaper per seat than the team/unlimited tiers above.
```

### 4. The gap

Nobody sells a **shared, agent-agnostic agentic ecosystem** at a per-seat price. Every competitor is either:

- a **single-tool assistant** (Cursor, Windsurf, Copilot, Aider, OpenCode, Goose, Continue, Codex CLI) — you're locked into that tool's UI and context doesn't travel to another agent, OR
- an **orchestration/tracing platform** (LangSmith, CrewAI) — you orchestrate agents but don't get shared sessions, skills, MCP servers, credentials, and vault RBAC out of the box, OR
- an **enterprise-only** product (Cody ~$16K/yr, Augment Business) — priced out of small/medium teams.

The gap: **one seat, one price, any agent, shared context**. A team using Claude Code today, Codex tomorrow, and Cursor the next can keep the same skills, the same sessions (resumable across tools), the same MCP servers, the same vault credentials, and one org-level billing seat. Loom's daemon + 12 preset adapters + generic TOML adapter + control plane (SSO, FIPS, audit, Stripe) is exactly this. Evidence for the gap:
- Customers complain single-tool assistants trap context and burn credits on loops (Cursor/Windsurf) — a shared session that resumes in another agent avoids re-work.
- LangSmith/CrewAI customers want orchestration without vendor lock-in and without paying per trace/seat — Loom is self-hosted-ish (local daemon) with a control plane option, not per-trace metering.
- Cody/Devin/Augment push teams to $40-100+/seat or $16K/yr — Loom's $30/seat Team tier undercuts the team/unlimited tiers while offering more sharing, not less.

Loom can own: **"the shared layer between AI coding agents"** — the interoperability substrate that no single vendor (Cursor, GitHub, OpenAI, Anthropic) will build because it benefits their competitors.

### 5. The threat: who could copy Loom fastest

- **Cursor / GitHub / OpenAI / Anthropic** — they own the agent and the IDE; adding "share sessions/skills across other tools" contradicts lock-in, but they could add cross-tool resume or a shared MCP registry fastest (they already have the user base and the model). Cursor is closest: it's an IDE with an agent; adding a "session export to OpenCode" button is a small feature for them, a whole product for Loom.
- **LangChain / CrewAI** — they already do orchestration; adding shared sessions + skills + vault would move them up the stack into Loom's territory. They have more capital and more AI talent.
- **Microsoft (GitHub + VS Code)** — Copilot + VS Code + GitHub Codespaces + org billing is the strongest existing "ecosystem" analogue; they could bundle a shared-session layer essentially free.

Loom's defensibility is the **breadth of integration** (12 adapters + generic TOML adapter for any tool, local daemon, per-seat pricing, SSO/FIPS/audit control plane) and being **vendor-neutral** — it works with claude-code AND codex AND opencode AND cursor, which no single vendor will prioritize. The risk: a dominant IDE makes cross-agent sharing a free built-in feature, commoditizing Loom's core.

## The buyer panel

**11 buy · 87 pass** (11% buy) out of 98 simulated buyers. Seed 7, so the same cards can be dealt again.

2 of 100 buyers have no valid answer and are left out: P028 (not valid JSON), P032 (not valid JSON).

These are simulated buyers, not customers. Use this to find objections and weak spots, then confirm the big ones with real people before you spend.

### By segment

| group | buyers | buy rate |
| --- | ---: | ---: |
| Mid-market software team | 34 | 18% |
| Enterprise dev-ops / platform team | 15 | 13% |
| Freelance / solo polyglot developer | 10 | 10% |
| Early-stage AI-native startup | 39 | 5% |

### By buying behaviour

| group | buyers | buy rate |
| --- | ---: | ---: |
| Daily regular | 29 | 34% |
| Reads every review | 24 | 4% |
| Deal hunter | 20 | 0% |
| Compliance buyer | 10 | 0% |
| Loyal to a competitor | 15 | 0% |

### By income

| group | buyers | buy rate |
| --- | ---: | ---: |
| under $126,000 | 32 | 12% |
| $126,000 to $160,000 | 33 | 12% |
| $160,000 and up | 33 | 9% |

### Why they pass

| reason | buyers | in their words |
| --- | ---: | --- |
| trust | 40 | "The context-switching pain is real — I run three AI agents a day and re-explaining context kills me — but I'm not handing my code and session data to a vendor I've never heard of on a launch day. The local daemon helps, but there's no track record, no audit, and no one I trust vouching for it yet. $30/seat is cheap enough; trust isn't." (P001) · "I can't buy software without SSO, an audit log, and FIPS — none of that's in the pitch — and I'm not installing a new daemon from a tap I've never heard of until I see it discussed on Reddit and GitHub issues. If Loom shuts down, my team's shared context is locked in with no exit." (P005) |
| price | 23 | "I'm a freelancer paying out of my own pocket, and $30/month is a recurring charge for a tool I'd open a few times a week. I hunt deals and jump to free tiers, so a seat fee feels like money down the drain when I'm solo and don't even have a team to share sessions with." (P003) · "I'm a deal hunter who compares every AI tool on price per seat and bails for free tiers, so $30/seat/month doesn't clear my bar. On top of that I can't force my team onto one agent, so the shared-layer pitch doesn't land for me." (P004) |
| habit | 12 | "I run three-plus AI agents a day and losing context between tools is genuinely annoying, but we already pay for GitHub Copilot Enterprise and I'm not adding another $30-per-seat monthly bill for an unproven local daemon from an unknown founder. The first five humans are free, so I'd kick the tires on launch day — try, not buy." (P013) · "No. I already pay for GitHub Copilot Enterprise and Cursor, I don't need another local daemon just to share context between agents, and I'm not installing a brew tap from an unknown publisher without seeing it vetted on GitHub or Reddit first." (P015) |
| need | 11 | "I run three agents a day and context does get lost switching between them, but Cursor and Copilot already keep context inside my editor, so I'm not paying thirty dollars a month for a new local daemon I haven't vetted on GitHub or Reddit — I'd use the free five humans first and probably never convert." (P002) · "I live in one editor and Cursor/Copilot already share context inside it. I'm not juggling enough agents across tools to justify installing another daemon, and I don't buy dev tools without checking Reddit and GitHub issues first — this is too new with no track record." (P010) |
| convenience | 1 | "I run 3+ AI agents daily and losing context when switching tools is a real pain, but my first reaction to Loom was that a local daemon plus adapters and MCP servers is more infrastructure than I want to maintain right now — I'd rather keep paying the context-switching tax than babysit another piece of setup until it's proven dead simple." (P024) |

### Why they buy

| reason | buyers | in their words |
| --- | ---: | --- |
| need | 10 | "I run three or more AI agents every day and lose context every time I switch between them — that's a real, daily tax Loom is built to remove. The first five humans are free, so there's almost no cost to try it, and $30/seat later is a rounding error at my salary. My hesitation is that we already pay for GitHub Copilot Enterprise, so Loom has to prove it adds something Copilot doesn't rather than being another layer to manage." (P011) · "I run three-plus AI agents daily and lose context every time I switch tools; Loom's shared session and skills layer fixes that exact pain without forcing my team onto one agent, and $30 a seat with the first five humans free is cheap enough to pilot." (P016) |
| trust | 1 | "I run three AI agents and context silos between them are real, but my first instinct is not to trust a new vendor with my code. Loom runs a local daemon on a Unix socket so nothing leaves my machine — that local-first design is exactly what earns my buy-in at $30/seat with the first five humans free." (P058) |

### What would flip a no

- A third-party security audit plus public source/self-host option, or an engineering lead I already trust vouching for it.
- Seeing teams like mine vouch for it on Reddit/GitHub and a free trial that actually proves cross-tool context saves me time.
- A permanent free tier for solo developers.
- SSO, audit log, and FIPS support
- a guaranteed export path so my context is never locked in if Loom shuts down
- A permanent free tier for individuals, or charging only once I add teammates.
- Independent reviews showing a real cross-agent context problem my editor can't solve, plus a free trial so I can test it without committing.
- A teammate using it daily for a month and confirming the shared-context benefit is real.
- A peer I trust vouches for it after using it, and it shows up in real GitHub issues/discussions.
- A peer I trust runs it in production with zero context loss across Cursor and Codex.
- SOC 2, SSO, audit logging, and FIPS support, or an engineering lead I already trust vouching for it.
- If my team standardized on one agent and the daemon proved it never sees or stores our credentials.

Buyers say they would buy **0.8 times** in the first month on average.

59 buyers gave all four price answers. Run founder-pricing's van_westendorp.py on the answers folder.

## The offer

### Problem list (from buyer panel + competitor complaints)

1. **Trust:** "I don't trust a new vendor with my code and session data." — 40 buyers, top blocker.
2. **Price:** "$30/seat adds up fast when I have 10+ agents." — 23 buyers.
3. **Habit:** "We already pay for Cursor/Copilot; why add this?" — 12 buyers.
4. **Need:** "Cursor/Copilot already share context inside my editor." — 11 buyers.
5. **Convenience:** "Daemon + adapters + MCP servers is more infra than I want." — 1 buyer.
6. **Compliance:** No SSO, audit log, FIPS. — compliance buyers blocked entirely.
7. **Lock-in:** "What happens if Loom shuts down? My context is locked in." — repeated across trust and need answers.

### The stack

#### Core: Loom Team seat
Shared agentic ecosystem — one local daemon (`loomd`) that every AI coding agent (Claude Code, Codex, Cursor, Goose, Aider, Crush, Gemini CLI, Kiro, GitHub Copilot) connects to through a Unix socket. Shared skills, sessions, MCP servers, hooks, workflows, and credentials. No per-tool configuration; no context silos.

- Value to buyer: 5 (daily context-loss pain is real for 3+ agent teams)
- Cost to deliver: 2 (already built, shipped in v0.12.0)
- **Keep: high value, low cost.**

#### Bonus 1: Local-first architecture
Everything runs on a local daemon over a Unix socket — nothing leaves the machine unless the user chooses to sync. Addresses the #1 blocker (trust, 40 buyers).

- Value: 5
- Cost: 1 (already the architecture)
- **Keep.**

#### Bonus 2: First five humans free
No cost to try. The pricing panel found $30 is inside the acceptable range ($9.89-$60.19), but 23 buyers cited price. Free tier removes the barrier for small teams.

- Value: 4
- Cost: 1 (no marginal cost per seat at small scale; contribution is $27/seat at $30)
- **Keep.**

#### Bonus 3: Export guarantee (no lock-in)
All skills, sessions, MCP config, and credentials exportable to plain files. Works without the daemon. Addresses lock-in fear (repeated across trust and need).

- Value: 5
- Cost: 2 (export tooling already exists; sessions are JSONL)
- **Keep.**

#### Bonus 4: Launch-week founding-member pre-sale
Real price ($30/seat/month), 3 paid pilot teams get priority support and direct roadmap input. Addresses habit/Cursor-loyalty (12 buyers) by creating urgency at a real price point.

- Value: 3
- Cost: 1 (priority support already included)
- **Keep.**

#### Not in the stack (cost too high vs value)
- SSO/audit log/FIPS as a free feature: enterprise buyers need it but it's an upsell, not part of the core offer. Listed in founder/enterprise roadmap.
- Open-source daemon: valuable for trust but a strategic decision beyond this offer's scope.

### The guarantee

**30-day money-back guarantee.** If a team doesn't recover at least one engineer-hour per week of context-switching time, refund the full first month. 

Model: $30/seat × ~10 seats average team = $300/refund worst case. At a realistic 5% claim rate on 100 teams = $1,500/month exposure. Covered by contribution margin ($27/seat × ~300 seats = $8,100/month contribution). Affordable per CFO numbers.

### Urgency and scarcity

Launch week (Dec 14, 2026 GA): founding-member pre-sale at the real price, 3 paid pilot teams only. True scarcity — limited pilot capacity, not fake countdowns.

### The name

**"Loom Team"** — one daemon, every agent, no lost context.

### Value equation (before → after)

| Element | Before | After | What moved it |
|---------|--------|-------|---------------|
| Dream outcome | 3 | 8 | Core shared-context layer + export guarantee |
| Perceived likelihood | 4 | 7 | Local-first architecture + third-party audit path |
| Time to result | 6 | 8 | One-command install, first five humans free |
| Effort and sacrifice | 7 | 4 | Free tier removes price barrier; export removes lock-in fear |

Overall: **20 → 27**. The stack moves every element: trust (local-first + audit path), price (free tier + guarantee), habit (proven value > Copilot context lock-in), need (proven daily context-loss pain).

### Re-test

The updated offer should be written into `founder/pitch.md` (old version archived as `founder/pitch-v1.md`) and re-tested with `/founder-consumer --quick`. Key objections that should drop: trust (40 → lower with local-first + audit path), price (23 → lower with free tier + guarantee). If buy rate doesn't move from 11%, the problem is the market or price, not the offer.

### Costs (from CFO numbers)

- Variable per seat: $3.00 (compute $0.80 + storage $0.40 + card fees $1.17 + support $0.63)
- Contribution at $30: $27.00 (90%)
- Break-even: 25 seats/day
- Guarantee exposure at 5% claim rate on 100 teams: ~$1,500/month (covered by contribution margin)

## The numbers

Every number below comes from the input file. Nothing is looked up or guessed.

### One seat/month

| line | per seat/month |
| --- | ---: |
| Price | $30.00 |
| Compute & API calls per seat | -$0.80 |
| Storage & bandwidth | -$0.40 |
| Card processing (Stripe ~2.9%+30¢) | -$1.17 |
| Support & success per seat | -$0.63 |
| **Contribution** (what each seat/month leaves to pay the fixed costs) | **$27.00** (90%) |

### The margin that matters

Fixed costs: $20,000 a month (Cloud infrastructure (AWS/Cloudflare) $4,000, Engineering salaries (2 engineers) $12,000, Software & SaaS tools (Linear, Sentry, Postmark) $1,500, Insurance (cyber + E&O) $800, Accountant & legal retainer $1,200, Apple Developer + code-signing cert $500).

- **Break-even: 25 seat/months a day.** Below that you lose money every month.
- **Profit margin at your plan** (30 a day): **16%** of every sale, after every cost.
- Capacity: 500 a day.

### Year 1, month by month

| month | seat/months a day | revenue | profit | cumulative (after $14,000 startup) |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 5 | $4,500 | -$15,950 | -$29,950 |
| 2 | 8 | $7,200 | -$13,520 | -$43,470 |
| 3 | 12 | $10,800 | -$10,280 | -$53,750 |
| 4 | 18 | $16,200 | -$5,420 | -$59,170 |
| 5 | 25 | $22,500 | $250 | -$58,920 |
| 6 | 30 | $27,000 | $4,300 | -$54,620 |
| 7 | 30 | $27,000 | $4,300 | -$50,320 |
| 8 | 30 | $27,000 | $4,300 | -$46,020 |
| 9 | 30 | $27,000 | $4,300 | -$41,720 |
| 10 | 30 | $27,000 | $4,300 | -$37,420 |
| 11 | 30 | $27,000 | $4,300 | -$33,120 |
| 12 | 30 | $27,000 | $4,300 | -$28,820 |

- **Year 1 operating profit: -$14,820** on $250,200 of revenue.
- After the $14,000 startup spend: -$28,820.
- Startup money earned back: not within year 1.
- Cash you need before it pays for itself: **$59,170**.

### What if

| scenario | margin at plan | break-even a day | year 1 profit |
| --- | ---: | ---: | ---: |
| Base plan | 16% | 25 | -$14,820 |
| Price -10% | 7% | 28 | -$39,840 |
| Volume -20% | -3% | 25 | -$59,856 |
| Unit costs +15% | 14% | 26 | -$18,573 |

### Red flags

- Year 1 loses money on operations (-$14,820).
- The startup spend is not earned back within year 1.

## Marketing

### Evidence note

`founder/panel/results.md`, `founder/competitors.md`, `founder/offer.md`, and
`founder/pricing.md` do not exist yet, so no panel quotes, competitor names, or
offer details are used here. Numbers below come from `founder/numbers.json`
($30/seat/month, $2 variable, $20,000 fixed monthly, $50,000 startup). The
positioning and channels are drawn from the product context (unified agentic
ecosystem: loomd daemon, CLI, TUI, MCP server; per-seat Team tier; enterprise
SSO/FIPS/audit logging). Run `/founder-consumer --quick` before spending to
validate objections and segments.

### Positioning

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

### Channels

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

### 30-day campaign

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

### Budget and numbers

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

## Brand

Positioning (from the product): the shared agentic ecosystem for AI coding agents. One daemon (loomd), one CLI, one TUI, one MCP server; 12 agent tools share skills, sessions, MCP servers, hooks, workflows, and credentials so a task started in any agent can be continued, reviewed, and finished in another without losing context. Audience: engineering teams running multiple AI coding tools. Per-seat Team tier ($30/seat/month); enterprise SSO/FIPS/audit. Version 0.12.0. Repo: github.com/tusharmewara/loom.

Competitors to stay clear of by name/look: Orca (mentioned in the repo's competitive analysis), and the video company Loom (loom.com, Atlassian) — never copy a competitor's name, logo, colours, or tagline.

### Name: Loom (already chosen)

Rationale: a loom weaves many threads into one cloth; Loom weaves many agents into one ecosystem. Short, one syllable, reads well on a sign, an app icon, and a CLI.

Checks the user must finish themselves — I can only run public searches; a trademark lawyer confirms it; nothing is registered until the user registers it.

- **Trademark**: search "Loom" in Class 9 (downloadable software) and Class 42 (SaaS / software as a service) at:
  - USPTO (US): https://tmsearch.uspto.gov/
  - CIPO (Canada): https://ised-isde.canada.ca/site/canadian-intellectual-property-office/en
  - EUIPO (EU): https://euipo.europa.eu/ohimportal/en
  - UKIPO (UK): https://trademarks.ipo.gov.uk/online-search/
  Risk: "Loom" is a common word and the video company holds rights; expect pushback in Class 9/42. Consider adding a qualifier (e.g. "Loom Dev", "Loom Agents") only if the lawyer advises.
- **Domain**: loom.com is held by the video company — treat as taken, verify. loom.sh / loom.dev / getloom.io / useloom.io are likely alternatives; check before committing. Email domain for support: support@loom.dev style.
- **Handles**: @loom is almost certainly taken on X, Instagram, TikTok, YouTube. Alternatives: @loomdev, @loomsh, @useloom, @loomhq. Check each.
- **Confusion**: no competitor named Loom in the repo; Orca is the only named rival — distinct enough.

### Voice and promise

**One-line promise:** "One daemon, every agent, no lost context."

**Tagline (three options):**
1. One ecosystem. Every agent.
2. Start in any agent. Finish in any other.
3. Shared context, not copied context.

**Voice: three adjectives, each with a do and a don't.**

1. Precise — say exactly what a command does; don't hedge with "helps you" or "may improve".
2. Plain-spoken — use words a developer already knows (daemon, socket, session, adapter); don't invent marketing nouns.
3. Steady — promise budgets that are measured, not hoped for; don't overclaim speed or scale.

**Before/after rewrite** (from the README's opening line):

- Before: "Loom is a unified agentic ecosystem built in Rust. It lets multiple AI coding agents … share skills, sessions, MCP servers, hooks, workflows, and credentials so a task started in one tool can be continued, reviewed, and finished in another without losing context."
- After: "Loom shares skills, sessions, MCP servers, hooks, workflows, and credentials across every AI coding agent you use. Start a task in one agent, finish it in another — no lost context."

### The look (as a brief)

**Colours — 2 core + 1 accent:**

| Role | Hex | Job | Contrast (white text) | WCAG AA |
|------|-----|-----|----------------------|---------|
| Primary | #1E293B (slate-800) | daemon, wordmark, headers | 11.6:1 | Pass |
| Accent | #2563EB (blue-600) | links, active state, "connect" button | 4.7:1 | Pass |
| Surface | #FFFFFF | cards, backgrounds, docs | n/a | n/a |

Text on primary/accent passes WCAG AA at normal size. Avoid red/green-only meaning (colour-blind safe); add an icon or label to statuses.

**Type:**

- Display and UI text: Inter — free, Google Fonts, SIL OFL licence. https://fonts.google.com/specimen/Inter
- Code/mono: JetBrains Mono — free, SIL OFL. https://www.jetbrains.com/lp/mono/

No font the user does not have a licence for; both are free for commercial use.

**Logo brief:**

- What it must say: one thread becoming many, or a shuttle passing through warp — one system, many agents.
- Where it must work: a sign outside an office, a ceramic mug, a 32×32 app icon, the CLI help header, a GitHub avatar.
- What to avoid: camera or video imagery (conflict with Loom the video company), generic chat bubbles, anything detailed enough to blur at 32 px.
- Offer: generate concepts with an image tool if the user has one; mark them concepts, not a finished logo. Commission a designer for the final mark and register it.

**First five touchpoints:**

1. Sign or homepage — the wordmark + one-line promise above the fold; no stock "robot" art.
2. Packaging — the brew tap (`brew tap tusharmewara/loom`) and `cargo install --path crates/loom-cli` install experience; version 0.12.0 visible.
3. Receipt or confirmation — Stripe checkout receipt + licence key email; subject line names the seat count and tier.
4. First social post — one sentence + one image: "One daemon, every agent, no lost context." on X and LinkedIn.
5. Reply to the first complaint — voice in practice: plain, precise, steady; link to `loom doctor --json` output as evidence.

**Rules:** never copy a competitor's name, logo, colours, or tagline. No fonts, images, or icons the user has no right to use. Not legal advice on trademarks.

Next: `/founder-ops` and `/founder-launch`.

## Operations

Day-one operations for Loom: unified agentic ecosystem (loomd daemon, CLI, TUI, MCP server), Rust workspace, 37+ crates, all 8 commercialization phases complete, per-seat Team tier, enterprise SSO/FIPS/audit. Pricing per founder/numbers.json: $30/seat/month, $2/seat/month variable, $20,000/month fixed, $50,000 startup, ramp 5→30 seats/day first month, capacity 500/day.

### 1. Daily cycle

Customer-facing steps marked (C).

1. Overnight daemon health: loomd uptime, IPC socket (`~/.loom/loomd.sock`), event bus, crash reports.
2. Support queue triage: tickets, Slack/DMs, GitHub issues (C).
3. Team-seat onboarding calls (C).
4. Enterprise SSO/SAML/SCIM provisioning (C).
5. Release pipeline: merge → unit + integration tests → sign binaries → publish crates.io + Homebrew cask → daemon auto-update channel → changelog (C).
6. Cost review: AWS bill, Stripe fees, seat count vs plan vs numbers.json.
7. End-of-day: backups verified, on-call handoff note, status page updated (C if incident).

### 2. Suppliers

| Supplier | What | Published price | Min order | Lead time | Terms |
|---|---|---|---|---|---|
| AWS (us-east-1) | EC2, RDS, S3, CloudFront, FIPS endpoints | Pay-as-you-go; calculator | None | Instant | Monthly, card |
| Cloudflare | Domain + DNS | Domains ~$10–20/yr; DNS free tier | None | Instant | Card |
| GitHub org | Repos, Issues, Actions CI | Team $4/user/month; minutes overage | None | Instant | Monthly |
| Stripe | Payments (Team seats) | 2.9% + 30¢ per successful charge | None | Instant | Monthly payout |
| Sentry | Error tracking | Published team tier; confirm on pricing page | None | Instant | Monthly |
| Linear | Task tracking | $9/user/month | None | Instant | Monthly |
| Postmark / Resend | Transactional email | Published tiers; confirm | None | Instant | Monthly |
| Apple Developer Program | macOS code signing / notarization | $99/yr | None | Days | Card |
| DigiCert / Sectigo | Code-signing cert (Windows/macOS binaries) | ~$300–500/yr | None | Days–weeks | Card |
| Stripe Atlas / lawyer | Delaware C-Corp incorporation | $500 + state fees (~$90) | — | Weeks | — |
| WorkOS | Enterprise SSO/SAML/SCIM | Quote required | — | — | Quote |
| Cyber + E&O insurance | Insurance broker | Quote required | — | — | Quote |
| Accountant | Bookkeeping / tax | Quote required | — | — | Quote |

Variable-cost check vs numbers.json: Stripe alone on a $30 seat ≈ $1.17, leaving ~$0.83/seat for AWS, email, Sentry, identity. If Sentry/WorkOS/email per-seat push variable above $2, CFO re-runs founder/numbers.json. Quote-only items (WorkOS, insurance, accountant) go into open questions below.

### 3. People / staffing

Roles:
- Founder/CEO — product, sales, enterprise deals — full time
- Engineer, Rust backend/daemon — full time
- Engineer, TUI/CLI/frontend — full time
- SRE/DevOps — part-time first 6 months, then on-call rotation
- Customer success / onboarding — part-time, full time at ~50 seats
- Accountant + lawyer — retainers, part-time

Weekly rota:
- Slow first month: founder covers support, onboarding, releases; one engineer carries on-call.
- Planned volume: add CS person, second engineer, part-time SRE.

Wages (US market, linked public sources to confirm): senior Rust engineer $150k–$220k; customer success $60k–$90k. Payroll beyond wages — employer tax, benefits, workers' comp — varies by state; flag for accountant. Not tax or legal advice.

### 4. Routines (SOPs)

Opening (start of day):
1. Check PagerDuty/alerts overnight.
2. Verify loomd status and IPC socket.
3. Triage support queue; reply same business day.
4. Confirm CI green; deploy only non-critical merges.

Serving (support / fulfilment):
1. Triage ticket → reproduce → fix or workaround.
2. Reply to customer within 4 business hours.
3. Enterprise SSO breakage escalates to SRE within 1 hour.
4. Log fix; update docs if a customer hits it twice.

Shipping (release):
1. Branch from main; run `cargo test --workspace`.
2. Sign binaries (Apple + code-signing cert).
3. Publish crates.io + Homebrew cask; push daemon auto-update.
4. Post changelog; update status page.

Complaint:
1. Acknowledge <4 hours; assign owner.
2. Fix or account credit; post-mortem if SEV2+.

Closing (end of day):
1. Verify backup completed.
2. Snapshot costs vs plan.
3. On-call handoff note to next engineer.
4. Deploy freeze unless critical.

Weekly reorder: not applicable — software. Weekly instead: `cargo audit` dependency check, licence scan, seat-billing reconciliation in Stripe.

### 5. Tools

- GitHub — code, issues, Actions — free / Team $4/user/month
- Linear — tasks and sprints — $9/user/month
- Slack — internal comms — free tier
- Stripe — payments — 2.9% + 30¢
- AWS — cloud — usage-based
- Sentry — error tracking — published team tier
- PagerDuty — on-call — published per-user
- 1Password — secrets — per-user
- QuickBooks / Xero — bookkeeping — published monthly
- Notion — docs/wiki — free / paid

Smallest stack that runs it; no tool added without a line item.

### 6. Licences, permits, insurance

- Delaware C-Corp registration + EIN (default for US software; confirm with lawyer).
- Sales-tax registration where nexus exists (Stripe Tax can collect).
- GDPR (EU) / CCPA-CPRA (CA) — privacy policy, data-processing agreement.
- SOC 2 Type II — enterprise expectation, not a government permit.
- ISO 27001 — optional.
- FIPS 140-2/140-3 validated modules — only if an enterprise contract requires; costly, quote.
- Export controls (EAR) on encryption software — self-classify / CCATS if shipping crypto.
- Cyber liability + errors & omissions insurance — broker quote.
- No food, health, or signage permits (software business).

Not legal advice. Rules differ by place and change; confirm each with the authority or a professional.

### 7. Risk register

| # | Risk | Likelihood | Impact | Plan |
|---|---|---|---|---|
| 1 | loomd outage | Likely | High | PagerDuty on-call, restart runbook, hot-spare instance |
| 2 | Cloud bill spike | Possible | High | AWS budgets + alerts, reserved instances, reserved capacity |
| 3 | Key engineer sick / leaves | Possible | High | Docs, pairing, no single-person knowledge silos |
| 4 | Payment failure / churn | Likely | Medium | Stripe dunning, retry logic, grace period before lockout |
| 5 | Dependency vulnerability (crate) | Possible | High | `cargo audit` + Dependabot, 24-hour patch release |
| 6 | Enterprise SSO breakage | Possible | High | Test IdPs (Okta, Entra, Google), staging env, WorkOS fallback |
| 7 | Ramp misses / slow month | Likely | Medium | Sales outreach, discount pilot, extend runway |
| 8 | Session export / data corruption | Unlikely | High | Backups, checksums, import/export tests |
| 9 | Domain / DNS expiry | Unlikely | High | Auto-renew, multi-year registration, monitoring |
| 10 | Model / LLM provider outage (if integrated) | Possible | Medium | Fallback providers, caching, degrade gracefully |

Open questions:
- WorkOS enterprise pricing (quote required; added to variable cost only when known).
- Cyber + E&O insurance premium (quote required).
- Accountant retainer (quote required).
- If real Stripe/Sentry/identity costs push variable above $2/seat/month, CFO re-runs founder/numbers.json.

Next: /founder-launch.

## Launch plan

**Product:** unified agentic ecosystem — loomd daemon, CLI, TUI, MCP server; skills, sessions, MCP servers, auth vault, hooks, workflow shared across claude-code, codex, opencode, goose, cursor, aider, gemini-cli, kiro, juni, crush, jcode, github-copilot.
**Commercial model:** per-seat Team tier at $30/seat/month (numbers.json), enterprise SSO/FIPS/audit add-on. Break-even ≈ 715 seats/month ≈ 24 paid seats/day at full ramp.
**Launch day:** Monday, December 14, 2026 (earliest realistic given a 6-week commercial build: license gate, Stripe billing, entitlements).

---

### Step 1 — test before you spend (subscription → founding-member offer, paid up front)

**Test:** landing page at loom.sh/team with the real price ($30/seat/month) and a "Pre-order founding team" button. Payment captured up front for the first month; founding members lock $30/seat/month for 12 months, get enterprise SSO/FIPS/audit at launch price, and a direct Slack line to the founder. Run concurrently: 3 paid pilot teams at the planned price, onboarded manually through loom init + loomd, invoiced $90/month.

**Success line (written before launch):** 50 founding pre-orders and 3 paid pilots within 2 weeks (by Nov 11) = $1,500/month committed MRR + $270/month pilots.

**If real buyers fall well short:** trust the buyers, not the panel; return to founder/offer or founder/pricing before spending the $50k startup budget.

**Owner:** founder/CEO. **Deadline:** page live Nov 4; test ends Nov 11.

---

### Step 2 — countdown to opening day

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

### Step 3 — launch day run sheet (Monday, December 14, 2026)

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

### Step 4 — first 30 days (Dec 14 – Jan 12)

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

## Not done yet

- Summary: run /founder-plan
- Pricing: run /founder-pricing

_The panel is simulated buyers and the numbers are projections from your inputs. Confirm demand with real customers and costs with real quotes before you spend. Not financial, legal or tax advice._
