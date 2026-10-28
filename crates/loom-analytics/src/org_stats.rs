//! Org-level analytics aggregates.
//!
//! Aggregates are computed per-org, per-tool, per-day from pushed-only data.
//! No per-run data is ever stored on the control plane.

use serde::{Deserialize, Serialize};

/// One aggregated data point in the org registry.
///
/// Represents a single row in the `org_analytics` table: one org, one tool,
/// one day, with aggregated counts/seats/costs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgAggregate {
    pub org_id: String,
    pub tool: String,
    pub day: String,  // YYYY-MM-DD
    pub runs: u64,
    pub measured_runs: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub est_cost_usd: Option<f64>,
}

/// Aggregate of one tool across all days within an org.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAggregate {
    pub tool: String,
    pub runs: u64,
    pub measured_runs: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub est_cost_usd: Option<f64>,
    /// Calibration ratio: measured cost / estimated cost.
    /// None if no measured cost or no estimated cost.
    pub calibration_ratio: Option<f64>,
}

/// Aggregate analytics for an org, broken down by tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsAggregate {
    pub org_id: String,
    pub tools: Vec<ToolAggregate>,
    /// Computed totals across all tools.
    pub totals: OrgAggregate,
}

impl AnalyticsAggregate {
    /// Build an `AnalyticsAggregate` from a list of per-tool aggregates.
    pub fn new(org_id: String, tools: Vec<ToolAggregate>) -> Self {
        let mut totals = OrgAggregate {
            org_id: org_id.clone(),
            tool: "total".to_string(),
            day: String::new(),
            runs: 0,
            measured_runs: 0,
            input_tokens: None,
            output_tokens: None,
            cost_usd: None,
            est_cost_usd: None,
        };

        for tool in &tools {
            totals.runs += tool.runs;
            totals.measured_runs += tool.measured_runs;
            totals.input_tokens = match (totals.input_tokens, tool.input_tokens) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            totals.output_tokens = match (totals.output_tokens, tool.output_tokens) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            totals.cost_usd = match (totals.cost_usd, tool.cost_usd) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            totals.est_cost_usd = match (totals.est_cost_usd, tool.est_cost_usd) {
                (Some(a), Some(b)) => Some(a + b),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
        }

        Self {
            org_id,
            tools,
            totals,
        }
    }
}
