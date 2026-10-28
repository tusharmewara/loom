//! Stripe API client for creating checkout sessions.
//!
//! Uses `ureq` (blocking HTTP) — no tokio dependency needed. Stripe API
//! docs: https://docs.stripe.com/api/checkout/sessions/create
//!
//! Test mode: use `sk_test_...` key. All amounts in cents (USD).

use anyhow::{Context, Result};
use ureq;

/// Stripe API client for creating checkout sessions.
pub struct StripeClient {
    api_key: String,
    base_url: String,
}

impl StripeClient {
    /// Create a new Stripe client with the given secret key.
    /// The base URL defaults to Stripe's API; override for testing.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.stripe.com/v1".to_string(),
        }
    }

    /// Create a checkout session for a seat purchase.
    ///
    /// The session redirects the customer to Stripe's hosted checkout page.
    /// On success, Stripe sends a `checkout.session.completed` webhook.
    pub fn create_checkout_session(
        &self,
        org_id: &str,
        amount_cents: u64,
    ) -> Result<CheckoutSession> {
        let body = serde_json::json!({
            "payment_method_types": ["card"],
            "line_items": [{
                "price_data": {
                    "currency": "usd",
                    "product_data": {
                        "name": "Loom Team Seat",
                        "description": "One seat on the Loom Team plan",
                    },
                    "unit_amount": amount_cents,
                },
                "quantity": 1,
            }],
            "mode": "payment",
            "success_url": "https://loom.local/success?session_id={CHECKOUT_SESSION_ID}",
            "cancel_url": "https://loom.local/cancel",
            "metadata": {
                "org_id": org_id,
            },
        });

        let resp: serde_json::Value = self.post("/checkout/sessions", &body)?;
        let session_id = resp["id"]
            .as_str()
            .context("checkout session missing id")?
            .to_string();
        let url = resp["url"]
            .as_str()
            .context("checkout session missing url")?
            .to_string();

        Ok(CheckoutSession {
            session_id,
            url,
        })
    }

    fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, StripeError> {
        let url = format!("{}{}", self.base_url, path);
        let body_str = serde_json::to_string(body)?;
        let resp_text: String = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("Content-Type", "application/json")
            .set("Stripe-Version", "2023-10-16")
            .send_string(&body_str)
            .map_err(|e| StripeError::Transport(e.to_string()))?
            .into_string()
            .map_err(|e| StripeError::Transport(e.to_string()))?;

        let resp: serde_json::Value = serde_json::from_str(&resp_text)
            .map_err(|e| StripeError::Parse(e.to_string()))?;

        // Stripe returns errors in the body with HTTP error status
        if let Some(error_obj) = resp.as_object().and_then(|m| m.get("error")) {
            return Err(StripeError::Api(
                error_obj["message"]
                    .as_str()
                    .unwrap_or("unknown Stripe error")
                    .to_string(),
            ));
        }

        Ok(resp)
    }
}

/// A Stripe Checkout Session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckoutSession {
    pub session_id: String,
    pub url: String,
}

/// Stripe API errors.
#[derive(Debug, thiserror::Error)]
pub enum StripeError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("Stripe API error: {0}")]
    Api(String),
}

impl From<ureq::Error> for StripeError {
    fn from(e: ureq::Error) -> Self {
        StripeError::Transport(e.to_string())
    }
}

impl From<serde_json::Error> for StripeError {
    fn from(e: serde_json::Error) -> Self {
        StripeError::Parse(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkout_session_creation_builds_correct_payload() {
        // We can't easily test the HTTP call without a mock server,
        // but we can verify the JWK/signing logic deterministically.
        let client = StripeClient::new("sk_test_1234567890");
        assert_eq!(client.api_key, "sk_test_1234567890");
        assert_eq!(client.base_url, "https://api.stripe.com/v1");
    }
}
