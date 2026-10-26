# ADR 0002 — Egress allowlist proxy for the `restricted` network posture

**Status:** Accepted (Docker backend wired; Podman and Kubernetes follow-up)

## Context

`NetworkPolicy::Restricted { allowed_hosts }` is the posture for "this run
may reach these hosts, nothing else". Until PR2 it parsed and *recorded* the
allowlist (labels, logs) but enforced nothing: the engine got `--network
none`, and every `restricted` run logged a downgrade diagnostic
("allowed_hosts recorded but unenforced"). The v0.9 contract shipped allowlists
that were only labels — an operator asking for `restricted:api.github.com`
got zero egress, not allowlisted egress.

## Decision

A **userland CONNECT-tunnel proxy** (`loom-egress-proxy`), containerized and
composed by the Docker backend:

1. Runtime creates a **user-defined `--internal` network** per run
   (`loom-net-<run>`). Internal networks get no NAT — the workload cannot
   reach anything off-network, engine-guaranteed, same as `--network none`.
2. Runtime runs a **proxy sidecar** (`loom-{run}-proxy`) attached to the
   internal network *and* to the default bridge — the bridge is its only
   route upstream. The proxy enforces the allowlist on CONNECT (host
   matched as written, CIDR entries for IP literals); fail-closed at
   startup (empty/malformed allowlist ⇒ proxy exits, dispatch fails) and at
   connection time (unlisted host ⇒ 403).
3. Workload env gets `HTTP_PROXY`/`HTTPS_PROXY=http://<proxy>:3128` and
   `NO_PROXY=localhost,127.0.0.1`.
4. Proxy readiness is proven from its own log line; **proxy failure fails
   the dispatch** — never a silent widening of egress.

Why a userland proxy and not engine flags: Docker/Podman have no per-host
egress control in `--network`, and Kubernetes NetworkPolicy needs a CNI that
implements it — verified unavailable in the repo's own k8s posture notes —
while a `restricted` posture must not depend on cluster-default behavior.
One userland proxy is backend-agnostic and testable.

## Alternatives considered

- **Per-backend iptables/nftables rules** — rejected: not engine-portable
  (requires poking each engine's iptables plumbing; breaks rootless Podman
  and managed Docker).
- **Keep the do-nothing status quo** — rejected: the v0.9 contract lies.
  Labels that promise an allowlist that never constrains anything are worse
  than no allowlist.
- **MITM/inspecting TLS inside tunnels** — rejected: no interception of
  arbitrary TLS; the proxy relays bytes of allowed connections unread.

## Consequences

- One extra container per `restricted` run, named deterministically
  (`loom-{run}-proxy`, `loom-net-{run}`) and removed with the workload in
  the same cleanup call.
- DNS semantics: matching is **hostname-as-written** (case-insensitive,
  trailing dot ignored) done *before* any DNS resolution, so denial is
  resolution-independent. The proxy resolves allowed hosts on its own egress
  path. Unlisted hostnames are refused without a lookup.
- No MITM: allowed CONNECT tunnels are byte-relayed — the proxy sees SNI/CONNECT
  authorities, not TLS payload.
- Kubernetes/podman backends do not compose the proxy yet: `restricted`
  non-empty there stays deny-all (fail-closed, allowlist not honored) —
  wiring them is the explicit follow-up. Podman's docker-compatible API can
  reuse the same composition; Kubernetes needs a sidecar Pod shape.
- The proxy image ships separately (`crates/loom-egress-proxy/Dockerfile`);
  the standalone crate keeps the image build to tokio+anyhow only.
