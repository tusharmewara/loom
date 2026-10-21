use loom_core::{ComplexityScore, RoutingDecision};
use std::fmt::Write;

/// Inject routing context into the agent's prompt prefix.
///
/// This provides the agent with awareness of task complexity, recommended model,
/// and cost estimate, enabling it to adjust its reasoning depth and tool usage.
pub fn inject_routing_context(task: &ComplexityScore, decision: &RoutingDecision) -> String {
    let mut ctx = String::new();

    // Only inject if we have both score and decision
    if task.numeric == 0.0 && decision.estimated_cost_usd == 0.0 {
        return ctx;
    }

    write!(
        ctx,
        "[ROUTING]\nTask complexity: {} (score: {:.2})\nRecommended tier: {}\nRecommended model: {} ({})\n",
        format!("{:?}", task.level).to_uppercase(),
        task.numeric,
        format!("{:?}", decision.recommended_tier).to_uppercase(),
        decision.recommended_model,
        decision.recommended_provider
    )
    .unwrap();

    write!(
        ctx,
        "Estimated tokens: {} input / {} output\nEstimated cost: ${:.3f} ({} estimate, method: {})\n",
        decision.cost_estimate.input_tokens_estimated,
        decision.cost_estimate.output_tokens_estimated,
        decision.cost_estimate.estimated_cost_usd,
        decision.cost_estimate.source.as_str(),
        if decision.cost_estimate.method.is_empty() {
            "n/a"
        } else {
            &decision.cost_estimate.method
        }
    )
    .unwrap();

    if !decision.reason.is_empty() {
        writeln!(ctx, "Reason: {}", decision.reason).unwrap();
    }

    // Provide hint for override if needed
    write!(
        ctx,
        "\nOverride: 'uae routing override <task-id> --tier <slm|mid|flagship>' to change.\n"
    )
    .unwrap();

    ctx
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{ComplexityLevel, ComplexitySignals, CostEstimate, ModelTier};

    fn test_score() -> ComplexityScore {
        ComplexityScore {
            level: ComplexityLevel::High,
            numeric: 0.82,
            model_tier: ModelTier::Flagship,
            scored_at: "2026-05-20T00:00:00Z".to_string(),
            signals: ComplexitySignals {
                description_length: 250,
                keyword_indicators: vec!["refactor".to_string()],
                has_cross_module_refs: true,
                dependency_depth: 3,
                scope_file_count: Some(8),
                agent_role: Some("trellis-implement".to_string()),
                files_modified_count: None,
                prd_length: None,
                context_lines_count: None,
                phase_iteration_count: None,
                error_rate: None,
                user_override: None,
                knowledge_entities: Vec::new(),
            },
        }
    }

    fn test_decision() -> RoutingDecision {
        RoutingDecision {
            recommended_tier: ModelTier::Flagship,
            recommended_provider: "anthropic".to_string(),
            recommended_model: "claude-opus-4".to_string(),
            reason: "High complexity due to cross-module refactor".to_string(),
            overrides_role_default: false,
            decided_at: "2026-05-20T00:00:00Z".to_string(),
            cost_estimate: CostEstimate {
                input_tokens_estimated: 15000,
                output_tokens_estimated: 5000,
                estimated_cost_usd: 0.60,
                confidence: "high".to_string(),
                source: loom_core::UsageSource::Estimated,
                method: "chars/4".to_string(),
            },
        }
    }

    #[test]
    fn test_inject_context() {
        let ctx = inject_routing_context(&test_score(), &test_decision());
        assert!(ctx.contains("HIGH"));
        assert!(ctx.contains("0.82"));
        assert!(ctx.contains("FLAGSHIP"));
        assert!(ctx.contains("claude-opus-4"));
        assert!(ctx.contains("anthropic"));
        assert!(ctx.contains("$0.600"));
        assert!(ctx.contains("cross-module refactor"));
        // The number says what it is.
        assert!(ctx.contains("estimated estimate"));
        assert!(ctx.contains("chars/4"));
    }
}
