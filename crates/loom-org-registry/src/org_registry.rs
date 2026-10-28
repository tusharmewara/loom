//! Organization registry entry types pulled from the control plane.
//!
//! An org registry entry is a kind-tagged, JSON-serialized payload that the
//! CP serves at `GET /api/v1/org/{org_id}/registry`. Kinds: `skill`, `mcp`,
//! `template`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The full org registry response from the CP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgRegistryResponse {
    pub org_id: String,
    pub entries: Vec<OrgEntry>,
    pub synced_at: String,
}

/// One entry in the org registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgEntry {
    pub kind: String,       // "skill" | "mcp" | "template"
    pub id: String,         // unique within kind
    pub content: Value,     // SkillEntry / McpServerEntry / WorkflowTemplate as JSON
    pub updated_at: String, // RFC3339 from CP
}

impl OrgEntry {
    /// Try to deserialize the content as a `SkillEntry`.
    pub fn as_skill(&self) -> Option<loom_core::SkillEntry> {
        if self.kind != "skill" {
            return None;
        }
        serde_json::from_value(self.content.clone()).ok()
    }

    /// Try to deserialize the content as an `McpServerEntry`.
    pub fn as_mcp(&self) -> Option<loom_core::McpServerEntry> {
        if self.kind != "mcp" {
            return None;
        }
        serde_json::from_value(self.content.clone()).ok()
    }

    /// Try to deserialize the content as a `WorkflowTemplate`.
    pub fn as_template(&self) -> Option<loom_core::WorkflowTemplate> {
        if self.kind != "template" {
            return None;
        }
        serde_json::from_value(self.content.clone()).ok()
    }
}
