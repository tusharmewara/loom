# Loom Competitive Analysis & Evolution Roadmap

## Executive Summary

Loom is a **local-first, daemon-backed agent orchestration platform** with deep infrastructure: IPC, event bus, knowledge graph, skill/MCP/session/auth/hook/routing subsystems, and a TOML-driven generic adapter layer. The comparison tools fall into two categories:

- **Orchestration UIs** (Emdash, Conductor, Superset, Vibe Kanban, Nimbalyst, Vibe Console, Orca) — focus on multi-agent management, review flows, visual workspaces
- **Terminal-native minimalists** (Claude Squad, RunPane) — focus on lightweight tmux/pane control
- **Post-session review layers** (Conifer Parallax) — capture the session, other-lab models re-do it blind, sandbox-verified findings become a fix PR; monetized per episode on their gateway

Loom's differentiated position: **programmable infrastructure layer** (daemon + IPC + event bus + knowledge graph + routing) that UIs and CLIs both consume. The evolution path is hardening that infrastructure and adding the *control plane* features these tools demonstrate.

---

## Tool-by-Tool Comparison

### 1. Emdash (Free, OSS, 25+ agents, Desktop)
| Aspect | Emdash | Loom | Gap |
|--------|--------|------|-----|
| Agent breadth | 25+ via config | 12 preset adapters + GenericAdapter | Loom covers major ones; GenericAdapter fills rest |
| Automations | YAML rules engine | Hooks (script-based) | Emdash: declarative; Loom: imperative scripts |
| Remote SSH | Built-in | ✅ `loom remote exec/sync` + capture | **Shipped** (loom-remote, 0.8.0) |
| UI | Electron desktop | TUI (ratatui) | Different paradigm; Loom TUI is daemon-aware |
| Pricing | Free | Free (OSS) | Parity |

**Loom-aligned features:** Broad agent support (GenericAdapter already solves this), automation via hooks (Loom hooks are more powerful — exit codes, signing, ACL), remote execution (shipped: `loom remote list|add|remove|exec --capture-session|sync`).

### 2. Conductor (Melty Labs, Mac + Cloud, $50–60/mo)
| Aspect | Conductor | Loom | Gap |
|--------|-----------|------|-----|
| Review/merge flow | Polished Mac-native PR review | Workflow phases + dispatch + `loom issues create-pr/link-pr/diff/ci-status` | **Integration shipped** (0.8.0); native review UI still absent |
| Cloud execution | First-class | No | **Gap** — Loom is local-only |
| Platform | Mac + cloud | Cross-platform (Mac/Linux/Win) | Loom broader |
| Agents | 4 (Claude, Codex, Cursor, OpenCode) | 12 + GenericAdapter | Loom broader |

**Loom-aligned features:** Workflow orchestration (Loom workflow crate), multi-agent dispatch, PR integration via `loom issues` (create-branch, link-pr, create-pr, diff, ci-status). **Gap:** cloud execution backend, native PR review UI. Loom's routing + workflow could *become* the review engine.

### 3. Superset (OSS, 14.7k★, Desktop, 100+ parallel agents)
| Aspect | Superset | Loom | Gap |
|--------|----------|------|-----|
| Parallel scale | 100+ agents | Daemon + IPC (untested at scale) | Loom daemon is single-process; no horizontal scale |
| Phone control | Companion app | No | **Gap** |
| Agent support | 4 + any | 12 + GenericAdapter | Parity |
| Architecture | Electron + backend | Rust daemon + TUI/CLI | Loom lighter, faster |

**Loom-aligned features:** Multi-agent orchestration (workflow crate), any-agent support. **Gaps:** Horizontal scaling (Loom daemon is vertical), mobile control plane. Loom's event bus (ZeroMQ cross-process) *could* support distributed workers if extended.

