# PR3 — Adapter contribution guide

> **Spec-kit pickup:** `loom workflow create --title "PR3 adapter contribution guide" --description "Preflight prerequisite for pillar 4. Author docs/adapter-contribution.md so an outside contributor can add an adapter (TOML tool registration or a preset Rust adapter in loom-adapters) without reading the repo internals. Include validation commands and a test template." --spec` — start from this file as the spec seed; Docs-class task, gate routes straight to Plan.

## Overview

Two contribution routes exist and neither is documented for outsiders:
1. **TOML path (no Rust)** — a `~/.loom/tools/{tool_name}.toml` file makes `loom-generic-adapter` derive 6 trait impls (`SkillProvider`, `McpConsumer`, `AuthProviderTrait`, `HookExecutor`, `RoutingTarget`, `SessionExportable`); missing sections become no-ops. Zero code, already the cheapest path, completely undocumented.
2. **Preset Rust adapter** — a hand-written struct in `crates/loom-adapters/src/` (13 exist: aider, claude_code, codex, crush, cursor_cli, gemini_cli, github_copilot, goose, jcode, juno, kiro, opencode…), plus a `packages/{tool}/` entry and `TerminalLaunch` PATH checks (`AGENTS.md` gotcha #8 names the check order).

## Expected behavior

`docs/adapter-contribution.md` exists and answers, for a contributor who only knows the agent tool they're wrapping:
- Which route to pick (decision rule: wraps a CLI with stable flags → TOML first; needs conditional command construction, streaming quirks, or session tricks → Rust).
- TOML path: full annotated example of every section, no-op behavior on absent sections, `loom thread <tool>` + `loom scan` validation loop, where the file must live (`~/.loom/tools/`, `LOOM_AGENTS_DIR` override noted).
- Rust path: struct shape copied from the simplest existing adapter (jcode is likely smallest — verify at write time), where it registers, the adapters' shared trait surface, and how `packages/{tool}/` packaging works.
- A test template: what one minimal test must assert per adapter (command construction + absent-binary error), matching the conventions already used in `loom-adapters`.
- PR checklist: `cargo build --workspace`, crate-scoped tests, `cargo clippy`, no `~/.loom` hardcoding (use `loom_core::loom_dir()`), session export must round-trip.
- Runs "six Traits" table: each trait, what a default no-op means, when to implement it.

## Implementation plan

1. Read `crates/loom-generic-adapter/src/` (TOML schema) and one small preset adapter; extract the real schema, don't guess.
2. Write `docs/adapter-contribution.md` (~2 pages, no repo-history prose).
3. Add a **smoke validation script or make/Command snippet** in the guide: "run `loom debug adapters` / `loom thread` against your new TOML and expect this output" — one command, one expected output.
4. Link the guide from `README.md` (Contributing section) and from `crates/loom-adapters/README`-equivalent if one exists (else one-line pointer in the crate's `lib.rs` docs).
5. Wire into PR1's guide section: SDK/FFI example links here for "what to do with a registered tool next".

## Phases

1. **Guide + test template** — exit: an external contributor (no repo context) can add a TOML adapter in one sitting following the doc alone.
2. **README links + SDK cross-ref** — 10-minute follow-on, same phase.

## Testing strategy

- Docs-class task: the proof is a **new-contributor simulation** — validate every command in the guide on a tempdir loom dir (`LOOM_AGENTS_DIR=/tmp/...`), confirm each example output is real. One checklist section inside the doc records the simulation date + env.
- Rust-path claim verified by diffing the guide's struct example against an actual adapter file.

## Exit criterion (preflight gate)

Guide merged, every command in it executes verbatim against a tempdir, README links it. Pillar-1 "SDK/extension story" has a public face.

## Open questions

- Whether ` TerminalLaunch` PATH-check order (gotcha #8) should be renamed/documented as stable API or an internal detail — decide while writing the Rust section.
