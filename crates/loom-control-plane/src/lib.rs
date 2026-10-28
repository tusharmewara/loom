//! Control-plane MVP: org CRUD, roster seat cap, machine enrollment with
//! OAuth 2.1 / DPoP sender-constrained tokens, membership + license download.
//!
//! One binary (`loom-cp`), plain HTTP on `--addr` (default `127.0.0.1:7333`),
//! SQLite storage at `--db`.

mod auth;
mod db;
mod handlers;

use anyhow::{Context, Result};
use axum::{
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// An enrollment challenge bound to one machine key + one org.
#[derive(Clone, Debug)]
pub struct EnrollmentPending {
    pub nonce: String,
    pub org_id: String,
}

/// Shared server state. The SQLite connection is guarded by a mutex — fine for
/// the single-process MVP; every handler holds it only across its SQL call.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
    /// Enrollment challenges bound to a machine pub key (machine_pub -> {nonce, org_id}).
    pub pending_enrollments: Arc<Mutex<HashMap<String, EnrollmentPending>>>,
    /// DPoP `jti` anti-replay (in-process, single-process MVP).
    pub jtis: Arc<Mutex<HashSet<String>>>,
    /// Org registries: org_id -> {skills, mcps, templates}.
    /// Client-side pull-through (phase 5) reads this via GET /api/v1/org/{id}/registry.
    pub org_registry: Arc<Mutex<HashMap<String, OrgRegistryData>>>,
    /// Billing handler: Stripe webhook verification + license issuance.
    /// None if no Stripe webhook secret configured (billing disabled).
    pub billing: Option<Arc<loom_billing::WebhookHandler>>,
    /// Stripe API key for creating checkout sessions.
    pub stripe_api_key: Option<String>,
}

/// Org registry data served at GET /api/v1/org/{id}/registry.
#[derive(Debug, Clone, Default)]
pub struct OrgRegistryData {
    pub skills: Vec<loom_core::SkillEntry>,
    pub mcps: Vec<loom_core::McpServerEntry>,
    pub templates: Vec<loom_core::WorkflowTemplate>,
}

impl AppState {
    pub async fn new(
        loom_dir: &Path,
        db_path: &Path,
        stripe_api_key: Option<String>,
        stripe_webhook_secret: Option<String>,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(db_path.parent().unwrap_or(loom_dir))
            .with_context(|| format!("creating org dir {:?}", db_path.parent()))?;
        let conn = rusqlite::Connection::open(db_path)
            .with_context(|| format!("opening sqlite db {:?}", db_path))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        db::init_schema(&conn)?;

        let billing = match (&stripe_api_key, &stripe_webhook_secret) {
            (Some(api_key), Some(webhook_secret)) => {
                let issuer = loom_billing::LicenseIssuer::new(api_key)
                    .map_err(|e| anyhow::anyhow!("invalid billing key: {e}"))?;
                Some(Arc::new(loom_billing::WebhookHandler::new(
                    webhook_secret,
                    issuer,
                )))
            }
            _ => None,
        };

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            pending_enrollments: Arc::new(Mutex::new(HashMap::new())),
            jtis: Arc::new(Mutex::new(HashSet::new())),
            org_registry: Arc::new(Mutex::new(HashMap::new())),
            billing,
            stripe_api_key: stripe_api_key.clone(),
        })
    }
}

/// Build the axum application with all routes and auth middleware.
pub fn build_app(state: AppState) -> Router {
    // Public routes (no auth)
    let public = Router::new()
        .route("/health", get(handlers::health))
        .route(
            "/.well-known/oauth-protected-resource",
            get(handlers::oauth_protected_resource),
        )
        // Org CRUD + roster (bootstrap: org admin ops, public pre-enrollment)
        .route("/api/v1/orgs", post(handlers::create_org))
        .route(
            "/api/v1/org/{org_id}/roster",
            post(handlers::add_human),
        )
        .route(
            "/api/v1/org/{org_id}/roster/{email}",
            delete(handlers::remove_human),
        )
        // Machine enrollment is public (no token yet)
        .route("/api/v1/machine/enroll", post(handlers::enroll_challenge))
        .route(
            "/api/v1/machine/enroll/confirm",
            post(handlers::enroll_confirm),
        )
        // Checkout (public — customer initiates purchase)
        .route(
            "/api/v1/org/{org_id}/checkout",
            get(handlers::checkout),
        )
        // Stripe webhook (public — Stripe sends events here)
        .route(
            "/api/v1/webhooks/stripe",
            post(handlers::stripe_webhook),
        )
        .with_state(state.clone());

    // Org-scoped routes — all require machine auth (Bearer token + DPoP proof)
    let protected = Router::new()
        .route("/api/v1/org/{org_id}", get(handlers::get_org))
        // Membership + license download
        .route(
            "/api/v1/org/{org_id}/membership",
            get(handlers::get_membership),
        )
        .route("/api/v1/org/{org_id}/license", get(handlers::get_license))
        .route(
            "/api/v1/org/{org_id}/license",
            post(handlers::set_license),
        )
        // IdP connect (designed, not wired — returns 501 until phase 4)
        .route(
            "/api/v1/org/{org_id}/idp/connect",
            post(handlers::idp_connect_stub),
        )
        // Dashboard (server-rendered HTML)
        .route("/api/v1/org/{org_id}/dashboard", get(handlers::dashboard))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_machine_auth,
        ))
        .with_state(state.clone());

    // Org registry read — public (client pull-through, no auth required).
    let public = public
        .route(
            "/api/v1/org/{org_id}/registry",
            get(handlers::get_org_registry),
        )
        .route(
            "/api/v1/org/{org_id}/registry",
            post(handlers::set_org_registry),
        )
        .with_state(state.clone());

    public.merge(protected).with_state(state)
}

#[derive(Debug, thiserror::Error)]
pub enum CpError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("internal: {0}")]
    Internal(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

impl IntoResponse for CpError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match &self {
            CpError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            CpError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            CpError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            CpError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            CpError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            CpError::NotImplemented(msg) => (StatusCode::NOT_IMPLEMENTED, msg),
        };
        let json = serde_json::json!({"error": self.to_string(), "message": body});
        (status, axum::response::Json(json)).into_response()
    }
}

impl From<anyhow::Error> for CpError {
    fn from(e: anyhow::Error) -> Self {
        CpError::Internal(e.to_string())
    }
}

impl From<loom_billing::issuance::IssuanceError> for CpError {
    fn from(e: loom_billing::issuance::IssuanceError) -> Self {
        CpError::Internal(e.to_string())
    }
}

impl From<rusqlite::Error> for CpError {
    fn from(e: rusqlite::Error) -> Self {
        CpError::Internal(e.to_string())
    }
}

pub type CpResult<T> = Result<T, CpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp_error_statuses() {
        assert_eq!(
            CpError::NotFound("x".into()).into_response().status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            CpError::Unauthorized("x".into()).into_response().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            CpError::NotImplemented("x".into()).into_response().status(),
            StatusCode::NOT_IMPLEMENTED
        );
    }
}