### 4. Sculptor (Imbue, Free OSS, Mac/Linux, Container isolation)
| Aspect | Sculptor | Loom | Gap |
|--------|----------|------|-----|
| Isolation | Containerized (Docker/podman) | Process-level only | **Gap** — Loom has no sandboxing |
| Suggestions | Catches agent mistakes | Hooks (pre/post) + routing | Loom hooks can validate; no "suggestion" UI |
| Agents | Claude, Pi, any terminal | 12 + GenericAdapter | Parity |

**Loom-aligned features:** Safety via hooks (Loom hooks execute scripts with exit codes). **Gaps:** Container sandboxing, suggestion/review UI. Loom could add container backend for `loom exec` / workflow dispatch.

### 5. Vibe Kanban (BloopAI, Self-hosted Web, 10+ backends)
| Aspect | Vibe Kanban | Loom | Gap |
|--------|-------------|------|-----|
| Kanban board | Cards = worktrees + agents | ✅ TUI Kanban tab (phases as columns) | **Shipped** (0.8.0) |
| Worktree per card | Native git worktree mgmt | ✅ `loom exec --parallel` (per-task worktrees) | **Shipped** (0.8.0) |
| Self-hosted web | Yes | No (TUI/CLI only) | **Gap** |
| Backend | 10+ | 12 + GenericAdapter | Parity |

**Loom-aligned features:** Task orchestration (workflow crate), session persistence, Kanban tab (Plan → Implement → Verify → Finish columns), per-task worktrees. **Gaps:** web UI. Loom's workflow + sessions + knowledge graph could *power* a web Kanban backend.

### 6. Claude Squad (Free OSS, tmux, 4 agents)
| Aspect | Claude Squad | Loom | Gap |
|--------|--------------|------|-----|
| Terminal-native | tmux panes | TUI + CLI + daemon | Loom richer |
| Background/yolo mode | Built-in | Daemon + workflow dispatch | Loom equivalent via daemon |
| Agents | 4 | 12 + GenericAdapter | Loom broader |
| Simplicity | Extreme | Moderate | Loom more complex |

**Loom-aligned features:** Background execution (daemon), multi-agent. **Gap:** tmux-native pane management (Loom TUI is ratatui, not tmux). Loom's `loom exec` + daemon covers background execution.

### 7. RunPane / Pane (Free OSS AGPL-3.0, Cross-platform, Any CLI agent)
| Aspect | RunPane | Loom | Gap |
|--------|---------|------|-----|
| Terminal-first | Panes + remote control | TUI (ratatui) | Different: panes vs full TUI |
| Remote-pane from phone | Yes | No | **Gap** |
| Lightweight | Very | Moderate (daemon) | Loom heavier |
| Agent support | Any CLI | 12 + GenericAdapter | Parity |

**Loom-aligned features:** Any-agent via GenericAdapter, terminal-first. **Gaps:** Pane-based UI (vs TUI tabs), mobile remote control. Loom's IPC + event bus could expose pane-like control via FFI.

### 8. Nimbalyst (Free for individuals, MIT, Cross-platform + iOS, Visual workspace)
| Aspect | Nimbalyst | Loom | Gap |
|--------|-----------|------|-----|
| Visual workspace | Docs, mockups, task boards | Knowledge graph + workflow | Loom has data; no visual canvas |
| Multiplayer | Real-time collab | No | **Gap** |
| iOS app | Native | No | **Gap** |
| Agents | 3 via extensions | 12 + GenericAdapter | Loom broader |

**Loom-aligned features:** Knowledge graph (entities/relationships), task boards (workflow). **Gaps:** Visual canvas, real-time multiplayer, mobile app. Loom's knowledge graph + event bus *could* sync for collab.

### 9. Vibe Console (Paid, 14-day trial, macOS, 8 agents, Notch monitoring)
| Aspect | Vibe Console | Loom | Gap |
|--------|--------------|------|-----|
| Single-window Mac UX | Native SwiftUI | TUI (cross-platform) | Different paradigm |
| Usage-limit monitoring | Notch integration | Telemetry crate (histograms) | Loom has data; no notch UI |
| Agents | 8 CLIs | 12 + GenericAdapter | Loom broader |

