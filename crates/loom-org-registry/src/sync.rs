//! OrgSync — pull-through sync between a control-plane org registry and local
//! registries (skills, MCP servers, workflow templates).
//!
//! Advisory-deletion conflict rule:
//! - Org adds/updates flow through (upsert).
//! - Local wins on divergence (same id, different content → local kept).
//! - Org deletion → `retired-by-org` sidecar, never silent delete.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use ureq;

use crate::org_registry::{OrgEntry, OrgRegistryResponse};
use crate::report::SyncReport;
use crate::retired::RetiredStore;

/// `OrgSync` pulls an org's shared registry from the control plane and
/// reconciles it against local registries.
pub struct OrgSync {
    base_url: String,
    org_id: String,
    skills: loom_skills::registry::SkillRegistry,
    mcp: loom_mcp::McpRegistry,
    templates: loom_workflow::TemplateManager,
    retired: RetiredStore,
}

impl OrgSync {
    /// Create a new `OrgSync` for `org_id` talking to `cp_base_url`, using
    /// local registries under `loom_dir`.
    pub fn new(loom_dir: &str, cp_base_url: &str, org_id: &str) -> Self {
        let skills =
            loom_skills::registry::SkillRegistry::new(loom_dir).expect("skills registry");
        let mcp = loom_mcp::McpRegistry::new(loom_dir).expect("mcp registry");
        let templates = loom_workflow::TemplateManager::new(loom_dir);
        let retired_path = Path::new(loom_dir).join("org").join(org_id).join("retired.json");
        let retired = RetiredStore::new(&retired_path);

        Self {
            base_url: cp_base_url.to_string(),
            org_id: org_id.to_string(),
            skills,
            mcp,
            templates,
            retired,
        }
    }

    /// Pull the org registry from the CP and reconcile against local
    /// registries. Returns a `SyncReport` with counts of upserts, skips
    /// (divergence), retirements, and restorations.
    pub fn sync(&mut self) -> anyhow::Result<SyncReport> {
        let response = self.pull_registry()?;

        let mut report = SyncReport::default();

        // Collect org entry ids by kind for the advisory-deletion step.
        let mut org_ids: HashMap<String, Vec<String>> = HashMap::new();
        for entry in &response.entries {
            org_ids
                .entry(entry.kind.clone())
                .or_default()
                .push(entry.id.clone());
        }

        // Reconcile each org entry against local registries.
        for entry in &response.entries {
            match entry.kind.as_str() {
                "skill" => self.sync_skill(entry, &mut report)?,
                "mcp" => self.sync_mcp(entry, &mut report)?,
                "template" => self.sync_template(entry, &mut report)?,
                _ => {} // unknown kind, ignore
            }
        }

        // Advisory-deletion: local items not in org registry → retire.
        self.retire_absent_locals(&org_ids, &mut report)?;

        // Restoration: retired items now back in org registry → restore.
        self.restore_returning_items(&org_ids, &mut report)?;

        Ok(report)
    }

    /// Pull `GET {base_url}/api/v1/org/{org_id}/registry` from the CP.
    fn pull_registry(&self) -> anyhow::Result<OrgRegistryResponse> {
        let url = format!("{}/api/v1/org/{}/registry", self.base_url, self.org_id);
        let resp: String = ureq::get(&url).call()?.into_string()?;
        serde_json::from_str(&resp).map_err(|e| anyhow::anyhow!("parse registry: {e}"))
    }

    /// Sync a single skill entry.
    fn sync_skill(
        &mut self,
        entry: &OrgEntry,
        report: &mut SyncReport,
    ) -> anyhow::Result<()> {
        let Some(skill) = entry.as_skill() else {
            return Ok(());
        };

        let existing = self.skills.get(&entry.id);
        if let Some(existing) = existing {
            if self.content_diverges(&existing, &entry.content) {
                // Local wins on divergence.
                report.skips += 1;
                return Ok(());
            }
        }

        self.skills.upsert(skill)?;
        report.upserts += 1;
        Ok(())
    }

    /// Sync a single MCP server entry.
    fn sync_mcp(
        &mut self,
        entry: &OrgEntry,
        report: &mut SyncReport,
    ) -> anyhow::Result<()> {
        let Some(mcp) = entry.as_mcp() else {
            return Ok(());
        };

        let existing = self.mcp.get(&entry.id).ok().flatten();
        if let Some(existing) = existing {
            if self.content_diverges(&existing, &entry.content) {
                report.skips += 1;
                return Ok(());
            }
        }

        self.mcp.upsert(&mcp)?;
        report.upserts += 1;
        Ok(())
    }

