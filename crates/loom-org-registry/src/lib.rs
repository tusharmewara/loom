//! `loom-org-registry` — client-side org registry pull-through.
//!
//! Pulls an org's shared registry (skills, MCP servers, workflow templates)
//! from the control plane and reconciles it against local registries using
//! the advisory-deletion conflict rule:
//! - Org adds/updates flow through (upsert).
//! - Local wins on divergence (same id, different content → local kept).
//! - Org deletion → `retired-by-org` sidecar, never silent delete.
//!
//! Library only — no CLI. `loom org registry sync` deferred to a later phase.

pub mod org_registry;
pub mod report;
pub mod retired;
pub mod sync;

pub use org_registry::{OrgEntry, OrgRegistryResponse};
pub use report::SyncReport;
pub use retired::RetiredStore;
pub use sync::OrgSync;
