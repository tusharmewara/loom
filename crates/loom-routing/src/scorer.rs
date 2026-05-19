use loom_core::{ComplexityLevel, ComplexityScore, ModelTier};
use serde::{Deserialize, Serialize};

const HIGH_COMPLEXITY_KEYWORDS: &[&str] = &[
    "architect",
    "refactor",
    "security",
    "distributed",
    "migrate",
    "design system",
    "performance",
    "multi-file",
    "cross-module",
    "architecture",
    "redesign",
    "rewrite",
    "critical",
];

const LOW_COMPLEXITY_KEYWORDS: &[&str] = &[
    "rename",
    "typo",
    "comment",
    "format",
    "lint",
    "simple",
    "fix test",
    "bump version",
    "update config",
    "add logging",
];

/// Static signals extracted from task description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticSignals {
    pub description_length: usize,
    #[serde(default)]
    pub keyword_indicators: Vec<String>,
    #[serde(default)]
    pub has_cross_module_refs: bool,
    #[serde(default)]
    pub dependency_depth: u8,
    pub scope_file_count: Option<usize>,
    pub agent_role: Option<String>,
}

/// Dynamic signals collected at phase transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicSignals {
    #[serde(default)]
    pub files_modified_count: usize,
    pub prd_length: Option<usize>,
    pub context_lines_count: Option<u32>,
    #[serde(default)]
    pub phase_iteration_count: u8,
    pub error_rate: Option<f64>,
}

/// Extract static signals from prompt text.
pub fn extract_static_signals(prompt: &str, agent_role: Option<&str>) -> StaticSignals {
    let description_length = prompt.len();
    let lower = prompt.to_lowercase();
    let mut keyword_indicators = Vec::new();

    for kw in HIGH_COMPLEXITY_KEYWORDS {
        if lower.contains(kw) {
            keyword_indicators.push(kw.to_string());
        }
    }
    for kw in LOW_COMPLEXITY_KEYWORDS {
        if lower.contains(kw) {
            keyword_indicators.push(kw.to_string());
        }
    }

    let has_cross_module_refs = lower.contains("cross-module")
        || lower.contains("across modules")
        || lower.contains("multiple services")
        || lower.contains("inter-module");

    let dependency_depth = if lower.contains("deep") || lower.contains("through") {
        // crude estimate: count occurrences of "layer" or mentions of depth
        let count = lower.matches("layer").count()
            + lower.matches("through").count()
            + lower.matches("chain").count();
        (count as u8).min(5)
    } else {
        0
    };

    let scope_file_count = {
        // Look for file glob patterns like src/**/*.rs, tests/*.py, etc.
        let glob_patterns = [r"src/**/*", r"tests/*", r"lib/**", r"app/*"];
        let mut count = 0;
        for pattern in glob_patterns.iter() {
            if prompt.contains(pattern) {
                count += 1;
            }
        }
        if count > 0 {
            Some(count * 3)
        } else {
            None
        } // rough multiplier
    };

    StaticSignals {
        description_length,
        keyword_indicators,
        has_cross_module_refs,
        dependency_depth,
        scope_file_count,
        agent_role: agent_role.map(String::from),
    }
}

