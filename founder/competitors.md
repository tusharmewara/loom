# Loom — Competitor Map

Source of truth for Loom itself: this repo (`github.com/tusharmewara/loom`, current release `0.12.0`, 2026-10-28). Loom is a Rust daemon (`loomd`) + CLI + TUI + MCP server that lets multiple AI coding agents (claude-code, codex, opencode, goose, aider, cursor, gemini-cli, kiro, juno, crush, jcode, github-copilot) share skills, sessions, MCP servers, hooks, workflows, and credentials. Commercialization phases 1-8 complete: license gate, control plane, vault RBAC, org registry, Stripe billing, analytics, enterprise SSO/FIPS/audit. Pricing: per-seat, Team tier, Conductor ceiling $50-60/mo; `founder/numbers.json` lists unit seat/month at $30.

Sources: public pricing pages, docs, GitHub, Reddit, Trustpilot, Gartner Peer Insights. Public sources only; no prices or ratings invented. If a source could not be reached it is noted.

## 1. Competitor table (sorted by how directly each competes)

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

## 2. What their customers complain about (public reviews)

Ranked by recurrence across the sources read. Quotes are verbatim and short, with links.

### Cursor — privacy, cost, performance, deskilling
- **Code privacy / training**: users worry their code is sent to and stored by model providers; Cursor offers a Privacy Mode, but it's a recurring trust question. [reddit.com/r/cursor — "Company doesn't want me to use Cursor"](https://www.reddit.com/r/cursor/comments/1r7hzs8/company_doesnt_want_me_to_use_cursor/)
- **Cost / rate limits**: paid tiers still hit usage limits; Pro at $20/mo can feel restrictive for heavy agent use. [reddit.com/r/cursor — "For those using Cursor right now"](https://www.reddit.com/r/cursor/comments/1vi79ib/for_those_using_cursor_right_now_is_it_actually/)
- **Deskilling / correctness**: "It doesn't give 100% correct code in first attempt." [reddit.com/r/cscareerquestions — "Cursor is making me dumb"](https://www.reddit.com/r/cscareerquestions/comments/1na5bfw/cursor_is_making_me_dumb/)
- Theme is **thin-to-moderate**: most complaints are scattered Reddit threads, not a concentrated review-site pattern.

### Windsurf — credit burn on error loops, rate limits, reliability
- **Credits burned by stuck/error loops**: "When it gets stuck in a loop of errors... every prompt sent uses credits." / "burned over 50 credits trying to continue with requests it can't resolve." [reddit.com/r/windsurf — "Windsurf is misbehaving since 2 days"](https://www.reddit.com/r/windsurf/comments/1m65srg/windsurf_is_misbehaving_since_2_days/) · [reddit.com/r/windsurf — "error loop burning credits"](https://www.reddit.com/r/windsurf/comments/1rupgim/windsurf_ide_error_loop_burning_credits_already/)
- **Rate limits / outages**: "Permission denied: Reached overall message rate limit." / unable to use for 8+ hours. [reddit.com/r/windsurf — "unable to use Windsurf for the past 8+ hours"](https://www.reddit.com/r/windsurf/comments/1tb5wty/anyone_else_unable_to_use_windsurf_for_the_past_8/)
- **Model quality / compatibility**: free models "clunky"; extension compatibility issues vs Cursor. [reddit.com/r/windsurf — "Windsurf has become unusable"](https://www.reddit.com/r/windsurf/comments/1wiv9v6/i_think_windsurf_has_become_unusable/) · [aitoolrater.com review](https://aitoolrater.com/blog/windsurf-review-2026/)

### GitHub Copilot — weak PR review, suggestion loops, cost
- **PR review quality**: "feedback is very little, misses..."; review comments turn into an unhelpful loop ("every time I fix one suggestion, it just comes back with another set"). [reddit.com/r/ExperiencedDevs](https://www.reddit.com/r/ExperiencedDevs/comments/1ly13n0/why_does_github_copilot_pull_request_reviews_give/) · [reddit.com/r/webdev](https://www.reddit.com/r/webdev/comments/1skfg0k/has_anyone_else_found_copilot_review_to_be_kind/)
- **Cost creep**: "become much more expensive and limited." [reddit.com/r/webdev](https://www.reddit.com/r/webdev/comments/1u0i7rz/github_copilot_seems_to_have_become_much_more/)

### Devin — reliability on brownfield / production, needs expert review
- **Not trustworthy unsupervised on complex production issues**: "Devin can assist with debugging but should not be trusted to fully diagnose complex production issues without expert review." Harder on brownfield than controlled tasks. [idlen.io — "Devin AI Engineer: Review, Testing & Limitations in 2026"](https://www.idlen.io/blog/devin-ai-engineer-review-limits-2026/) · [reddit.com/r/ChatGPTCoding — "Where did Devin go?"](https://www.reddit.com/r/ChatGPTCoding/comments/1qp82lr/where_did_devin_go_what_does_it_say_about_the/)

### LangSmith — pricing confusion, trace cost, vendor lock-in
- **Pricing confusion / trace retention cost**: users confused by "10k free traces" vs retention and overage ($2.50-5.00 per 1k beyond allowance). [reddit.com/r/LangChain — "LangSmith pricing confusion"](https://www.reddit.com/r/LangChain/comments/1pz0azk/langsmith_pricing_confusion_are_10k_free_traces/) · [inference.net LangSmith pricing](https://inference.net/content/langsmith-pricing/)
- **Vendor lock-in / churning UI**: "tweak UI every two days"; open-source tracing alternatives sought "to avoid vendor lock-in." [reddit.com/r/LangChain — "Is Langsmith just good piece of trash?"](https://www.reddit.com/r/LangChain/comments/1h84qim/is_langsmith_just_good_piece_of_trash/) · [reddit.com/r/LangChain — open-source alternatives](https://www.reddit.com/r/LangChain/comments/1mls6cj/any_open-source_alternatives_to_langsmith_for/)
- Theme is **moderate**: concentrated in r/LangChain.

## 3. Positioning map (text)

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

## 4. The gap

Nobody sells a **shared, agent-agnostic agentic ecosystem** at a per-seat price. Every competitor is either:

- a **single-tool assistant** (Cursor, Windsurf, Copilot, Aider, OpenCode, Goose, Continue, Codex CLI) — you're locked into that tool's UI and context doesn't travel to another agent, OR
- an **orchestration/tracing platform** (LangSmith, CrewAI) — you orchestrate agents but don't get shared sessions, skills, MCP servers, credentials, and vault RBAC out of the box, OR
- an **enterprise-only** product (Cody ~$16K/yr, Augment Business) — priced out of small/medium teams.

The gap: **one seat, one price, any agent, shared context**. A team using Claude Code today, Codex tomorrow, and Cursor the next can keep the same skills, the same sessions (resumable across tools), the same MCP servers, the same vault credentials, and one org-level billing seat. Loom's daemon + 12 preset adapters + generic TOML adapter + control plane (SSO, FIPS, audit, Stripe) is exactly this. Evidence for the gap:
- Customers complain single-tool assistants trap context and burn credits on loops (Cursor/Windsurf) — a shared session that resumes in another agent avoids re-work.
- LangSmith/CrewAI customers want orchestration without vendor lock-in and without paying per trace/seat — Loom is self-hosted-ish (local daemon) with a control plane option, not per-trace metering.
- Cody/Devin/Augment push teams to $40-100+/seat or $16K/yr — Loom's $30/seat Team tier undercuts the team/unlimited tiers while offering more sharing, not less.

Loom can own: **"the shared layer between AI coding agents"** — the interoperability substrate that no single vendor (Cursor, GitHub, OpenAI, Anthropic) will build because it benefits their competitors.

## 5. The threat: who could copy Loom fastest

- **Cursor / GitHub / OpenAI / Anthropic** — they own the agent and the IDE; adding "share sessions/skills across other tools" contradicts lock-in, but they could add cross-tool resume or a shared MCP registry fastest (they already have the user base and the model). Cursor is closest: it's an IDE with an agent; adding a "session export to OpenCode" button is a small feature for them, a whole product for Loom.
- **LangChain / CrewAI** — they already do orchestration; adding shared sessions + skills + vault would move them up the stack into Loom's territory. They have more capital and more AI talent.
- **Microsoft (GitHub + VS Code)** — Copilot + VS Code + GitHub Codespaces + org billing is the strongest existing "ecosystem" analogue; they could bundle a shared-session layer essentially free.

Loom's defensibility is the **breadth of integration** (12 adapters + generic TOML adapter for any tool, local daemon, per-seat pricing, SSO/FIPS/audit control plane) and being **vendor-neutral** — it works with claude-code AND codex AND opencode AND cursor, which no single vendor will prioritize. The risk: a dominant IDE makes cross-agent sharing a free built-in feature, commoditizing Loom's core.