**Loom-aligned features:** Usage tracking (telemetry crate), model routing. **Gap:** Native Mac app, notch/menu-bar integration. Loom telemetry could feed a Mac menu-bar helper.

### 10. Orca (stablyai, MIT OSS, Electron desktop + mobile companion, ships daily)
| Aspect | Orca | Loom | Gap |
|--------|------|------|-----|
| Agent breadth | Any CLI agent (30+ listed) | 12 adapters + GenericAdapter | Parity — both run any CLI |
| GUI | Electron desktop (macOS/Win/Linux) + iOS/Android companion | TUI (ratatui, 15 tabs) | **Gap** — desktop/mobile GUI is an explicit non-goal (ROADMAP) |
| Parallel worktrees | Native (tab split per worktree) | `loom exec --parallel` (per-task worktrees) | Parity |
| SSH remote | SSH worktrees, auto-reconnect, port forwarding | `loom remote exec/sync` + session capture | Parity (loom captures as indexed sessions; Orca is interactive) |
| Task tracking | GitHub + Linear workflows | `loom issues` (GitHub + Linear) | Parity |
| Diff review | Annotate AI Diffs (inline comments → agent) | TUI Review tab + `loom conflict resolve` | Different: GUI annotation vs TUI |
| Usage tracking | Account switcher, usage + rate-limit resets, hot-swap accounts | Telemetry + Cost tab + routing tiers | Parity on data; **Gap** — no multi-account hot-swap |
| Agent drives tool | `orca` CLI (`worktree create`, `snapshot`, `click`, `fill`) | ACP dispatch (`loom acp run`) + 36-cmd CLI | Parity |
| Infra depth | App-level ( Electron app is the product) | daemon + IPC + event bus + knowledge graph + skills + MCP pool + vault + hooks + routing | Loom is a programmable layer Orca-style apps could build on |

**Loom-aligned features:** any-CLI adapters, parallel worktrees, SSH remote, usage/cost tracking, GitHub/Linear integration, agent-drivable CLI, "own subscription" model (loom routes to your own providers via auth vault). **Gaps (deliberate):** desktop GUI, mobile companion, Design Mode/Computer Use, embedded editor/browser, diff-annotation UI — all GUI-app territory, all on the ROADMAP non-goals list.

**Benchmark value:** Orca validates three already-planned roadmap items: (1) multi-account credential switching (vault roles → account hot-swap), (2) usage/cost metering with rate-limit awareness (Cost tab exists; add reset tracking), (3) remote-control API → mobile companion (P1 item 7). Orca is the benchmark to watch, not the destination — it's an application; Loom is the substrate.

### 11. Conifer Parallax (hosted session-review layer, per-episode credits)
| Aspect | Parallax | Loom | Gap |
|--------|----------|------|-----|
| Post-session review | Other-lab models re-do the session blind; findings labelled by a sandbox | ❌ None — TUI Review tab is manual task reassignment | **Gap** — scoped as **Pillar 5** (ROADMAP.md); spec: [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) |
| Evidence labels | `proven` / `suite_green` / `unverified` / `could_not_run` / `refuted`; verification is evidence, not a gate | ❌ | Copy the label set and invariants verbatim |
| Sandbox verification | Throwaway repo copy; repro fails before fix, passes after | `loom-parallel` worktrees shipped; `loom-runtime` containers feature-gated | Partial — pillar 2 hardens the container path |
| Fix PR policy | One per source PR, verified patches only, never reopens/overwrites/self-merges | `loom issues create-pr` (manual) | Mechanics shipped; the policy is the new work |
| Lessons (memory) | Outcome-gated only (proven/merged/applied); 30-day expiry; dismissal suppresses forever | Knowledge graph + hindsight ingest broadly | Discipline to copy: outcome-gated, receipted lessons |
| Scoreboard | Per seat×lens calibration from your merges/closes | `loom routing outcomes` (tier-level) | Extend to per-agent×lens |
| Deployment | Hosted reviews; capture uploads every session | Local-first; sessions never leave the machine | Loom wedge: own keys via vault, no uploads |
| Pricing | Per-episode credits on their gateway | MIT core | Loom monetizes the substrate (pillar 4), not the loop |

