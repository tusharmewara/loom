# ADR 0006: Vault broker v1 — admin/member RBAC roles

## Status

Accepted.

## Context

`loom-auth` has a vault, ACL engine, grant store, and audit logger — but no
concept of org members vs. admins. Every identity with vault access can list
all grants, export any credential, and read the full audit log. Phase 4
(vault broker v1) adds role-based access so that members cannot read other
members' grants or the full audit log, while admins retain full access.

No user accounts exist in loomd; roles are keyed off org membership supplied
by the control plane (phase 3 provides membership caching at
`{loom_dir}/org/membership.json`) with an offline interim in
`{loom_dir}/config/org.toml`.

## Decision

**Two roles**: `Admin` and `Member`.

- `Admin` — full access: list/inspect all grants, read full audit log,
  export any credential, modify ACL rules.
- `Member` — scoped access: own grants only (grants scoped to their tool
  identity), own audit entries only, own credentials only; cannot modify ACL
  rules.

**Role resolution order** (first match wins):

1. `{loom_dir}/auth/roles.json` — explicit `{"roles": {"<identity>": "admin"}}`
   override. Admin can pin a role regardless of CP membership.
2. `{loom_dir}/org/membership.json` — CP-downloaded org membership; identity
   listed under org `admins` → Admin, otherwise → Member.
3. `{loom_dir}/config/org.toml` — offline interim `[org] members` /
   `[org] admins`. Replaced by CP download once a connection exists.
4. Default → `Member` (conservative: least privilege).

**Identity** = requesting tool / provisioned context string, not a human
login. No human login / SSO this phase (phase 8 enterprise).

**API surface** (additive only, no public signature changes):

- `AuthManager::role_store() -> &RoleStore`
- `AuthManager::is_admin(identity) -> bool`
- `AuthManager::is_member(identity) -> bool`
- `AuthManager::require_admin(identity) -> Result<()>`
- `AuthManager::list_grants_for(identity) -> Vec<GrantRecord>`
- `AuthManager::export_credential_for(provider, requesting_tool, identity)`
- `AuthManager::redeem_grant_for(token, tool, run, identity) -> Result<String>`
- `AuthManager::audit_entries_for(identity) -> Vec<AuditEntry>`

Existing `list_grants()`, `export_credential()`, `redeem_grant()`, and
`audit()` remain unchanged (admin view). Member-scoped variants are additive.

**Storage**: `{loom_dir}/auth/roles.json` at mode 0600, consistent with
vault/grant 0600 precedent. `config/org.toml` parsed with the `toml` crate
(already a workspace dep).

## Consequences

### Positive

- Members cannot read other members' grants or the full audit log (phase-4
  exit criterion met).
- Additive-only on `loom-auth` — no signature changes, no breakage of
  existing callers (`loom-cli`, `loomd`, `loom-mcp`).
- Explicit `roles.json` override lets admins pin roles independent of CP
  membership (useful before CP connection exists).
- Offline interim via `config/org.toml` means RBAC works without a CP.

### Negative

- Identity = tool name means 1:1 mapping; no multi-tool identity this phase.
- CLI `loom auth` role gating deferred (broker library only this phase).
- No per-provider/per-tool ACL matrix (v2 follow-up).

## Alternatives considered

- **Full RBAC matrix** (per provider × per tool × per role): Rejected —
  over-engineered for phase 4; the exit criterion only requires
  member-vs-admin separation.
- **Human login + SSO**: Rejected — no user accounts this phase; phase 8.
- **Three roles** (admin/member/read-only): Rejected — adds complexity
  without a clear use case; two roles suffice.

## Related

- ADR 0005 (plugin verify-by-default): same 0600 file-permission precedent.
- Phase 3 spec: CP provides `OrgClient` + membership cache at
  `{loom_dir}/org/membership.json`.
- `blueprint/commercialization/plan-commercialization.md` §phase 4.
