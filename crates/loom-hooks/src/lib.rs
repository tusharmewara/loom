//! Loom Hooks Framework
//!
//! Manages tool-specific hooks that can intercept and modify loom behavior.
//! Supports registration, execution, and signing of hooks for security.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A hook that can be registered to intercept events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub id: String,
    pub event_type: String,
    pub script_path: String,
    pub enabled: bool,
    pub priority: HookPriority,
    pub signature: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookPriority {
    Ecosystem,
    Tool,
    User,
}

impl std::fmt::Display for HookPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookPriority::Ecosystem => write!(f, "Ecosystem"),
            HookPriority::Tool => write!(f, "Tool"),
            HookPriority::User => write!(f, "User"),
        }
    }
}

/// Whether `security.require_signature` is enabled for this loom dir
/// (routing-debt.md Step 5). Defaults to false when config loading
/// fails or the key is absent — unsigned hooks keep running, matching
/// the historic trust model.
pub fn require_signature(loom_dir: &str) -> bool {
    loom_core::ConfigReader::new(loom_dir)
        .load()
        .map(|c| c.security.require_signature)
        .unwrap_or(false)
}

/// Registry for hooks
#[derive(Debug, Default)]
pub struct HookRegistry {
    hooks: Arc<RwLock<HashMap<String, Hook>>>,
}

/// Engine for managing hooks
#[derive(Debug)]
pub struct HooksEngine {
    registry: HookRegistry,
    audit_log: HookAuditLog,
    loom_dir: String,
}

impl HooksEngine {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            registry: HookRegistry::new(),
            audit_log: HookAuditLog::new(),
            loom_dir: loom_dir.to_string(),
        }
    }

    pub fn registry(&self) -> &HookRegistry {
        &self.registry
    }

    pub fn audit_log(&self) -> &HookAuditLog {
        &self.audit_log
    }

    pub async fn load_hooks(&self) -> anyhow::Result<()> {
        let hooks_dir = std::path::Path::new(&self.loom_dir).join("hooks");
        self.registry.load_from_dir(&hooks_dir).await
    }

    pub async fn execute_hooks(
        &self,
        event_type: &str,
        context: &HookContext,
    ) -> anyhow::Result<Vec<HookResult>> {
        execute_hooks(
            &self.loom_dir,
            &self.registry,
            event_type,
            context,
            require_signature(&self.loom_dir),
        )
        .await
    }

    pub fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-hooks",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: std::collections::HashMap::new(),
        })
    }
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new hook
    pub async fn register(&self, hook: Hook) -> Result<()> {
        let mut hooks = self.hooks.write().await;
        hooks.insert(hook.id.clone(), hook);
        Ok(())
    }

    /// Get a hook by ID
    pub async fn get(&self, id: &str) -> Option<Hook> {
        let hooks = self.hooks.read().await;
        hooks.get(id).cloned()
    }

    /// List all hooks
    pub async fn list(&self) -> Vec<Hook> {
        let hooks = self.hooks.read().await;
        hooks.values().cloned().collect()
    }

    /// Remove a hook
    pub async fn remove(&self, id: &str) -> Result<()> {
        let mut hooks = self.hooks.write().await;
        hooks.remove(id);
        Ok(())
    }

    /// Load hooks from configuration directory
    pub async fn load_from_dir(&self, hooks_dir: &Path) -> Result<()> {
        if !hooks_dir.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(hooks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let content = fs::read_to_string(&path).context("Failed to read hook config")?;
                let hook: Hook = toml::from_str(&content).context("Failed to parse hook config")?;
                self.register(hook).await?;
            }
        }
        Ok(())
    }
}

/// Execute hooks for a given event
///
/// `require_signature` comes from `security.require_signature` in the
/// loom dir's config (see [`require_signature`]); when true, unsigned
/// hooks are skipped.
pub async fn execute_hooks(
    loom_dir: &str,
    registry: &HookRegistry,
    event_type: &str,
    context: &HookContext,
    require_signature: bool,
) -> Result<Vec<HookResult>> {
    let hooks = registry.list().await;
    let mut results = Vec::new();

    for hook in hooks {
        if hook.enabled && hook.event_type == event_type {
            // Trust model (routing-debt.md Step 5): unsigned hooks are
            // allowed by default; `security.require_signature = true`
            // skips them. Note the live signature scheme is an unkeyed
            // SHA-256 integrity hash — it detects accidental drift, not
            // forgery by someone who can write to the hooks dir.
            if require_signature && hook.signature.is_none() {
                eprintln!(
                    "⚠  skipped unsigned hook '{}' (security.require_signature = true)",
                    hook.id
                );
                continue;
            }
            // Check signature if present
            if let Some(sig) = &hook.signature {
                if !verify_signature(&hook, sig)? {
                    continue;
                }
            }

            // Execute the hook script
            match execute_hook_script(loom_dir, &hook.script_path, context).await {
                Ok((success, output)) => {
                    results.push(HookResult {
                        hook_id: hook.id.clone(),
                        success,
                        output,
                    });
                }
                Err(e) => {
                    results.push(HookResult {
                        hook_id: hook.id.clone(),
                        success: false,
                        output: format!("Error: {}", e),
                    });
                }
            }
        }
    }

    Ok(results)
}

