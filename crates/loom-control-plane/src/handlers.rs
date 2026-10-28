//! HTTP route handlers for the control-plane MVP.

use super::auth;
use super::db;
use super::AppState;
use super::CpError;
use super::CpResult;
use super::EnrollmentPending;
use super::OrgRegistryData;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, Json};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use loom_core::constants::LICENSE_VERIFY_PUBLIC_KEY;
use loom_core::license::verify_license_key;
use loom_billing::StripeClient;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use std::fmt::Write;

// ---------------------------------------------------------------------------
// Request / response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateOrg {
    pub name: String,
}

#[derive(Deserialize)]
pub struct AddHuman {
    pub email: String,
    #[serde(default = "default_role")]
    pub role: String,
}

#[derive(Deserialize)]
pub struct SetLicense {
    pub license_key: String,
}

#[derive(Deserialize)]
pub struct EnrollChallenge {
    pub machine_pub: Value, // JWK OKP Ed25519
    pub org_id: String,
}

#[derive(Deserialize)]
pub struct EnrollConfirm {
    pub machine_pub: Value, // JWK OKP Ed25519
    pub signature: String,   // hex Ed25519 signature over machine_pub_bytes || nonce_bytes
}

#[derive(Deserialize)]
pub struct CheckoutRequest {
    pub seats: Option<u32>,
}

#[derive(Serialize)]
pub struct CheckoutResponse {
    pub session_id: String,
    pub url: String,
}

fn default_role() -> String {
    "member".to_string()
}

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

pub async fn health() -> &'static str {
    "ok"
}

pub async fn oauth_protected_resource() -> Json<Value> {
    Json(serde_json::json!({
        "resource": "/api/v1",
        "authorization_servers": [],
        "documentation": "https://docs.loom.local/oauth-protected-resource"
    }))
}

// ---------------------------------------------------------------------------
// Org CRUD + roster
// ---------------------------------------------------------------------------

pub async fn create_org(
    State(state): State<AppState>,
    Json(payload): Json<CreateOrg>,
) -> CpResult<(StatusCode, Json<Value>)> {
    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return Err(CpError::BadRequest("org name must not be empty".into()));
    }
    let id = db::new_id();
    {
        let db = state.db.lock().unwrap();
        db::insert_org(&db, &id, &name)?;
    }
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"id": id, "name": name, "tier": "free"})),
    ))
}

pub async fn get_org(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<Json<Value>> {
    let db = state.db.lock().unwrap();
    let Some((name, tier, _license_key)) = db::get_org(&db, &org_id)? else {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    };
    let human_count = db::human_count(&db, &org_id)?;
    Ok(Json(serde_json::json!({
        "id": org_id,
        "name": name,
        "tier": tier,
        "human_count": human_count,
        "license_set": !_license_key.is_empty(),
    })))
}

pub async fn add_human(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
    Json(payload): Json<AddHuman>,
) -> CpResult<(StatusCode, Json<Value>)> {
    let email = payload.email.trim().to_string();
    let role = payload.role.trim().to_string();
    if email.is_empty() {
        return Err(CpError::BadRequest("email must not be blank".into()));
    }
    if role.is_empty() {
        return Err(CpError::BadRequest("role must not be blank".into()));
    }

    let db = state.db.lock().unwrap();
    if db::get_org(&db, &org_id)?.is_none() {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    }

    let count = db::human_count(&db, &org_id)?;
    let license_key = db::get_license_key(&db, &org_id)?;
    let cap: u32 = match &license_key {
        Some(key) => match verify_license_key(key, LICENSE_VERIFY_PUBLIC_KEY) {
            Ok(payload) => {
                let unexpired = payload
                    .expires_at
                    .parse::<chrono::DateTime<Utc>>()
                    .map(|e| e > Utc::now())
                    .unwrap_or(false);
                if unexpired {
                    std::cmp::max(5, payload.seats)
                } else {
                    5
                }
            }
            Err(_) => 5,
        },
        None => 5,
    };
    if count >= cap {
        return Err(CpError::Forbidden(format!(
            "seat cap reached for org {org_id}: {cap} humans"
        )));
    }

    db::insert_human(&db, &org_id, &email, &role)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"email": email, "role": role})),
    ))
}

