# ADR 0001 — FFI registration exports for cross-language SDK story

## Status

Accepted

## Context

The commercialization plan's SDK story needs a non-Rust process (Python
`ctypes`, Node FFI, Swift, Go cgo) to register a tool and a skill, and to
appear in `loom scan`, through the exact codepaths the CLI uses — one
codepath, zero drift. The alternative is packaging full per-language SDKs
(Python wheel, npm package, ...) over an IPC/HTTP surface.

## Decision

Three C-ABI exports in `loom-ffi`, reusing existing subsystems, never
reimplementing them:

- `loom_register_tool(json)` — `{"name", "version"?, "command"?, "args"?,
  "env"?}` → `loom-generic-adapter`'s `ToolToml::write` (the same writer
  `loom thread` routes through) → `{loom_dir}/tools/{name}.toml`.
- `loom_register_skill(json)` — `{"name"?, "content"}` → temp file →
  `loom_skills::install::register_skill` (the full `loom skills install`
  flow: scan validation, registry upsert, lock-file sync).
- `loom_unregister_tool(name)` — removes `{loom_dir}/tools/{name}.toml`.

Errors are structured JSON (`{ok:false,error}`) and panics are contained by
a `catch_unwind` trampoline; strings are returned as `*mut CString` and
freed with the existing `loom_free_string`. The loom dir resolves through
`loom_core::loom_dir()` so `LOOM_AGENTS_DIR` applies identically to CLI,
daemon, and FFI callers. Support for languages ships as documented
examples, not per-language packages, until a customer asks (YAGNI).

Skill-install logic moved from `loom-cli` into `loom-skills::install` so
the CLI and the FFI share it — a move, not a copy.

## Alternatives considered

- **Full per-language SDK packages** (Python wheel + npm + Swift module).
  Rejected: three maintained packages shipping a snapshot of the same
  registration logic; each one drifts from `loom thread` independently and
  each needs its own release cadence. The FFI exports bind to the compiled
  Rust code itself, so drift is structurally impossible.
- **FFI → IPC daemon calls** (reuse the existing `register_tool` IPC
  command). Rejected: requires a running daemon; offline machines lose the
  story entirely. Filesystem registration is the substrate everything
  already agrees on.

## Consequences

- `loom-ffi` gains `loom-skills` and `loom-generic-adapter` as native
  dependencies (dylib grows), and drops an `rlib` target so tests (and
  foreign bindgens) can link against it.
- A permanent API commitment: the three export signatures and their JSON
  contracts become the compatibility surface for foreign callers.
- Foreign callers must read the returned `*mut CString` correctly
  (deref the first 8 bytes for the data pointer in `ctypes`; bindgens
  handle this natively) and pass UTF-8, NUL-terminated strings.
- `offset` data the TOML schema has no slot for (e.g. `description`) is
  ignored until the schema grows a field; nothing errors on it.