/// Context passed to hook scripts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    pub event_type: String,
    pub payload: serde_json::Value,
    pub tool_name: Option<String>,
}

/// Result of hook execution
#[derive(Debug, Clone)]
pub struct HookResult {
    pub hook_id: String,
    pub success: bool,
    pub output: String,
}

/// Execute a hook script using the HookRunner
async fn execute_hook_script(
    loom_dir: &str,
    script_path: &str,
    context: &HookContext,
) -> Result<(bool, String)> {
    use std::process::Stdio;
    use tokio::process::Command;
    use tokio::time::timeout;

    let loom_dir_path = PathBuf::from(loom_dir);
    let script = Path::new(script_path);
    // Path containment: the resolved script must live inside loom_dir.
    // Rejects `..` traversal and absolute paths outside loom_dir.
    let resolved = if script.is_absolute() {
        script.to_path_buf()
    } else {
        loom_dir_path.join(script_path)
    };
    if !path_is_inside(&resolved, &loom_dir_path) {
        return Ok((
            false,
            format!(
                "hook script path escapes loom_dir: '{script_path}' resolves to '{}' outside '{}'",
                resolved.display(),
                loom_dir_path.display()
            ),
        ));
    }
    if !script.exists() {
        return Ok((
            false,
            format!("hook script not found: {}", script.display()),
        ));
    }

    let mut cmd = Command::new(script);
    cmd.env("LOOM_HOOK_ID", "test")
        .env("LOOM_EVENT", &context.event_type)
        .env(
            "LOOM_TOOL",
            context.tool_name.as_deref().unwrap_or("unknown"),
        )
        .env("LOOM_AGENTS_DIR", loom_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // For PostPhase events, surface task/agent/exit info as flat env vars
    // so simple shell scripts can read them without parsing JSON.
    if context.event_type == "PostPhase" {
        if let Some(tid) = context.payload.get("task_id").and_then(|v| v.as_str()) {
            cmd.env("LOOM_TASK_ID", tid);
        }
        if let Some(p) = context.payload.get("phase").and_then(|v| v.as_str()) {
            cmd.env("LOOM_PHASE", p);
        }
        if let Some(a) = context.payload.get("agent").and_then(|v| v.as_str()) {
            cmd.env("LOOM_AGENT", a);
        }
        if let Some(c) = context.payload.get("exit_code").and_then(|v| v.as_i64()) {
            cmd.env("LOOM_EXIT_CODE", c.to_string());
        }
    }

    let output = timeout(std::time::Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("hook timed out"))??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !stderr.is_empty() {
        return Ok((false, format!("hook stderr: {}", stderr)));
    }

    let success = output.status.success();
    Ok((success, stdout))
}

/// Check that `path` (which may not exist) is contained inside `dir` (which must exist).
/// Handles `..` traversal lexically for non-existent paths; uses canonicalize
/// (symlink-aware) for paths that exist.
fn path_is_inside(path: &Path, dir: &Path) -> bool {
    // Path exists: canonicalize handles symlinks on both sides.
    if let Ok(p) = path.canonicalize() {
        if let Ok(d) = dir.canonicalize() {
            return p.starts_with(&d);
        }
    }
    // Path doesn't exist: normalize `..` lexically, compare against dir as-is.
    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                normalized.push(comp);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(c) => {
                normalized.push(c);
            }
        }
    }
    normalized.starts_with(dir)
}

/// Sign a hook for security verification
pub fn sign_hook(hook: &Hook) -> String {
    let mut hasher = Sha256::new();
    hasher.update(hook.id.as_bytes());
    hasher.update(hook.event_type.as_bytes());
    hasher.update(hook.script_path.as_bytes());
    hasher.update(hook.priority.to_string().as_bytes());

    format!("{:x}", hasher.finalize())
}

/// Verify a hook signature
pub fn verify_hook(hook: &Hook, signature: &str) -> Result<bool> {
    let expected = sign_hook(hook);
    Ok(expected == signature)
}

/// Verify a hook signature
fn verify_signature(hook: &Hook, signature: &str) -> Result<bool> {
    let expected = sign_hook(hook);
    Ok(expected == signature)
}

/// Fire all `PostPhase` hooks. Errors are logged to stderr but never
/// propagated — the workflow chain should keep going even if a hook script
/// fails. The agent process and the workflow loop are unaffected.
pub async fn run_post_phase_hooks(
    loom_dir: &str,
    registry: &HookRegistry,
    task_id: &str,
    phase_name: &str,
    agent: &str,
    exit_code: i32,
    require_signature: bool,
) -> Result<usize> {
    let context = HookContext {
        event_type: "PostPhase".to_string(),
        payload: serde_json::json!({
            "task_id": task_id,
            "phase": phase_name,
            "agent": agent,
            "exit_code": exit_code,
        }),
        tool_name: Some(agent.to_string()),
    };
    let results = execute_hooks(loom_dir, registry, "PostPhase", &context, require_signature).await?;
    Ok(results.len())
}

