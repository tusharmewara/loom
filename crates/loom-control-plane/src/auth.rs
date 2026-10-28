//! Machine authentication: enrollment challenge-response (Ed25519 proof of
//! possession) + OAuth 2.1 / DPoP (RFC 9449) sender-constrained machine tokens.
//!
//! The DPoP proof JWT is signed with the machine's Ed25519 key and binds the
//! token to (method, URI) with a single-use `jti`. A leaked bearer token is
//! useless without the machine's proof-of-possession key.

use super::db;
use super::AppState;
use axum::extract::State;
use axum::http::{header, Request};
use axum::middleware::Next;
use axum::response::Response;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand_core::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};

impl From<String> for super::CpError {
    fn from(s: String) -> Self {
        super::CpError::BadRequest(s)
    }
}

/// Verify an enrollment signature: signature over `machine_pub_bytes || nonce_bytes`.
pub fn verify_enrollment_signature(
    vk: &VerifyingKey,
    signature_hex: &str,
    nonce_hex: &str,
) -> Result<bool, String> {
    let pub_bytes = vk.to_bytes();
    let nonce_bytes = hex::decode(nonce_hex).map_err(|e| format!("invalid nonce hex: {e}"))?;
    let sig_bytes = hex::decode(signature_hex).map_err(|e| format!("invalid sig hex: {e}"))?;
    let signature: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    let mut signed = pub_bytes.to_vec();
    signed.extend_from_slice(&nonce_bytes);
    Ok(vk.verify(&signed, &Signature::from_bytes(&signature)).is_ok())
}

/// Parse an Ed25519 OKP JWK into a VerifyingKey.
pub fn verifying_key_from_jwk(jwk: &Value) -> Result<VerifyingKey, String> {
    let crv = jwk.get("crv").and_then(|v| v.as_str()).ok_or("missing crv")?;
    if crv != "Ed25519" {
        return Err("unsupported crv".into());
    }
    let kty = jwk.get("kty").and_then(|v| v.as_str()).ok_or("missing kty")?;
    if kty != "OKP" {
        return Err("unsupported kty".into());
    }
    let x_b64 = jwk.get("x").and_then(|v| v.as_str()).ok_or("missing x")?;
    let x_bytes = URL_SAFE_NO_PAD.decode(x_b64).map_err(|e| format!("invalid x base64: {e}"))?;
    let bytes: [u8; 32] = x_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "x must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&bytes).map_err(|e| format!("invalid verifying key: {e}"))
}