    /// Sync a single template entry.
    fn sync_template(
        &mut self,
        entry: &OrgEntry,
        report: &mut SyncReport,
    ) -> anyhow::Result<()> {
        let Some(template) = entry.as_template() else {
            return Ok(());
        };

        let existing = self.templates.get(&entry.id);
        if let Some(existing) = existing {
            if self.content_diverges(&existing, &entry.content) {
                report.skips += 1;
                return Ok(());
            }
        }

        self.templates.save(&template)?;
        report.upserts += 1;
        Ok(())
    }

    /// Compare a local serialized item against the org's content Value.
    /// Returns true if they diverge (local should win).
    fn content_diverges<T: Serialize>(
        &self,
        local: &T,
        org_content: &Value,
    ) -> bool {
        let local_value = serde_json::to_value(local).unwrap_or(Value::Null);
        local_value != *org_content
    }

    /// Advisory-deletion: for each local item not present in the org
    /// registry, mark it `retired-by-org` instead of deleting it.
    fn retire_absent_locals(
        &mut self,
        org_ids: &HashMap<String, Vec<String>>,
        report: &mut SyncReport,
    ) -> anyhow::Result<()> {
        // Local skills not in org → retire.
        for skill in self.skills.list() {
            if !org_ids
                .get("skill")
                .map(|ids| ids.iter().any(|i| i == &skill.name))
                .unwrap_or(false)
                && !self.retired.is_retired("skill", &skill.name)
            {
                self.retired.retire("skill", &skill.name);
                report.retirements += 1;
            }
        }

        // Local MCP servers not in org → retire.
        if let Ok(servers) = self.mcp.list() {
            for server in servers {
                if !org_ids
                    .get("mcp")
                    .map(|ids| ids.iter().any(|i| i == &server.id))
                    .unwrap_or(false)
                    && !self.retired.is_retired("mcp", &server.id)
                {
                    self.retired.retire("mcp", &server.id);
                    report.retirements += 1;
                }
            }
        }

        // Local templates not in org → retire.
        for template in self.templates.list() {
            if !org_ids
                .get("template")
                .map(|ids| ids.iter().any(|i| i == &template.name))
                .unwrap_or(false)
                && !self.retired.is_retired("template", &template.name)
            {
                self.retired.retire("template", &template.name);
                report.retirements += 1;
            }
        }

        self.retired.save()?;
        Ok(())
    }

    /// Restoration: for each retired item now back in the org registry,
    /// clear the retirement marker.
    fn restore_returning_items(
        &mut self,
        org_ids: &HashMap<String, Vec<String>>,
        report: &mut SyncReport,
    ) -> anyhow::Result<()> {
        let retired_items = self.retired.list_retired();
        for item in &retired_items {
            if org_ids
                .get(&item.kind)
                .map(|ids| ids.contains(&item.id))
                .unwrap_or(false)
            {
                self.retired.restore(&item.kind, &item.id);
                report.restorations += 1;
            }
        }

        self.retired.save()?;
        Ok(())
    }

    /// Filtered views — exclude retired-by-org items.
    /// List skills, excluding retired ones.
    pub fn list_skills(&self) -> Vec<loom_core::SkillEntry> {
        self.skills
            .list()
            .into_iter()
            .filter(|s| !self.retired.is_retired("skill", &s.name))
            .collect()
    }

