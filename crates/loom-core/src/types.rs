use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ─── Ecosystem Config ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoomConfig {
    pub ecosystem: EcosystemConfig,
    pub acp_registry_url: Option<String>,
    /// Config files to extend from (deep-merged, lower priority).
    /// Supports `~` in paths. Use TOML array: `extends = ["/path/to/base.toml"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<Vec<String>>,
    /// Remote config URL to fetch (HTTPS) with SHA-256 caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_config_url: Option<String>,
    #[serde(default)]
    pub routing: Option<RoutingConfig>,
    #[serde(default)]
    pub mcp: Option<McpGlobalConfig>,
    #[serde(default)]
    pub hooks: Option<HooksGlobalConfig>,
}

// ... rest of file unchanged ...

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcosystemConfig {
    pub version: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_native_first")]
    pub conflict_resolution: ConflictResolution,
    #[serde(default)]
    pub layers: LayerFlags,
    #[serde(default)]
    pub tools: ToolAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolution {
    NativeFirst,
    SharedFirst,
    Manual,
}

impl fmt::Display for ConflictResolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConflictResolution::NativeFirst => write!(f, "NativeFirst"),
            ConflictResolution::SharedFirst => write!(f, "SharedFirst"),
            ConflictResolution::Manual => write!(f, "Manual"),
        }
    }
}

