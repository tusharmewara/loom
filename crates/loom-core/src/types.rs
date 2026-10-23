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
    #[serde(default)]
    pub workflow: Option<WorkflowConfig>,
    #[serde(default)]
    pub security: SecurityConfig,
}

/// Security posture knobs. Defaults preserve the historic trust model.
///
/// Trust model (docs/routing-debt.md Step 5): everything under the loom
/// dir is user-owned local state. Signed artifacts (hooks, plugins) are
/// verified whenever a signature is present; unsigned ones have always
/// been allowed, because write access to the loom dir already implies
/// trust. `require_signature = true` flips the posture to
/// verify-by-default: unsigned hooks and plugins are rejected outright.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Reject hooks and plugins that carry no signature.
    /// Default false (back-compat: unsigned local artifacts are allowed).
    #[serde(default)]
    pub require_signature: bool,
    /// Audit chain retention window, in days.
    ///
    /// Entries older than this are dropped from an audit export
    /// (`loom auth export`). The on-disk chain is never rewritten — dropping
    /// entries from the head would break the hash linkage the chain's
    /// integrity check relies on — so the window bounds what leaves the
    /// machine, and the log file itself stays bounded by size rotation.
    ///
    /// `0` keeps everything (no expiry). Default: 365 days.
    #[serde(default = "default_audit_retention_days")]
    pub audit_retention_days: u32,
}

/// Default audit retention window: one year.
pub const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 365;

fn default_audit_retention_days() -> u32 {
    DEFAULT_AUDIT_RETENTION_DAYS
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_signature: false,
            audit_retention_days: default_audit_retention_days(),
        }
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityThresholds {
    #[serde(default = "default_complexity_low_max")]
    pub low_max: f64,
    #[serde(default = "default_complexity_high_min")]
    pub high_min: f64,
}

// Default must match the serde defaults (0.35 / 0.7), not f64::default().
// The derived Default previously yielded 0.0/0.0, so any routing config
// without a [complexity_thresholds] section treated every score as High
// (routing-debt.md Step 3).
impl Default for ComplexityThresholds {
    fn default() -> Self {
        Self {
            low_max: default_complexity_low_max(),
            high_min: default_complexity_high_min(),
        }
    }
}

fn default_complexity_low_max() -> f64 {
    0.35
}

fn default_complexity_high_min() -> f64 {
    0.7
}

/// Derive a `ComplexityLevel` from a numeric score and configured thresholds.
///
/// Single source of truth for level cutoffs (routing-debt.md Step 3): every
/// numeric→level derivation must route through here so config thresholds
/// govern both level and tier.
pub fn derive_level(numeric: f64, thresholds: &ComplexityThresholds) -> ComplexityLevel {
    if numeric < thresholds.low_max {
        ComplexityLevel::Low
    } else if numeric < thresholds.high_min {
        ComplexityLevel::Medium
    } else {
        ComplexityLevel::High
    }
}

/// Derive a `ModelTier` from a `ComplexityLevel`.
///
/// Single source of truth for the level→tier mapping (routing-debt.md Step 3).
pub fn derive_tier(level: ComplexityLevel) -> ModelTier {
    match level {
        ComplexityLevel::Low => ModelTier::Slm,
        ComplexityLevel::Medium => ModelTier::Mid,
        ComplexityLevel::High => ModelTier::Flagship,
    }
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

fn default_hook_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksGlobalConfig {
    #[serde(default = "default_hook_timeout")]
    pub default_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default = "default_workflow_worktree_auto")]
    pub worktree_auto: bool,
    #[serde(default)]
    pub worktree_base: Option<String>,
    #[serde(default = "default_runtime")]
    pub runtime: String,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        Self {
            worktree_auto: true,
            worktree_base: None,
            runtime: default_runtime(),
        }
    }
}

fn default_workflow_worktree_auto() -> bool {
    true
}

fn default_runtime() -> String {
    "process".to_string()
}

/// Launch configuration for tool execution.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaunchConfig {
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
    /// Container network posture (for docker/podman runtimes).
    ///
    /// One of `none` (default, no egress), `restricted` (egress limited to an
    /// allowlist) or `unrestricted` (engine default network, full egress).
    /// Free-form network names are rejected.
    pub network: Option<String>,
    /// Auto-remove container after execution (docker/podman).
    #[serde(default = "default_auto_remove")]
    pub auto_remove: bool,
}

