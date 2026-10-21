# Scoped Credential Grants — Trust Model

**Date:** 2026-10-21
**Status:** Implemented (branch `p2/scoped-creds`)

---

## Overview

The vault stores provider credentials encrypted (ChaCha20Poly1305) under a master key
derived from a password (Argon2id), enforces an ACL per tool, and records every access in
a hash-chained audit log. What it did not have was a *grant*: a credential an agent can
hold for a bounded scope.

Before grants, `AuthManager::export_credential` handed a tool the credential itself
(encrypted for transport, but the plaintext existed in the caller's process) for the whole
session, for any tool the ACL allowed. A grant replaces that with a derived,
time-boxed, revocable derivative.

## What a grant is

A grant is a token that authorizes **one credential, for one tool, in one run, until one
expiry**, and that reveals nothing about the stored secret.

The stored secret never appears in the token, in the grant registry, or in any log line.
The token is an AEAD ciphertext that only the vault (holding the master key) can open.

### Derivation

```
grant_key = SHA-256( "loom-grant-key-v1"
                   ‖ master_key            (32 bytes, vault-internal, never leaves the vault)
                   ‖ len(provider_id) ‖ provider_id
                   ‖ len(secret)      ‖ secret        (zeroized right after hashing)
                   ‖ blob.nonce                    (storage-epoch marker, see Rotation)
                   )
```

`grant_key` is used directly as the ChaCha20Poly1305 key. No new primitive was added:
the crate already depends on `sha2` and `chacha20poly1305`. The construction is
SHA-256 as a KDF over a secret that is already high-entropy (the stored credential);
it is **not** HKDF, and it is not a password KDF — the password → master key step
remains Argon2id, unchanged.

The token is:

```
"loom-grant-v1" ‖ "." ‖ grant_id (16 random bytes, hex)
                ‖ "." ‖ nonce (12 random bytes, hex)
                ‖ "." ‖ ciphertext (hex)
```

where `ciphertext = ChaCha20Poly1305(grant_key).encrypt(
    nonce,
    plaintext = "loom-grant-v1",
    aad = canonical(grant_id, provider_id, run_id, tool, issued_at, expires_at)
)`.

The AAD is length-prefixed, so no two scope tuples can produce the same AAD bytes.

### Verification

`AuthManager::verify_grant(token, tool, run_id)`:

1. Parse the token (format check only).
2. Look the `grant_id` up in the grant registry (`{loom_dir}/auth/grants.json`),
   reloading the file if its content hash changed — so a revocation written by another
   process is visible immediately.
3. Reject if the record is revoked.
4. Reject if `now >= expires_at` (expiry is checked **at use time**, every use).
5. Reject if `tool` or `run_id` differs from the record.
6. Recompute `grant_key` from the current master key and the current stored secret, and
   open the token. The AEAD tag authenticates the token; the AAD authenticates the
   scope and expiry. A wrong key, a rotated secret, or a tampered record fails here.
7. Write a `grant.use` entry to the existing hash-chained audit log.

## Scope

| Dimension | Bound to |
|---|---|
| Credential | one `provider_id` |
| Tool | one `tool` (the same identity the ACL checks) |
| Run | one caller-supplied `run_id` string |
| Time | `issued_at` … `expires_at` (default TTL 900s) |

A grant issued for tool X in run R fails for tool Y and for run R+1. There is no
wildcard: the scope is a single value, not a pattern.

The run identity is a plain string supplied by the caller. Loom has no first-class
run object in the auth layer, so the caller is responsible for using a unique value
per run (the CLI takes `--run`; a runtime would pass its run id). A caller that
reuses a run id across runs gets a grant that outlives the run it was meant for —
this is a caller contract, not something the vault can verify.

## Revocation

`AuthManager::revoke_grant(grant_id)` flips the `revoked` flag in the registry, saves
it, and writes a `grant.revoke` entry to the **existing** hash-chained audit log
(`{loom_dir}/auth/audit.log`, same `AuditLogger`, same chain). No second audit
channel, no new log file, no new hash chain.

Effect is immediate: the registry is the source of truth for the revoked flag, and
every verification reloads the registry if the file changed, so a revocation from
another process/manager is seen on the next use. The revoked flag is checked before
the AEAD open, so the error is explicit ("grant has been revoked") rather than an
opaque authentication failure.

Revocation is recorded, not enforced against a tampered registry: someone who can
write `grants.json` can un-revoke a grant. The audit chain still shows the original
revocation, so the tamper is visible on inspection, but the runtime does not fail
closed against it. Closing that gap needs a signed registry — out of scope here.
The AEAD does bound the damage: the scope fields (`provider_id`, `run_id`, `tool`,
`issued_at`, `expires_at`) are inside the AAD, so a tampered record cannot widen a
grant's scope or push its expiry out. The `revoked` flag and `revoked_at` are the
only fields outside the AEAD.

## Expiry

Expiry is stored in the record and enforced on every verification, not only at issue
time. A grant with a 900s TTL is rejected at `now >= expires_at` — checked before the
AEAD open, so an expired grant is rejected regardless of whether the key still matches.
Expiry trusts the local clock; a skewed host clock can extend or shorten a grant's
life. There is no monotonic-clock protection and no clock source outside the host.

## Rotation

Two rotations invalidate outstanding grants, both by changing `grant_key`:

- **Master password rotation** (`vault.change_password`) re-derives the master key.
  Every previously issued grant's `grant_key` no longer matches → verification fails.
- **Credential rotation** (`vault.set_credential` for the same provider) writes a new
  blob with a fresh nonce. The nonce is mixed into `grant_key` as a storage-epoch
  marker, so a re-stored credential also invalidates old grants even if the secret
  text is identical.

Both rotations require the vault, and both are recorded by the existing vault
behaviour (credential blobs are rewritten; the audit log keeps the grant-use trail).

## Providers: what can and cannot be scoped

Every credential Loom supports is a **bearer credential**: `AuthMethod` is one of
`ApiKey`, `OAuth2`, `DeviceCode`, `Custom`. A bearer credential is opaque to the
provider — there is no way to derive a restricted variant of it, and no provider Loom
talks to offers a scoped-credential API.

So the honest answer is: **there is no upstream scoping.** The grant is a Loom-side
*capability* over the credential, and the fallback is a **proxy**:

- The agent holds only the grant token.
- A trusted broker (the process that holds the unlocked vault) calls
  `AuthManager::redeem_grant(token, tool, run_id)`, which verifies the grant and
  returns the plaintext credential for that one call, writing a `grant.redeem` entry
  to the audit chain.
- There is deliberately **no CLI `redeem` subcommand**. A shell-capable agent must
  not be able to turn a grant back into plaintext on its own; redemption belongs to
  the broker.

Providers that *do* offer scoped tokens (none today, but e.g. cloud IAM with
per-role/per-resource service-account tokens) can be added later by storing the
provider-issued scoped token as the credential and letting the grant wrap it — the
grant machinery does not change.

## API

Library API in `loom-auth` (`crates/loom-auth/src/lib.rs`, implementation in
`crates/loom-auth/src/grant.rs`):

- `AuthManager::issue_grant(provider_id, run_id, tool, ttl_secs) -> Result<IssuedGrant>`
  — ACL-checked, audited, returns the token.
- `AuthManager::verify_grant(token, tool, run_id) -> Result<GrantRecord>` — scope,
  expiry and revocation enforced; audited.
- `AuthManager::redeem_grant(token, tool, run_id) -> Result<String>` — broker only;
  returns the plaintext credential; audited.
- `AuthManager::revoke_grant(grant_id) -> Result<bool>` — flips the flag, audited.
- `AuthManager::list_grants() -> Vec<GrantRecord>` — no secret material in the output.

The ACL check and the audit chain are unchanged: `issue_grant` calls
`acl.check_access(provider_id, tool)` exactly as `export_credential` does, and a denial
writes `log_denied_access` and refuses. `export_credential` itself was not modified.

## CLI surface

Under the existing `loom auth` group — no new top-level command:

```
loom auth grant issue  --provider <id> --run <run_id> --tool <tool> \
                        [--ttl-secs 900] [--vault-password <pw>] [--json]
loom auth grant verify --token <token> --run <run_id> --tool <tool> \
                        [--vault-password <pw>] [--json]
loom auth grant revoke --grant-id <id> [--json]
loom auth grant list   [--json]
```

The vault password may also come from `LOOM_VAULT_PASSWORD`. `revoke` and `list` do
not need the master key. There is no `redeem` subcommand — see above.

## Residual risks

1. **A grant is a bearer credential for the broker.** Whoever holds the token can
   redeem it (for the right tool and run, before expiry, unless revoked). It is not
   bound to a process, a socket, or a caller identity. Scope + TTL + revocation are
   the only limits.
2. **The registry is not signed.** Anyone who can write `grants.json` can un-revoke a
   grant or alter a record. The audit chain preserves evidence of the original
   revocation, but the runtime does not fail closed. The AEAD tag does stop a
   *modified scope* from verifying, so a tamper cannot widen a grant's scope — only
   the revoked flag and the timestamps live outside the AEAD.
3. **Redemption is plaintext at the boundary.** `redeem_grant` returns the secret to
   the broker. The grant moves the plaintext out of the agent's hands, not out of the
   system.
4. **Verification needs the vault unlocked.** The broker holds the master key in
   memory to verify a grant. A grant does not reduce the broker's blast radius.
5. **Clock trust.** Expiry depends on the host clock.
6. **Run identity is caller-asserted.** A caller that reuses a run id breaks the
   "one run" guarantee.
7. **No upstream narrowing.** The grant does not reduce what the provider credential
   can do at the provider — it constrains who in Loom may use it, for how long.

## What a grant does NOT protect against

- **A compromised broker.** If the process that holds the unlocked vault is
  compromised, the master key and the credentials are gone. The grant does not help.
- **Exfiltration of the token.** A stolen, unexpired, unrevoked grant can be
  redeemed by the thief for the same tool and run. Revocation limits the window, not
  the fact.
- **Provider-side abuse of the credential.** The token is worthless against the
  provider — but a redeemed credential is the full provider credential. A grant
  cannot restrict what a provider account is allowed to do.
- **Registry tampering** (see residual risk 2): scope tampering is caught by the
  AEAD, but a tampered `revoked` flag is not.
- **Clock manipulation** on the verifying host.
- **Anything the ACL already permits.** The grant sits *on top of* the ACL; it never
  widens access, and it does not substitute for the ACL.