/// JWK thumbprint = base64url(SHA-256(canonical JWK)), RFC 7638.
pub fn jwk_thumbprint(vk: &VerifyingKey) -> String {
    let hash = Sha256::digest(jwk_canonical(vk).as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

/// Canonical JWK for JWK thumbprint (RFC 7638).
fn jwk_canonical(vk: &VerifyingKey) -> String {
    let x = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    format!(r#"{{"crv":"Ed25519","kty":"OKP","x":"{x}"}}"#)
}

/// Generate a random hex nonce.
pub fn new_nonce_hex() -> String {
    let mut bytes = [0u8; 16];
    rand_core::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Generate a random machine token (32 bytes hex).
pub fn new_token_hex() -> String {
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Issue a bearer token bound to a machine key + org. Persisted.
pub fn issue_token(
    state: &AppState,
    machine_pub: &str,
    org_id: &str,
    _consumed_nonce: &str,
) -> Result<String, super::CpError> {
    let token = new_token_hex();
    {
        let db = state.db.lock().unwrap();
        db::insert_token(&db, &token, machine_pub, org_id)?;
    }
    Ok(token)
}

/// Look up a bearer token -> (machine_pub, org_id), or Unauthorized.
pub fn resolve_token(state: &AppState, token: &str) -> super::CpResult<(String, String)> {
    let db = state.db.lock().unwrap();
    match db::get_token(&db, token)? {
        Some(pair) => Ok(pair),
        None => Err(super::CpError::Unauthorized("unknown or revoked token".into())),
    }
}

/// Parse a DPoP proof JWT: header.payload.signature (base64url).
/// The machine's verifying key is supplied by the caller (from the token),
/// so the proof need not carry a `jwk` field — only its `jkt` thumbprint,
/// which must match the machine key.
pub fn verify_dpop_proof(
    state: &AppState,
    machine_pub_key: &VerifyingKey,
    proof: &str,
    method: &str,
    uri: &str,
) -> super::CpResult<()> {
    let expected_jkt = jwk_thumbprint(machine_pub_key);

    let parts: Vec<&str> = proof.split('.').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return Err(super::CpError::Unauthorized("malformed DPoP proof".into()));
    }
    let header_b64 = parts[0];
    let payload_b64 = parts[1];
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig_b64 = if parts.len() == 3 { parts[2] } else { "" };

    let header: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(header_b64).map_err(|e| {
            super::CpError::Unauthorized(format!("invalid DPoP header: {e}"))
        })?,
    )
    .map_err(|e| super::CpError::Unauthorized(format!("invalid DPoP header json: {e}")))?;
    let payload: Value = serde_json::from_slice(
        &URL_SAFE_NO_PAD.decode(payload_b64).map_err(|e| {
            super::CpError::Unauthorized(format!("invalid DPoP payload: {e}"))
        })?,
    )
    .map_err(|e| super::CpError::Unauthorized(format!("invalid DPoP payload json: {e}")))?;

    // alg must be EdDSA, typ dpop+jwt
    if header.get("alg").and_then(|v| v.as_str()) != Some("EdDSA") {
        return Err(super::CpError::Unauthorized("DPoP alg must be EdDSA".into()));
    }
    if header.get("typ").and_then(|v| v.as_str()) != Some("dpop+jwt") {
        return Err(super::CpError::Unauthorized("DPoP typ must be dpop+jwt".into()));
    }

    // jti single-use anti-replay
    let jti = payload
        .get("jti")
        .and_then(|v| v.as_str())
        .ok_or_else(|| super::CpError::Unauthorized("DPoP jti required".into()))?;
    {
        let mut jtis = state.jtis.lock().unwrap();
        if !jtis.insert(jti.to_string()) {
            return Err(super::CpError::Unauthorized("DPoP jti replayed".into()));
        }
    }

    // htu / htm binding
    let htu = payload
        .get("htu")
        .and_then(|v| v.as_str())
        .ok_or_else(|| super::CpError::Unauthorized("DPoP htu required".into()))?;
    let htm = payload
        .get("htm")
        .and_then(|v| v.as_str())
        .ok_or_else(|| super::CpError::Unauthorized("DPoP htm required".into()))?;
    if htu != uri {
        return Err(super::CpError::Unauthorized("DPoP htu mismatch".into()));
    }
    if htm != method {
        return Err(super::CpError::Unauthorized("DPoP htm mismatch".into()));
    }

    // jkt binding to the machine's public key
    let jkt = header
        .get("jkt")
        .and_then(|v| v.as_str())
        .ok_or_else(|| super::CpError::Unauthorized("DPoP jkt required".into()))?;
    if jkt != expected_jkt {
        return Err(super::CpError::Unauthorized("DPoP jkt does not match machine key".into()));
    }

    // Verify EdDSA signature over header.payload using the machine key.
    let sig_bytes =
        URL_SAFE_NO_PAD.decode(sig_b64).map_err(|e| super::CpError::Unauthorized(format!("invalid sig: {e}")))?;
    let signature: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| super::CpError::Unauthorized("signature must be 64 bytes".into()))?;
    machine_pub_key
        .verify(signing_input.as_bytes(), &Signature::from_bytes(&signature))
        .map_err(|_| super::CpError::Unauthorized("DPoP signature verification failed".into()))?;
    Ok(())
}

/// Axum middleware: require `Authorization: Bearer <token>` + `DPoP: <proof JWT>`
/// on every org-scoped route. Extracts machine_pub + org_id into request extensions.
pub async fn require_machine_auth(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, super::CpError> {
    let Some(header_value) = request.headers().get(header::AUTHORIZATION) else {
        return Err(super::CpError::Unauthorized("missing Authorization header".into()));
    };
    let auth = header_value.to_str().map_err(|_| {
        super::CpError::Unauthorized("invalid Authorization header".into())
    })?;
    let (scheme, token) = auth.split_once(' ').ok_or_else(|| {
        super::CpError::Unauthorized("Authorization must be `Bearer <token>`".into())
    })?;
    if scheme != "Bearer" {
        return Err(super::CpError::Unauthorized("Authorization scheme must be Bearer".into()));
    }

    let Some(proof) = request.headers().get("DPoP").and_then(|v| v.to_str().ok()) else {
        return Err(super::CpError::Unauthorized("missing DPoP header".into()));
    };

    let (machine_pub, org_id) = resolve_token(&state, token)?;
    let machine_pub_bytes = URL_SAFE_NO_PAD.decode(machine_pub.as_str()).map_err(|e| {
        super::CpError::Unauthorized(format!("invalid machine public key: {e}"))
    })?;
    let machine_pub_key = VerifyingKey::from_bytes(
        machine_pub_bytes.as_slice().try_into().map_err(|_| {
            super::CpError::Unauthorized("machine public key must be 32 bytes".into())
        })?,
    )
    .map_err(|e| super::CpError::Unauthorized(format!("invalid machine public key: {e}")))?;

    let method = request.method().as_str();
    let uri = request.uri().path().to_string();
    verify_dpop_proof(&state, &machine_pub_key, proof, method, &uri)?;

    request.extensions_mut().insert(machine_pub.to_string());
    request.extensions_mut().insert(org_id.to_string());

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    #[test]
    fn enrollment_signature_roundtrip() {
        let sk = SigningKey::generate(&mut rand_core::OsRng);
        let vk = sk.verifying_key();
        let nonce = new_nonce_hex();
        let mut signed = vk.to_bytes().to_vec();
        signed.extend_from_slice(&hex::decode(&nonce).unwrap());
        let sig = sk.sign(&signed);
        assert!(verify_enrollment_signature(&vk, &hex::encode(sig.to_bytes()), &nonce).unwrap());
        // wrong nonce fails
        assert!(!verify_enrollment_signature(&vk, &hex::encode(sig.to_bytes()), "deadbeef").unwrap());
    }

    #[test]
    fn jwk_thumbprint_is_deterministic() {
        let sk = SigningKey::generate(&mut rand_core::OsRng);
        let vk = sk.verifying_key();
        let a = jwk_thumbprint(&vk);
        let b = jwk_thumbprint(&vk);
        assert_eq!(a, b);
        assert_eq!(a.len(), 43); // base64url of 32 bytes = 43 chars
    }

    #[test]
    fn rejecting_wrong_kty() {
        let bad = json!({"kty": "RSA", "crv": "Ed25519", "x": "abc"});
        assert!(verifying_key_from_jwk(&bad).is_err());
    }

    #[test]
    fn enroll_challenge_and_confirm_roundtrip() {
        let sk = SigningKey::generate(&mut rand_core::OsRng);
        let vk = sk.verifying_key();
        let machine_pub = URL_SAFE_NO_PAD.encode(vk.to_bytes());

        // Server side: issue a challenge nonce bound to the machine key.
        let nonce = new_nonce_hex();

        // Client side: sign machine_pub_bytes || nonce.
        let mut signed = vk.to_bytes().to_vec();
        signed.extend_from_slice(&hex::decode(&nonce).unwrap());
        let sig = sk.sign(&signed);

        // Server side: verify.
        assert!(verify_enrollment_signature(&vk, &hex::encode(sig.to_bytes()), &nonce).unwrap());

        // Wrong nonce must fail.
        assert!(!verify_enrollment_signature(&vk, &hex::encode(sig.to_bytes()), "deadbeef").unwrap());

        // _machine_pub is the lookup key for the pending challenge.
        let _ = machine_pub;
    }
}