/// Audit log for hook executions
#[derive(Debug, Default)]
pub struct HookAuditLog {
    entries: Arc<RwLock<Vec<HookAuditEntry>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookAuditEntry {
    pub timestamp: DateTime<Utc>,
    pub hook_id: String,
    pub event_type: String,
    pub success: bool,
    pub output: String,
}

impl HookAuditLog {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn log(&self, entry: HookAuditEntry) {
        let mut entries = self.entries.write().await;
        entries.push(entry);
    }

    pub async fn save_to_file(&self, log_path: &Path) -> Result<()> {
        let entries = self.entries.read().await;
        let content = serde_json::to_string_pretty(&*entries)?;
        fs::write(log_path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hook_registration() {
        let registry = HookRegistry::new();
        let hook = Hook {
            id: "test-hook".to_string(),
            event_type: "skill.added".to_string(),
            script_path: "/path/to/script.sh".to_string(),
            enabled: true,
            priority: HookPriority::Ecosystem,
            signature: None,
            created_at: "2026-05-17T16:00:00Z".to_string(),
        };

        registry.register(hook.clone()).await.unwrap();
        let retrieved = registry.get("test-hook").await.unwrap();
        assert_eq!(retrieved.id, hook.id);
    }

    #[tokio::test]
    async fn test_hook_execution() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_AGENTS_DIR", tmp.path());
        let script = tmp.path().join("echo.sh");
        std::fs::write(&script, "#!/bin/sh\necho hello\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let registry = HookRegistry::new();
        let hook = Hook {
            id: "test-hook".to_string(),
            event_type: "skill.added".to_string(),
            script_path: script.to_string_lossy().to_string(),
            enabled: true,
            priority: HookPriority::Ecosystem,
            signature: None,
            created_at: "2026-05-17T16:00:00Z".to_string(),
        };

        registry.register(hook).await.unwrap();

        let context = HookContext {
            event_type: "skill.added".to_string(),
            payload: serde_json::json!({"skill_id": "test"}),
            tool_name: Some("jcode".to_string()),
        };

        let results = execute_hooks(
            tmp.path().to_str().unwrap(),
            &registry,
            "skill.added",
            &context,
            false,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].output.contains("hello"));
    }

    #[tokio::test]
    async fn test_unsigned_hook_skipped_when_required() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("LOOM_AGENTS_DIR", tmp.path());
        let script = tmp.path().join("echo.sh");
        std::fs::write(&script, "#!/bin/sh\necho hello\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script, perms).unwrap();
        }

        let registry = HookRegistry::new();
        registry
            .register(Hook {
                id: "unsigned".to_string(),
                event_type: "skill.added".to_string(),
                script_path: script.to_string_lossy().to_string(),
                enabled: true,
                priority: HookPriority::Ecosystem,
                signature: None,
                created_at: "2026-05-17T16:00:00Z".to_string(),
            })
            .await
            .unwrap();

        let context = HookContext {
            event_type: "skill.added".to_string(),
            payload: serde_json::json!({"skill_id": "test"}),
            tool_name: Some("jcode".to_string()),
        };

        // Historic default: unsigned hooks run.
        let results = execute_hooks(
            tmp.path().to_str().unwrap(),
            &registry,
            "skill.added",
            &context,
            false,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);

        // security.require_signature = true: unsigned hooks are skipped.
        let results = execute_hooks(
            tmp.path().to_str().unwrap(),
            &registry,
            "skill.added",
            &context,
            true,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_hook_signing() {
        let hook = Hook {
            id: "test-hook".to_string(),
            event_type: "skill.added".to_string(),
            script_path: "/path/to/script.sh".to_string(),
            enabled: true,
            priority: HookPriority::Ecosystem,
            signature: None,
            created_at: "2026-05-17T16:00:00Z".to_string(),
        };

        let signature = sign_hook(&hook);
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_path_is_inside_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let loom_dir = tmp.path().to_path_buf();

        // Inside → OK.
        assert!(path_is_inside(&loom_dir.join("scripts/hook.sh"), &loom_dir));
        // `..` traversal → rejected.
        assert!(!path_is_inside(
            &loom_dir.join("../../../etc/passwd"),
            &loom_dir
        ));
        // Mixed traversal → rejected.
        assert!(!path_is_inside(
            &loom_dir.join("scripts/../../../etc/passwd"),
            &loom_dir
        ));
        // Absolute external path → rejected.
        assert!(!path_is_inside(&PathBuf::from("/etc/passwd"), &loom_dir));
    }
}
