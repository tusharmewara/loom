//! `loom-billing` — client-side Stripe billing for the control plane.
//!
//! Pulls an org's shared registry (skills, MCP servers, workflow templates)
//! from the control plane and reconciles it against local registries using
//! the advisory-deletion conflict rule:
//! - Org adds/updates flow through (upsert).
//! - Local wins on divergence (same id, different content → local kept).
//! - Org deletion → `retired-by-org` sidecar, never silent delete.
//!
//! Library only — no CLI. `loom org registry sync` deferred to a later phase.

pub mod stripe;
pub mod webhook;
pub mod issuance;

pub use stripe::StripeClient;
pub use webhook::WebhookHandler;
pub use issuance::{LicenseIssuer, IssuanceError};