**Loom-aligned features:** every stage maps to an existing crate (sessions, routing, ACP dispatch, parallel worktrees, runtime, trackers, knowledge, hindsight) — this is the one competitor whose product is assembled entirely from parts Loom already ships. **Gaps:** the review orchestration itself and the CI-tested sandbox (pillar 2). **Idea to steal:** the evidence-label semantics and fix-PR delivery invariants — they are the honesty rules that make a review worth paying for.

---

## Feature Matrix: Loom vs Competitors

| Feature Category | Loom Current | Emdash | Conductor | Superset | Sculptor | Vibe Kanban | Claude Squad | RunPane | Nimbalyst | Vibe Console | Orca |
|------------------|--------------|--------|-----------|----------|----------|-------------|--------------|---------|-----------|--------------|------|
| **Daemon/IPC** | ✅ Full | ❌ | ❌ | Backend | ❌ | Backend | ❌ | ❌ | ❌ | ❌ | ❌ (app-internal) |
| **Event Bus** | ✅ ZeroMQ + tokio | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Knowledge Graph** | ✅ SQLite + FTS5 | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ⚠️ Basic | ❌ | ❌ |
| **Skill System** | ✅ Hot-reload, SQLite | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **MCP Pool** | ✅ Registry + health | ⚠️ Config | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ (agents own theirs) |
| **Auth Vault** | ✅ Encrypted + ACL | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ⚠️ account switcher |
| **Hooks** | ✅ Scripts + exit codes | ✅ YAML | ❌ | ❌ | ✅ Suggestions | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Workflow/Phases** | ✅ Full orchestration | ⚠️ Automations | ✅ Review flow | ⚠️ Parallel | ❌ | ✅ Kanban | ❌ | ❌ | ✅ Boards | ❌ | ✅ task workflows |
| **Routing/Scoring** | ✅ Complexity + tiers | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Usage | ❌ |
| **Session Mgmt** | ✅ Compress + export + recall | ❌ | ❌ | ❌ | ❌ | ✅ Worktree | ❌ | ❌ | ❌ | ❌ | ⚠️ snapshots |
| **Generic Adapter** | ✅ TOML → 6 traits | ❌ | ❌ | ⚠️ Any agent | ❌ | ✅ Backends | ❌ | ✅ Any CLI | ✅ Extensions | ❌ | ✅ any CLI |
| **TUI** | ✅ 15 tabs, 1s refresh | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Panes | ❌ | ❌ | ❌ (GUI instead) |
| **CLI** | ✅ 36 subcommands | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ tmux | ✅ CLI | ❌ | ❌ | ✅ agent-facing CLI |
| **Remote SSH** | ✅ `loom remote` + capture | ✅ | ☁️ Cloud | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ SSH worktrees |
| **Cloud Execution** | ❌ | ❌ | ✅ | ❌ | ❌ | Self-hosted | ❌ | ❌ | ❌ | ❌ | ❌ (remote runtime = SSH) |
| **Mobile Control** | ❌ | ❌ | ❌ | ✅ Phone | ❌ | ❌ | ❌ | ✅ Phone | ✅ iOS | ❌ | ✅ companion |
| **PR Review UI** | ⚠️ TUI Review tab + issues integration | ❌ | ✅ Native | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ annotate diffs |
| **Kanban Board** | ✅ TUI Kanban tab | ❌ | ❌ | ❌ | ❌ | ✅ Cards=worktrees | ❌ | ❌ | ✅ Boards | ❌ | ❌ |
| **Container Sandbox** | ✅ loom-runtime (Docker/Podman/K8s) | ❌ | ❌ | ❌ | ✅ Docker | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Visual Canvas** | ⚠️ Graph tab (knowledge graph) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Docs/mockups | ❌ | ⚠️ Design Mode |
| **Multiplayer** | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Real-time | ❌ | ❌ |
| **Mac Native/Notch** | ❌ | ❌ | ✅ Mac | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ Notch | ❌ (Electron) |

