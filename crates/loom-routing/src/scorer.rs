use loom_core::{ComplexityLevel, ComplexityScore, ComplexitySignals, ModelTier};
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

/// Signals extracted from prompt text (static + dynamic).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Signals {
    // Static signals
    #[serde(default)]
    pub description_length: usize,
    #[serde(default)]
    pub keyword_indicators: Vec<String>,
    #[serde(default)]
    pub has_cross_module_refs: bool,
    #[serde(default)]
    pub dependency_depth: u8,
    #[serde(default)]
    pub scope_file_count: Option<usize>,
    #[serde(default)]
    pub agent_role: Option<String>,

    // Dynamic signals (collected at phase transition)
    #[serde(default)]
    pub files_modified_count: usize,
    #[serde(default)]
    pub prd_length: Option<usize>,
    #[serde(default)]
    pub context_lines_count: Option<u32>,
    #[serde(default)]
    pub phase_iteration_count: u8,
    #[serde(default)]
    pub error_rate: Option<f64>,

    // User override
    #[serde(default)]
    pub user_override: Option<ComplexityLevel>,

    // Knowledge graph context
    #[serde(default)]
    pub knowledge_entities: Vec<String>,
}

/// Dynamic signals collected at phase transition (backward compat).
pub type DynamicSignals = Signals;

/// Static signals extracted from task description (backward compat).
pub type StaticSignals = Signals;

/// Extract signals from prompt text.
pub fn extract_signals(prompt: &str, agent_role: Option<&str>) -> Signals {
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
        let count = lower.matches("layer").count()
            + lower.matches("through").count()
            + lower.matches("chain").count();
        (count as u8).min(5)
    } else {
        0
    };

    let scope_file_count = {
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
        }
    };

    Signals {
        description_length,
        keyword_indicators,
        has_cross_module_refs,
        dependency_depth,
        scope_file_count,
        agent_role: agent_role.map(String::from),
        ..Default::default()
    }
}

/// Unified complexity scoring from signals.
/// If `dynamic` is provided, dynamic signals are merged in.
pub fn score(signals: &Signals, dynamic: Option<&Signals>) -> ComplexityScore {
    let mut numeric = 0.0;

    // description_length (weight 0.15) - normalized to [0,1]
    numeric += (signals.description_length as f64 / 2000.0).clamp(0.0, 1.0) * 0.15;

    // keyword_indicators (weight 0.35)
    let mut keyword_score: f64 = 0.5;
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
            r if r.contains("brainstorm") => 0.2,
            r if r.contains("research") => 0.5,
            r if r.contains("implement") => 0.8,
            r if r.contains("check") => 0.5,
            r if r.contains("update-spec") => 0.2,
            _ => 0.5,
        };
        numeric += role_score * 0.10;
    }

    // Dynamic signals (additive)
    if let Some(dyn_sig) = dynamic {
        if let Some(prd_len) = dyn_sig.prd_length {
            numeric += (prd_len as f64 / 5000.0).clamp(0.0, 0.15);
        }
        if let Some(ctx_lines) = dyn_sig.context_lines_count {
            numeric += (ctx_lines as f64 / 2000.0).clamp(0.0, 0.15);
        }
        if dyn_sig.files_modified_count > 0 {
            numeric += (dyn_sig.files_modified_count as f64 / 10.0).clamp(0.0, 0.20);
        }
        if dyn_sig.phase_iteration_count > 0 {
            numeric += (dyn_sig.phase_iteration_count as f64 / 3.0).clamp(0.0, 0.20);
        }
        if let Some(err_rate) = dyn_sig.error_rate {
            numeric += err_rate.clamp(0.0, 0.5) * 0.15;
        }
    }

    // User override has highest priority
    let level = if let Some(override_level) = signals.user_override {
        override_level
    } else {
        numeric = numeric.clamp(0.0, 1.0);
        if numeric < 0.35 {
            ComplexityLevel::Low
        } else if numeric < 0.7 {
            ComplexityLevel::Medium
        } else {
            ComplexityLevel::High
        }
    };

    let model_tier = match level {
        ComplexityLevel::Low => ModelTier::Slm,
        ComplexityLevel::Medium => ModelTier::Mid,
        ComplexityLevel::High => ModelTier::Flagship,
    };

    // Build full signals for output
    let mut full_signals = ComplexitySignals {
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
        user_override: signals.user_override,
        knowledge_entities: signals.knowledge_entities.clone(),
    };

    if let Some(dyn_sig) = dynamic {
        full_signals.files_modified_count = Some(dyn_sig.files_modified_count);
        full_signals.prd_length = dyn_sig.prd_length;
        full_signals.context_lines_count = dyn_sig.context_lines_count;
        full_signals.phase_iteration_count = Some(dyn_sig.phase_iteration_count);
        full_signals.error_rate = dyn_sig.error_rate;
    }

    ComplexityScore {
        level,
        numeric,
        model_tier,
        scored_at: loom_core::rfc3339_now(),
        signals: full_signals,
    }
}

/// Simple static scoring from prompt text (backward compat).
pub fn score_static(prompt: &str) -> ComplexityScore {
    let signals = extract_signals(prompt, None);
    score(&signals, None)
}

/// Static scoring with agent role (backward compat).
pub fn score_static_signals(signals: &Signals) -> ComplexityScore {
    score(signals, None)
}

/// Dynamic scoring with both static and dynamic signals (backward compat).
pub fn score_dynamic_signals(
    static_signals: &Signals,
    dynamic_signals: &Signals,
) -> ComplexityScore {
    score(static_signals, Some(dynamic_signals))
}

/// Dynamic scoring hook — delegates to static with optional context boost (backward compat).
pub fn score_dynamic(prompt: &str, context_token_estimate: u32) -> ComplexityScore {
    let mut score = score_static(prompt);
    if context_token_estimate > 8000 {
        score.numeric = (score.numeric + 0.15).min(1.0);
        if matches!(score.level, ComplexityLevel::Low) {
            score.level = ComplexityLevel::Medium;
            score.model_tier = ModelTier::Mid;
        }
    }
    score.scored_at = loom_core::rfc3339_now();
    score
}

/// Backward compat: extract static signals
pub fn extract_static_signals(prompt: &str, agent_role: Option<&str>) -> Signals {
    extract_signals(prompt, agent_role)
}