pub async fn remove_human(
    State(state): State<AppState>,
    Path((org_id, email)): Path<(String, String)>,
) -> CpResult<Json<Value>> {
    let db = state.db.lock().unwrap();
    if db::get_org(&db, &org_id)?.is_none() {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    }
    if !db::delete_human(&db, &org_id, &email)? {
        return Err(CpError::NotFound(format!("human {email} not in org {org_id}")));
    }
    Ok(Json(serde_json::json!({"deactivated": true})))
}

// ---------------------------------------------------------------------------
// Membership + license download
// ---------------------------------------------------------------------------

pub async fn get_membership(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<Json<Value>> {
    let db = state.db.lock().unwrap();
    let Some((name, tier, _)) = db::get_org(&db, &org_id)? else {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    };
    let humans = db::list_humans(&db, &org_id)?;
    let machines = db::list_machines_for_membership(&db, &org_id)?;
    Ok(Json(serde_json::json!({
        "org": {"id": org_id, "name": name, "tier": tier},
        "humans": humans.into_iter().map(|(e, r)| serde_json::json!({"email": e, "role": r})).collect::<Vec<_>>(),
        "machines": machines,
    })))
}

pub async fn get_license(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<Json<Value>> {
    let db = state.db.lock().unwrap();
    if db::get_org(&db, &org_id)?.is_none() {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    }
    let Some(license_key) = db::get_license_key(&db, &org_id)? else {
        return Err(CpError::NotFound("no license set on this org".into()));
    };
    let payload = verify_license_key(&license_key, LICENSE_VERIFY_PUBLIC_KEY)
        .map_err(|_| CpError::BadRequest("stored license key failed verification against embedded key".into()))?;
    Ok(Json(serde_json::json!({
        "license_key": license_key,
        "tier": payload.tier,
        "seats": payload.seats,
        "expires_at": payload.expires_at,
        "features": payload.features,
    })))
}

pub async fn set_license(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
    Json(payload): Json<SetLicense>,
) -> CpResult<Json<Value>> {
    let db = state.db.lock().unwrap();
    if db::get_org(&db, &org_id)?.is_none() {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    }
    verify_license_key(&payload.license_key, LICENSE_VERIFY_PUBLIC_KEY)
        .map_err(|_| CpError::BadRequest("license key failed verification against embedded key".into()))?;
    db::set_license_key(&db, &org_id, &payload.license_key)?;
    Ok(Json(serde_json::json!({"stored": true})))
}

// ---------------------------------------------------------------------------
// Machine enrollment
// ---------------------------------------------------------------------------

pub async fn enroll_challenge(
    State(state): State<AppState>,
    Json(payload): Json<EnrollChallenge>,
) -> CpResult<Json<Value>> {
    let vk = auth::verifying_key_from_jwk(&payload.machine_pub)
        .map_err(|e| CpError::BadRequest(format!("invalid machine_pub JWK: {e}")))?;
    let machine_pub = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    let org_id = payload.org_id.trim().to_string();
    if org_id.is_empty() {
        return Err(CpError::BadRequest("org_id must not be blank".into()));
    }
    {
        let db = state.db.lock().unwrap();
        if let Some(existing_org) = db::get_machine_key(&db, &machine_pub)? {
            if existing_org != org_id {
                return Err(CpError::Forbidden(format!(
                    "machine already enrolled in org {existing_org}"
                )));
            }
        }
    }

    let nonce = auth::new_nonce_hex();
    {
        let mut pending = state.pending_enrollments.lock().unwrap();
        pending.insert(
            machine_pub.clone(),
            EnrollmentPending {
                nonce: nonce.clone(),
                org_id: org_id.clone(),
            },
        );
    }
    Ok(Json(serde_json::json!({
        "challenge": nonce,
        "machine_pub": machine_pub,
    })))
}

pub async fn enroll_confirm(
    State(state): State<AppState>,
    Json(payload): Json<EnrollConfirm>,
) -> CpResult<Json<Value>> {
    let vk = auth::verifying_key_from_jwk(&payload.machine_pub)
        .map_err(|e| CpError::BadRequest(format!("invalid machine_pub JWK: {e}")))?;
    let machine_pub = URL_SAFE_NO_PAD.encode(vk.to_bytes());

    let pending = {
        let mut pending = state.pending_enrollments.lock().unwrap();
        pending.remove(&machine_pub).ok_or_else(|| {
            CpError::BadRequest("no enrollment challenge for this machine key; call /enroll first".into())
        })?
    };

    if !auth::verify_enrollment_signature(&vk, &payload.signature, &pending.nonce)? {
        return Err(CpError::Unauthorized("enrollment signature verification failed".into()));
    }
    let org_id = pending.org_id;

    // Persist machine key bound to org (idempotent: same machine_pub+org is a rotation no-op).
    {
        let db = state.db.lock().unwrap();
        match db::get_machine_key(&db, &machine_pub)? {
            Some(existing_org) if existing_org != org_id => {
                return Err(CpError::Forbidden(format!(
                    "machine already enrolled in org {existing_org}"
                )));
            }
            _ => {}
        }
        let already = db::get_machine_key(&db, &machine_pub)?.is_some();
        if !already {
            db::insert_machine_key(&db, &org_id, &machine_pub)?;
        }
    }

    let token = auth::issue_token(&state, &machine_pub, &org_id, &pending.nonce)?;
    Ok(Json(serde_json::json!({
        "token": token,
        "org_id": org_id,
        "machine_pub": machine_pub,
    })))
}

pub async fn idp_connect_stub() -> CpResult<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "message": "IdP federation not yet wired — see phase 4"
        })),
    ))
}

