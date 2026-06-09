use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    McpSection, RoutingSection, SessionsSection, SkillsSection, ToolSection, ToolToml,
};

pub struct KiroAdapter;

impl LoomToolAdapter for KiroAdapter {
    fn tool_name(&self) -> &'static str {
        "kiro"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        // Kiro is a VS Code fork (like Cursor).
        // User data dir: ~/Library/Application Support/Kiro/ (macOS) or ~/.config/Kiro/ (Linux)
        // CLI: kiro (symlinked from /Applications/Kiro.app/Contents/Resources/app/bin/code)
        let user_dir = if cfg!(target_os = "linux") {
            format!("{home}/.config/Kiro")
        } else {
            format!("{home}/Library/Application Support/Kiro")
        };

        ToolToml {
            tool: ToolSection {
                name: "kiro".into(),
                version: "0.2.13".into(),
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
        std::env::set_var("KIRO_MODEL", &decision.recommended_model);
        std::env::set_var("LOOM_ROUTING_PROVIDER", &decision.recommended_provider);
        Ok(())
    }

    fn sync_from_native(&self, loom_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(loom_dir)
    }
}
