use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    AuthSection, McpSection, SkillsSection, ToolSection, ToolToml,
};
use std::path::Path;

pub struct ClaudeCodeAdapter;

impl LoomToolAdapter for ClaudeCodeAdapter {
    fn tool_name(&self) -> &'static str {
        "claude-code"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        ToolToml {
            tool: ToolSection {
                name: "claude-code".into(),
                version: "1.0.0".into(),
            },
            skills: Some(SkillsSection {
                native_dir: Some(format!("{home}/.claude/skills")),
                import_on_register: true,
                shared_dir: None,
                extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{home}/.claude/mcp.json")),
                shared_servers: vec![],
                import_on_register: true,
                import_tags: vec![],
            }),
            auth: Some(AuthSection {
                store_path: Some(format!("{home}/.claude/credentials.json")),
                providers: vec!["anthropic".into()],
                import_on_register: true,
            }),
            hooks: None,
            routing: None,
            sessions: None,
            lsp: None,
            debug: None,
                    launch: None,
}
    }

    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        std::env::set_var("ANTHROPIC_MODEL", &decision.recommended_model);
        Ok(())
    }

    fn sync_from_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport> {
        let mut report = crate::sync_tools_dir(loom_dir)?;
        let creds = Path::new(home).join(".claude/credentials.json");
        if creds.exists() {
            let mut auth = loom_auth::AuthManager::new(loom_dir);
            if auth.vault_is_locked() {
                report.notes.push(
                    "claude credentials found; unlock vault to import providers".into(),
                );
            } else if let Ok(providers) = auth.import_from_auth_store(creds.to_str().unwrap()) {
                report.providers_imported = providers.len();
            }
        }
        Ok(report)
    }
}
