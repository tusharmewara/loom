# Horizontal Scaling Investigation

**Status:** Investigation (P4.3) — no implementation. Answers: what scales today, what doesn't, and the cheapest path if multi-host ever becomes a requirement.

## Current posture: single-host by design

Loom is a **workstation-layer** ecosystem. Every network surface binds loopback only:

| Surface | Transport | Bind | Location |
|---|---|---|---|
| Daemon IPC | Unix socket `~/.loom/loomd.sock` | local only | `loomd/src/lib.rs` |
| Daemon gRPC | TCP | `127.0.0.1:50051` | `loomd/src/grpc_server.rs:39` |
| ACP server | TCP | `127.0.0.1:0` (ephemeral) | `loom-sessions/src/acp_server.rs:59` |
| Event bus (cross-process) | ZeroMQ `ipc://~/.loom/.loom-events` | local only | `loom-event-bus` |
| Remote exec | SSH/SFTP (user-initiated, vault-backed keys) | outbound | `loom-remote` |

All state is filesystem-local under `~/.loom/`: SQLite (`skills.db`, `mcp_registry.db`, `knowledge/graph.db`, per-session `index.db`), JSONL sessions, TOML configs. SQLite has a single writer per database file — **correct** for one daemon, a hard ceiling for many.

## What already scales

- **Agent fan-out**: `loom-workflow` chief mode already isolates parallel tasks in git worktrees (`chief/scheduler.rs`), and `loom-runtime` has `docker`/`podman`/`kubernetes` container features (`loomd/src/lib.rs:67-78`). The *agents* can run anywhere; only the *coordinator* is local.
- **Cross-tool sharing**: the entire point of Loom — one daemon serves all local tools (jcode, crush, claude-code, opencode, goose) with a shared MCP pool, refcounted so N tools reuse 1 server process (`loom-mcp/pool.rs`).
- **Remote execution**: `loom remote exec` + `loom remote sync` already push work and artifacts to remote hosts over SSH.

## What does not scale (and why it's fine)

1. **Multiple daemons on one host** — no need; the daemon aggregates all subsystems and idles out after 5 min. Two daemons would double-spawn MCP processes and race on SQLite write locks.
2. **Multiple hosts sharing one `~/.loom`** — SQLite over NFS is corruption-prone; session JSONL appends would interleave. Not supported, don't try.
3. **Load-balanced MCP pool** — the pool's value is *deduplication* (one process, N consumers), not throughput. A network pool would add latency to a latency-sensitive local path for zero dedup win.

## If multi-host ever becomes real: the cheapest path

Ordered by ascending effort; none are needed today (YAGNI — recorded here so the decision is pre-made when the requirement appears):

1. **Federation over ACP (weeks).** The ACP server already speaks TCP; changing `bind` from `127.0.0.1` to a real address + a peer registry in `acp.toml` makes daemons addressable cross-host. Sessions/knowledge stay per-host; dispatch delegates across the wire. Smallest diff, matches the existing protocol.
2. **State export/sync (days of work per store).** `loom sessions export`/`import` already round-trips JSONL. A scheduled `remote sync` of `~/.loom/sessions` + `~/.loom/knowledge` gives eventual-consistency sharing without touching SQLite concurrency. Fine for "share knowledge across my machines", wrong for teams.
3. **Server state layer (months — a different product).** Replacing the SQLite registries with Postgres + object storage turns Loom into a hosted service. That contradicts the workstation-layer thesis (README: tools keep autonomy, Loom is the shared fabric). Do this only if Loom grows a server SKU.

## Verdict

**No horizontal scaling work is warranted.** The architecture is deliberately local-first; the escape hatches (container runtimes for agents, SSH remote exec, TCP-capable ACP) already exist for the cases that matter. The correct next investment on this axis is none — revisit only when a concrete multi-host requirement arrives, then start with ACP federation.

*ponytail: this doc is the whole P4.3 deliverable — an investigation, not an implementation.*
