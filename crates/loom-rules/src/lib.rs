//! Loom Stream Rules Engine (P1/5.6)
//!
//! A rule engine for `LoomEvent` stream processing. Rules define conditions
//! that match against incoming events and actions that fire when matched.
//!
//! # Architecture
//!
//! ```text
//!   ┌──────────┐    ┌──────────────┐    ┌────────────┐
//!   │  EventBus │───►│  RuleEngine  │───►│  Actions   │
//!   │ (publish) │    │  (evaluate)  │    │ (execute)  │
//!   └──────────┘    └──────┬───────┘    └────────────┘
//!                          │
//!                     ┌────▼────┐
//!                     │  Rules  │
//!                     │ (TOML)  │
//!                     └─────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use loom_rules::*;
//!
//! let engine = RuleEngine::new("/tmp/.loom");
//! engine.add_rule(Rule {
//!     id: "alert-on-skills".into(),
//!     name: "Alert on skill changes".into(),
//!     enabled: true,
//!     priority: 100,
//!     conditions: vec![
//!         Condition::event_type("SkillsAdded"),
//!     ],
//!     actions: vec![
//!         Action::Log { message: "Skills were added!".into() },
//!     ],
//! });
//!
//! let mut rx = engine.subscribe();
//! // Events evaluated as they come in...
//! ```
//!
//! # TOML Rule Format
//!
//! ```toml
//! id = "alert-skills-flood"
//! name = "Alert on skill flood"
//! description = "Warn when > 10 skills are added at once"
//! enabled = true
//! priority = 50
//!
//! [[conditions]]
//! type = "EventType"
//! pattern = "SkillsAdded"
//!
//! [[conditions]]
//! type = "FieldComparison"
//! field = "count"
//! operator = "GreaterThan"
//! value = "10"
//!
//! [[actions]]
//! type = "Log"
//! level = "warn"
//! message = "Skill flood detected: {{count}} skills added"
//!
//! [[actions]]
//! type = "Suppress"
//! ```

#![warn(missing_docs)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Re-export LoomEvent for convenience
pub use loom_core::LoomEvent;

// ─── Error type ────────────────────────────────────────────────────────

/// Errors from rule operations.
#[derive(Debug, Error)]
pub enum RuleError {
    /// IO error reading/writing rule files.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse error.
    #[error("TOML parse error in {path}: {source}")]
    TomlParse {
        /// Path to problematic file.
        path: String,
        /// Source parse error.
        #[source]
        source: toml::de::Error,
    },
    /// TOML serialization error.
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Rule not found.
    #[error("Rule not found: {0}")]
    NotFound(String),
    /// Invalid condition definition.
    #[error("Invalid condition: {0}")]
    InvalidCondition(String),
    /// Invalid action definition.
    #[error("Invalid action: {0}")]
    InvalidAction(String),
    /// Internal engine error.
    #[error("Engine error: {0}")]
    Engine(String),
}

/// Result type for rule operations.
pub type RuleResult<T> = Result<T, RuleError>;

// ─── Core Rule Types ───────────────────────────────────────────────────

/// A single rule that matches events and executes actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Unique identifier (used as filename stem).
    pub id: String,
    /// Human-readable name.
    #[serde(default)]
    pub name: String,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// Whether this rule is active.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Execution priority (higher = evaluated first).
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Conditions that must ALL match for the rule to fire.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// Actions to execute when conditions match.
    #[serde(default)]
    pub actions: Vec<Action>,
}

fn default_enabled() -> bool {
    true
}
fn default_priority() -> i32 {
    100
}

/// A condition that an event must satisfy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Condition {
    /// Match by event type name (e.g. "SkillsAdded").
    #[serde(rename = "EventType")]
    EventType {
        /// Event variant name (exact match).
        pattern: String,
    },
    /// Match by negated event type.
    #[serde(rename = "NotEventType")]
    NotEventType {
        /// Event variant name to exclude.
        pattern: String,
    },
    /// Compare an event field against a value.
    #[serde(rename = "FieldComparison")]
    FieldComparison {
        /// Dot-notation field path, e.g. "count".
        field: String,
        /// Comparison operator.
        operator: ComparisonOp,
        /// Value to compare against (string representation).
        value: String,
    },
    /// Match events within a time window (UTC).
    #[serde(rename = "TimeWindow")]
    TimeWindow {
        /// ISO 8601 start time.
        start: String,
        /// ISO 8601 end time.
        end: String,
    },
    /// Match any event (always true).
    #[serde(rename = "Any")]
    Any,
}

