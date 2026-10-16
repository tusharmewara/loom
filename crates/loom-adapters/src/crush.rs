use crate::{LoomToolAdapter, SyncReport};
use anyhow::Result;
use loom_core::RoutingDecision;
use loom_generic_adapter::config::{
    HooksSection, McpSection, RoutingSection, SkillsSection, ToolSection, ToolToml,
};

pub struct CrushAdapter;

impl LoomToolAdapter for CrushAdapter {
    fn tool_name(&self) -> &'static str {
        "crush"
    }

    fn default_toml(&self, home: &str) -> ToolToml {
        ToolToml {
            tool: ToolSection {
                name: "crush".into(),
                version: "1.0.0".into(),
            },
            skills: Some(SkillsSection {
                native_dir: Some(format!("{home}/.crush/skills")),
                import_on_register: true,
                shared_dir: None,
                extension: "md".into(),
            }),
            mcp: Some(McpSection {
                config_path: Some(format!("{home}/.crush/mcp.json")),
                shared_servers: vec!["filesystem".into(), "github".into()],
                import_on_register: true,
                import_tags: vec![],
            }),
            auth: None,
            hooks: Some(HooksSection {
                scripts_dir: Some(format!("{home}/.crush/hooks")),
                register_on_start: true,
                default_timeout_seconds: 30,
            }),
            routing: Some(RoutingSection {
                default_tier: "mid".into(),
                catalog_path: None,
                strategy: "cost_optimize".into(),
            }),
            sessions: None,
            lsp: None,
            debug: None,
            launch: None,
        }
    }

    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()> {
        // Crush HTTP API: POST /config/model (plan 4.5c) — best-effort via curl if CRUSH_API_URL set
        if let Ok(base) = std::env::var("CRUSH_API_URL") {
            let body = serde_json::json!({
                "model": decision.recommended_model,
                "provider": decision.recommended_provider,
            });
            let _ = std::process::Command::new("curl")
                .args([
                    "-sf",
                    "-X",
                    "POST",
                    &format!("{base}/config/model"),
                    "-H",
                    "Content-Type: application/json",
                    "-d",
                    &body.to_string(),
                ])
                .status();
            return Ok(());
        }
        std::env::set_var("CRUSH_MODEL", &decision.recommended_model);
        Ok(())
    }

    fn sync_from_native(&self, loom_dir: &str, _home: &str) -> Result<SyncReport> {
        crate::sync_tools_dir(loom_dir)
    }
}
