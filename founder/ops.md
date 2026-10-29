# Loom — founder/ops

Day-one operations for Loom: unified agentic ecosystem (loomd daemon, CLI, TUI, MCP server), Rust workspace, 37+ crates, all 8 commercialization phases complete, per-seat Team tier, enterprise SSO/FIPS/audit. Pricing per founder/numbers.json: $30/seat/month, $2/seat/month variable, $20,000/month fixed, $50,000 startup, ramp 5→30 seats/day first month, capacity 500/day.

## 1. Daily cycle

Customer-facing steps marked (C).

1. Overnight daemon health: loomd uptime, IPC socket (`~/.loom/loomd.sock`), event bus, crash reports.
2. Support queue triage: tickets, Slack/DMs, GitHub issues (C).
3. Team-seat onboarding calls (C).
4. Enterprise SSO/SAML/SCIM provisioning (C).
5. Release pipeline: merge → unit + integration tests → sign binaries → publish crates.io + Homebrew cask → daemon auto-update channel → changelog (C).
6. Cost review: AWS bill, Stripe fees, seat count vs plan vs numbers.json.
7. End-of-day: backups verified, on-call handoff note, status page updated (C if incident).

## 2. Suppliers

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

## 3. People / staffing

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

## 4. Routines (SOPs)

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

## 5. Tools

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

## 6. Licences, permits, insurance

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

## 7. Risk register

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