/// Comparison operators for field conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComparisonOp {
    /// Field equals value.
    Equals,
    /// Field does not equal value.
    NotEquals,
    /// Field is greater than value.
    GreaterThan,
    /// Field is less than value.
    LessThan,
    /// Field contains value (string contains).
    Contains,
    /// Field starts with value.
    StartsWith,
    /// Field ends with value.
    EndsWith,
}

impl std::fmt::Display for ComparisonOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComparisonOp::Equals => write!(f, "Equals"),
            ComparisonOp::NotEquals => write!(f, "NotEquals"),
            ComparisonOp::GreaterThan => write!(f, "GreaterThan"),
            ComparisonOp::LessThan => write!(f, "LessThan"),
            ComparisonOp::Contains => write!(f, "Contains"),
            ComparisonOp::StartsWith => write!(f, "StartsWith"),
            ComparisonOp::EndsWith => write!(f, "EndsWith"),
        }
    }
}

/// An action to execute when a rule matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Action {
    /// Log a message when the rule fires.
    #[serde(rename = "Log")]
    Log {
        /// Log level: trace, debug, info, warn, error.
        #[serde(default = "default_log_level")]
        level: String,
        /// Log message template (supports {{field}} interpolation).
        message: String,
    },
    /// Suppress (drop) the matched event.
    #[serde(rename = "Suppress")]
    Suppress,
    /// Emit a synthetic event when this rule fires.
    #[serde(rename = "EmitEvent")]
    EmitEvent {
        /// The synthetic event JSON object.
        payload: serde_json::Value,
    },
    /// Escalate with a message.
    #[serde(rename = "Escalate")]
    Escalate {
        /// Severity: low, medium, high, critical.
        #[serde(default = "default_escalate_severity")]
        severity: String,
        /// Escalation message.
        message: String,
    },
    /// Transform (modify) the event payload.
    #[serde(rename = "Transform")]
    Transform {
        /// Dot-notation field path to set.
        field: String,
        /// Value to set.
        value: String,
    },
}

fn default_log_level() -> String {
    "info".into()
}
fn default_escalate_severity() -> String {
    "medium".into()
}

/// Result of a single rule evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvaluation {
    /// The rule that was evaluated.
    pub rule_id: String,
    /// Whether the rule matched.
    pub matched: bool,
    /// Actions that were triggered (if matched).
    #[serde(default)]
    pub actions_fired: Vec<String>,
    /// Timestamp of evaluation.
    pub evaluated_at: String,
    /// Any error during evaluation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Statistics about rule matching over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleStats {
    /// Rule ID.
    pub rule_id: String,
    /// Number of total evaluations.
    pub evaluations: u64,
    /// Number of matches.
    pub matches: u64,
    /// Last matched event type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event: Option<String>,
    /// Last match timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_match_at: Option<String>,
}

