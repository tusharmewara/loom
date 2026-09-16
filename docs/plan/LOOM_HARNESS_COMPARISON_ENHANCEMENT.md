# Loom vs HarnessRouter / emdash / bb — Compatibility Analysis & Enhancement Document

> Status: analysis, 2026-09-13; re-checked against the tree 2026-09-16 (items marked ✅ verified, 🟡 partial, ⬜ open).
> Scope: Loom v0.7.2 (`ECOSYSTEM_VERSION` 2.0.0) vs HarnessRouter Community Edition (UHP), emdash, and bb.

## Executive Summary

Loom, HarnessRouter, emdash, and bb are all members of the same category: an **orchestration layer above the same CLI agent harnesses** (Claude Code, Codex, OpenCode, Gemini CLI, …). None of them replace a harness — each standardizes how a user or product *drives* one.

| | Loom | HarnessRouter (CE) | emdash | bb |
|---|---|---|---|---|
| Form factor | Rust CLI + daemon + TUI | Python server (UHP, Responses API) | Node desktop app (ADE) | Node self-building IDE + HTTP/SDK |
| Primary job | Cross-tool shared ecosystem (skills, sessions, MCP, auth, hooks) | Run harnesses as a product backend | Parallel agents in git worktrees, diff/PR review | Threads you steer/hand off; self-extending |
| Tool coverage | 12 preset adapters | 10 base harnesses | 22 providers | 7 providers |
| Sessions | Cross-tool export/import/merge/resume + run-linked sessions (every ACP dispatch captures stdout/stderr into a session, memory-indexed, knowledge-extracted) + canonical-ACP transcripts via `loom acp run` | Persistent via `previous_response_id` | Per-worktree resume | Threads |
| Remote/SSH | SSH/SFTP connections + `remote exec/sync` (List/Add/Remove/Exec/Sync); `remote exec --capture-session` persists output as an indexed session. No remote-*worktree* orchestration UX yet | Sandboxed containers | SSH/SFTP | No |
| HTTP API | Unix-socket IPC only | Full HTTP + SSE streaming | Web/desktop UI | HTTP + CLI + SDK |
| Licensing | MIT (workspace) | Apache-2.0 | Apache-2.0 | OSS |

**Bottom line**: Loom already owns the one thing none of the others provide — **session portability and shared resources *across* heterogeneous agents**. Its main loses relative to the others are (a) adapter breadth, (b) a product-facing HTTP/streaming surface, (c) a parallel worktree UI, and (d) remote *worktree orchestration* UX (remote execution itself exists). This document maps both the compatibility surface and a prioritized feature list to close the gap.

## 1. Supported-Tool Compatibility Matrix

Loom's preset adapters live in `crates/loom-adapters/src/lib.rs` (`adapter_for` / `supported_tools`). HarnessRouter's base catalog lives in `gateway/app.py` (`_BASE_CATALOG`); emdash's registry is `src/shared/providers/registry.ts`; bb's providers are enumerated in its `bb-app` README.

### 1.1 Overlap

| Loom adapter | HarnessRouter | emdash | bb | Shared native roots |
|---|---|---|---|---|
| `claude-code` | ✅ | ✅ | ✅ | `~/.claude/` |
| `codex` | ✅ | ✅ | ✅ | `~/.codex/` |
| `opencode` | ✅ | ✅ | ✅ | `~/.opencode/` |
| `gemini-cli` | ✅ `gemini` | ✅ | ❌ | `~/.gemini/` |
| `cursor-cli` | ❌ | ✅ | ✅ (ACP) | `~/.cursor/` |
| `goose` | ❌ | ✅ | ❌ | `~/.goose/` |
| `kiro` | ❌ | ✅ | ❌ | `~/.config/Kiro` |
| `github-copilot` | ❌ | ✅ | ❌ | `~/.config/github-copilot` |
| `crush` | ❌ | ✅ `Charm` | ❌ | `~/.crush/` |
| `aider` / `jcode` / `juno` | ❌ | ❌ | ❌ | Loom-only |

Universal overlap (all four): **`claude-code`, `codex`, `opencode`**.

### 1.2 Tools each orchestrator runs that Loom has no preset adapter for

| Orchestrator | Missing under Loom |
|---|---|
| HarnessRouter (10) | `hermes`, `pi`, `omp`, `dsh`, `qwen`, `cline` (6) |
| emdash (22) | `Amp`, `Auggie`, `Autohand Code`, `Cline`, `Codebuff`, `Continue`, `Devin`, `Droid`, `Kilocode`, `Kimi`, `Mistral Vibe`, `Pi`, `Qwen Code`, `Rovo Dev` (14) |
| bb (7) | `pi`, `grok`, `hermes-agent`, `omp` (4) |