// ---------------------------------------------------------------------------
// Stripe billing (phase 6) — checkout + webhook.
// ---------------------------------------------------------------------------

pub async fn checkout(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
    Json(payload): Json<CheckoutRequest>,
) -> CpResult<Json<CheckoutResponse>> {
    let stripe_api_key = state
        .stripe_api_key
        .as_ref()
        .ok_or_else(|| CpError::BadRequest("billing not configured".into()))?;

    let stripe_client = StripeClient::new(stripe_api_key.clone());
    let seats = payload.seats.unwrap_or(1);
    let session = stripe_client.create_checkout_session(&org_id, (seats * 2000) as u64)?;

    Ok(Json(CheckoutResponse {
        session_id: session.session_id,
        url: session.url,
    }))
}

pub async fn stripe_webhook(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> CpResult<Json<Value>> {
    let billing = state
        .billing
        .as_ref()
        .ok_or_else(|| CpError::BadRequest("billing not configured".into()))?;

    let event_type = payload["type"]
        .as_str()
        .ok_or_else(|| CpError::BadRequest("missing event type".into()))?
        .to_string();

    // Only handle checkout.session.completed
    if event_type != "checkout.session.completed" {
        return Ok(Json(serde_json::json!({
            "received": true,
            "event_type": event_type,
            "action": "ignored"
        })));
    }

    // Parse the checkout session from the payload
    let session_id = payload["data"]["object"]["id"]
        .as_str()
        .ok_or_else(|| CpError::BadRequest("missing checkout session id".into()))?
        .to_string();
    let org_id = payload["data"]["object"]["metadata"]["org_id"]
        .as_str()
        .ok_or_else(|| CpError::BadRequest("missing org_id in session metadata".into()))?
        .to_string();

    // Issue a paid license (1 seat for now)
    let license_key = billing.issue_license_for_session(&org_id, 1, &session_id)?;

    Ok(Json(serde_json::json!({
        "received": true,
        "event_type": event_type,
        "action": "license_issued",
        "org_id": org_id,
        "session_id": session_id,
        "license_key_prefix": &license_key[..8.min(license_key.len())],
    })))
}

// ---------------------------------------------------------------------------
// Org registry (phase 5) — client pull-through + admin write.
// ---------------------------------------------------------------------------

pub async fn get_org_registry(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<Json<Value>> {
    let registry = state.org_registry.lock().unwrap();
    let data = registry.get(&org_id).cloned().unwrap_or_default();

    let entries: Vec<Value> = data
        .skills
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "kind": "skill",
                "id": s.name,
                "content": serde_json::to_value(&s).unwrap_or(Value::Null),
                "updated_at": s.last_modified,
            })
        })
        .chain(
            data.mcps
                .into_iter()
                .map(|m| {
                    serde_json::json!({
                        "kind": "mcp",
                        "id": m.id,
                        "content": serde_json::to_value(&m).unwrap_or(Value::Null),
                        "updated_at": m.last_health_check,
                    })
                })
        )
        .chain(
            data.templates
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "kind": "template",
                        "id": t.name,
                        "content": serde_json::to_value(&t).unwrap_or(Value::Null),
                        "updated_at": chrono::Utc::now().to_rfc3339(),
                    })
                })
        )
        .collect();

    Ok(Json(serde_json::json!({
        "org_id": org_id,
        "entries": entries,
        "synced_at": chrono::Utc::now().to_rfc3339(),
    })))
}