/// Static complexity scoring from signals (no LLM).
pub fn score_static_signals(signals: &StaticSignals) -> ComplexityScore {
    let mut numeric = 0.0;

    // description_length (weight 0.15) - normalized to [0,1]
    numeric += (signals.description_length as f64 / 2000.0).clamp(0.0, 1.0) * 0.15;

    // keyword_indicators (weight 0.35)
    let mut keyword_score: f64 = 0.5; // neutral baseline
    for kw in &signals.keyword_indicators {
        if HIGH_COMPLEXITY_KEYWORDS.contains(&kw.as_str()) {
            keyword_score += 0.15;
        }
        if LOW_COMPLEXITY_KEYWORDS.contains(&kw.as_str()) {
            keyword_score -= 0.1;
        }
    }
    keyword_score = keyword_score.clamp(0.0, 1.0);
    numeric += keyword_score * 0.35;

    // has_cross_module_refs (weight 0.20)
    if signals.has_cross_module_refs {
        numeric += 0.20;
    }

    // dependency_depth (weight 0.15) - normalized to [0,1]
    numeric += (signals.dependency_depth as f64 / 5.0).clamp(0.0, 1.0) * 0.15;

    // scope_file_count (weight 0.15)
    if let Some(count) = signals.scope_file_count {
        numeric += (count as f64 / 10.0).clamp(0.0, 1.0) * 0.15;
    }

    // agent_role default tier (weight 0.10) - baseline based on role
    if let Some(role) = &signals.agent_role {
        let role_score = match role {
            r if r.contains("brainstorm") => 0.2,  // SLM default
            r if r.contains("research") => 0.5,    // Mid default
            r if r.contains("implement") => 0.8,   // Flagship default
            r if r.contains("check") => 0.5,       // Mid default
            r if r.contains("update-spec") => 0.2, // SLM default
            _ => 0.5,
        };
        numeric += role_score * 0.10;
    }

    numeric = numeric.clamp(0.0, 1.0);

    let level = if numeric < 0.35 {
        ComplexityLevel::Low
    } else if numeric < 0.7 {
        ComplexityLevel::Medium
    } else {
        ComplexityLevel::High
    };

    let model_tier = match level {
        ComplexityLevel::Low => ModelTier::Slm,
        ComplexityLevel::Medium => ModelTier::Mid,
        ComplexityLevel::High => ModelTier::Flagship,
    };

    ComplexityScore {
        level,
        numeric,
        model_tier,
        scored_at: super::util::rfc3339_now(),
        signals: loom_core::ComplexitySignals {
            description_length: signals.description_length,
            keyword_indicators: signals.keyword_indicators.clone(),
            has_cross_module_refs: signals.has_cross_module_refs,
            dependency_depth: signals.dependency_depth,
            scope_file_count: signals.scope_file_count,
            agent_role: signals.agent_role.clone(),
            files_modified_count: None,
            prd_length: None,
            context_lines_count: None,
            phase_iteration_count: None,
            error_rate: None,
            user_override: None,
        },
    }
}

/// Simple static scoring from prompt text (convenience wrapper).
pub fn score_static(prompt: &str) -> ComplexityScore {
    let signals = extract_static_signals(prompt, None);
    score_static_signals(&signals)
}

/// Dynamic scoring with both static and dynamic signals.
pub fn score_dynamic_signals(
    static_signals: &StaticSignals,
    dynamic_signals: &DynamicSignals,
) -> ComplexityScore {
    let score = score_static_signals(static_signals);
    let mut numeric = score.numeric;

    // dynamic signals influence
    if let Some(prd_len) = dynamic_signals.prd_length {
        // weight: 0.15, longer PRDs indicate complexity
        numeric += (prd_len as f64 / 5000.0).clamp(0.0, 0.15);
    }

    if let Some(ctx_lines) = dynamic_signals.context_lines_count {
        numeric += (ctx_lines as f64 / 2000.0).clamp(0.0, 0.15);
    }

    if dynamic_signals.files_modified_count > 0 {
        numeric += (dynamic_signals.files_modified_count as f64 / 10.0).clamp(0.0, 0.20);
    }

    if dynamic_signals.phase_iteration_count > 0 {
        numeric += (dynamic_signals.phase_iteration_count as f64 / 3.0).clamp(0.0, 0.20);
    }

    if let Some(err_rate) = dynamic_signals.error_rate {
        numeric += err_rate.clamp(0.0, 0.5) * 0.15;
    }

    numeric = numeric.clamp(0.0, 1.0);

    let level = if numeric < 0.35 {
        ComplexityLevel::Low
    } else if numeric < 0.7 {
        ComplexityLevel::Medium
    } else {
        ComplexityLevel::High
    };

    let model_tier = match level {
        ComplexityLevel::Low => ModelTier::Slm,
        ComplexityLevel::Medium => ModelTier::Mid,
        ComplexityLevel::High => ModelTier::Flagship,
    };

    // Update the signals with dynamic values
    let mut full_signals = score.signals.clone();
    full_signals.files_modified_count = Some(dynamic_signals.files_modified_count);
    full_signals.prd_length = dynamic_signals.prd_length;
    full_signals.context_lines_count = dynamic_signals.context_lines_count;
    full_signals.phase_iteration_count = Some(dynamic_signals.phase_iteration_count);
    full_signals.error_rate = dynamic_signals.error_rate;

    ComplexityScore {
        level,
        numeric,
        model_tier,
        scored_at: super::util::rfc3339_now(),
        signals: full_signals,
    }
}

/// Dynamic scoring hook — currently delegates to static with optional context boost.
pub fn score_dynamic(prompt: &str, context_token_estimate: u32) -> ComplexityScore {
    let mut score = score_static(prompt);
    if context_token_estimate > 8000 {
        score.numeric = (score.numeric + 0.15).min(1.0);
        if matches!(score.level, ComplexityLevel::Low) {
            score.level = ComplexityLevel::Medium;
            score.model_tier = ModelTier::Mid;
        }
    }
    score.scored_at = super::util::rfc3339_now();
    score
}
