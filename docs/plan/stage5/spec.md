# Stage 5: SSRF + path-traversal guards + dependency hardening

## Problem

Loom's trust boundaries accept user-controlled URLs and paths without SSRF or traversal guards:
- OIDC provider discovery follows redirects from a user-controlled issuer URL (`loom-sso/src/oidc.rs:27`) with no private/loopback IP blocklist → reaches cloud metadata or internal services.
- Plugin/tool-TOML downloads and remote-config fetch only check `url.starts_with("https://")` (`loom-core/src/plugin_registry.rs:10-17`, `config.rs:270`) → `https://169.254.169.254/...` and `https://[::1]/` pass and reach internal/loopback services.
- Hook script resolution accepts absolute paths and joins relative paths to loom_dir with no `..` containment (`loom-hooks/src/executor.rs:161-166`) → path traversal outside loom_dir.
- `cargo audit` surfaces 2 vulnerable crates: `lru` 0.12.5 (RUSTSEC-2024-0449, RUSTSEC-2026-0253) and `tokio` 1.53.2 (RUSTSEC-2026-0002, RUSTSEC-2026-0386).

Fix these so outbound HTTP and local script execution cannot reach internal/loopback services or traverse outside loom_dir.

## Acceptance

1. `cargo audit` reports 0 vulnerabilities (lru and tokio bumped to non-vulnerable versions). Grading: `cargo audit`.
2. `validate_https_url` (plugin_registry.rs) and the remote-config URL path reject private/loopback IP literals before any network I/O: RFC 1918, `169.254.169.254`, `::1`, `::ffff:127.0.0.1`, and decimal/octal/hex IP forms. Grading: new unit test `cargo test -p loom-core` (`test_https_url_rejects_private_ips`).
3. OIDC provider discovery rejects private/loopback issuer URLs before discovery. Grading: new unit test `cargo test -p loom-sso` (`test_oidc_issuer_rejects_private_url`).
4. Hook script path resolution enforces containment inside loom_dir: canonicalize both sides and assert `resolved.starts_with(canonical_loom_dir)`; reject absolute paths outside loom_dir and `..` traversal. Grading: new unit test `cargo test -p loom-hooks` (`test_hook_script_path_stays_inside_loom_dir`).
5. `cargo check --workspace --exclude loom-tui` passes; `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
6. `git diff main..HEAD -- '**/Cargo.toml'` adds no new dependency beyond version bumps for lru/tokio.

## Out of scope

- New CLI flags, commands, or command groups.
- New crates or new dependencies beyond lru/tokio version bumps.
- TUI changes.
- Injecting spec/findings contents into dispatch prompts.
- loom-rules, event-bus, persistence migrations.
- Hook signature/verification changes (already present); only path containment.
- Centralizing all `ureq`/`reqwest` calls behind one SSRF helper (per-call-site guards for now; see Needs clarification).

## Constraints

- Existing crates only: loom-core, loom-sso, loom-hooks (and their tests).
- No new dependency except version bumps for lru and tokio.
- Old task/config JSON must keep loading (serde defaults).
- Change confined to the named crates.

## Decisions (resolved from Needs clarification)

- **Single shared SSRF blocklist helper** in `loom-core`, applied at every `ureq`/`reqwest` outbound call site (plugin registry download, remote-config merge, OIDC provider discovery, org-registry sync). Matches loom's existing egress-allowlist posture (ADR 0002, `crates/loom-egress-proxy/`) and avoids per-call-site guard decay as agents add new fetch sites.
- **Fail-closed:** a blocked (private/loopback) URL returns `Err` and is logged; no silent-skip. Silent-skip is reserved for optional non-security config only.

## Needs clarification

None.