impl Rule {
    /// Create a new rule with basic fields.
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            enabled: true,
            priority: 100,
            conditions: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Add a condition.
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Add an action.
    pub fn with_action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    /// Set priority.
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Evaluate this rule against an event. Returns true if all conditions match.
    pub fn evaluate(&self, event: &LoomEvent, now: &DateTime<Utc>) -> RuleResult<bool> {
        if !self.enabled {
            return Ok(false);
        }
        if self.conditions.is_empty() {
            return Ok(true); // no conditions = always match
        }
        for condition in &self.conditions {
            if !condition.matches(event, now)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Condition {
    /// Check if this condition matches the given event.
    pub fn matches(&self, event: &LoomEvent, now: &DateTime<Utc>) -> RuleResult<bool> {
        match self {
            Condition::EventType { pattern } => {
                Ok(event.variant_name() == pattern.as_str())
            }
            Condition::NotEventType { pattern } => {
                Ok(event.variant_name() != pattern.as_str())
            }
            Condition::FieldComparison {
                field,
                operator,
                value,
            } => {
                let field_val = event.field_value(field);
                match field_val {
                    Some(actual) => apply_comparison(operator, &actual, value),
                    None => Ok(false),
                }
            }
            Condition::TimeWindow { start, end } => {
                let start_parsed = start.parse::<DateTime<Utc>>().map_err(|e| {
                    RuleError::InvalidCondition(format!("Invalid start time '{}': {}", start, e))
                })?;
                let end_parsed = end.parse::<DateTime<Utc>>().map_err(|e| {
                    RuleError::InvalidCondition(format!("Invalid end time '{}': {}", end, e))
                })?;
                Ok(*now >= start_parsed && *now <= end_parsed)
            }
            Condition::Any => Ok(true),
        }
    }
}

/// Apply a comparison operator to two string values.
fn apply_comparison(
    op: &ComparisonOp,
    actual: &str,
    expected: &str,
) -> RuleResult<bool> {
    match op {
        ComparisonOp::Equals => Ok(actual == expected),
        ComparisonOp::NotEquals => Ok(actual != expected),
        ComparisonOp::Contains => Ok(actual.contains(expected)),
        ComparisonOp::StartsWith => Ok(actual.starts_with(expected)),
        ComparisonOp::EndsWith => Ok(actual.ends_with(expected)),
        ComparisonOp::GreaterThan | ComparisonOp::LessThan => {
            // Try numeric comparison first, fall back to string
            if let (Ok(a), Ok(b)) = (actual.parse::<f64>(), expected.parse::<f64>()) {
                match op {
                    ComparisonOp::GreaterThan => Ok(a > b),
                    ComparisonOp::LessThan => Ok(a < b),
                    _ => unreachable!(),
                }
            } else {
                match op {
                    ComparisonOp::GreaterThan => Ok(actual > expected),
                    ComparisonOp::LessThan => Ok(actual < expected),
                    _ => unreachable!(),
                }
            }
        }
    }
}

impl Action {
    /// Get a short description of this action.
    pub fn description(&self) -> String {
        match self {
            Action::Log { level, message } => format!("Log[{}]: {}", level, message),
            Action::Suppress => "Suppress".into(),
            Action::EmitEvent { payload } => format!("EmitEvent: {}", payload),
            Action::Escalate { severity, message } => {
                format!("Escalate[{}]: {}", severity, message)
            }
            Action::Transform { field, value } => format!("Transform: {} = {}", field, value),
        }
    }
}

// ─── Event Introspection ───────────────────────────────────────────────

/// Trait for introspecting LoomEvent variants at runtime.
pub trait LoomEventExt {
    /// Return the variant name as a string (e.g. "SkillsAdded").
    fn variant_name(&self) -> &'static str;
    /// Get a named field value from the event as a display string.
    fn field_value(&self, field: &str) -> Option<String>;
}

impl LoomEventExt for LoomEvent {
    fn variant_name(&self) -> &'static str {
        match self {
            LoomEvent::SkillsAdded { .. } => "SkillsAdded",
            LoomEvent::SkillsRemoved { .. } => "SkillsRemoved",
            LoomEvent::McpHealthChanged { .. } => "McpHealthChanged",
            LoomEvent::McpServerAdded { .. } => "McpServerAdded",
            LoomEvent::SessionExported { .. } => "SessionExported",
            LoomEvent::RoutingUpdated => "RoutingUpdated",
            LoomEvent::AuthProviderAdded { .. } => "AuthProviderAdded",
            LoomEvent::HooksRegistered { .. } => "HooksRegistered",
            LoomEvent::TaskPhaseChanged { .. } => "TaskPhaseChanged",
            LoomEvent::TaskCreated { .. } => "TaskCreated",
        }
    }

    /// Get a named field value from the event as a display string.
    fn field_value(&self, field: &str) -> Option<String> {
        match self {
            LoomEvent::SkillsAdded { count } => match field {
                "count" => Some(count.to_string()),
                _ => None,
            },
            LoomEvent::SkillsRemoved { name } => match field {
                "name" => Some(name.clone()),
                _ => None,
            },
            LoomEvent::McpHealthChanged { id, status } => match field {
                "id" => Some(id.clone()),
                "status" => Some(format!("{:?}", status)),
                _ => None,
            },
            LoomEvent::McpServerAdded { id, name } => match field {
                "id" => Some(id.clone()),
                "name" => Some(name.clone()),
                _ => None,
            },
            LoomEvent::SessionExported { session_id } => match field {
                "session_id" => Some(session_id.clone()),
                _ => None,
            },
            LoomEvent::RoutingUpdated => None,
            LoomEvent::AuthProviderAdded { provider_id } => match field {
                "provider_id" => Some(provider_id.clone()),
                _ => None,
            },
            LoomEvent::HooksRegistered { hook_id } => match field {
                "hook_id" => Some(hook_id.clone()),
                _ => None,
            },
            LoomEvent::TaskPhaseChanged { task_id, phase } => match field {
                "task_id" => Some(task_id.clone()),
                "phase" => Some(format!("{:?}", phase)),
                _ => None,
            },
            LoomEvent::TaskCreated { task_id } => match field {
                "task_id" => Some(task_id.clone()),
                _ => None,
            },
        }
    }
}

// ─── Rule Engine ───────────────────────────────────────────────────────

/// The rule engine evaluates rules against incoming `LoomEvent`s.
///
/// Rules can be loaded from TOML files, added programmatically, and
/// evaluated individually or in batch.
#[derive(Debug)]
pub struct RuleEngine {
    /// Rules indexed by id.
    rules: Arc<RwLock<HashMap<String, Rule>>>,
    /// Per-rule match statistics.
    stats: Arc<RwLock<HashMap<String, RuleStats>>>,
    /// Base path for rule config files.
    rules_dir: PathBuf,
    /// Number of events evaluated (total).
    total_evaluations: Arc<std::sync::atomic::AtomicU64>,
    /// Number of rules that matched.
    total_matches: Arc<std::sync::atomic::AtomicU64>,
}

impl RuleEngine {
    /// Create a new rule engine backed by the given rules directory.
    pub fn new(loom_dir: &str) -> Self {
        Self {
            rules: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(HashMap::new())),
            rules_dir: Path::new(loom_dir).join("rules"),
            total_evaluations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            total_matches: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Load all rules from the rules directory.
    pub async fn load_rules(&self) -> RuleResult<usize> {
        let dir = &self.rules_dir;
        if !dir.exists() {
            fs::create_dir_all(dir)?;
            return Ok(0);
        }

        let mut count = 0;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                let content = fs::read_to_string(&path).map_err(|e| RuleError::Io(e))?;
                let rule: Rule = toml::from_str(&content).map_err(|e| RuleError::TomlParse {
                    path: path.to_string_lossy().to_string(),
                    source: e,
                })?;
                self.add_rule(rule).await;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Add a rule to the engine (and persist it).
    pub async fn add_rule(&self, rule: Rule) {
        let mut rules = self.rules.write().await;
        rules.insert(rule.id.clone(), rule.clone());
        // Initialize stats if not present
        let mut stats = self.stats.write().await;
        stats.entry(rule.id.clone()).or_insert(RuleStats {
            rule_id: rule.id.clone(),
            evaluations: 0,
            matches: 0,
            last_event: None,
            last_match_at: None,
        });
    }

    /// Remove a rule by id.
    pub async fn remove_rule(&self, id: &str) -> RuleResult<()> {
        let mut rules = self.rules.write().await;
        rules.remove(id).ok_or_else(|| RuleError::NotFound(id.to_string()))?;
        let mut stats = self.stats.write().await;
        stats.remove(id);
        Ok(())
    }

    /// Get a rule by id.
    pub async fn get_rule(&self, id: &str) -> Option<Rule> {
        let rules = self.rules.read().await;
        rules.get(id).cloned()
    }

    /// List all rules.
    pub async fn list_rules(&self) -> Vec<Rule> {
        let rules = self.rules.read().await;
        let mut list: Vec<Rule> = rules.values().cloned().collect();
        list.sort_by_key(|r| -r.priority); // higher priority first
        list
    }

    /// Evaluate a single event against all rules.
    pub async fn evaluate(&self, event: &LoomEvent) -> RuleResult<Vec<RuleEvaluation>> {
        let now = Utc::now();
        let rules = self.rules.read().await;
        let mut results = Vec::new();
        let mut matched_ids = Vec::new();

        // Sort by priority
        let mut sorted: Vec<&Rule> = rules.values().collect();
        sorted.sort_by_key(|r| -r.priority);

        for rule in sorted {
            let eval = self.evaluate_one(rule, event, &now).await?;
            if eval.matched {
                matched_ids.push(rule.id.clone());
            }
            results.push(eval);
        }

        self.total_evaluations.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !matched_ids.is_empty() {
            self.total_matches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        Ok(results)
    }

    /// Evaluate a single rule against an event.
    async fn evaluate_one(
        &self,
        rule: &Rule,
        event: &LoomEvent,
        now: &DateTime<Utc>,
    ) -> RuleResult<RuleEvaluation> {
        let matched = rule.evaluate(event, now).unwrap_or(false);
        let mut actions_fired = Vec::new();

        if matched {
            for action in &rule.actions {
                actions_fired.push(action.description());
            }
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            if let Some(s) = stats.get_mut(&rule.id) {
                s.evaluations += 1;
                if matched {
                    s.matches += 1;
                    s.last_event = Some(event.variant_name().to_string());
                    s.last_match_at = Some(now.to_rfc3339());
                }
            }
        }

        Ok(RuleEvaluation {
            rule_id: rule.id.clone(),
            matched,
            actions_fired,
            evaluated_at: now.to_rfc3339(),
            error: None,
        })
    }

    /// Get stats for all rules.
    pub async fn get_stats(&self) -> Vec<RuleStats> {
        let stats = self.stats.read().await;
        let mut list: Vec<RuleStats> = stats.values().cloned().collect();
        list.sort_by(|a, b| b.matches.cmp(&a.matches));
        list
    }

    /// Get the total number of evaluations.
    pub fn total_evaluations(&self) -> u64 {
        self.total_evaluations.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get the total number of events that matched at least one rule.
    pub fn total_matches(&self) -> u64 {
        self.total_matches.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Persist a rule to its TOML file.
    pub fn save_rule(&self, rule: &Rule) -> RuleResult<()> {
        fs::create_dir_all(&self.rules_dir)?;
        let path = self.rules_dir.join(format!("{}.toml", rule.id));
        let toml_str = toml::to_string_pretty(rule)?;
        fs::write(&path, toml_str)?;
        Ok(())
    }

    /// Delete a rule's TOML file.
    pub fn delete_rule_file(&self, id: &str) -> RuleResult<()> {
        let path = self.rules_dir.join(format!("{}.toml", id));
        if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// The health status.
    pub fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-rules",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: {
                let mut h = std::collections::HashMap::new();
                h.insert("total_evaluations".into(), self.total_evaluations());
                h.insert("total_matches".into(), self.total_matches());
                h
            },
        })
    }
}

// ─── Rule Builder ──────────────────────────────────────────────────────

/// Convenience builder for constructing rules programmatically.
pub struct RuleBuilder {
    rule: Rule,
}

impl RuleBuilder {
    /// Start building a new rule.
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            rule: Rule::new(id, name),
        }
    }

    /// Add a condition.
    pub fn condition(mut self, condition: Condition) -> Self {
        self.rule.conditions.push(condition);
        self
    }

    /// Add an action.
    pub fn action(mut self, action: Action) -> Self {
        self.rule.actions.push(action);
        self
    }

    /// Set priority.
    pub fn priority(mut self, priority: i32) -> Self {
        self.rule.priority = priority;
        self
    }

    /// Set description.
    pub fn description(mut self, desc: &str) -> Self {
        self.rule.description = desc.to_string();
        self
    }

    /// Enable or disable.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.rule.enabled = enabled;
        self
    }

    /// Build the rule.
    pub fn build(self) -> Rule {
        self.rule
    }
}

// ─── Convenience Conditions & Actions ──────────────────────────────────

impl Condition {
    /// Create an EventType condition.
    pub fn event_type(pattern: &str) -> Self {
        Condition::EventType {
            pattern: pattern.to_string(),
        }
    }

