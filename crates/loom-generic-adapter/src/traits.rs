//! Conditional trait implementations for the GenericAdapter.
//!
//! Each tool TOML section maps to a Loom trait. This module provides:
//!
//! - `ImplSwitch<T>` — wraps an implementation or acts as a no-op.
//! - Concrete impls for each trait that delegate to TOML-derived config.

#![allow(dead_code, unused)]

use loom_core::{
    AuthProviderTrait, EncryptedCredential, HookEntry, HookExecutor, McpConsumer, McpServerEntry,
    ModelEntry, ModelTier, ProviderEntry, RoutingTarget, SessionExportable, SessionManifest,
    SkillEntry, SkillProvider,
};
use std::path::{Path, PathBuf};

use crate::config::ToolToml;

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
                    last_modified: loom_core::rfc3339_now(),
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

    fn export_credential(
        &self,
        _provider_id: &str,
        _requesting_tool: &str,
    ) -> anyhow::Result<EncryptedCredential> {
        Err(anyhow::anyhow!(
            "GenericAdapter: no real credential store configured"
        ))
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
        Ok(self.tier)
    }
}

// ─── SessionExportable ───────────────────────────────────────────────

/// Generic SessionExportable.
pub struct GenericSessionExportable;

impl SessionExportable for GenericSessionExportable {
    fn export_session(&self, id: &str) -> anyhow::Result<SessionManifest> {
        Err(anyhow::anyhow!(
            "GenericAdapter: session '{}' not found (no store)",
            id
        ))
    }

    fn list_sessions(&self) -> anyhow::Result<Vec<SessionManifest>> {
        Ok(vec![])
    }
}

// ─── Builder ─────────────────────────────────────────────────────────

/// Builds concrete trait implementations from a ToolToml config.
pub struct TraitBuilder {
    config: ToolToml,
    loom_dir: PathBuf,
}

impl TraitBuilder {
    pub fn new(config: &ToolToml, loom_dir: &Path) -> Self {
        Self {
            config: config.clone(),
            loom_dir: loom_dir.to_path_buf(),
        }
    }

    /// Build SkillProvider if `[skills]` is present, else None.
    pub fn build_skill_provider(&self) -> Option<GenericSkillProvider> {
        self.config.skills.as_ref().map(|skills| {
            let native_dir = skills
                .native_dir
                .as_ref()
                .map(|d| PathBuf::from(shellexpand::tilde(d).as_ref()))
                .unwrap_or_else(|| self.loom_dir.join("skills"));
            GenericSkillProvider { native_dir }
        })
    }

    /// Build McpConsumer if `[mcp]` is present, else None.
    pub fn build_mcp_consumer(&self) -> Option<GenericMcpConsumer> {
        if self.config.mcp.is_some() {
            Some(GenericMcpConsumer)
        } else {
            None
        }
    }

    /// Build AuthProviderTrait if `[auth]` is present, else None.
    pub fn build_auth_provider(&self) -> Option<GenericAuthProvider> {
        if self.config.auth.is_some() {
            Some(GenericAuthProvider)
        } else {
            None
        }
    }

    /// Build HookExecutor if `[hooks]` is present, else None.
    pub fn build_hook_executor(&self) -> Option<GenericHookExecutor> {
        if self.config.hooks.is_some() {
            Some(GenericHookExecutor)
        } else {
            None
        }
    }

    /// Build RoutingTarget if `[routing]` is present, else None.
    pub fn build_routing_target(&self) -> Option<GenericRoutingTarget> {
        self.config.routing.as_ref().map(|r| {
            let tier = match r.default_tier.as_str() {
                "flagship" => ModelTier::Flagship,
                "slm" => ModelTier::Slm,
                _ => ModelTier::Mid,
            };
            GenericRoutingTarget { tier }
        })
    }

    /// Build SessionExportable if `[sessions]` is present, else None.
    pub fn build_session_exportable(&self) -> Option<GenericSessionExportable> {
        if self.config.sessions.is_some() {
            Some(GenericSessionExportable)
        } else {
            None
        }
    }

    /// Build all traits and return them as a tuple.
    #[allow(clippy::type_complexity)]
    pub fn build_all(
        &self,
    ) -> (
        Option<GenericSkillProvider>,
        Option<GenericMcpConsumer>,
        Option<GenericAuthProvider>,
        Option<GenericHookExecutor>,
        Option<GenericRoutingTarget>,
        Option<GenericSessionExportable>,
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
