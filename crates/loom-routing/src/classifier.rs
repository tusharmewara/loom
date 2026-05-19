use crate::scorer::{self, DynamicSignals, StaticSignals};
use loom_core::ComplexityScore;

pub struct Classifier;

impl Classifier {
    pub fn new() -> Self {
        Self {}
    }

    /// Basic static scoring from description text only.
    pub fn classify(&self, prompt: &str) -> ComplexityScore {
        let signals = scorer::extract_static_signals(prompt, None);
        scorer::score_static_signals(&signals)
    }

    /// Static scoring with agent role context.
    pub fn classify_with_role(&self, prompt: &str, agent_role: Option<&str>) -> ComplexityScore {
        let signals = scorer::extract_static_signals(prompt, agent_role);
        scorer::score_static_signals(&signals)
    }

    /// Dynamic scoring using static + dynamic signals (phase transition).
    pub fn classify_dynamic(
        &self,
        prompt: &str,
        agent_role: Option<&str>,
        dynamic_signals: &DynamicSignals,
    ) -> ComplexityScore {
        let static_signals = scorer::extract_static_signals(prompt, agent_role);
        scorer::score_dynamic_signals(&static_signals, dynamic_signals)
    }
}
