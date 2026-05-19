//! In-memory hook index for filtering and tests.
//!
//! Production execution uses [`crate::HookRunner`]; this type is kept for
//! lightweight filtering and backward-compatible APIs.

use crate::registry::HookRegistry;
use loom_core::{HookEntry, HookEventType};

pub struct HookEngine {
    hooks: Vec<HookEntry>,
}

impl HookEngine {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn from_entries(entries: Vec<HookEntry>) -> Self {
        Self { hooks: entries }
    }

    /// Build an in-memory view from the on-disk registry.
    pub fn from_registry(registry: &HookRegistry) -> Self {
        Self::from_entries(registry.list())
    }

    pub fn register(&mut self, hook: HookEntry) {
        self.hooks.push(hook);
    }

    pub fn get_for_event(&self, event: HookEventType) -> Vec<&HookEntry> {
        self.hooks
            .iter()
            .filter(|h| h.event_type == event)
            .collect()
    }

    pub fn entries(&self) -> &[HookEntry] {
        &self.hooks
    }
}