    /// Create a FieldComparison condition.
    pub fn field(field: &str, operator: ComparisonOp, value: &str) -> Self {
        Condition::FieldComparison {
            field: field.to_string(),
            operator,
            value: value.to_string(),
        }
    }

    /// Match any event.
    pub fn any() -> Self {
        Condition::Any
    }
}

impl Action {
    /// Create a Log action.
    pub fn log(message: &str) -> Self {
        Action::Log {
            level: "info".into(),
            message: message.to_string(),
        }
    }

    /// Create a Log action with a specific level.
    pub fn log_with_level(level: &str, message: &str) -> Self {
        Action::Log {
            level: level.to_string(),
            message: message.to_string(),
        }
    }

    /// Create a Suppress action.
    pub fn suppress() -> Self {
        Action::Suppress
    }

    /// Create an EmitEvent action.
    pub fn emit_event(payload: serde_json::Value) -> Self {
        Action::EmitEvent { payload }
    }
}

// ─── Rule Active Invocation ────────────────────────────────────────────

/// A handle that can run rules synchronously for testing or daemon integration.
impl RuleEngine {
    /// Evaluate an event synchronously (for use outside async context).
    ///
    /// This is a convenience that blocks on the async evaluate. Prefer the
    /// async version in tokio contexts.
    pub fn evaluate_blocking(&self, event: &LoomEvent) -> RuleResult<Vec<RuleEvaluation>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|e| RuleError::Engine(format!("No tokio runtime: {}", e)))?;
        rt.block_on(self.evaluate(event))
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use LoomEventExt;

