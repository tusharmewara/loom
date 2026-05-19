use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    McpSection, RoutingSection, SessionsSection, SkillsSection, ToolSection, ToolToml,
};
pub struct JcodeAdapter;

impl LoomToolAdapter for JcodeAdapter {
    fn tool_name(&self) -> &'static str {
        "jcode"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        ToolToml {
            tool: ToolSection {
                name: "jcode".into(),
                version: "1.0.0".into(),
            },
            skills: Some(SkillsSection {
                native_dir: Some(format!("{home}/.jcode/skills")),
                import_on_register: true,
                shared_dir: None,
                extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{home}/.jcode/mcp.json")),
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
                store_path: Some(format!("{home}/.jcode/sessions")),
                auto_export: true,
                max_shared_sessions: 50,
            }),
        }
    }

    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        // jcode respects ANTHROPIC_MODEL when set (plan 4.5c)
        std::env::set_var("ANTHROPIC_MODEL", &decision.recommended_model);
        std::env::set_var("LOOM_ROUTING_PROVIDER", &decision.recommended_provider);
        Ok(())
    }

    fn sync_from_native(&self, agents_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(agents_dir)
    }
}
