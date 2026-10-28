//! Sync report — outcome of one `OrgSync::sync()` cycle.

/// Outcome of one pull + reconcile cycle.
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    /// Items inserted or updated in local registries.
    pub upserts: usize,
    /// Items skipped because local copy differs from org (local wins).
    pub skips: usize,
    /// Items marked retired-by-org (present locally, absent from org).
    pub retirements: usize,
    /// Items restored from retirement (org re-added them).
    pub restorations: usize,
}

impl SyncReport {
    pub fn is_empty(&self) -> bool {
        self.upserts == 0 && self.skips == 0 && self.retirements == 0 && self.restorations == 0
    }
}