fn default_auto_remove() -> bool {
    true
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Hash of the previous entry's `entry_hash` (chain head marker for the
    /// first entry). Absent in legacy logs written before integrity chaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_hash: Option<String>,
    /// SHA-256 over this entry's canonical fields plus `prev_hash`.
    /// Absent in legacy logs written before integrity chaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_hash: Option<String>,
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
    /// Provenance of the numbers above. A routing decision is taken before the
    /// run, so this is an estimate or unavailable — never a measurement.
    #[serde(default)]
    pub source: UsageSource,
    /// How the estimate was derived ("chars/4"). Empty when unavailable.
    #[serde(default)]
    pub method: String,
}

impl CostEstimate {
    /// Estimate the tokens and cost of a prompt of `chars` characters.
    ///
    /// Method: `chars/4` for the input (about four characters per token) and
    /// a quarter of that for the output. The result is an ESTIMATE — `source`
    /// says so, and a measurement is never stored here.
    pub fn estimated(
        chars: usize,
        input_cost_per_1k: f64,
        output_cost_per_1k: f64,
        confidence: &str,
    ) -> Self {
        let Some(input) = estimate_tokens(chars) else {
            return Self {
                confidence: confidence.to_string(),
                source: UsageSource::Unavailable,
                method: String::new(),
                ..Default::default()
            };
        };
        let output = ((input as f64) / 4.0).ceil() as u32;
        Self {
            input_tokens_estimated: input.min(u32::MAX as u64) as u32,
            output_tokens_estimated: output,
            estimated_cost_usd: (input as f64 / 1000.0) * input_cost_per_1k
                + (output as f64 / 1000.0) * output_cost_per_1k,
            confidence: confidence.to_string(),
            source: UsageSource::Estimated,
            method: "chars/4".to_string(),
        }
    }
}

// ─── Usage metering ─────────────────────────────────────────────────

/// Where a token or cost number came from.
///
/// `Measured` — the provider or protocol reported it.
/// `Estimated` — a documented heuristic produced it.
/// `Unavailable` — no count exists. A row that says `Unavailable` never
/// carries a placeholder count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UsageSource {
    Measured,
    Estimated,
    #[default]
    Unavailable,
}

impl UsageSource {
    /// The storage/wire form of this provenance.
    pub fn as_str(&self) -> &'static str {
        match self {
            UsageSource::Measured => "measured",
            UsageSource::Estimated => "estimated",
            UsageSource::Unavailable => "unavailable",
        }
    }

    /// Parse the storage/wire form. Anything unknown reads as `Unavailable`.
    pub fn from_str_lossy(value: &str) -> Self {
        match value {
            "measured" => UsageSource::Measured,
            "estimated" => UsageSource::Estimated,
            _ => UsageSource::Unavailable,
        }
    }
}

/// Rough token count for a prompt of `chars` characters: about four
/// characters per token. `None` for an empty prompt — no text means no
/// estimate, not zero.
pub fn estimate_tokens(chars: usize) -> Option<u64> {
    if chars == 0 {
        return None;
    }
    Some(((chars as f64) / 4.0).ceil() as u64)
}

/// The counts a source reported for one run. `None` = the source reported
/// no count — never a placeholder, never a silently substituted zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageCounts {
    /// Input tokens consumed.
    pub input_tokens: Option<u64>,
    /// Output tokens produced.
    pub output_tokens: Option<u64>,
    /// Context-window usage in tokens (ACP `usage_update.used`). Not a turn
    /// count.
    pub context_tokens: Option<u64>,
    /// Cost in USD, as reported by the source.
    pub cost_usd: Option<f64>,
}

impl UsageCounts {
    /// Did the source report a token count?
    ///
    /// A run that reported only a context-window size or only a cost did
    /// NOT report a token count. The two are different provenance, and
    /// conflating them makes a ledger say "zero tokens measured" for a
    /// run that only ever reported a cost.
    pub fn reported_token_count(&self) -> bool {
        self.input_tokens.is_some() || self.output_tokens.is_some()
    }
}

/// Cost in USD of a measured token count at the given per-1k prices.
///
/// `None` when either token count is missing: a cost needs both legs, and a
/// half-cost is a lie.
pub fn measured_cost_usd(
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    input_cost_per_1k: f64,
    output_cost_per_1k: f64,
) -> Option<f64> {
    let input = input_tokens?;
    let output = output_tokens?;
    Some(
        (input as f64 / 1000.0) * input_cost_per_1k + (output as f64 / 1000.0) * output_cost_per_1k,
    )
}

