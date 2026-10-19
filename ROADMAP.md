# Loom Roadmap

*Decision document. Feature-level backlog lives in [docs/loom-improvements.md](docs/loom-improvements.md); competitive context in [docs/competitive-analysis.md](docs/competitive-analysis.md); scaling verdict in [docs/horizontal-scaling-investigation.md](docs/horizontal-scaling-investigation.md).*

## North Star

Loom is the **shared agentic ecosystem** — the interoperability fabric that makes AI coding tools (claude-code, opencode, goose, crush, jcode, …) composable instead of sileroed. Skills, MCP servers, sessions, auth, hooks, and workflows are written once and shared across every tool. Loom is **not** another agent or another orchestration UI; it is the programmable infrastructure layer those tools and UIs sit on.

## Where We Are (v0.9.3)

A 31-crate Rust workspace: daemon (`loomd`) + CLI + TUI, Unix-socket IPC, working ACP dispatch with tracked runs, encrypted vault with hash-chained audit, MCP registry with health checks, session export/branch/merge/search, git-worktree parallel execution, GitHub/Linear trackers, SSH remote capture, TOML generic adapters, honest CI (clippy `-D warnings`, 6-job matrix), Homebrew distribution with bottles.

## Pillars

Five pillars, deliberately sequenced: nobody adopts — let alone pays for — infrastructure that breaks. Pillars 1–3 are the product; pillar 4 is the business built on top; pillar 5 is the demand driver that makes the business worth paying for.

### 1. Harden the Core *(v0.9.x → v0.10)* — refine existing features

Fix what exists before building what's next. Evidence: v0.9.3 shipped a five-handler panic class that CI had been greenwashing for months; daemon lifecycle still produces socket races and duplicate daemons.

- **Daemon lifecycle done right.** Single-instance lock (not socket-file-existence checks), stale-socket recovery in `daemon start`, and `loom daemon install/uninstall` that owns its launchd/systemd unit — no more competing agents keeping duplicate daemons alive. CLI↔daemon version handshake over IPC; warn hard on skew.
- **IPC protocol hardening.** Versioned request envelope, request timeouts, panic isolation as a rule (the v0.9.3 scoped-thread pattern), connection-backoff guidance for CLI.
- **Finish-or-delete sweep.** Every placeholder or unwired feature gets shipped or removed: `EmbeddedPane` dispatch, the `snapshot_builder.rs` orphan, anything still returning "not yet wired" (VISION.md gap list). Dead code is a lie the roadmap stops telling.
- **Release automation.** One command: version bump → changelog → tag → CI → bottles dispatch. Today this is four manual steps, one of which (bottles) is easy to dispatch with the wrong `version` input.
- **Docs coherence.** README/STATUS/VISION drift (subcommand counts, clone URLs, stale STATUS roadmap stub) reconciled into one truth; STATUS.md roadmap section points here.
- **Test depth.** Daemon-lifecycle integration tests (restart races, socket contention, version skew). Continue burning the `docs/loom-improvements.md` backlog in priority order — that doc stays the feature inventory; this file stays the direction.

### 2. Security & Trust *(v0.10 → v0.11)*

Agents execute arbitrary code with your credentials. Trust is the adoption gate — and the enterprise sales gate.

- **Sandboxed execution.** `loom-runtime` container isolation becomes a first-class, CI-tested path (not just feature-gated compile), with per-run network policy. Default documented; opt-in until proven.
- **Scoped credentials.** Vault already does ChaCha20Poly1305 + ACL + hash-chained audit. Next: per-run, per-tool scoped credentials — an agent gets a derived, revocable grant, never the master key. Key rotation.
- **Supply chain.** Plugin Ed25519 verification moves from opt-in (`--verify`) to the default path; key-continuity pinning for registry installs.
- **Compliance posture.** Audit chain export + documented retention — the SOC2 groundwork pillar 4 depends on.

### 3. Ecosystem & Protocol *(v0.11 → v1.0)*

Adoption comes from being the easiest layer to plug into. The moat is the protocol, not the adapters.