---

## Features Along Loom's Evolutionary Path

These features *align with Loom's architecture* (daemon, IPC, event bus, knowledge graph, workflow, routing) and would strengthen its position:

### Tier 1: Core Infrastructure Hardening (P0 — mostly shipped)

| Feature | Why It's On Loom's Path | Status | Source Tools |
|---------|------------------------|--------|--------------|
| **Git worktree integration** | Workflow tasks → worktrees = natural isolation; sessions already track git state | ✅ **Shipped** — `loom exec --parallel` per-task worktrees (0.8.0) | Vibe Kanban, Sculptor |
| **Container sandbox backend** | `loom exec` / workflow dispatch → pluggable runtime (process → container → remote) | ✅ **Shipped** — loom-runtime: Docker/Podman/K8s Jobs (0.9.0) | Sculptor |
| **PR/forge integration** | Workflow phases + routing → assign PR review to agents; knowledge graph links code ↔ tasks | ✅ **Shipped** — `loom issues create-branch/link-pr/create-pr/diff/ci-status` (0.8.0) | Conductor |
| **Horizontal daemon scaling** | Event bus (ZeroMQ) already cross-process; add worker pool + leader election | ❌ Open | Superset |
| **Remote execution via SSH** | IPC + daemon → forward commands to remote loomd; GenericAdapter works anywhere | ✅ **Shipped** — `loom remote exec --capture-session` + `loom remote sync` (0.8.0) | Emdash, Conductor |

### Tier 2: Control Plane & UX (P1 — three of five shipped)

| Feature | Why It's On Loom's Path | Status | Source Tools |
|---------|------------------------|--------|--------------|
| **Kanban board (TUI tab)** | Workflow tasks + sessions → columns = phases; knowledge graph = card context | ✅ **Shipped** — TUI Kanban tab, phases as columns (0.8.0) | Vibe Kanban, Nimbalyst |
| **Visual canvas / graph view** | Knowledge graph (entities/relationships) → render as graph; sessions → nodes | ✅ **Shipped** — TUI Graph tab (0.8.0) | Nimbalyst |
| **Mobile/remote control API** | IPC + event bus → expose via HTTP/WebSocket; TUI already daemon-aware | ❌ Open — validated by Orca's mobile companion; also enables multi-account credential switching (vault roles) | Superset, RunPane, Nimbalyst, Orca |
| **Real-time multiplayer sync** | Event bus (ZeroMQ PUB/SUB) → broadcast session/workflow state; CRDT for offline | ❌ Open | Nimbalyst |
| **Usage/cost dashboard (TUI tab)** | Telemetry + routing catalog + session tokens → real-time cost per task/agent | ✅ **Shipped** — TUI Cost tab (0.8.0); rate-limit reset tracking still open (Orca does this) | Vibe Console, Conductor, Orca |

### Tier 3: Differentiation & Moat (P2 — two of five shipped)

