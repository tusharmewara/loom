use crate::registry::HookRegistry;
use crate::signing::verify_hook;
use anyhow::{bail, Result};
use loom_core::{HookEntry, HookEventType};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Debug, Clone)]
pub struct HookContext {
    pub event: HookEventType,
    pub tool_name: String,
    pub matcher: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HookRunResult {
    pub hook_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub blocked: bool,
}

pub struct HookRunner {
    agents_dir: PathBuf,
    registry: HookRegistry,
}

impl HookRunner {
    pub fn new(agents_dir: &str) -> Self {
        Self {
            agents_dir: PathBuf::from(agents_dir),
            registry: HookRegistry::new(agents_dir),
        }
    }

    pub fn registry(&self) -> &HookRegistry {
        &self.registry
    }

    pub fn reload(&mut self) {
        self.registry = HookRegistry::new(self.agents_dir.to_str().unwrap());
    }

    /// Hooks matching event, sorted ecosystem-first then by id.
    pub fn hooks_for(&self, ctx: &HookContext) -> Vec<HookEntry> {
        let mut hooks: Vec<HookEntry> = self
            .registry
            .list()
            .into_iter()
            .filter(|h| h.event_type == ctx.event)
            .filter(|h| matcher_matches(h, ctx))
            .collect();

        hooks.sort_by(|a, b| {
            let pa = priority_rank(&a.priority);
            let pb = priority_rank(&b.priority);
            pa.cmp(&pb).then_with(|| a.id.cmp(&b.id))
        });
        hooks
    }

    /// Run all matching hooks in parallel. PreToolUse: any non-zero exit blocks.
    pub async fn run_parallel(&self, ctx: &HookContext) -> Result<Vec<HookRunResult>> {
        let hooks = self.hooks_for(ctx);
        if hooks.is_empty() {
            return Ok(vec![]);
        }

        let mut handles = Vec::new();
        for hook in hooks {
            let agents_dir = self.agents_dir.clone();
            let ctx = ctx.clone();
            handles.push(tokio::spawn(async move {
                run_one_hook(&agents_dir, &hook, &ctx).await
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            results.push(handle.await??);
        }

        if matches!(ctx.event, HookEventType::PreToolUse) {
            for r in &results {
                if r.exit_code != 0 {
                    bail!(
                        "hook '{}' blocked action (exit {})",
                        r.hook_id,
                        r.exit_code
                    );
                }
            }
        }

        Ok(results)
    }
}

fn priority_rank(p: &loom_core::HookPriority) -> u8 {
    match p {
        loom_core::HookPriority::Ecosystem => 0,
        loom_core::HookPriority::Native => 1,
    }
}

fn matcher_matches(hook: &HookEntry, ctx: &HookContext) -> bool {
    match (&hook.matcher, &ctx.matcher) {
        (None, _) => true,
        (Some(pattern), Some(value)) => value.contains(pattern) || pattern == "*",
        (Some(_), None) => true,
    }
}

async fn run_one_hook(
    agents_dir: &Path,
    hook: &HookEntry,
    ctx: &HookContext,
) -> Result<HookRunResult> {
    if hook.signature.is_some() && !verify_hook(hook)? {
        bail!("hook '{}' failed signature verification", hook.id);
    }

    let script = resolve_script_path(agents_dir, &hook.script_path)?;
    if !script.exists() {
        bail!("hook script not found: {}", script.display());
    }

    let timeout_dur = Duration::from_secs(hook.timeout_seconds.max(1));

    let mut cmd = Command::new(&script);
    cmd.env("LOOM_HOOK_ID", &hook.id)
        .env("LOOM_EVENT", format!("{:?}", hook.event_type))
        .env("LOOM_TOOL", &ctx.tool_name)
        .env("LOOM_AGENTS_DIR", agents_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(m) = &ctx.matcher {
        cmd.env("LOOM_MATCHER", m);
    }

    let output = timeout(timeout_dur, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("hook '{}' timed out after {}s", hook.id, hook.timeout_seconds))??;

    let exit_code = output.status.code().unwrap_or(-1);
    Ok(HookRunResult {
        hook_id: hook.id.clone(),
        exit_code,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        blocked: exit_code != 0,
    })
}

fn resolve_script_path(agents_dir: &Path, script_path: &str) -> Result<PathBuf> {
    let p = Path::new(script_path);
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    Ok(agents_dir.join(script_path))
}
