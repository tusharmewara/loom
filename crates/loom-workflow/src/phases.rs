use loom_core::{ModelTier, TaskPhase};

/// Default model tier for each workflow phase.
pub fn default_model_tier(phase: &TaskPhase) -> ModelTier {
    match phase {
        TaskPhase::Plan => ModelTier::Flagship,
        TaskPhase::Implement => ModelTier::Mid,
        TaskPhase::Verify => ModelTier::Mid,
        TaskPhase::Finish => ModelTier::Slm,
    }
}

/// Advance to the next phase in the workflow pipeline.
pub fn next_phase(current: &TaskPhase) -> Option<TaskPhase> {
    match current {
        TaskPhase::Plan => Some(TaskPhase::Implement),
        TaskPhase::Implement => Some(TaskPhase::Verify),
        TaskPhase::Verify => Some(TaskPhase::Finish),
        TaskPhase::Finish => None,
    }
}