| Feature | Why It's On Loom's Path | Status | Source Tools |
|---------|------------------------|--------|--------------|
| **Agent suggestion/review engine** | Hooks + routing + knowledge graph → pre-commit analysis, "suggestions" like Sculptor; post-session half now scoped as Pillar 5 verified review engine | ❌ Open (pre-dispatch half); Pillar 5 spec: [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md) | Sculptor, Conifer Parallax |
| **ACP-native agent registry** | `loom add-acp-agent` exists; expand to full ACP client + capability negotiation | ✅ **Shipped** — tracked ACP dispatch: `loom acp run <program> --prompt`, `loom acp agents`, `loom acp run-status` (0.8.0) | Emdash (ACP), Superset |
| **Declarative automation DSL** | Hooks are imperative; add YAML/TOML rules engine (Event → Condition → Action) | ❌ Open | Emdash |
| **Cross-session memory/recall** | Knowledge graph + session compression → semantic search across all history | ✅ **Shipped** — `loom sessions recall`, `loom memory retain/recall/reflect "…"`, Hindsight memory (0.6.0) | Nimbalyst, Vibe Kanban |
| **Plugin/WASM extension system** | GenericAdapter is TOML; WASM hooks/skills → sandboxed user extensions | ⚠️ Partial — plugin registry with SHA-256 + Ed25519 signatures exists; WASM runtime open | All (extensibility) |

---

## Prioritized Feature List with Details

### P0 — Core Infrastructure (all four shipped 0.8–0.9)

#### 1. Git Worktree Integration — ✅ shipped as `loom exec --parallel` (per-task worktrees under ~/.loom/worktrees/)
**Detail:** Each workflow task creates/uses a git worktree. `loom workflow create` → `git worktree add .loom/worktrees/<task-id>`. Session manifest stores worktree path. Dispatch runs in worktree. Cleanup on task completion.
**Architecture touchpoints:** `loom-workflow`, `loom-sessions`, `loom-cli` (dispatch), `loom-generic-adapter` (working_dir).
**Priority rationale:** Unlocks true parallel task isolation (like Vibe Kanban) without containers. Low risk, high leverage.

#### 2. Container Sandbox Backend — ✅ shipped as loom-runtime (Docker/Podman/K8s Jobs, 0.9.0)
**Detail:** Add `Runtime` trait: `ProcessRuntime` (current), `ContainerRuntime` (Docker/podman), `RemoteRuntime` (SSH). `loom exec` and workflow dispatch accept `--runtime`. Container runtime mounts workspace, injects auth/env, enforces resource limits.
**Architecture touchpoints:** `loom-generic-adapter` (launch config), `loom-workflow` (dispatch), new `loom-runtime` crate.
**Priority rationale:** Security/isolation (Sculptor parity) + enables cloud/remote execution path.

#### 3. PR/Forge Integration — ✅ shipped as `loom issues` (GitHub/Linear/Jira/GitLab: create-branch, link-pr, create-pr, diff, ci-status)
**Detail:** `loom workflow create --from-pr <url>` fetches PR, creates task, links knowledge entities (files, symbols). `loom workflow dispatch` posts review comments via API. Routing selects reviewer agent based on code ownership (knowledge graph).
**Architecture touchpoints:** `loom-workflow`, `loom-routing`, `loom-knowledge`, new `loom-forge` crate.
**Priority rationale:** Conductor's killer feature; Loom's routing + knowledge graph makes it *smarter*.

#### 4. Remote Execution via SSH — ✅ shipped as `loom remote` (exec --capture-session, sync, 0.8.0)
**Detail:** `loom daemon start --remote <host>` provisions remote loomd via SSH, establishes reverse tunnel for IPC. Local CLI connects to local daemon which forwards to remote. GenericAdapter works unchanged (tools run on remote).
**Architecture touchpoints:** `loomd`, `loom-ipc`, `loom-cli`, `loom-generic-adapter`.
**Priority rationale:** Emdash/Conductor feature; leverages existing IPC + daemon architecture.

---

### P1 — Control Plane (3 of 4 shipped)

#### 5. Kanban Board — ✅ shipped as TUI Kanban tab (phases as columns, 0.8.0)
**Detail:** Tab 7 (new) = Kanban. Columns = workflow phases (Backlog → Planning → Active → Review → Done). Cards = tasks. Drag-drop advances phase (calls `loom workflow advance`). Card detail shows session tail, knowledge entities, routing decision.
**Architecture touchpoints:** `loom-tui` (new tab), `loom-workflow`, `loom-sessions`, `loom-knowledge`.
**Priority rationale:** Vibe Kanban/Nimbalyst core UX; Loom already has phases + tasks + sessions.

