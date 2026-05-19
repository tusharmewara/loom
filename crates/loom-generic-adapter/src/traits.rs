//! Conditional trait implementations for the GenericAdapter.
//!
//! Each tool TOML section maps to a Loom trait. This module provides:
//!
//! - `ImplSwitch<T>` — wraps an implementation or acts as a no-op.
//! - Concrete impls for each trait that delegate to TOML-derived config.

#![allow(dead_code, unused)]

use loom_core::{
    AuthProviderTrait, EncryptedCredential, HookEntry, HookExecutor, McpConsumer,
    McpServerEntry, ModelEntry, ModelTier, ProviderEntry, RoutingTarget, SessionExportable,
    SessionManifest, SkillEntry, SkillProvider,
};
use std::path::PathBuf;

use crate::config::ToolToml;

/// Wraps a concrete implementation or provides a no-op fallback.
///
/// This is the core of the GenericAdapter pattern: check `is_present()` to
/// determine if the TOML had a matching section before relying on an impl.
#[derive(Debug)]
pub struct ImplSwitch<T: Send + Sync> {
    inner: Option<T>,
}

impl<T: Send + Sync> ImplSwitch<T> {
    /// Create a new switch with a concrete implementation.
    pub fn some(inner: T) -> Self {
        Self { inner: Some(inner) }
    }

    /// Create a new switch with no implementation (no-op).
    pub fn none() -> Self {
        Self { inner: None }
    }

    /// Whether a real implementation is present.
    pub fn is_present(&self) -> bool {
        self.inner.is_some()
    }

    /// Access the inner implementation, if present.
    pub fn as_ref(&self) -> Option<&T> {
        self.inner.as_ref()
    }
}

// ─── SkillProvider ───────────────────────────────────────────────────

/// Generic SkillProvider backed by a tool's native skills directory.
pub struct GenericSkillProvider {
    pub native_dir: PathBuf,
}

impl SkillProvider for GenericSkillProvider {
    fn discover_skills(&self) -> anyhow::Result<Vec<SkillEntry>> {
        if !self.native_dir.is_dir() {
            return Ok(vec![]);
        }
        let mut skills = Vec::new();
        for entry in std::fs::read_dir(&self.native_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let content = std::fs::read_to_string(&path)?;
                let hash = {
                    use sha2::{Digest, Sha256};
                    let mut h = Sha256::new();
                    h.update(content.as_bytes());
                    format!("{:x}", h.finalize())
                };
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                skills.push(SkillEntry {
                    name: stem,
                    description: format!("Skill from {}", self.native_dir.display()),
                    path: path.to_string_lossy().to_string(),
                    content_hash: hash,
                    tags: vec![],
                    ecosystem_compatible: vec![],
                    ecosystem_requires: vec![],
                    version: "1.0.0".to_string(),
                    last_modified: chrono_now(),
                });
            }
        }
        Ok(skills)
    }

    fn read_skill_content(&self, name: &str) -> anyhow::Result<String> {
        let path = self.native_dir.join(format!("{}.md", name));
        Ok(std::fs::read_to_string(&path)?)
    }
}

fn chrono_now() -> String {
    // Simple UTC timestamp without pulling in chrono
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let micros = dur.subsec_micros();
    // ISO-8601-like
    format!("2025-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        (secs / 86400 / 30) % 12 + 1, // approximate month
        (secs / 86400) % 30 + 1,       // approximate day
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60,
        micros,
    )
}

// ─── McpConsumer ─────────────────────────────────────────────────────

/// Generic McpConsumer backed by a tool's MCP config.
pub struct GenericMcpConsumer;

impl McpConsumer for GenericMcpConsumer {
    fn add_mcp_server(&self, _entry: &McpServerEntry) -> anyhow::Result<()> {
        // In a full implementation, this would write to the tool's MCP config.
        // For now, a no-op that logs.
        tracing::debug!("GenericMcpConsumer: add_mcp_server (no-op)");
        Ok(())
    }

    fn remove_mcp_server(&self, id: &str) -> anyhow::Result<()> {
        tracing::debug!("GenericMcpConsumer: remove_mcp_server {} (no-op)", id);
        Ok(())
    }
}

// ─── AuthProviderTrait ───────────────────────────────────────────────

/// Generic AuthProviderTrait.
pub struct GenericAuthProvider;

impl AuthProviderTrait for GenericAuthProvider {
    fn discover_providers(&self) -> anyhow::Result<Vec<ProviderEntry>> {
        Ok(vec![])
    }

    fn export_credential(&self, _provider_id: &str) -> anyhow::Result<EncryptedCredential> {
        Err(anyhow::anyhow!("GenericAdapter: no real credential store configured"))
    }
}

// ─── HookExecutor ────────────────────────────────────────────────────

/// Generic HookExecutor.
pub struct GenericHookExecutor;

