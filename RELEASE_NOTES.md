<!--
Release notes template.

How to use:
  1. Replace the comment block under "## Notes" with the release notes, in the
     style used elsewhere in CHANGELOG.md (e.g. "- feat(cli): ..." bullets, or
     "### Added" / "### Fixed" / "### Changed" subsections).
  2. Run: scripts/release.sh <version>
     The content under "## Notes" is copied into CHANGELOG.md as the body of
     the new "## [<version>] - <date>" section. The "## Notes" heading itself
     is dropped.
  3. If "## Notes" is left empty, the script inserts a visible TODO placeholder
     instead of failing the release.

Only the content under "## Notes" is used; this comment block is ignored.
-->

## Notes

### Added

- feat(ffi): cross-language registration — `loom_register_tool`, `loom_register_skill`, `loom_unregister_tool` C-ABI exports (JSON in/out, panic-contained). One codepath: `ToolToml::write` and the skill-install flow moved into `loom-generic-adapter`/`loom-skills` so CLI and FFI cannot drift. Python ctypes example validated in `docs/adapter-contribution.md`
- feat(runtime): the egress allowlist is **enforced** on Docker — new standalone `loom-egress-proxy` crate (CONNECT-tunnel proxy, host/CIDR allowlist from `LOOM_ALLOWED_HOSTS`, plain byte relay, no MITM). `restricted:<hosts>` composes an internal no-NAT network + proxy sidecar, injects `HTTP(S)_PROXY`/`NO_PROXY`; proxy failure fails the dispatch, never widens egress. Container CI job builds the image and asserts allow/deny
- feat(admin): `loom-admin keygen` / `loom-admin licensemint` — offline Ed25519 license minting (`~/.loom-admin/license.key` 0600, claims = org/tier/seats/expiry, ≤1 year). `loom-core::license` carries the mint+verify pair the future control plane reuses
- docs: `docs/adapter-contribution.md` (TOML-first contribution path, every command validated against a tempdir), ADRs 0001–0005 under `docs/decisions/`, and the pillar-4 commercialization plan + preflight plans under `blueprint/commercialization/`

### Changed

- change(security): plugin Ed25519 verification is on by **default** (`security.require_signature` now true) — unsigned remote installs are refused with the opt-out named; local `loom plugin register` is unchanged. Enforcement moved inside the registry so an unsigned plugin can never land half-installed
- change(security): key-continuity pins for plugin installs — `{loom_dir}/auth/plugin-pins.json` (0600) records the signing key per tool; a later install signed with a different key refuses until the pin entry is deleted
- change(runtime): `restricted` posture docs/AGENTS.md now describe the enforced state; Podman and Kubernetes stay deny-all until their wiring lands (ADR 0002)
- docs: README/STATUS/VISION/ROADMAP reconciled against the code — 32 workspace members (incl. the standalone egress-proxy image crate), 37 top-level subcommands, Homebrew tap and source-install wording

### Fixed

- fix(cli): post-merge test-module seams in `skills.rs` (`update_lock` import, `validate_skill_file` path)
