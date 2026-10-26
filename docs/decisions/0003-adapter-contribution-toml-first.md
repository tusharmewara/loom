# ADR 0003 — Adapter contribution is TOML-first; Rust presets are the power path

**Status:** Accepted (2026-10-26)

## Context

Two ways exist to integrate an agent tool with Loom: a TOML registration
(`<loom-dir>/tools/{name}.toml`, interpreted by `loom-generic-adapter`, which
derives the six Loom traits and no-ops absent sections) and a hand-written
preset adapter in `crates/loom-adapters`. Neither had a documented contract:
the TOML file was discovered by reading `config.rs`, and the Rust route by
imitating `aider.rs`. Outside contributors had no public face for the SDK/
extension story.

## Decision

1. **The TOML registration file is the public, first-class contribution
   path.** A tool integrator starts there; `loom thread <tool>` creates the
   skeleton and `loom scan` validates. No Rust, no recompile.
2. **Rust preset adapters are the power path**, reserved for behavior the
   TOML schema cannot express (dynamic command assembly, protocol quirks,
   routing hooks). They live in `crates/loom-adapters`, implement
   `LoomToolAdapter`, and register via `adapter_for()` + `supported_tools()`.
3. `docs/adapter-contribution.md` is the contract: it documents the full TOML
   schema section-by-section, the validation loop, the struct shape, a test
   template, and the PR checklist. Every command in it is verbatim-executable
   against a `LOOM_AGENTS_DIR` sandbox.

## Consequences

- **The ToolToml schema is now a compatibility surface.** Renaming,
  removing, or changing the meaning of a section in
  `crates/loom-generic-adapter/src/config.rs` breaks external contributor
  files and this documented contract. Section changes need a deprecation note
  in the guide (keep the field, warn, migrate later — do not hard-remove).
- The guide must track schema drift the way code does — cite the section
  names from `config.rs`, not the doc, as the source of truth, but keep the
  doc honest when they change.
- The TerminalLaunch PATH-check order (`opencode`, `claude-code`, `jcode`,
  `crush`) is documented as an internal detail, not a stable API — new
  presets must not be coupled to it.