    /// List MCP servers, excluding retired ones.
    pub fn list_mcp_servers(&self) -> Vec<loom_core::McpServerEntry> {
        self.mcp
            .list()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| !self.retired.is_retired("mcp", &s.id))
            .collect()
    }

    /// List templates, excluding retired ones.
    pub fn list_templates(&self) -> Vec<loom_core::WorkflowTemplate> {
        self.templates
            .list()
            .into_iter()
            .filter(|t| !self.retired.is_retired("template", &t.name))
            .collect()
    }

    /// Get a skill by name, excluding retired ones.
    pub fn get_skill(&self, name: &str) -> Option<loom_core::SkillEntry> {
        if self.retired.is_retired("skill", name) {
            return None;
        }
        self.skills.get(name)
    }

    /// Get an MCP server by id, excluding retired ones.
    pub fn get_mcp_server(&self, id: &str) -> Option<loom_core::McpServerEntry> {
        if self.retired.is_retired("mcp", id) {
            return None;
        }
        self.mcp.get(id).ok().flatten()
    }

    /// Get a template by name, excluding retired ones.
    pub fn get_template(&self, name: &str) -> Option<loom_core::WorkflowTemplate> {
        if self.retired.is_retired("template", name) {
            return None;
        }
        self.templates.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{McpServerEntry, McpTransport, McpTransportType, SkillEntry, WorkflowTemplate};
    use tempfile::tempdir;

    fn test_skill(name: &str) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            description: format!("desc for {name}"),
            path: format!("/tmp/{name}.md"),
            content_hash: format!("hash-{name}"),
            tags: vec![],
            ecosystem_compatible: vec![],
            ecosystem_requires: vec![],
            version: "1.0.0".to_string(),
            last_modified: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn test_template(name: &str) -> WorkflowTemplate {
        WorkflowTemplate {
            name: name.to_string(),
            description: format!("template {name}"),
            default_title: None,
            default_description: None,
            default_agent: None,
            sub_agents: None,
            default_provider: None,
            default_model: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sync_new_items() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-test";

        let dir = tempdir().unwrap();
        let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);

        let report = sync.sync().unwrap();

        // Should have upserted all org entries.
        assert!(report.upserts > 0, "expected upserts, got {report:?}");
        assert_eq!(report.retirements, 0);
        assert_eq!(report.restorations, 0);

        // Verify skills were synced.
        let skills = sync.list_skills();
        assert!(!skills.is_empty(), "expected synced skills");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_two_machines_converge() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-converge";

        // Machine 1.
        let dir1 = tempdir().unwrap();
        let mut sync1 = OrgSync::new(dir1.path().to_str().unwrap(), &base_url, org_id);
        sync1.sync().unwrap();

        // Machine 2.
        let dir2 = tempdir().unwrap();
        let mut sync2 = OrgSync::new(dir2.path().to_str().unwrap(), &base_url, org_id);
        sync2.sync().unwrap();

        // Both should have the same skills.
        let skills1: Vec<String> = sync1.list_skills().into_iter().map(|s| s.name).collect();
        let skills2: Vec<String> = sync2.list_skills().into_iter().map(|s| s.name).collect();
        assert_eq!(skills1, skills2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_advisory_deletion_retires_not_deletes() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-retire";

        // Seed a local skill that is NOT in the org registry (write to disk,
        // since OrgSync reads skills from the filesystem).
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills").join("local-only");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: local-only\ndescription: A local-only skill\nversion: 1.0.0\n---\n",
        )
        .unwrap();

        let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);
        let report = sync.sync().unwrap();

        // The local-only skill should be retired, not deleted.
        assert!(report.retirements > 0, "expected retirements, got {report:?}");

        // The skill is still in the underlying registry (not deleted).
        assert!(sync.skills.get("local-only").is_some(), "skill not deleted");

        // But hidden from filtered views.
        let visible_skills = sync.list_skills();
        assert!(
            !visible_skills.iter().any(|s| s.name == "local-only"),
            "retired skill hidden from list"
        );

        // The retired store tracks it.
        assert!(sync.retired.is_retired("skill", "local-only"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_local_wins_on_divergence() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-diverge";

        // Seed a local skill with same name as org skill but different content.
        // Write to disk since OrgSync reads skills from filesystem.
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills").join("shared-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: shared-skill\ndescription: locally modified\nversion: 1.0.0\n---\n",
        )
        .unwrap();

        let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);
        let report = sync.sync().unwrap();

        // The divergence should be a skip, not an upsert.
        assert!(report.skips > 0, "expected skips for divergence, got {report:?}");

        // Local content preserved.
        let skill = sync.skills.get("shared-skill").unwrap();
        assert_eq!(skill.description, "locally modified");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cp_registry_endpoint_shape() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-endpoint";

        let resp: OrgRegistryResponse = serde_json::from_str(
            &ureq::get(&format!("{base_url}/api/v1/org/{org_id}/registry"))
                .call()
                .unwrap()
                .into_string()
                .unwrap(),
        )
        .unwrap();

        assert_eq!(resp.org_id, org_id);
        assert!(!resp.entries.is_empty());
        for entry in &resp.entries {
            assert!(matches!(entry.kind.as_str(), "skill" | "mcp" | "template"));
            assert!(!entry.id.is_empty());
            assert!(!entry.updated_at.is_empty());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_restoration_of_retired_item() {
        let cp_port = spawn_test_cp_with_registry().await;
        let base_url = format!("http://127.0.0.1:{cp_port}");
        let org_id = "org-restore";

        let dir = tempdir().unwrap();

        // Write "comeback-skill" to disk (OrgSync reads from filesystem)
        let skill_dir = dir.path().join("skills").join("comeback-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: comeback-skill\ndescription: A comeback skill\nversion: 1.0.0\n---\n",
        )
        .unwrap();

        // Seed CP org with the skill
        let skill = test_skill("comeback-skill");
        let entries = vec![serde_json::json!({
            "kind": "skill",
            "id": "comeback-skill",
            "content": serde_json::to_value(&skill).unwrap(),
            "updated_at": "2026-01-01T00:00:00Z",
        })];
        let _ = ureq::post(&format!("{base_url}/api/v1/org/{org_id}/registry"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(&serde_json::json!({"entries": entries})).unwrap())
            .unwrap();

        // First sync - skill is in both org and local (local from disk)
        {
            let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);
            let _ = sync.sync().unwrap();
            assert!(sync.list_skills().iter().any(|s| s.name == "comeback-skill"));
        }

        // Remove the skill from the CP org registry (empty it).
        let _ = ureq::post(&format!("{base_url}/api/v1/org/{org_id}/registry"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(&serde_json::json!({"entries": []})).unwrap())
            .unwrap();

        // Second sync - skill gets retired.
        {
            let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);
            let report = sync.sync().unwrap();
            assert!(report.retirements > 0, "expected retirements, got {report:?}");
            assert!(sync.retired.is_retired("skill", "comeback-skill"));
        }

        // Re-add the skill to the CP org registry.
        let _ = ureq::post(&format!("{base_url}/api/v1/org/{org_id}/registry"))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(&serde_json::json!({"entries": entries})).unwrap())
            .unwrap();

        // Third sync - skill gets restored.
        {
            let mut sync = OrgSync::new(dir.path().to_str().unwrap(), &base_url, org_id);
            let report = sync.sync().unwrap();
            assert!(report.restorations > 0);
            assert!(!sync.retired.is_retired("skill", "comeback-skill"));
            assert!(sync.list_skills().iter().any(|s| s.name == "comeback-skill"));
        }
    }

    /// Spawn a test CP with a pre-seeded org registry on an ephemeral port.
    async fn spawn_test_cp_with_registry() -> u16 {
        use loom_control_plane::AppState;
        use std::mem;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_cp.db");
        let state = AppState::new(dir.path(), &db_path).await.expect("cp state");

        // Seed orgs with registry entries directly in AppState's org_registry.
        {
            let mut org_registry = state.org_registry.lock().unwrap();

            // Org "org-test" with a skill.
            org_registry.insert(
                "org-test".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![test_skill("shared-skill")],
                    mcps: vec![],
                    templates: vec![],
                },
            );

            // Org "org-converge" with same skill.
            org_registry.insert(
                "org-converge".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![test_skill("shared-skill")],
                    mcps: vec![],
                    templates: vec![],
                },
            );

            // Org "org-endpoint" with one of each kind.
            org_registry.insert(
                "org-endpoint".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![test_skill("endpoint-skill")],
                    mcps: vec![McpServerEntry {
                        id: "endpoint-mcp".to_string(),
                        name: "Endpoint MCP".to_string(),
                        transport: McpTransport {
                            transport_type: McpTransportType::Stdio,
                            command: Some("echo".to_string()),
                            args: vec![],
                            url: None,
                            env: std::collections::HashMap::new(),
                        },
                        shared: true,
                        tools_filter: vec![],
                        health_status: loom_core::HealthStatusStr::Unknown,
                        last_health_check: "2026-01-01T00:00:00Z".to_string(),
                        tags: vec![],
                        compatible_tools: vec![],
                        required_by_skills: vec![],
                    }],
                    templates: vec![test_template("endpoint-template")],
                },
            );

            // Org "org-diverge" with a skill (for divergence test).
            org_registry.insert(
                "org-diverge".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![test_skill("shared-skill")],
                    mcps: vec![],
                    templates: vec![],
                },
            );

            // Org "org-retire" - empty org (local-only skill will be retired).
            org_registry.insert(
                "org-retire".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![],
                    mcps: vec![],
                    templates: vec![],
                },
            );

            // Org "org-restore" - will be manipulated via POST in the test.
            org_registry.insert(
                "org-restore".to_string(),
                loom_control_plane::OrgRegistryData {
                    skills: vec![],
                    mcps: vec![],
                    templates: vec![],
                },
            );
        }
        mem::forget(dir);

        let app = loom_control_plane::build_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }
}
