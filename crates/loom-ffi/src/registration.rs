//! Registration logic behind the FFI exports.
//!
//! Everything here takes the loom dir explicitly (the export layer resolves it
//! via `loom_core::loom_dir()`, which honors `LOOM_AGENTS_DIR`) and returns a
//! plain `{"ok":true,...}` / `{"ok":false,"error":...}` JSON value. Errors and
//! panics are converted by the FFI wrappers in [`crate`], never propagated.

use loom_generic_adapter::config::{LaunchSection, ToolSection, ToolToml};
use loom_skills::install::{is_safe_name, register_skill};
use serde_json::{json, Value};

/// Every error becomes `{ok:false,error}` so nothing but a panic can escape.
use std::collections::HashMap;
use std::path::Path;

/// Extract a JSON string field, typing mismatches as errors.
fn json_str(payload: &Value, field: &str, required: bool) -> anyhow::Result<Option<String>> {
    match payload.get(field) {
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Null) | None if !required => Ok(None),
        Some(_) => anyhow::bail!("'{field}' must be a string"),
        None => anyhow::bail!("missing required field '{field}'"),
    }
}

/// Register a tool from a JSON payload — the same TOML `loom thread` writes.
///
/// Payload: `{"name", "version"?, "command"?, "args"?: [string...], "env"?: {k:v}}`.
/// Keys other than these are ignored; the tool TOML schema (`loom-generic-adapter`
/// `ToolToml`) has no slot for a `description` yet. Unknown tool names take
/// `loom thread`'s fallback shape; there is deliberately no preset-adapter
/// detection here — foreign callers describe the tool, they don't import one.
pub fn register_tool(loom_dir: &Path, payload: &str) -> Value {
    register_tool_inner(loom_dir, payload).unwrap_or_else(|e| json!({ "ok": false, "error": format!("{e:#}") }))
}

fn register_tool_inner(loom_dir: &Path, payload: &str) -> anyhow::Result<Value> {
    let payload: Value =
        serde_json::from_str(payload).map_err(|e| anyhow::anyhow!("invalid JSON payload: {e}"))?;

    let name = json_str(&payload, "name", true)?.expect("required checked");
    if !is_safe_name(&name) {
        anyhow::bail!("invalid tool name '{name}' (must be a single path segment)");
    }
    let version = json_str(&payload, "version", false)?.unwrap_or_else(|| "1.0.0".into());

    let launch = match payload.get("command") {
        Some(Value::String(command)) => {
            let args = match payload.get("args") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|i| {
                        i.as_str()
                            .ok_or_else(|| anyhow::anyhow!("'args' must be an array of strings"))
                            .map(String::from)
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?,
                Some(_) => anyhow::bail!("'args' must be an array of strings"),
            };
            let env = match payload.get("env") {
                None | Some(Value::Null) => HashMap::new(),
                Some(Value::Object(map)) => map
                    .iter()
                    .map(|(k, v)| {
                        v.as_str()
                            .ok_or_else(|| anyhow::anyhow!("'env' values must be strings"))
                            .map(String::from)
                            .map(|v| (k.clone(), v))
                    })
                    .collect::<anyhow::Result<HashMap<_, _>>>()?,
                Some(_) => anyhow::bail!("'env' must be an object of strings"),
            };
            Some(LaunchSection {
                runtime: "process".to_string(),
                image: None,
                command: command.clone(),
                args,
                env,
                working_dir: None,
                network: None,
                auto_remove: true,
            })
        }
        Some(_) => anyhow::bail!("'command' must be a string"),
        None => None,
    };

    // `loom thread` treats a re-register as "Updated" (overwrite, flagged).
    let updated = loom_dir.join("tools").join(format!("{name}.toml")).exists();
    let toml_path = ToolToml {
        tool: ToolSection {
            name: name.clone(),
            version,
        },
        skills: None,
        mcp: None,
        auth: None,
        hooks: None,
        routing: None,
        sessions: None,
        lsp: None,
        debug: None,
        launch,
    }
    .write(loom_dir)?;

    Ok(json!({
        "ok": true,
        "toml_path": toml_path.to_string_lossy(),
        "updated": updated,
    }))
}

/// Register a skill from `{"name"?, "content"}` — the full `loom skills install`
/// flow (files, registry, lock file) applied to a temp file.
pub fn register_skill_from_json(loom_dir: &Path, payload: &str) -> Value {
    register_skill_inner(loom_dir, payload)
        .unwrap_or_else(|e| json!({ "ok": false, "error": format!("{e:#}") }))
}

fn register_skill_inner(loom_dir: &Path, payload: &str) -> anyhow::Result<Value> {
    let payload: Value =
        serde_json::from_str(payload).map_err(|e| anyhow::anyhow!("invalid JSON payload: {e}"))?;
    let content = json_str(&payload, "content", true)?.expect("required checked");
    let name = json_str(&payload, "name", false)?;
    if let Some(name) = name.as_deref() {
        if !is_safe_name(name) {
            anyhow::bail!("invalid skill name '{name}' (must be a single path segment)");
        }
    }

    // The stem is the fallback name; frontmatter `name:` still wins like the CLI.
    let tmp = tempfile::tempdir()?;
    let src = tmp.path().join(format!("{}.md", name.as_deref().unwrap_or("skill")));
    std::fs::write(&src, content)?;

    let name = register_skill(loom_dir, &src)?;
    Ok(json!({ "ok": true, "name": name }))
}

/// Remove `{loom_dir}/tools/{name}.toml` — same removal the plugin manager
/// performs for its tool TOMLs; absent file is an error.
pub fn unregister_tool(loom_dir: &Path, name: &str) -> Value {
    unregister_tool_inner(loom_dir, name)
        .unwrap_or_else(|e| json!({ "ok": false, "error": format!("{e:#}") }))
}

fn unregister_tool_inner(loom_dir: &Path, name: &str) -> anyhow::Result<Value> {
    if !is_safe_name(name) {
        anyhow::bail!("invalid tool name '{name}' (must be a single path segment)");
    }
    let path = loom_dir.join("tools").join(format!("{name}.toml"));
    if !path.exists() {
        anyhow::bail!("Tool '{name}' not found at {}", path.display());
    }
    std::fs::remove_file(&path)?;
    Ok(json!({ "ok": true, "removed": path.to_string_lossy() }))
}
