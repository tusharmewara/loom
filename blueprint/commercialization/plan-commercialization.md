# Plan — Loom Commercialization (Pillar 4)

*Source docs: `ROADMAP.md` §Pillar 4 + Open Questions, `docs/loom-improvements.md`, `docs/competitive-analysis.md`, `docs/plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md`. Blueprint Q&A synthesized into this plan.*

## Overview

- **Open-core, single codebase.** Core stays MIT and fully free for individuals and small teams. All NEW team surfaces are gated behind a license; **zero existing behavior changes** for current users.
- **License model:** signed offline license key (Ed25519 claims: tier, soft seat count, expiry) verified locally by `loomd` against a built-in public key — reuses the crypto patterns already in `loom-core::plugin_registry`, `loom-hooks::signing`, and `loom-auth`'s audit export. No phone-home. **Revocation is approximate by design: licenses dye with expiry (≤1 yr) — a lapsed subscription expires at the stored license's `expires_at`, and revocation-before-expiry ships a new shorter-dated key at renewal.** True online revocation is explicitly out of scope (it requires the phone-home this model rejects).
- **Free tier: 5 humans** (GitLab/Bitbucket-precedent generous marketing number), unlimited tool connections per human. Paid seats start at human #6. Seat = human (org identity managed on the control plane); the license's seat count is a purchasing term. **Enforcement root invariant: loomd's embedded Ed25519 license public key is the ONLY non-self-hostable check** — a self-hosted CP is under the customer's control and enforces nothing against the vendor. Seat caps and roster rules bind only on the hosted CP (convenience + abuse-prevention); the binding guarantee to the vendor is license features + expiry verified locally. No seat count is enforced at runtime in loomd.
- **Control plane = one unified Rust service**, MIT like the core, assembled from existing crates (`loom-sessions` sync over HTTPS, `loom-auth` grants, `loom-telemetry` rollup). SaaS = convenience + trust; enterprise additionally gets on-prem CP hardening. loomd stores only license + org membership — no user accounts in the daemon; local-first is preserved.
- **Pricing direction:** per-human seat for Team (final number open; Conductor $50–60/mo is the ceiling — Loom is infra, priced below). Stripe subscriptions integrated with license issuance/revocation. 30-day self-serve trial licenses via Stripe checkout.
- **Review engine stays free locally** (demand driver); paid analytics = team scoreboard, cost dashboards, agent calibration on the control plane — fed by **aggregate-only, opt-in** pushes (same redaction philosophy as `loom auth export`: per-run data never leaves the machine by default).
- **Ship order (license-first ladder):** preflight → license gate → control plane MVP → vault RBAC → registry org sync → Stripe → analytics → enterprise. Each phase shippable; no calendar dates. (The CP lands before the team slices that consume its artifacts — roster and registry — so no phase depends on something not yet built.)

## Expected behavior

- `loom license activate <key>` / `loom license status` — local, offline; store file at `{loom_dir}/license.json` (`0600`). `status` reports tier, seat count, expiry, verified/invalid.
- Without a license: every current command behaves as today; team surfaces return a clear "requires a Loom Team license — `loom license activate`" message (single shared wording).
- With a Team license:
  - **Vault RBAC v1:** `loom auth` gains admin/member roles — admins list/inspect all grants and the audit log; members see only their own providers/grants. Vault unlock and dispatch behavior is otherwise unchanged. Roster source: the downloaded org membership; **offline interim (before a CP connection exists): a local `[org] members` config list** — the CP download replaces it once one is configured.
  - **Registry org sync:** skills/MCP/templates sync from the org's registry on the control plane, riding the existing config `extends`/remote-config machinery. Conflict order: **local registry wins on divergence; org adds/updates flow through; org deletions are advisories** — a deleted-by-org item is locally marked `retired-by-org` (hidden from sync output, not deleted) instead of silently vanishing from an offline customer's machine; divergence becomes visible state, not a hole.
  - **Control plane connection:** `loom org connect <cp-url>`; loomd registers its Ed25519 machine key with the org. **Crypto substrate reused (Ed25519 verify/keygen, challenge-response shape of `acp_handshake`); enrollment design is new** — persistent machine keys, rotation, and a CP-side revocation list are a different trust lifetime than ACP's ephemeral handshakes and get their own spec before phase 3. Downloads license + org membership; pushes opt-in aggregates.
  - **Analytics**: `loom org scorecard|cost|calibration` (and a CP web dashboard) render aggregated team data — per agent-tool × task-type success + cost, spend rollups from the usage ledger, calibration from review verdicts. Only aggregates the org chose to push are visible.
