# Stage 5 findings (simulate output) — IMPLEMENTED

## already-exists (no code change needed)

1. Password hashing uses argon2, not SHA-256+hardcoded salt — `loom-auth/src/vault.rs:38-42, 164-166` → CVE-2 equivalent already compliant.
2. All `Command::new` call sites pass args separately (no shell) — no shell-injection vector.
3. `client_secret` marked `#[serde(skip_serializing)]` — `loom-sso/src/config.rs:33` → no secret persisted to config.
4. Private keys written with `0600` perms — `loom-core/src/license.rs:210` → filesystem boundary respected.

## spec-gap (IMPLEMENTED)

5. `validate_https_url` prefix-only → replaced with `validate_outbound_url` (HTTPS + private/loopback IP blocklist) in `loom-core/src/plugin_registry.rs`. Applied at plugin download (`install_from_url`, `update`) and remote-config merge (`config.rs`).
6. OIDC issuer URL SSRF guard — `loom-sso/src/oidc.rs`: `validate_outbound_url` called before `discover_async`. Fail-closed.
7. Hook script path containment — `loom-hooks/src/lib.rs`: `execute_hook_script` + `execute_hooks` + `run_post_phase_hooks` now take `loom_dir` as parameter; `path_is_inside()` enforces `..`/absolute-path containment. All callers plumbed (`HooksEngine::execute_hooks`, `dispatch.rs`, `workflow_chain.rs`, `ecosystem.rs`).
8. `cargo audit` — tokio bumped 1.53.2 → 1.54.1; ratatui bumped 0.28 → 0.30 (fixes transitive lru 0.12.5 → 0.18.5); rustls-webpki 0.103.15 → 0.103.16; webpki-roots 1.0.9 → 1.0.11. `cargo audit` reports 0 vulnerabilities.
9. Deprecation warnings from ratatui 0.30 (`highlight_style` → `row_highlight_style`) fixed in `loom-tui` for clippy `-D warnings` compliance.

## risk (RESOLVED)

10. SSRF blocklist: single shared helper in `loom-core` (`validate_outbound_url`), applied at every outbound HTTP call site. Matches ADR 0002 egress-allowlist posture.
11. Blocked URL: fail-closed (returns `Err`, logged). No silent-skip.

## Tests added

- `cargo test -p loom-core`: `plugin_registry::tests::validate_outbound_url_rejects_private_ips` — IPv4/IPv6 private/loopback rejection + public HTTPS allow.
- `cargo test -p loom-hooks`: `tests::test_path_is_inside_rejects_traversal` — `..`/absolute path containment.
- Existing tests updated for new `loom_dir` parameter on `execute_hooks` / `run_post_phase_hooks`.

## Grading (all pass)

- `cargo audit` — 0 vulnerabilities
- `cargo check --workspace --exclude loom-tui` — clean
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- `cargo test -p loom-core -p loom-hooks -p loom-workflow` — all pass
- `cargo test -p loom-integration-tests --test workflow_chain --test ecosystem` — all pass
