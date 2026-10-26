# PR1 — SDK story: register tools/skills from other languages

> **Spec-kit pickup:** `loom workflow create --title "PR1 SDK story: cross-language tool/skill registration" --description "Preflight prerequisite for pillar 4. Extend loom-ffi so a non-Rust process can register a tool (GenericAdapter TOML) and a skill through the same codepaths the CLI uses." --spec` — start from this file as the spec seed; ComplexityScore gate routes per repo convention.

## Overview

`loom-ffi` currently exposes 4 C-ABI functions only (`loom_version`, `loom_health_json`, `loom_classify_prompt`, `loom_resolve_route`) — all read-only, no registration. Meanwhile the CLI already has the registration machinery: `loom thread <tool>` writes a GenericAdapter TOML at `{loom_dir}/tools/{tool}.toml`, and `loom skills install <path>` installs a skill. The SDK story is **not** a new SDK crate: it is FFI exports that call those same registration functions, so bindings in Python/Node/Swift/Go register through the exact path the CLI does (one codepath, zero drift).

## Expected behavior

- `loom_register_tool(json: *const c_char) -> *mut c_char` — JSON payload `{name, command, args?, env?, description?}` → validates → writes the same TOML a `loom thread` run would. Returns `{ok, toml_path}` or `{ok:false, error}` as JSON.
- `loom_register_skill(json) -> *mut c_char` — JSON `{name, content}` → same path as `loom skills install` from a temp file.
- `loom_unregister_tool(name) -> *mut c_char` — removes the TOML file (same behavior as `loom mcp remove` / tool removal path).
- Errors return structured JSON, never panics across the FFI boundary (existing `catch_unwind` discipline in `bindings.rs` if present — else add once at the trampoline).
- `cdylib` target in `loom-ffi` so this is loadable from Python `ctypes`, Node `ffi-napi`-class FFI, Swift, Go `cgo` — no new dependency.

## Implementation plan

1. `crates/loom-ffi/src/bindings.rs` — add the three exports; shave JSON with the workspace's existing serde stack (no new deps). Reuse `loom-generic-adapter`'s TOML writer — call it, don't re-serialize by hand.
2.libs: one `bindings` helperapi file per export, small; string-return convention identical to existing `loom_health_json` (caller frees via an added matcher if the codebase already has one — check and reuse, else add `loom_free`).
3. `crates/loom-ffi/tests/` — round-trip: register → TOML exists → `loom-generic-adapter` loads it → unregister → gone.
4. Docs: one short section appended in the adapter contribution guide (PR3) pointing at the FFI surface — "registering from Python in 6 lines" example.
5. Rebind/copy structure: sibling language bindings ship as **examples in the guide**, not as maintained packages (YAGNI until a customer asks for a specific language).

## Phases

1. **FFI exports + TOML reuse** (loom-ffi only). Exit: Python `ctypes` example registers a tool and `loom scan` lists it.
2. **Skill registration + unregister**. Exit: full lifecycle works from a foreign process without touching `{loom_dir}` by hand.

## Testing strategy

- Unit: TOML round-trip, name validation (reject `/`, empty, duplicates per existing tool-reg behavior — match `loom thread`'s checks exactly), error JSON shape.
- Integration: tempdir loom dir (repo convention), register→scan→unregister lifecycle; thrown-panic containment test.
- FFI: cdylib test via a small inline C harness mirroring how the existing `#[cfg(test)]` in `bindings.rs` works.

## Exit criterion (preflight gate)

A process in a **non-Rust** language registers a tool and a skill through loom-ffi, both appear in `loom scan`, unregistration cleans up. That is the whole pillar-1 "SDK story" bar — nothing more.

## Open questions

- Whether `loom_register_skill` needs metadata fields beyond `{name, content}` — resolve at spec time against the skills DB schema.
