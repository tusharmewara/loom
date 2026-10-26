# Adapter contribution guide

How to register an agent tool you're wrapping with the Loom ecosystem. Two
routes:

| | TOML registration | Rust preset adapter |
|---|---|---|
| Code required | None | A ~60-line Rust module |
| Where it lives | `<loom-dir>/tools/{name}.toml` | `crates/loom-adapters/src/` |
| Suits | CLI with stable flags, config paths, standard dirs | Conditional command construction, streaming quirks, custom session or routing logic |

**Decision rule: TOML first.** Pick Rust only when the tool needs behavior the
TOML schema cannot express — dynamic command assembly, protocol-level hacks
(AC mode flags, health probes), or code-level routing hooks. A TOML file is
picked up by any running Loom (CLI, daemon, TUI) with no recompile; a Rust
preset ships with the repo and needs a PR.

`<loom-dir>` throughout = `LOOM_AGENTS_DIR` if set, else `~/.loom/`. In code
this is `loom_core::loom_dir()` — never hardcode `~/.loom` (CI and worktrees
rely on the env override).

---

## Route 1 — TOML (no code)

### 1. Create the file

```bash
loom thread mytool
```

Creates `<loom-dir>/tools/mytool.toml` containing just `[tool]` with
`name = "mytool"`, `version = "1.0.0"`. Every other section is absent — that is
fine: absent sections become **no-op implementations** and never error.

> ⚠ `loom thread <tool>` **overwrites** the file on re-run (it cannot know
> which sections you added). Write your full TOML now; don't re-run `loom
> thread` afterward unless you want the reset.

### 2. Fill in the sections you need

Every section maps 1:1 to a Loom trait:

```toml
# [tool] is REQUIRED. Missing [tool] fails TOML parse.
[tool]
name = "mytool"            # display name / registration id
version = "1.0.0"

# [skills] → SkillProvider. Tilde paths are expanded.
[skills]
native_dir = "~/.mytool/skills/"   # tool's own skill dir; default: <loom-dir>/skills
import_on_register = true          # pull these skills into the ecosystem at registration
shared_dir = "~/.loom/skills/"     # optional: also expose this shared dir
extension = "md"                   # file extension filter (default "md")

# [mcp] → McpConsumer
[mcp]
config_path = "~/.mytool/mcp.json" # tool's native MCP config (JSON)
shared_servers = ["filesystem"]    # server IDs this tool shares with the ecosystem
import_on_register = true          # import ecosystem servers at registration
import_tags = []                   # optional tag filter on import

# [auth] → AuthProviderTrait
[auth]
store_path = "~/.mytool/auth.json" # tool's local credential store
providers = []                     # provider IDs to register with the ecosystem
import_on_register = false

# [hooks] → HookExecutor
[hooks]
scripts_dir = "~/.mytool/hooks/"   # directory containing hook scripts
register_on_start = false          # register all scripts in scripts_dir at startup
default_timeout_seconds = 30       # per-hook timeout (default 30)

# [routing] → RoutingTarget
[routing]
default_tier = "mid"               # "slm" | "mid" | "flagship" (default "mid")
catalog_path = "~/.mytool/models.json"  # optional: tool's own model catalog
strategy = "balanced"              # routing strategy hint (default "balanced")

# [sessions] → SessionExportable
[sessions]
store_path = "~/.mytool/sessions/" # tool's session storage
auto_export = true                 # export sessions to the shared index
max_shared_sessions = 50           # recent sessions kept in shared index (default 50)
```

Three non-trait sections are also understood:

```toml
# [lsp] — LSP servers this tool uses (loom lsp exec / loom lsp find-references)
[lsp]
servers = [{ language = "rust", command = "rust-analyzer", args = [], file_extensions = ["rs"] }]

# [debug] — DAP debug adapters (loom debug launch / attach)
[debug]
adapters = [{ language = "rust", command = "lldb-dap", args = [] }]

