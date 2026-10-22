# Loom Public Protocol — Adapter Contract & ACP Dispatch

**Date:** 2026-10-21
**Status:** Published (spec v1.0.0)
**Spec version:** 1.0.0
**Describes:** Adapter contract v1, ACP protocol v1

---

## Overview

Loom has three public extension points. A third party plugs into any of them
without forking the Loom repository.

| Extension point | What you write | What Loom reads it from | Spec section |
|---|---|---|---|
| **Adapter contract** | One TOML file, `~/.loom/tools/<tool>.toml` | `loom-generic-adapter` | [§1](#1-the-adapter-contract) |
| **ACP dispatch** | A JSON-RPC 2.0 agent on stdio, plus a registry entry | `loom-sessions` / `loomd` | [§2](#2-the-acp-protocol) |
| **ACP dispatch server** | A TCP client that speaks the JSON-line dispatch protocol, behind an Ed25519 handshake | `loom-sessions` (server) / `loomd` (host) | [§2.9](#29-the-acp-dispatch-server-agent-to-agent-tcp-dispatch) |

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
`initialize` (`crates/loom-sessions/src/acp_protocol.rs:425`) and rejects any
response whose `protocolVersion` is not exactly `1`
(`crates/loom-sessions/src/acp_protocol.rs:427`).

**The adapter contract has no runtime version constant.** A grep across
`crates/` finds no `ADAPTER_VERSION` / `CONTRACT_VERSION` / `TOOL_TOML_VERSION`
constant. The contract version is therefore *this document's* version: the TOML
shape is versioned by the spec, and a future incompatible change to the TOML
shape must bump the spec version and be announced in `CHANGELOG.md`. Loom's
semantic version (`0.9.6`, `crates/loom-core/src/lib.rs` workspace
`Cargo.toml:34`) is **not** the contract version.

---

## 1. The Adapter contract

### 1.1 The `LoomToolAdapter` trait

`pub trait LoomToolAdapter: Send + Sync` — `crates/loom-adapters/src/lib.rs:41`.

Implementing this trait is the **Rust-native** path: it is how Loom's 12 built-in
presets are written (`crates/loom-adapters/src/lib.rs:443` lists them: `jcode`,
`crush`, `claude-code`, `opencode`, `goose`, `codex`, `cursor`, `gemini-cli`,
`aider`, `kiro`, `juno`, `github-copilot`). A third party that only needs
configuration, not code, should use the TOML path in [§1.4](#14-registering-a-tool)
instead — it needs no Rust.

**Names and aliases.** `adapter_for` (`crates/loom-adapters/src/lib.rs:425`)
also accepts four aliases that are **not** in `supported_tools()`: `claude_code`
(for `claude-code`, `crates/loom-adapters/src/lib.rs:429`), `cursor-cli` (for
`cursor`, `:324`), `gemini` (for `gemini-cli`, `:325`) and `copilot` (for
`github-copilot`, `:328`). `supported_tools()`
(`crates/loom-adapters/src/lib.rs:443`) lists only the 12 canonical names, so
a third party that enumerates presets should use the canonical names; the
aliases exist only so a hand-typed CLI argument resolves.

**Required methods** (no default body):

| Method | Anchor | Contract |
|---|---|---|
| `tool_name(&self) -> &'static str` | `crates/loom-adapters/src/lib.rs:41` | The registration file stem: `~/.loom/tools/<tool_name>.toml` (`crates/loom-adapters/src/lib.rs:418`). |
| `default_toml(&self, home: &str) -> ToolToml` | `crates/loom-adapters/src/lib.rs:41` | The full TOML document for this tool. `home` is the tilde-expanded user home (`crates/loom-core/src/lib.rs:15`). |
| `apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()>` | `crates/loom-adapters/src/lib.rs:41` | Apply a routing decision to the tool's environment. `RoutingDecision` carries `recommended_provider` / `recommended_model` (`crates/loom-core/src/types.rs:609`). |
| `sync_from_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport>` | `crates/loom-adapters/src/lib.rs:41` | Import the tool's native resources into the Loom ecosystem. `SyncReport` (`crates/loom-adapters/src/lib.rs:236`) reports `skills_imported`, `skills_synced`, `mcp_servers_imported`, `mcp_servers_synced`, `providers_imported`, `notes`. |

**Defaulted methods** (override only if you need to):

| Method | Anchor | Default behaviour |
|---|---|---|
| `sync_to_native(&self, loom_dir, home) -> Result<SyncReport>` | `crates/loom-adapters/src/lib.rs:47` | Pushes shared MCP servers into the tool's native config (`sync_mcp_to_native`, `crates/loom-adapters/src/lib.rs:172`) and symlinks shared skills into the tool's skills dir (`sync_skills_to_native`, `crates/loom-adapters/src/lib.rs:235`). |
| `lsp_config(&self, home) -> Option<LspSection>` | `crates/loom-adapters/src/lib.rs:95` | Returns the `[lsp]` section of `default_toml`. |
| `lsp_servers(&self, home) -> Vec<LspServerInfo>` | `crates/loom-adapters/src/lib.rs:100` | Projects `lsp_config` into server infos. |
| `resolve_lsp_server(&self, extension, home) -> Option<LspServerInfo>` | `crates/loom-adapters/src/lib.rs:117` | First server whose `file_extensions` contains the extension. |
| `debug_config(&self, home) -> Option<DebugSection>` | `crates/loom-adapters/src/lib.rs:124` | Returns the `[debug]` section of `default_toml`. |
| `debug_adapters(&self, home) -> Vec<DebugAdapterInfo>` | `crates/loom-adapters/src/lib.rs:129` | Projects `debug_config` into adapter infos. |
| `resolve_debug_adapter(&self, language, home) -> Option<DebugAdapterInfo>` | `crates/loom-adapters/src/lib.rs:146` | First adapter whose `language` matches. |
| `apply_hashline_edit(&self, file_path, start, end, new_content, context_lines) -> Result<Option<String>>` | `crates/loom-adapters/src/lib.rs:161` | Reads the file, applies the edit with stale detection, returns the new content (the caller writes it). |

#### The `apply_routing_decision` environment convention

`apply_routing_decision` has **no shared helper**: each preset implements it
itself, and the convention is therefore a *convention*, not an enforced
contract. The convention is:

```rust
std::env::set_var("<TOOL>_MODEL", &decision.recommended_model);
std::env::set_var("LOOM_ROUTING_PROVIDER", &decision.recommended_provider);
```

Ten of the twelve presets follow it exactly:

| Preset | Model variable | `LOOM_ROUTING_PROVIDER` | Anchor |
|---|---|---|---|
| `aider` | `AIDER_MODEL` | yes | `crates/loom-adapters/src/aider.rs:52`, `:53` |
| `codex` | `CODEX_MODEL` | yes | `crates/loom-adapters/src/codex.rs:52`, `:53` |
| `cursor` | `CURSOR_MODEL` | yes | `crates/loom-adapters/src/cursor_cli.rs:52`, `:53` |
| `gemini-cli` | `GEMINI_MODEL` | yes | `crates/loom-adapters/src/gemini_cli.rs:52`, `:53` |
| `github-copilot` | `COPILOT_MODEL` | yes | `crates/loom-adapters/src/github_copilot.rs:56`, `:57` |
| `goose` | `GOOSE_MODEL` | yes | `crates/loom-adapters/src/goose.rs:52`, `:53` |
| `jcode` | `ANTHROPIC_MODEL` | yes | `crates/loom-adapters/src/jcode.rs:78`, `:79` |
| `juno` | `JUNO_MODEL` | yes | `crates/loom-adapters/src/juno.rs:60`, `:61` |
| `kiro` | `KIRO_MODEL` | yes | `crates/loom-adapters/src/kiro.rs:61`, `:62` |
| `opencode` | `OPENCODE_MODEL` | yes | `crates/loom-adapters/src/opencode.rs:52`, `:53` |

**Two presets deviate.**

- `claude-code` sets **only** `ANTHROPIC_MODEL` (`crates/loom-adapters/src/claude_code.rs:47`) and never sets `LOOM_ROUTING_PROVIDER`.
- `crush` does **not** set an env var at all when `CRUSH_API_URL` is set: it
  POSTs `{"model": …, "provider": …}` to `{CRUSH_API_URL}/config/model` with
  `curl` and returns (`crates/loom-adapters/src/crush.rs:58`, `:63`, `:70`).
  Only the fallback path sets `CRUSH_MODEL`
  (`crates/loom-adapters/src/crush.rs:72`).

**The read-back rule — the part a third-party adapter must match.** The
consumers do not know which variable an adapter set; they *reconstruct* the
name from the tool name:

```rust
format!("{}_MODEL", agent.to_uppercase().replace('-', "_"))
```

— `crates/loom-cli/src/commands/resume.rs:329` and `:356`, and
`crates/loom-cli/src/commands/exec.rs:273`.

So the variable name a consumer looks for is derived from the **tool name**,
not from anything the adapter declares. An adapter that sets any other name
has its routing decision **silently ignored**: `loom sessions resume --model`
and `loom exec` will launch the tool with no model override and no error.

Under that rule only eight of the twelve presets are actually read back —
`aider`, `codex`, `cursor`, `goose`, `juno`, `kiro`, `opencode`, and `crush`
(whose `CRUSH_MODEL` matches, but only on its fallback path). The other four
set a name the read-back never looks for:

| Preset | Variable it sets | Variable the read-back looks for |
|---|---|---|
| `claude-code` | `ANTHROPIC_MODEL` | `CLAUDE_CODE_MODEL` |
| `jcode` | `ANTHROPIC_MODEL` | `JCODE_MODEL` |
| `gemini-cli` | `GEMINI_MODEL` | `GEMINI_CLI_MODEL` |
| `github-copilot` | `COPILOT_MODEL` | `GITHUB_COPILOT_MODEL` |

**A third-party adapter that wants its routing decision honoured must set
`{TOOL_NAME_UPPERCASED_UNDERSCORED}_MODEL` — for example `ACME_MODEL` for a
tool named `acme` — and should also set `LOOM_ROUTING_PROVIDER`**, which is
the only route by which `loom exec` learns the provider at all
(`crates/loom-cli/src/commands/exec.rs:362`).

**One more read-back caveat.** `loom exec` inserts the value of
`LOOM_ROUTING_PROVIDER` into the `{TOOL}_MODEL` slot
(`crates/loom-cli/src/commands/exec.rs:362`, `:362`, `:363`) — that is, the
*provider* string is written into the *model* variable, and the model string
is not propagated at all. `loom sessions resume` does the opposite: it reads
the tool's own model variable and forwards it
(`crates/loom-cli/src/commands/resume.rs:329`, `:350`, `:351`). The two
commands therefore disagree about what a routing decision means; see §4 #21.

**Rust-side extension points.** Two core traits exist for LSP and DAP but are
**not** implemented by `LoomToolAdapter` itself: `LspProvider`
(`crates/loom-core/src/types.rs:1284`) and `DebugProvider`
(`crates/loom-core/src/types.rs:1284`). `loom lsp exec <tool> …` resolves the
tool through `adapter_for` and then `lsp_servers`
(`crates/loom-cli/src/commands/lsp.rs:98`).

### 1.2 The tool TOML shape

`pub struct ToolToml` — `crates/loom-generic-adapter/src/config.rs:82`. It is
parsed by `toml::from_str` (`crates/loom-generic-adapter/src/lib.rs:42`), so it
follows TOML semantics with serde.

**Required:**

| Section | Anchor | Fields |
|---|---|---|
| `[tool]` | `crates/loom-generic-adapter/src/config.rs:36` | `name: String`, `version: String` — both required (`crates/loom-generic-adapter/src/config.rs:70`). A TOML without `[tool]` fails to parse (`missing field 'tool'`). |

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
- **Unknown keys and unknown sections are ignored.** `ToolToml` (`crates/loom-generic-adapter/src/config.rs:34`) has no
  `deny_unknown_fields`, so a
  forward-compatible TOML with extra keys still parses.
- Four fields are hard errors, all surfacing as `toml::de::Error`
  `missing field …`: a missing `[tool]`, a missing `[tool].name`, a missing
  `[tool].version`, and a missing `command` in a `[launch]`, `[lsp].servers[]`
  or `[debug].adapters[]` entry. Everything else has a default or is
  optional.

### 1.3 The no-op contract

> "Missing sections produce no-op implementations — the adapter never errors
> on absent sections."
> — `crates/loom-generic-adapter/src/lib.rs:28`

The mechanism is concrete: `TraitBuilder::build_*` returns `Option<…>`, and
`None` means "no implementation for this trait" — not "an error"
(`crates/loom-generic-adapter/src/traits.rs:194`, `:190`, `:199`, `:208`,
`:217`, `:229`).

| Section | Trait implemented | Anchor |
|---|---|---|
| `[skills]` | `SkillProvider` | `crates/loom-generic-adapter/src/traits.rs:178` |
| `[mcp]` | `McpConsumer` | `crates/loom-generic-adapter/src/traits.rs:190` |
| `[auth]` | `AuthProviderTrait` | `crates/loom-generic-adapter/src/traits.rs:199` |
| `[hooks]` | `HookExecutor` | `crates/loom-generic-adapter/src/traits.rs:208` |
| `[routing]` | `RoutingTarget` | `crates/loom-generic-adapter/src/traits.rs:217` |
| `[sessions]` | `SessionExportable` | `crates/loom-generic-adapter/src/traits.rs:229` |

When the section *is* present, the generic implementation is still deliberately
thin: `GenericMcpConsumer::add_mcp_server` / `remove_mcp_server` are no-ops
(`crates/loom-generic-adapter/src/traits.rs:83`, `:83`),
`GenericHookExecutor::execute_hook` is a no-op
(`crates/loom-generic-adapter/src/traits.rs:116`),
`GenericAuthProvider::discover_providers` returns empty
(`crates/loom-generic-adapter/src/traits.rs:95`), and
`GenericSessionExportable::export_session` errors with "no store"
(`crates/loom-generic-adapter/src/traits.rs:149`). **A TOML registration
declares integration points; it does not by itself move data.** The data-moving
paths are the preset adapters' `sync_from_native` / `sync_to_native` and the
daemon's `register_tool` handler
(`crates/loomd/src/ipc_handler.rs:377`).

`present_features()` / `absent_features()` report only these six features
(`crates/loom-generic-adapter/src/lib.rs:126`, `:126`). The `[lsp]`, `[debug]`
and `[launch]` sections are **not** traits and never appear in those lists.

`[skills]` with no `native_dir` falls back to `{loom_dir}/skills`
(`crates/loom-generic-adapter/src/traits.rs:184`).

`[routing].default_tier` maps `"flagship"` → `ModelTier::Flagship`,
`"slm"` → `ModelTier::Slm`, and **anything else** → `ModelTier::Mid`
(`crates/loom-generic-adapter/src/traits.rs:222`).

`[launch]` is the only section the generic path actually executes. It is
projected into `loom_runtime::LaunchConfig` by
`launch_config` (`crates/loom-generic-adapter/src/lib.rs:156`)
and consumed by `loom exec`
(`crates/loom-cli/src/commands/exec.rs:722`), workflow dispatch
(`crates/loom-workflow/src/dispatch.rs:16`), the TUI
(`crates/loom-tui/src/dispatch.rs:618`) and the daemon's ACP launcher
(`crates/loomd/src/capture.rs:80`).

### 1.4 Registering a tool

The ecosystem directory is `~/.loom`, or `$LOOM_AGENTS_DIR` when set
(`loom_core::loom_dir()`, `crates/loom-core/src/lib.rs:25`; directory name
constant `AGENTS_DIR = ".loom"`, `crates/loom-core/src/constants.rs:4`). Tool
files live in `{loom_dir}/tools/` (`TOOLS_DIR`, `crates/loom-core/src/constants.rs:10`).

**Option A — `loom thread <tool>`** (`crates/loom-cli/src/cli.rs:63`):

- For a **preset** tool (one of the 12 in `supported_tools()`), Loom writes the
  preset's `default_toml` to `{loom_dir}/tools/<tool>.toml` via
  `write_tool_registration` (`crates/loom-adapters/src/lib.rs:401`) and runs
  `sync_from_native` (`crates/loom-cli/src/commands/thread.rs:28`).
- For an **unknown** tool name, Loom writes a minimal registration with
  `[tool]` only and all nine optional sections absent
  (`crates/loom-cli/src/commands/thread.rs:40`) — you then edit that file.
  **This overwrites an existing file of the same name**
  (`crates/loom-cli/src/commands/thread.rs:95`), so for a hand-written tool,
  write the TOML *first* and do not re-run `loom thread` for that name.
- Either way it notifies a running daemon over the `register_tool` IPC command
  (`crates/loom-cli/src/commands/thread.rs:62`). The daemon's own
  `register_tool` handler writes a registration file **only for preset
  tools**; for a non-preset name it reports an empty feature list
  (`crates/loomd/src/ipc_handler.rs:377`, `:397`).

**Option B — write the file yourself.** Create
`{loom_dir}/tools/<tool>.toml` with the shape in [§1.2](#12-the-tool-toml-shape).
Nothing else is required; `GenericAdapter::new` reads it on demand
(`crates/loom-generic-adapter/src/lib.rs:38`).

**Option C — `loom plugin install` / `loom plugin register`** downloads or
copies a tool TOML into `{loom_dir}/tools/` and records a manifest with a
SHA-256 checksum (`crates/loom-core/src/plugin_registry.rs:171`, `:209`;
CLI definitions `crates/loom-cli/src/cli.rs:263`, `:275`).

**Reading a tool file back.** `AdapterDirScanner::scan` reads every `*.toml` in
`{loom_dir}/tools/` (`crates/loom-generic-adapter/src/scanner.rs:28`). A file
that fails to parse is **skipped with a warning**, not fatal
(`crates/loom-generic-adapter/src/scanner.rs:53`). Non-fatal validation
warnings are emitted for an empty `tool.name`, an empty `tool.version`, and a
`[skills]` section with neither `native_dir` nor `import_on_register`
(`crates/loom-generic-adapter/src/scanner.rs:76`, `:71`, `:76`).

**One more reader to know about.** `loom-auth` scans the same directory with a
*different* schema: it looks for a top-level `[auth]` or `[provider]` section
whose `provider_id` is set (`crates/loom-auth/src/import.rs:13`, `:73`,
`:127`, `:145`). That schema accepts keys such as `provider_id`, `name`,
`auth_method`, `api_base`, `models_endpoint`, `credential`, `credential_file`,
`auth_store`, `supported_tools`
(`crates/loom-auth/src/import.rs:19`). The `ToolToml` `[auth]` section is a
*different* shape (`store_path` / `providers` / `import_on_register`,
`crates/loom-generic-adapter/src/config.rs:82`) — the two do not overlap.
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
(`crates/loom-cli/src/commands/thread.rs:40`, `:54`). See §4 #18.

To tell a *running daemon* about a non-preset tool, send the `register_tool`
IPC command; it reports the adapter's features without writing anything
(`crates/loomd/src/ipc_handler.rs:377`, `:397`). In practice the file alone
is enough: `GenericAdapter::new` reads it on demand
(`crates/loom-generic-adapter/src/lib.rs:38`), and the daemon's ACP
launcher prefers a tool's `[launch]` section over everything else
(`crates/loomd/src/capture.rs:80`). The CLI's `loom acp run` does **not**
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
  (`crates/loom-sessions/src/acp_protocol.rs:100`).
- **Envelope:** Loom's requests are built by
  `build_request(id, method, params)` → `{"jsonrpc":"2.0","id":<n>,"method":<s>,"params":<obj>}`
  (`crates/loom-sessions/src/acp_protocol.rs:100`).
- **Parsing:** `parse_message` classifies a line as `Response`, `Notification`,
  `Request` or `Invalid` (`crates/loom-sessions/src/acp_protocol.rs:118`, `:72`).
  A message must be a JSON object; `id` must be a number or a string
  (`crates/loom-sessions/src/acp_protocol.rs:107`).
- **Ids must be echoed as JSON numbers.** Loom keys its pending-response map by
  `id.to_string()` (`crates/loom-sessions/src/acp_protocol.rs:107`) and the
  reader by the serialized JSON value (`crates/loom-sessions/src/acp_protocol.rs:107`).
  The request ids Loom uses are `0`, `1`, `2` (and `3` for cancel). A response
  with `"id": "0"` (string) does **not** match request `0` and the request will
  time out.
- **stdout must be JSON only.** A line that is not a JSON object classifies as
  `Invalid` and is silently dropped
  (`crates/loom-sessions/src/acp_protocol.rs:107`, `:300`); blank lines are
  skipped (`crates/loom-sessions/src/acp_protocol.rs:107`).
- **stderr is free-form.** It is drained on a dedicated thread and echoed to
  Loom's stderr as `[STDERR] {line}` (`crates/loom-sessions/src/acp_protocol.rs:107`).
  stderr never enters the transcript.
- **Exit code is irrelevant.** The child is killed unconditionally after the
  turn (`crates/loom-sessions/src/acp_protocol.rs:107`); the daemon records exit
  code `0` regardless (`crates/loomd/src/capture.rs:151`). The agent must not
  exit before answering the three requests below.

### 2.2 The request flow

`AcpProtocolClient::run_prompt` (`crates/loom-sessions/src/acp_protocol.rs:707`)
runs exactly three requests.

**① `initialize`** (id `0`) — `crates/loom-sessions/src/acp_protocol.rs:425`

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
version …"* (`crates/loom-sessions/src/acp_protocol.rs:427`).
`agentCapabilities` in the response is **not inspected**.

Loom advertises `fs.readTextFile: false`, `fs.writeTextFile: false` and
`terminal: false` (`crates/loom-sessions/src/acp_protocol.rs:429`). An agent
that depends on the client for filesystem or terminal access will hang.

**② `session/new`** (id `1`) — `crates/loom-sessions/src/acp_protocol.rs:337`

```json
{"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"<dir>","mcpServers":[]}}
```

`mcpServers` is **always empty** — Loom offers no MCP servers over ACP. The
agent must answer with a string `result.sessionId`, otherwise the run fails with
*"session/new response missing sessionId"*
(`crates/loom-sessions/src/acp_protocol.rs:466`).

`cwd` is the CLI's `--cwd` value, or Loom's own current directory when unset
(`crates/loom-sessions/src/acp_protocol.rs:452`).

**③ `session/prompt`** (id `2`) — `crates/loom-sessions/src/acp_protocol.rs:475`

```json
{"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{
  "sessionId":"<id from session/new>",
  "prompt":[{"type":"text","text":"<the prompt>"}]
}}
```

The prompt is always exactly one text block. The agent must answer with a
string `result.stopReason`, otherwise the run fails with *"prompt response
missing stopReason"* (`crates/loom-sessions/src/acp_protocol.rs:638`).

**Cancel on timeout.** If `session/prompt` times out, Loom sends
`session/cancel` with id `3` and `{"sessionId": …}`
(`crates/loom-sessions/src/acp_protocol.rs:487`) and then waits a further 2
seconds for the original response before reporting the original error
(`crates/loom-sessions/src/acp_protocol.rs:433`).

**Timeouts are per request**, not per turn: each of `initialize`,
`session/new` and `session/prompt` gets the full timeout
(`crates/loom-sessions/src/acp_protocol.rs:232`). The minimum is 1 second. The
CLI default is 120 seconds (`crates/loom-cli/src/cli.rs:1869`); the daemon's
`acp.dispatch` default is also 120 (`crates/loomd/src/ipc_handler.rs:549`).

### 2.3 Agent → Loom notifications

`session/update` notifications carry the agent's output
(`crates/loom-sessions/src/acp_protocol.rs:638`). The update object is at
`params.update`, and `params.update.sessionUpdate` selects the kind
(`crates/loom-sessions/src/acp_protocol.rs:156`):

| `sessionUpdate` | Fields Loom reads | Anchor |
|---|---|---|
| `agent_message_chunk` | `params.update.content.text` — appended to the transcript text | `crates/loom-sessions/src/acp_protocol.rs:161` |
| `tool_call` | `toolCallId`, `title`, `status` — recorded as a tool call | `crates/loom-sessions/src/acp_protocol.rs:180` |
| `tool_call_update` | `toolCallId`, `status` — overwrites the status of the matching call | `crates/loom-sessions/src/acp_protocol.rs:180` |

Any other `sessionUpdate` kind is ignored
(`crates/loom-sessions/src/acp_protocol.rs:156`). A `tool_call` whose
`title`/`status` are missing defaults to `"?"` / `"pending"`.

### 2.4 Agent → Loom requests

Only one request method is answered: **`session/request_permission`**
(`crates/loom-sessions/src/acp_protocol.rs:361`). Loom answers on the agent's
stdin with:

```json
{"jsonrpc":"2.0","id":<same id>,"result":{"outcome":{"outcome":"selected","optionId":"<id>"}}}
```

The option is chosen by `permission_outcome`
(`crates/loom-sessions/src/acp_protocol.rs:249`): the **first** option whose
`kind` starts with `"allow"`, otherwise the **first** option, otherwise
`{"outcome":"cancelled"}` (`crates/loom-sessions/src/acp_protocol.rs:269`).

**Every other agent request is left unanswered** and will time out. A
third-party agent must therefore not require `fs/read_text_file`,
`fs/write_text_file` or terminal APIs from the client.

### 2.5 The transcript and the persisted session

The collected outcome is `AcpTranscript` (`crates/loom-sessions/src/acp_protocol.rs:39`):
`session_id`, `stop_reason`, `agent_text` (concatenated chunks) and
`tool_calls` (each rendered as `"{title} [{status}]"`,
`crates/loom-sessions/src/acp_protocol.rs:39`).

`build_acp_session` (`crates/loom-sessions/src/acp_protocol.rs:536`) turns a
transcript into a Loom session:

- session id `acp-<8 hex>` (`crates/loom-sessions/src/acp_protocol.rs:473`);
- three messages — System, User, Assistant
  (`crates/loom-sessions/src/acp_protocol.rs:566`);
- assistant text = agent text (or `"(agent produced no text output)"`), then a
  `Tools:` list, then `[stop reason: …]`
  (`crates/loom-sessions/src/acp_protocol.rs:499`);
- manifest `source_tool = "loom-acp/{agent}"`, `model_provider = {agent}`,
  tags `["acp", "acp-protocol", "agent:{agent}"]`, topic = prompt truncated to
  80 characters (`crates/loom-sessions/src/acp_protocol.rs:597`, `:523`, `:529`,
  `:464`).

The session is written under `{loom_dir}/sessions/{session_id}/` as
`manifest.json` + `messages.jsonl` (`crates/loom-sessions/src/store.rs:43`),
then indexed into cross-session memory and the knowledge graph
(`crates/loom-cli/src/commands/acp_run.rs:126`).

`loom acp run --json` prints
`{session_id, agent_session_id, stop_reason, tool_calls}`
(`crates/loom-cli/src/commands/acp_run.rs:178`).

### 2.6 Registering an ACP agent

#### `config/agents.toml`

The local agent registry is `{loom_dir}/config/agents.toml`
(`crates/loom-sessions/src/acp_launch.rs:263`). It is **not** created by
`loom init` — only `routing.toml`, `mcp.toml`, `hooks.toml` and `acp.toml`
are (`crates/loom-cli/src/commands/init.rs:203`).

There is **no Rust struct** for this file: it is read as a generic
`toml::Table` and each agent id is a top-level table
(`crates/loom-sessions/src/acp_launch.rs:265`, `:266`).

| Key | Type | Meaning | Anchor |
|---|---|---|---|
| `runner` | string | `"npx"` / `"npm"` → `npx <package> [args]`; `"uvx"` → `uvx <package> [args]`; `"pip"` / `"pipx"` → `pipx run <package> [args]`; `"binary"` → resolve `cmd` on `PATH`; **absent or unknown** → the agent id itself is the program | `crates/loom-sessions/src/acp_launch.rs:374` |
| `package` | string | npm / uv package name | `crates/loom-sessions/src/acp_launch.rs:273` |
| `args` | array of strings | Arguments passed to the program | `crates/loom-sessions/src/acp_launch.rs:274` |
| `env` | table of string→string | Environment for the agent | `crates/loom-sessions/src/acp_launch.rs:275` |
| `cmd` | string | **Only** consulted when `runner = "binary"`; defaults to the agent id; the directory part is stripped (`./pkg/foo` → `foo`) | `crates/loom-sessions/src/acp_launch.rs:420`, `:85` |
| `source` | string | Written by `loom add-acp-agent` as `"acp-registry"`. **Never read** by any Loom code path | `crates/loom-cli/src/commands/tools_acp.rs:56` |

There is **no** `command`, `path`, `cwd` or `timeout` key.

#### `loom add-acp-agent <agent_id>`

CLI definition `crates/loom-cli/src/cli.rs:155`, dispatch
`crates/loom-cli/src/cli.rs:1731`, handler
`crates/loom-cli/src/commands/tools_acp.rs:3`.

- It **requires network access** — it fetches the public registry and has no
  offline fallback (`crates/loom-cli/src/commands/tools_acp.rs:3`, `:17`).
- It validates only that the id exists in the registry
  (`crates/loom-cli/src/commands/tools_acp.rs:22`) and that a distribution
  exists for this host platform (`crates/loom-cli/src/commands/tools_acp.rs:64`).
  It does **not** check that the package is installable, nor that the agent
  actually speaks ACP.
- It writes the entry into `{loom_dir}/config/agents.toml`, preserving other
  entries (`crates/loom-cli/src/commands/tools_acp.rs:47`, `:76`).

The registry URL is hard-coded: `ACP_REGISTRY_URL =
"https://cdn.agentclientprotocol.com/registry"`
(`crates/loom-sessions/src/acp_launch.rs:316`); the registry JSON is fetched from
`{ACP_REGISTRY_URL}/v1/latest/registry.json`
(`crates/loom-sessions/src/acp_client.rs:58`) and parsed as
`{ "agents": [ … ] }` (`crates/loom-sessions/src/acp_client.rs:25`).

**Distribution choice** is deterministic: `npx` first, then `uvx`, then a
`binary` distribution for the host platform
(`crates/loom-sessions/src/acp_launch.rs:159`). Platform keys are
`darwin-aarch64`, `darwin-x86_64`, `linux-aarch64`, `linux-x86_64`,
`windows-aarch64`, `windows-x86_64` (`crates/loom-sessions/src/acp_launch.rs:75`).

#### Launch resolution — the CLI path

`resolve(loom_dir, agent_id)` — `crates/loom-sessions/src/acp_launch.rs:344`:

1. **Explicit path.** If the agent id contains `/` or `\`, it is used verbatim
   as the program, with no args (`crates/loom-sessions/src/acp_launch.rs:479`).
2. **Local `config/agents.toml`** (`crates/loom-sessions/src/acp_launch.rs:354`).
3. **Public registry** (`crates/loom-sessions/src/acp_launch.rs:354`). This
   fetch is **always attempted** when the id is not a path, even if a local
   entry exists; the error is swallowed (`.ok()`).
4. **A `PATH` binary** (`crates/loom-sessions/src/acp_launch.rs:374`). If
   nothing resolves, the error is *"Agent '{id}' is not registered and not on
   PATH. Register it with: loom add-acp-agent {id}"*.

**Merge rule** when both a local and a registry entry exist: if the local entry
has **no** `args` and the registry entry **has** args, the local launch takes
the registry's args and its env is extended with the registry's env
(`crates/loom-sessions/src/acp_launch.rs:359`). This exists because most agents
need a protocol mode flag (`acp`, `--acp`) before they speak ACP at all.

`ensure_runnable` then checks that the resolved program exists on `PATH`
(`crates/loom-sessions/src/acp_launch.rs:374`). Explicit paths are accepted
as-is.

#### Launch resolution — the daemon path (different!)

The daemon's `acp.dispatch` uses a **different** resolver,
`resolve_agent_launch` (`crates/loomd/src/capture.rs:51`):

- A `config/agents.toml` entry wins **only if** its program differs from the
  agent id or it carries args (`crates/loomd/src/capture.rs:60`).
- Otherwise the daemon scans `PATH` for an executable with the agent's name
  (`crates/loomd/src/capture.rs:80`).
- The daemon **never** consults the public registry and does **not** accept an
  explicit path.
- `resolve_agent_launch_with_adapter` first tries the tool TOML's `[launch]`
  section (`crates/loomd/src/capture.rs:80`).

A third-party agent that must work under the daemon should therefore be
registered in `config/agents.toml` **with** its protocol-mode args.

#### Running an agent

```bash
loom acp run <agent> --prompt "<text>" [--cwd DIR] [--timeout-secs N] [-- args…]
```

CLI definition `crates/loom-cli/src/cli.rs:1731`; implementation
`crates/loom-cli/src/commands/acp_run.rs:53`. Explicit CLI args **replace** the
resolved args (`crates/loom-cli/src/commands/acp_run.rs:105`).

The daemon's counterpart is the `acp.dispatch` IPC command
(`crates/loomd/src/ipc_handler.rs:549`, implementation
`crates/loomd/src/capture.rs:151`), which additionally records a tracked run
with a `run_id`.

### 2.7 Publishing an agent to the public registry

The registry lives upstream at
<https://github.com/agentclientprotocol/registry> and is vendored in this
repository as `acp_registry/`, committed as a **gitlink** pinned at
`2edbfd245997eae3b19c0b6cba46f86826dc5f44` — but with **no `.gitmodules`
entry**, so a fresh clone does not populate it (see §4 #9). The rules below
are transcribed from that directory's own schema files.

**Per-agent layout** (`acp_registry/CONTRIBUTING.md:15`): a directory named
exactly like the agent id, containing `agent.json` and `icon.svg` (16×16,
monochrome, `currentColor`).

**`agent.json` schema** (`acp_registry/agent.schema.json`):

| Rule | Anchor |
|---|---|
| Required: `id`, `name`, `version`, `description`, `distribution` | `acp_registry/agent.schema.json:7` |
| `id` must match `^[a-z][a-z0-9-]*$` | `acp_registry/agent.schema.json:12` |
| `version` must match `^[0-9]+\.[0-9]+\.[0-9]+` | `acp_registry/agent.schema.json:22` |
| `distribution` accepts **only** `binary`, `npx` or `uvx` (`additionalProperties: false`) | `acp_registry/agent.schema.json:54` |
| A `binary` distribution is keyed by the six platform ids | `acp_registry/agent.schema.json:76` |
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
`acp_registry/acme-agent/agent.json` — **in a fork of
<https://github.com/agentclientprotocol/registry>, not in a Loom working
tree**: `acp_registry/` is a gitlink with no `.gitmodules` entry, so a fresh
Loom clone cannot populate it (see [§2.7](#27-publishing-an-agent-to-the-public-registry)
and §4 #9). The file below is the upstream layout:

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

### 2.9 The ACP dispatch server (agent-to-agent TCP dispatch)

Everything above in §2 is Loom-as-**client**: Loom speaks JSON-RPC 2.0 to an
agent over that agent's stdio. This section is the other direction —
Loom-as-**server**, so that one agent can hand a task to another agent over
the network. It is a *different* protocol from the ACP spec: a **JSON-line
protocol of its own**, over **TCP**, with an **Ed25519 challenge-response
handshake** in front of it. It is not the Agent Client Protocol, and an ACP
agent that speaks JSON-RPC 2.0 cannot talk to it.

The server lives in `crates/loom-sessions/src/acp_server.rs` — module doc
*"ACP Dispatch Server — TCP server for agent-to-agent task dispatch"*
(`crates/loom-sessions/src/acp_server.rs:103`); it provides an Ed25519
challenge-response handshake, the JSON-line dispatch protocol, a health-check
endpoint, and configurable bind address and key management
(`crates/loom-sessions/src/acp_server.rs:103`, `:103`, `:103`, `:103`).

#### 2.9.1 The wire protocol

Newline-delimited JSON — one object per line, in both directions
(`crates/loom-sessions/src/acp_server.rs:341`). Five message types:

| Type | Direction | Shape | Anchor |
|---|---|---|---|
| `handshake` | client → server | `{"type":"handshake","challenge":"<hex>","signature":"<hex>","public_key":"<hex>"}` | `crates/loom-sessions/src/acp_server.rs:299` |
| `handshake_ack` | server → client | `{"type":"handshake_ack","challenge":"<hex>","signature":"<hex>","public_key":"<hex>"}` | `crates/loom-sessions/src/acp_server.rs:319` |
| `dispatch` | client → server | `{"type":"dispatch","request_id":"…","task_id":"…","agent_id":"…","payload":{…}}` | `crates/loom-sessions/src/acp_server.rs:362` |
| `health` | client → server | `{"type":"health","request_id":"…"}` | `crates/loom-sessions/src/acp_server.rs:439` |
| `response` | server → client | `{"type":"response","request_id":"…","status":"ok\|error","data":{…},"error":null}` | `crates/loom-sessions/src/acp_server.rs:345` |

**The handshake comes first, always.** The server reads exactly one line
before anything else and rejects the connection unless it is a `handshake`
(`crates/loom-sessions/src/acp_server.rs:299`, `:299`, `:300` — the error is
*"Expected handshake, got '{msg_type}'"*). The handshake object must carry
`challenge`, `signature` and `public_key`, all hex strings
(`crates/loom-sessions/src/acp_server.rs:309`, `:306`, `:309`). The server
answers with `handshake_ack`
(`crates/loom-sessions/src/acp_server.rs:319`, `:324`).

**After the handshake** the server loops over commands
(`crates/loom-sessions/src/acp_server.rs:333`):

- `dispatch` **requires** `agent_id` and `task_id`; a missing one is an
  error response carrying `"agent_id required"` / `"task_id required"`
  (`crates/loom-sessions/src/acp_server.rs:364`, `:372`, `:377`, `:383`).
  The server looks up a handler registered for that `agent_id`; if there is
  none it falls back to the default handler, and if there is no default
  handler either the error is *"No handler registered for agent
  '{agent_id}'"* (`crates/loom-sessions/src/acp_server.rs:394`, `:406`,
  `:417`). The default handler field is checked at `status()`
  (`crates/loom-sessions/src/acp_server.rs:198`).
- `health` answers `{"status":"healthy","version":<loom version>,"public_key":<hex>}`
  (`crates/loom-sessions/src/acp_server.rs:439`, `:437`).
- Any other type is an error response: *"Unknown command type: '{cmd_type}'"*
  (`crates/loom-sessions/src/acp_server.rs:190`, `:449`).
- A line that is not valid JSON is answered with an error response
  (*"Invalid JSON: …"*) and the connection **stays open**
  (`crates/loom-sessions/src/acp_server.rs:190`, `:348`, `:354`).

Every response is newline-framed
`{"type":"response","request_id","status","data","error"}`
(`crates/loom-sessions/src/acp_server.rs:465`, `:467`, `:474`).

#### 2.9.2 The Ed25519 handshake

The handshake primitives live in `crates/loom-sessions/src/acp_handshake.rs`
— *"ACP secure handshake — Ed25519-based challenge-response
authentication"* (`crates/loom-sessions/src/acp_handshake.rs:81`).

| Item | Anchor |
|---|---|
| `AcpKeyPair` — Ed25519 signing + verifying key | `crates/loom-sessions/src/acp_handshake.rs:61` |
| `AcpKeyPair::generate` (OS RNG) | `crates/loom-sessions/src/acp_handshake.rs:304` |
| `from_seed_hex` (32-byte hex seed) | `crates/loom-sessions/src/acp_handshake.rs:81` |
| `public_key_hex` / `secret_key_hex` | `crates/loom-sessions/src/acp_handshake.rs:100`, `:100` |
| `AcpHandshake` — the exchange state | `crates/loom-sessions/src/acp_handshake.rs:116` |
| `generate_challenge` (32 random bytes, hex) | `crates/loom-sessions/src/acp_handshake.rs:134` |
| `sign_challenge` / `verify_challenge` | `crates/loom-sessions/src/acp_handshake.rs:151`, `:151` |
| `derive_session_key` — HMAC-SHA256 over `"acp-session-key-v1"` | `crates/loom-sessions/src/acp_handshake.rs:182`, `:188` |
| `initiate_handshake` (client side) | `crates/loom-sessions/src/acp_handshake.rs:226` |
| `complete_initiator` | `crates/loom-sessions/src/acp_handshake.rs:245` |
| `perform_responder_handshake` (server side) | `crates/loom-sessions/src/acp_handshake.rs:269` |

The server side of the exchange is
`perform_responder_handshake(&key_pair, challenge, initiator_signature,
initiator_public_key_hex)` (`crates/loom-sessions/src/acp_server.rs:315`,
`crates/loom-sessions/src/acp_handshake.rs:269`): it verifies the initiator's
signature over the challenge — failing with *"Initiator signature
verification failed"* (`crates/loom-sessions/src/acp_handshake.rs:269`) — then
signs the same challenge itself and derives the shared session key
(`crates/loom-sessions/src/acp_handshake.rs:288`, `:291`).

**What the handshake does and does not buy you.** The signature proves the
initiator holds the private key for the public key it presented; the server
does **not** check that key against an allow-list, so a valid Ed25519
signature is sufficient to get a `dispatch` accepted. The derived session key
(`crates/loom-sessions/src/acp_handshake.rs:196`) is **not** used to
authenticate any message on the dispatch server: `verify_message`
(`crates/loom-sessions/src/acp_handshake.rs:196`) has no caller anywhere in
the tree, and the dispatch command loop reads no MAC. The handshake is
therefore a **one-time proof of key possession at connect time only**. The
server also binds to `127.0.0.1` by default (§2.9.3), so the network
exposure is limited unless the bind address is changed.

#### 2.9.3 Configuration, keys and the server object

`AcpServerConfig` (`crates/loom-sessions/src/acp_server.rs:45`):

| Field | Type | Meaning | Anchor |
|---|---|---|---|
| `bind` | string | Address to listen on; default `"127.0.0.1:0"` — loopback, **ephemeral port** | `crates/loom-sessions/src/acp_server.rs:48`, `:58` |
| `secret_key` | string | Ed25519 seed, hex-encoded (32 bytes) | `crates/loom-sessions/src/acp_server.rs:52` |
| `key_path` | string | Where the generated key is persisted | `crates/loom-sessions/src/acp_server.rs:55` |

`AcpDispatchServer::new` (`crates/loom-sessions/src/acp_server.rs:529`)
resolves the key in this order: the `secret_key` hex seed, else the
`key_path` file if it exists, else a **freshly generated** key which is
written to `key_path` when one is set
(`crates/loom-sessions/src/acp_server.rs:125`, `:125`, `:130`, `:136`). The
public key is exposed as `public_key_hex`
(`crates/loom-sessions/src/acp_server.rs:177`).

The server object also offers `register_handler`
(`crates/loom-sessions/src/acp_server.rs:153`), `unregister_handler`
(`:159`), `set_default_handler` (`:166`), `listen_addr` (`:172`),
`uptime_secs` (`:182`), `status` (`:190`), `start` (`:204`) and `stop`
(`:267`). A handler is a `DispatchFn` —
`Arc<dyn Fn(&DispatchRequest) -> Result<Value> + Send + Sync>`
(`crates/loom-sessions/src/acp_server.rs:75`).

`DispatchRequest` (`crates/loom-sessions/src/acp_server.rs:79`) carries
`request_id`, `task_id`, `agent_id` and an arbitrary `payload`
(`crates/loom-sessions/src/acp_server.rs:88`, `:82`, `:84`, `:88`).
`DispatchResponse` (`crates/loom-sessions/src/acp_server.rs:93`) — `request_id`,
`status`, `data: Option<Value>`, `error: Option<String>`
(`crates/loom-sessions/src/acp_server.rs:93`, `:95`, `:96`, `:97`) — is the
Rust-side mirror of the `response` wire message, but it is **not** the type
the server writes: the wire response is built as a `serde_json::json!` value
(`crates/loom-sessions/src/acp_server.rs:469`), and `DispatchResponse` has no
caller outside its own module and the crate's re-export
(`crates/loom-sessions/src/lib.rs:25`).

#### 2.9.4 The daemon wiring

The daemon **auto-starts** the dispatch server. There is no separate service.

- The daemon holds the server in a field
  `acp_server: Arc<RwLock<Option<Arc<loom_sessions::AcpDispatchServer>>>>`
  (`crates/loomd/src/lib.rs:30`), initialised to `None` in `Loomd::new`
  (`crates/loomd/src/lib.rs:64`).
- It is reachable through `acp_server()`
  (`crates/loomd/src/lib.rs:122`) and `acp_server_inner()`
  (`crates/loomd/src/lib.rs:127`).
- On startup the daemon builds the config with
  `key_path: format!("{}/acp_server_key.txt", self.loom_dir)`
  (`crates/loomd/src/lib.rs:240`, `:240`) and creates the server
  (`crates/loomd/src/lib.rs:243`).
- It installs a **default dispatch handler** that reads `payload.prompt` and
  `payload.timeout_secs` (default `120`) and runs the canonical ACP
  `initialize` → `session/new` → `session/prompt` turn through
  `capture::dispatch_acp_protocol` (`crates/loomd/src/lib.rs:264`, `:254`,
  `:259`, `:264`).
- The server is started in a background task (`crates/loomd/src/lib.rs:147`,
  `:286`) and the daemon writes
  `{loom_dir}/acp_server.json` — `listen_addr`, `public_key`, `started_at` —
  once it is listening (`crates/loomd/src/lib.rs:299`, `:304`, `:305`).
- The server reference is stored before the IPC handler starts
  (`crates/loomd/src/lib.rs:122`, `:317`).

**Key material.** Two files in the ecosystem directory:
`{loom_dir}/acp_server_key.txt` — the hex Ed25519 secret seed, written by
`AcpDispatchServer::new` (`crates/loomd/src/lib.rs:243`,
`crates/loom-sessions/src/acp_server.rs:122`) — and
`{loom_dir}/acp_server.json` — the public runtime info
(`crates/loomd/src/lib.rs:305`). **The secret seed is stored in plain text**;
protect that file.

#### 2.9.5 The IPC commands

The daemon's IPC handler holds the same server handle
(`crates/loomd/src/ipc_handler.rs:784`, initialised at
`crates/loomd/src/ipc_handler.rs:39`) and answers three commands:

| Command | Behaviour | Anchor |
|---|---|---|
| `acp.status` | Returns the server's `status()` JSON (`running`, `listen_addr`, `public_key`, `uptime_secs`, `handlers_count`, `handlers`, `has_default_handler`), or `{"running": false, "message": "ACP dispatch server not initialized"}` when the server is absent | `crates/loom-sessions/src/acp_server.rs:198` |
| `acp.start` | **Always an error**: *"ACP dispatch server auto-starts with daemon; use `loom daemon restart` to restart"* | `crates/loomd/src/ipc_handler.rs:534`, `:530` |
| `acp.stop` | Stops the server and returns `{"stopped": true}`, or `{"stopped": false, "message": "ACP dispatch server not running"}` | `crates/loomd/src/ipc_handler.rs:549`, `:535`, `:539`, `:541` |

Because `acp.start` is a hard error, **the only way to bring the server back
is to restart the daemon** (`loom daemon restart`).

#### 2.9.6 The CLI surface

`loom acp …` is the whole ACP CLI group (`crates/loom-cli/src/cli.rs:1866`).
Its subcommands are the `AcpAction` enum
(`crates/loom-cli/src/cli.rs:1866`): `Status` (`:1775`), `Start` (`:1781`),
`Stop` (`:1783`), `Show` (`:1785`), `Agents` (`:1791`), `Registry` (`:1797`),
`Run` (`:1806`) and `RunStatus` (`:1829`). The dispatch
(`crates/loom-cli/src/commands/acp.rs:9`, `:9`, `:10`, `:11`, `:12`, `:13`,
`:18`, `:22`) is reached from the top-level command match
(`crates/loom-cli/src/cli.rs:1866`).

| Command | What it does | Handler anchor |
|---|---|---|
| `loom acp status [--json]` | Sends `acp.status` over the daemon IPC socket and prints the running state | `crates/loom-cli/src/commands/acp.rs:50`, `:51` |
| `loom acp start` | Sends `acp.start` — **fails**, because the server auto-starts with the daemon | `crates/loom-cli/src/commands/acp.rs:120`, `:119` |
| `loom acp stop` | Sends `acp.stop` | `crates/loom-cli/src/commands/acp.rs:131`, `:132` |
| `loom acp show [--json]` | Sends `acp.status` and prints the detailed view (address, public key, uptime, handler list) | `crates/loom-cli/src/commands/acp.rs:158`, `:158` |
| `loom acp agents [--json]` | Lists launchable agents: daemon-registered handlers merged with `config/agents.toml` entries | `crates/loom-cli/src/commands/acp.rs:232` |
| `loom acp registry [--available] [--json]` | Lists the public ACP registry | `crates/loom-cli/src/commands/acp.rs:306` |
| `loom acp run <agent> --prompt …` | One canonical ACP prompt turn ([§2.6](#26-registering-an-acp-agent)) | `crates/loom-cli/src/commands/acp.rs:9` |
| `loom acp run-status <run_id> [--json]` | Shows a tracked run's status | `crates/loom-cli/src/commands/acp.rs:9`, `:403` |

`loom add-acp-agent <agent_id>` is a **separate** top-level command
(`crates/loom-cli/src/cli.rs:155`), not part of `loom acp`.

#### 2.9.7 The remaining session modules

Two more `loom-sessions` modules belong to this surface (the third,
`acp_handshake.rs`, is the handshake itself — see §2.9.2).

**`crates/loom-sessions/src/acp_discovery.rs`** — *"ACP peer discovery — find
ACP agents on the local network"* (`crates/loom-sessions/src/acp_discovery.rs:18`).
`AcpPeer` (`crates/loom-sessions/src/acp_discovery.rs:18`) is a peer record:
`id`, `name`, `address: SocketAddr`, `public_key: Option<String>` (hex
Ed25519), `capabilities`, `last_seen`, `online`
(`crates/loom-sessions/src/acp_discovery.rs:33`, `:22`, `:24`, `:26`, `:29`,
`:31`, `:33`). `PeerDiscovery` (`crates/loom-sessions/src/acp_discovery.rs:54`)
is the local peer table: `register_peer` (`:66`), `remove_peer` (`:80`),
`get_peers` (`:86`), `get_online_peers` (`:92`), `health_check_peer` (`:98`),
`health_check_all` (`:119`), `prune_offline_peers` (`:132`),
`load_from_file` (`:144`), `save_to_file` (`:152`). The health check is a
plain TCP connect with a 3-second timeout
(`crates/loom-sessions/src/acp_discovery.rs:152`, `:163`).

**Caveat:** mDNS discovery is described in the module doc
(`crates/loom-sessions/src/acp_discovery.rs:33`) but **no `mdns` feature
exists** in `crates/loom-sessions/Cargo.toml` and no mDNS code is present —
the module doc overstates the implementation. And nothing in the tree
constructs a `PeerDiscovery`; the module is exported
(`crates/loom-sessions/src/lib.rs:25`, `:25`) but unused. Treat peer discovery
as **not yet a usable extension point**.

**`crates/loom-sessions/src/agent_provider.rs`** — the `AgentProvider` trait
(`crates/loom-sessions/src/agent_provider.rs:5`): `name`
(`crates/loom-sessions/src/agent_provider.rs:7`),
`discover_features` (`:9`), `create_session` (`:11`) and `validate` (`:13`).
`FeatureMetadata` (`:18`), `SessionHandle` (`:27`) and the
`SimpleAgentProvider` implementation (`:35`, `:42`, `:55`) come with it.
`SessionManager` keeps a map of custom agents
(`crates/loom-sessions/src/lib.rs:44`) filled by `add_custom_agent`
(`crates/loom-sessions/src/lib.rs:64`) and read back through
`custom_agents()` (`crates/loom-sessions/src/lib.rs:80`).

**Caveat:** `add_custom_agent` has **no caller** anywhere in the tree, and
`custom_agents()` is likewise unused, so the `AgentProvider` trait is a
declared extension point that no Loom code path currently consumes.

#### 2.9.8 What a third party must do

To use the dispatch server as a client:

1. Get the server's public key and address — from `{loom_dir}/acp_server.json`
   (written by the daemon, `crates/loomd/src/lib.rs:305`) or from
   `loom acp show --json` (`crates/loom-cli/src/commands/acp.rs:158`).
2. Connect over TCP and send the `handshake` line
   (`crates/loom-sessions/src/acp_server.rs:124`). Use
   `initiate_handshake` (`crates/loom-sessions/src/acp_handshake.rs:226`) to
   produce `challenge` + `signature`, or implement the same two-step
   Ed25519 exchange by hand.
3. Read the `handshake_ack` line (`crates/loom-sessions/src/acp_server.rs:319`)
   and verify the server's signature over the challenge with
   `complete_initiator` (`crates/loom-sessions/src/acp_handshake.rs:245`).
4. Send `dispatch` lines (`crates/loom-sessions/src/acp_server.rs:362`) and
   correlate each `response` by `request_id`
   (`crates/loom-sessions/src/acp_server.rs:469`). `agent_id` and `task_id`
   are mandatory (`crates/loom-sessions/src/acp_server.rs:364`, `:377`).
5. `health` (`crates/loom-sessions/src/acp_server.rs:439`) checks liveness.

The daemon's default handler runs the canonical ACP turn, so the `payload`
it understands is `{"prompt": "<text>", "timeout_secs": <n>}`
(`crates/loomd/src/lib.rs:259`, `:259`) — the `agent_id` selects the agent
program through the daemon's own resolver
([§2.6](#26-registering-an-acp-agent)), and `task_id` is recorded with the
run.

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
- The ACP dispatch-server wire contract (§2.9): newline-delimited JSON over
  TCP, the `handshake` → `handshake_ack` → `dispatch` / `health` → `response`
  order, and the `dispatch` request fields `request_id`, `task_id`, `agent_id`,
  `payload`.
- The `{loom_dir}` layout for tool files and agent files.

**May change without a spec-version bump:**

- New optional TOML sections and new optional fields inside existing sections
  (they parse today, because unknown keys are ignored —
  `crates/loom-generic-adapter/src/config.rs:34`).
- New `sessionUpdate` kinds (ignored today —
  `crates/loom-sessions/src/acp_protocol.rs:156`).
- New `runner` values in `config/agents.toml` (unknown values currently fall
  back to "the agent id is the program" —
  `crates/loom-sessions/src/acp_launch.rs:275`).
- The set of built-in preset adapters.
- The registry contents and the `loom acp …` CLI surface.

**Requires a spec-version bump:**

- Renaming or removing a TOML section, or removing/renaming a field.
- Changing the ACP request order, the required result fields, or the permission
  answer shape.
- Changing the dispatch-server wire contract: the message types, their
  required fields, the handshake order, or the `dispatch` request fields
  (§2.9.1).
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
| 1 | `crates/loom-sessions/src/acp_protocol.rs:319` | The file header still says *"NOTE (TDD RED step): tests only — `AcpProtocolClient` and friends do not exist yet."* The implementation exists at `:201`. |
| 2 | `crates/loom-sessions/src/acp_protocol.rs:319` | The comment says `session/cancel` is a notification, but it is built by `build_request` (`:99`) which always emits an `id` — it is a request. |
| 3 | `crates/loom-sessions/src/acp_launch.rs:22` | The module doc says *"Three sources"*; there are four (the explicit-path branch at `:345` is not mentioned). |
| 4 | `crates/loom-sessions/src/acp_launch.rs:344` | The doc says the local entry keeps the common case *"stays offline"*, but `:355` always performs a registry fetch. `reqwest::Client::new()` (`crates/loom-sessions/src/acp_client.rs:38`) sets no timeout, so a hung CDN stalls the launch. |
| 5 | `crates/loom-sessions/src/acp_protocol.rs:30` | `cwd` is documented as the *"Session working directory (absolute)"* but is only echoed into `session/new`; the child process is never chdir'd (no `Command::current_dir()` in the file). The persisted `working_directory` is Loom's cwd, not the agent's (`:519`). |
| 6 | `crates/loom-sessions/src/acp_launch.rs:214` | `to_toml_entry` documents itself as rendering *the* `agents.toml` entry, but renders a partial one — `source` is added separately (`crates/loom-cli/src/commands/tools_acp.rs:56`). |
| 7 | `crates/loom-cli/src/commands/tools_acp.rs:56` | `source = "acp-registry"` is written but never read by any Loom code path. |
| 8 | `crates/loom-sessions/src/acp_client.rs:37` | `AgentMetadata` has no `version` field, although the registry schema requires one (`acp_registry/agent.schema.json:7`); `repository`, `website`, `authors`, `license`, `icon` are dropped too. `auth_methods` / `platforms` / `tags` are parsed and never used, so the registry's authentication guarantee (`acp_registry/AUTHENTICATION.md:7`) is not surfaced by Loom. |
| 9 | `acp_registry/` | The directory is committed as a gitlink with **no `.gitmodules` entry**: `git submodule status` fails with *"fatal: no submodule mapping found in .gitmodules for path 'acp_registry'"*. A fresh clone cannot populate it, so the registry schemas are unavailable to contributors. |
| 10 | `crates/loom-cli/src/commands/acp_run.rs:53` | `--runtime` is advertised in the CLI help as *"Runtime backend: process, docker, podman, kubernetes"* (`crates/loom-cli/src/cli.rs:1065`) but is never implemented — it warns *"runtime '…' does not support the ACP protocol yet; running directly"* and runs directly. |
| 11 | `crates/loomd/src/capture.rs:151` | `complete_run(&run_id, 0, …)` hardcodes exit code `0`, so `loom acp run-status` shows `Exit: 0` even for a failed agent; and because the error path returns before `complete_run`, a failed dispatch leaves the run "still running" forever. |
| 12 | `crates/loom-core/src/config.rs:200` | `config/acp.toml`'s `registry_url` is parsed and displayed by `loom config show` (`crates/loom-cli/src/commands/config.rs:110`) but **never used to fetch** — every fetch site uses the hard-coded `ACP_REGISTRY_URL` (`crates/loom-sessions/src/acp_launch.rs:317`, `crates/loom-cli/src/commands/tools_acp.rs:3`, `crates/loom-cli/src/commands/acp.rs:310`). The documented "ACP registry URL" setting is dead for registry access. |
| 13 | `crates/loom-generic-adapter/src/lib.rs:126` | `present_features` / `absent_features` cover only the six trait sections; `[lsp]`, `[debug]` and `[launch]` are never reported. `GenericAdapter` does not implement the `LspProvider` / `DebugProvider` core traits (`crates/loom-core/src/types.rs:1308`, `:984`), so a TOML-registered tool's `[lsp]` and `[debug]` sections are parsed but unused by `loom lsp exec` and `loom debug`, which resolve tools through `adapter_for` only (`crates/loom-cli/src/commands/lsp.rs:89`). |
| 14 | `crates/loom-generic-adapter/src/traits.rs:83` | The generic trait implementations are documented as "no-ops", and they are: `GenericMcpConsumer` never writes the tool's MCP config, `GenericHookExecutor` never executes a hook, `GenericAuthProvider::export_credential` always errors. A `[mcp]` / `[hooks]` / `[auth]` section in a hand-written TOML therefore declares intent that the generic runtime does not fulfil. |
| 15 | `crates/loom-auth/src/import.rs:19` | `loom-auth` reads tool TOMLs with a *different* schema (`[auth]`/`[provider]` with `provider_id`, `credential`, …) than `ToolToml`'s `[auth]` section (`store_path` / `providers` / `import_on_register`). The two coexist in one file and neither documents the other. |
| 16 | `AGENTS.md` | The agent guide omits `config/agents.toml` and `config/security.toml` from the filesystem layout, omits `--runtime` from the ACP command list, does not say `loom add-acp-agent` needs network, and does not distinguish the daemon resolver (`crates/loomd/src/capture.rs:51`) from the CLI resolver (`crates/loom-sessions/src/acp_launch.rs:344`). |
| 17 | `ROADMAP.md:75` | The pillar-3 promise — *"publish the `acp_registry` schemas"* — cannot be satisfied from this repository alone, because of #9: the schema directory is an unpopulatable gitlink. This document restates the schema rules inline (§2.7) so the promise is at least satisfiable in prose. |
| 18 | `crates/loom-cli/src/commands/thread.rs:40` | `loom thread <tool>` for a tool name that is not one of the 12 presets **overwrites** `{loom_dir}/tools/<tool>.toml` with a minimal `[tool]`-only file (`:54`), destroying any hand-written sections. The CLI reports *"✓ Updated tool"*, not *"replaced"*. A third-party tool therefore must not be registered with `loom thread` after its TOML is written — but nothing in the CLI or `AGENTS.md` says so. |
| 19 | `crates/loomd/src/ipc_handler.rs:386` | The daemon's `register_tool` handler writes a registration file **only for preset tools** (`:382`). For a third-party tool it returns `{"tool": …, "features": []}` (`:397`) and writes nothing, so notifying the daemon about a hand-written tool has no effect on disk. The CLI's `loom thread` path behaves the opposite way (it writes, then notifies), so the two registration paths disagree. |
| 20 | `crates/loom-generic-adapter/src/config.rs:200` | `command` is a required field in three places — `[launch].command` (`:247`), `[lsp].servers[].command` (`:200`) and `[debug].adapters[].command` (`:228`) — so an LSP or debug server entry without a command makes the **whole tool TOML unparseable**, and triggers a `warn` — the scanner then skips the file entirely (`crates/loom-generic-adapter/src/scanner.rs:53`). |
| 21 | `crates/loom-cli/src/commands/exec.rs:362` | The two consumers of a routing decision disagree. `loom exec` reads `LOOM_ROUTING_PROVIDER` and writes it into the `{TOOL}_MODEL` slot (`:362`) — the **provider** string lands in the *model* variable, and the recommended model is never propagated. `loom sessions resume` instead forwards `{TOOL}_MODEL` and resolves the provider separately (`crates/loom-cli/src/commands/resume.rs:329`, `:350`, `:356`). Same decision, two different meanings. |
| 22 | `crates/loomd/src/ipc_handler.rs:534` | `acp.start` is a hard error (*"ACP dispatch server auto-starts with daemon"*, `:530`) because the daemon starts the server itself (`crates/loomd/src/lib.rs:147`). The CLI still advertises the subcommand (`crates/loom-cli/src/cli.rs:1065`, `crates/loom-cli/src/commands/acp.rs:120`), so `loom acp start` always fails and the only way back is `loom daemon restart`. |
| 23 | `crates/loom-sessions/src/acp_discovery.rs:33` | The module doc advertises mDNS discovery, but the crate has **no `mdns` feature** (`crates/loom-sessions/Cargo.toml`) and no mDNS code; the peer table is a plain in-process `HashMap` (`crates/loom-sessions/src/acp_discovery.rs:54`). Nothing constructs a `PeerDiscovery` — the module is exported (`crates/loom-sessions/src/lib.rs:25`, `:25`) and otherwise unused. The same is true of the `AgentProvider` trait: `add_custom_agent` (`crates/loom-sessions/src/lib.rs:64`) has no caller, so the declared extension point consumes nothing. |
| 24 | `crates/loomd/src/lib.rs:243` | The dispatch server's Ed25519 secret seed is written to `{loom_dir}/acp_server_key.txt` as **plain hex, unencrypted** (`crates/loom-sessions/src/acp_server.rs:122`), and the session key derived during the handshake (`crates/loom-sessions/src/acp_handshake.rs:182`) authenticates nothing: `verify_message` (`:195`) has no caller and the dispatch command loop reads no MAC. The handshake is a one-time proof of key possession at connect time; the server also accepts any key that presents a valid signature — there is no allow-list. |

---

## 5. Anchor index

**`crates/loom-sessions/src/acp_protocol.rs`** — `ACP_PROTOCOL_VERSION`: `20`; `initialize`: `425`; `protocolVersion`: `365`; `line`: `123`; `obj`: `123`; `Invalid`: `96`; `acp_protocol`: `96`; `to_string`: `96`; `line`: `96`; `run_prompt`: `707`; `false`: `398`; `new`: `337`; `sessionId`: `466`; `cwd`: `452`; `prompt`: `475`; `stopReason`: `505`; `sessionId`: `486`; `acp_protocol`: `433`; `prompt`: `215`; `update`: `505`; `sessionUpdate`: `153`; `text`: `160`; `status`: `180`; `sessionUpdate`: `687`; `request_permission`: `277`; `permission_outcome`: `249`; `cancelled`: `269`; `AcpTranscript`: `39`; `status`: `39`; `build_acp_session`: `536`; `hex`: `472`; `acp_protocol`: `566`; `reason`: `478`; `agent`: `597`; `line`: `276`; `AcpProtocolClient`: `276`; `acp_client`: `30`.
**`crates/loom-core/src/constants.rs`** — `ECOSYSTEM_VERSION`: `3`; `loom`: `4`; `TOOLS_DIR`: `10`.
**`Cargo.toml`** — `lib`: `34`.
**`crates/loom-adapters/src/lib.rs`** — `Sync`: `38`; `line`: `24`; `adapter_for`: `425`; `code`: `429`; `supported_tools`: `24`; `str`: `38`; `toml`: `291`; `ToolToml`: `38`; `Result`: `39`; `SyncReport`: `40`; `SyncReport`: `236`; `SyncReport`: `46`; `sync_mcp_to_native`: `172`; `sync_skills_to_native`: `235`; `LspSection`: `95`; `LspServerInfo`: `100`; `LspServerInfo`: `117`; `DebugSection`: `124`; `DebugAdapterInfo`: `129`; `DebugAdapterInfo`: `146`; `String`: `154`; `write_tool_registration`: `401`.
**`crates/loom-core/src/lib.rs`** — `home`: `15`; `loom_dir`: `25`.
**`crates/loom-core/src/types.rs`** — `recommended_model`: `609`; `LspProvider`: `1308`; `DebugProvider`: `1308`.
**`crates/loom-adapters/src/aider.rs`** — `AIDER_MODEL`: `52`.
**`crates/loom-adapters/src/codex.rs`** — `CODEX_MODEL`: `52`.
**`crates/loom-adapters/src/cursor_cli.rs`** — `CURSOR_MODEL`: `52`.
**`crates/loom-adapters/src/gemini_cli.rs`** — `GEMINI_MODEL`: `52`.
**`crates/loom-adapters/src/github_copilot.rs`** — `COPILOT_MODEL`: `56`.
**`crates/loom-adapters/src/goose.rs`** — `GOOSE_MODEL`: `52`.
**`crates/loom-adapters/src/jcode.rs`** — `ANTHROPIC_MODEL`: `78`.
**`crates/loom-adapters/src/juno.rs`** — `JUNO_MODEL`: `60`.
**`crates/loom-adapters/src/kiro.rs`** — `KIRO_MODEL`: `61`.
**`crates/loom-adapters/src/opencode.rs`** — `OPENCODE_MODEL`: `52`.
**`crates/loom-adapters/src/claude_code.rs`** — `LOOM_ROUTING_PROVIDER`: `47`.
**`crates/loom-adapters/src/crush.rs`** — `curl`: `58`; `CRUSH_MODEL`: `72`.
**`crates/loom-cli/src/commands/resume.rs`** — `line`: `332`; `resume`: `332`; `_MODEL`: `332`.
**`crates/loom-cli/src/commands/exec.rs`** — `resume`: `273`; `exec`: `360`; `_MODEL`: `360`; `exec`: `730`; `lsp`: `360`.
**`crates/loom-cli/src/commands/lsp.rs`** — `lsp_servers`: `89`; `adapter_for`: `89`.
**`crates/loom-generic-adapter/src/config.rs`** — `ToolToml`: `34`; `tool`: `36`; `String`: `69`; `skills`: `39`; `mcp`: `42`; `auth`: `45`; `hooks`: `48`; `routing`: `51`; `sessions`: `54`; `lsp`: `57`; `debug`: `60`; `launch`: `63`; `deny_unknown_fields`: `23`; `import_on_register`: `34`; `line`: `23`; `thread`: `200`.
**`crates/loom-generic-adapter/src/lib.rs`** — `from_str`: `42`; `line`: `26`; `SkillProvider`: `19`; `McpConsumer`: `20`; `AuthProviderTrait`: `21`; `HookExecutor`: `22`; `RoutingTarget`: `23`; `SessionExportable`: `24`; `absent_features`: `102`; `launch_config`: `156`; `new`: `38`; `acp`: `102`.
**`crates/loom-generic-adapter/src/traits.rs`** — `None`: `178`; `remove_mcp_server`: `76`; `execute_hook`: `116`; `discover_providers`: `95`; `export_session`: `149`; `skills`: `184`; `Mid`: `219`; `lsp`: `76`.
**`crates/loomd/src/ipc_handler.rs`** — `register_tool`: `392`; `register_tool`: `377`; `dispatch`: `549`; `line`: `28`; `ipc_handler`: `39`; `initialized`: `487`; `restart`: `532`; `running`: `537`; `AGENTS`: `386`; `resume`: `532`.
**`crates/loom-workflow/src/dispatch.rs`** — `exec`: `31`.
**`crates/loom-tui/src/dispatch.rs`** — `dispatch`: `628`.
**`crates/loomd/src/capture.rs`** — `dispatch`: `88`; `launch`: `88`; `acp_protocol`: `242`; `resolve_agent_launch`: `51`; `toml`: `60`; `PATH`: `88`; `ipc_handler`: `151`; `cli`: `242`; `agent`: `51`.
**`crates/loom-cli/src/cli.rs`** — `tool`: `60`; `plugin_registry`: `262`; `acp_protocol`: `1868`; `line`: `154`; `cli`: `1935`; `line`: `1935`; `acp`: `1867`; `AcpAction`: `1867`; `acp`: `1924`; `agent_id`: `154`; `runtime`: `19`; `lib`: `19`.
**`crates/loom-cli/src/commands/thread.rs`** — `sync_from_native`: `28`; `tool`: `40`; `thread`: `55`; `register_tool`: `60`; `acp_registry`: `40`.
**`crates/loom-core/src/plugin_registry.rs`** — `tools`: `171`.
**`crates/loom-generic-adapter/src/scanner.rs`** — `tools`: `27`; `scanner`: `53`; `import_on_register`: `69`; `command`: `53`.
**`crates/loom-auth/src/import.rs`** — `provider_id`: `13`; `supported_tools`: `13`; `auth`: `13`.
**`crates/loom-sessions/src/store.rs`** — `jsonl`: `42`.
**`crates/loom-cli/src/commands/acp_run.rs`** — `store`: `127`; `tool_calls`: `151`; `cli`: `88`; `acp_run`: `105`; `status`: `53`.
**`crates/loom-sessions/src/acp_launch.rs`** — `toml`: `263`; `Table`: `262`; `PATH`: `374`; `package`: `273`; `args`: `274`; `env`: `275`; `foo`: `420`; `registry`: `22`; `binary`: `140`; `x86_64`: `71`; `agent_id`: `344`; `line`: `367`; `toml`: `354`; `acp_launch`: `354`; `args`: `359`; `PATH`: `382`; `toml`: `302`; `build_request`: `3`; `acp_launch`: `335`; `working_directory`: `214`; `ACP_REGISTRY_URL`: `317`; `capture`: `344`.
**`crates/loom-cli/src/commands/init.rs`** — `toml`: `200`.
**`crates/loom-cli/src/commands/tools_acp.rs`** — `registry`: `65`; `cli`: `13`; `line`: `14`; `tools_acp`: `22`; `tools_acp`: `64`; `toml`: `47`; `source`: `65`; `tools_acp`: `65`; `acp_launch`: `14`.
**`crates/loom-sessions/src/acp_client.rs`** — `json`: `58`; `agents`: `25`; `new`: `38`; `registry`: `10`.
**`acp_registry/CONTRIBUTING.md`** — `line`: `15`.
**`acp_registry/agent.schema.json`** — `distribution`: `7`; `json`: `12`; `version`: `22`; `false`: `54`; `binary`: `76`; `cmd`: `91`; `package`: `121`; `version`: `7`.
**`acp_registry/registry.schema.json`** — `agents`: `7`.
**`acp_registry/AUTHENTICATION.md`** — `json`: `7`; `tags`: `7`.
**`crates/loom-sessions/src/acp_server.rs`** — `acp_server`: `1`; `acp_server`: `4`; `line`: `9`; `hex`: `29`; `hex`: `319`; `payload`: `207`; `request_id`: `435`; `null`: `344`; `handshake`: `296`; `public_key`: `303`; `handshake_ack`: `317`; `line`: `332`; `required`: `366`; `agent_id`: `394`; `hex`: `435`; `acp_server`: `190`; `error`: `460`; `initiator_public_key_hex`: `315`; `AcpServerConfig`: `45`; `bind`: `48`; `secret_key`: `52`; `key_path`: `55`; `new`: `529`; `key_path`: `123`; `public_key_hex`: `177`; `register_handler`: `153`; `Sync`: `75`; `DispatchRequest`: `79`; `payload`: `81`; `DispatchResponse`: `93`; `String`: `94`; `json`: `467`; `lib`: `296`; `handshake`: `29`; `handshake_ack`: `319`; `dispatch`: `207`; `request_id`: `467`; `task_id`: `366`; `health`: `435`; `txt`: `296`.
**`crates/loom-sessions/src/acp_handshake.rs`** — `acp_handshake`: `1`; `AcpKeyPair`: `61`; `generate`: `304`; `from_seed_hex`: `81`; `secret_key_hex`: `100`; `AcpHandshake`: `116`; `generate_challenge`: `134`; `verify_challenge`: `151`; `key`: `195`; `initiate_handshake`: `226`; `complete_initiator`: `245`; `perform_responder_handshake`: `269`; `acp_server`: `269`; `acp_handshake`: `269`; `acp_handshake`: `288`; `dispatch`: `195`; `verify_message`: `195`; `acp_server`: `195`.
**`crates/loom-sessions/src/lib.rs`** — `DispatchResponse`: `18`; `PeerDiscovery`: `18`; `SessionManager`: `41`; `add_custom_agent`: `64`; `custom_agents`: `80`.
**`crates/loomd/src/lib.rs`** — `AcpDispatchServer`: `30`; `new`: `64`; `acp_server`: `122`; `acp_server_inner`: `127`; `loom_dir`: `239`; `lib`: `243`; `dispatch_acp_protocol`: `264`; `lib`: `4`; `started_at`: `296`; `lib`: `122`; `new`: `243`; `json`: `305`; `timeout_secs`: `254`; `start`: `4`.
**`crates/loom-cli/src/commands/acp.rs`** — `RunStatus`: `7`; `status`: `50`; `start`: `118`; `stop`: `131`; `status`: `157`; `toml`: `231`; `json`: `304`; `prompt`: `7`; `json`: `7`; `json`: `157`; `tools_acp`: `310`; `cli`: `118`.
**`crates/loom-sessions/src/acp_discovery.rs`** — `acp_discovery`: `18`; `AcpPeer`: `18`; `online`: `20`; `PeerDiscovery`: `52`; `save_to_file`: `161`; `line`: `6`; `restart`: `6`; `HashMap`: `52`.
**`crates/loom-sessions/src/agent_provider.rs`** — `AgentProvider`: `5`; `name`: `7`.
**`crates/loom-core/src/config.rs`** — `complete_run`: `200`.
**`crates/loom-cli/src/commands/config.rs`** — `show`: `8`.
**`ROADMAP.md`** — `License`: `75`.
