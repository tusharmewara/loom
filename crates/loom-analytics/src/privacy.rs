//! Privacy invariant tests for the control-plane analytics store.
//!
//! The core privacy invariant: the control plane never stores per-run data.
//! All data stored is aggregated per org+tool+day. These tests verify
//! that the privacy invariant holds by direct database inspection.

#[cfg(test)]
mod tests {
    use crate::store::AnalyticsStore;

    /// Verifies that no per-run table exists in the analytics database.
    ///
    /// This is the core privacy invariant of phase 5: the control plane never
    /// stores per-run data. All data is aggregated per org+tool+day. If a
    /// `usage_runs` table (or any per-run table) existed, this test would fail.
    pub fn privacy_invariant_holds() -> bool {
        let dir = tempfile::tempdir().unwrap();
        let store = AnalyticsStore::new(dir.path().to_str().unwrap()).unwrap();
        !store.has_per_run_rows()
    }

    #[test]
    fn no_per_run_table_exists() {
        assert!(privacy_invariant_holds());
    }

    #[test]
    fn store_has_no_per_run_table() {
        let (store, _dir) = test_store();
        assert!(!store.has_per_run_rows());
    }

    fn test_store() -> (AnalyticsStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AnalyticsStore::new(dir.path().to_str().unwrap()).unwrap();
        (store, dir)
    }
}
