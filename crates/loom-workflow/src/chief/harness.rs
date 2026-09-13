//! Chief/worker harness role selection.
//!
//! The Chief planner and the worker pool are chosen from the *installed*
//! harnesses, ranked by a preference list. The chief defaults to the best
//! planner; every other installed harness is a potential worker.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessRole {
    /// Decomposition/planning harness — planner-only, structured output.
    Chief,
    /// Task execution harness — runs one task in one worktree.
    Worker,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HarnessSelection {
    pub chief: String,
    pub workers: Vec<String>,
}

/// Picks a chief + worker set from installed harness names.
#[allow(dead_code)] // fields read only after `select` is implemented (Task 3 TDD)
pub struct HarnessSelector<'a> {
    preferences: &'a [&'a str],
    installations: Vec<String>,
}

impl<'a> HarnessSelector<'a> {
    /// Preference order for the chief role (highest first), e.g.
    /// `["codex", "claude-code", "opencode"]`.
    pub fn new(preferences: &'a [&'a str]) -> Self {
        Self {
            preferences,
            installations: Vec::new(),
        }
    }

    /// Feed the set of installed/available harness names.
    pub fn installations<I>(mut self, installations: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.installations = installations.into_iter().map(Into::into).collect();
        self
    }

    /// The first installed harness in preference order becomes chief; the rest
    /// become workers (sorted for determinism).
    pub fn select(&self) -> HarnessSelection {
        unimplemented!("HarnessSelector::select")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_preferred_installed_chief() {
        let sel = HarnessSelector::new(&["codex", "claude-code"])
            .installations(vec!["goose", "jcode", "codex", "crush"])
            .select();
        assert_eq!(sel.chief, "codex");
        let workers: Vec<&str> = sel.workers.iter().map(|w| w.as_str()).collect();
        assert!(workers.contains(&"goose"));
        assert!(workers.contains(&"jcode"));
        assert!(!workers.contains(&"codex"));
        assert_eq!(workers.len(), 3);
    }

    #[test]
    fn empty_installations_yields_none_chief_and_no_workers() {
        let sel = HarnessSelector::new(&["codex", "claude-code"])
            .installations(std::iter::empty::<&str>())
            .select();
        assert_eq!(sel.chief, "");
        assert!(sel.workers.is_empty());
    }

    #[test]
    fn no_preferred_harness_installed_means_no_chief() {
        let sel = HarnessSelector::new(&["codex", "claude-code"])
            .installations(vec!["goose", "jcode"])
            .select();
        assert_eq!(sel.chief, "");
        assert_eq!(sel.workers.len(), 2);
    }
}