- Free orgs: CP registration works (machine key + roster), but the dashboard caps roster at 5 humans and hides analytics beyond the org's own local view (`loom telemetry usage` stays fully free and local).
- Usage ledger (`loom telemetry usage`) and the review engine (`loom review run|fix-pr`) remain free in full, including multi-seat runs.
- Billing: buying via Stripe issues a signed license immediately (email + dashboard); subscription lapse flips stored license claims past expiry — locally verified, no brick at once: **bounded read-only grace — gated surfaces stay readable for 30 days after `expires_at`, then refuse** (GitLab precedent: license expiry → instance read-only for up to 30 days; JetBrains' shorter ~48h grace applies to server-polling floating licenses, not offline-verified keys, so 30 days is the matching convention for our no-phone-home model).

## Implementation plan

### License subsystem (core gate — everything else keys off this)

- `crates/loom-core/src/license.rs` — `License { org_id, tier, seats, expires_at, features, sig }`; `verify(key, public_key) -> Result<License>`; claims as a compact JWT-like signed payload, Ed25519 (same crate pattern as `plugin_registry`); public key embedded at build time plus `{loom_dir}/license.json` store.
- `crates/loom-core/src/constants.rs` — `LICENSE_VERIFY_PUBLIC_KEY`, `LICENSE_STORE_FILE = "license.json"`.
- `crates/loom-cli/src/commands/license.rs` — subcommands `activate`, `status`, `deactivate`; wired in `cli.rs`.
- Feature gate helper: `loom_core::License::unlocks(TeamFeature)` in loomd handlers (daemon-side UTC-time check; expired → bounded 30-day read-only grace, per Expected behavior; past grace → refuse).
- Health/diagnostics: `loom doctor` gains a License row; status banner in TUI when gated.

### Vault broker (team tier, slice 1)

- `crates/loom-auth/` — extend the existing ACL (`vault.rs` / ACL model) with member/admin **roles per org member**; membership source: downloaded org roster, or the local `[org] members` config as the offline interim (see Expected behavior).
- Grant listing/audit read paths gain role checks (`auth list/show/grant list`); per-member scoping for members.
- v2 (follow-up phase): per-provider/per-tool ACL matrix (`matrix.rs`) — org-level restriction "which tools may member X use", enforced at provider lookup; enterprise later: approval flows on grant issue.

### Registry org sync (team tier, slice 2)

- `crates/loom-skills/` + `crates/loom-mcp/` + templates: `sync_from_org(client, org)` pulling from CP `/api/v1/org/{id}/registry`; reconciles through the existing registry upsert paths; offline → local registry wins, resync on next connect.
- Rides the existing 5-layer config merge; the CP registry is just another (highest, org-scoped) overlay source.

### Control plane (unified Rust service)

- New crate `crates/loom-control-plane/` (axum + SQLite storage — v1 decision: one binary, small aggregate data, simple file backup; Postgres moves to the enterprise phase if scale demands), deployable as one binary; MIT.
- Endpoints (JSON, versioned `/api/v1/`):
  - `POST /api/v1/orgs`, roster CRUD (dashboard = server-rendered HTML + JSON)
  - `POST /api/v1/machine/enroll` — Ed25519 machine-key registration (same verify/keygen primitives as `loom-sessions::acp_handshake`; enrollment design itself is new, see Client↔CP plumbing)
  - `GET /api/v1/org/{id}/membership`, `/license` — what loomd downloads
  - `POST /api/v1/org/{id}/usage` — **aggregate-only** ingest (totals, per-tool, verdict counts; schema rejects per-run payloads)
  - Analytics reads: `/api/v1/org/{id}/scoreboard|cost|calibration`
- Roster seat enforcement lives here (free orgs cap roster at 5 humans).
- Data model: `Org`, `Human`, `MachineKey`, `AggregateRecord` (from `loom-telemetry::usage` totals shapes), `ReviewVerdictAggregate`.

### Client↔CP plumbing in core

- Enrollment design (machine-key persistence, rotation, CP-side revocation list) gets its own spec section inside this crate's work; `loom-sessions::acp_handshake` is crypto substrate only, not a design to inherit.
- `crates/loom-cli/src/commands/org.rs` — `loom org connect|status|scorecard|cost|calibration|push-usage` (push is `--enable` aggregate-only, opt-in config `[org] share_usage_aggregates = true`).
- `crates/loom-core/src/org.rs` — `OrgClient` over HTTPS with the machine key; membership/license download and caching into `{loom_dir}/org/`.

### Stripe + issuance

- Control-plane module `billing.rs` (Stripe checkout + customer portal + webhooks), `license_issuance.rs` mints signed license keys (the CP holds the Ed25519 *private* key; loomd holds only the public key).
- Trial: 30-day Team license issued at checkout start; on expiry, the same bounded 30-day read-only grace applies.

### Enterprise (later phase, designed not built in MVP phases)

- SSO/Okta roster federation: CP consumes IdP directory as roster source.
- On-prem CP hardening: audit-export chain verification of CP itself, config for self-hosted CP.
- Support SLA definitions (contractual, no code).

### Docs/distribution

- `ROADMAP.md` pillar 4 updated to point at this plan; `README.md` pricing/tier section; LICENSE unchanged (MIT core + CP).

## Implementation phases

*Every phase enters the repo's own spec-kit loop (`loom workflow create --spec`): the ComplexityScore gate routes design-heavy phases to Simulate, mechanical ones straight to Plan; each triaged expectations below. Phase 0 is three independent features — each its own spec task. For phases 2, 3, 5 and 7 a spec + simulated review earns its cost (Stage-1 trials: 4/5 blocking catches); Phases 4, 6, 8 will gate below Mid and go straight to Plan. Phases 2 (license), 3 (CP + enrollment) and 7 (analytics) additionally each get a dedicated design spec + implementation-plan cycle before coding.*

1. **Preflight (pillars 1–3 → 1.0):** SDK story (`loom-ffi` bindings for registering tools/skills/handlers from other languages), container egress proxy (make `restricted` an actual allowlist, not fail-closed `none`), adapter contribution guide. Exit: 1.0 tagged; pillar 4 gate satisfied honestly. **Each prerequisite has its own spec-kit-ready implementation plan in `blueprint/commercialization/preflight/`** — `PR1-sdk-story.md`, `PR2-egress-proxy.md`, `PR3-adapter-guide.md`, `PR4-decisions-gate.md` (license-mint decision + the readiness checklist that unlocks phases 2–8). Dispatch each with `loom workflow create --spec` per its header; preflight is complete only when PR4's checklist is fully checked and dated.
2. **License gate:** `loom-core/src/license.rs`, `loom license` CLI, doctor row, gate helper + single shared "requires Team" message. Exit: a signed key activates and `unlocks()` is testable across the workspace.
3. **Control plane MVP + enrollment:** org/roster/machine-key enrollment (new design, built on the `acp_handshake` crypto substrate) + server-rendered dashboard + roster seat cap; `loom org connect` in loomd. Exit: a real org with 2 hosts registered end-to-end; the 6th-human roster addition is rejected on a free org.
4. **Vault broker v1:** admin/member roles + scoped grant/audit reads in `loom-auth` (roster from the CP download, `[org] members` interim offline). Exit: member cannot read admin's grants; audit proof.
5. **Registry org sync:** org registry pull-through into skills/MCP/templates with the advisory-deletion conflict rule. Exit: two machines in one org converge on the same registry; an org deletion marks `retired-by-org` locally rather than silently deleting.
6. **Stripe billing:** checkout, webhooks, license issuance + trial. Exit: buy a seat card in test mode → working license key activates loomd. **Trial anti-abuse (per-org one-trial rule) resolved and enforced before this phase ships** — it gates the exit, it is not an open question left open at ship time.
7. **Analytics:** scorecard/cost/calibration ingest + views, `loom org` read commands. Exit: aggregates computed from pushed-only data; privacy test proves no per-run rows exist server-side.
8. **Enterprise:** SSO federation, on-prem hardening, SLA docs.

## Testing strategy

- **License:** unit tests in `loom-core` — valid key activates; tampered payload, forged signature, wrong-tier fail with distinct errors; expired → grace state (read-only) for 30 days, then refusal; seat/claims parsing round-trips; embedded-key pinning test (a key signed by a different private key never verifies).
- **Gate behavior:** integration test — every gated entry returns the shared Team message without a license and works with a test-signed key (test keypair injected by cargo feature `license-test` or dev fixture).
- **RBAC:** unit tests in `loom-auth` (tempfile loom dir everywhere, per repo convention): member scopes, admin powers, cross-member denial.
- **Registry sync:** crash/offline resilience — partial sync leaves local registry valid; idempotency; conflict order vs local edits.
- **Control plane:** integration tests spawn the CP binary on a random port (mirroring `crates/loomd/tests/daemon_lifecycle.rs` style): enroll handshakes reuse the crypto-shape test patterns from `acp_handshake` (keygen/verify fixtures), not the enrollment design; roster cap (6th human rejected on a free org); aggregate ingest **rejects per-run-shaped payloads** (privacy assertion, not just absence).
- **Client plumbing:** machine-key rotation, membership cache staleness, CP unreachable → offline behaviour unchanged (no hard dependency).
- **Stripe:** contract tests against recorded webhook fixtures; issuance tombstones (same license id never re-mints after revocation).
- **CI:** new crates join the existing workspace test matrix; CP gets an integration job (its own SQLite migration path); pagination/limits smoke-tested against test data.

## Open questions

- **Final seat price** and any per-host alternative packaging for infra-heavy buyers (per-human anchor chosen; market anchor studied: hybrid is dominant — seats now, meter later).
- **License private key custody:** CP-only vs a simple offline mint CLI (`loom-admin licensemint`) that lets the maintainer issue keys manually before the CP exists.
- **Public key rotation path:** how loom ships a rolling list of accepted license public keys (one key today; rotation story needed before many customers exist).
- **Control plane persistence:** ~~SQLite vs Postgres~~ — decided: **SQLite for v1** (single binary, aggregates are small, file backup); Postgres revisited in the enterprise phase if hosted-CP scale demands it.
- **Whether gated code sits in the same crates directory** (`crates/loom-auth` et al.) with runtime `unlocks()` checks only — assumed yes; no separate repository.
- **Analytics dashboard UX long-term:** server-rendered HTML on the CP vs Flutter/web SPA (MVP: server-rendered).
- **Trial conversion memory:** the per-org one-trial anti-abuse rule is decided and enforced as a **phase-6 exit requirement** (not deferred): either the CP tracks per-license-id issued trials, or trials are unmodified self-serve — pick before phase 6 ships.
- **Roster enforcement edge:** an org that deletes the 5th human down to 4 mid-license — settled by "seats are a purchasing term" (hosted-CP-side cap only; loomd never seats).
- **Usage ledger retention for teams:** does CP-side aggregate retention get its own `[security]-style` knob set?
