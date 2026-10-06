pub mod config;
pub mod scanner;

mod traits;

use loom_core::HealthCheck;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::ToolToml;

/// The GenericAdapter auto-implements all 6 Loom traits from a single TOML file.
///
/// When instantiated for a tool (e.g. "cursor"), it reads `~/.loom/tools/cursor.toml`
/// and conditionally implements:
///
/// | TOML Section | Trait |
/// |---|---|
/// | `[skills]` | `SkillProvider` |
/// | `[mcp]` | `McpConsumer` |
/// | `[auth]` | `AuthProviderTrait` |
/// | `[hooks]` | `HookExecutor` |
/// | `[routing]` | `RoutingTarget` |
/// | `[sessions]` | `SessionExportable` |
///
/// Missing sections produce no-op implementations — the adapter never errors
/// on absent sections.
pub struct GenericAdapter {
    loom_dir: PathBuf,
    tool_name: String,
    config: ToolToml,
}

impl GenericAdapter {
    /// Create a GenericAdapter for a specific tool.
    ///
    /// `loom_dir` = `~/.loom/`, `tool_name` = filename stem in `tools/`.
    pub fn new(loom_dir: &str, tool_name: &str) -> anyhow::Result<Self> {
        let tools_dir = PathBuf::from(loom_dir).join("tools");
        let toml_path = tools_dir.join(format!("{}.toml", tool_name));
        let raw = std::fs::read_to_string(&toml_path)?;
        let config: ToolToml = toml::from_str(&raw)?;

        Ok(Self {
            loom_dir: PathBuf::from(loom_dir),
            tool_name: tool_name.to_string(),
            config,
        })
    }

    /// Create a GenericAdapter from raw TOML content (for testing or in-memory use).
    pub fn from_toml(
        loom_dir: &str,
        tool_name: &str,
        toml_content: &str,
    ) -> anyhow::Result<Self> {
        let config: ToolToml = toml::from_str(toml_content)?;
        Ok(Self {
            loom_dir: PathBuf::from(loom_dir),
            tool_name: tool_name.to_string(),
            config,
        })
    }

    /// Build a TraitSet that vends conditional trait implementations.
    pub fn build(&self) -> TraitSet {
        TraitSet {
            config: self.config.clone(),
            loom_dir: self.loom_dir.clone(),
            tool_name: self.tool_name.clone(),
        }
    }

    /// The tool's display name from the TOML.
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Version from the TOML.
    pub fn tool_version(&self) -> &str {
        &self.config.tool.version
    }
}

/// A complete set of possible trait implementations for one tool.
///
/// All 6 traits are available (as `ImplSwitch<T>`); use `.is_present()` to check
/// if the TOML had the relevant section before calling into the implementation.
pub struct TraitSet {
    config: ToolToml,
    #[allow(dead_code)]
    loom_dir: PathBuf,
    #[allow(dead_code)]
    tool_name: String,
}

impl TraitSet {
    /// Whether this tool has a `[skills]` section.
    pub fn has_skills(&self) -> bool {
        self.config.skills.is_some()
    }
    /// Whether this tool has an `[mcp]` section.
    pub fn has_mcp(&self) -> bool {
        self.config.mcp.is_some()
    }
    /// Whether this tool has an `[auth]` section.
    pub fn has_auth(&self) -> bool {
        self.config.auth.is_some()
    }
    /// Whether this tool has a `[hooks]` section.
    pub fn has_hooks(&self) -> bool {
        self.config.hooks.is_some()
    }
    /// Whether this tool has a `[routing]` section.
    pub fn has_routing(&self) -> bool {
        self.config.routing.is_some()
    }
    /// Whether this tool has a `[sessions]` section.
    pub fn has_sessions(&self) -> bool {
        self.config.sessions.is_some()
    }

    /// List of present (enabled) feature names.
    pub fn present_features(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.has_skills() {
            f.push("skills");
        }
        if self.has_mcp() {
            f.push("mcp");
        }
        if self.has_auth() {
            f.push("auth");
        }
        if self.has_hooks() {
            f.push("hooks");
        }
        if self.has_routing() {
            f.push("routing");
        }
        if self.has_sessions() {
            f.push("sessions");
        }
        f
    }

    /// Get the runtime configured for this tool from the `[launch]` section.
    pub fn runtime(&self) -> Option<String> {
        self.config.launch.as_ref().and_then(|l| Some(l.runtime.clone()))
    }

    /// Get the full LaunchConfig for this tool from the `[launch]` section.
    /// Returns None if no `[launch]` section is present.
    pub fn launch_config(&self) -> Option<loom_runtime::LaunchConfig> {
        self.config.launch.as_ref().map(|l| loom_runtime::LaunchConfig {
            runtime: l.runtime.clone(),
            image: l.image.clone(),
            command: l.command.clone(),
            args: l.args.clone(),
            env: l.env.clone(),
            working_dir: l.working_dir.clone(),
            network: l.network.clone(),
            auto_remove: l.auto_remove,
        })
    }

    /// List of absent (no-op) feature names.
    pub fn absent_features(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if !self.has_skills() {
            f.push("skills");
        }
        if !self.has_mcp() {
            f.push("mcp");
        }
        if !self.has_auth() {
            f.push("auth");
        }
        if !self.has_hooks() {
            f.push("hooks");
        }
        if !self.has_routing() {
            f.push("routing");
        }
        if !self.has_sessions() {
            f.push("sessions");
        }
        f
    }
}

// ─── HealthCheck ────────────────────────────────────────────────────

impl HealthCheck for GenericAdapter {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let ts = self.build();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-generic-adapter",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: {
                let mut h = std::collections::HashMap::new();
                h.insert("tool_name".into(), 0);
                h.insert(
                    "features_present".into(),
                    ts.present_features().len() as u64,
                );
                h
            },
        })
    }
}

// Arc-friendly wrapper
impl GenericAdapter {
    /// Wrap into Arc<Self>.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}
