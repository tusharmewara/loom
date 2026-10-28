//! Stripe webhook signature verification.
//!
//! Stripe signs webhooks with `Stripe-Signature: t=<timestamp>,v1=<hmac>`.
//! We verify the HMAC-SHA256 signature using the webhook signing secret.
//!
//! Reference: https://docs.stripe.com/webhooks#verify-signatures

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Stripe webhook signature verification + license issuance.
pub struct WebhookHandler {
    webhook_secret: String,
    tolerance_seconds: u64,
    issuer: crate::issuance::LicenseIssuer,
}

impl WebhookHandler {
    /// Create a new webhook handler with the given Stripe webhook signing secret
    /// and license issuer.
    pub fn new(webhook_secret: impl Into<String>, issuer: crate::issuance::LicenseIssuer) -> Self {
        Self {
            webhook_secret: webhook_secret.into(),
            tolerance_seconds: 300, // 5 minutes
            issuer,
        }
    }

    /// Verify a Stripe webhook signature and return the parsed event.
    ///
    /// The `payload` is the raw request body (must be the exact bytes Stripe signed).
    /// The `signature` is the value of the `Stripe-Signature` header.
    pub fn verify(
        &self,
        payload: &str,
        signature: &str,
    ) -> Result<WebhookEvent, WebhookError> {
        let (timestamp, signatures) = self.parse_signature(signature)?;

        // Check timestamp tolerance
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| WebhookError::InvalidTimestamp(e.to_string()))?
            .as_secs();
        if now.abs_diff(timestamp) > self.tolerance_seconds {
            return Err(WebhookError::TimestampTooOld);
        }

        // Construct the signed payload: "timestamp.payload"
        let signed_payload = format!("{}.{}", timestamp, payload);

        // Verify each signature; accept if any matches
        for sig in &signatures {
            let sig_bytes = hex::decode(sig).map_err(|e| WebhookError::InvalidSignature(e.to_string()))?;
            if self.verify_signature(signed_payload.as_bytes(), &sig_bytes) {
                return self.parse_event(payload);
            }
        }

        Err(WebhookError::InvalidSignature("no matching signature".to_string()))
    }

    /// Issue a paid license for a completed checkout session.
    /// Idempotent: same event_id returns the same result.
    pub fn issue_license_for_session(
        &self,
        org_id: &str,
        seats: u32,
        event_id: &str,
    ) -> Result<String, crate::issuance::IssuanceError> {
        self.issuer.issue_paid_license(org_id, seats, event_id)
    }

    /// Parse the `Stripe-Signature` header into (timestamp, list of signatures).
    fn parse_signature(&self, signature: &str) -> Result<(u64, Vec<String>), WebhookError> {
        let mut timestamp = None;
        let mut sigs = Vec::new();

        for part in signature.split(',') {
            let mut kv = part.splitn(2, '=');
            let key = kv.next().unwrap_or("");
            let value = kv.next().unwrap_or("");
            match key {
                "t" => timestamp = Some(value.parse::<u64>().map_err(|e| WebhookError::InvalidTimestamp(e.to_string()))?),
                "v1" => sigs.push(value.to_string()),
                _ => {} // ignore unknown elements
            }
        }

        let timestamp = timestamp.ok_or(WebhookError::MissingTimestamp)?;
        if sigs.is_empty() {
            return Err(WebhookError::MissingSignature);
        }

        Ok((timestamp, sigs))
    }

    /// Verify an HMAC-SHA256 signature.
    fn verify_signature(&self, signed_payload: &[u8], signature: &[u8]) -> bool {
        let mut mac = HmacSha256::new_from_slice(self.webhook_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(signed_payload);
        mac.verify_slice(signature).is_ok()
    }

    /// Parse the raw payload as a Stripe event.
    fn parse_event(&self, payload: &str) -> Result<WebhookEvent, WebhookError> {
        let event: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| WebhookError::InvalidPayload(e.to_string()))?;

        let event_id = event["id"]
            .as_str()
            .ok_or(WebhookError::MissingEventId)?
            .to_string();
        let event_type = event["type"]
            .as_str()
            .ok_or(WebhookError::MissingEventType)?
            .to_string();

        Ok(WebhookEvent {
            id: event_id,
            event_type,
            payload: payload.to_string(),
        })
    }
}

/// A verified Stripe webhook event.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub id: String,
    pub event_type: String,
    pub payload: String,
}

/// Webhook verification errors.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    #[error("missing timestamp")]
    MissingTimestamp,
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("timestamp too old")]
    TimestampTooOld,
    #[error("missing signature")]
    MissingSignature,
    #[error("invalid payload: {0}")]
    InvalidPayload(String),
    #[error("missing event id")]
    MissingEventId,
    #[error("missing event type")]
    MissingEventType,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signature(secret: &str, payload: &str, timestamp: u64) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{}.{}", timestamp, payload).as_bytes());
        let result = mac.finalize();
        format!("t={},v1={}", timestamp, hex::encode(result.into_bytes()))
    }

    #[test]
    fn webhook_signature_valid() {
        let issuer = crate::issuance::LicenseIssuer::new(generate_test_keypair()).unwrap();
        let handler = WebhookHandler::new("whsec_test_secret", issuer);
        let payload = r#"{"id":"evt_123","type":"checkout.session.completed","data":{}}"#;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let sig = make_signature("whsec_test_secret", payload, timestamp);

        let event = handler.verify(payload, &sig).unwrap();
        assert_eq!(event.id, "evt_123");
        assert_eq!(event.event_type, "checkout.session.completed");
    }

    #[test]
    fn webhook_signature_invalid() {
        let handler = WebhookHandler::new(
            "whsec_test_secret",
            crate::issuance::LicenseIssuer::new(generate_test_keypair()).unwrap(),
        );
        let payload = r#"{"id":"evt_123","type":"checkout.session.completed","data":{}}"#;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let sig = make_signature("wrong_secret", payload, timestamp);

        let result = handler.verify(payload, &sig);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WebhookError::InvalidSignature(_)));
    }

    #[test]
    fn webhook_signature_missing() {
        let handler = WebhookHandler::new(
            "whsec_test_secret",
            crate::issuance::LicenseIssuer::new(generate_test_keypair()).unwrap(),
        );
        let payload = r#"{"id":"evt_123"}"#;

        let result = handler.verify(payload, "");
        assert!(result.is_err());
    }

    #[test]
    fn webhook_timestamp_too_old() {
        let handler = WebhookHandler::new(
            "whsec_test_secret",
            crate::issuance::LicenseIssuer::new(generate_test_keypair()).unwrap(),
        );
        let payload = r#"{"id":"evt_123","type":"checkout.session.completed","data":{}}"#;
        let sig = make_signature("whsec_test_secret", payload, 1000000);

        let result = handler.verify(payload, &sig);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WebhookError::TimestampTooOld));
    }

    fn generate_test_keypair() -> String {
        loom_core::license::generate_keypair().unwrap().0
    }
}