- **Public protocol.** Versioned, documented spec for the adapter contract and ACP dispatch; publish the `acp_registry` schemas. Third-party agents register without forking.
- **SDK story.** Grow `loom-ffi` beyond minimal; a thin binding story for registering tools, skills, and handlers from other languages.
- **Adapter coverage.** More presets; `GenericAdapter` TOML stays the escape hatch; a contribution guide so coverage scales without us.
- **Consumption surfaces.** `loom-mcp-server` as the bridge for external orchestrators/UIs to use Loom services — the "UIs consume Loom" play from the competitive analysis.

### 4. Commercialization *(post-1.0, gated on 1–3)*

Market proof exists: Conductor at $50–60/mo/user sells orchestration with cloud execution and less infrastructure depth; Conifer sells the same thesis from the other end — a hosted gateway (drop-in provider-key replacement) monetized by Parallax review episodes. Both validate Loom's defensible wedge: **team-grade shared infrastructure** — the vault, registries, and audit — not another UI.

- **Licensing.** Core stays MIT (it is the adoption funnel). Commercial surface = hosted services + team features under a separate license; no re-licensing the core. *Decision required — see Open Questions.*
- **API key management as a product.** The team vault broker: daemon proxies and scopes every credential agents use, with full audit. loom-auth is already the strongest asset here; productize it.
- **Deployment tiers.** Documented headless daemon for servers/containers; `loom-remote` as the managed-remote story; a hosted control plane (registry sync, relay, fleet status) — **not** hosted scale-out: the scaling investigation's verdict is local-first by design.
- **Team tier.** Shared skill/MCP/template registries with org sync, RBAC on the vault, org-wide audit rollup, usage reporting.
- **Usage metering.** Per-run, per-tool, per-token accounting from the routing/telemetry crates — the honesty layer for both cost-conscious users and future billing. The verified review engine (pillar 5) is its flagship consumer: every review is a metered multi-seat workload.

### 5. Verified Review Engine *(post-1.0, gated on 2; the pillar-4 demand driver)*

When a session ends, agents that did not write the change re-do the important part blind; a sandbox labels every finding with what it established (`proven` / `suite_green` / `unverified` / `could_not_run` / `refuted`); one fix PR carries only verified patches; a lesson is written from each proven finding and injected into the next session touching that file; a scoreboard tracks which agent is right on which kind of task, at what cost. Conifer Parallax proved this loop is the thing people pay a gateway for. Loom already owns every stage's substrate — capture (`loom-sessions`), seating and outcomes (`loom-routing`), blind dispatch (`loom acp run`), worktree isolation (`loom-parallel`), test execution (`loom-runtime`), PR creation (`loom-trackers`), lessons (`loom-knowledge` + `loom-hindsight`). The two missing pieces are the review orchestration itself (a new `loom-review` crate) and the CI-tested container sandbox, which is pillar 2. Reviewers are existing registered agents dispatched over ACP — Loom does not build a reviewer model. Full build spec, MVP→V2 sequencing, and commercial framing: [docs/plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](docs/plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md).

## Non-Goals

- **Horizontal daemon scaling.** Investigated; local-first is the design. Don't revisit without new evidence.
- **Electron/GUI client.** The TUI is the reference frontend; UIs consume Loom via the protocol/MCP instead.
- **Building another coding agent.** Loom orchestrates tools; it does not become one.
- **Chasing orchestration-UI competitors feature-for-feature.** Consume their good ideas; stay the infrastructure layer.

## Open Questions

Decisions that change the plan, in order of when they bite:

1. **License for commercial features** — separate closed repo, BSL for team-tier crates, or pure hosted-service (SaaS) model with everything else MIT?
2. **1.0 target** — what date/feature set earns the protocol-stability promise? (Proposal: pillars 1–3 complete = 1.0.)
3. **Cloud posture** — self-hosted-first (customer runs daemon, we sell control plane) vs SaaS-first (we host). Self-host-first fits local-first and the security story.
4. **Pricing anchor** — per-seat team pricing vs per-daemon/host. Conductor anchors per-user at $50–60/mo; Loom's infra depth may support per-host with unlimited seats.

## Changelog

- 2026-10-19 — Conifer Parallax comparison → Pillar 5 added (verified review engine, the pillar-4 demand driver); build spec in docs/plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md; competitive-analysis.md gained §11 (Conifer Parallax) and pillar-4 market proof gained the Conifer anchor.
- 2026-10-10 — initial roadmap drafted after v0.9.3 release; pillars validated against VISION.md, competitive analysis, and the 0.9.3 hardening experience.