# [launch] — how loom exec / workflow dispatch runs the tool
[launch]
runtime = "process"        # "process" (default), "docker", "podman"
image = "mytool:latest"    # container runtimes only
command = "/usr/local/bin/mytool"
args = ["serve"]
env = { MYTOOL_LOG = "debug" }
# network = "none"          # container runtimes: none (default) | restricted | unrestricted
auto_remove = true          # container runtimes: remove container after run
```

Everything absent = no-op. A TOML with only `[tool]` registers a working but
silent tool.

### 3. Validate

```bash
loom init --import   # once, if the ecosystem tree does not exist yet
loom scan
```

Expected (tools row goes from `empty` to `active`):

```
+-------------+-------+--------+
| Layer       | Count | Status |
+==============================+
| mcp_servers | 0     | empty  |
|-------------+-------+--------|
| skills      | 0     | empty  |
|-------------+-------+--------|
| providers   | 0     | empty  |
|-------------+-------+--------|
| sessions    | 0     | empty  |
|-------------+-------+--------|
| tools       | 1     | active |
+-------------+-------+--------+
```

`loom status` shows the same `tools` count in its summary table.
`loom debug adapters` lists DAP adapters on PATH only — it does **not** list
tool registrations; `loom scan` is the smoke check.

First-time wipe-safe loop (vanilla macOS/Linux):

```bash
export LOOM_AGENTS_DIR=$(mktemp -d)
loom init --import && loom thread mytool && loom scan
```

Because `LOOM_AGENTS_DIR` is the single source of truth for both CLI and
daemon, everything above exercises the real registration path — nothing
touches a genuine `~/.loom`.

### Reading it programmatically

Registering from outside Rust: `loom-ffi` exposes C-ABI registration exports
that write the same TOML this guide covers — `loom_register_tool`,
`loom_register_skill`, `loom_unregister_tool` (JSON payload string in, JSON
reply string out).

Python example — validated inline during preflight (run
`cargo build -p loom-ffi` first for `target/debug/libloom_ffi.dylib` on macOS /
`.so` on Linux). Two rules bite if you skip them:
`restype` must be a 64-bit pointer type (the default `c_int` truncates the
pointer and segfaults), and the loom string convention returns a
**boxed `CString` struct** — read the char pointer from its first field, copy
the text, then free the outer box with `loom_free_string`:

```python
import ctypes, json
lib = ctypes.CDLL("target/debug/libloom_ffi.dylib")
for f in ("loom_register_tool", "loom_register_skill", "loom_unregister_tool"):
    getattr(lib, f).argtypes = [ctypes.c_char_p]
    getattr(lib, f).restype = ctypes.c_void_p
lib.loom_free_string.argtypes = [ctypes.c_void_p]
lib.loom_free_string.restype = None

def call(fn, payload):
    raw = getattr(lib, fn)(payload.encode())          # *mut CString box
    inner = ctypes.cast(raw, ctypes.POINTER(ctypes.c_void_p)).contents.value
    text = ctypes.cast(inner, ctypes.c_char_p).value.decode()
    lib.loom_free_string(raw)
    return json.loads(text)

print(call("loom_register_tool", json.dumps({"name": "pytool"})))
# {'ok': True, 'toml_path': '<loomdir>/tools/pytool.toml', 'updated': False}
print(call("loom_register_skill", json.dumps(
    {"name": "pyskill", "content": "---\ndescription: demo\n---\n# pyskill"})))
print(call("loom_unregister_tool", "pytool"))   # {'ok': True, 'removed': ...}
print(call("loom_unregister_tool", "pytool"))   # {'ok': False, 'error': "...not found..."}
```

`loom scan` (tools row) and `loom skills list` show the result — the same
registration path the CLI uses, reached from Python. Skill content needs
`description:` frontmatter (skills are dropped silently at load without it —
the FFI surface refuses up front to keep the failure loud).

`loom_generic_adapter::GenericAdapter::new(loom_dir, "mytool")` parses the
TOML and reports which features are present/absent (`present_features()` /
`absent_features()`), so tests can assert section presence without filesystem
effects.

---

## Route 2 — Rust preset adapter (`crates/loom-adapters`)

Only for behavior TOML cannot express. Smallest existing example:
[`aider.rs`](../crates/loom-adapters/src/aider.rs) — 60 lines:

```rust
use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    McpSection, RoutingSection, SessionsSection, SkillsSection, ToolSection, ToolToml,
};

pub struct MyToolAdapter;

impl LoomToolAdapter for MyToolAdapter {
    fn tool_name(&self) -> &'static str { "mytool" }

    /// The preset TOML written by `loom thread mytool`. `home` = $HOME.
    fn default_toml(&self, home: &str) -> ToolToml {
        ToolToml {
            tool: ToolSection { name: "mytool".into(), version: "1.0.0".into() },
            // Fill the sections your tool actually has; None everywhere else.
            skills: Some(SkillsSection {
                native_dir: Some(format!("{home}/.mytool/skills")),
                import_on_register: true, shared_dir: None, extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{home}/.mytool/mcp.json")),
                shared_servers: vec![], import_on_register: true, import_tags: vec![],
            }),
            auth: None, hooks: None,
            routing: Some(RoutingSection {
                default_tier: "mid".into(), catalog_path: None, strategy: "balanced".into(),
            }),
            sessions: Some(SessionsSection {
                store_path: Some(format!("{home}/.mytool/sessions")),
                auto_export: true, max_shared_sessions: 50,
            }),
            lsp: None, debug: None, launch: None,
        }
    }