pub async fn set_org_registry(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
    Json(payload): Json<Value>,
) -> CpResult<Json<Value>> {
    let entries = payload
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut skills = Vec::new();
    let mut mcps = Vec::new();
    let mut templates = Vec::new();

    for entry in entries {
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let content = entry.get("content").cloned().unwrap_or(Value::Null);

        match kind {
            "skill" => {
                if let Ok(skill) = serde_json::from_value::<loom_core::SkillEntry>(content) {
                    skills.push(skill);
                }
            }
            "mcp" => {
                if let Ok(mcp) = serde_json::from_value::<loom_core::McpServerEntry>(content) {
                    mcps.push(mcp);
                }
            }
            "template" => {
                if let Ok(template) =
                    serde_json::from_value::<loom_core::WorkflowTemplate>(content)
                {
                    templates.push(template);
                }
            }
            _ => {}
        }
    }

    let mut registry = state.org_registry.lock().unwrap();
    registry.insert(
        org_id,
        OrgRegistryData {
            skills,
            mcps,
            templates,
        },
    );

    Ok(Json(serde_json::json!({"stored": true})))
}

// ---------------------------------------------------------------------------
// Dashboard (server-rendered HTML, no framework)
// ---------------------------------------------------------------------------
//
// Minimal "clean developer tool" aesthetic: system-ui type, restrained gray
// slate/indigo-ish color hierarchy, generous spacing, tables for the roster
// and machine tables, status badges that carry text labels (never color
// alone). Inline CSS strings only — no framework, no template engine.