/// One metered run: what a run cost, and where the numbers came from.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageRecord {
    /// When the run was recorded (RFC3339, UTC).
    pub recorded_at: String,
    /// Identifies the run.
    pub run_id: String,
    /// The tool that produced the run ("resume", "acp", ...).
    pub tool: String,
    /// The model or agent that served it.
    pub model: String,
    /// Provenance of the counts below.
    pub source: UsageSource,
    /// Where the numbers came from ("provider", "acp:prompt-usage", ...).
    pub method: String,
    /// Input tokens actually consumed. `None` = the source reported no count.
    pub input_tokens: Option<u64>,
    /// Output tokens actually consumed. `None` = the source reported no count.
    pub output_tokens: Option<u64>,
    /// Context-window size in tokens (ACP `usage_update`). Not a turn count.
    pub context_tokens: Option<u64>,
    /// Cost in USD. `None` = neither reported nor computable from a price.
    pub cost_usd: Option<f64>,
    /// Forecast input tokens for the same run. Always an estimate.
    pub est_input_tokens: Option<u64>,
    /// Forecast output tokens for the same run. Always an estimate.
    pub est_output_tokens: Option<u64>,
    /// Forecast cost in USD for the same run. Always an estimate.
    pub est_cost_usd: Option<f64>,
}

impl UsageRecord {
    /// A run whose source reported real token counts.
    pub fn measured(
        run_id: &str,
        tool: &str,
        model: &str,
        input: u64,
        output: u64,
        method: &str,
    ) -> Self {
        Self {
            recorded_at: crate::rfc3339_now(),
            run_id: run_id.to_string(),
            tool: tool.to_string(),
            model: model.to_string(),
            source: UsageSource::Measured,
            method: method.to_string(),
            input_tokens: Some(input),
            output_tokens: Some(output),
            ..Default::default()
        }
    }

    /// A run whose source reported no usage at all. No counts are invented.
    pub fn unavailable(run_id: &str, tool: &str, model: &str) -> Self {
        Self {
            recorded_at: crate::rfc3339_now(),
            run_id: run_id.to_string(),
            tool: tool.to_string(),
            model: model.to_string(),
            source: UsageSource::Unavailable,
            ..Default::default()
        }
    }

    /// A run built from whatever counts the source reported.
    ///
    /// `Measured` when the source reported any usage figure at all;
    /// `Unavailable` when it reported nothing. Legs the source did not
    /// report stay `None` — a context-window size is not a turn count, so
    /// it is never promoted to `input_tokens`/`output_tokens`.
    ///
    /// Two provenance bits come out of this, and they are NOT the same
    /// thing: `source` says the source reported *something* (a token
    /// count, a context size, a cost), while the token legs say whether a
    /// token *count* was reported. A run that reported only a cost is
    /// measured, but it is not a run that reported a token count — see
    /// [`UsageRecord::reported_token_count`].
    pub fn from_counts(
        run_id: &str,
        tool: &str,
        model: &str,
        counts: UsageCounts,
        method: &str,
    ) -> Self {
        let reported = counts.input_tokens.is_some()
            || counts.output_tokens.is_some()
            || counts.context_tokens.is_some()
            || counts.cost_usd.is_some();
        Self {
            recorded_at: crate::rfc3339_now(),
            run_id: run_id.to_string(),
            tool: tool.to_string(),
            model: model.to_string(),
            source: if reported {
                UsageSource::Measured
            } else {
                UsageSource::Unavailable
            },
            method: method.to_string(),
            input_tokens: counts.input_tokens,
            output_tokens: counts.output_tokens,
            context_tokens: counts.context_tokens,
            cost_usd: counts.cost_usd,
            ..Default::default()
        }
    }

    /// Did this run report a token count?
    ///
    /// `false` for a run that reported only a context size or only a cost:
    /// the source was `Measured`, but there is no token count to sum. An
    /// aggregate over such runs must stay `None`, not `0`.
    pub fn reported_token_count(&self) -> bool {
        self.input_tokens.is_some() || self.output_tokens.is_some()
    }

    /// Attach the `chars/4` forecast for the prompt this run was given.
    /// `prices` is (input_cost_per_1k, output_cost_per_1k); without it the
    /// forecast stays in tokens and no cost is invented.
    pub fn with_estimate(mut self, prompt_chars: usize, prices: Option<(f64, f64)>) -> Self {
        let Some(input) = estimate_tokens(prompt_chars) else {
            return self;
        };
        let output = ((input as f64) / 4.0).ceil() as u64;
        self.est_input_tokens = Some(input);
        self.est_output_tokens = Some(output);
        if let Some((input_cost_per_1k, output_cost_per_1k)) = prices {
            self.est_cost_usd = Some(
                (input as f64 / 1000.0) * input_cost_per_1k
                    + (output as f64 / 1000.0) * output_cost_per_1k,
            );
        }
        self
    }
}

