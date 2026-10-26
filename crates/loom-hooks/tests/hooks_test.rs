use loom_core::HookEventType;
use loom_hooks::{sign_hook, verify_hook, Hook, HookContext, HookPriority};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

fn sample_hook(id: &str, script: &str) -> Hook {
    Hook {
        id: id.into(),
        event_type: HookEventType::PreToolUse.to_string(),
        script_path: script.into(),
        enabled: true,
        priority: HookPriority::Ecosystem,
        signature: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[tokio::test]
async fn test_hook_runner_executes_script() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let hooks_dir = dir.path().join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    let script = dir.path().join("allow.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();

    let hook = sample_hook("allow", script.to_str().unwrap());
    fs::write(
        hooks_dir.join("allow.toml"),
        toml::to_string(&hook).unwrap(),
    )
    .unwrap();

    let engine = loom_hooks::HooksEngine::new(agents);
    engine.load_hooks().await.unwrap();
    let registry = engine.registry();
    let ctx = HookContext {
        event_type: HookEventType::PreToolUse.to_string(),
        payload: serde_json::json!({"tool": "bash"}),
        tool_name: Some("bash".into()),
    };
    let results = loom_hooks::execute_hooks(
        registry,
        HookEventType::PreToolUse.to_string().as_str(),
        &ctx,
        false,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].success);
}

#[tokio::test]
async fn test_hook_blocks_on_nonzero_exit() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let hooks_dir = dir.path().join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    let script = dir.path().join("deny.sh");
    fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();

    let hook = sample_hook("deny", "deny.sh");
    fs::write(hooks_dir.join("deny.toml"), toml::to_string(&hook).unwrap()).unwrap();

    let engine = loom_hooks::HooksEngine::new(agents);
    engine.load_hooks().await.unwrap();
    let registry = engine.registry();
    let ctx = HookContext {
        event_type: HookEventType::PreToolUse.to_string(),
        payload: serde_json::json!({"tool": "bash"}),
        tool_name: Some("bash".into()),
    };
    let results = loom_hooks::execute_hooks(
        registry,
        HookEventType::PreToolUse.to_string().as_str(),
        &ctx,
        false,
    )
    .await
    .unwrap();
    assert!(!results[0].success);
}

#[tokio::test]
async fn test_engine_skips_unsigned_hooks_when_required() {
    // Full plumbing: hooks dir with an unsigned hook + config.toml with
    // [security] require_signature = true → the engine must skip it.
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let hooks_dir = dir.path().join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();

    let script = dir.path().join("noop.sh");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();

    let hook = sample_hook("noop", script.to_str().unwrap());
    fs::write(hooks_dir.join("noop.toml"), toml::to_string(&hook).unwrap()).unwrap();

    fs::write(
        dir.path().join("config.toml"),
        "[security]\nrequire_signature = true\n",
    )
    .unwrap();

    let engine = loom_hooks::HooksEngine::new(agents);
    engine.load_hooks().await.unwrap();
    let ctx = HookContext {
        event_type: HookEventType::PreToolUse.to_string(),
        payload: serde_json::json!({"tool": "bash"}),
        tool_name: Some("bash".into()),
    };
    let results = engine
        .execute_hooks(HookEventType::PreToolUse.to_string().as_str(), &ctx)
        .await
        .unwrap();
    assert_eq!(results.len(), 0, "unsigned hook must be skipped");

    // Explicit opt-out config (re-read per execution) → hook runs again.
    // (Removing the config would now default the flag on — ADR 0005.)
    fs::write(
        dir.path().join("config.toml"),
        "[security]\nrequire_signature = false\n",
    )
    .unwrap();
    let results = engine
        .execute_hooks(HookEventType::PreToolUse.to_string().as_str(), &ctx)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
}

#[test]
fn test_sign_and_verify_hook() {
    let hook = sample_hook("signed", "x.sh");
    let signature = sign_hook(&hook);
    assert!(verify_hook(&hook, &signature).unwrap());
}
