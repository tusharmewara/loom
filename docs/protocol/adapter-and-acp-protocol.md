# Loom Public Protocol — Adapter Contract & ACP Dispatch

**Date:** 2026-10-21
**Status:** Published (spec v1.0.0)
**Spec version:** 1.0.0
**Describes:** Adapter contract v1, ACP protocol v1

---

## Overview

Loom has two public extension points. A third party plugs into either of them
without forking the Loom repository.

| Extension point | What you write | What Loom reads it from | Spec section |
|---|---|---|---|
| **Adapter contract** | One TOML file, `~/.loom/tools/<tool>.toml` | `loom-generic-adapter` | [§1](#1-the-adapter-contract) |
| **ACP dispatch** | A JSON-RPC 2.0 agent on stdio, plus a registry entry | `loom-sessions` / `loomd` | [§2](#2-the-acp-protocol) |

This document is **derived from the implementation**. Every normative statement
carries a `file:line` anchor into the Rust source so a reader can verify it and
so a future change can tell what this document failed to cover. Where the code
and an older promise disagree, **the code wins** — see
[§4 Known inconsistencies](#4-known-inconsistencies-code-vs-earlier-promises).

---

## 0. Versioning

| Version | Value | Where it lives |
|---|---|---|
| This document | `1.0.0` | This file. Bump on any normative change. |
| ACP protocol | `1` | `ACP_PROTOCOL_VERSION` — `crates/loom-sessions/src/acp_protocol.rs:20` |
| Adapter contract | `1` (documented here) | **No runtime constant exists.** See below. |
| Loom ecosystem | `2.0.0` | `ECOSYSTEM_VERSION` — `crates/loom-core/src/constants.rs:3` |

The ACP version is enforced at runtime: Loom sends `protocolVersion` in
`initialize` (`crates/loom-sessions/src/acp_protocol.rs:354`) and rejects any
response whose `protocolVersion` is not exactly `1`
(`crates/loom-sessions/src/acp_protocol.rs:368`).

**The adapter contract has no runtime version constant.** A grep across
`crates/` finds no `ADAPTER_VERSION` / `CONTRACT_VERSION` / `TOOL_TOML_VERSION`
constant. The contract version is therefore *this document's* version: the TOML
shape is versioned by the spec, and a future incompatible change to the TOML
shape must bump the spec version and be announced in `CHANGELOG.md`. Loom's
semantic version (`0.9.6`, `crates/loom-core/src/lib.rs` workspace
`Cargo.toml:38`) is **not** the contract version.

---

## 1. The Adapter contract

### 1.1 The `LoomToolAdapter` trait

`pub trait LoomToolAdapter: Send + Sync` — `crates/loom-adapters/src/lib.rs:36`.

Implementing this trait is the **Rust-native** path: it is how Loom's 12 built-in
presets are written (`crates/loom-adapters/src/lib.rs:334` lists them: `jcode`,
`crush`, `claude-code`, `opencode`, `goose`, `codex`, `cursor`, `gemini-cli`,
`aider`, `kiro`, `juno`, `github-copilot`). A third party that only needs
configuration, not code, should use the TOML path in [§1.4](#14-registering-a-tool)
instead — it needs no Rust.

**Required methods** (no default body):

| Method | Anchor | Contract |
|---|---|---|
| `tool_name(&self) -> &'static str` | `crates/loom-adapters/src/lib.rs:37` | The registration file stem: `~/.loom/tools/<tool_name>.toml` (`crates/loom-adapters/src/lib.rs:299`). |
| `default_toml(&self, home: &str) -> ToolToml` | `crates/loom-adapters/src/lib.rs:38` | The full TOML document for this tool. `home` is the tilde-expanded user home (`crates/loom-core/src/lib.rs:15`). |
| `apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()>` | `crates/loom-adapters/src/lib.rs:39` | Apply a routing decision to the tool's environment. `RoutingDecision` carries `recommended_provider` / `recommended_model` (`crates/loom-core/src/types.rs:575`). |
| `sync_from_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport>` | `crates/loom-adapters/src/lib.rs:40` | Import the tool's native resources into the Loom ecosystem. `SyncReport` (`crates/loom-adapters/src/lib.rs:307`) reports `skills_imported`, `skills_synced`, `mcp_servers_imported`, `mcp_servers_synced`, `providers_imported`, `notes`. |

**Defaulted methods** (override only if you need to):

| Method | Anchor | Default behaviour |
|---|---|---|
| `sync_to_native(&self, loom_dir, home) -> Result<SyncReport>` | `crates/loom-adapters/src/lib.rs:46` | Pushes shared MCP servers into the tool's native config (`sync_mcp_to_native`, `crates/loom-adapters/src/lib.rs:169`) and symlinks shared skills into the tool's skills dir (`sync_skills_to_native`, `crates/loom-adapters/src/lib.rs:227`). |
| `lsp_config(&self, home) -> Option<LspSection>` | `crates/loom-adapters/src/lib.rs:92` | Returns the `[lsp]` section of `default_toml`. |
| `lsp_servers(&self, home) -> Vec<LspServerInfo>` | `crates/loom-adapters/src/lib.rs:97` | Projects `lsp_config` into server infos. |
| `resolve_lsp_server(&self, extension, home) -> Option<LspServerInfo>` | `crates/loom-adapters/src/lib.rs:114` | First server whose `file_extensions` contains the extension. |
| `debug_config(&self, home) -> Option<DebugSection>` | `crates/loom-adapters/src/lib.rs:121` | Returns the `[debug]` section of `default_toml`. |
| `debug_adapters(&self, home) -> Vec<DebugAdapterInfo>` | `crates/loom-adapters/src/lib.rs:126` | Projects `debug_config` into adapter infos. |
| `resolve_debug_adapter(&self, language, home) -> Option<DebugAdapterInfo>` | `crates/loom-adapters/src/lib.rs:143` | First adapter whose `language` matches. |
| `apply_hashline_edit(&self, file_path, start, end, new_content, context_lines) -> Result<Option<String>>` | `crates/loom-adapters/src/lib.rs:151` | Reads the file, applies the edit with stale detection, returns the new content (the caller writes it). |

**Rust-side extension points.** Two core traits exist for LSP and DAP but are
**not** implemented by `LoomToolAdapter` itself: `LspProvider`
(`crates/loom-core/src/types.rs:960`) and `DebugProvider`
(`crates/loom-core/src/types.rs:984`). `loom lsp exec <tool> …` resolves the
tool through `adapter_for` and then `lsp_servers`
(`crates/loom-cli/src/commands/lsp.rs:89`).

### 1.2 The tool TOML shape

`pub struct ToolToml` — `crates/loom-generic-adapter/src/config.rs:34`. It is
parsed by `toml::from_str` (`crates/loom-generic-adapter/src/lib.rs:42`), so it
follows TOML semantics with serde.

**Required:**

| Section | Anchor | Fields |
|---|---|---|
| `[tool]` | `crates/loom-generic-adapter/src/config.rs:36` | `name: String`, `version: String` — both required (`crates/loom-generic-adapter/src/config.rs:68`). A TOML without `[tool]` fails to parse (`missing field 'tool'`). |

**Optional** (each is `Option<Section>` with `#[serde(default)]`, so the whole
section may be omitted):

| Section | Anchor | Section struct | Fields (defaults) |
|---|---|---|---|
| `[skills]` | `crates/loom-generic-adapter/src/config.rs:39` | `SkillsSection` (`:77`) | `native_dir: Option<String>`, `import_on_register: bool` (false), `shared_dir: Option<String>`, `extension: String` (`"md"`, `:90`) |
| `[mcp]` | `crates/loom-generic-adapter/src/config.rs:42` | `McpSection` (`:96`) | `config_path: Option<String>`, `shared_servers: Vec<String>` (empty), `import_on_register: bool` (false), `import_tags: Vec<String>` (empty) |
| `[auth]` | `crates/loom-generic-adapter/src/config.rs:45` | `AuthSection` (`:112`) | `store_path: Option<String>`, `providers: Vec<String>` (empty), `import_on_register: bool` (false) |
| `[hooks]` | `crates/loom-generic-adapter/src/config.rs:48` | `HooksSection` (`:125`) | `scripts_dir: Option<String>`, `register_on_start: bool` (false), `default_timeout_seconds: u64` (`30`, `:136`) |
| `[routing]` | `crates/loom-generic-adapter/src/config.rs:51` | `RoutingSection` (`:142`) | `default_tier: String` (`"mid"`, `:153`), `catalog_path: Option<String>`, `strategy: String` (`"balanced"`, `:157`) |
| `[sessions]` | `crates/loom-generic-adapter/src/config.rs:54` | `SessionsSection` (`:163`) | `store_path: Option<String>`, `auto_export: bool` (false), `max_shared_sessions: usize` (`50`, `:174`) |
| `[lsp]` | `crates/loom-generic-adapter/src/config.rs:57` | `LspSection` (`:183`) | `servers: Vec<LspServerConfig>` (empty), `root_uri: Option<String>`, `file_extensions: Vec<String>` (empty). `LspServerConfig` (`:196`): `language`, `command` (both required), `args` (empty), `file_extensions` (empty). |
| `[debug]` | `crates/loom-generic-adapter/src/config.rs:60` | `DebugSection` (`:214`) | `adapters: Vec<DebugAdapterConfig>` (empty), `cwd: Option<String>`. `DebugAdapterConfig` (`:224`): `language`, `command` (both required), `args` (empty), `name: Option<String>`. |
| `[launch]` | `crates/loom-generic-adapter/src/config.rs:63` | `LaunchSection` (`:240`) | `runtime: String` (`"process"`, `:263`), `image: Option<String>`, `command: String` (**required**, `:247`), `args: Vec<String>` (empty), `env: HashMap<String,String>` (empty), `working_dir: Option<String>`, `network: Option<String>`, `auto_remove: bool` (`true`, `:267`) |

Parsing rules, all verified against the runtime:

- A section's `Option<String>` fields may be omitted — serde maps a missing
  `Option` field to `None`. Omitting `native_dir` from `[skills]` parses.
- **Unknown keys and unknown sections are ignored.** `ToolToml` has no
  `deny_unknown_fields` (`crates/loom-generic-adapter/src/config.rs:33`), so a
  forward-compatible TOML with extra keys still parses.
- Four fields are hard errors, all surfacing as `toml::de::Error`
  `missing field …`: a missing `[tool]`, a missing `[tool].name`, a missing
  `[tool].version`, and a missing `command` in a `[launch]`, `[lsp].servers[]`
  or `[debug].adapters[]` entry. Everything else has a default or is
  optional.

### 1.3 The no-op contract

> "Missing sections produce no-op implementations — the adapter never errors
> on absent sections."
> — `crates/loom-generic-adapter/src/lib.rs:26`

The mechanism is concrete: `TraitBuilder::build_*` returns `Option<…>`, and
`None` means "no implementation for this trait" — not "an error"
(`crates/loom-generic-adapter/src/traits.rs:178`, `:190`, `:199`, `:208`,
`:217`, `:229`).

| Section | Trait implemented | Anchor |
|---|---|---|
| `[skills]` | `SkillProvider` | `crates/loom-generic-adapter/src/lib.rs:19` |
| `[mcp]` | `McpConsumer` | `crates/loom-generic-adapter/src/lib.rs:20` |
| `[auth]` | `AuthProviderTrait` | `crates/loom-generic-adapter/src/lib.rs:21` |
| `[hooks]` | `HookExecutor` | `crates/loom-generic-adapter/src/lib.rs:22` |
| `[routing]` | `RoutingTarget` | `crates/loom-generic-adapter/src/lib.rs:23` |
| `[sessions]` | `SessionExportable` | `crates/loom-generic-adapter/src/lib.rs:24` |

When the section *is* present, the generic implementation is still deliberately
thin: `GenericMcpConsumer::add_mcp_server` / `remove_mcp_server` are no-ops
(`crates/loom-generic-adapter/src/traits.rs:76`, `:83`),
`GenericHookExecutor::execute_hook` is a no-op
(`crates/loom-generic-adapter/src/traits.rs:116`),
`GenericAuthProvider::discover_providers` returns empty
(`crates/loom-generic-adapter/src/traits.rs:95`), and
`GenericSessionExportable::export_session` errors with "no store"
(`crates/loom-generic-adapter/src/traits.rs:149`). **A TOML registration
declares integration points; it does not by itself move data.** The data-moving
paths are the preset adapters' `sync_from_native` / `sync_to_native` and the
daemon's `register_tool` handler
(`crates/loomd/src/ipc_handler.rs:388`).

`present_features()` / `absent_features()` report only these six features
(`crates/loom-generic-adapter/src/lib.rs:102`, `:126`). The `[lsp]`, `[debug]`
and `[launch]` sections are **not** traits and never appear in those lists.

`[skills]` with no `native_dir` falls back to `{loom_dir}/skills`
(`crates/loom-generic-adapter/src/traits.rs:184`).

`[routing].default_tier` maps `"flagship"` → `ModelTier::Flagship`,
`"slm"` → `ModelTier::Slm`, and **anything else** → `ModelTier::Mid`
(`crates/loom-generic-adapter/src/traits.rs:219`).

`[launch]` is the only section the generic path actually executes. It is
projected into `loom_runtime::LaunchConfig` by
`GenericAdapter::launch_config` (`crates/loom-generic-adapter/src/lib.rs:156`)
and consumed by `loom exec`
(`crates/loom-cli/src/commands/exec.rs:714`), workflow dispatch
(`crates/loom-workflow/src/dispatch.rs:31`), the TUI
(`crates/loom-tui/src/dispatch.rs:604`) and the daemon's ACP launcher
(`crates/loomd/src/capture.rs:94`).

### 1.4 Registering a tool

The ecosystem directory is `~/.loom`, or `$LOOM_AGENTS_DIR` when set
(`loom_core::loom_dir()`, `crates/loom-core/src/lib.rs:25`; directory name
constant `AGENTS_DIR = ".loom"`, `crates/loom-core/src/constants.rs:4`). Tool
files live in `{loom_dir}/tools/` (`TOOLS_DIR`, `crates/loom-core/src/constants.rs:10`).

**Option A — `loom thread <tool>`** (`crates/loom-cli/src/cli.rs:61`):

- For a **preset** tool (one of the 12 in `supported_tools()`), Loom writes the
  preset's `default_toml` to `{loom_dir}/tools/<tool>.toml` via
  `write_tool_registration` (`crates/loom-adapters/src/lib.rs:292`) and runs
  `sync_from_native` (`crates/loom-cli/src/commands/thread.rs:22`).
- For an **unknown** tool name, Loom writes a minimal registration with
  `[tool]` only and all nine optional sections absent
  (`crates/loom-cli/src/commands/thread.rs:39`) — you then edit that file.
  **This overwrites an existing file of the same name**
  (`crates/loom-cli/src/commands/thread.rs:54`), so for a hand-written tool,
  write the TOML *first* and do not re-run `loom thread` for that name.
- Either way it notifies a running daemon over the `register_tool` IPC command
  (`crates/loom-cli/src/commands/thread.rs:60`). The daemon's own
  `register_tool` handler writes a registration file **only for preset
  tools**; for a non-preset name it reports an empty feature list
  (`crates/loomd/src/ipc_handler.rs:373`, `:397`).

**Option B — write the file yourself.** Create
`{loom_dir}/tools/<tool>.toml` with the shape in [§1.2](#12-the-tool-toml-shape).
Nothing else is required; `GenericAdapter::new` reads it on demand
(`crates/loom-generic-adapter/src/lib.rs:38`).

**Option C — `loom plugin install` / `loom plugin register`** downloads or
copies a tool TOML into `{loom_dir}/tools/` and records a manifest with a
SHA-256 checksum (`crates/loom-core/src/plugin_registry.rs:169`, `:209`;
CLI definitions `crates/loom-cli/src/cli.rs:262`, `:275`).

**Reading a tool file back.** `AdapterDirScanner::scan` reads every `*.toml` in
`{loom_dir}/tools/` (`crates/loom-generic-adapter/src/scanner.rs:27`). A file
that fails to parse is **skipped with a warning**, not fatal
(`crates/loom-generic-adapter/src/scanner.rs:50`). Non-fatal validation
warnings are emitted for an empty `tool.name`, an empty `tool.version`, and a
`[skills]` section with neither `native_dir` nor `import_on_register`
(`crates/loom-generic-adapter/src/scanner.rs:68`, `:71`, `:76`).

**One more reader to know about.** `loom-auth` scans the same directory with a
*different* schema: it looks for a top-level `[auth]` or `[provider]` section
whose `provider_id` is set (`crates/loom-auth/src/import.rs:42`, `:73`,
`:127`, `:145`). That schema accepts keys such as `provider_id`, `name`,
`auth_method`, `api_base`, `models_endpoint`, `credential`, `credential_file`,
`auth_store`, `supported_tools`
(`crates/loom-auth/src/import.rs:13`). The `ToolToml` `[auth]` section is a
*different* shape (`store_path` / `providers` / `import_on_register`,
`crates/loom-generic-adapter/src/config.rs:112`) — the two do not overlap.
Use the `loom-auth` keys in a tool file if you want credential auto-import.

### 1.5 Worked example — a complete tool registration

Copy this to `~/.loom/tools/acme.toml` (or `{loom_dir}/tools/acme.toml`). It
parses under the rules in §1.2 and activates the `skills`, `mcp`, `auth`,
`hooks`, `routing` and `sessions` traits.

```toml
# ~/.loom/tools/acme.toml
[tool]
name = "acme"
version = "1.0.0"

[skills]
native_dir = "~/.acme/skills"   # optional; defaults to {loom_dir}/skills
import_on_register = true
extension = "md"

[mcp]
config_path = "~/.acme/mcp.json"
shared_servers = ["filesystem"]
import_on_register = true

[auth]
store_path = "~/.acme/auth.json"
providers = ["anthropic"]
import_on_register = true

[hooks]
scripts_dir = "~/.acme/hooks"
register_on_start = true
default_timeout_seconds = 30

[routing]
default_tier = "mid"
strategy = "balanced"

[sessions]
store_path = "~/.acme/sessions"
auto_export = true
max_shared_sessions = 50

[launch]
runtime = "process"
command = "acme"
args = ["--acp"]
```

That file **is** the registration — there is no separate command to run. Copy
it into place and Loom picks it up on the next read.

```bash
mkdir -p ~/.loom/tools
cp acme.toml ~/.loom/tools/acme.toml
```

Do **not** run `loom thread acme` for a hand-written tool: for a name that is
not one of the 12 presets it rewrites the file with a minimal `[tool]`-only
version and drops every section above
(`crates/loom-cli/src/commands/thread.rs:39`, `:54`). See §4 #18.

To tell a *running daemon* about a non-preset tool, send the `register_tool`
IPC command; it reports the adapter's features without writing anything
(`crates/loomd/src/ipc_handler.rs:373`, `:397`). In practice the file alone
is enough: `GenericAdapter::new` reads it on demand
(`crates/loom-generic-adapter/src/lib.rs:38`), and the daemon's ACP
launcher prefers a tool's `[launch]` section over everything else
(`crates/loomd/src/capture.rs:94`). The CLI's `loom acp run` does **not**
consult the tool TOML — it resolves the agent through `config/agents.toml`
instead (see [§2.6](#26-registering-an-acp-agent)).

---

## 2. The ACP protocol

Loom speaks the **Agent Client Protocol v1** as a JSON-RPC 2.0 client over an
agent's stdio. The client lives in `crates/loom-sessions/src/acp_protocol.rs`;
the launch/registration logic in `crates/loom-sessions/src/acp_launch.rs`.

### 2.1 Transport

- **Framing:** newline-delimited JSON-RPC 2.0. One JSON object per line.
  Requests are written to the agent's **stdin**; the agent writes to **stdout**
  (`crates/loom-sessions/src/acp_protocol.rs:208`).
- **Envelope:** Loom's requests are built by
  `build_request(id, method, params)` → `{"jsonrpc":"2.0","id":<n>,"method":<s>,"params":<obj>}`
  (`crates/loom-sessions/src/acp_protocol.rs:99`).
- **Parsing:** `parse_message` classifies a line as `Response`, `Notification`,
  `Request` or `Invalid` (`crates/loom-sessions/src/acp_protocol.rs:52`, `:72`).
  A message must be a JSON object; `id` must be a number or a string
  (`crates/loom-sessions/src/acp_protocol.rs:85`).
- **Ids must be echoed as JSON numbers.** Loom keys its pending-response map by
  `id.to_string()` (`crates/loom-sessions/src/acp_protocol.rs:323`) and the
  reader by the serialized JSON value (`crates/loom-sessions/src/acp_protocol.rs:109`).
  The request ids Loom uses are `0`, `1`, `2` (and `3` for cancel). A response
  with `"id": "0"` (string) does **not** match request `0` and the request will
  time out.
- **stdout must be JSON only.** A line that is not a JSON object classifies as
  `Invalid` and is silently dropped
  (`crates/loom-sessions/src/acp_protocol.rs:73`, `:300`); blank lines are
  skipped (`crates/loom-sessions/src/acp_protocol.rs:268`).
- **stderr is free-form.** It is drained on a dedicated thread and echoed to
  Loom's stderr as `[STDERR] {line}` (`crates/loom-sessions/src/acp_protocol.rs:248`).
  stderr never enters the transcript.
- **Exit code is irrelevant.** The child is killed unconditionally after the
  turn (`crates/loom-sessions/src/acp_protocol.rs:222`); the daemon records exit
  code `0` regardless (`crates/loomd/src/capture.rs:237`). The agent must not
  exit before answering the three requests below.

### 2.2 The request flow

`AcpProtocolClient::run_prompt` (`crates/loom-sessions/src/acp_protocol.rs:206`)
runs exactly three requests.

**① `initialize`** (id `0`) — `crates/loom-sessions/src/acp_protocol.rs:352`

```json
{"jsonrpc":"2.0","id":0,"method":"initialize","params":{
  "protocolVersion": 1,
  "clientCapabilities": {
    "fs": {"readTextFile": false, "writeTextFile": false},
    "terminal": false
  },
  "clientInfo": {"name":"loom","title":"Loom","version":"<loom version>"}
}}
```

The agent must answer with `result.protocolVersion` equal to the **integer**
`1`. Any other value fails the run with *"agent speaks unsupported protocol
version …"* (`crates/loom-sessions/src/acp_protocol.rs:368`).
`agentCapabilities` in the response is **not inspected**.

Loom advertises `fs.readTextFile: false`, `fs.writeTextFile: false` and
`terminal: false` (`crates/loom-sessions/src/acp_protocol.rs:356`). An agent
that depends on the client for filesystem or terminal access will hang.

**② `session/new`** (id `1`) — `crates/loom-sessions/src/acp_protocol.rs:388`

```json
{"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"<dir>","mcpServers":[]}}
```

`mcpServers` is **always empty** — Loom offers no MCP servers over ACP. The
agent must answer with a string `result.sessionId`, otherwise the run fails with
*"session/new response missing sessionId"*
(`crates/loom-sessions/src/acp_protocol.rs:393`).

`cwd` is the CLI's `--cwd` value, or Loom's own current directory when unset
(`crates/loom-sessions/src/acp_protocol.rs:379`).

**③ `session/prompt`** (id `2`) — `crates/loom-sessions/src/acp_protocol.rs:402`

```json
{"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{
  "sessionId":"<id from session/new>",
  "prompt":[{"type":"text","text":"<the prompt>"}]
}}
```

The prompt is always exactly one text block. The agent must answer with a
string `result.stopReason`, otherwise the run fails with *"prompt response
missing stopReason"* (`crates/loom-sessions/src/acp_protocol.rs:427`).

**Cancel on timeout.** If `session/prompt` times out, Loom sends
`session/cancel` with id `3` and `{"sessionId": …}`
(`crates/loom-sessions/src/acp_protocol.rs:413`) and then waits a further 2
seconds for the original response before reporting the original error
(`crates/loom-sessions/src/acp_protocol.rs:420`).

**Timeouts are per request**, not per turn: each of `initialize`,
`session/new` and `session/prompt` gets the full timeout
(`crates/loom-sessions/src/acp_protocol.rs:207`). The minimum is 1 second. The
CLI default is 120 seconds (`crates/loom-cli/src/cli.rs:1816`); the daemon's
`acp.dispatch` default is also 120 (`crates/loomd/src/ipc_handler.rs:550`).

### 2.3 Agent → Loom notifications

`session/update` notifications carry the agent's output
(`crates/loom-sessions/src/acp_protocol.rs:281`). The update object is at
`params.update`, and `params.update.sessionUpdate` selects the kind
(`crates/loom-sessions/src/acp_protocol.rs:127`):

| `sessionUpdate` | Fields Loom reads | Anchor |
|---|---|---|
| `agent_message_chunk` | `params.update.content.text` — appended to the transcript text | `crates/loom-sessions/src/acp_protocol.rs:134` |
| `tool_call` | `toolCallId`, `title`, `status` — recorded as a tool call | `crates/loom-sessions/src/acp_protocol.rs:143` |
| `tool_call_update` | `toolCallId`, `status` — overwrites the status of the matching call | `crates/loom-sessions/src/acp_protocol.rs:161` |

Any other `sessionUpdate` kind is ignored
(`crates/loom-sessions/src/acp_protocol.rs:170`). A `tool_call` whose
`title`/`status` are missing defaults to `"?"` / `"pending"`.

### 2.4 Agent → Loom requests

Only one request method is answered: **`session/request_permission`**
(`crates/loom-sessions/src/acp_protocol.rs:288`). Loom answers on the agent's
stdin with:

```json
{"jsonrpc":"2.0","id":<same id>,"result":{"outcome":{"outcome":"selected","optionId":"<id>"}}}
```

The option is chosen by `permission_outcome`
(`crates/loom-sessions/src/acp_protocol.rs:176`): the **first** option whose
`kind` starts with `"allow"`, otherwise the **first** option, otherwise
`{"outcome":"cancelled"}` (`crates/loom-sessions/src/acp_protocol.rs:196`).

**Every other agent request is left unanswered** and will time out. A
third-party agent must therefore not require `fs/read_text_file`,
`fs/write_text_file` or terminal APIs from the client.

### 2.5 The transcript and the persisted session

The collected outcome is `AcpTranscript` (`crates/loom-sessions/src/acp_protocol.rs:39`):
`session_id`, `stop_reason`, `agent_text` (concatenated chunks) and
`tool_calls` (each rendered as `"{title} [{status}]"`,
`crates/loom-sessions/src/acp_protocol.rs:440`).

`build_acp_session` (`crates/loom-sessions/src/acp_protocol.rs:451`) turns a
transcript into a Loom session:

- session id `acp-<8 hex>` (`crates/loom-sessions/src/acp_protocol.rs:463`);
- three messages — System, User, Assistant
  (`crates/loom-sessions/src/acp_protocol.rs:480`);
- assistant text = agent text (or `"(agent produced no text output)"`), then a
  `Tools:` list, then `[stop reason: …]`
  (`crates/loom-sessions/src/acp_protocol.rs:467`);
- manifest `source_tool = "loom-acp/{agent}"`, `model_provider = {agent}`,
  tags `["acp", "acp-protocol", "agent:{agent}"]`, topic = prompt truncated to
  80 characters (`crates/loom-sessions/src/acp_protocol.rs:512`, `:523`, `:529`,
  `:464`).

The session is written under `{loom_dir}/sessions/{session_id}/` as
`manifest.json` + `messages.jsonl` (`crates/loom-sessions/src/store.rs:38`),
then indexed into cross-session memory and the knowledge graph
(`crates/loom-cli/src/commands/acp_run.rs:126`).

`loom acp run --json` prints
`{session_id, agent_session_id, stop_reason, tool_calls}`
(`crates/loom-cli/src/commands/acp_run.rs:142`).

### 2.6 Registering an ACP agent

#### `config/agents.toml`

The local agent registry is `{loom_dir}/config/agents.toml`
(`crates/loom-sessions/src/acp_launch.rs:263`). It is **not** created by
`loom init` — only `routing.toml`, `mcp.toml`, `hooks.toml` and `acp.toml`
are (`crates/loom-cli/src/commands/init.rs:201`).

There is **no Rust struct** for this file: it is read as a generic
`toml::Table` and each agent id is a top-level table
(`crates/loom-sessions/src/acp_launch.rs:262`, `:266`).

| Key | Type | Meaning | Anchor |
|---|---|---|---|
| `runner` | string | `"npx"` / `"npm"` → `npx <package> [args]`; `"uvx"` → `uvx <package> [args]`; `"pip"` / `"pipx"` → `pipx run <package> [args]`; `"binary"` → resolve `cmd` on `PATH`; **absent or unknown** → the agent id itself is the program | `crates/loom-sessions/src/acp_launch.rs:271` |
| `package` | string | npm / uv package name | `crates/loom-sessions/src/acp_launch.rs:273` |
| `args` | array of strings | Arguments passed to the program | `crates/loom-sessions/src/acp_launch.rs:274` |
| `env` | table of string→string | Environment for the agent | `crates/loom-sessions/src/acp_launch.rs:275` |
| `cmd` | string | **Only** consulted when `runner = "binary"`; defaults to the agent id; the directory part is stripped (`./pkg/foo` → `foo`) | `crates/loom-sessions/src/acp_launch.rs:296`, `:85` |
| `source` | string | Written by `loom add-acp-agent` as `"acp-registry"`. **Never read** by any Loom code path | `crates/loom-cli/src/commands/tools_acp.rs:56` |

There is **no** `command`, `path`, `cwd` or `timeout` key.

#### `loom add-acp-agent <agent_id>`

CLI definition `crates/loom-cli/src/cli.rs:155`, dispatch
`crates/loom-cli/src/cli.rs:1902`, handler
`crates/loom-cli/src/commands/tools_acp.rs:13`.

- It **requires network access** — it fetches the public registry and has no
  offline fallback (`crates/loom-cli/src/commands/tools_acp.rs:14`, `:17`).
- It validates only that the id exists in the registry
  (`crates/loom-cli/src/commands/tools_acp.rs:22`) and that a distribution
  exists for this host platform (`crates/loom-cli/src/commands/tools_acp.rs:62`).
  It does **not** check that the package is installable, nor that the agent
  actually speaks ACP.
- It writes the entry into `{loom_dir}/config/agents.toml`, preserving other
  entries (`crates/loom-cli/src/commands/tools_acp.rs:47`, `:76`).

The registry URL is hard-coded: `ACP_REGISTRY_URL =
"https://cdn.agentclientprotocol.com/registry"`
(`crates/loom-sessions/src/acp_launch.rs:22`); the registry JSON is fetched from
`{ACP_REGISTRY_URL}/v1/latest/registry.json`
(`crates/loom-sessions/src/acp_client.rs:58`) and parsed as
`{ "agents": [ … ] }` (`crates/loom-sessions/src/acp_client.rs:24`).

**Distribution choice** is deterministic: `npx` first, then `uvx`, then a
`binary` distribution for the host platform
(`crates/loom-sessions/src/acp_launch.rs:142`). Platform keys are
`darwin-aarch64`, `darwin-x86_64`, `linux-aarch64`, `linux-x86_64`,
`windows-aarch64`, `windows-x86_64` (`crates/loom-sessions/src/acp_launch.rs:72`).

#### Launch resolution — the CLI path

`resolve(loom_dir, agent_id)` — `crates/loom-sessions/src/acp_launch.rs:344`:

1. **Explicit path.** If the agent id contains `/` or `\`, it is used verbatim
   as the program, with no args (`crates/loom-sessions/src/acp_launch.rs:345`).
2. **Local `config/agents.toml`** (`crates/loom-sessions/src/acp_launch.rs:354`).
3. **Public registry** (`crates/loom-sessions/src/acp_launch.rs:355`). This
   fetch is **always attempted** when the id is not a path, even if a local
   entry exists; the error is swallowed (`.ok()`).
4. **A `PATH` binary** (`crates/loom-sessions/src/acp_launch.rs:367`). If
   nothing resolves, the error is *"Agent '{id}' is not registered and not on
   PATH. Register it with: loom add-acp-agent {id}"*.

**Merge rule** when both a local and a registry entry exist: if the local entry
has **no** `args` and the registry entry **has** args, the local launch takes
the registry's args and its env is extended with the registry's env
(`crates/loom-sessions/src/acp_launch.rs:358`). This exists because most agents
need a protocol mode flag (`acp`, `--acp`) before they speak ACP at all.

`ensure_runnable` then checks that the resolved program exists on `PATH`
(`crates/loom-sessions/src/acp_launch.rs:382`). Explicit paths are accepted
as-is.

#### Launch resolution — the daemon path (different!)

The daemon's `acp.dispatch` uses a **different** resolver,
`resolve_agent_launch` (`crates/loomd/src/capture.rs:51`):

- A `config/agents.toml` entry wins **only if** its program differs from the
  agent id or it carries args (`crates/loomd/src/capture.rs:60`).
- Otherwise the daemon scans `PATH` for an executable with the agent's name
  (`crates/loomd/src/capture.rs:66`).
- The daemon **never** consults the public registry and does **not** accept an
  explicit path.
- `resolve_agent_launch_with_adapter` first tries the tool TOML's `[launch]`
  section (`crates/loomd/src/capture.rs:94`).

A third-party agent that must work under the daemon should therefore be
registered in `config/agents.toml` **with** its protocol-mode args.

#### Running an agent

```bash
loom acp run <agent> --prompt "<text>" [--cwd DIR] [--timeout-secs N] [-- args…]
```

CLI definition `crates/loom-cli/src/cli.rs:1806`; implementation
`crates/loom-cli/src/commands/acp_run.rs:86`. Explicit CLI args **replace** the
resolved args (`crates/loom-cli/src/commands/acp_run.rs:105`).

The daemon's counterpart is the `acp.dispatch` IPC command
(`crates/loomd/src/ipc_handler.rs:550`, implementation
`crates/loomd/src/capture.rs:151`), which additionally records a tracked run
with a `run_id`.

### 2.7 Publishing an agent to the public registry

The registry lives upstream at
<https://github.com/agentclientprotocol/registry> and is vendored in this
repository as `acp_registry/` (a git submodule, pinned at
`2edbfd245997eae3b19c0b6cba46f86826dc5f44`).

**Per-agent layout** (`acp_registry/CONTRIBUTING.md:13`): a directory named
exactly like the agent id, containing `agent.json` and `icon.svg` (16×16,
monochrome, `currentColor`).

**`agent.json` schema** (`acp_registry/agent.schema.json`):

| Rule | Anchor |
|---|---|
| Required: `id`, `name`, `version`, `description`, `distribution` | `acp_registry/agent.schema.json:7` |
| `id` must match `^[a-z][a-z0-9-]*$` | `acp_registry/agent.schema.json:11` |
| `version` must match `^[0-9]+\.[0-9]+\.[0-9]+` | `acp_registry/agent.schema.json:21` |
| `distribution` accepts **only** `binary`, `npx` or `uvx` (`additionalProperties: false`) | `acp_registry/agent.schema.json:54` |
| A `binary` distribution is keyed by the six platform ids | `acp_registry/agent.schema.json:78` |
| A binary target requires `archive` and `cmd` | `acp_registry/agent.schema.json:91` |
| A package distribution requires `package` | `acp_registry/agent.schema.json:121` |

The registry file itself requires `version` and `agents`
(`acp_registry/registry.schema.json:7`). Agents must support **Agent Auth** or
**Terminal Auth** (`acp_registry/AUTHENTICATION.md:7`).

### 2.8 Worked example — a complete agent registration

**Step 1 — the agent program.** A minimal ACP agent is a program that reads
JSON-RPC lines from stdin and writes JSON-RPC lines to stdout. The reference
implementation Loom tests against is
`crates/loom-sessions/tests/fixtures/fake_acp_server.py` (modes: `happy`,
`permission`, `bad-version`, `hang`, `error`).

**Step 2 — register it locally.** Create `~/.loom/config/agents.toml`:

```toml
# ~/.loom/config/agents.toml
[acme-agent]
runner = "npx"
package = "@acme/agent@1.0.0"
args = ["--acp"]
env = { ACME_API_KEY = "..." }

[acme-local]
runner = "binary"
cmd = "acme-agent"
args = ["acp"]
```

That is the complete shape. `runner = "npx"` launches
`npx @acme/agent@1.0.0 --acp`; `runner = "binary"` resolves `acme-agent` on
`PATH` and runs it with `acp`.

**Step 3 — publish to the registry** (optional). Add
`acp_registry/acme-agent/agent.json`:

```json
{
  "id": "acme-agent",
  "name": "Acme Agent",
  "version": "1.0.0",
  "description": "An ACP agent that talks to Loom over stdio.",
  "distribution": {
    "npx": {
      "package": "@acme/agent@1.0.0",
      "args": ["--acp"]
    }
  }
}
```

plus `acp_registry/acme-agent/icon.svg` (16×16, monochrome, `currentColor`).
Once merged upstream, users run `loom add-acp-agent acme-agent`.

**Step 4 — run it.**

```bash
loom acp run acme-agent --prompt "summarise this repo" --json
```

---

## 3. Compatibility policy

**Stable within this spec version (1.x):**

- The `[tool]` section and the nine optional section names and their field
  names, in `~/.loom/tools/<tool>.toml`.
- The rule that a missing section is a no-op, never an error.
- The `config/agents.toml` key set: `runner`, `package`, `args`, `env`, `cmd`.
- The ACP wire contract: JSON-RPC 2.0 over stdio, the `initialize` →
  `session/new` → `session/prompt` order, the required result fields
  (`protocolVersion`, `sessionId`, `stopReason`), the `session/update` kinds,
  and the `session/request_permission` answer shape.
- The `{loom_dir}` layout for tool files and agent files.

**May change without a spec-version bump:**

- New optional TOML sections and new optional fields inside existing sections
  (they parse today, because unknown keys are ignored —
  `crates/loom-generic-adapter/src/config.rs:33`).
- New `sessionUpdate` kinds (ignored today —
  `crates/loom-sessions/src/acp_protocol.rs:170`).
- New `runner` values in `config/agents.toml` (unknown values currently fall
  back to "the agent id is the program" —
  `crates/loom-sessions/src/acp_launch.rs:307`).
- The set of built-in preset adapters.
- The registry contents and the `loom acp …` CLI surface.

**Requires a spec-version bump:**

- Renaming or removing a TOML section, or removing/renaming a field.
- Changing the ACP request order, the required result fields, or the permission
  answer shape.
- Changing the meaning of an existing `config/agents.toml` key.

**What Loom does not promise.** Loom does not promise that a *specific* ACP
agent keeps working across Loom releases: the agent's own version, its
protocol-mode flag, and its willingness to run headless are outside Loom's
control. Loom promises only the wire contract above.

**Version reporting.** The ACP protocol version a given Loom build speaks is
`ACP_PROTOCOL_VERSION` (`crates/loom-sessions/src/acp_protocol.rs:20`). The
adapter contract version is the version of this document.

---

## 4. Known inconsistencies (code vs earlier promises)

These are reported, **not fixed** — this document changes no runtime
behaviour. Each entry names the code that decides, so a reader can see the
truth and a maintainer can fix it separately.

| # | Where | Disagreement |
|---|---|---|
| 1 | `crates/loom-sessions/src/acp_protocol.rs:9` | The file header still says *"NOTE (TDD RED step): tests only — `AcpProtocolClient` and friends do not exist yet."* The implementation exists at `:201`. |
| 2 | `crates/loom-sessions/src/acp_protocol.rs:416` | The comment says `session/cancel` is a notification, but it is built by `build_request` (`:99`) which always emits an `id` — it is a request. |
| 3 | `crates/loom-sessions/src/acp_launch.rs:3` | The module doc says *"Three sources"*; there are four (the explicit-path branch at `:345` is not mentioned). |
| 4 | `crates/loom-sessions/src/acp_launch.rs:335` | The doc says the local entry keeps the common case *"stays offline"*, but `:355` always performs a registry fetch. `reqwest::Client::new()` (`crates/loom-sessions/src/acp_client.rs:38`) sets no timeout, so a hung CDN stalls the launch. |
| 5 | `crates/loom-sessions/src/acp_protocol.rs:30` | `cwd` is documented as the *"Session working directory (absolute)"* but is only echoed into `session/new`; the child process is never chdir'd (no `Command::current_dir()` in the file). The persisted `working_directory` is Loom's cwd, not the agent's (`:519`). |
| 6 | `crates/loom-sessions/src/acp_launch.rs:214` | `to_toml_entry` documents itself as rendering *the* `agents.toml` entry, but renders a partial one — `source` is added separately (`crates/loom-cli/src/commands/tools_acp.rs:56`). |
| 7 | `crates/loom-cli/src/commands/tools_acp.rs:56` | `source = "acp-registry"` is written but never read by any Loom code path. |
| 8 | `crates/loom-sessions/src/acp_client.rs:10` | `AgentMetadata` has no `version` field, although the registry schema requires one (`acp_registry/agent.schema.json:7`); `repository`, `website`, `authors`, `license`, `icon` are dropped too. `auth_methods` / `platforms` / `tags` are parsed and never used, so the registry's authentication guarantee (`acp_registry/AUTHENTICATION.md:7`) is not surfaced by Loom. |
| 9 | `acp_registry/` | The directory is committed as a gitlink with **no `.gitmodules` entry**: `git submodule status` fails with *"fatal: no submodule mapping found in .gitmodules for path 'acp_registry'"*. A fresh clone cannot populate it, so the registry schemas are unavailable to contributors. |
| 10 | `crates/loom-cli/src/commands/acp_run.rs:53` | `--runtime` is advertised in the CLI help as *"Runtime backend: process, docker, podman, kubernetes"* (`crates/loom-cli/src/cli.rs:1818`) but is never implemented — it warns *"runtime '…' does not support the ACP protocol yet; running directly"* and runs directly. |
| 11 | `crates/loomd/src/capture.rs:237` | `complete_run(&run_id, 0, …)` hardcodes exit code `0`, so `loom acp run-status` shows `Exit: 0` even for a failed agent; and because the error path returns before `complete_run`, a failed dispatch leaves the run "still running" forever. |
| 12 | `crates/loom-core/src/config.rs:197` | `config/acp.toml`'s `registry_url` is parsed and displayed by `loom config show` (`crates/loom-cli/src/commands/config.rs:110`) but **never used to fetch** — every fetch site uses the hard-coded `ACP_REGISTRY_URL` (`crates/loom-sessions/src/acp_launch.rs:317`, `crates/loom-cli/src/commands/tools_acp.rs:14`, `crates/loom-cli/src/commands/acp.rs:310`). The documented "ACP registry URL" setting is dead for registry access. |
| 13 | `crates/loom-generic-adapter/src/lib.rs:102` | `present_features` / `absent_features` cover only the six trait sections; `[lsp]`, `[debug]` and `[launch]` are never reported. `GenericAdapter` does not implement the `LspProvider` / `DebugProvider` core traits (`crates/loom-core/src/types.rs:960`, `:984`), so a TOML-registered tool's `[lsp]` and `[debug]` sections are parsed but unused by `loom lsp exec` and `loom debug`, which resolve tools through `adapter_for` only (`crates/loom-cli/src/commands/lsp.rs:89`). |
| 14 | `crates/loom-generic-adapter/src/traits.rs:76` | The generic trait implementations are documented as "no-ops", and they are: `GenericMcpConsumer` never writes the tool's MCP config, `GenericHookExecutor` never executes a hook, `GenericAuthProvider::export_credential` always errors. A `[mcp]` / `[hooks]` / `[auth]` section in a hand-written TOML therefore declares intent that the generic runtime does not fulfil. |
| 15 | `crates/loom-auth/src/import.rs:42` | `loom-auth` reads tool TOMLs with a *different* schema (`[auth]`/`[provider]` with `provider_id`, `credential`, …) than `ToolToml`'s `[auth]` section (`store_path` / `providers` / `import_on_register`). The two coexist in one file and neither documents the other. |
| 16 | `AGENTS.md` | The agent guide omits `config/agents.toml` and `config/security.toml` from the filesystem layout, omits `--runtime` from the ACP command list, does not say `loom add-acp-agent` needs network, and does not distinguish the daemon resolver (`crates/loomd/src/capture.rs:51`) from the CLI resolver (`crates/loom-sessions/src/acp_launch.rs:344`). |
| 17 | `ROADMAP.md:41` | The pillar-3 promise — *"publish the `acp_registry` schemas"* — cannot be satisfied from this repository alone, because of #9: the schema directory is an unpopulatable gitlink. This document restates the schema rules inline (§2.7) so the promise is at least satisfiable in prose. |
| 18 | `crates/loom-cli/src/commands/thread.rs:39` | `loom thread <tool>` for a tool name that is not one of the 12 presets **overwrites** `{loom_dir}/tools/<tool>.toml` with a minimal `[tool]`-only file (`:54`), destroying any hand-written sections. The CLI reports *"✓ Updated tool"*, not *"replaced"*. A third-party tool therefore must not be registered with `loom thread` after its TOML is written — but nothing in the CLI or `AGENTS.md` says so. |
| 19 | `crates/loomd/src/ipc_handler.rs:381` | The daemon's `register_tool` handler writes a registration file **only for preset tools** (`:382`). For a third-party tool it returns `{"tool": …, "features": []}` (`:397`) and writes nothing, so notifying the daemon about a hand-written tool has no effect on disk. The CLI's `loom thread` path behaves the opposite way (it writes, then notifies), so the two registration paths disagree. |
| 20 | `crates/loom-generic-adapter/src/config.rs:200` | `command` is a required field in three places — `[launch].command` (`:247`), `[lsp].servers[].command` (`:200`) and `[debug].adapters[].command` (`:228`) — so an LSP or debug server entry without a command makes the **whole tool TOML unparseable**, and the scanner then skips the file entirely (`crates/loom-generic-adapter/src/scanner.rs:53`). A tool author who only wanted a `[launch]` section cannot ship a half-filled `[lsp]` block. |

---

## 5. Anchor index

Every `file:line` anchor in this document, grouped by file.

**`crates/loom-adapters/src/lib.rs`** — trait: `:36`; required: `:37`, `:38`,
`:39`, `:40`; defaulted: `:46`, `:92`, `:97`, `:114`, `:121`, `:126`, `:143`,
`:151`; `SyncReport`: `:307`; `write_tool_registration`: `:292`;
`adapter_for`: `:316`; `supported_tools`: `:334`; `sync_tools_dir`: `:351`;
`sync_mcp_to_native`: `:169`; `sync_skills_to_native`: `:227`.

**`crates/loom-generic-adapter/src/config.rs`** — `ToolToml`: `:34`; `[tool]`: `:36`;
`[skills]`: `:39`; `[mcp]`: `:42`; `[auth]`: `:45`; `[hooks]`: `:48`;
`[routing]`: `:51`; `[sessions]`: `:54`; `[lsp]`: `:57`; `[debug]`: `:60`;
`[launch]`: `:63`; `ToolSection`: `:68`; `SkillsSection`: `:77`;
`McpSection`: `:96`; `AuthSection`: `:112`; `HooksSection`: `:125`;
`RoutingSection`: `:142`; `SessionsSection`: `:163`; `LspSection`: `:183`;
`LspServerConfig`: `:196`; `DebugSection`: `:214`; `DebugAdapterConfig`: `:224`;
`LaunchSection`: `:240`; defaults: `:90`, `:136`, `:153`, `:157`, `:174`,
`:263`, `:267`.

**`crates/loom-generic-adapter/src/lib.rs`** — trait table: `:12`; section rows:
`:19`, `:20`, `:21`, `:22`, `:23`, `:24`; no-op rule: `:26`;
`GenericAdapter::new`: `:38`; `from_toml`: `:52`; `builder`: `:62`;
`present_features`: `:102`; `absent_features`: `:126`; `launch_config`: `:156`.

**`crates/loom-generic-adapter/src/traits.rs`** — `build_skill_provider`: `:178`;
skills fallback: `:184`; `build_mcp_consumer`: `:190`;
`build_auth_provider`: `:199`; `build_hook_executor`: `:208`;
`build_routing_target`: `:217`; tier match: `:219`;
`build_session_exportable`: `:229`; `build_all`: `:239`.

**`crates/loom-generic-adapter/src/scanner.rs`** — `scan`: `:27`; skip-invalid:
`:50`; `parse_file`: `:62`; warnings: `:68`, `:71`, `:76`.

**`crates/loom-sessions/src/acp_protocol.rs`** — `ACP_PROTOCOL_VERSION`: `:20`;
`AcpClientConfig`: `:24`; `AcpTranscript`: `:39`; `AcpIncoming`: `:52`;
`parse_message`: `:72`; id-scalar check: `:85`; `build_request`: `:99`;
`id_key`: `:109`; `accumulate`: `:127`; `agent_message_chunk`: `:134`;
`tool_call`: `:143`; `tool_call_update`: `:161`; `permission_outcome`: `:176`;
cancelled: `:196`; `run_prompt`: `:206`; timeout clamp: `:207`; kill: `:222`;
`drive`: `:227`; stderr thread: `:243`; stderr echo: `:248`; blank-line skip:
`:268`; response dispatch: `:272`; `session/update`: `:281`; request dispatch:
`:287`; permission reply: `:289`; invalid: `:300`; send: `:310`; response key:
`:323`; timeout error: `:344`; `initialize`: `:352`; `protocolVersion`: `:354`;
capabilities: `:356`; version check: `:368`; `session/new`: `:388`; `cwd`:
`:379`; `sessionId`: `:393`; `session/prompt`: `:402`; prompt block: `:405`;
`session/cancel`: `:413`; cancel grace: `:420`; `stopReason`: `:427`;
`build_acp_session`: `:451`; session id: `:463`; topic: `:464`; assistant
text: `:467`; messages: `:480`; `source_tool`: `:512`; `working_directory`:
`:519`; tags: `:529`.

**`crates/loom-sessions/src/acp_launch.rs`** — `ACP_REGISTRY_URL`: `:22`;
`AcpLaunch`: `:27`; `Runner`: `:38`; `Distribution`: `:59`; `platform_key`:
`:72`; `exe_name`: `:85`; `pick_distribution`: `:142`; `to_launch`: `:181`;
`to_toml_entry`: `:214`; `from_agents_toml`: `:262`; `launch_from_entry`: `:271`;
`registry_entry`: `:316`; `from_registry`: `:326`; `resolve` doc: `:334`;
`resolve`: `:344`; explicit path: `:345`; local: `:354`; registry fetch: `:355`;
merge: `:358`; PATH: `:367`; `ensure_runnable`: `:382`.

**`crates/loom-sessions/src/acp_client.rs`** — `AgentMetadata`: `:10`;
`RegistryResponse`: `:24`; client construction: `:38`; registry URL: `:58`,
`:73`.

**`crates/loom-core/`** — `loom_dir`: `src/lib.rs:25`; `home_dir`:
`src/lib.rs:15`; `find_binary`: `src/lib.rs:40`; `ECOSYSTEM_VERSION`:
`src/constants.rs:3`; `AGENTS_DIR`: `src/constants.rs:4`; `TOOLS_DIR`:
`src/constants.rs:10`; `CONFIG_DIR`: `src/constants.rs:12`; `acp.toml` overlay:
`src/config.rs:197`; `RoutingDecision`: `src/types.rs:575`; `LspProvider`:
`src/types.rs:960`; `DebugProvider`: `src/types.rs:984`; plugin tool write:
`src/plugin_registry.rs:169`, `:209`.

**`crates/loom-cli/`** — `Thread`: `src/cli.rs:61`; `AddAcpAgent`: `:155`;
plugin actions: `:262`, `:275`; `AcpAction::Run`: `:1806`; `--timeout-secs`
default: `:1816`; `--runtime` help: `:1818`; dispatch: `:1856`, `:1902`,
`:1924`; `thread.rs` run: `src/commands/thread.rs:7`; preset branch: `:22`;
minimal TOML: `:39`; TOML write: `:54`; daemon notify: `:60`; `tools_acp.rs`
handle: `:13`; registry fetch: `:14`; id filter: `:22`; `write_agent_entry`:
`:42`; agents path: `:46`; `source` key: `:56`; distribution: `:62`;
`acp_run.rs`: `src/commands/acp_run.rs:86`; resolve: `:104`; CLI args
override: `:105`; `ensure_runnable`: `:110`; `--runtime` warn: `:63`; JSON
output: `:142`; `acp.rs` agents: `src/commands/acp.rs:231`; agents path:
`:233`; registry: `:304`; local ids: `:387`; `exec.rs` launch config:
`src/commands/exec.rs:714`; adapter_for: `:57`, `:295`; `lsp.rs` adapter:
`src/commands/lsp.rs:89`; `config.rs` registry_url display:
`src/commands/config.rs:110`; `init.rs` overlays:
`src/commands/init.rs:201`.

**`crates/loomd/`** — `capture.rs` daemon resolver: `src/capture.rs:51`;
`agents.toml` precedence: `:60`; PATH scan: `:66`; adapter launch:
`:88`, `:94`; dispatch: `:151`; exit code: `:237`; `ipc_handler.rs`
`register_tool`: `src/ipc_handler.rs:373`; preset write: `:382`;
`GenericAdapter::new`: `:388`; features response: `:397`; `acp.dispatch`:
`:550`.

**`crates/loom-auth/`** — `ToolAuthConfig`: `src/import.rs:10`;
`provider_id` field: `:13`; `ToolConfig`: `:42`; `scan_tools`: `:73`;
`scan_tool_file`: `:117`; `[auth]` branch: `:127`; `[provider]` branch: `:145`.

**`crates/loom-sessions/tests/fixtures/fake_acp_server.py`** — reference agent:
`:1`; modes: `:4`; permission flow: `:71`.

**`acp_registry/`** — `agent.schema.json` required: `:7`; id pattern: `:11`;
version pattern: `:21`; distribution: `:54`; platform keys: `:78`; binary
target: `:91`; package target: `:121`; `registry.schema.json` required: `:7`;
`CONTRIBUTING.md` dir-name rule: `:13`; `AUTHENTICATION.md` auth rule: `:7`.
