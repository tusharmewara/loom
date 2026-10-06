//! TOML configuration types for the GenericAdapter.
//!
//! Each tool registers itself with a single TOML file in `~/.loom/tools/{name}.toml`.
//! Sections map 1:1 to Loom traits; absent sections produce no-op implementations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root TOML document for a tool registration.
///
/// # Example
///
/// ```toml
/// [tool]
/// name = "cursor"
/// version = "0.45.0"
///
/// [skills]
/// native_dir = "~/.cursor/skills/"
/// import_on_register = true
///
/// [mcp]
/// config_path = "~/.cursor/mcp.json"
/// shared_servers = ["filesystem", "github"]
///
/// [auth]
/// store_path = "~/.cursor/auth.json"
/// providers = ["anthropic", "openai"]
///
/// [hooks]
/// scripts_dir = "~/.cursor/hooks/"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolToml {
    /// The `[tool]` section (required).
    pub tool: ToolSection,
    /// `[skills]` — implements SkillProvider.
    #[serde(default)]
    pub skills: Option<SkillsSection>,
    /// `[mcp]` — implements McpConsumer.
    #[serde(default)]
    pub mcp: Option<McpSection>,
    /// `[auth]` — implements AuthProviderTrait.
    #[serde(default)]
    pub auth: Option<AuthSection>,
    /// `[hooks]` — implements HookExecutor.
    #[serde(default)]
    pub hooks: Option<HooksSection>,
    /// `[routing]` — implements RoutingTarget.
    #[serde(default)]
    pub routing: Option<RoutingSection>,
    /// `[sessions]` — implements SessionExportable.
    #[serde(default)]
    pub sessions: Option<SessionsSection>,
    /// `[lsp]` — LSP server configuration for the tool.
    #[serde(default)]
    pub lsp: Option<LspSection>,
    /// `[debug]` — Debug adapter configuration for the tool.
    #[serde(default)]
    pub debug: Option<DebugSection>,
    /// `[launch]` — Runtime execution configuration.
    #[serde(default)]
    pub launch: Option<LaunchSection>,
}

/// The `[tool]` section — required metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSection {
    /// Tool display name (e.g. "cursor").
    pub name: String,
    /// Tool version (e.g. "0.45.0").
    pub version: String,
}

/// The `[skills]` section — skill discovery and import.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsSection {
    /// Path to the tool's native skill directory.
    pub native_dir: Option<String>,
    /// Automatically import skills from `native_dir` on registration.
    #[serde(default)]
    pub import_on_register: bool,
    /// If set, also share skills from this directory.
    pub shared_dir: Option<String>,
    /// File extension filter (e.g. "md", "yaml").
    #[serde(default = "default_skill_ext")]
    pub extension: String,
}

fn default_skill_ext() -> String {
    "md".to_string()
}

/// The `[mcp]` section — MCP server config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSection {
    /// Path to the tool's native MCP config (JSON).
    pub config_path: Option<String>,
    /// List of server IDs to share with the ecosystem.
    #[serde(default)]
    pub shared_servers: Vec<String>,
    /// Import servers from the ecosystem on registration.
    #[serde(default)]
    pub import_on_register: bool,
    /// Filter which ecosystem servers to import by tag.
    #[serde(default)]
    pub import_tags: Vec<String>,
}

/// The `[auth]` section — auth provider config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSection {
    /// Path to the tool's local auth store.
    pub store_path: Option<String>,
    /// List of provider IDs to register with the ecosystem.
    #[serde(default)]
    pub providers: Vec<String>,
    /// Import providers from the ecosystem on registration.
    #[serde(default)]
    pub import_on_register: bool,
}

/// The `[hooks]` section — hook script config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksSection {
    /// Directory containing hook scripts.
    pub scripts_dir: Option<String>,
    /// Register all hooks in `scripts_dir` on registration.
    #[serde(default)]
    pub register_on_start: bool,
    /// Default timeout for hook execution (seconds).
    #[serde(default = "default_hook_timeout")]
    pub default_timeout_seconds: u64,
}

fn default_hook_timeout() -> u64 {
    30
}

/// The `[routing]` section — routing metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingSection {
    /// Default model tier for this tool.
    #[serde(default = "default_routing_tier")]
    pub default_tier: String,
    /// Path to model catalog (if different from default).
    pub catalog_path: Option<String>,
    /// Routing strategy hint.
    #[serde(default = "default_routing_strategy")]
    pub strategy: String,
}

fn default_routing_tier() -> String {
    "mid".to_string()
}

fn default_routing_strategy() -> String {
    "balanced".to_string()
}

/// The `[sessions]` section — session export config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionsSection {
    /// Path to the tool's session storage.
    pub store_path: Option<String>,
    /// Auto-export sessions to the shared directory.
    #[serde(default)]
    pub auto_export: bool,
    /// Number of recent sessions to keep in the shared index.
    #[serde(default = "default_max_sessions")]
    pub max_shared_sessions: usize,
}

fn default_max_sessions() -> usize {
    50
}

/// The `[lsp]` section — LSP server declarations for the adapter.
///
/// When present, the adapter can resolve and use LSP operations
/// for code-aware tasks (rename, find-references, code actions, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspSection {
    /// List of LSP servers this tool uses.
    #[serde(default)]
    pub servers: Vec<LspServerConfig>,
    /// Root URI for LSP operations (default: "file://$PWD").
    pub root_uri: Option<String>,
    /// File extensions that trigger LSP server start.
    #[serde(default)]
    pub file_extensions: Vec<String>,
}

/// Configuration for a single LSP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Language ID (e.g. "rust", "python", "typescript").
    pub language: String,
    /// Command to start the server (e.g. "rust-analyzer").
    pub command: String,
    /// Arguments to pass to the server.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this server handles.
    #[serde(default)]
    pub file_extensions: Vec<String>,
}

/// The `[debug]` section — debug adapter declarations for the adapter.
///
/// When present, the adapter can resolve and use DAP operations
/// for debugging tasks (launch, attach, breakpoints, step, evaluate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugSection {
    /// List of debug adapters this tool supports.
    #[serde(default)]
    pub adapters: Vec<DebugAdapterConfig>,
    /// Root directory for debug sessions.
    pub cwd: Option<String>,
}

/// Configuration for a single debug adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugAdapterConfig {
    /// Language ID (e.g. "rust", "go", "python", "cpp").
    pub language: String,
    /// Command to start the debug adapter (e.g. "lldb-dap").
    pub command: String,
    /// Arguments to pass to the adapter.
    #[serde(default)]
    pub args: Vec<String>,
    /// Display name.
    pub name: Option<String>,
}

/// The `[launch]` section — runtime execution configuration.
///
/// Controls how the tool is launched for task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchSection {
    /// Runtime backend: "process" (default), "docker", "podman".
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// Container image to use when runtime is "docker" or "podman".
    pub image: Option<String>,
    /// Command to execute (e.g. the agent binary).
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory (defaults to current directory or worktree).
    pub working_dir: Option<String>,
    /// Container network mode (for docker/podman runtimes).
    pub network: Option<String>,
    /// Auto-remove container after execution (docker/podman).
    #[serde(default = "default_auto_remove")]
    pub auto_remove: bool,
}

fn default_runtime() -> String {
    "process".to_string()
}

fn default_auto_remove() -> bool {
    true
}