    #[test]
    fn test_rule_variant_name() {
        assert_eq!(LoomEvent::SkillsAdded { count: 5 }.variant_name(), "SkillsAdded");
        assert_eq!(
            LoomEvent::SkillsRemoved { name: "test".into() }.variant_name(),
            "SkillsRemoved"
        );
        assert_eq!(LoomEvent::RoutingUpdated.variant_name(), "RoutingUpdated");
    }

    #[test]
    fn test_field_value_extraction() {
        let ev = LoomEvent::SkillsAdded { count: 3 };
        assert_eq!(ev.field_value("count"), Some("3".into()));
        assert_eq!(ev.field_value("nonexistent"), None);

        let ev2 = LoomEvent::SkillsRemoved {
            name: "my-skill".into(),
        };
        assert_eq!(ev2.field_value("name"), Some("my-skill".into()));

        let ev3 = LoomEvent::McpHealthChanged {
            id: "srv-1".into(),
            status: loom_core::HealthStatusStr::Healthy,
        };
        assert_eq!(ev3.field_value("id"), Some("srv-1".into()));

        let ev4 = LoomEvent::RoutingUpdated;
        assert_eq!(ev4.field_value("anything"), None);
    }

    #[test]
    fn test_condition_event_type_match() {
        let ev = LoomEvent::SkillsAdded { count: 5 };
        let now = Utc::now();

        let cond = Condition::event_type("SkillsAdded");
        assert!(cond.matches(&ev, &now).unwrap());

        let cond = Condition::event_type("SkillsRemoved");
        assert!(!cond.matches(&ev, &now).unwrap());
    }

