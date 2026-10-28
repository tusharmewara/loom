# Plan — Phase 8: Enterprise

Card: `phase8-enterprise` · spec: `blueprint/commercialization/phase8-enterprise.spec.md` (hardened, all decisions accepted).

## Implementation Scope

New crates: `loom-sso` (OIDC/SAML), `loom-fips` (FIPS/air-gap), `loom-audit` (audit logging).
Modified: `loom-control-plane` (SSO endpoints, org SSO config, audit log), `loom-cli` (`loom org connect --sso`, `loom-cp` subcommands), `loom-core` (config encryption helpers).

## File:Line Homes

### 1. `crates/loom-sso/Cargo.toml` (NEW)

Workspace member. Dependencies: `loom-core`, `openidconnect`, `xmlsec`, `x509-parser`, `chrono`, `serde`, `anyhow`, `thiserror`.

### 2. `crates/loom-sso/src/lib.rs` (NEW)

```rust
pub mod oidc;
pub mod saml;
pub mod config;
pub use config::{SsoConfig, SsoMethod};
pub use oidc::{OidcClient, OidcConfig};
pub use saml::{SamlSp, SamlConfig};
```

### 3. `crates/loom-sso/src/config.rs` (NEW)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum SsoConfig {
    Oidc { issuer_url: String, client_id: String, client_secret: String, attribute_map: AttributeMap },
    Saml { entity_id: String, sso_url: String, slo_url: Option<String>, x509_cert: String, attribute_map: AttributeMap },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeMap {
    pub email_claim: String,
    pub name_claim: String,
    pub groups_claim: Option<String>,
}
```

### 4. `crates/loom-sso/src/oidc.rs` (NEW)

`OidcClient::new(config)` — discovers issuer, builds `openidconnect` client.
`OidcClient::authorize_url()` — returns authorize URL with PKCE.
`OidcClient::exchange_code(code)` — exchanges code for tokens, returns `UserInfo { email, name, groups }`.

### 5. `crates/loom-sso/src/saml.rs` (NEW)

`SamlSp::new(config)` — loads SP cert/key, validates IdP cert.
`SamlSp::auth_request()` — returns SAML AuthnRequest XML (redirect binding).
`SamlSp::parse_assertion(xml)` — validates signature, decrypts if needed, returns `UserInfo`.

### 6. `crates/loom-fips/Cargo.toml` (NEW)

Workspace member. Dependencies: `ring` (with `fips` feature), `loom-core`.

### 7. `crates/loom-fips/src/lib.rs` (NEW)

```rust
pub fn fips_enabled() -> bool { std::env::var("LOOM_FIPS").is_ok() }
pub fn air_gap_enabled() -> bool { std::env::var("LOOM_AIR_GAP").is_ok() }
pub mod crypto { pub fn ed25519_keypair() -> Result<(Vec<u8>, Vec<u8>)>; pub fn ed25519_sign(sk, msg) -> Result<Vec<u8>>; pub fn ed25519_verify(pk, msg, sig) -> bool; }
pub mod config_crypto { pub fn encrypt(key, plaintext) -> Result<Vec<u8>>; pub fn decrypt(key, ciphertext) -> Result<Vec<u8>>; }
```

Uses `ring::signature::Ed25519` when FIPS enabled, falls back to `ed25519-dalek` otherwise. Config encryption uses `ring::aead::AES_256_GCM`.

### 8. `crates/loom-audit/Cargo.toml` (NEW)

Workspace member. Dependencies: `loom-core`, `chrono`, `serde`, `anyhow`.

### 9. `crates/loom-audit/src/lib.rs` (NEW)

```rust
pub struct AuditLogger { conn: Mutex<Connection> }
impl AuditLogger {
    pub fn new(loom_dir) -> Result<Self>;
    pub fn log(&self, entry: AuditEntry) -> Result<()>;
    pub fn query(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>>;
}
```

### 10. `crates/loom-control-plane/src/lib.rs` (MODIFIED)

- Add `sso: Option<Arc<loom_sso::SsoManager>>` and `audit: Arc<loom_audit::AuditLogger>` to `AppState`
- `AppState::new` initializes SSO manager (if org has SSO config) and audit logger
- `--fips` and `--air-gap` flags in `main.rs`, set env vars `LOOM_FIPS`, `LOOM_AIR_GAP`

### 11. `crates/loom-control-plane/src/handlers.rs` (MODIFIED)

New handlers:
- `get_org_sso`, `set_org_sso` — org admin CRUD for SSO config
- `saml_metadata`, `saml_acs` — SAML SP endpoints
- `oidc_authorize`, `oidc_callback` — OIDC flow
- `get_audit_log` — admin query with filters (actor, action, date range)
- `health_ready` — readiness probe (DB + Stripe if configured)

Middleware: audit log on all mutating endpoints (POST/PUT/DELETE).

### 12. `crates/loom-control-plane/src/main.rs` (MODIFIED)

```rust
#[derive(Parser)]
struct Args {
    ...
    #[arg(long)] fips: bool,
    #[arg(long)] air_gap: bool,
}
fn main() {
    if args.fips { std::env::set_var("LOOM_FIPS", "1"); }
    if args.air_gap { std::env::set_var("LOOM_AIR_GAP", "1"); }
    ...
}
```

### 13. `crates/loom-cli/src/commands/org.rs` (MODIFIED)

Add `SsoConnect` variant to `OrgAction`:
```rust
SsoConnect {
    url: String,
    org: String,
    #[arg(long)] device_code: bool,  // use device code flow instead of browser
}
```

Handler `sso_connect` — initiates OIDC device code or browser flow, polls for token, stores membership.

### 14. `crates/loom-cli/src/commands/cp.rs` (NEW or MODIFIED)

Add `loom-cp` subcommands:
```bash
loom-cp config encrypt --key <base64> --in <file> --out <file>
loom-cp config decrypt --key <base64> --in <file> --out <file>
```

### 15. `crates/loom-core/src/crypto.rs` (MODIFIED)

Add `encrypt_config(key, plaintext)` / `decrypt_config(key, ciphertext)` using AES-256-GCM (ring or aes-gcm).

### Tests to Add

| Test | File |
|------|------|
| OIDC auth code flow with mock issuer | `loom-sso/src/oidc.rs` |
| SAML assertion parse + signature validation | `loom-sso/src/saml.rs` |
| FIPS mode: `ed25519_keypair` uses `ring` | `loom-fips/src/lib.rs` |
| Air-gap: Stripe client returns error | `loom-control-plane/src/handlers.rs` |
| Config encrypt/decrypt round-trip | `loom-fips/src/lib.rs` |
| Audit log written on admin action | `loom-control-plane/src/handlers.rs` |
| JIT provisioning: first SSO login creates human | `loom-control-plane/src/handlers.rs` |
| Seat cap enforced on JIT provisioning | `loom-control-plane/src/handlers.rs` |

### Acceptance → Test Mapping

| AC | Test | File |
|----|------|------|
| SSO enrollment flow | OIDC/SAML integration tests | `loom-control-plane/src/handlers.rs` |
| FIPS mode | `fips_enabled()` + crypto tests | `loom-fips/src/lib.rs` |
| On-prem hardening | Air-gap, config encrypt, audit log | `loom-control-plane/src/handlers.rs` |
| SLA docs | Manual review | `docs/enterprise/` |

### Verification Commands

```bash
cargo test -p loom-sso --all-features
cargo test -p loom-fips --all-features
cargo test -p loom-audit --all-features
cargo test -p loom-control-plane --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

### ADR

`docs/decisions/0008-enterprise-sso-onprem.md`