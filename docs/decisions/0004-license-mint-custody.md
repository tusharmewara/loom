# ADR 0004 — License mint and private-key custody

**Status:** Ratified (2026-10-26, PR4 preflight)

**Context.** The commercialization plan (pillar 4 phase 2, license gate) needs a
signed Ed25519 license key to exist before its exit test can run, but the
control plane that would issue keys doesn't exist until phase 6. The gate would
otherwise be blocked by it. Crypto substrate is already in the workspace
(`ed25519-dalek` in loom-core, keygen/0600-keyfile patterns in loom-auth).
Note: `loom-hooks/src/signing.rs` is not wired (`no mod signing;` in lib.rs) and
loom-core cannot depend on loom-hooks, so it is not a reusable home.

**Decision.**

1. **Placement — `crates/loom-core/src/license.rs`.** `LicensePayload` +
   `mint_license` (the ONE mint path: control-plane issuance at phase 6 calls
   the same function, never a parallel path) + `verify_license_key` (what
   phase 2's license gate consumes). loom-core already has ed25519-dalek +
   base64; only the `rand_core` keygen feature was added. Dependency arrows
   point the right way for phase 2 (`loom-core` is the plan's named home).

2. **Wiring — separate `loom-admin` binary, not a hidden `loom admin`
   subcommand.** Two Cargo.toml lines vs. a command-enum entry + dispatch arm
   in the customer-facing CLI. The vendor's `loom --help` surface stays
   untouched, and key operations never share a process with the customer
   daemon. Key dir: `LOOM_ADMIN_DIR` env override, else `~/.loom-admin/` —
   separate from `~/.loom/` by design.

3. **Surface.** `loom-admin keygen [--force]` writes `<dir>/license.key`
   (Ed25519 hex, 0600, refuses overwrite without `--force`) and prints only the
   public key. `loom-admin licensemint --org --tier --seats --expires
   <RFC3339> [--feature ...] [--out path]` emits
   `base64url(payload JSON).base64url(sig)`. Mint validates the model
   invariants up front: non-empty org/tier, seats ≥ 1, expiry in the future
   and ≤ 365 days (keys dye with expiry — a longer key could never verify).

**Alternatives considered.**

- *CP-only issuance — rejected:* blocks phase 2's exit before the CP exists.
- *Hosted mint service — rejected:* expands the trust perimeter for the vendor
  root key; offline keygen + manual key delivery keeps it on the maintainer's
  machine.
- *Hidden `loom admin` subcommand — rejected:* couples vendor-only surface into
  the shipped user CLI for no reuse benefit (both packages use loom-core).

**Consequences.**

- Private-key custody = maintainer laptop (`~/.loom-admin/license.key`, 0600) +
  offline backup. Key compromise = re-key everything; treat like a vendor root.
- Rotation story stays the plan's open question (rolling accepted public keys)
  — deferred to a pre-gathered-customers point, as planned.
- Pinning invariant for phase 2 unchanged: loomd's embedded public key is the
  only non-self-hostable check; a key minted with a different private key never
  verifies (tested).
