> **RESOLVED 2026-10-22 — this is the review record, kept as it was written.**
>
> The findings below were raised against the two pillar-3/pillar-4 branches and were
> closed by the fix cards (P3-F1, P4-F1) and by the merge work before
> `p3p4/merge` landed on `main`:
>
> | Finding | Severity | Resolution |
> |---|---|---|
> | P3-1 | HIGH | Closed. The spec now has §2.9 "The ACP dispatch server (agent-to-agent TCP dispatch)": the TCP/JSON-line server, the Ed25519 challenge-response handshake, the daemon wiring (`acp.status` / `acp.start` / `acp.stop`), the `loom acp …` CLI actions, and `acp_discovery` / `agent_provider` / `acp_handshake` — each anchored to a source line. |
> | P3-2 | MEDIUM | Closed. Both branches ship together, so the spec was re-anchored against the merged tree: 95 body anchors and 106 §5 index entries were re-pointed at the line that holds the construct the sentence names (the authored line's text is located in the merged tree; where the merge rewrote the line, the named construct is used instead). The index labels are unchanged — only the line numbers moved. |
> | P3-3 | MEDIUM | Closed. §1.1 states the `apply_routing_decision` convention: `<TOOL>_MODEL` + `LOOM_ROUTING_PROVIDER`, the read-back rule, and `crush`'s HTTP deviation. |
> | P3-4 | LOW–MEDIUM | Closed. §5 is a complete per-file body-anchor index; the claim is narrowed to the body. |
> | P3-5 | LOW | Closed. The four `adapter_for` aliases (`claude_code`, `cursor-cli`, `gemini`, `copilot`) are listed. |
> | P3-6 | LOW | Closed. §2.8 says the registry file belongs in a fork of `github.com/agentclientprotocol/registry` — the directory is an unpopulatable gitlink. |
> | P3-7 | LOW | Closed. One in-tree test parses the published §1.5 tool TOML out of the spec itself (`crates/loom-generic-adapter/tests/spec_example_test.rs`), so the document and the schema cannot drift apart. |
> | P4-1 | HIGH | Closed. The token aggregates are nullable end to end (`SUM(input_tokens)` without `COALESCE`; `UsageTotals.input_tokens` / `ToolUsage.input_tokens` are `Option<u64>`), and the ledger prints "unavailable (no run reported a token count)". The provenance is split in two: `reported_runs` vs `measured_runs` — a cost-only run is *reported*, not *measured*. |
> | P4-2 | HIGH | Closed. The scope now lives in the tree: a `ROADMAP.md` pillar-4 bullet, the `crates/loom-telemetry/src/usage.rs` module doc, and a `CHANGELOG.md` entry — what is measurable now, what is **not** yet metered, and that pillar 4 remains post-1.0 and gated on pillars 1–3. |
> | P4-3 | MEDIUM–LOW | Closed. `loom telemetry reset` says it resets the daemon's in-memory counters only; the ledger is untouched. |
> | P4-4 | MEDIUM–LOW | Closed. The method labels name what upstream defines: `acp:session-usage` and `acp:session-context-cost` — ACP's counts are cumulative across the session, not per turn. |
> | P4-5 | LOW–MEDIUM | Closed. `cargo fmt --all` was run; the three mangled lines are gone. No CI fmt job exists, so this is a human check — see "Follow-up" below. |
> | P4-6 | LOW | Closed. `acp.dispatch` returns the metered `usage` object in its reply, matching the CLI path. |
> | P4-7 | LOW | Closed. One `chars/4` estimator, in `loom-core`. |
> | P4-8 | LOW | Closed. `loom sessions resume` hands the adapter a real cost estimate instead of a default one. |
>
> The review's own baseline is reproduced below unchanged.


# Pillar 3 & Pillar 4 Review — `p3/protocol-spec` and `p4/usage-metering`

**Reviewer:** lead (review only — no fixes applied, no merges, no pushes)
**Date:** 2026-10-21
**Base:** `main` @ `b77b5cf`
**Branches under review:** `p3/protocol-spec` @ `0c6c01e`, `p4/usage-metering` @ `c5e596a`
**Test bed:** `.worktrees/p34-review` on branch `p3p4/review` (both branches merged, HEAD `a7d7d85`)

---

## 0. Summary

| Branch | Verdict | One-line reason |
|---|---|---|
| `p3/protocol-spec` | **FIX FIRST** | The spec is unusually honest and its anchors are all true at its own base, but it documents "ACP dispatch" while omitting the ACP **dispatch server** entirely, and ~21% of its `file:line` anchors go stale the moment `p4` is merged alongside it. |
| `p4/usage-metering` | **FIX FIRST** | The hardcoded constant is genuinely gone, there is exactly one meter, and the tests are honest — but the reporting layer still prints `Measured tokens: 0 in / 0 out` for a run that reported no count, and the branch states its now-vs-pillar-4 scope **nowhere in the tree**. |

Both branches are close to shippable. The fixes below are named, small, and specific. Neither branch needs to be reworked.

---

# PART A — `p3/protocol-spec`

## A.0 What passed

- **Anchor truth at the spec's own base (item 1).** I extracted every explicit `file:line` anchor in the body (236) and every entry in the §5 index (219 parsed / 146 line-checkable) with a script, then resolved each one against `git show main:<file>`. **Every anchor resolves to the construct the spec claims.** I spot-checked >100 of them by hand across `loom-adapters/src/lib.rs`, `loom-generic-adapter/src/{config,lib,traits,scanner}.rs`, `loom-sessions/src/{acp_protocol,acp_launch,acp_client}.rs`, `loom-core/src/{lib,constants,types,config,plugin_registry}.rs`, `loom-cli/src/cli.rs` + `commands/{thread,tools_acp,acp_run,acp,exec,lsp,config,init}.rs`, `loomd/src/{capture,ipc_handler}.rs`, `loom-auth/src/import.rs`, the ACP registry schemas and `fake_acp_server.py`. **Zero false anchors against `main`.**
- **Compatibility policy (item 3) — PRESENT.** `docs/protocol/adapter-and-acp-protocol.md` §3 (spec lines 677–719) states what is stable within 1.x, what may change without a bump, what requires a bump, what Loom does not promise, and where the version lives. This is a complete policy; the card's requirement is met.
- **Worked example (item 4) — VERIFIED AGAINST THE REAL PARSER.** I wrote a throwaway test in a tempdir that fed the §1.5 38-line tool TOML to `toml::from_str::<loom_generic_adapter::config::ToolToml>`: it parses, and `LaunchSection { runtime: "process", command: "acme", args: ["--acp"], auto_remove: true }` is what the runtime sees. I also verified the §2.8 `agents.toml` example through the real resolver `loom_sessions::acp_launch::launch_from_entry`: `runner = "npx"` → program `npx`, args `["@acme/agent@1.0.0", "--acp"]`, env `ACME_API_KEY`; `runner = "binary"`, `cmd = "acme-agent"` → program `acme-agent`, args `["acp"]`. Both examples are copyable and correct. (Probe files were deleted; the worktree is clean.)
- **Honesty about the registry gap (item 5) — HONEST.** The spec does **not** claim the `acp_registry` schemas are published. §2.7 states the directory "is committed as a **gitlink** pinned at `2edbfd245997eae3b19c0b6cba46f86826dc5f44` — but with **no `.gitmodules` entry**, so a fresh clone does not populate it", §4 #9 repeats it, and the ROADMAP bullet it adds says "the directory is an unpopulatable gitlink today". I confirmed independently: `git ls-tree main acp_registry` → mode `160000`, and `git submodule status` → *"fatal: no submodule mapping found in .gitmodules for path 'acp_registry'"*. The transcribed schema rules are true against the pinned commit (verified `agent.schema.json:7/:11/:21/:54/:78/:91/:121`, `registry.schema.json:7`, `CONTRIBUTING.md:13`, `AUTHENTICATION.md:7`).
- **Scope.** `git diff main..p3/protocol-spec --stat` = 4 files, all documentation (`docs/protocol/adapter-and-acp-protocol.md` +857, `CHANGELOG.md` +6, `README.md` +2, `ROADMAP.md` +6). **No runtime behaviour changed**, no new dependency, no new CLI group. The card's "do not change runtime behaviour" rule is respected. README.md:80 and CHANGELOG.md link the spec; ROADMAP.md:42 carries the pillar-3 status.

## A.1 Findings — `p3/protocol-spec`

### P3-1 (High) — The spec documents "ACP dispatch" and never mentions the ACP dispatch server

**Evidence.** `docs/protocol/adapter-and-acp-protocol.md` line 16 lists the second extension point as "**ACP dispatch** … What Loom reads it from: `loom-sessions` / `loomd`". Grepping the whole 857-line document for `acp_server|dispatch server|AcpDispatchServer|acp start|acp status|ed25519|handshake` returns **zero** hits (only `fake_acp_server.py`, the test fixture).

The omitted surface is substantial and public:

| Omitted | Evidence |
|---|---|
| The dispatch server itself | `crates/loom-sessions/src/acp_server.rs:1-27` (module doc: "TCP server for agent-to-agent task dispatch", Ed25519 challenge-response handshake, JSON-line `handshake`/`handshake_ack`/`dispatch`/`health`/`response` protocol), `:45` `AcpServerConfig`, `:58` `default_bind` → `"127.0.0.1:0"`, `:79` `DispatchRequest`, `:93` `DispatchResponse`, `:103` `AcpDispatchServer`, `:122` `new`, `:177` `public_key_hex` |
| The daemon wiring | `crates/loomd/src/lib.rs:30`, `:122-128`, `:239-243`, `:283-316` (auto-starts with the daemon), `crates/loomd/src/ipc_handler.rs:21`, `:39`, `:493`, `:535` — IPC commands `acp.status` / `acp.start` (errors: auto-starts) / `acp.stop`; key material in `{loom_dir}/acp_server.json` + `acp_server_key.txt` |
| The CLI surface | `crates/loom-cli/src/cli.rs:1773-1805` `enum AcpAction { Status, Start, Stop, Show, Agents, Registry, Run, RunStatus }`, dispatched in `crates/loom-cli/src/commands/acp.rs:9-48`. The spec documents only `loom acp run` and `loom add-acp-agent` (grep `loom acp` → 6 hits, all `run`/`run-status`) |
| Three more session modules | `crates/loom-sessions/src/acp_handshake.rs` (Ed25519 `AcpKeyPair`, `perform_responder_handshake`), `crates/loom-sessions/src/acp_discovery.rs` (`AcpPeer`, local peer table, HTTP health check, mDNS feature), `crates/loom-sessions/src/agent_provider.rs` (`AgentProvider` trait) |

**Why it matters.** A third party reading a spec titled "Adapter Contract & **ACP Dispatch**" to learn how to plug an agent into Loom will get the client/stdio side and nothing about the daemon-side dispatch server — the one that is actually auto-started by `loomd` and is the substrate pillar 5 ("blind dispatch") depends on. It is also the only Loom extension point with a **network** surface and a **handshake**, i.e. the one where a third party most needs a written contract.

**Required fix (do not make it here):** add a section covering the dispatch server — the JSON-line message shapes, the Ed25519 handshake, `AcpServerConfig`, the `acp.status`/`acp.start`/`acp.stop` IPC commands, the `loom acp status|start|stop|show|agents|registry|run-status` CLI actions, and where the key pair lives — with `file:line` anchors in the same style as the rest of the document.

### P3-2 (Medium) — ~21% of the spec's anchors go stale the moment `p4` is merged with it

**Evidence.** Re-running the anchor resolver against the **merged** worktree (both branches, HEAD `a7d7d85`):

- Body: **236 explicit anchors, 49 stale** — `crates/loom-sessions/src/acp_protocol.rs` 41, `crates/loom-core/src/types.rs` 3, `crates/loom-cli/src/commands/acp_run.rs` 5.
- §5 index: **146 line-checkable entries, 47 stale** — `acp_protocol.rs` 46, plus `crates/loom-sessions/tests/fixtures/fake_acp_server.py:4`/`:71` (the fixture gained a `usage` mode, so the mode anchors moved).

Concrete mis-resolutions in the merged tree (spec claim → what the merged file actually has at that line):

| Spec anchor | Claim | Merged reality |
|---|---|---|
| `acp_protocol.rs:354` | `"protocolVersion": ACP_PROTOCOL_VERSION,` | `"id": id,` |
| `acp_protocol.rs:368` | `if version != Some(ACP_PROTOCOL_VERSION) {` | `}` |
| `acp_protocol.rs:99` | `pub fn build_request(id: u64, method: &str, params: Value) -> String {` | `let method = value` |
| `acp_protocol.rs:52` | `pub enum AcpIncoming {` | a doc comment about `usage_update` |
| `types.rs:960` | `pub trait LspProvider: Send + Sync {` | `#[serde(default)]` |
| `types.rs:984` | `pub trait DebugProvider: Send + Sync {` | a doc comment |

Cause: `p4` adds +102 lines to `crates/loom-sessions/src/acp_protocol.rs` (at the top of the file, `:48-66` and `:191-235`), +265 to `crates/loom-core/src/types.rs`, and +29 to `crates/loom-cli/src/commands/acp_run.rs` — all before the anchors the spec cites.

**Why it matters.** The spec's entire value proposition is the sentence at line 20: *"Every normative statement carries a `file:line` anchor into the Rust source so a reader can verify it."* A spec whose anchors no longer verify is worse than no spec (item 1 of the review brief). This is **not** a p3 authoring error — every anchor is true at `main` — it is a merge-ordering hazard that both branches ignore.

**Required fix:** re-anchor the affected entries after the p3/p4 merge lands together (a mechanical pass over `acp_protocol.rs`, `types.rs`, `acp_run.rs`, `fake_acp_server.py`), or move the p4 additions to the end of those files. Consider anchoring by symbol name plus line (`ACP_PROTOCOL_VERSION` at `acp_protocol.rs:20`) so future insertions above the symbol do not invalidate the index.

### P3-3 (Medium) — `apply_routing_decision` is documented as "apply to the tool's environment" without saying how

**Evidence.** Spec §1.1 row for `apply_routing_decision` (`crates/loom-adapters/src/lib.rs:39`) reads, in full: *"Apply a routing decision to the tool's environment."* Grepping the spec for `_MODEL|LOOM_ROUTING_PROVIDER|set_var` → **zero** hits.

The actual convention every preset uses is a **specific environment-variable name**:

- `crates/loom-adapters/src/aider.rs:52-53`, `claude_code.rs:47`, `codex.rs:52-53`, `cursor_cli.rs:52-53`, `gemini_cli.rs:52-53`, `github_copilot.rs:56-57`, `goose.rs:52-53`, `jcode.rs:78-79`, `juno.rs:60-61`, `kiro.rs:61-62`, `opencode.rs:52-53` — `std::env::set_var("<TOOL>_MODEL", recommended_model)` + `set_var("LOOM_ROUTING_PROVIDER", recommended_provider)`.
- `crates/loom-adapters/src/crush.rs:52-73` — a different mechanism entirely: it POSTs to `CRUSH_API_URL/config/model` and only falls back to `CRUSH_MODEL`.

The consumer side depends on the exact variable name: `crates/loom-cli/src/commands/resume.rs:349`/`:356` and `crates/loom-cli/src/commands/exec.rs:362` read it back as `format!("{}_MODEL", agent.to_uppercase().replace('-', "_"))`.

**Why it matters.** A third-party adapter author who implements `apply_routing_decision` any way they like will have their routing decision silently ignored by `loom exec` / `loom sessions resume`, because the read-back path is keyed on `<TOOL>_MODEL`. The spec is the only public statement of this contract.

**Required fix:** state the convention explicitly — the `<TOOL>_MODEL` + `LOOM_ROUTING_PROVIDER` environment-variable protocol, the `format!("{}_MODEL", agent.to_uppercase().replace('-', "_"))` read-back rule, and the fact that `crush` is the one preset that deviates.

### P3-4 (Low–Medium) — The §5 anchor index is incomplete, though it claims to be complete

**Evidence.** Spec line 755: *"Every `file:line` anchor in this document, grouped by file."* The index (§5, lines 753–857) has 14 file headings + 5 directory headings, but **five files that the body anchors reference are absent from it**:

- `crates/loom-cli/src/commands/tools_acp.rs` (body lines 502, 510, 513, 515, 516, 520, 735, 736, 741)
- `crates/loom-sessions/src/store.rs` (body line 474)
- `crates/loom-tui/src/dispatch.rs` (body line 185)
- `crates/loom-workflow/src/dispatch.rs` (body line 184)
- `acp_registry/acme-agent/agent.json` (body line 649)

**Required fix:** add the five entries to §5, or change the claim to "every anchor in the code fences and tables" if the intent is narrower.

### P3-5 (Low) — `adapter_for` accepts four aliases the spec does not list

**Evidence.** `crates/loom-adapters/src/lib.rs:316-331` accepts `"claude_code"`, `"cursor-cli"`, `"gemini"` and `"copilot"` as aliases; `supported_tools()` (`:334-347`) lists only the 12 canonical names, and spec §1.1 lists only those 12. A third party calling `adapter_for("copilot")` gets an adapter, but the spec says there is no such tool.

**Required fix:** one sentence in §1.1 listing the accepted aliases and stating that only the canonical 12 appear in `supported_tools()`.

### P3-6 (Low) — The §2.8 worked example asks the reader to write into a directory that does not exist in their clone

**Evidence.** Spec §2.8 step 3 (line 649): *"Add `acp_registry/acme-agent/agent.json`"*. The same document's §2.7 (line 594) and §4 #9 (line 738) state that `acp_registry/` is a gitlink with no `.gitmodules` entry, so a fresh clone cannot populate it. Step 3 therefore cannot be followed as written.

**Required fix:** say explicitly that the step applies to a fork of `github.com/agentclientprotocol/registry` (which is where the vendored copy's origin lives — `acp_registry/.git` remote is `https://github.com/agentclientprotocol/registry.git`), not to a Loom working tree.

### P3-7 (Low) — The published examples have no executable check in the tree

**Evidence.** `p3` commit `ec2c5b1` ("chore(P3): drop the throwaway spec-check test file") removed the verification file the card asked to be written. The card only required the check to be *performed*, so dropping it is compliant — but it means a future edit to `ToolToml` can invalidate the published §1.5 example with no test failing. The §2.8 `agents.toml` example and the `agent.json` example are likewise unverified in-tree.

**Required fix (optional, cheap):** a single test in `crates/loom-generic-adapter/tests/` that parses the published example TOML via `toml::from_str::<ToolToml>`, using `tempfile::tempdir()` per `AGENTS.md`.

---

# PART B — `p4/usage-metering`

## B.0 What passed

- **The constant is gone (item 6, first half).** `crates/loom-routing/src/router.rs:74-84` no longer hardcodes anything; it calls `CostEstimate::estimated(score.signals.description_length, model.input_cost_per_1k, model.output_cost_per_1k, "medium")` where `description_length` is the prompt's character count (`crates/loom-routing/src/scorer.rs:83`). A **second** copy of the same constant existed on the manual-override path (`crates/loom-cli/src/commands/routing.rs:320-323` on `main`) and was removed too (now `routing.rs:322-331`). Grepping the merged tree for `1000u32` / `input_tokens: 1000` returns nothing.
- **Measured and estimated are distinguishable in the model (item 6, second half).** `crates/loom-core/src/types.rs:641-648` `pub enum UsageSource { Measured, Estimated, #[default] Unavailable }` with `as_str()` (`:652`) and `from_str_lossy()` (`:661`, unknown → `Unavailable`); `CostEstimate` (`:586-598`) carries `source` and `method`; `UsageRecord` (`:713-742`) carries `source`, `method`, the measured legs (`Option<u64>`) and separate forecast columns (`est_input_tokens`, `est_output_tokens`, `est_cost_usd`). Every print site shows the source: `crates/loom-cli/src/commands/routing.rs:211-217`, `:295-299`; `crates/loom-workflow/src/context_injector.rs:27-40`; `crates/loom-cli/src/commands/telemetry.rs:84-99`. `estimated_cost_usd` is never populated with a measured value — `CostEstimate::estimated` always sets `source = Estimated` (`types.rs:606-630`).
- **Exactly one meter owner (item 7).** `crates/loom-telemetry/src/usage.rs:1-18` states the ownership rule, and it holds in the code: every write goes through `loom_telemetry::UsageMeter` into one database, `{loom_dir}/telemetry/usage.db` (`usage.rs:75`). The six call sites are `crates/loom-cli/src/commands/workflow.rs:25`, `acp_run.rs:147`, `knowledge.rs:463`, `resume.rs:259`, `telemetry.rs:164`, and `crates/loomd/src/capture.rs:244` — no second writer, no second DB. On `main`, `crates/loom-telemetry/src/` contains **no** token/cost code at all (grep `token|cost|usd` → no matches), so the ledger is new, not a fork of an existing meter. The routing outcome store (`crates/loom-routing/src/outcomes.rs:50-51`, `:78-79`, `:124-125`) persists `est_input_tokens`/`est_output_tokens` for a *decision*, in a different database, and `p4` did not touch it — that is the "one owner" boundary the module doc claims, and it is real.
- **Honest absence in the data model (item 8, first half).** A run whose source reports nothing stores NULL counts, not zero: `crates/loom-telemetry/src/usage.rs:120-145` maps `Option` → NULL; `crates/loom-core/src/types.rs:784-812` `from_counts` returns `UsageSource::Unavailable` with all legs `None` when nothing was reported; `crates/loom-cli/src/commands/telemetry.rs:156-161` `money()` renders `None` as "unavailable", never `$0.0000`; `crates/loom-cli/src/commands/acp_run.rs:161-167` prints "Tokens: not reported by the agent [unavailable]". The provider paths were also fixed: `crates/loom-cli/src/commands/resume.rs:591`/`:653` no longer `.unwrap_or(0)`, and `crates/loom-cli/src/commands/knowledge.rs:508-524`/`:551-570` return `None` when the provider omits `usage`.
- **Test honesty (item 10).** Every test that touches the filesystem uses `tempfile::tempdir()` (`crates/loom-telemetry/src/usage.rs:312`, `crates/loom-cli/src/commands/telemetry.rs:221`) — the `AGENTS.md` convention holds. The tests are **not** tautological:
  - `crates/loom-telemetry/src/usage.rs:344-364` `unavailable_run_stores_no_counts` asserts `totals.cost_usd == None`, `row.source == Unavailable`, `row.input_tokens == None` — it would **fail** if a placeholder zero or the old constant came back.
  - `crates/loom-routing/tests/routing_test.rs:33-54` `test_cost_estimate_grows_with_the_prompt` asserts a 4400-char prompt costs more than a 15-char one *and* `assert_ne!(small.cost_estimate.input_tokens_estimated, 1000, "the old fixed 1000-token constant must be gone")`.
  - `crates/loom-routing/tests/routing_test.rs:56-74` `test_cost_estimate_is_computed_from_the_token_count` asserts exactly 100 in / 25 out for a 400-char prompt and recomputes the cost from the catalog to 1e-12.
  - `crates/loom-sessions/src/acp_protocol.rs:707-729` — two tests against the real fixture server (`crates/loom-sessions/tests/fixtures/fake_acp_server.py`, which gained a `usage` mode used at `acp_protocol.rs:721`): "no usage ⇒ all legs `None`, method empty" and "turn usage 1200/300, context 4321, cost 0.0123, method `acp:turn-usage`".
  - `crates/loom-cli/src/commands/telemetry.rs:219-279` asserts the measured and forecast legs are separate fields and that the unavailable row's `input_tokens` is `null` in JSON.
- **Scope (item 11).** No new crate, no new external dependency (`Cargo.lock` gains three lines: `loom-cli → loom-telemetry`, `loom-telemetry → rusqlite`, `loom-telemetry → serde`; `rusqlite` and `serde` are already workspace dependencies), and **no new top-level CLI group** — the new `Usage` subcommand lives under the existing `loom telemetry` group (`crates/loom-cli/src/cli.rs:245-248` `Command::Telemetry { action: TelemetryAction }`, `crates/loom-cli/src/commands/telemetry.rs:19-27`). Files touched are all accounting/estimation surfaces: `loom-core/src/types.rs`, `loom-routing/{src/router.rs,src/catalog.rs,tests/routing_test.rs}`, `loom-telemetry/{src/lib.rs,src/usage.rs,Cargo.toml}`, `loom-sessions/src/acp_protocol.rs` + fixture, `loom-cli/src/commands/{acp_run,knowledge,resume,routing,telemetry,workflow}.rs` + `Cargo.toml`, `loom-workflow/src/context_injector.rs`, `loomd/src/capture.rs`, `crates/loom-integration-tests/tests/routing_integration.rs`. No behaviour change outside the metering/estimation surface.

## B.1 Findings — `p4/usage-metering`

### P4-1 (High) — The reporting layer still prints `Measured tokens: 0 in / 0 out` for a run that reported no token count

**Evidence.**

- `crates/loom-core/src/types.rs:791-800` `from_counts` marks a record `UsageSource::Measured` if **any** leg is present — including a context-window size or a cumulative cost, with `input_tokens`/`output_tokens` left `None`.
- `crates/loom-telemetry/src/usage.rs:190-191` aggregates tokens with `SUM(COALESCE(input_tokens, 0))` into `UsageTotals.input_tokens: u64` (`:40`); `by_tool` does the same at `:233-234` into `ToolUsage.input_tokens: u64` (`:62`). `NULL` becomes `0`.
- `crates/loom-cli/src/commands/telemetry.rs:84-88` prints `"  Measured tokens: {} in / {} out"` whenever `measured_runs > 0` — using exactly those COALESCEd sums. The per-tool line at `:113-120` prints `tokens={}/{}` from the same sums.

**Proof (throwaway test, since removed).** I recorded a run whose source reported only the **stable ACP v1** `usage_update` notification (`used = 4321`, `cost.amount = 0.0123`) — no per-turn count, which is the normal ACP v1 shape. Result: `source = Measured`, `input_tokens = None`, and `totals = { runs: 1, measured_runs: 1, unavailable_runs: 0, input_tokens: 0, output_tokens: 0, cost_usd: Some(0.0123) }`. The CLI therefore prints:

```
  Runs: 1 (1 measured, 0 unavailable)
  Measured tokens: 0 in / 0 out
  Measured cost:   $0.0123
```

**Why it matters.** This is exactly the failure the card's rule 4 forbids ("do not silently store the constant"; zero is a false claim that the call was free). The *row* is honest — the columns are NULL and `cost_usd` is real — but the *aggregate* manufactures a 0 and labels it "Measured". A user reading the ledger is told the agent consumed zero tokens, which reads as a measurement. The same COALESCE also makes a per-tool line read `tokens=0/0` for a tool whose runs all reported nothing.

**Required fix.**
1. Keep token sums nullable end to end: aggregate with `SUM(input_tokens)` (no COALESCE) and keep `UsageTotals.input_tokens` / `ToolUsage.input_tokens` as `Option<u64>`, so "no run reported this leg" stays `None` and renders as "unavailable" via the existing `money()`-style helper.
2. In `crates/loom-cli/src/commands/telemetry.rs:84-88`, print "Measured tokens: unavailable (no run reported a token count)" when the measured token sum is `None`, even when `measured_runs > 0`.
3. Consider a second provenance bit — `measured_runs` currently conflates "reported a token count" with "reported a context size or a cost" (`types.rs:791-800`).

### P4-2 (High) — The branch states its scope nowhere in the tree; the now-vs-pillar-4 split exists only in the commit message

**Evidence.**

- `git diff main..p4/usage-metering --stat -- '*.md'` is **empty** — no `CHANGELOG.md` entry, no `ROADMAP.md` entry, no `docs/` note.
- Grepping the branch's new/changed code for `pillar` — `crates/loom-telemetry/`, `crates/loom-core/src/types.rs`, `crates/loom-cli/src/commands/telemetry.rs` — returns **nothing**.
- The only place the split is written down is the body of commit `c5e596a`: *"Pillar 4 groundwork. … Measured paths: resume (provider usage), acp (protocol usage), knowledge extraction (provider usage). Unavailable-but-recorded: workflow dispatch (an external agent reports no count)."* A commit message is not something a user of the code ever sees.

**Why it matters.** The card's hard rule 7 says: *"Pillar 4 is POST-1.0 and gated on pillars 1-3. This card must therefore be scoped so it does not block 1.0. Say in your plan which part is safe to land now … and which part is the pillar-4 work."* `ROADMAP.md:46` states pillar 4 is *"Commercialization (post-1.0, gated on 1–3)"* and `ROADMAP.md:54` promises *"Per-run, per-tool, per-token accounting from the routing/telemetry crates"*. A user of `loom telemetry usage` today sees a ledger that records four tool paths (`resume`, `acp`, `workflow`, `knowledge-extract`) and **silently omits** `loom exec` (`crates/loom-cli/src/commands/exec.rs` has no `UsageMeter` call at all), the TUI dispatch path (`crates/loom-tui/src/dispatch.rs`), and the TUI Cost tab (`crates/loom-tui/src/ui/tabs/cost.rs:92`, `:118-123`). Nothing in the product says so.

**Required fix (specific):**
1. Add a `ROADMAP.md` pillar-4 bullet stating what is measurable now vs what remains for pillar 4, in the same style as the p3 branch's pillar-3 bullet (ROADMAP.md:42).
2. Extend the module doc at `crates/loom-telemetry/src/usage.rs:1-18` with the same split, and name the paths that are **not** yet metered (`loom exec`, TUI dispatch, TUI Cost tab, multi-turn session cost).
3. Add a `CHANGELOG.md` Unreleased entry — the branch is a user-visible behaviour change (`loom telemetry usage` exists now) and currently ships with no changelog line at all.

### P4-3 (Medium–Low) — The ledger is append-only and has no supported way to be cleared

**Evidence.** `loom telemetry reset` (`crates/loom-cli/src/commands/telemetry.rs:52-61`) resets the *daemon's* metrics via the `telemetry.reset` IPC command, whose handler (`crates/loomd/src/ipc_handler.rs:407-410`) calls `self.telemetry.metrics().reset()` — the in-memory counter collector. It never touches `{loom_dir}/telemetry/usage.db`. There is no other code path that deletes or truncates the `usage_runs` table (`crates/loom-telemetry/src/usage.rs:94-116` creates the schema; `:120-303` only insert/select). A user who wants a clean slate has no supported way to get one, and the two "telemetry" surfaces silently diverge in what "reset" means.

**Required fix:** either add a `usage` reset to the existing `loom telemetry` group (delete rows from `usage_runs`) or state in the `loom telemetry` help text that `reset` covers daemon counters only, not the usage ledger.

### P4-4 (Medium–Low) — The ACP turn-usage method label says "turn" but the upstream field is session-cumulative

**Evidence.** `crates/loom-sessions/src/acp_protocol.rs:218-235` reads `usage.get("inputTokens")` / `usage.get("outputTokens")` and the surrounding comment (`:216-218`) calls it *"the UNSTABLE per-turn `usage` object"*, and the method string recorded is `"acp:turn-usage"` (`:502-504`).

Upstream ACP (schema `schema/v1/schema.unstable.json` → `$defs.Usage`, fetched from `github.com/agentclientprotocol/agent-client-protocol`) defines those fields as **cumulative across the session**, not per-turn:

- `inputTokens`: *"Total input tokens across all turns."*
- `outputTokens`: *"Total output tokens across all turns."*
- `totalTokens`: *"Sum of all token types across session."*

The stable v1 schema confirms the split: `UsageUpdate` = `{used, size, cost}` (context window + **cumulative session cost**), while `PromptResponse.usage` exists only in the **unstable** schema. `p4` gets this right for the stable half — `crates/loom-sessions/src/acp_protocol.rs:197-214` records `used` as `context_tokens` only and never promotes it to a turn count, and the comment says so — but the unstable half is stored as a per-run count and the ledger sums it across runs (`crates/loom-telemetry/src/usage.rs:190-191`).

**Why it matters.** Today it is correct by accident: one Loom ACP run is one `session/prompt` turn, so "cumulative across all turns" equals that turn. The label and the schema disagree, however, so the first time a Loom run spans more than one turn — or the same session is metered twice — the ledger will double-count, and nothing in the code or the method string says that is not allowed. The same applies to `Cost.amount`, which upstream defines as *"Total cumulative cost for session"*: `crates/loom-sessions/src/acp_protocol.rs:207` stores it in `cost_usd`, and `usage.rs:192` sums `cost_usd` across rows.

**Required fix:** rename the method to match the field's actual semantics (e.g. `acp:session-usage` / `acp:session-cost`) and either store these in cumulative-only columns or document in `crates/loom-telemetry/src/usage.rs:1-18` that the ACP legs are session-scoped and are therefore summed only within a session, never across sessions.

### P4-5 (Low–Medium) — The branch breaks `cargo fmt`, including three visibly mangled lines, and CI will not catch it

**Evidence.** `cargo fmt --all -- --check`:

- On `main`: **CLEAN**.
- On the merged worktree: **FAILS**, in 6 p4 files — `crates/loom-cli/src/commands/acp_run.rs:164`, `crates/loom-cli/src/commands/knowledge.rs:52`, `crates/loom-cli/src/commands/telemetry.rs:94`/`:113`/`:161`/`:260`, `crates/loom-cli/src/commands/workflow.rs:618`, `crates/loom-routing/tests/routing_test.rs:68`/`:86`, `crates/loom-telemetry/src/usage.rs:276`/`:320`.

Three of those are not "rustfmt would reformat this" but mangled edits that survived review:

- `crates/loom-cli/src/commands/acp_run.rs:167` — `}        }` (a brace and eight spaces on one line).
- `crates/loom-cli/src/commands/telemetry.rs:164` — `fn usage_json(loom_dir: &str, limit: usize) -> Result<serde_json::Value> {    let meter = UsageMeter::new(loom_dir)?;` (statement on the signature line).
- `crates/loom-routing/tests/routing_test.rs:68` — `.expect("the decided model must be in the catalog");    let expected = (100.0 / 1000.0) * model.input_cost_per_1k`.

There is **no `cargo fmt` job in CI** (`.github/workflows/ci.yml` runs `cargo test --workspace --exclude loom-tui --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, and `cargo test --workspace --doc --all-features`), so this will not break the build — but `main` is fmt-clean and this branch is the only thing in the tree that is not.

**Required fix:** run `cargo fmt --all` on the branch (one command, no logic change).

### P4-6 (Low) — The daemon and TUI surfaces do not carry the usage, so the flagship consumer cannot see it

**Evidence.**

- The CLI path returns usage in JSON: `crates/loom-cli/src/commands/acp_run.rs:171-180` adds a `"usage"` key.
- The daemon path does not: `crates/loomd/src/capture.rs:258-265` returns `{dispatched, session_id, agent_session_id, stop_reason, tool_calls, run_id}` — no usage — even though the daemon *does* meter the run at `:244-256`. The TUI dispatches through the daemon (`crates/loom-tui/src/dispatch.rs`), so the TUI path never sees a number.
- The TUI Cost tab is still a placeholder: `crates/loom-tui/src/ui/tabs/cost.rs:92` prints "Per-Task Estimates (from routing catalog):", `:118` "Get estimated cost from routing catalog (placeholder)", and `:119-123` reads `/cost_estimate` with `.as_str()` on what is a JSON **object** — so it always renders "—". The new ledger is not wired into it at all.
- `loom routing outcomes` prints decision rows but not the stored estimate columns: `crates/loom-cli/src/commands/routing.rs:404-424` omits `est_input_tokens`/`est_output_tokens`, which `crates/loom-routing/src/outcomes.rs:78-79`/`:124-125` persist.

**Why it matters.** This is not a violation of the card's write scope (the card asked for `loom telemetry` only), but it is exactly the "what remains" that P4-2 requires the branch to state, and pillar 5's flagship consumer (the review engine) will drive dispatches through the daemon.

**Required fix:** state these as remaining work in the scope note required by P4-2. Optionally add the usage object to the daemon's `acp.dispatch` reply at `crates/loomd/src/capture.rs:258-265` — a two-line change that makes the daemon surface match the CLI surface.

### P4-7 (Low) — A second, disagreeing `chars/4` estimator now coexists with the ledger's labelled one

**Evidence.** `crates/loom-core/src/types.rs:673-678` `estimate_tokens(chars)` — `ceil(chars/4)`, the one the ledger's `"chars/4"` method label names. `crates/loom-sessions/src/compression.rs:69-74` `estimate_tokens(messages)` — `(len + 4) / 4` floored, per message, summed. Both are "a token estimate", they disagree, and only the first is covered by the `method: "chars/4"` provenance string.

**Required fix (optional):** mention the compression estimator in the `crates/loom-telemetry/src/usage.rs:1-18` module doc, or have it call the shared `loom_core::estimate_tokens` so the two agree.

### P4-8 (Low) — The resume path still hands the adapter a placeholder cost estimate

**Evidence.** `crates/loom-cli/src/commands/resume.rs:379` builds the `RoutingDecision` it passes to `adapter.apply_routing_decision` with `cost_estimate: loom_core::CostEstimate::default()` — i.e. `source = Unavailable`, `method = ""`, cost 0.0 — while the same command now computes a real estimate elsewhere. Pre-existing on `main` (`main:crates/loom-cli/src/commands/resume.rs:344`), so not a regression, but after this branch "unavailable" is a *statement about the data*, and here it is an artefact of a default value.

**Required fix (optional):** build the decision's `cost_estimate` with `CostEstimate::estimated(...)` so the value the adapter sees matches the value the user is shown.

---

# GENERAL — scope, dependencies, build

## Scope (item 11)

| Check | `p3/protocol-spec` | `p4/usage-metering` |
|---|---|---|
| New crate | none | none |
| New external dependency | none | none (`rusqlite`, `serde` were already workspace deps; `Cargo.lock` +3 lines) |
| New top-level CLI group | none | none (new `Usage` subcommand under the existing `loom telemetry`) |
| Behaviour change outside scope | none — 4 documentation files only | none found; all 20 files are accounting/estimation surfaces |
| Files touched | `docs/protocol/adapter-and-acp-protocol.md`, `CHANGELOG.md`, `README.md`, `ROADMAP.md` | `loom-core/src/types.rs`, `loom-routing/{src/router.rs,src/catalog.rs,tests/routing_test.rs}`, `loom-telemetry/{Cargo.toml,src/lib.rs,src/usage.rs}`, `loom-sessions/src/acp_protocol.rs`, `crates/loom-sessions/tests/fixtures/fake_acp_server.py`, `loom-cli/{Cargo.toml,src/commands/{acp_run,knowledge,resume,routing,telemetry,workflow}.rs}`, `loom-workflow/src/context_injector.rs`, `loomd/src/capture.rs`, `crates/loom-integration-tests/tests/routing_integration.rs`, `Cargo.lock` |

## Build (item 12)

Worktree: `git worktree add .worktrees/p34-review -b p3p4/review main`, then `p3/protocol-spec` and `p4/usage-metering` merged in (clean, no conflicts, HEAD `a7d7d85`). `.gitignore` was not touched.

| Command | Where | Result |
|---|---|---|
| `cargo test --workspace` | merged worktree | **exit 0** |
| `cargo test --workspace --exclude loom-tui --all-features` (CI style) | merged worktree | **exit 0** — 85 test binaries ok, 0 failed |
| `cargo test --workspace --exclude loom-tui --all-features` (CI style) | `main` | **exit 0** — 85 test binaries ok, 0 failed → **same result, no regression introduced by either branch** |
| `cargo check --workspace` | merged worktree | **exit 0** |
| `cargo clippy --workspace` | merged worktree | **exit 0** |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (CI style) | merged worktree | **exit 0** |
| `cargo fmt --all -- --check` | `main` | **CLEAN** |
| `cargo fmt --all -- --check` | merged worktree | **FAILS** — 6 p4 files (see P4-5) |
| `cargo test -p loom-routing -p loom-telemetry -p loom-cli` | merged worktree | covered by the workspace run above; all pass |

**Pre-existing failures:** none. `main` and the merged tree both report 85 passing test binaries and zero failures under the CI command, so nothing in either branch is masked by a pre-existing break.

---

# VERDICTS

## `p3/protocol-spec` — **FIX FIRST**

The approach is right and the execution is careful: the anchors are all true at the spec's own base, the compatibility policy is complete, both worked examples were verified against the real parser and resolver, and the registry gap is stated honestly rather than papered over. Nothing here needs reworking.

Required before merge:

1. **P3-1** — add the ACP **dispatch server** to the spec (`crates/loom-sessions/src/acp_server.rs`, the `acp.status`/`acp.start`/`acp.stop` IPC commands, the `loom acp status|start|stop|show|agents|registry|run-status` CLI actions, `acp_handshake.rs`, `acp_discovery.rs`, `agent_provider.rs`), with `file:line` anchors. A spec titled "ACP dispatch" that omits the dispatch server is not a complete public contract.
2. **P3-2** — re-anchor the ~49 body anchors and ~47 index entries that `p4` invalidates (`crates/loom-sessions/src/acp_protocol.rs`, `crates/loom-core/src/types.rs`, `crates/loom-cli/src/commands/acp_run.rs`, `crates/loom-sessions/tests/fixtures/fake_acp_server.py`) once p3 and p4 ship together; or anchor by symbol name so the index survives future insertions.
3. **P3-3** — document the `apply_routing_decision` environment-variable convention (`<TOOL>_MODEL`, `LOOM_ROUTING_PROVIDER`, the read-back rule, and `crush`'s HTTP deviation).

Recommended (cheap, same PR): **P3-4** (complete the §5 index), **P3-5** (list the four `adapter_for` aliases), **P3-6** (fix the `acp_registry/` step in §2.8), **P3-7** (keep one example-parsing test in-tree).

## `p4/usage-metering` — **FIX FIRST**

The core design is right: the constant is genuinely gone (in *both* places it lived), there is exactly one meter with a defensible ownership boundary, unavailable is modelled as unavailable, and the tests would fail if the old constant came back. The approach does not need rework.

Required before merge:

1. **P4-1** — stop printing `Measured tokens: 0 in / 0 out`. Make the token aggregates nullable (`SUM(input_tokens)` without `COALESCE`, `UsageTotals.input_tokens`/`ToolUsage.input_tokens` as `Option<u64>`) and render "unavailable" when no run reported a token count, even if `measured_runs > 0`. Touches `crates/loom-telemetry/src/usage.rs:40`, `:62`, `:190-191`, `:233-234` and `crates/loom-cli/src/commands/telemetry.rs:84-88`, `:113-120`.
2. **P4-2** — state the scope in the tree, not only in the commit message: a `ROADMAP.md` pillar-4 bullet, an extension of the `crates/loom-telemetry/src/usage.rs:1-18` module doc, and a `CHANGELOG.md` entry — saying what is measurable now (`resume`, `acp`, `workflow` [unavailable], `knowledge-extract`), what is **not** yet metered (`loom exec`, TUI dispatch, TUI Cost tab, multi-turn session cost), and that pillar 4 is post-1.0 and gated on pillars 1–3.
3. **P4-5** — run `cargo fmt --all` (6 files, including the three mangled lines at `acp_run.rs:167`, `telemetry.rs:164`, `routing_test.rs:68`).

Recommended: **P4-3** (a way to clear the usage ledger), **P4-4** (rename/re-scope the `acp:turn-usage` method to match upstream's session-cumulative semantics, or document the constraint), **P4-6** (add `usage` to the daemon's `acp.dispatch` reply at `crates/loomd/src/capture.rs:258-265`), **P4-7**/**P4-8**.

---

# Appendix — how the anchor checks were run

- **Body anchors:** a Python script extracted every `path:line` token from the body of `p3/protocol-spec:docs/protocol/adapter-and-acp-protocol.md` (236 explicit anchors over 22 files), then resolved each against `git show main:<file>` (the spec's base) and against the merged worktree, comparing the resolved line's content against the construct the surrounding sentence names.
- **Index anchors:** the same treatment for the §5 index (219 parsed entries, 146 with an explicit line number; the rest are path-grouped or directory-level).
- **Schema anchors:** `acp_registry/agent.schema.json`, `registry.schema.json`, `CONTRIBUTING.md`, `AUTHENTICATION.md` cannot be read from git (the gitlink has no blobs in this repository), so they were checked against the on-disk checkout of the pinned commit `2edbfd245997eae3b19c0b6cba46f86826dc5f44` — whose `origin` is `https://github.com/agentclientprotocol/registry.git`, confirming the URL the spec prints.
- **Worked examples:** exercised through the real code paths — `toml::from_str::<loom_generic_adapter::config::ToolToml>` for §1.5, `loom_sessions::acp_launch::launch_from_entry` for §2.8 — in throwaway tests under `tempfile::tempdir()`. All three probe files were deleted; `git status` in the worktree is clean.
- **Upstream ACP:** `schema/v1/schema.json` and `schema/v1/schema.unstable.json` fetched from `github.com/agentclientprotocol/agent-client-protocol` to confirm what `usage_update` and the unstable per-turn `usage` actually carry.

**No fixes, no merges, no pushes were made. This report is committed on `p3p4/review` only.**