const DASHBOARD_CSS: &str = r#"* { box-sizing: border-box; }
body {
  color: #111827;
  background: #ffffff;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans", sans-serif, "Apple Color Emoji", "Segoe UI Emoji";
  font-size: 14px;
  line-height: 1.5;
  margin: 0;
  padding: 48px 20px;
}
.shell { max-width: 700px; margin: 0 auto; }
h1 {
  font-size: 18px;
  font-weight: 600;
  letter-spacing: -0.01em;
  margin: 0 0 8px 0;
}
h2 {
  font-size: 26px;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin: 0 0 4px 0;
}
h3 {
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.14em;
  color: #6b7280;
  margin: 40px 0 12px 0;
}
p { color: #4b5563; margin: 0 0 16px 0; }
.meta {
  display: flex;
  align-items: baseline;
  gap: 16px;
  font-size: 13px;
  color: #6b7280;
  margin: 16px 0 32px 0;
}
table { width: 100%; border-collapse: collapse; margin-top: 8px; }
th {
  text-align: left;
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.14em;
  color: #9ca3af;
  padding: 10px 12px;
  border-bottom: 2px solid #e5e7eb;
}
td {
  padding: 10px 12px;
  border-bottom: 1px solid #e5e7eb;
  font-size: 14px;
  color: #111827;
}
tr:last-child > td { border-bottom: none; }
code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 12px;
  color: #111827;
  background: #f3f4f6;
  border-radius: 4px;
  padding: 1px 6px;
}
.badge {
  display: inline-flex;
  align-items: center;
  padding: 1px 8px;
  border-radius: 12px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  border: 1px solid;
}
.badge--active { background: #ecfdf5; color: #065f46; border-color: #86efac; }
.badge--revoked { background: #fef2f2; color: #991b1b; border-color: #fecaca; }
.dim { color: #6b7280; }
.note { font-size: 12px; color: #6b7280; }
"#;

pub async fn dashboard(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<Html<String>> {
    let db = state.db.lock().unwrap();
    let Some((name, tier, _)) = db::get_org(&db, &org_id)? else {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    };
    let humans = db::list_humans(&db, &org_id)?;
    let machines = db::list_machines(&db, &org_id)?;

    let mut roster = String::new();
    for (email, role) in &humans {
        write!(
            roster,
            "<tr><td>{}</td><td>{}</td></tr>",
            escape_html(email),
            escape_html(role)
        )
        .unwrap();
    }

    let mut machine_rows = String::new();
    for (machine_pub, revoked) in &machines {
        let (status, badge_class) = if *revoked {
            ("revoked", "badge--revoked")
        } else {
            ("active", "badge--active")
        };
        write!(
            machine_rows,
            "<tr><td><code>{}</code></td><td><span class=\"badge {}\">{}</span></td></tr>",
            escape_html(machine_pub),
            badge_class,
            status
        )
        .unwrap();
    }

    let mut html = String::new();
    write!(
        html,
        "<!doctype html><html lang='en'><head><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1'><title>Loom — {}</title><style>",
        escape_html(&name)
    )
    .unwrap();
    html.push_str(DASHBOARD_CSS);
    html.push_str("</style></head><body><div class='shell'>");
    write!(
        html,
        "<h1>Loom Control Plane</h1><h2>{}</h2>",
        escape_html(&name)
    )
    .unwrap();
    write!(
        html,
        "<p class='meta'><span class='dim'>Tier</span> {} &middot; <span class='dim'>Org</span> {}</p>",
        escape_html(&tier),
        escape_html(&org_id)
    )
    .unwrap();
    html.push_str("<h3>Roster</h3><table><thead><tr><th>Email</th><th>Role</th></tr></thead><tbody>");
    html.push_str(&roster);
    html.push_str("</tbody></table>");
    html.push_str("<h3>Enrolled Machines</h3><table><thead><tr><th>Machine public key</th><th>Status</th></tr></thead><tbody>");
    html.push_str(&machine_rows);
    html.push_str("</tbody></table>");
    html.push_str("<p class='note'>Machines enroll via POST /api/v1/machine/enroll with an Ed25519 key and DPoP-signed challenge.</p>");
    html.push_str("</div></body></html>");
    Ok(Html(html))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::{db, AppState, dashboard};
    use axum::extract::{Path, State};
    use axum::http::{header::CONTENT_TYPE, StatusCode};
    use axum::response::{Html, IntoResponse};
    use rusqlite::params;
    use tempfile::{tempdir, TempDir};

    async fn seed_state() -> (AppState, TempDir) {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("dashboard.db");
        let state = AppState::new(
            dir.path(),
            &db_path,
            None,
            None,
        )
        .await
        .expect("open control-plane state");
        (state, dir)
    }

    #[tokio::test]
    async fn dashboard_returns_html_with_org_roster_and_machine_status() {
        let (state, _dir) = seed_state().await;
        let org_id = "org-demo";

        {
            let db = state.db.lock().unwrap();
            db::insert_org(&db, org_id, "Demo Org").unwrap();
            db::insert_human(&db, org_id, "alice@example.com", "admin").unwrap();
            db::insert_human(&db, org_id, "bob@example.com", "member").unwrap();
            db::insert_machine_key(&db, org_id, "machine-pub-active").unwrap();
            db::insert_machine_key(&db, org_id, "machine-pub-revoked").unwrap();
            db.execute(
                "UPDATE machine_keys SET revoked = 1 WHERE machine_pub = ?1",
                params!["machine-pub-revoked"],
            )
            .unwrap();
        }

        let Html(html) = dashboard(State(state.clone()), Path(org_id.to_owned()))
            .await
            .expect("dashboard returns Ok(html)");

        // 200 text/html contract.
        let response = Html(html.clone()).into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[CONTENT_TYPE],
            "text/html; charset=utf-8"
        );

        // Org identity, roster emails + roles, machine public keys.
        assert!(html.contains("<title>Loom — Demo Org</title>"));
        assert!(html.contains("Demo Org"));
        assert!(html.contains("alice@example.com"));
        assert!(html.contains("bob@example.com"));
        assert!(html.contains("admin"));
        assert!(html.contains("member"));
        assert!(html.contains("machine-pub-active"));
        assert!(html.contains("machine-pub-revoked"));

        // Status strings per machine, text labels present (not color alone).
        assert!(html.contains(">active<"));
        assert!(html.contains(">revoked<"));

        // Semantic tables for roster and machines.
        assert_eq!(html.matches("<table").count(), 2);

        // Status cells carry text labels; no inline color-only styling.
        assert!(!html.contains("style="));
        assert!(html.contains("badge--active"));
        assert!(html.contains("badge--revoked"));
    }
}
