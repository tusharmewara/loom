<!--
Release notes template.

How to use:
  1. Replace the comment block under "## Notes" with the release notes, in the
     style used elsewhere in CHANGELOG.md (e.g. "- feat(cli): ..." bullets, or
     "### Added" / "### Fixed" / "### Changed" subsections).
  2. Run: scripts/release.sh <version>
     The content under "## Notes" is copied into CHANGELOG.md as the body of
     the new "## [<version>] - <date>" section. The "## Notes" heading itself
     is dropped.
  3. If "## Notes" is left empty, the script inserts a visible TODO placeholder
     instead of failing the release.

Only the content under "## Notes" is used; this comment block is ignored.
-->

## Notes

### Added

- feat(analytics): `loom-analytics` crate — push-only aggregated analytics (org, tool, day buckets) with privacy invariant (no per-run rows on control plane). SQLite storage at `{loom_dir}/control-plane/analytics.db`
- feat(control-plane): `POST /api/v1/org/{org_id}/analytics/push` and `GET /api/v1/org/{org_id}/analytics` endpoints for org analytics ingestion and rollup
- feat(cli): `loom org analytics` / `scorecard` / `cost` / `calibration` — org-level rollup, per-tool breakdown, cost-only view, calibration ratios (measured vs estimated)
- feat(enterprise): `loom-sso` crate — OIDC (openidconnect) + SAML (xmlsec) federation for Entra ID, Okta, Google Workspace, ADFS. SP-initiated SAML ACS, OIDC auth code + PKCE, device code flow
- feat(enterprise): `loom-fips` crate — FIPS mode via `ring` (Ed25519, AES-256-GCM), air-gap flag (`LOOM_AIR_GAP=1` disables all outbound), config encryption for secrets at rest
- feat(enterprise): `loom-audit` crate — structured audit logging to SQLite (`{loom_dir}/control-plane/audit.db`) with actor, action, resource, outcome, timestamp; query endpoint with filters
- feat(control-plane): SSO config CRUD (`GET/PUT /api/v1/org/{org_id}/sso`), SAML metadata/ACS (`/api/v1/saml/metadata`, `/api/v1/saml/acs`), OIDC authorize/callback (`/api/v1/oidc/authorize`, `/api/v1/oidc/callback`), JIT human provisioning with seat cap enforcement
- feat(control-plane): `/health/ready` readiness probe (DB + Stripe connectivity), FIPS/air-gap status flags
- feat(cli): `loom org connect --sso` — initiates OIDC device code or browser flow against control plane
- feat(control-plane): `loom-cp config encrypt/decrypt` — AES-256-GCM config encryption for on-prem deployments
- docs: SLA scaffolding under `docs/enterprise/` (SLA.md, DATA_RESIDENCY.md, SUPPORT_TIERS.md)
- docs: ADR 0008 (enterprise SSO/on-prem decisions) under `docs/decisions/`

### Changed

- change(control-plane): `AppState` extended with SSO configs, OIDC/SAML client caches, audit logger, FIPS/air-gap flags
- change(analytics): push-only model — hosts push pre-aggregated buckets; control plane never stores per-run data (privacy invariant structural)
- change(billing): trial anti-abuse enforced — per-org one-trial rule tracked in-memory in control plane
- change(security): config encryption for Stripe secrets and SAML certs at rest (AES-256-GCM)

### Fixed

- fix(cli): test module ordering in `org.rs` (tests after implementation)
- fix(control-plane): `build_app` return type fix (removed unneeded `return` statement for clippy)

### Security

- sec(enterprise): FIPS mode uses only FIPS-approved algorithms (ring); non-FIPS `ed25519-dalek` disabled when `LOOM_FIPS=1`
- sec(enterprise): air-gap mode blocks all outbound calls (Stripe, registry fetch, update checks) at the control plane
- sec(audit): structured audit log for all admin mutating actions (SSO config, roster, license, registry)