impl HookExecutor for GenericHookExecutor {
    fn execute_hook(&self, _entry: &HookEntry) -> anyhow::Result<()> {
        tracing::debug!("GenericHookExecutor: execute_hook (no-op)");
        Ok(())
    }

    fn list_available_hooks(&self) -> anyhow::Result<Vec<HookEntry>> {
        Ok(vec![])
    }
}

// ─── RoutingTarget ───────────────────────────────────────────────────

/// Generic RoutingTarget.
pub struct GenericRoutingTarget {
    pub tier: ModelTier,
}

impl RoutingTarget for GenericRoutingTarget {
    fn routing_info(&self) -> anyhow::Result<Vec<ModelEntry>> {
        Ok(vec![])
    }

    fn current_tier(&self) -> anyhow::Result<ModelTier> {
        Ok(self.tier.clone())
    }
}

// ─── SessionExportable ───────────────────────────────────────────────

/// Generic SessionExportable.
pub struct GenericSessionExportable;

impl SessionExportable for GenericSessionExportable {
    fn export_session(&self, id: &str) -> anyhow::Result<SessionManifest> {
        Err(anyhow::anyhow!("GenericAdapter: session '{}' not found (no store)", id))
    }

    fn list_sessions(&self) -> anyhow::Result<Vec<SessionManifest>> {
        Ok(vec![])
    }
}

// ─── Builder ─────────────────────────────────────────────────────────

/// Builds concrete trait implementations from a ToolToml config.
pub struct TraitBuilder {
    config: ToolToml,
    agents_dir: PathBuf,
}

impl TraitBuilder {
    pub fn new(config: &ToolToml, agents_dir: &PathBuf) -> Self {
        Self {
            config: config.clone(),
            agents_dir: agents_dir.clone(),
        }
    }

    /// Build SkillProvider if `[skills]` is present, else no-op.
    pub fn build_skill_provider(&self) -> ImplSwitch<GenericSkillProvider> {
        match self.config.skills.as_ref() {
            Some(skills) => {
                let native_dir = skills
                    .native_dir
                    .as_ref()
                    .map(|d| PathBuf::from(shellexpand::tilde(d).as_ref()))
                    .unwrap_or_else(|| self.agents_dir.join("skills"));
                ImplSwitch::some(GenericSkillProvider { native_dir })
            }
            None => ImplSwitch::none(),
        }
    }

    /// Build McpConsumer (always available, no-op by default).
    pub fn build_mcp_consumer(&self) -> ImplSwitch<GenericMcpConsumer> {
        if self.config.mcp.is_some() {
            ImplSwitch::some(GenericMcpConsumer)
        } else {
            ImplSwitch::none()
        }
    }

    /// Build AuthProviderTrait.
    pub fn build_auth_provider(&self) -> ImplSwitch<GenericAuthProvider> {
        if self.config.auth.is_some() {
            ImplSwitch::some(GenericAuthProvider)
        } else {
            ImplSwitch::none()
        }
    }

    /// Build HookExecutor.
    pub fn build_hook_executor(&self) -> ImplSwitch<GenericHookExecutor> {
        if self.config.hooks.is_some() {
            ImplSwitch::some(GenericHookExecutor)
        } else {
            ImplSwitch::none()
        }
    }

    /// Build RoutingTarget.
    pub fn build_routing_target(&self) -> ImplSwitch<GenericRoutingTarget> {
        if self.config.routing.is_some() {
            let tier = match self.config.routing.as_ref().map(|r| r.default_tier.as_str()) {
                Some("flagship") => ModelTier::Flagship,
                Some("slm") => ModelTier::Slm,
                _ => ModelTier::Mid,
            };
            ImplSwitch::some(GenericRoutingTarget { tier })
        } else {
            ImplSwitch::none()
        }
    }

    /// Build SessionExportable.
    pub fn build_session_exportable(&self) -> ImplSwitch<GenericSessionExportable> {
        if self.config.sessions.is_some() {
            ImplSwitch::some(GenericSessionExportable)
        } else {
            ImplSwitch::none()
        }
    }

    /// Build all traits and return them as a tuple.
    #[allow(clippy::type_complexity)]
    pub fn build_all(
        &self,
    ) -> (
        ImplSwitch<GenericSkillProvider>,
        ImplSwitch<GenericMcpConsumer>,
        ImplSwitch<GenericAuthProvider>,
        ImplSwitch<GenericHookExecutor>,
        ImplSwitch<GenericRoutingTarget>,
        ImplSwitch<GenericSessionExportable>,
    ) {
        (
            self.build_skill_provider(),
            self.build_mcp_consumer(),
            self.build_auth_provider(),
            self.build_hook_executor(),
            self.build_routing_target(),
            self.build_session_exportable(),
        )
    }
}
