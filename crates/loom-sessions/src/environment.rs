use loom_core::SessionManifest;
use std::collections::HashMap;

/// Reconstruct environment variables/tools would need to resume a session.
pub fn reconstruct_environment(manifest: &SessionManifest) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("LOOM_SESSION_ID".into(), manifest.session_id.clone());
    env.insert("LOOM_SOURCE_TOOL".into(), manifest.source_tool.clone());
    env.insert("LOOM_WORKING_DIRECTORY".into(), manifest.working_directory.clone());
    env.insert("LOOM_MODEL_PROVIDER".into(), manifest.model_provider.clone());

    if !manifest.active_skills.is_empty() {
        env.insert(
            "LOOM_ACTIVE_SKILLS".into(),
            manifest.active_skills.join(","),
        );
    }
    if !manifest.active_mcp_servers.is_empty() {
        env.insert(
            "LOOM_ACTIVE_MCP".into(),
            manifest.active_mcp_servers.join(","),
        );
    }

    env
}
