# PR2 — Container egress allowlist proxy (make `restricted` real)

> **Spec-kit pickup:** `loom workflow create --title "PR2 container egress allowlist proxy" --description "Preflight prerequisite for pillar 4. Enforce NetworkPolicy::Restricted as an actual allowlist per the loom-runtime network posture contract (repo AGENTS.md): engine network is none, egress flows through an enforcing proxy; remove the documented downgrade diagnostic." --spec` — start from this file as the spec seed.

## Overview

`loom-runtime` parses `restricted:<hosts>` and stores the allowlist, but the engine gets `--network none` — the allowlist is **not enforced** and the runtime logs a downgrade on every run (`container network posture downgraded: restricted (posture=none, allowed_hosts not enforced yet: …)`). `docs/AGENTS.md` and the module docs promise an egress proxy later. This task is that proxy.

Why a sidecar proxy and not engine flags: Docker/Podman have no per-host egress control in `--network`; Kubernetes NetworkPolicy needs a CNI that implements it (verified unavailable — see the repo's own k8s posture notes), and `restricted` forbids cluster-default behavior. One userland proxy is backend-agnostic and testable.

## Expected behavior

- `NetworkPolicy::Restricted { allowed_hosts }` non-empty:
  1. Runtime launches a proxy container on the workload's network (exact attach story in Phases).
  2. Workload env: `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY=localhost,127.0.0.1` point at it.
  3. Proxy enforces the allowlist on CONNECT/absolute-URL: host not in list → connection refused (fail-closed).
  4. **Direct egress stays impossible** — engine network posture for the workload remains `none` on Docker/Podman (workload talks only to the proxy over the internal network); for Kubernetes the proxy runs as a sidecar with no shared-namespace surprises, workload netnone.
- `NetworkPolicy::Restricted` with empty allowlist: unchanged — resolves `none`, logs the existing diagnostics (still fail-closed).
- `none` / `unrestricted`: byte-for-byte unchanged.
- Downgrade logging for non-empty `restricted` goes away; the `diagnostic()` path and the `posture_diagnostic.rs` tests are updated to say *enforced* instead of *downgraded*.
- Proxy failure (image absent, port conflict) = dispatch fails with a clear error, **never** silently downgrades to more egress.

## Implementation plan

1. New binary `crates/loom-egress-proxy/` (or a `[[bin]]` in `loom-runtime` if the workspace prefers fewer crates — decide at spec): axum/hyper CONNECT-tunnel proxy, allowlist match on destination host (SNI/CONNECT authority), read from `LOOM_ALLOWED_HOSTS` CSV env (the runtime already sets `loom.network_allowed_hosts`-style env at src/lib.rs:1115 — reuse that env, do not invent a file format). Mark deliberate ceiling: `# ponytail: host-only matching; wildcard/path rules if a customer asks`.
2. `loom-runtime`: when policy is restricted-nonempty, compose the proxy as a second container on the same user-defined network (Docker/Podman) or sidecar (k8s); inject env vars; remove the downgrade log for this case.
3. Image story: proxy ships in the loom release (homebrew bottles already build binaries); container tag mirrors the binary version. For CI (`container` job gates `LOOM_CONTAINER_SMOKE=1`), the smoke test extends to run restricted with the proxy and prove an allowlisted host is reachable while a non-listed one is refused.
4. DNS: allowlist matches the *requested hostname* as written (both bare + `:port`), with CIDR entries accepted the same way the parser already does (`10.0.0.0/8`).

## Phases

1. **Proxy binary + env contract**, unit-tested standalone. Exit: proxy passes for listed hosts, refuses others, logs denies.
2. **Runtime wiring (Docker first)**, `restricted` non-empty → proxy + env + engine `none` for the workload. Exit: `network_policy.rs` + `container_smoke.rs` extended, downgraded-log test flipped to enforced. CI `container` job green.
3. **Podman + Kubernetes backends** — same wiring per backend; k8s gets the fail-closed caveat documented (CNI-dependent enforcement only via the userland proxy, not NetworkPolicy). Exit: all three backends honor the same contract, backend matrix test.
4. **Docs**: posture table in `loom-runtime/src/lib.rs` module docs + `AGENTS.md` snippet updated — "restricted is countable as an enforce-state, and the allowlist is in effect."

## Testing strategy

- Proxy unit: allowlisted host ↔ 200, unlisted ↔ 403/refused, `NO_PROXY` honor list, malformed env → refuse to start (fail-closed).
- Runtime integration ( tempfile contract, CI-gated container smoke): restricted+proxy on Docker — inside-container `curl api.example.com` succeeds, `curl evil.example.com` fails; env injection; proxy-kill → workload cannot egress (no silent degrade).
- Existing tests updated, not loosened: `posture_diagnostic.rs` expectations flip from "downgraded" to "enforced" for the non-empty case.
- Kubernetes path: kubeconfig-absent skip, sidecar placement via the existing container_smoke gating pattern.

## Exit criterion (preflight gate)

`restricted:api.github.com` inside a container reaches api.github.com and nothing else, on the default backend, with the CI `container` job passing. The "not enforced yet" string disappears from the repo.

## Open questions

- Proxy binary placement: own crate vs `[[bin]]` in loom-runtime — decide at spec time (lean one crate if axum is already a workspace dep).
- Proxy image registry/ownership for external users (loom-owned image vs build-at-release) — small, resolve in phase 2.
