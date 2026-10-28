//! HTTP route handlers for the control-plane MVP.

use super::auth;
use super::db;
use super::AppState;
use super::CpError;
use super::CpResult;
use super::EnrollmentPending;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use loom_core::constants::LICENSE_VERIFY_PUBLIC_KEY;
use loom_core::license::verify_license_key;
use serde::Deserialize;
use serde_json::Value;

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

    if !auth::verify_enrollment_signature(&payload.machine_pub, &payload.signature, &pending.nonce)? {
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
// Dashboard (server-rendered HTML, no framework)
// ---------------------------------------------------------------------------

pub async fn dashboard(
    State(state): State<AppState>,
    Path(org_id): Path<String>,
) -> CpResult<String> {
    let db = state.db.lock().unwrap();
    let Some((name, tier, _)) = db::get_org(&db, &org_id)? else {
        return Err(CpError::NotFound(format!("org {org_id} not found")));
    };
    let humans = db::list_humans(&db, &org_id)?;
    let machines = db::list_machines(&db, &org_id)?;
    let mut html = String::from(
        "<!doctype html><html><head><meta charset='utf-8'><title>Loom — Org </title>",
    );
    html.push_str(&name);
    html.push_str("</title><style>body{font-family:system-ui,sans-serif;max-width:900px;margin:2rem;padding:1rem;line-height:1.5}table{border-collapse:collapse;width:100%}th,td{border:1px solid #ccc;padding:.5rem;text-align:left}th{background:#f5f5f5}.badge{display:inline-block;padding:.1rem .4rem;border-radius:.3rem;font-size:.85rem}.ok{background:#e7f7e7;color:#1b5e20}.no{background:#fdecea;color:#b3261e}</style></head><body>");
    html.push_str(&format!("<h1>Loom Control Plane</h1><h2>{}</h2>", escape_html(&name)));
    html.push_str(&format!("<p><strong>Tier:</strong> {} &nbsp; <strong>Org:</strong> {}</p>", escape_html(&tier), org_id));
    html.push_str("<h3>Roster</h3><table><thead><tr><th>Email</th><th>Role</th></tr></thead><tbody>");
    for (email, role) in &humans {
        html.push_str(&format!("<tr><td>{}</td><td>{}</td></tr>", escape_html(email), escape_html(role)));
    }
    html.push_str("</tbody></table>");
    html.push_str("<h3>Enrolled Machines</h3><table><thead><tr><th>Machine public key</th><th>Status</th></tr></thead><tbody>");
    for (machine_pub, revoked) in &machines {
        let status = if *revoked { "revoked" } else { "active" };
        let badge = if *revoked { "no" } else { "ok" };
        html.push_str(&format!(
            "<tr><td><code>{}</code></td><td><span class='badge {}'>{}</span></td></tr>",
            escape_html(machine_pub),
            badge,
            status
        ));
    }
    html.push_str("</tbody></table>");
    html.push_str("<p><em>Machines enroll via POST /api/v1/machine/enroll with an Ed25519 key and DPoP-signed challenge.</em></p>");
    html.push_str("</body></html>");
    Ok(html)
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
