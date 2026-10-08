use crate::cli::HooksAction;
use loom_core::HookEventType;
use loom_hooks::{verify_hook, HooksEngine};
use std::path::Path;

fn parse_event(s: &str) -> anyhow::Result<HookEventType> {
    match s.to_lowercase().replace('_', "-").as_str() {
        "pre-tool-use" | "pretooluse" => Ok(HookEventType::PreToolUse),
        "post-tool-use" | "posttooluse" => Ok(HookEventType::PostToolUse),
        "session-start" | "sessionstart" => Ok(HookEventType::SessionStart),
        other => anyhow::bail!("unknown event type: {other}"),
    }
}

pub async fn run(loom_dir: &Path, action: &HooksAction) -> anyhow::Result<()> {
    let engine = HooksEngine::new(loom_dir.to_str().unwrap());

    match action {
        HooksAction::List { json } => {
            let hooks = engine.registry().list().await;
            if *json {
                let hook_entries: Vec<_> = hooks.iter().map(|h| {
                    serde_json::json!({
                        "id": h.id,
                        "event_type": format!("{:?}", h.event_type),
                        "script_path": h.script_path,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "hooks.list",
                    "count": hook_entries.len(),
                    "hooks": hook_entries
                }))?);
            } else {
                for hook in &hooks {
                    println!("{}  {:?}  {}", hook.id, hook.event_type, hook.script_path);
                }
                if hooks.is_empty() {
                    println!("No hooks registered.");
                }
            }
        }
        HooksAction::Show { id, json } => {
            let hook = engine
                .registry()
                .get(id)
                .await
                .ok_or_else(|| anyhow::anyhow!("Hook '{id}' not found"))?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "hooks.show",
                    "hook": hook
                }))?);
            } else {
                println!("{}", toml::to_string_pretty(&hook)?);
            }
        }
        HooksAction::Register { path } => {
            let src = Path::new(path);
            if !src.exists() {
                anyhow::bail!("Path does not exist: {path}");
            }
            let dst = loom_dir.join("hooks").join(
                src.file_name()
                    .ok_or_else(|| anyhow::anyhow!("invalid path"))?,
            );
            std::fs::create_dir_all(loom_dir.join("hooks"))?;
            std::fs::copy(src, &dst)?;
            println!("Registered hook from '{path}'");
        }
        HooksAction::Remove { id } => {
            let reg = loom_hooks::HookRegistry::new();
            reg.remove(id).await?;
            println!("Removed hook '{id}'");
        }
        HooksAction::Run {
            event,
            tool,
            matcher,
        } => {
            let ctx = loom_hooks::HookContext {
                event_type: parse_event(event)?.to_string(),
                payload: serde_json::json!({ "tool": tool, "matcher": matcher }),
                tool_name: Some(tool.clone()),
            };
            let results = loom_hooks::execute_hooks(engine.registry(), event, &ctx).await?;
            for r in results {
                println!("{} success={} output={}", r.hook_id, r.success, r.output);
            }
        }
        HooksAction::Verify => {
            let mut ok = 0;
            let mut fail = 0;
            let hooks = engine.registry().list().await;
            for hook in hooks {
                let sig = hook.signature.clone().unwrap_or_default();
                match verify_hook(&hook, &sig) {
                    Ok(true) => {
                        println!("✓ {}", hook.id);
                        ok += 1;
                    }
                    Ok(false) => {
                        println!("✗ {} (invalid signature)", hook.id);
                        fail += 1;
                    }
                    Err(e) => {
                        println!("✗ {} ({e})", hook.id);
                        fail += 1;
                    }
                }
            }
            if fail > 0 {
                anyhow::bail!("{fail} hook(s) failed verification ({ok} ok)");
            }
        }
    }
    Ok(())
}
