# Spec — Phase 8: Enterprise

Card: `phase8-enterprise` · seeds: `blueprint/commercialization/plan-commercialization.md` (phase 8), phases 1–7 (all merged).

## Problem

Loom has license gating, control plane, vault RBAC, registry sync, Stripe billing, and analytics — but lacks enterprise-grade SSO federation, on-prem hardening, and SLA documentation. Enterprise customers require:
- **SSO federation** — OIDC/SAML integration so orgs use their existing IdP (Entra ID, Okta, Google Workspace, ADFS)
- **On-prem hardening** — FIPS mode, air-gap support, audit logging, config encryption
- **SLA docs** — formal commitments for uptime, support tiers, data residency

## Scope (MVP)

### 1. SSO Federation (OIDC/SAML)

**Control Plane additions:**
- New org config: `auth_method` enum (`local` | `oidc` | `saml`)
- OIDC: discovery via `/.well-known/openid-configuration`, `authorization_code` flow with PKCE, `openid email profile` scopes
- SAML: SP-initiated ACS at `/api/v1/saml/acs`, metadata at `/api/v1/saml/metadata`, signature validation via `xmldsig`
- Org admin UI (server-rendered) to configure IdP connection
- On enrollment: if org has SSO enabled, redirect to IdP instead of local challenge
- JIT provisioning: first successful SSO login → create human in roster (up to seat cap)

**Client (loomd/loom org):**
- `loom org connect --sso` — initiates device-code or browser flow against CP
- Machine enrollment unchanged (still uses Ed25519 challenge-response); SSO only gates human admin actions

### 2. On-Prem Hardening

**Control Plane:**
- `--fips` flag: enables `ring` FIPS mode, rejects non-approved algorithms
- `--air-gap` flag: disables all outbound calls (no Stripe, no public registry fetch, no update checks)
- Config encryption: `loom-cp config encrypt/decrypt` — AES-256-GCM for secrets at rest (Stripe keys, SAML certs)
- Structured audit log: JSONL to stdout + optional syslog, includes: actor, action, resource, outcome, timestamp
- Health endpoint `/health/ready` (checks DB, Stripe connectivity if configured)

**Client:**
- `loom doctor --fips` — verifies FIPS-compliant crypto paths
- Offline license verification: embedded public key works without CP reachability

### 3. SLA Documentation

- `docs/enterprise/SLA.md` — 99.9% uptime target, <5min P99 latency, 4hr response for critical
- `docs/enterprise/DATA_RESIDENCY.md` — data never leaves CP's SQLite file; no telemetry exfil
- `docs/enterprise/SUPPORT_TIERS.md` — Community / Team / Enterprise response times

## Data Model

### Org SSO Config (new table `org_sso`)

```sql
CREATE TABLE org_sso (
    org_id     TEXT PRIMARY KEY REFERENCES orgs(id),
    method     TEXT NOT NULL CHECK (method IN ('oidc','saml')),
    -- OIDC
    issuer_url      TEXT,
    client_id       TEXT,
    client_secret   TEXT,  -- encrypted at rest
    -- SAML
    entity_id       TEXT,
    sso_url         TEXT,
    slo_url         TEXT,
    x509_cert       TEXT,  -- encrypted at rest
    -- Common
    attribute_map   TEXT,  -- JSON: email, name, groups claim mapping
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### Audit Log (new table `audit_log`)

```sql
CREATE TABLE audit_log (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp    TEXT NOT NULL DEFAULT (datetime('now')),
    actor_type   TEXT NOT NULL,  -- 'human' | 'machine' | 'system'
    actor_id     TEXT NOT NULL,
    action       TEXT NOT NULL,
    resource     TEXT,
    outcome      TEXT NOT NULL CHECK (outcome IN ('success','failure')),
    details      TEXT  -- JSON
);
```

## API Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/org/{org_id}/sso` | admin | Get SSO config |
| PUT | `/api/v1/org/{org_id}/sso` | admin | Set SSO config |
| GET | `/api/v1/saml/metadata` | public | SP metadata XML |
| POST | `/api/v1/saml/acs` | public | Assertion Consumer Service |
| GET | `/api/v1/oidc/authorize` | public | OIDC authorize redirect |
| POST | `/api/v1/oidc/callback` | public | OIDC callback |
| GET | `/api/v1/org/{org_id}/audit` | admin | Query audit log |
| GET | `/health/ready` | public | Readiness probe |

## Acceptance

Graded by repo command.

1. **`cargo test -p loom-control-plane`** — SSO enrollment flow:
   - Configure OIDC for org "acme", simulate IdP callback, verify human created in roster
   - Configure SAML for org "acme", parse signed assertion, verify human created
   - SSO-disabled org still uses local challenge-response

2. **`cargo test -p loom-fips`** (new crate) — FIPS mode:
   - `--fips` enables `ring` FIPS; non-FIPS algorithms rejected
   - `loom doctor --fips` passes on FIPS build

3. **`cargo test -p loom-control-plane`** — On-prem:
   - `--air-gap` disables Stripe/registry/outbound calls
   - Config encrypt/decrypt round-trip works
   - Audit log written for admin actions

4. **`cargo clippy --workspace --all-targets --all-features -- -D warnings`** — warning-free.

5. **`cargo test --workspace --all-features`** — no regressions.

## Out of Scope

- SCIM provisioning (deferred)
- Multi-region CP deployment (deferred)
- Custom IdP templates per customer (deferred)
- Advanced SAML features (artifact binding, encryption)

## Resolved Decisions

1. **OIDC + SAML both in MVP** — covers Entra/Okta/Google/ADFS
2. **SP-initiated SAML only** — simpler than IdP-initiated
3. **JIT provisioning with seat cap** — first SSO login creates human; 6th human rejected on free org
4. **FIPS via `ring` crate** — only FIPS-approved algorithms when flag enabled
5. **SQLite stays** — Postgres deferred until hosted-CP scale demands it
6. **Audit log to stdout + syslog** — no external SIEM dependency

## Needs Verification (Tests)

- OIDC flow with mock IdP (device code + auth code)
- SAML assertion parsing + signature validation
- FIPS mode rejects `ed25519-dalek` (non-FIPS), uses `ring::signature::Ed25519`
- Air-gap mode: Stripe client returns error, no outbound TCP
- Config encryption round-trip with AES-256-GCM
- Audit log includes all admin actions

## ADR

Recorded in `docs/decisions/0008-enterprise-sso-onprem.md`: OIDC/SAML federation, FIPS/air-gap modes, audit logging, SQLite retention, JIT provisioning with seat cap.