Union of missing tools (18): `hermes`, `pi`, `omp`, `dsh`, `qwen`, `cline`, `grok`, `amp`, `auggie`, `autohand-code`, `codebuff`, `continue`, `devin`, `droid`, `kilocode`, `kimi`, `mistral-vibe`, `rovo-dev`.

These are still runnable today via the TOML **generic adapter** or `loom exec` binary fallback (`exec.rs:72`), but without presets (no native skills dir, no MCP config path, no routing env var).

### 1.3 Feature-level compatibility

1. **Agent Skills standard — mutually compatible.** emdash installs skills once to `~/.agentskills/` and syncs to each agent's native dir; bb indexes documented native skill roots plus the compatibility root `.agents/skills`; Loom symlinks `~/.loom/skills` into adapter native dirs (`sync_skills_to_native`, `lib.rs:222`). All target the same native dirs for the shared tools, so skills placed by Loom are picked up by emdash/bb and vice versa.
2. **MCP sharing.** Loom's `sync_mcp_to_native` merges under `mcpServers` additively (`lib.rs:201`), so emdash/bb coexists without clobbering. Tool-defined servers take precedence.
3. **Hook collision risk.** emdash writes marker-tagged lifecycle hooks into agent user-level config. Loom also writes tool config. Both must merge additively or one will clobber the other.
4. **Different control planes.** bb drives `cursor` and `hermes-agent` over **ACP**; HarnessRouter runs harnesses headless in sandboxes; emdash launches PTYs; Loom launches CLIs with env + config. Same binaries, different driving — no conflicts, but session state is not shared across orchestrators unless it lives in the agent's own store.

## 2. Heads-up: current-state caveats