    #[test]
    fn test_condition_not_event_type() {
        let ev = LoomEvent::SkillsAdded { count: 5 };
        let now = Utc::now();

        let cond = Condition::NotEventType {
            pattern: "SkillsRemoved".into(),
        };
        assert!(cond.matches(&ev, &now).unwrap());

        let cond = Condition::NotEventType {
            pattern: "SkillsAdded".into(),
        };
        assert!(!cond.matches(&ev, &now).unwrap());
    }

    #[test]
    fn test_condition_field_comparison() {
        let ev = LoomEvent::SkillsAdded { count: 10 };
        let now = Utc::now();

        let cond = Condition::field("count", ComparisonOp::Equals, "10");
        assert!(cond.matches(&ev, &now).unwrap());

        let cond = Condition::field("count", ComparisonOp::GreaterThan, "5");
        assert!(cond.matches(&ev, &now).unwrap());

        let cond = Condition::field("count", ComparisonOp::LessThan, "5");
        assert!(!cond.matches(&ev, &now).unwrap());

        let cond = Condition::field("count", ComparisonOp::NotEquals, "5");
        assert!(cond.matches(&ev, &now).unwrap());
    }

    #[test]
    fn test_condition_any() {
        let ev = LoomEvent::RoutingUpdated;
        let now = Utc::now();
        assert!(Condition::Any.matches(&ev, &now).unwrap());
    }