#### 6. Knowledge Graph Visualizer — ✅ shipped as TUI Graph tab (0.8.0)
**Detail:** Tab 8 (new) = Graph. Renders entities/relationships from `graph.db` as navigable graph (crossterm + ratatui-canvas or text-based). Click entity → shows sessions, tasks, related entities. Filter by type, depth, session.
**Architecture touchpoints:** `loom-tui`, `loom-knowledge`, `loom-sessions`.
**Priority rationale:** Nimbalyst's visual workspace; Loom's knowledge graph is unique differentiator — make it visible.

#### 7. Remote Control API (HTTP/WebSocket) — ❌ open (next big unlock; Orca mobile companion validates it)
**Detail:** `loom daemon start --api-port 8080` exposes IPC commands + event stream via REST + WS. Auth via vault token. Enables mobile app, web dashboard, VS Code extension.
**Architecture touchpoints:** `loomd`, `loom-ipc`, `loom-event-bus`, new `loom-api` crate.
**Priority rationale:** Superset/RunPane/Nimbalyst mobile control; unlocks ecosystem integrations.

#### 8. Usage/Cost Dashboard — ✅ shipped as TUI Cost tab (0.8.0)
**Detail:** Tab 9 (new) = Cost. Real-time: tokens/session, cost/model, budget alerts. Historical: daily/weekly/monthly per agent, per task. Data from `loom-telemetry` + `loom-routing` catalog pricing.
**Architecture touchpoints:** `loom-tui`, `loom-telemetry`, `loom-routing`, `loom-sessions`.
**Priority rationale:** Vibe Console notch monitoring; Loom already has telemetry + routing catalog.

---

### P2 — Differentiation (2 shipped, 1 partial, 2 open)

#### 9. Agent Suggestion/Review Engine — ❌ open (pre-dispatch half; the post-session half is now Pillar 5)
**Detail:** Pre-dispatch hook analyzes task + context (knowledge graph, recent sessions) → emits "suggestions" (warnings, alternative approaches, missing tests). Post-dispatch hook validates output against suggestions. UI shows in TUI dispatch pane.
**Architecture touchpoints:** `loom-hooks`, `loom-routing`, `loom-knowledge`, `loom-tui`.
**Priority rationale:** Sculptor's "Suggestions" + Loom's knowledge graph = unique safety layer.
**2026-10-19:** the *post-session* half of this item is scoped as the **Pillar 5 verified review engine** (ROADMAP.md; spec in [plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md](plan/LOOM_VERIFIED_REVIEW_PROPOSAL.md)) — sandbox-verified findings, evidence labels, fix-PR policy, outcome-gated lessons. This item keeps the *pre-dispatch* half.

#### 10. ACP-Native Agent Registry — ✅ shipped as `loom acp` (status/start/stop/show/agents/run/run-status; capability negotiation itself remains open)
**Detail:** Extend `loom add-acp-agent` → full ACP client: `loom acp list`, `loom acp capabilities <agent>`, `loom acp run <agent> <prompt>`. GenericAdapter generates ACP manifest from TOML. Capability negotiation routes to best agent.
**Architecture touchpoints:** `loom-sessions` (acp_protocol.rs), `loom-generic-adapter`, `loom-cli`, `loom-routing`.
**Priority rationale:** Industry standard (ACP); Emdash/Superset use it. Loom becomes ACP hub.