// ─── Model Catalog ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCatalog {
    #[serde(default)]
    pub models: Vec<ModelEntry>,
}

impl ModelCatalog {
    /// (input_cost_per_1k, output_cost_per_1k) for a model id, if the
    /// catalog knows it. Used to price a measured token count.
    ///
    /// Falls back to a prefix match so a dated model id
    /// ("claude-sonnet-4-20250514") finds its family entry
    /// ("claude-sonnet-4").
    pub fn prices_for(&self, model_id: &str) -> Option<(f64, f64)> {
        let exact = self
            .models
            .iter()
            .find(|m| m.model_id == model_id)
            .map(|m| (m.input_cost_per_1k, m.output_cost_per_1k));
        if exact.is_some() {
            return exact;
        }
        self.models
            .iter()
            .filter(|m| model_id.starts_with(m.model_id.as_str()))
            .max_by_key(|m| m.model_id.len())
            .map(|m| (m.input_cost_per_1k, m.output_cost_per_1k))
    }
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
    /// Free-form tags for grouping/filtering tasks (e.g. plan imports).
    /// `#[serde(default)]` keeps older task files loadable.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Path to git worktree for this task, if auto-created.
    #[serde(default)]
    pub worktree_path: Option<String>,
    /// Runtime backend to use for this task (process, docker, etc.).
    #[serde(default)]
    pub runtime: Option<String>,
    /// Outcome of the most recent daemon-tracked dispatch run, stamped
    /// when the run completes so `loom workflow list/show`, the TUI and
    /// event subscribers can see async agent outcomes. Only the
    /// daemon's ACP dispatch path writes this; the CLI workflow loop
    /// records richer `phase_results` instead.
    #[serde(default)]
    pub last_run: Option<TaskLastRun>,
    pub created_at: String,
    pub updated_at: String,
    pub status: TaskStatus,
}

/// Outcome of the most recent daemon-tracked dispatch run for a task
/// (see `Task::last_run`). Mirrors the daemon's run registry entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLastRun {
    pub run_id: String,
    pub agent: String,
    /// `succeeded` or `failed` — matches the run registry's `RunStatus`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// RFC3339 timestamp when the run completed.
    pub ended_at: String,
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

fn default_max_retries() -> u8 {
    3
}

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
    /// A daemon-tracked dispatch run finished and its outcome was
    /// stamped onto the task manifest. `status` is `succeeded`/`failed`
    /// (see `TaskLastRun::status`).
    TaskRunCompleted {
        task_id: String,
        run_id: String,
        status: String,
        session_id: Option<String>,
    },
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
    fn export_credential(
        &self,
        provider_id: &str,
        requesting_tool: &str,
    ) -> anyhow::Result<EncryptedCredential>;
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
        self.lsp_servers()
            .into_iter()
            .find(|s| s.file_extensions.iter().any(|e| e == extension))
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
        self.debug_adapters()
            .into_iter()
            .find(|a| a.language == language)
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

#[cfg(test)]
mod derive_tests {
    use super::*;

    #[test]
    fn level_boundaries_follow_thresholds() {
        let t = ComplexityThresholds::default();
        assert_eq!(t.low_max, 0.35);
        assert_eq!(t.high_min, 0.7);
        assert!(matches!(derive_level(0.0, &t), ComplexityLevel::Low));
        assert!(matches!(derive_level(0.34999, &t), ComplexityLevel::Low));
        assert!(matches!(derive_level(0.35, &t), ComplexityLevel::Medium));
        assert!(matches!(derive_level(0.69999, &t), ComplexityLevel::Medium));
        assert!(matches!(derive_level(0.7, &t), ComplexityLevel::High));
    }

    #[test]
    fn custom_thresholds_resolve_contradiction_case() {
        // routing-debt Step 3 live bug: config low_max=0.3 + numeric=0.32
        // used to yield level=Low (hardcoded 0.35 cutoff) but tier=Mid
        // (config cutoff). One derivation now: both sides say Medium/Mid.
        let t = ComplexityThresholds {
            low_max: 0.3,
            high_min: 0.7,
        };
        let level = derive_level(0.32, &t);
        assert!(matches!(level, ComplexityLevel::Medium));
        assert!(matches!(derive_tier(level), ModelTier::Mid));
    }

    #[test]
    fn tier_maps_levels() {
        assert_eq!(derive_tier(ComplexityLevel::Low), ModelTier::Slm);
        assert_eq!(derive_tier(ComplexityLevel::Medium), ModelTier::Mid);
        assert_eq!(derive_tier(ComplexityLevel::High), ModelTier::Flagship);
    }
}
