# ADR 0005: Plugin verification is the default path

Date: 2026-10-26
Status: Accepted

## Decision

Verification is the default path for plugin installs from a remote URL.
`security.require_signature` defaults to `true`; an unsigned remote
install is refused with the opt-out named in the refusal message.
Additionally, every **successful signed install** writes a key-continuity
pin (`{loom_dir}/auth/plugin-pins.json`, mode 0600, `{tool_name:
public_key_hex}`). Later installs of the same tool name signed with a
**different valid key** are refused until the user deletes the pin entry
— an explicit, auditable acknowledgement, never a silent swap.

## Context

`loom plugin install <name> <url>` downloads an arbitrary TOML from an
arbitrary HTTPS URL and executes it as a tool config (the TOML becomes a
launchable tool with commands). Before this ADR, Ed25519 verification
existed but was **opt-in**: `require_signature` defaulted to `false`
(`crates/loom-core/src/types.rs`, test `security_require_signature_*`
previously asserted "defaults off"). A default-off verify switch against
an agent-run supply chain is greenwashing — the same evidence class as
the pillar-1 story where panics protected code paths no agent ever
exercised. The roadmap names this as the remaining pillar-2 item.

## Alternatives

1. **Accept only signed artifacts, no exit hatch.** Rejected: a hard
   cutover bricks every existing unsigned user at upgrade with no
   migration path (the hardcoded flag in every config would otherwise
   be needed).
2. **Keep opt-in.** Rejected: that is precisely the greenwash this ADR
   closes; the pillar-2 review explicitly retains this item.
3. **Verify-by-default with named opt-out (chosen).** Users who must
   install unsigned plugins flip one documented knob
   (`[security] require_signature = false`) — an explicit, visible
   posture change, not a silent allowance.

## Implementation notes (crates/loom-core/src/plugin_registry.rs)

- `install_from_url` delegates the trust work to `install_from_content`
  (download are the network step; every trust decision is testable
  without a server).
- `enforce_trust` gates **before anything is written**: verify when the
  artifact carries both `public_key` + `signature`; refuse half-signed
  artifacts (as before); refuse unsigned when `require_signature` is
  true. Passed a `require_signature` parameter instead of reading
  config — the CLI (`crates/loom-cli/src/commands/plugin.rs`) resolves
  it from `ConfigReader` and passes it down.
- `update()` passes the same gate on freshly downloaded content.
- `check_and_write_pin` implements TOFU-with-acknowledgement: first
  signed install pins the raw 32-byte key (hex) alongside tool name;
  later installs match the pin or refuse with the pin file path named.
- **Unsigned installs write no pin** — pin records only exist for keys
  that produced a verified install.
- **Local registration is unchanged** (`loom plugin register <name>
  <path>` / `PluginAction::Register`): registering a tool TOML you
  wrote yourself from local disk is not remote supply chain; there is
  no download to verify against. Noted here so the asymmetry is
  deliberate and documented, not accidental.

## Consequences

- Existing users installing signed plugins: unchanged behavior, plus a
  pin record on first install.
- Existing users installing unsigned plugins: a clear refusal naming
  the opt-out knob — no silent brick, no silent downgrade.
- The same flip applies to hooks: `loom-hooks` skips unsigned hooks
  when `require_signature` is true, so unsigned hook configs now
  silently stop running by default (fail-closed, not a crash; the skip
  is logged). Users relying on unsigned hooks should set the opt-out
  or sign their hooks.
- Signing keys are *extracted from the artifact itself*; there is no
  publisher registry. The pin store is what makes key continuity
  detectable after first contact.

## Residual risks

- **First-contact key theft (TOFU bootstrap).** The pin trust check
  narrows the MITM window to exactly one install: the first signed
  install of a tool name records whatever key was presented. An
  attacker who controls that first session can pin their own key.
  This risk cannot be removed without anchoring the default key in the
  Loom binaries themselves (a signed default publisher key embedded in
  release artifacts). Idiomatic distribution now favors the pinned-key
  submodule + config extends (`docs/routing-debt.md`), which bypasses
  the URL-download path entirely, shrinking the exposure of this
  residual risk.
- Pin store tampering: the file is user-owned local state (0600).
  Deleting an entry is the acknowledgement mechanism itself, so a
  tampered store degrades to unpinned, not to silently-broken.
