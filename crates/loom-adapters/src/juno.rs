use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    McpSection, RoutingSection, SessionsSection, SkillsSection, ToolSection, ToolToml,
};

pub struct JunoAdapter;

impl LoomToolAdapter for JunoAdapter {
    fn tool_name(&self) -> &'static str {
        "juno"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        // Juno is a VS Code fork (like Cursor/Kiro).
        // User data dir: ~/Library/Application Support/Juno/ (macOS) or ~/.config/Juno/ (Linux)
        let user_dir = if cfg!(target_os = "linux") {
            format!("{home}/.config/Juno")
        } else {
            format!("{home}/Library/Application Support/Juno")
        };

        ToolToml {
            tool: ToolSection {
                name: "juno".into(),
                version: "1.0.0".into(),
            },
            skills: Some(SkillsSection {
                native_dir: Some(format!("{user_dir}/User/skills")),
                import_on_register: true,
                shared_dir: None,
                extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{user_dir}/User/globalStorage/loom/mcp.json")),
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
                store_path: Some(format!("{user_dir}/User/globalStorage/sessions")),
                auto_export: true,
                max_shared_sessions: 50,
            }),
            lsp: None,
            debug: None,
        }
    }

    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        std::env::set_var("JUNO_MODEL", &decision.recommended_model);
        std::env::set_var("LOOM_ROUTING_PROVIDER", &decision.recommended_provider);
        Ok(())
    }

    fn sync_from_native(&self, loom_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(loom_dir)
    }
}