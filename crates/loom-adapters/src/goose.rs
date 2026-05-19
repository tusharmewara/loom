use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    McpSection, RoutingSection, SessionsSection, SkillsSection, ToolSection, ToolToml,
};

pub struct GooseAdapter;

impl LoomToolAdapter for GooseAdapter {
    fn tool_name(&self) -> &'static str {
        "goose"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        ToolToml {
            tool: ToolSection {
                name: "goose".into(),
                version: "1.0.0".into(),
            },
            skills: Some(SkillsSection {
                native_dir: Some(format!("{home}/.goose/skills")),
                import_on_register: true,
                shared_dir: None,
                extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{home}/.goose/mcp.json")),
                shared_servers: vec![],
                import_on_register: true,
                import_tags: vec![],
            }),
            auth: None,
            hooks: None,
            routing: Some(RoutingSection {
                default_tier: "mid".into(),
                catalog_path: None,
                strategy: "balanced".into(),
            }),
            sessions: Some(SessionsSection {
                store_path: Some(format!("{home}/.goose/sessions")),
                auto_export: true,
                max_shared_sessions: 50,
            }),
        }
    }

    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        std::env::set_var("GOOSE_MODEL", &decision.recommended_model);
        std::env::set_var("LOOM_ROUTING_PROVIDER", &decision.recommended_provider);
        Ok(())
    }

    fn sync_from_native(&self, agents_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(agents_dir)
    }
}