fn default_native_first() -> ConflictResolution {
    ConflictResolution::NativeFirst
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerFlags {
    #[serde(default = "default_true")]
    pub skills: bool,
    #[serde(default = "default_true")]
    pub mcp: bool,
    #[serde(default = "default_true")]
    pub sessions: bool,
    #[serde(default = "default_true")]
    pub auth: bool,
    #[serde(default = "default_true")]
    pub hooks: bool,
    #[serde(default = "default_true")]
    pub workflow: bool,
}

impl Default for LayerFlags {
    fn default() -> Self {
        Self {
            skills: true,
            mcp: true,
            sessions: true,
            auth: true,
            hooks: true,
            workflow: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAccess {
    #[serde(default)]
    pub allowed: Vec<String>,
    #[serde(default)]
    pub denied: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default = "default_routing_strategy")]
    pub strategy: RoutingStrategy,
    #[serde(default)]
    pub default_tier: Option<ModelTier>,
    /// Default LLM provider name (e.g. "openai", "anthropic").
    #[serde(default)]
    pub default_provider: Option<String>,
    /// Default model name (e.g. "gpt-4", "claude-sonnet-4-20250514").
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, ModelTier>,
    #[serde(default)]
    pub complexity_thresholds: ComplexityThresholds,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComplexityThresholds {
    #[serde(default = "default_complexity_low_max")]
    pub low_max: f64,
    #[serde(default = "default_complexity_high_min")]
    pub high_min: f64,
}

fn default_complexity_low_max() -> f64 {
    0.35
}

fn default_complexity_high_min() -> f64 {
    0.7
}

fn default_routing_strategy() -> RoutingStrategy {
    RoutingStrategy::Balanced
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpGlobalConfig {
    #[serde(default = "default_health_interval")]
    pub health_check_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub auto_import: bool,
}

fn default_health_interval() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksGlobalConfig {
    #[serde(default = "default_hook_timeout")]
    pub default_timeout_seconds: u64,
}

fn default_hook_timeout() -> u64 {
    30
}

// ─── Skill Registry ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub path: String,
    pub content_hash: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub ecosystem_compatible: Vec<String>,
    #[serde(default)]
    pub ecosystem_requires: Vec<String>,
    pub version: String,
    pub last_modified: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRegistry {
    pub version: String,
    pub skills: Vec<SkillEntry>,
}

// ─── MCP ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default = "default_true")]
    pub shared: bool,
    #[serde(default)]
    pub tools_filter: Vec<String>,
    #[serde(default = "default_health_unknown")]
    pub health_status: HealthStatusStr,
    pub last_health_check: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub compatible_tools: Vec<String>,
    #[serde(default)]
    pub required_by_skills: Vec<String>,
}

fn default_health_unknown() -> HealthStatusStr {
    HealthStatusStr::Unknown
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTransport {
    pub transport_type: McpTransportType,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum McpTransportType {
    Stdio,
    Http,
    Sse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatusStr {
    Unknown,
    Healthy,
    Degraded,
    Offline,
}

// ─── Session Manifest ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionManifest {
    pub session_id: String,
    pub source_tool: String,
    pub source_version: String,
    pub exported_at: String,
    pub topic: String,
    pub message_count: u32,
    #[serde(default)]
    pub active_skills: Vec<String>,
    #[serde(default)]
    pub active_mcp_servers: Vec<String>,
    pub working_directory: String,
    pub model_provider: String,
    #[serde(default)]
    pub files_modified: Vec<String>,
    pub compatibility_note: Option<String>,
    /// Free-form, user-supplied tags for grouping/filtering sessions.
    /// `#[serde(default)]` keeps v0.3.5–v0.3.7 manifests loadable — old
    /// manifests surface with `tags = []`. Tags are stored verbatim (no
    /// case folding, no validation) but the CLI normalizes whitespace.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Timestamp of the most recent auto-summarization pass, RFC 3339.
    /// `None` for sessions that have never been summarized (and for
    /// pre-v0.3.9 manifests — `#[serde(default)]` keeps them loadable).
    /// The summary itself lives inline as the first message of the
    /// session (with `MessageRole::System` and id `summary-v1`); this
    /// field is a fast-lookup hint for `sessions show`/`list` so we
    /// don't have to scan `messages.jsonl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarized_at: Option<String>,
    /// Number of messages that were folded into the inline summary at
    /// `summarized_at` time. `None` when never summarized. Together with
    /// `summarized_at`, this lets a consumer know how much of the
    /// session is "compressed view" vs "full fidelity" (the last
    /// `SESSION_FULL_FIDELITY_COUNT` messages are always full).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarized_message_count: Option<u32>,
}

/// Marker `Message.id` value used for inline summary messages that
/// `compress_session` inserts at the head of the message list. The
/// trailing `v1` lets us evolve the format without confusing older
/// manifests — on read we recognize summaries by id, not by content.
pub const SUMMARY_MESSAGE_ID: &str = "loom-summary-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: String,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub timestamp: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub input: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub result_type: Option<String>,
}

// ─── Auth ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub provider_id: String,
    pub name: String,
    pub auth_method: AuthMethod,
    pub api_base_url: String,
    pub models_endpoint: String,
    pub credential_ref: String,
    #[serde(default = "default_provider_status")]
    pub status: ProviderStatus,
    pub last_validated: String,
    #[serde(default)]
    pub supported_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    ApiKey,
    OAuth2,
    DeviceCode,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderStatus {
    Active,
    Expired,
    Unconfigured,
}

fn default_provider_status() -> ProviderStatus {
    ProviderStatus::Unconfigured
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedCredential {
    pub provider_id: String,
    pub encrypted_data: String,
    pub nonce: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AclRule {
    pub tool: String,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub requesting_tool: String,
    pub provider_id: String,
    pub access_type: String,
    pub result: String,
}

// ─── Routing ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ComplexityLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelTier {
    Slm,
    Mid,
    Flagship,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpeedTier {
    Fast,
    Medium,
    Slow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingStrategy {
    CostOptimize,
    QualityOptimize,
    Balanced,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityScore {
    pub level: ComplexityLevel,
    pub numeric: f64,
    pub model_tier: ModelTier,
    pub scored_at: String,
    #[serde(default)]
    pub signals: ComplexitySignals,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComplexitySignals {
    // Static signals (available at task creation)
    #[serde(default)]
    pub description_length: usize,
    #[serde(default)]
    pub keyword_indicators: Vec<String>,
    #[serde(default)]
    pub has_cross_module_refs: bool,
    #[serde(default)]
    pub dependency_depth: u8,
    pub scope_file_count: Option<usize>,
    pub agent_role: Option<String>,
    // Dynamic signals (available at phase transition)
    #[serde(default)]
    pub files_modified_count: Option<usize>,
    #[serde(default)]
    pub prd_length: Option<usize>,
    #[serde(default)]
    pub context_lines_count: Option<u32>,
    #[serde(default)]
    pub phase_iteration_count: Option<u8>,
    #[serde(default)]
    pub error_rate: Option<f64>,
    // User override
    #[serde(default)]
    pub user_override: Option<ComplexityLevel>,
    // Knowledge graph context
    #[serde(default)]
    pub knowledge_entities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    pub recommended_tier: ModelTier,
    pub recommended_provider: String,
    pub recommended_model: String,
    pub reason: String,
    pub overrides_role_default: bool,
    pub decided_at: String,
    pub cost_estimate: CostEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostEstimate {
    pub input_tokens_estimated: u32,
    pub output_tokens_estimated: u32,
    pub estimated_cost_usd: f64,
    pub confidence: String,
}

// ─── Model Catalog ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCatalog {
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub model_id: String,
    pub provider_id: String,
    pub tier: ModelTier,
    pub context_window: u32,
    pub input_cost_per_1k: f64,
    pub output_cost_per_1k: f64,
    pub speed_tier: SpeedTier,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default = "default_true")]
    pub available: bool,
}

// ─── Hooks ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    pub id: String,
    pub event_type: HookEventType,
    pub matcher: Option<String>,
    pub script_path: String,
    #[serde(default = "default_hook_timeout")]
    pub timeout_seconds: u64,
    pub priority: HookPriority,
    pub public_key: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HookEventType {
    PreToolUse,
    PostToolUse,
    SessionStart,
    /// Fired after a workflow phase completes (agent exit). Provides
    /// `LOOM_TASK_ID`, `LOOM_TASK_PHASE`, `LOOM_AGENT`, `LOOM_EXIT_CODE`.
    PostPhase,
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookEventType::PreToolUse => write!(f, "PreToolUse"),
            HookEventType::PostToolUse => write!(f, "PostToolUse"),
            HookEventType::SessionStart => write!(f, "SessionStart"),
            HookEventType::PostPhase => write!(f, "PostPhase"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookPriority {
    Ecosystem,
    Native,
}

// ─── Workflow ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub phase: TaskPhase,
    #[serde(default)]
    pub phase_history: Vec<PhaseHistoryEntry>,
    pub prd_path: Option<String>,
    #[serde(default)]
    pub context_files: Vec<String>,
    #[serde(alias = "assigned_agent")]
    pub routed_provider: Option<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
    pub complexity_score: Option<ComplexityScore>,
    pub routing_decision: Option<RoutingDecision>,
    /// Name of the last agent tool that worked on this task (e.g. "opencode", "goose").
    /// Used as default for `loom workflow dispatch` when `--agent` is omitted.
    #[serde(default)]
    pub last_agent: Option<String>,
    /// Optional linked issue reference (e.g. "owner/repo#123").
    #[serde(default)]
    pub linked_issue: Option<String>,
    /// Per-phase sub-agent configuration for multi-agent dispatch.
    #[serde(default)]
    pub sub_agents: Option<SubAgentConfig>,
    /// Results captured after each phase completes. Keyed by phase name
    /// (e.g. "Plan", "Implement", "Verify", "Finish"). The most recent
    /// completed phase's result is injected as `LOOM_PRIOR_PHASE_RESULT`
    /// when the next phase is dispatched.
    #[serde(default)]
    pub phase_results: Vec<PhaseResult>,
    pub created_at: String,
    pub updated_at: String,
    pub status: TaskStatus,
}

/// Output captured from a dispatched agent after a phase completes.
/// The agent writes `~/.loom/workflow/tasks/{task_id}.{phase}.result.json`
/// (or its body) and loom parses/stores it here for the next phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseResult {
    /// Phase name (e.g. "Plan", "Implement").
    pub phase: String,
    /// Agent tool that produced the result (e.g. "opencode").
    pub agent: String,
    /// Human-readable summary written by the agent.
    pub summary: String,
    /// Files modified or produced by the agent during this phase.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// RFC3339 timestamp when the result was captured.
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPhase {
    Plan,
    Implement,
    Verify,
    Finish,
}

impl TaskPhase {
    /// Canonical phase name used in `phase_history` and `PhaseResult.phase`.
    /// Matches the existing `format!("{:?}", phase)` output for backward
    /// compatibility with previously-written task JSON files.
    pub fn name(&self) -> &'static str {
        match self {
            TaskPhase::Plan => "Plan",
            TaskPhase::Implement => "Implement",
            TaskPhase::Verify => "Verify",
            TaskPhase::Finish => "Finish",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseHistoryEntry {
    pub phase: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub complexity_score: Option<ComplexityScore>,
    #[serde(default)]
    pub routing_decision: Option<RoutingDecision>,
}

/// Per-phase agent assignment for sub-agent orchestration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubAgentConfig {
    /// Agent tool for the Plan phase (e.g. "opencode").
    #[serde(default)]
    pub plan: Option<String>,
    /// Agent tool for the Implement phase (e.g. "goose").
    #[serde(default)]
    pub implement: Option<String>,
    /// Agent tool for the Verify phase (e.g. "jcode").
    #[serde(default)]
    pub verify: Option<String>,
    /// Agent tool for the Finish phase (e.g. "crush").
    #[serde(default)]
    pub finish: Option<String>,
    /// Max retries per phase before falling back to `fallback_agent`.
    #[serde(default = "default_max_retries")]
    pub max_retries: u8,
    /// Optional fallback agent when all retries are exhausted.
    #[serde(default)]
    pub fallback_agent: Option<String>,
}

fn default_max_retries() -> u8 { 3 }

/// Dispatch mode for workflow tasks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum DispatchMode {
    /// Single agent (current behavior).
    #[default]
    Single,
    /// Per-phase sub-agents.
    SubAgents,
}

impl SubAgentConfig {
    /// Get the agent for a given phase, falling back to `default_agent`.
    pub fn agent_for(&self, phase: &TaskPhase, default_agent: Option<&str>) -> Option<String> {
        let specific = match phase {
            TaskPhase::Plan => self.plan.as_ref(),
            TaskPhase::Implement => self.implement.as_ref(),
            TaskPhase::Verify => self.verify.as_ref(),
            TaskPhase::Finish => self.finish.as_ref(),
        };
        specific
            .cloned()
            .or_else(|| default_agent.map(|s| s.to_string()))
    }
}

/// A reusable workflow template that pre-configures task creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template name (used as filename stem).
    pub name: String,
    /// Human-readable description of what this template is for.
    pub description: String,
    /// Pre-filled task title (supports `{issue_title}` placeholder).
    #[serde(default)]
    pub default_title: Option<String>,
    /// Pre-filled task description.
    #[serde(default)]
    pub default_description: Option<String>,
    /// Default agent tool for dispatch.
    #[serde(default)]
    pub default_agent: Option<String>,
    /// Per-phase sub-agent configuration.
    #[serde(default)]
    pub sub_agents: Option<SubAgentConfig>,
    /// Default LLM provider override.
    #[serde(default)]
    pub default_provider: Option<String>,
    /// Default model override.
    #[serde(default)]
    pub default_model: Option<String>,
}

// ─── Discovery ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryResult {
    pub available_layers: Vec<String>,
    pub resource_counts: std::collections::HashMap<String, usize>,
    pub warnings: Vec<String>,
    pub ecosystem_version: String,
}

// ─── Events ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoomEvent {
    SkillsAdded { count: usize },
    SkillsRemoved { name: String },
    McpHealthChanged { id: String, status: HealthStatusStr },
    McpServerAdded { id: String, name: String },
    SessionExported { session_id: String },
    RoutingUpdated,
    AuthProviderAdded { provider_id: String },
    HooksRegistered { hook_id: String },
    TaskPhaseChanged { task_id: String, phase: TaskPhase },
    TaskCreated { task_id: String },
}

// ─── Adapter Traits ─────────────────────────────────────────────────

/// Tool provides skills for discovery
pub trait SkillProvider: Send + Sync {
    fn discover_skills(&self) -> anyhow::Result<Vec<SkillEntry>>;
    fn read_skill_content(&self, name: &str) -> anyhow::Result<String>;
}

/// Tool can consume skills from ecosystem
pub trait SkillConsumer: Send + Sync {
    fn install_skill(&self, skill: &SkillEntry, target: &std::path::Path) -> anyhow::Result<()>;
    fn render_neutral(&self, skill: &SkillEntry) -> anyhow::Result<String>;
}

/// Tool provides MCP servers
pub trait McpProvider: Send + Sync {
    fn discover_mcp_servers(&self) -> anyhow::Result<Vec<McpServerEntry>>;
    fn read_mcp_config(&self) -> anyhow::Result<serde_json::Value>;
}

/// Tool can consume MCP servers from ecosystem
pub trait McpConsumer: Send + Sync {
    fn add_mcp_server(&self, entry: &McpServerEntry) -> anyhow::Result<()>;
    fn remove_mcp_server(&self, id: &str) -> anyhow::Result<()>;
}

/// Tool can export session data
pub trait SessionExportable: Send + Sync {
    fn export_session(&self, id: &str) -> anyhow::Result<SessionManifest>;
    fn list_sessions(&self) -> anyhow::Result<Vec<SessionManifest>>;
}

/// Tool provides authentication providers
pub trait AuthProviderTrait: Send + Sync {
    fn discover_providers(&self) -> anyhow::Result<Vec<ProviderEntry>>;
    fn export_credential(&self, provider_id: &str) -> anyhow::Result<EncryptedCredential>;
}

/// Tool can execute hooks from the shared hook registry
pub trait HookExecutor: Send + Sync {
    fn execute_hook(&self, entry: &HookEntry) -> anyhow::Result<()>;
    fn list_available_hooks(&self) -> anyhow::Result<Vec<HookEntry>>;
}

/// Tool participates in routing decisions
pub trait RoutingTarget: Send + Sync {
    fn routing_info(&self) -> anyhow::Result<Vec<ModelEntry>>;
    fn current_tier(&self) -> anyhow::Result<ModelTier>;
}

/// Information about a language server.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspServerInfo {
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

/// Information about a debug adapter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DebugAdapterInfo {
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

/// Tool provides LSP (Language Server Protocol) capabilities.
///
/// Adapters implement this to enable code-aware operations such as
/// rename, find-references, code actions, goto-definition, and
/// document symbols within the tool's workspace.
pub trait LspProvider: Send + Sync {
    /// Returns the LSP servers this tool supports.
    fn lsp_servers(&self) -> Vec<LspServerInfo>;
    /// Root URI for LSP operations (default: "file://$PWD").
    fn lsp_root_uri(&self) -> Option<String> {
        None
    }
    /// File extensions that trigger LSP server start.
    fn lsp_file_extensions(&self) -> Vec<String> {
        vec![]
    }
    /// Resolve the best LSP server for a given file extension.
    fn resolve_lsp_server(&self, extension: &str) -> Option<LspServerInfo> {
        self.lsp_servers().into_iter().find(|s| s.file_extensions.iter().any(|e| e == extension))
    }
}

/// Tool provides DAP (Debug Adapter Protocol) capabilities.
///
/// Adapters implement this to enable debugging operations such as
/// launch, attach, breakpoints, step, and evaluate within the tool's
/// workspace.
pub trait DebugProvider: Send + Sync {
    /// Returns the debug adapters this tool supports.
    fn debug_adapters(&self) -> Vec<DebugAdapterInfo>;
    /// Root directory for debug sessions.
    fn debug_cwd(&self) -> Option<String> {
        None
    }
    /// Resolve the best debug adapter for a given language.
    fn resolve_debug_adapter(&self, language: &str) -> Option<DebugAdapterInfo> {
        self.debug_adapters().into_iter().find(|a| a.language == language)
    }
}

// ─── Plugin Registry ───────────────────────────────────────────────

/// Manifest for a registered plugin (extension/tool integration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique plugin name (also the filename stem).
    pub name: String,
    /// Semantic version of the plugin.
    pub version: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Author or maintainer.
    #[serde(default)]
    pub author: Option<String>,
    /// Homepage URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Source repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// URL to download the tool TOML that this plugin provides.
    #[serde(default)]
    pub tool_toml_url: Option<String>,
    /// ISO-8601 timestamp of installation.
    #[serde(default)]
    pub installed_at: Option<String>,
    /// SHA-256 checksum of the downloaded tool TOML (hex).
    #[serde(default)]
    pub checksum: Option<String>,
    /// Ed25519 public key (base64) of the plugin publisher.
    #[serde(default)]
    pub public_key: Option<String>,
    /// Ed25519 signature (base64, hex) over the tool TOML content.
    #[serde(default)]
    pub signature: Option<String>,
}

// ─── Knowledge Graph ──────────────────────────────────────────────

/// A node in the knowledge graph — an entity extracted or manually added.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeEntity {
    pub id: String,
    pub name: String,
    /// Entity type: "tool", "concept", "pattern", "person", "skill", "api", etc.
    pub entity_type: String,
    /// Canonical/parent name for grouping variants.
    #[serde(default)]
    pub canonical_name: Option<String>,
    /// Free-form metadata (JSON).
    #[serde(default)]
    pub metadata: serde_json::Value,
    /// ISO-8601 timestamp.
    pub created_at: String,
}

/// A directed relationship (triple) between two entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    pub id: String,
    pub subject_id: String,
    pub subject_name: String,
    pub predicate: String,
    pub object_id: String,
    pub object_name: String,
    /// Confidence weight 0.0–1.0.
    pub weight: f64,
    /// Optional source session that produced this relationship.
    #[serde(default)]
    pub source_session: Option<String>,
    pub created_at: String,
}

/// Result of a subgraph query around an entity.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubgraphResult {
    /// The central entity queried.
    pub entity: KnowledgeEntity,
    /// Directly related entities at depth 1.
    #[serde(default)]
    pub related: Vec<RelatedEntity>,
}

/// A related entity with relationship details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedEntity {
    pub entity: KnowledgeEntity,
    pub relationship: Relationship,
    /// Traversal depth from the query root.
    pub depth: u32,
}