- **Naming**: HR uses `gemini` / `claude-code` (alias `claude`); Loom accepts `gemini`/`gemini-cli` and `claude-code`/`claude_code` only.
- **Model catalog drift**: Loom's routing catalog pins `claude-sonnet-4`, `gpt-4o`, `gemini-2.5-pro-exp-03-25` (`catalog.rs:53`); the three peers are on `claude-sonnet-4.6`-class IDs. Decisions still apply via env vars but lag current model IDs.
- **Known stubs — both resolved 2026-09-16.** ACP dispatch is wired and tracked (`acp.dispatch` → headless spawn, per-run logs, completion + session capture, `run.status`; plus a canonical ACP-spec v1 client in `loom-sessions/src/acp_protocol.rs` with `loom acp run`). EmbeddedPane dispatch opens the pane and streams `workflow dispatch` output. (AGENTS.md gotchas #9/#10 corrected accordingly.)

## 3. Feature Recommendations — What To Add To Make Loom the Better Choice

Priorities: **P0** = biggest compatibility/differentiation lever. **P1** = closes feature-parity gaps. **P2** = ecosystem/alignment.

### P0 — Differentiators

1. **Preset adapters for the 18 missing tools** (`hermes`, `pi`, `omp`, `dsh`, `qwen`, `cline`, `grok`, `amp`, `auggie`, `autohand-code`, `codebuff`, `continue`, `devin`, `droid`, `kilocode`, `kimi`, `mistral-vibe`, `rovo-dev`).
   New files under `crates/loom-adapters/src/`, each wiring native skills dir, MCP config path, session dir, and routing env var. This alone makes Loom the widest-coverage layer of the four (12→30 adapters).
   *Parallel track*: bake sample TOMLs into `~/.loom/tools/` so each tool works via the generic adapter **immediately**, before the Rust presets land.

2. **`~/.agentskills/` and `.agents/skills` as shared skill sources.** Add these compatibility roots to `loom-skills` (in addition to `~/.loom/skills`). Loom becomes the hub: install once, sync into every native root emdash/bb read — and import skills written by emdash/bb users.

3. **Worktree-isolation UX parity.** `loom exec --parallel` already has `WorktreeManager` and `AgentPool` (`loom-parallel`). Expose it from the TUI task view and `loom workflow dispatch --parallel`, with per-task `agenter/<short>-<slug>` branches and base-ref pinning (the emdash/bb model). This is the single biggest visible feature the two IDEs have that Loom's terminal won't yet.

4. **UHP client adapter (dispatch *to* HarnessRouter).** `loom thread harnessrouter` → `GET /v1/harnesses`, `POST /v1/responses`, consume SSE, persist into Loom sessions. Makes HarnessRouter a first-class Loom backend and reuses the Responses-compatible surface.

### P1 — Parity

5. **UHP-compatible HTTP/SSE endpoint from `loomd`** (`/v1/responses`, `/v1/harnesses`, `/v1/models`), turning Loom into a server other products can drive — the mirror image of #4. Ship a conformance run against HarnessRouter's UHP conformance suite.

6. **✅ Complete ACP dispatch (P1/2.5).** DONE 2026-09-16: canonical ACP v1 client (`initialize` → `session/new` → `session/prompt` with stop reasons, `session/update` transcript collection, permission auto-policy, timeout + cancel) in `loom-sessions/src/acp_protocol.rs`, tested against a fixture fake server, exposed as `loom acp run` (persists indexed sessions). Remaining: TUI/daemon wiring of the protocol client (dispatch *through* ACP-speaking agents rather than only *to* them).

7. **Lifecycle-hook tagging + additive config merge.** Marker-tagged hooks for status/notifications/resume (emdash-style), with strict additive JSON merge to coexist safely.

8. **Model catalog refresh + cost-aware routing.** Update built-in model IDs and add cost-based model selection (extends existing `loom-routing`, per the earlier Xirp analysis).

9. **🟡 Remote execution over SSH/SFTP.** DONE 2026-09-16 as far as execution + capture go: `loom remote` already managed connections (List/Add/Remove/Exec/Sync over SSH/SFTP with vault-backed keys), and `remote exec --capture-session` now persists output as an indexed, knowledge-extracted session. Remaining (the actual gap): running the parallel *worktree* workflow on remote checkouts.

10. **Gateway for third-party session resume.** Read emdash/bb/HR session stores and surface them in `loom sessions` so a conversation started under another orchestrator can be resumed under any Loom-registered tool.

### P2 — Ecosystem alignment

11. **✅ Ticket-driven dispatch.** DONE 2026-09-16: `loom workflow create --from-ticket` accepts `github:owner/repo#123` (plus legacy bare forms), `linear:TEAM-1`, `gitlab:owner/repo#1`, and `jira:PROJ-1`, creating linked tasks via the trackers' credentials/env vars. (Live fetch needs tracker credentials + network; parsing/routing is unit-tested.)

12. **✅ Diff review + PR pipeline in TUI.** DONE 2026-09-16: diff view and CI status already existed in the PRs tab; `loom issues create-pr` (shells `gh`, `--fill` by default) plus the `c` key on the PRs tab close the loop (emdash's review loop, terminal-first).

13. **Trace/OpenTelemetry export.** Emit spans from dispatch, routing, and sessions; import/export the UHP trace fields.

14. **UHP + Agent Skills conformance suite runs.** Validate the new endpoints/skills roots against the published conformance suites so "works with Loom" is testable, not promised.

## 4. Implementation Roadmap

### Phase 1 (fastest ROI, P0)
- 18 generic TOML adapter templates + docs (no Rust needed).
- `~/.agentskills/` + `.agents/skills` shared-skill roots.
- TUI + workflow parallel worktree surface on top of `loom-parallel`.
- UHP client adapter (`loom thread harnessrouter`).

### Phase 2 (parity)
- UHP server endpoint on `loomd` + conformance run.
- ACP dispatch completion; lifecycle-hook tagging + additive merge.
- Model catalog refresh + cost-aware routing; SSH remote worktrees.

### Phase 3 (ecosystem)
- Ticket-driven dispatch, TUI diff/PR review, OTel export, conformance badges.
- Preset Rust adapters for the 18 tools as the generic TOMLs prove their native paths.

## 5. Competitive Positioning

- **Stay**: cross-tool session portability, shared skills/MCP/auth/hooks, security (encrypted vault, Ed25519, ACL), performance budgets — none of the three peers export/resume a session *across different agents*.
- **Add**: adapter breadth (#1–2) and worktree UX (#3) neutralize emdash/bb; UHP client/server (#4–5) makes Loom both a *front-end* to HarnessRouter and a *server* for products.
- **Verdict**: Loom's terminal-first orchestration + universal resource sharing + UHP duality is a defensible position that none of HarnessRouter, emdash, or bb occupies alone.

## Sources

- HarnessRouter Community Edition: `gateway/app.py` `_BASE_CATALOG` (10 bases), protocol/README.md (UHP), github.com/HarnessRouter/harnessrouter.
- emdash: `generalaction/emdash` README + providers docs (22 providers), `src/shared/providers/registry.ts`.
- bb: `get-bb/bb` `packages/bb-app/README.md` (7 providers, native skill roots).
- Loom: `crates/loom-adapters/src/lib.rs`, `crates/loom-cli/src/commands/exec.rs`, `crates/loom-routing/src/catalog.rs`.