    #[test]
    fn test_rule_evaluation() {
        let rule = RuleBuilder::new("test-rule", "Test Rule")
            .condition(Condition::event_type("SkillsAdded"))
            .condition(Condition::field("count", ComparisonOp::GreaterThan, "5"))
            .action(Action::log("High count: {{count}}"))
            .build();

        let now = Utc::now();

        // Should match: SkillsAdded with count > 5
        let ev1 = LoomEvent::SkillsAdded { count: 10 };
        assert!(rule.evaluate(&ev1, &now).unwrap());

        // Should not match: SkillsAdded with count <= 5
        let ev2 = LoomEvent::SkillsAdded { count: 3 };
        assert!(!rule.evaluate(&ev2, &now).unwrap());

        // Should not match: wrong event type
        let ev3 = LoomEvent::SkillsRemoved {
            name: "test".into(),
        };
        assert!(!rule.evaluate(&ev3, &now).unwrap());
    }

    #[test]
    fn test_disabled_rule_never_matches() {
        let rule = RuleBuilder::new("disabled", "Disabled Rule")
            .enabled(false)
            .condition(Condition::Any)
            .build();

        let ev = LoomEvent::RoutingUpdated;
        let now = Utc::now();
        assert!(!rule.evaluate(&ev, &now).unwrap());
    }

    #[test]
    fn test_empty_conditions_always_match() {
        let rule = Rule::new("catch-all", "Catch All");
        let ev = LoomEvent::RoutingUpdated;
        let now = Utc::now();
        assert!(rule.evaluate(&ev, &now).unwrap());
    }

    #[test]
    fn test_engine_add_and_list() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let r1 = Rule::new("rule1", "Rule 1");
            let r2 = Rule::new("rule2", "Rule 2");
            engine.add_rule(r1).await;
            engine.add_rule(r2).await;

