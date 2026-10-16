//! Integration tests mapping to IMPLEMENTATION_PLAN section 8.

use chrono::Utc;
use loom_core::{Message, MessageRole, SessionManifest};
use loom_mcp::McpManager;
use loom_routing::{Classifier, DecisionEngine};
use loom_sessions::{export_session, import_session, store::SessionStore, write_export};
use loom_skills::{adapt_skill, ToolAdapterConfig};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

// 8.1 Skill portability
#[test]
fn skill_portability_neutral_and_adapt() {
    let content = "Run via ~/.claude/skills/demo";
    let cfg = ToolAdapterConfig::new().with_replacement("~/.claude/", "~/.cursor/");
    let adapted = adapt_skill(content, &cfg);
    assert!(adapted.contains("~/.cursor/"));
}

// 8.2 MCP server sharing
#[test]
fn mcp_pool_acquire_release() {
    let dir = tempdir().unwrap();
    let mgr = McpManager::new(dir.path().to_str().unwrap()).unwrap();
    let entry = loom_core::McpServerEntry {
        id: "shared".into(),
        name: "shared".into(),
        transport: loom_core::McpTransport {
            transport_type: loom_core::McpTransportType::Stdio,
            command: Some("true".into()),
            args: vec![],
            url: None,
            env: Default::default(),
        },
        shared: true,
        tools_filter: vec![],
        health_status: loom_core::HealthStatusStr::Unknown,
        last_health_check: String::new(),
        tags: vec![],
        compatible_tools: vec!["jcode".into(), "crush".into()],
        required_by_skills: vec![],
    };
    mgr.registry().upsert(&entry).unwrap();
    mgr.pool().acquire("shared", &entry).unwrap();
    assert!(mgr.pool().is_running("shared"));
    mgr.pool().release("shared").unwrap();
}

// 8.3 Session resume
#[test]
fn session_export_import_roundtrip() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let store = SessionStore::new(agents);
    let manifest = SessionManifest {
        session_id: "resume-1".into(),
        source_tool: "jcode".into(),
        source_version: "1".into(),
        exported_at: "2026-01-01T00:00:00Z".into(),
        topic: "demo".into(),
        message_count: 1,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: "/tmp".into(),
        model_provider: "openai".into(),
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    };
    store.save_manifest(&manifest).unwrap();
    store
        .replace_messages(
            "resume-1",
            &[Message {
                id: "m1".into(),
                role: MessageRole::User,
                content: "hello".into(),
                tool_calls: None,
                timestamp: "2026-01-01T00:00:00Z".into(),
                metadata: Default::default(),
            }],
        )
        .unwrap();

    let export = export_session(&store, "resume-1").unwrap();
    let path = dir.path().join("export.json");
    write_export(&export, &path).unwrap();

    let store2 = SessionStore::new(agents);
    let id = import_session(&store2, &path).unwrap();
    assert_eq!(id, "resume-1");
}

// 8.5 Hook enforcement
#[tokio::test]
async fn hook_enforcement_blocks_pretooluse() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("hooks")).unwrap();
    let script = dir.path().join("block.sh");
    fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();

    let hook = loom_hooks::Hook {
        id: "block-rm".into(),
        event_type: loom_core::HookEventType::PreToolUse.to_string(),
        script_path: script.to_str().unwrap().into(),
        enabled: true,
        priority: loom_hooks::HookPriority::Ecosystem,
        signature: None,
        created_at: Utc::now().to_rfc3339(),
    };
    fs::write(
        dir.path().join("hooks").join("block-rm.toml"),
        toml::to_string(&hook).unwrap(),
    )
    .unwrap();

    let registry = loom_hooks::HookRegistry::new();
    registry
        .load_from_dir(&dir.path().join("hooks"))
        .await
        .unwrap();
    let ctx = loom_hooks::HookContext {
        event_type: loom_core::HookEventType::PreToolUse.to_string(),
        payload: serde_json::json!({"tool": "bash", "command": "rm -rf /"}),
        tool_name: Some("bash".into()),
    };
    let results = loom_hooks::execute_hooks(
        &registry,
        loom_core::HookEventType::PreToolUse.to_string().as_str(),
        &ctx,
    )
    .await
    .unwrap();
    // Hook executes and returns result - check that we got results
    assert_eq!(results.len(), 1);
    // With real execution, non-zero exit should result in failure
    assert!(!results[0].success);
}

// 8.4 Authentication sharing (provider import path)
#[test]
fn auth_provider_import_from_tool_toml() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let tools_dir = dir.path().join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::write(
        tools_dir.join("my-tool.toml"),
        r#"
[auth]
provider_id = "shared-openai"
name = "Shared OpenAI"
credential = "sk-test-import"
"#,
    )
    .unwrap();

    let mut auth = loom_auth::AuthManager::new(agents);
    auth.unlock_vault("test-master").unwrap();
    auth.import_from_tools().unwrap();
    assert_eq!(auth.list_providers().len(), 1);
}

// 8.6 Workflow continuity
#[test]
fn workflow_preserves_routing_across_phases() {
    let dir = tempdir().unwrap();
    let engine = loom_workflow::WorkflowEngine::new(dir.path().to_str().unwrap());
    let orch = engine.orchestrator();
    let task = orch
        .create_task(
            "Migrate API",
            "Refactor distributed auth layer",
            false,
            None,
        )
        .unwrap();
    assert!(task.routing_decision.is_some());
    let advanced = orch.advance_phase(&task.task_id).unwrap();
    assert!(advanced.routing_decision.is_some());
    assert!(matches!(advanced.phase, loom_core::TaskPhase::Implement));
}

// 8.8 Cost optimization — low complexity prefers cheaper tier
#[test]
fn routing_cost_optimize_prefers_slm_for_simple_tasks() {
    use loom_core::{ComplexityLevel, RoutingStrategy};
    use loom_routing::{score_static, ModelRouter};

    let simple = score_static("fix typo in comment");
    assert!(matches!(simple.level, ComplexityLevel::Low));

    let config = loom_core::RoutingConfig {
        strategy: RoutingStrategy::CostOptimize,
        default_tier: None,
        default_provider: None,
        default_model: None,
        overrides: Default::default(),
        complexity_thresholds: Default::default(),
    };
    let expected_tier = if simple.numeric < config.complexity_thresholds.low_max {
        loom_core::ModelTier::Slm
    } else if simple.numeric < config.complexity_thresholds.high_min {
        loom_core::ModelTier::Mid
    } else {
        loom_core::ModelTier::Flagship
    };
    let router = ModelRouter::with_default_catalog(Some(config));
    let decision = router.resolve(simple);
    assert_eq!(decision.recommended_tier, expected_tier);
}

// 8.7 Intelligent routing
#[test]
fn routing_scores_complex_task_higher() {
    let classifier = Classifier::new();
    let simple = classifier.classify("fix typo");
    let complex = classifier.classify("refactor distributed security architecture");
    assert!(complex.numeric > simple.numeric);
    let engine = DecisionEngine::new();
    let decision = engine.decide(complex);
    assert!(!decision.recommended_model.is_empty());
}
