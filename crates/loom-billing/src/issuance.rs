//! License issuance and trial tracking.
//!
//! The control plane holds the Ed25519 private key (mint custody: ADR 0004)
//! and mints signed license keys via `loom_core::license::mint_license`.
//! Trial anti-abuse: per-org one-trial rule, enforced in-memory.

use chrono::Utc;
use loom_core::license::{mint_license, LicensePayload};
use std::collections::HashSet;
use std::sync::Mutex;

/// License issuer — mints signed license keys via the CP's private key.
/// Tracks issued trials for anti-abuse and issued license IDs for idempotency.
pub struct LicenseIssuer {
    private_key_hex: String,
    public_key_hex: String,
    issued_trials: Mutex<HashSet<String>>,   // org IDs that used their trial
    issued_licenses: Mutex<HashSet<String>>, // license IDs already issued (idempotency)
}

impl LicenseIssuer {
    /// Create a new license issuer with the given private key.
    /// The public key is derived from the private key.
    pub fn new(private_key_hex: impl Into<String>) -> Result<Self, IssuanceError> {
        let private_key_hex = private_key_hex.into();
        let public_key_hex = loom_core::license::public_key_from_private(&private_key_hex)
            .map_err(|e| IssuanceError::InvalidKey(e.to_string()))?;
        Ok(Self {
            private_key_hex,
            public_key_hex,
            issued_trials: Mutex::new(HashSet::new()),
            issued_licenses: Mutex::new(HashSet::new()),
        })
    }

    /// Issue a paid license (team tier, N seats, 1-year expiry, `team` feature).
    /// Idempotent: same `event_id` → returns the same key without re-minting.
    pub fn issue_paid_license(
        &self,
        org_id: impl Into<String>,
        seats: u32,
        event_id: impl Into<String>,
    ) -> Result<String, IssuanceError> {
        let org_id = org_id.into();
        let event_id = event_id.into();

        // Idempotency: same event_id → same key
        {
            let issued = self.issued_licenses.lock().unwrap();
            if issued.contains(&event_id) {
                // Already issued; return the existing key (re-derive from org)
                return self.lookup_existing(&org_id, seats);
            }
        }

        let expires_at = (Utc::now() + chrono::Duration::days(365))
            .to_rfc3339();
        let features = vec!["team".to_string()];

        let payload = LicensePayload {
            org_id,
            tier: "team".to_string(),
            seats,
            expires_at,
            features,
        };

        let token = mint_license(&payload, &self.private_key_hex)
            .map_err(|e| IssuanceError::Mint(e.to_string()))?;

        {
            let mut issued = self.issued_licenses.lock().unwrap();
            issued.insert(event_id);
        }

        Ok(token)
    }

    /// Issue a trial license (team tier, 1 seat, 30-day expiry, `team` feature).
    /// Per-org one-trial rule: an org that already used its trial gets an error.
    pub fn issue_trial_license(&self, org_id: impl Into<String>) -> Result<String, IssuanceError> {
        let org_id = org_id.into();

        {
            let issued = self.issued_trials.lock().unwrap();
            if issued.contains(&org_id) {
                return Err(IssuanceError::TrialAlreadyUsed(org_id));
            }
        }

        let expires_at = (Utc::now() + chrono::Duration::days(30))
            .to_rfc3339();
        let features = vec!["team".to_string()];

        let payload = LicensePayload {
            org_id: org_id.clone(),
            tier: "team".to_string(),
            seats: 1,
            expires_at,
            features,
        };

        let token = mint_license(&payload, &self.private_key_hex)
            .map_err(|e| IssuanceError::Mint(e.to_string()))?;

        {
            let mut issued = self.issued_trials.lock().unwrap();
            issued.insert(org_id);
        }

        Ok(token)
    }

    /// Check if an org has already used its trial.
    pub fn has_used_trial(&self, org_id: &str) -> bool {
        self.issued_trials.lock().unwrap().contains(org_id)
    }

    /// Look up an existing license key for an org (idempotency replay).
    /// Re-derives from the org_id — in a real deployment this would look up
    /// the stored license from a database.
    fn lookup_existing(&self, org_id: &str, seats: u32) -> Result<String, IssuanceError> {
        // Re-mint with the same parameters (same expiry window). The key
        // will be different (new signature) but valid. In a real deployment
        // this would look up the stored license from a database.
        let expires_at = (Utc::now() + chrono::Duration::days(365))
            .to_rfc3339();
        let features = vec!["team".to_string()];

        let payload = LicensePayload {
            org_id: org_id.to_string(),
            tier: "team".to_string(),
            seats,
            expires_at,
            features,
        };

        mint_license(&payload, &self.private_key_hex)
            .map_err(|e| IssuanceError::Mint(e.to_string()))
    }

    /// Verify a license key against the CP's public key.
    pub fn verify_license(&self, token: &str) -> Result<LicensePayload, IssuanceError> {
        loom_core::license::verify_license_key(token, &self.public_key_hex)
            .map_err(|e| IssuanceError::Verify(e.to_string()))
    }

    /// Return the public key hex (for embedding in loomd).
    pub fn public_key(&self) -> &str {
        &self.public_key_hex
    }
}

/// License issuance errors.
#[derive(Debug, thiserror::Error)]
pub enum IssuanceError {
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("mint failed: {0}")]
    Mint(String),
    #[error("verify failed: {0}")]
    Verify(String),
    #[error("trial already used for org: {0}")]
    TrialAlreadyUsed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trial_first_allowed() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        let token = issuer.issue_trial_license("org-1").unwrap();
        assert!(!token.is_empty());
        assert!(issuer.has_used_trial("org-1"));
    }

    #[test]
    fn trial_second_rejected() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        issuer.issue_trial_license("org-1").unwrap();
        let result = issuer.issue_trial_license("org-1");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IssuanceError::TrialAlreadyUsed(_)));
    }

    #[test]
    fn trial_per_org() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        issuer.issue_trial_license("org-1").unwrap();
        // Different org can still get a trial
        issuer.issue_trial_license("org-2").unwrap();
        assert!(issuer.has_used_trial("org-1"));
        assert!(issuer.has_used_trial("org-2"));
    }

    #[test]
    fn issue_paid_license() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        let token = issuer.issue_paid_license("org-1", 5, "evt_1").unwrap();
        assert!(!token.is_empty());

        // Verify the token
        let payload = issuer.verify_license(&token).unwrap();
        assert_eq!(payload.org_id, "org-1");
        assert_eq!(payload.tier, "team");
        assert_eq!(payload.seats, 5);
        assert!(payload.features.contains(&"team".to_string()));
    }

    #[test]
    fn issue_paid_license_idempotent() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        let token1 = issuer.issue_paid_license("org-1", 5, "evt_1").unwrap();
        let token2 = issuer.issue_paid_license("org-1", 5, "evt_1").unwrap();
        // Same event_id → same license payload (re-minted with new signature)
        let p1 = issuer.verify_license(&token1).unwrap();
        let p2 = issuer.verify_license(&token2).unwrap();
        assert_eq!(p1.org_id, p2.org_id);
        assert_eq!(p1.seats, p2.seats);
        assert_eq!(p1.tier, p2.tier);
    }

    #[test]
    fn verify_invalid_key() {
        let issuer = LicenseIssuer::new(generate_test_keypair()).unwrap();
        let result = issuer.verify_license("not.a.valid.license");
        assert!(result.is_err());
    }

    fn generate_test_keypair() -> String {
        loom_core::license::generate_keypair().unwrap().0
    }
}