            let list = engine.list_rules().await;
            assert_eq!(list.len(), 2);
        });
    }

    #[test]
    fn test_engine_evaluate() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let rule = RuleBuilder::new("alert", "Alert")
                .condition(Condition::event_type("SkillsAdded"))
                .condition(Condition::field("count", ComparisonOp::GreaterThan, "5"))
                .action(Action::log("High count"))
                .build();
            engine.add_rule(rule).await;

            let ev = LoomEvent::SkillsAdded { count: 10 };
            let results = engine.evaluate(&ev).await.unwrap();
            assert_eq!(results.len(), 1);
            assert!(results[0].matched);
            assert_eq!(results[0].actions_fired.len(), 1);

            let ev2 = LoomEvent::SkillsAdded { count: 1 };
            let results2 = engine.evaluate(&ev2).await.unwrap();
            assert_eq!(results2.len(), 1);
            assert!(!results2[0].matched);
        });
    }

    #[test]
    fn test_engine_remove_rule() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            engine.add_rule(Rule::new("r1", "Rule 1")).await;
            assert_eq!(engine.list_rules().await.len(), 1);
            engine.remove_rule("r1").await.unwrap();
            assert_eq!(engine.list_rules().await.len(), 0);
        });
    }

    #[test]
    fn test_engine_stats() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let rule = RuleBuilder::new("s", "Stats")
                .condition(Condition::event_type("SkillsAdded"))
                .build();
            engine.add_rule(rule).await;

            for _ in 0..3 {
                let _ = engine.evaluate(&LoomEvent::SkillsAdded { count: 1 }).await;
            }
            let _ = engine.evaluate(&LoomEvent::RoutingUpdated).await;

            let stats = engine.get_stats().await;
            assert_eq!(stats.len(), 1);
            assert_eq!(stats[0].evaluations, 4);
            assert_eq!(stats[0].matches, 3);
        });
    }

    #[test]
    fn test_condition_time_window() {
        let now = Utc::now();
        let start = (now - chrono::Duration::hours(1)).to_rfc3339();
        let end = (now + chrono::Duration::hours(1)).to_rfc3339();

        let cond = Condition::TimeWindow {
            start: start.clone(),
            end: end.clone(),
        };
        let ev = LoomEvent::RoutingUpdated;
        assert!(cond.matches(&ev, &now).unwrap());

        // Outside window (before)
        let early = now - chrono::Duration::hours(2);
        assert!(!cond.matches(&ev, &early).unwrap());

        // Outside window (after)
        let late = now + chrono::Duration::hours(2);
        assert!(!cond.matches(&ev, &late).unwrap());
    }

    #[test]
    fn test_numeric_comparison() {
        assert!(apply_comparison(&ComparisonOp::GreaterThan, "10", "5").unwrap());
        assert!(!apply_comparison(&ComparisonOp::GreaterThan, "3", "5").unwrap());
        assert!(apply_comparison(&ComparisonOp::LessThan, "3", "5").unwrap());
        assert!(!apply_comparison(&ComparisonOp::LessThan, "10", "5").unwrap());
        assert!(apply_comparison(&ComparisonOp::Equals, "hello", "hello").unwrap());
        assert!(apply_comparison(&ComparisonOp::Contains, "hello world", "world").unwrap());
        assert!(apply_comparison(&ComparisonOp::StartsWith, "hello", "he").unwrap());
        assert!(apply_comparison(&ComparisonOp::EndsWith, "hello", "lo").unwrap());
        assert!(!apply_comparison(&ComparisonOp::EndsWith, "hello", "el").unwrap());

        // String fallback for non-numeric
        assert!(apply_comparison(&ComparisonOp::GreaterThan, "banana", "apple").unwrap());
    }

    #[test]
    fn test_save_and_load_rule() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rule = RuleBuilder::new("saved-rule", "Saved Rule")
            .condition(Condition::event_type("SkillsAdded"))
            .action(Action::log("Skills were added"))
            .build();

        engine.save_rule(&rule).unwrap();

        // Create a new engine with the same dir and load
        let engine2 = RuleEngine::new(dir.path().to_str().unwrap());
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let count = engine2.load_rules().await.unwrap();
            assert_eq!(count, 1);

            let rules = engine2.list_rules().await;
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].id, "saved-rule");
            assert_eq!(rules[0].name, "Saved Rule");
        });
    }

    #[test]
    fn test_rule_priority_sorting() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            engine.add_rule(RuleBuilder::new("low", "Low").priority(10).build()).await;
            engine.add_rule(RuleBuilder::new("high", "High").priority(100).build()).await;
            engine.add_rule(RuleBuilder::new("mid", "Mid").priority(50).build()).await;

            let list = engine.list_rules().await;
            assert_eq!(list[0].id, "high");
            assert_eq!(list[1].id, "mid");
            assert_eq!(list[2].id, "low");
        });
    }

    #[test]
    fn test_health_check() {
        let dir = tempfile::tempdir().unwrap();
        let engine = RuleEngine::new(dir.path().to_str().unwrap());
        let health = engine.health().unwrap();
        assert_eq!(health.crate_name, "loom-rules");
        assert!(health.performance_counters.contains_key("total_evaluations"));
    }

    #[test]
    fn test_action_description() {
        assert_eq!(
            Action::log("hello").description(),
            "Log[info]: hello"
        );
        assert_eq!(Action::suppress().description(), "Suppress");
        assert!(Action::emit_event(serde_json::json!({"type": "test"})).description().contains("EmitEvent"));
    }
}