    /// Delivered when a routing decision should steer this tool.
    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        std::env::set_var("MYTOOL_MODEL", &decision.recommended_model);
        Ok(())
    }

    /// Pull the tool's own resources into the ecosystem.
    fn sync_from_native(&self, loom_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(loom_dir)
    }
}
```

`LoomToolAdapter` has four required methods (`tool_name`, `default_toml`,
`apply_routing_decision`, `sync_from_native`) plus default methods
(`sync_to_native`, `lsp_servers`, `debug_adapters`, `apply_hashline_edit`) you
inherit unless you override them.

Registration — two places, keep them in sync:

- `crates/loom-adapters/src/lib.rs`: add your module, an arm in
  `adapter_for()` (`"mytool" => Some(Box::new(my_tool::MyToolAdapter))`), and
  the name in `supported_tools()`.
- After that, `loom thread mytool` writes your preset TOML automatically.

Dispatch lookup for `loom exec` checks PATH in this order (internal detail,
not a stable API): `opencode`, `claude-code`, `jcode`, `crush`. A new preset
does not change this order — the TerminalLaunch dispatcher is the consumer of
`adapter_for`, not a registry.

`packages/{tool}/` (see `packages/jcode/`): a placeholder crate re-exporting
Loom subsystems (`loom_core`, `loom_mcp`, …) for external consumers. The real
adapter logic always lives in `crates/loom-adapters/src/`; add a
`packages/jcode/`-style stub only if your package needs a standalone crate
entry (copy its `Cargo.toml` and `src/lib.rs`).

### Test template

Inline `#[cfg(test)]` in the crate (or the pattern used in
`crates/loom-cli/src/commands/thread.rs`), `tempfile::tempdir()` only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_toml_round_trips_through_the_parser() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();
        // The convention: registration writes valid TOML with the sections advertised.
        let adapter = MyToolAdapter;
        let toml = toml::to_string_pretty(
            adapter.default_toml(dir.path().to_str().unwrap()),
        ).unwrap();
        let parsed: loom_generic_adapter::config::ToolToml = toml::from_str(&toml).unwrap();
        assert!(parsed.skills.is_some(), "preset claims [skills]");
        assert!(parsed.mcp.is_some(), "preset claims [mcp]");
        assert!(!loom_dir.is_empty());
    }
}
```

Adapt the two assertions to the sections your preset really sets. If your
preset changed command behavior, add an assert for the constructed command and
one for the absent-binary error path (the error string surfaces from the
spawner — assert on it, not just `is_err()` if you can).

---

## Six traits — meaning and when to implement

`GenericAdapter` derives all six from the TOML. Absent section = no-op default
(that part of the adapter does nothing, silently):

| TOML → Trait | No-op means | Implement when |
|---|---|---|
| `[skills]` → `SkillProvider` | Tool shares no skills with the ecosystem | Tool has a native skill/extension dir worth importing |
| `[mcp]` → `McpConsumer` | No MCP servers imported or shared | Tool keeps its own `mcp.json` or can serve shared servers |
| `[auth]` → `AuthProviderTrait` | Tool manages credentials alone | Tool has a parseable local credential store to register |
| `[hooks]` → `HookExecutor` | No hook scripts are executed | Tool supports lifecycle/automation hooks |
| `[routing]` → `RoutingTarget` | Routing decisions never reach the tool | You want `loom routing` model picks to steer the tool |
| `[sessions]` → `SessionExportable` | No session sharing/indexing | Tool exports transcripts worth indexing |

---

## PR checklist

- [ ] `cargo build --workspace` — clean build
- [ ] `cargo test -p loom-adapters` (and `-p loom` for thread behavior) — new tests pass, no regressions
- [ ] `cargo clippy --workspace` — 0 warnings (README keeps this bar)
- [ ] No hardcoded `~/.loom` — use `loom_core::loom_dir()` / the `LOOM_AGENTS_DIR` env; paths in TOML may use `~` (expanded by the adapter)
- [ ] Session export round-trips: `cargo test -p loom-integration-tests -- sessions` (`session_export_import_roundtrip`)
- [ ] `loom scan` in a `LOOM_AGENTS_DIR=$(mktemp -d)` sandbox shows your tool registered
- [ ] Doc conventions in [`AGENTS.md`](../AGENTS.md); architectural rationale in `docs/decisions/0003-adapter-contribution-toml-first.md`

---

*Validated on 2026-10-26 (Loom 0.10.2 / ecosystem 2.0.0, macOS): every command
above ran verbatim against a tempdir loom dir; outputs shown are the real
ones (`loom init --import`, `loom thread demoCli`, `loom scan` tools row
1/active, `loom thread` overwrite behavior, `loom debug adapters` scope).*