#### 11. Declarative Automation DSL — ❌ open
**Detail:** `~/.loom/automations/*.toml`:
```toml
[[rule]]
on = "session.exported"
if = "session.tokens > 100000"
then = [{ action = "hook.run", id = "summarize" }, { action = "routing.recommend", tier = "slm" }]
```
Engine evaluates on event bus. Replaces ad-hoc scripts with auditable rules.
**Architecture touchpoints:** `loom-event-bus`, `loom-hooks`, `loom-routing`, new `loom-automation` crate.
**Priority rationale:** Emdash's YAML automations; Loom's event bus is the perfect substrate.

#### 12. Cross-Session Semantic Recall — ✅ shipped as `loom sessions recall` + `loom memory retain/recall/reflect` (0.6.0+)
**Detail:** `loom sessions recall <query>` → searches knowledge graph + session embeddings (local embedding model via `loom-ffi` or ONNX). Returns relevant entities, sessions, code snippets. Injects into new session via `--inject-knowledge`.
**Architecture touchpoints:** `loom-knowledge`, `loom-sessions`, `loom-ffi`, `loom-routing`.
**Priority rationale:** Nimbalyst/Vibe Kanban cross-project memory; Loom's knowledge graph + session compression = unique data moat.

#### 13. WASM Plugin System — ❌ open (plugin registry with SHA-256/Ed25519 verification exists; WASM runtime does not)
**Detail:** `loom plugin load <wasm>` → exports `SkillProvider`, `HookExecutor`, `RoutingTarget` via WASM component model. Sandboxed, hot-reloadable, language-agnostic. GenericAdapter generates WASM component from TOML.
**Architecture touchpoints:** `loom-generic-adapter`, `loom-skills`, `loom-hooks`, `loom-routing`, `loom-ffi`, new `loom-plugin` crate.
**Priority rationale:** Ultimate extensibility; all tools struggle with plugin ecosystems. Loom's trait-based architecture maps perfectly to WASM components.

---

## Implementation Sequencing Recommendation

```
Shipped (0.6–0.9):  P0 1–4 (Worktree, Container, PR, Remote SSH)
                     P1 5,6,8 (Kanban, Graph View, Cost Dashboard)
                     P2 10,12 (ACP registry, Cross-session recall)
Next (P1 remains):    Remote Control API (→ mobile companion), Multiplayer sync
Later (P2 remains):   Suggestions, Automation DSL, WASM runtime
```

**Rationale:** The P0 substrate work (worktrees, containers, remote execution, PR integration) has landed — the daemon is now a true distributed-capable runtime. P1's remaining items (remote-control API → mobile, multiplayer) are the next leverage point: Orca's mobile companion proves demand. P2's remaining items build the *moat* — features only possible because of Loom's unique architecture (knowledge graph + event bus + routing + traits).

---

## What NOT to Build (Off Path)

| Feature | Reason |
|---------|--------|
| Native Mac/SwiftUI app | Loom is cross-platform; TUI + remote API covers Mac |
| Electron desktop UI (Orca-style) | Explicit ROADMAP non-goal; TUI + remote API + web dashboard (via API) is lighter — Orca is the benchmark to watch, not the destination |
| Real-time collaborative editing | Too far from core; event bus sync is enough for now |
| Built-in LLM hosting | Loom routes *to* models; doesn't serve them |
| Visual mockup/design tools | Nimbalyst/Orca Design Mode territory; knowledge graph ≠ design canvas |
| Mobile-native apps | Remote API first; native apps later if API proves valuable |

---

## Conclusion

Loom's moat is its **programmable infrastructure layer**: daemon + IPC + event bus + knowledge graph + routing + trait-based adapters. The competitive tools — Orca included — build *applications* on top of weaker substrates. Much of the substrate hardening has already shipped (0.6–0.9: worktrees, containers, remote SSH, PR integration, Kanban/Graph/Cost tabs, ACP dispatch, cross-session recall). The evolution continues:

1. **Finish the control plane** (remote-control API → mobile companion; multiplayer sync)
2. **Build unique intelligence on top** (suggestions, automation DSL, WASM plugins)

Every feature above compiles to existing crates. No architectural pivots needed — only vertical integration.