//! Integration test for cross-tool workflows
//! Tests session sharing, skill portability, and MCP communication between tools.

use loom_core::{Message, SessionManifest};
use loom_skills::SkillEntry;
use loom_mcp::McpServerEntry;
use loom_sessions::{SessionManager, SessionExport};
use std::path::Path;
use tempfile::tempdir;

#[tokio::test]
async fn test_session_sharing_between_tools() -> anyhow::Result<()> {
    // Create temporary directory for agents
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    // Initialize Loom ecosystem
    let session_manager = loom_sessions::SessionManager::new(agents_path);

    // Create a session in jcode
    let jcode_session_id = "jcode-session-1";
    let jcode_manifest = SessionManifest {
        session_id: jcode_session_id.to_string(),
        source_tool: "jcode".to_string(),
        source_version: "1.0.0".to_string(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        topic: "code-generation".to_string(),
        message_count: 0,
        active_skills: vec!["jcode-skill-1".to_string()],
        active_mcp_servers: vec!["jcode-mcp-1".to_string()],
        working_directory: "/tmp/jcode".to_string(),
        model_provider: "jcode".to_string(),
        files_modified: "".to_string(),
        compatibility_note: "".to_string(),
    };

    // Save manifest
    session_manager.store().save_manifest(&jcode_manifest)?;

    // Export the session
    let export = session_manager.export_session(jcode_session_id)?;
    let export_path = agents_path.join("jcode_session_export.json");
    loom_sessions::write_export(&export, &export_path)?;

    // Now import the session into claude-code
    let imported_id = session_manager.import_session_json(&export_path.to_string_lossy())?;
    let imported_manifest = session_manager.store().get(&imported_id).unwrap();

    // Verify session was imported correctly
    assert_eq!(imported_manifest.session_id, jcode_session_id);
    assert_eq!(imported_manifest.source_tool, "claude-code");
    assert_eq!(imported_manifest.active_skills, vec!["jcode-skill-1"]);

    println!("✅ Session successfully shared between jcode and claude-code");
    Ok(())
}

#[tokio::test]
async fn test_skill_portability_between_tools() -> anyhow::Result<()> {
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    // Initialize skills engine
    let skills_engine = loom_skills::SkillsEngine::new(loom_dir)?;

    // Create a skill in jcode
    let skill_id = "test-skill-1";
    let skill_entry = loom_skills::SkillEntry::new(
        skill_id.to_string(),
        "Test Skill".to_string(),
        "This is a test skill content.".to_string(),
        vec!["test".to_string()],
        "1.0.0".to_string(),
    );

    // Save skill to registry
    skills_engine.registry().upsert(&skill_entry)?;

    // Simulate skill being used by claude-code
    // (In a real scenario, claude-code would load the skill from the registry)
    let loaded_skill = skills_engine
        .loader()
        .load_content(skill_id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded_skill, "This is a test skill content.");
    println!("✅ Skill successfully shared between jcode and claude-code");
    Ok(())
}

#[tokio::test]
async fn test_mcp_communication_between_tools() -> anyhow::Result<()> {
    let loom_dir = tempdir()?;
    let agents_path = loom_dir.path().to_str().unwrap();

    // Initialize MCP manager
    let mcp_manager = loom_mcp::McpManager::new(loom_dir)?;

    // Register MCP server from jcode
    let jcode_mcp = loom_mcp::McpServerEntry {
        id: "jcode-mcp".to_string(),
        name: "jCode MCP".to_string(),
        transport: loom_core::McpTransport {
            transport_type: loom_core::McpTransportType::Stdio,
            command: Some("node".to_string()),
            args: vec![],
            url: None,
            env: std::collections::HashMap::new(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: loom_core::HealthStatusStr::Healthy,
        last_health_check: chrono::Utc::now().to_rfc3339(),
        tags: vec![],
        compatible_tools: vec!["claude-code".to_string()],
        required_by_skills: vec![],
    };

    mcp_manager.registry().upsert(&jcode_mcp)?;

    // Verify MCP server is registered
    let fetched = mcp_manager.registry().get("jcode-mcp").unwrap().unwrap();
    assert_eq!(fetched.id, "jcode-mcp");

    println!("✅ MCP server registered and accessible");
    Ok(())
}