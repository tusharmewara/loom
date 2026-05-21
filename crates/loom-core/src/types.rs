use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ─── Ecosystem Config ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoomConfig {
    pub ecosystem: EcosystemConfig,
    pub acp_registry_url: Option<String>,
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
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HookEventType::PreToolUse => write!(f, "PreToolUse"),
            HookEventType::PostToolUse => write!(f, "PostToolUse"),
            HookEventType::SessionStart => write!(f, "SessionStart"),
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
    pub created_at: String,
    pub updated_at: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskPhase {
    Plan,
    Implement,
    Verify,
    Finish,
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
