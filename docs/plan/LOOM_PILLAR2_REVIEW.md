> **RESOLVED 2026-10-21 — this is the review record, kept as it was written.**
>
> The findings below were raised against the three pillar-2 branches and were
> closed by the fix cards (P2-F1b, P2-F2, P2-F3) before the merge branch `p2/merge`
> landed on `main`:
>
> | Finding | Severity | Resolution |
> |---|---|---|
> | F1, F2 | HIGH / MEDIUM | Closed. The export is self-verifying: the chain hash covers the *normalised* outcome (`chain_scheme: 2`) so a consumer can re-derive it, and the whole document is signed with a per-machine Ed25519 key. Three verdicts: `intact` / `tampered` / `unanchored` — never `intact` without a pinned key. Three attack tests (field tamper, whole-document forgery, re-hashed tamper) all fail verification. The raw `result` never leaves the machine. |
> | F3 | MEDIUM | Closed. The TUI dispatch path now calls `Runtime::cleanup(&result)` after a successful run, so the container is not leaked (the engine's auto-remove stays off so the runtime can read the logs after exit). |
> | F4 | MEDIUM | Closed. A `restricted` request that resolves to `none` is logged on every run: "container network posture downgraded: restricted (posture=none, allowed_hosts recorded but unenforced: …)". |
> | F5 | MEDIUM | Closed. The Kubernetes `NetworkPolicy` is created *before* the Job (the pod selector is computable from `loom.task_id`), and the doc comment states that a NetworkPolicy only denies egress when the cluster CNI enforces it — the k8s backend is best-effort, Docker/Podman are unconditionally fail-closed. |
> | F6 | LOW | Closed. `GrantRecord::is_usable` is deleted; `verify_grant` is the single owner of the grant-check rule. |
> | F7 | LOW | Closed. `auth/grants.json` is written through the vault's private writer (mode `0600`). |
> | F8 | LOW | Documented, not changed. The grant token is a bearer capability; `docs/architecture/grant-trust-model.md` says so and there is deliberately no CLI `redeem`. |
> | F9 | LOW | Closed. `impl Default for SecurityConfig` calls `default_audit_retention_days()` — one constant. |
> | F10 | LOW | Closed. The verify JSON carries one `verdict` plus a `verifies` discriminator (`"export"` / `"local_chain"`), so the local-chain case no longer presents one check as four. |
> | F11 | INFO | Resolved in the merge; both sides kept. `loom auth verify` (export) and `loom auth grant verify` (grant) are documented as different commands in the CLI help and in `AGENTS.md`. |
> | F12 | INFO | Noted. The Podman `remove_container` path is not 404-tolerant; it is unaffected by the Docker `auto_remove` flag. |
>
> The review's own baseline is reproduced below unchanged.


# Loom — Pillar 2 Review

**Reviewer role:** PILLAR-2 REVIEWER (review only; no fixes, no merges, no pushes).
**Repo:** `/Users/tmewara/codebase/loom` — base `main` @ `b77b5cf`.
**Date:** 2026-10-21.

## Branches under review

| Branch | Tip | Commits | Card | Card state at review time |
|---|---|---|---|---|
| `p2/scoped-creds` | `43032e9` | `d9b8192`, `43032e9` | `e06138a3-0bc5-4943-8881-ba89c2240a3d` | **done** — 1 execution, succeeded |
| `p2/container-policy` | `1e8a182` | `0684601`, `1e8a182` | `10341c12-5273-49d2-88b2-423d9f6c576e` | **done** — 2 executions (first cancelled, second succeeded) |
| `p2/audit-export` | `45a6c27` | `64d861d`, `45a6c27` | `d6c935ff-96cc-4fea-80b8-b60df6e003cc` | **done** — 3 executions (two failed `max-tokens`, latest `46ba7cea` succeeded) |

All three branches exist and were reviewed at the tips above. `p2/audit-export` moved
from `64d861d` to `45a6c27` while the review was in flight (the branch tip gained a
second commit, "fix(auth): keep the exported audit chain contiguous under retention");
the review below is against `45a6c27`.

---

## Findings, ordered by severity

### F1 — HIGH — `p2/audit-export`: the export is not tamper-evident against a party who can recompute the digest; a forged export verifies as `intact`

`crates/loom-auth/src/export.rs:285` `verify_export` checks three things:
`export_digest` matches (`:288`), consecutive entries are hash-linked (`:294-307`),
and the last hashed entry equals `source.chain_head` (`:319-323`).

`export_digest` (`:359`) is an **unkeyed** SHA-256 over the document with
`export_digest` blanked. The exported `entry_hash` values are carried **verbatim**
from the chain (`build_export`, `:230-247`) and are **never recomputed** — by
design, stated in the module doc (`export.rs:60-64`: "The chain's own hashes are
never recomputed here").

Consequence: the only thing that makes an export verifiable is a checksum the
attacker can recompute. I verified this empirically on the branch tip by adding a
throwaway test (since removed):

```
# 1. Edit one field of a real export, then recompute the digest the same way
#    export.rs does.
PROBE tampered+rehashed: ExportVerification { intact: true, digest_ok: true,
  chain_linked: true, chain_head_ok: true, source_chain_intact: true, entries: 1, problems: [] }
TAMPER SURVIVED VERIFICATION

# 2. Forge a COMPLETE export from scratch, with a self-consistent hash chain
#    and no audit log behind it at all.
PROBE forged: ExportVerification { intact: true, digest_ok: true, chain_linked: true,
  chain_head_ok: true, source_chain_intact: true, entries: 3, problems: [] }
FORGERY SURVIVED
```

Both the field-level tamper and the whole-document forgery pass `verify_export` and
would pass `loom auth verify <file>` (`crates/loom-cli/src/commands/auth.rs:206-215`
calls `verify_export` and only bails when `!result.intact`).

The card's HARD RULE 1 says the export must be *verifiable* and that "a consumer of
the exported file must be able to check the chain is intact". Today a consumer can
check *internal consistency* only. `source.chain_intact` and `source.chain_head` are
self-reported by the document and are inside the digest the attacker recomputes, so
they prove nothing about the real chain on disk.

The chain hashes themselves are recomputable by anyone: `compute_entry_hash`
(`crates/loom-auth/src/audit.rs:384`) is a plain SHA-256 over
`prev_hash‖timestamp‖requesting_tool‖provider_id‖access_type‖result` with no key
and no secret. A consumer *can* recompute the entry hashes and check them against
the exported `prev_hash`/`entry_hash` — but `verify_export` does not do this, and
the exported payload deliberately redacts `result` to `granted`/`denied`/`other`
(`normalize_outcome`, `export.rs:374`), so a consumer **cannot** recompute the
hashes from the exported fields either. The redaction and the verifiability
requirement are in direct conflict in the current design.

**Required fix (in `crates/loom-auth/src/export.rs`):** make the export
self-verifying. The cheapest correct shape is to export enough to re-derive each
entry hash (i.e. export the normalised-but-hash-relevant `result` value, or export
the per-entry hash preimage) and have `verify_export` recompute the chain hashes
and compare them with the exported `entry_hash` values — reusing the existing
`compute_entry_hash` rather than writing a second hasher. A keyed digest (a
per-export HMAC under a vault-held key) is the stronger option and also closes the
forgery hole, but it changes the artifact's nature (the consumer needs a key), so
the reviewer should pick one and say so in the AGENTS.md "Audit retention" section.
Note the chained entries are redacted, so the redaction rule and the verification
rule must be designed together, not independently.

### F2 — MEDIUM — `p2/audit-export`: the export's own chain hashes are not re-verified at export time either

`build_export` (`export.rs:204`) calls `logger.verify_integrity()?` (`:205`) and
records `report.intact` in `source.chain_intact`. But `verify_integrity` recomputes
the hash of each entry from the **on-disk** entry and compares to the entry's own
`entry_hash` — it never checks that `entry_hash` is *reproducible from the exported
fields*. So the two layers of verification are not linked: an export can be internally
consistent (F1) while the on-disk chain was tampered with, and `source.chain_intact`
is a claim the exporter makes about itself, not a fact the export carries.

This is a symptom of F1 rather than a separate defect; fixing F1 as described
(recompute the chain from the exported preimage) resolves both. If instead a keyed
integrity field is chosen, `source.chain_intact` should be dropped or clearly
relabelled as "self-reported at export time" in the module doc, so a consumer does
not read it as an independent attestation.

### F3 — MEDIUM — `p2/container-policy`: Docker `auto_remove` flipped from `true` to `false` — a behaviour change outside the card's scope, and it leaks containers when the runtime's own cleanup does not run

`crates/loom-runtime/src/lib.rs:1109` — the Docker client now passes
`auto_remove: Some(false)`; on `main` it was `auto_remove: Some(true)`
(`main:crates/loom-runtime/src/lib.rs:839`).

The branch's justification (inline comment at `:1105-1108`) is that the runtime owns
the container lifecycle so the daemon does not delete it out from under the runtime.
But the runtime's own cleanup is **not** guaranteed:

* `ContainerRuntime::run` removes the container only when `self.auto_remove` is
  true (`lib.rs:919-921`) — and that path is now the *only* removal path for Docker.
* The TUI dispatch path never calls `cleanup` at all:
  `crates/loom-tui/src/dispatch.rs:699` calls `runtime.run(...)` and discards the
  result — there is no `runtime.cleanup(&result)` call (compare
  `crates/loom-cli/src/commands/exec.rs:792-793` and
  `crates/loom-workflow/src/dispatch.rs:69-70`, which both do call it).
* `loomd` registers the container runtimes (`crates/loomd/src/lib.rs:73-82`) but
  has no run path that cleans up.

So on the TUI path a container now **survives** the run, where on `main` the Docker
daemon's `auto_remove` deleted it. That is a resource leak introduced by a security
branch, and it is not in the card's scope ("no behaviour change outside the card's
write scope").

**Required fix:** either (a) keep `auto_remove: Some(true)` and make the runtime's
`remove_container` tolerate the 404 the daemon already produces (the branch already
added 404 tolerance at `lib.rs:1191` for exactly this reason — use it), or (b) keep
`auto_remove: Some(false)` **and** add the missing `runtime.cleanup(&result)` to
`crates/loom-tui/src/dispatch.rs`. Option (a) is the smaller diff and restores main's
behaviour. The reviewer should also confirm the 404-tolerant `remove_container` is
what makes (a) safe.

### F4 — MEDIUM — `p2/container-policy`: `Restricted` is fail-closed but the allowlist is silently discarded — an operator who asks for `restricted:api.example.com` gets total isolation with no warning

`crates/loom-runtime/src/lib.rs:125` `engine_network_mode` maps
`NetworkPolicy::Restricted { .. }` to `Some("none")` — the same as `NoNetwork`.
`cli_args` (`:138`) therefore emits `--network none` for `restricted`.

This is the right fail-closed direction, and the branch documents it
(`lib.rs:86-90`, AGENTS.md "Container Network Posture" table, ROADMAP.md 2026-10-21
decision entry). The defect is in the *reporting*: the requested allowlist is
recorded only as a container label (`build_container_spec`, `lib.rs:990-1045`:
`loom.network_allowed_hosts`) and is otherwise discarded. Nothing at run time or in
the audit/log path tells the operator "you asked for restricted egress, you got
none". `NetworkPolicy::allowed_hosts` (`:157`) has no non-label consumer.

Note also that `allowed_hosts` is carried in the enum but `NetworkPolicy::parse`
(`:103`) accepts `restricted:` with an empty host list, and `restricted` with no
colon at all — both silently mean "no hosts, no egress". That is fail-closed and
acceptable, but it should be surfaced in the run output, not just the label.

**Required fix:** log the *resolved* posture and the *requested* posture whenever
they differ, e.g. at `lib.rs:903-904` where the policy is resolved — "restricted
(posture=none, allowed_hosts recorded but unenforced)". This is a `tracing::warn!` /
`tracing::info!` line, no new mechanism.

### F5 — MEDIUM — `p2/container-policy`: the Kubernetes `NetworkPolicy` is created *after* the Job, so the pod can start with egress before the policy exists

`crates/loom-runtime/src/lib.rs:666-673`: the Job is created first, then
`apply_network_policy` is called. The comment says "applied after the Job so the
Pod exists to be selected by the policy" — but that is the wrong order for
enforcement. A Job's pod is scheduled and started by the Job controller
immediately after creation, so there is a window in which the pod runs with the
cluster's default egress, before the NetworkPolicy object exists.

The fix is to create the NetworkPolicy **before** the Job. The pod labels are
already computable before the Job is created: `pod_labels` is built from
`spec.labels.get("loom.task_id")` (`lib.rs:618-628`), and `build_container_spec`
sets `loom.task_id` from the task id (`lib.rs:1022`) — so the selector is known
before the Job exists. The policy can be created first and the Job second; the
policy's `pod_selector` matches on labels the pod will carry, not on the pod
existing.

Also worth noting for the same backend: a Kubernetes NetworkPolicy with an empty
egress list (`lib.rs:541`) only denies egress if the cluster's CNI actually
enforces NetworkPolicy. There is no check that the CNI does, so "fail-closed" is
best-effort on Kubernetes. The branch's own doc comment should say so, and the
Docker/Podman backends are the ones that are genuinely fail-closed.

### F6 — LOW — `p2/scoped-creds`: `GrantRecord::is_usable` is dead code

`crates/loom-auth/src/grant.rs:98` `is_usable(tool, run_id, now)` is defined and
never called anywhere (`verify_grant` does the equivalent checks inline at
`crates/loom-auth/src/lib.rs:251`, `:256`, `:260`, `:274`). It is public API on a
`pub` struct, so it is not flagged by `clippy --workspace --all-targets
--all-features -- -D warnings` (which passes clean on the branch), but it is a
second, weaker copy of the enforcement rule: it does not check revocation against
the AEAD, and it is not the rule the runtime enforces.

This is a latent "two owners" hazard for exactly the behaviour the card cares
about (the grant check). Either use it from `verify_grant` or delete it. The
reviewer prefers deleting it — `verify_grant`'s explicit checks read better and
already log distinct audit reasons per failure.

### F7 — LOW — `p2/scoped-creds`: the grant registry `auth/grants.json` is written without restrictive file permissions

`crates/loom-auth/src/grant.rs:212-239` `GrantStore::save` writes the registry
with `std::fs::write` to a `.json.tmp` and renames it. No mode is set, so the file
lands with the process umask (typically `0644`).

This is not a secret leak — the registry holds only scope metadata
(`grant_id`, `provider_id`, `run_id`, `tool`, timestamps, revoked flag), never the
token or the secret, by design (`grant.rs:72-74`). But the registry **is** the
revocation source of truth: the branch's own trust model
(`docs/architecture/grant-trust-model.md`, "Revocation") says "Revocation is
recorded, not enforced against a tampered registry: someone who can write
`grants.json` can un-revoke a grant." A world-writable-by-default registry widens
that window. The pre-existing vault has the same characteristic
(`crates/loom-auth/src/vault.rs:220-223` writes `<provider>.enc` with plain
`std::fs::write`, no `set_permissions`), so this is consistent with the codebase
rather than a regression — but a security branch that introduces a new
trust-boundary file should set `0o600` on it.

**Required fix (minor):** set `0o600` on the registry file in `GrantStore::save`
(`crates/loom-auth/src/grant.rs:212`), matching what the vault *should* also do
(same one-line change at `crates/loom-auth/src/vault.rs:220`).

### F8 — LOW — `p2/scoped-creds`: the grant token is printed to stdout by `loom auth grant issue`

`crates/loom-cli/src/commands/grant.rs:55-57` prints `token: <token>` in the human
output, and `:47` includes it in the `--json` output. The token is a bearer
capability: anyone who reads it can redeem the credential for that tool/run until
it expires.

The CLI is a deliberate part of the design (the card asked for a `loom auth` subcommand,
and the tool is how an agent obtains the grant), and the token has to reach the agent
somehow. But writing it to stdout means it lands in any shell history, CI log, or
transcript that captures stdout. The `--json` mode in particular is the mode a
harness would use to parse the grant out.

**Note (not a blocking fix):** the branch's design deliberately excludes a
`redeem` subcommand (`crates/loom-cli/src/commands/grant.rs` module doc: redemption
"belongs to a trusted broker … not to a shell an agent can reach"), so a stolen
token is useless *without* a broker. That is the mitigation. The reviewer suggests
the trust-model doc (`docs/architecture/grant-trust-model.md`) state plainly that
the token is a bearer capability and that `loom auth grant issue` should be run with
stdout redirected when the transcript is not trusted.

### F9 — LOW — `p2/audit-export`: `SecurityConfig` loses its `Default` derive for no reason

`crates/loom-core/src/types.rs` — `SecurityConfig` keeps
`#[derive(Debug, Clone, Copy, Serialize, Deserialize)]` but the derive list loses
`Default`, and a manual `impl Default for SecurityConfig` is added
(`require_signature: false, audit_retention_days: 365`).

The manual impl is correct and matches the previous `Default::default()` output
exactly (`require_signature: false`), so this is **not** a behaviour change. It is
only worth noting that the `#[serde(default = "default_audit_retention_days")]`
attribute on the new field and the manual `Default` impl are two separate
definitions of the same default (365). They agree today; the reviewer recommends
the manual impl call `default_audit_retention_days()` so there is one constant.
Minor, non-blocking.

### F10 — LOW — `p2/audit-export`: `loom auth verify` without a path reports four different flags that are all the same boolean

`crates/loom-cli/src/commands/auth.rs:220-236`: when no path is given, the local
chain is verified and an `ExportVerification` is synthesised with `digest_ok`,
`chain_linked`, `chain_head_ok` and `source_chain_intact` all set to
`report.intact`. The JSON output then shows four distinct-looking checks that are
one check.

This is honest (the doc comments explain each flag) but a consumer parsing the
JSON will read four independent verdicts where there is one. A `verifies: "local_chain"`
discriminator, or omitting the export-specific flags in the local-chain case,
would remove the ambiguity. Non-blocking.

### F11 — INFO — merge conflicts: `p2/scoped-creds` and `p2/audit-export` conflict in four files

`git merge-tree` / a real `git merge --no-commit --no-ff p2/scoped-creds
p2/audit-export` in a throwaway worktree produce four content conflicts:

* `crates/loom-auth/src/lib.rs` — `pub mod grant;` + `use grant::{…}` vs
  `pub mod export;` + `pub use export::{…}`. Trivially resolvable (keep both).
* `crates/loom-cli/src/cli.rs` — `AuthAction::Grant { action: GrantAction }` +
  the new `pub enum GrantAction` vs `AuthAction::Export { … }` / `AuthAction::Verify { … }`.
  Structurally compatible once both are kept, but note the **name collision**:
  both branches introduce a `Verify` concept (`GrantAction::Verify` for grant
  verification, `AuthAction::Verify` for export verification) under the same
  `loom auth` group. They live in different enums so Rust accepts it, but
  `loom auth verify` (export) and `loom auth grant verify` (grant) must be
  documented unambiguously, and the `AuthAction::Verify` doc comment should say
  it is *export* verification.
* `crates/loom-cli/src/commands/auth.rs` — the `AuthAction::Grant` arm vs the
  `cmd_export` / `cmd_verify` arms. Trivially resolvable.
* `AGENTS.md` — the CLI-docs section and the new "Audit retention" section.

`p2/scoped-creds` + `p2/container-policy` is **clean**, and
`p2/container-policy` + `p2/audit-export` is **clean**. Only the
scoped-creds/audit-export pair conflicts, and all four are mechanical. The
merge itself was NOT performed (review-only).

### F12 — INFO — `p2/container-policy`: the Docker `auto_remove` and k8s `NetworkPolicy` changes also affect the Podman's `remove_container`, which is NOT 404-tolerant

`crates/loom-runtime/src/lib.rs:1349` (podman) still bails when `podman rm` fails
(`"Failed to remove container"`), unlike the Docker client's new 404 tolerance
(`lib.rs:1191`). On `main` the Docker client also had no 404 tolerance
(`main:crates/loom-runtime/src/lib.rs:921-931`). This is consistent with the
finding F3 fix (option (a)): if `auto_remove` stays `true` on Docker, the runtime's
own `remove_container` will hit a 404 and must tolerate it — the branch's Docker
client already does. The podman path is unaffected by the `auto_remove` flag
(podman has no equivalent), so it is not a defect, just a note for whoever applies
the F3 fix.

---

## What was checked and found CLEAN

These were explicit review criteria and they pass. Recording them so the verdict is
not mistaken for a blanket rejection.

**`p2/scoped-creds` — correctness and security**

* **The master secret never reaches the agent.** `derive_grant_key`
  (`crates/loom-auth/src/vault.rs:276`) returns a 32-byte key =
  `SHA-256("loom-grant-key-v1" ‖ master_key ‖ len‖provider_id ‖ len‖secret ‖
  blob.nonce)`; the secret is `zeroize()`d inside. The AEAD plaintext is the fixed
  marker `b"loom-grant-v1"` (`grant.rs:319` `mint_token`), never the credential.
  `redeem_grant` (`lib.rs:323`) returns plaintext but is a **library-only** API —
  `grep -rn "redeem_grant" crates/` outside `loom-auth` returns nothing, and there
  is deliberately no `redeem` CLI subcommand. The derivation is not reversible to
  the master.
* **Revocation is enforced at redeem time, not merely recorded.**
  `verify_grant` (`lib.rs:243`) checks `record.revoked` at `:251` *before* the AEAD
  open, and `redeem_grant` calls `verify_grant` (`:323`) — so a revoked grant
  cannot be redeemed. `GrantStore::reload_if_changed` (`grant.rs:199`) compares a
  SHA-256 of the file **content** (`:281` `file_stamp`), so a revocation written by
  another process is visible on the next use; `test_revocation_takes_effect_in_another_manager_instance`
  covers this.
* **Expiry is checked at USE time.** `lib.rs:256` `record.is_expired(now)` on
  every `verify_grant`; `grant.rs:93` `is_expired` is `now >= self.expires_at`.
  The card's "fails after expiry" test exists
  (`crates/loom-auth/tests/grant_test.rs::test_grant_fails_after_expiry`).
* **Key rotation invalidates outstanding grants.** Both rotations change
  `grant_key`: a master-password change changes the master key; a credential
  re-store writes a fresh blob nonce, which is mixed in as a storage-epoch marker
  (`vault.rs:290-293`, documented in `docs/architecture/grant-trust-model.md`
  "Rotation"). Tests: `test_rotated_master_password_invalidates_grants`,
  `test_rotated_credential_invalidates_grants`.
* **The audit chain is still hash-chained and `verify_integrity` still passes.**
  `audit_grant` (`lib.rs:355`) builds a `loom_core::AuditEntry` and calls
  `self.audit.log(&entry)` — the **same** `AuditLogger`, same chain, no second
  channel. `export_credential` (`lib.rs:425`) is unchanged from main. The branch
  does not touch `crates/loom-auth/src/audit.rs` at all.
* **No new crypto dependency.** `git diff main..p2/scoped-creds -- '*Cargo.toml'
  '*Cargo.lock'` is **empty**. The branch uses only crates `loom-auth` already
  depended on: `sha2`, `chacha20poly1305`, `argon2`, `zeroize`. The only
  `Cargo.toml` touch anywhere in the branch is `mod hex` → `pub(crate) mod hex` in
  `crates/loom-auth/src/vault.rs` so `grant.rs` can reuse the hand-rolled hex
  helpers — no new crate.
  *Not a finding, but a note:* the derivation uses plain SHA-256 as a KDF, not
  HKDF. The branch documents this explicitly and honestly
  (`docs/architecture/grant-trust-model.md:39-45`: "it is **not** HKDF, and it is
  not a password KDF"). The inputs are high-entropy (master key + stored
  credential), so this is acceptable; it is documented rather than hidden.

**`p2/container-policy` — correctness and security**

* **The default posture is fail-closed.** `NetworkPolicy` has `#[default]` on
  `NoNetwork` (`lib.rs:77-80`) and all three constructors
  (`docker()` `:778`, `podman()` `:787`, `kubernetes()` `:797`) set
  `network_policy: NetworkPolicy::default()`. `engine_network_mode` (`:125`) maps
  both `NoNetwork` and `Restricted` to `Some("none")`; only `Unrestricted` returns
  `None` (engine default = full egress).
* **An unknown/garbage posture cannot fall back to unrestricted.**
  `NetworkPolicy::parse` (`:103`) accepts only `none` / `restricted` /
  `unrestricted` (case-insensitive) and `restricted:<host>[,<host>]`; everything
  else is `anyhow::bail!("unknown network policy '{other}' …")`.
  `resolve_network_policy` (`:181`) propagates that error rather than guessing.
  This is the single most important property of the branch and it holds.
* **`restricted` is fail-closed, not silently unrestricted** — see F4 for the
  reporting gap, but the *enforcement* is correct: `Restricted` resolves to
  `--network none` on Docker/Podman and to an empty-egress NetworkPolicy on k8s.
* **The container path is still opt-in; a default `process` runtime does not lose
  networking.** `RuntimeRegistry::new()` defaults to `"process"`
  (`lib.rs:245-248`) and `RuntimeRegistry::default()` registers `ProcessRuntime`
  (`lib.rs:294`). `ProcessRuntime` has no network concept at all. The posture only
  applies once a container backend is selected. Documented in AGENTS.md ("Container
  Network Posture"), README.md (§Execution) and ROADMAP.md (2026-10-21 decision
  entry) — the card required the documentation change to land with the behaviour
  change, and it did.
* **One owner for the policy.** `NetworkPolicy::parse` exists only in
  `loom-runtime`; `crates/loom-core/src/types.rs:269` and
  `crates/loom-generic-adapter/src/config.rs:257` keep `pub network: Option<String>`
  and were changed **doc-comment only** (the diff is +5/-1 in each). The adapter
  copies the raw string (`crates/loom-generic-adapter/src/lib.rs:167`) and
  `loom-runtime` parses it in exactly one place (`lib.rs:903`). No duplicated
  parser.

**`p2/audit-export` — correctness, config, and redaction**

* **Secrets are redacted from the export.** The chain's only free-text field is
  `result`; `normalize_outcome` (`export.rs:374`) maps it to
  `granted` / `denied` / `other` and **never** exports the raw string. The raw
  value is the only place a caller can stuff arbitrary content into the chain, and
  it does not leave the machine. The count is reported in
  `redaction.redacted_entries` and the policy name is `result-normalised`
  (`export.rs:271`). Test: `test_export_never_contains_secret_value`
  (`crates/loom-auth/src/export.rs` in-file) logs `denied: sk-live-SUPERSECRETVALUE`
  and asserts the raw JSON does not contain it; CLI-level
  `test_export_never_writes_secret_values` too. AGENTS.md Gotcha 17 documents it.
* **The retention rule is actually applied and is configurable through the
  EXISTING five-layer merge — no second mechanism.** `SecurityConfig.audit_retention_days`
  (`crates/loom-core/src/types.rs`) defaults to 365
  (`DEFAULT_AUDIT_RETENTION_DAYS`) and is read in exactly one place:
  `retention_days()` (`crates/loom-cli/src/commands/auth.rs:280`) via
  `ConfigReader::new(loom_dir).load()`. The five-layer merge already carries a
  `security` section (`crates/loom-core/src/config.rs:198`
  `("security.toml", Some("security"))`), so the knob is settable from
  `config.toml`, `config/security.toml`, an `extends` file, or a layer overlay —
  no new config mechanism was invented. Default is safe (365 days).
* **Retention behaviour is documented and consistent.** `build_export`
  (`export.rs:204`) drops the longest **prefix** of out-of-window entries
  (`:218-225`) and exports the rest (`:230`); the doc comment at `:186-203`
  explains why the drop must be a prefix (the chain is linear — dropping a middle
  entry breaks the linkage `verify_export` checks). The on-disk chain is never
  rewritten; disk stays bounded by the logger's existing size-based rotation
  (`crates/loom-auth/src/audit.rs:12` `get_max_log_size`, `MAX_ROTATED_FILES = 5`).
  AGENTS.md has a new "### Audit retention" section stating the same thing.
  *This was a real bug in the branch's first commit (`64d861d`), which dropped a
  scattered subset; it was fixed in `45a6c27` before this review.*
* **One owner for the retention rule.** The default constant lives in
  `loom-core::types`, the rule in `loom-auth::export::build_export`, the read in
  the CLI's `retention_days()`. One read path, no duplication.
* **No new dependency.** `git diff main..p2/audit-export -- '*Cargo.toml'
  '*Cargo.lock'` is empty.
* **The export reuses `verify_integrity` — the chain hashing is NOT
  reimplemented.** `crates/loom-auth/src/audit.rs` was refactored so
  `verify_integrity` calls a new `pub fn entries_chronological()`
  (`audit.rs:311`) and reuses the existing `compute_entry_hash` (`audit.rs:384`)
  and `read_last_entry_hash` (`audit.rs:397`). The algorithm and the rotated-file
  ordering are identical to main's inline loop (`main:crates/loom-auth/src/audit.rs:313-365`).
  `build_export` calls `logger.verify_integrity()?` (`export.rs:205`) — the card's
  HARD RULE 1 is satisfied in the "reuse, don't reimplement" sense. (See F1/F2 for
  what the *export* still fails to verify.)

**Test honesty**

* Every test on all three branches uses `tempfile::TempDir::new()` /
  `tempfile::tempdir()`. No test touches the real `~/.loom` or a real vault. The
  `LOOM_AGENTS_DIR` indirection is respected.
* The 12 pure-logic tests in `crates/loom-runtime/tests/network_policy.rs` assert
  real values (`engine_network_mode() == Some("none")`,
  `resolve_network_policy(…).is_err()` for garbage, per-run value beating the
  runtime default). **None of them would pass with the feature removed** — they
  assert the enum's behaviour, not a tautology.
* `crates/loom-runtime/tests/container_smoke.rs` is gated behind
  `LOOM_CONTAINER_SMOKE == "1"` and **skips** (returns, printing `skipped: …`)
  rather than fails when the env var is unset, the `docker` feature is off, `docker
  version` fails, or the runtime cannot reach an engine. Verified both ways:
  * without the env var: `2 passed; 0 failed` (both took the skip path);
  * **with a real Docker daemon present on this machine**
    (`docker version --format '{{.Server.Version}}'` → `29.8.2`),
    `LOOM_CONTAINER_SMOKE=1 cargo test -p loom-runtime --features docker --test
    container_smoke -- --nocapture` → `2 passed; 0 failed` in 6.84 s, i.e. the
    smoke test **really ran** and asserted `NetworkMode == "none"` and that the
    container has no `eth0`. The skip path is not a silent pass — the assertions
    are real and they pass against a live engine.
* The 16 tests in `crates/loom-auth/tests/grant_test.rs` and the 6 CLI tests in
  `crates/loom-cli/src/commands/auth.rs` are behavioural, not tautological:
  grant works for its tool/run, fails for another tool, fails for another run,
  fails after expiry, fails after revocation, revocation appears in the audit
  chain, revocation is visible from another manager instance, both rotations
  invalidate grants, the stored secret never appears in a grant, ACL denial blocks
  issuance *and* redemption, a locked vault blocks grants.
* The export tests cover: export writes a file, a re-imported export verifies
  intact, a tampered export fails, a tampered `source` block fails, a deleted
  entry breaks linkage, a secret value never appears, retention drops the
  out-of-window prefix, everything-dropped still verifies, legacy unchained
  entries are tolerated.
  *Caveat:* the "tampered export fails" test tampers **without** recomputing the
  digest (`crates/loom-cli/src/commands/auth.rs:520-647`,
  `test_verify_rejects_tampered_export` replaces `provider-1` → `provider-9`). It
  therefore only proves the digest check works, not that the chain linkage does.
  That is precisely the gap F1 exposes. The test is not tautological — it fails
  if the digest check is removed — but it is weaker than it looks.

**CI reality**

* The `container` job **exists** in `.github/workflows/ci.yml` on
  `p2/container-policy` (`runs-on: ubuntu-latest`, `timeout-minutes: 30`).
* It is **wired to the trigger**: the workflow's `on:` is `push: branches
  [main, develop]` + `pull_request`, and the `container` job has **no `if:`
  gate** — it runs on every push to main/develop and every PR.
* It is **not** `continue-on-error`. The smoke test step runs
  `LOOM_CONTAINER_SMOKE=1 cargo test -p loom-runtime --features docker --test
  container_smoke -- --nocapture 2>&1 | tee container-smoke-output.log` under
  `set -o pipefail`, and a follow-up guard fails the job if the log contains
  `skipped:` — so a runner with Docker present cannot silently skip. That guard
  is exactly the anti-tautology measure the card asked for.
* The existing jobs (the `test` matrix, `integration`, `benchmarks`, `security`) are
  unchanged in structure — the card's "without restructuring the existing workflow"
  rule is met.
* One gap: the `container` job runs `cargo test -p loom-runtime --features docker
  --test container_smoke` only. The **pure-logic** policy tests
  (`crates/loom-runtime/tests/network_policy.rs`) are picked up by the normal
  `cargo test --workspace --exclude loom-tui --all-features` matrix entry
  (ci.yml:57), so they do run in CI. Confirmed — no gap.

**Scope**

* No new CLI group on any branch. `p2/scoped-creds` adds `loom auth grant
  {issue,verify,revoke,list}` and `p2/audit-export` adds `loom auth
  {export,verify}` — both under the **existing** `loom auth` group, which the
  card explicitly allows.
* No new crate, no new dependency on any branch (all three `Cargo.toml`/`Cargo.lock`
  diffs are empty).
* `p2/container-policy` changes `crates/loom-core/src/types.rs` and
  `crates/loom-generic-adapter/src/config.rs` — **doc comments only** on
  `pub network: Option<String>`. The field type is unchanged; no behaviour
  outside the card's scope there.
* The one out-of-scope behaviour change found is F3 (the Docker `auto_remove`
  flip and its consequence for the TUI path).

**Board / branch state**

* `p2/scoped-creds` card `e06138a3` — **done**, 1 execution, succeeded.
* `p2/container-policy` card `10341c12` — **done**, 2 executions (the first was
  cancelled, the second succeeded).
* `p2/audit-export` card `d6c935ff` — was **running** with two prior `max-tokens`
  failures when the review started; it has since completed as **done** with
  execution `46ba7cea` **succeeded**. The branch tip moved `64d861d` → `45a6c27`
  accordingly. The review above is against the final tip.

---

## VERDICT

**FIX FIRST**

The three branches are close, and the two security-critical ones are structurally
sound. `p2/scoped-creds` is in good shape: the master secret never leaves the
vault, revocation and expiry are genuinely enforced at use time, both rotations
invalidate outstanding grants, the audit chain is untouched, and there is no new
crypto dependency. `p2/container-policy` gets the single most important property
right — the default posture is fail-closed and a garbage posture value can never
fall back to unrestricted — and it keeps the container path opt-in, with the
documentation change landing in the same commit as the behaviour change, as the
card demanded. `p2/audit-export` gets the config, redaction and retention
mechanisms right, and reuses `verify_integrity` rather than reimplementing the
chain.

But `p2/audit-export` fails the card's first HARD RULE in substance: the export is
not verifiable against a party who can recompute the digest. I demonstrated both a
field-level tamper and a **complete forgery** passing `verify_export` with
`intact: true`. The exported `result` is redacted, so the chain hashes cannot be
re-derived from the exported fields either, and `verify_export` never recomputes
them. For a compliance artifact whose whole purpose is to be checkable by a third
party, that is a disqualifying gap. It is also fixable without new dependencies
and without leaving the "reuse `verify_integrity`" constraint: export the
hash-relevant preimage (or a keyed digest) and recompute the chain in
`verify_export` using the existing `compute_entry_hash`.

Specific fixes required, by owner:

1. **`crates/loom-auth/src/export.rs`** — make the export self-verifying. Export
   enough to re-derive each entry hash (or use a keyed digest), and have
   `verify_export` recompute the chain hashes and compare them against the
   exported `entry_hash` values, reusing `compute_entry_hash`
   (`crates/loom-auth/src/audit.rs:384`). Re-derive the redaction rule and the
   verification rule **together** — a redacted `result` cannot be re-hashed.
   Add a test that tampers an entry *and* recomputes the export digest, and a
   test that forges a whole export; both must fail verification. *(F1, F2)*
2. **`crates/loom-runtime/src/lib.rs:1109`** — resolve the Docker `auto_remove`
   flip. Preferred: restore `auto_remove: Some(true)` and rely on the
   404-tolerant `remove_container` already added at `:1191`; alternatively keep
   `Some(false)` and add the missing `runtime.cleanup(&result)` to
   `crates/loom-tui/src/dispatch.rs:699`. Also add a note to the k8s
   `apply_network_policy` doc comment (`:505-512`) that an empty egress list only
   denies egress when the cluster CNI enforces NetworkPolicy. *(F3, F5)*
3. **`crates/loom-runtime/src/lib.rs`** — create the Kubernetes NetworkPolicy
   **before** the Job (`:666-673`); the pod selector is already computable from
   `spec.labels["loom.task_id"]` (`:618-628`, set at `:1022`). *(F5)*
4. **`crates/loom-runtime/src/lib.rs:903-904`** — log when the requested posture
   and the resolved posture differ (e.g. `restricted` → `none`, allowlist not yet
   enforced), so an operator asking for `restricted:api.example.com` is not
   silently given total isolation. *(F4)*
5. **`crates/loom-auth/src/grant.rs:98`** — delete the unused `is_usable`, or make
   `verify_grant` use it. One owner for the grant-check rule. *(F6)*
6. **`crates/loom-auth/src/grant.rs:212`** — set `0o600` on `auth/grants.json`
   (and, if the reviewer agrees, on the vault blobs at
   `crates/loom-auth/src/vault.rs:220`). The registry is the revocation source of
   truth. *(F7)*
7. **`crates/loom-cli/src/commands/auth.rs:220-236`** — disambiguate the local-chain
   verification result so the JSON does not present one check as four. *(F10)*
8. **`crates/loom-core/src/types.rs`** — have the manual
   `impl Default for SecurityConfig` call `default_audit_retention_days()` so the
   365 default is defined once. *(F9)*
9. **`crates/loom-cli/src/cli.rs`** — when resolving the F11 conflict, document
   that `loom auth verify` means *export* verification and `loom auth grant
   verify` means *grant* verification, in both the CLI help and AGENTS.md.

`p2/scoped-creds` is **SHIP** on its own (findings F6–F8 are minor and none of them
block the card's acceptance criteria). `p2/container-policy` is **FIX FIRST** on F3
and F5 (F4 is a reporting gap). `p2/audit-export` is **FIX FIRST** on F1/F2.

---

## Commands run, and their results

All commands were run in the four existing worktrees
(`.worktrees/p2-{scoped-creds,container-policy,audit-export,review}`), each
checked out at its branch tip. `main` was used as the baseline to prove
pre-existing failures. The root working tree was left clean; the only file I
created and removed was a throwaway probe test
(`.worktrees/p2-audit-export/crates/loom-auth/tests/zz_probe_tamper.rs`), used
for the F1/F2 empirical proof and deleted afterwards.

### Build matrix (each worktree: `cargo test -p loom-auth`, `cargo test -p loom-runtime`, `cargo test -p loom`, `cargo check --workspace`, `cargo clippy --workspace`)

| Worktree | `loom-auth` | `loom-runtime` | `loom` (CLI) | `check --workspace` | `clippy --workspace` |
|---|---|---|---|---|---|
| `main` (baseline) | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 |
| `p2/scoped-creds` | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 |
| `p2/container-policy` | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 |
| `p2/audit-export` | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 | EXIT 0 |

**No pre-existing failures on `main`** — the baseline matrix is green, so there is
nothing to attribute to the branches. Zero `test result: FAILED` lines in any log.

### Feature-gated builds (container-policy)

| Command | Result |
|---|---|
| `cargo check -p loom-runtime --features docker` | exit 0 |
| `cargo check -p loom-runtime --features kubernetes` | exit 0 ("Finished dev profile in 2m 03s") — the k8s NetworkPolicy code compiles |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (container-policy) | exit 0 ("Finished in 1m 17s") |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (scoped-creds) | exit 0 ("Finished in 4m 39s") |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (audit-export) | exit 0 ("Finished in 4m 43s") |

The `--all-features -- -D warnings` run is the CI clippy invocation
(`.github/workflows/ci.yml:21`), so all three branches are clippy-clean under the
CI's own gate.

### Container smoke test — both the skip path and the real path

| Command | Result |
|---|---|
| `cargo test -p loom-runtime --features docker --test container_smoke` (no env var) | `2 passed; 0 failed` — both took the **skip** path |
| `docker version --format '{{.Server.Version}}'` | `29.8.2` — a real daemon is reachable on this host |
| `LOOM_CONTAINER_SMOKE=1 cargo test -p loom-runtime --features docker --test container_smoke -- --nocapture` | `2 passed; 0 failed` in 6.84 s — the test **really ran** against a live engine and asserted `NetworkMode == "none"` and no `eth0` |

The skip path is therefore proven to skip, and the assertions are proven to be real
(asserted against a live container, not skipped into a vacuous pass).

### F1/F2 empirical proof (throwaway probe, since removed)

```
# 1. Edit one field of a real export, then recompute the digest exactly as
#    export.rs does (unkeyed SHA-256 over the document with the digest blanked).
PROBE tampered+rehashed: ExportVerification { intact: true, digest_ok: true,
  chain_linked: true, chain_head_ok: true, source_chain_intact: true, entries: 1, problems: [] }
TAMPER SURVIVED VERIFICATION

# 2. Forge a complete export from scratch — self-consistent hash chain, no
#    audit log behind it at all.
PROBE forged: ExportVerification { intact: true, digest_ok: true, chain_linked: true,
  chain_head_ok: true, source_chain_intact: true, entries: 3, problems: [] }
FORGERY SURVIVED
```

### Dependency hygiene

| Command | Result |
|---|---|
| `git diff main..p2/scoped-creds -- '*Cargo.toml' '*Cargo.lock'` | **empty** — no new dependency, **no new crypto crate** |
| `git diff main..p2/container-policy -- '*Cargo.toml' '*Cargo.lock'` | **empty** |
| `git diff main..p2/audit-export -- '*Cargo.toml' '*Cargo.lock'` | **empty** |

### Merge check (performed in the throwaway worktree, then aborted)

```
git worktree add .worktrees/p2-review -b p2/review main
git merge --no-commit --no-ff p2/scoped-creds p2/container-policy p2/audit-export
→ conflicts in 4 files: AGENTS.md, crates/loom-auth/src/lib.rs,
  crates/loom-cli/src/cli.rs, crates/loom-cli/src/commands/auth.rs
git merge --abort
```

`p2/scoped-creds` + `p2/container-policy` — **clean**.
`p2/container-policy` + `p2/audit-export` — **clean**.
`p2/scoped-creds` + `p2/audit-export` — 4 conflicts (F11), all mechanical.
**No merge to `main` was performed and nothing was pushed.**
