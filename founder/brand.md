# Loom — brand

Positioning (from the product): the shared agentic ecosystem for AI coding agents. One daemon (loomd), one CLI, one TUI, one MCP server; 12 agent tools share skills, sessions, MCP servers, hooks, workflows, and credentials so a task started in any agent can be continued, reviewed, and finished in another without losing context. Audience: engineering teams running multiple AI coding tools. Per-seat Team tier ($30/seat/month); enterprise SSO/FIPS/audit. Version 0.12.0. Repo: github.com/tusharmewara/loom.

Competitors to stay clear of by name/look: Orca (mentioned in the repo's competitive analysis), and the video company Loom (loom.com, Atlassian) — never copy a competitor's name, logo, colours, or tagline.

## Name: Loom (already chosen)

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

## Voice and promise

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

## The look (as a brief)

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
