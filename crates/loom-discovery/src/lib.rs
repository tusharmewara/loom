pub mod scanner;
pub mod registry;
pub mod config_reader;
pub mod initializer;
pub mod merger;
pub mod watcher;

use loom_core::HealthCheck;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// Two-phase discovery engine.
///
/// Phase 1 (<3ms): Read SQLite snapshot → return cached state
/// Phase 2 (30-50ms): Validate entries → merge conflicts → update snapshot
///
/// CLI with daemon: Phase 1 only (daemon handles Phase 2)
/// CLI without daemon: Phase 1 + Phase 2 (slightly slower but functional)
pub struct DiscoveryEngine {
    loom_dir: String,
    snapshot: Arc<registry::SnapshotDb>,
    watcher: Arc<watcher::DiscoveryWatcher>,
}

pub struct Phase1Result {
    pub resource_counts: std::collections::HashMap<String, usize>,
    pub layers: Vec<String>,
    pub ecosystem_version: String,
    pub phase_1_duration_us: u64,
}

pub struct Phase2Result {
    pub entries_validated: usize,
    pub conflicts_resolved: usize,
    pub entries_updated: usize,
    pub warnings: Vec<String>,
    pub phase_2_duration_us: u64,
}

impl DiscoveryEngine {
    pub fn new(loom_dir: &str) -> Self {
        let snapshot = Arc::new(registry::SnapshotDb::new(loom_dir));
        let watcher = Arc::new(watcher::DiscoveryWatcher::new(loom_dir));
        Self {
            loom_dir: loom_dir.to_string(),
            snapshot,
            watcher,
        }
    }

    pub fn snapshot(&self) -> &registry::SnapshotDb {
        &self.snapshot
    }

    pub fn watcher(&self) -> &watcher::DiscoveryWatcher {
        &self.watcher
    }

    pub fn as_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Phase 1: Fast SQLite snapshot read (<3ms for 1000 entries).
    /// Returns immediately with cached state. No validation.
    pub fn phase_1(&self) -> anyhow::Result<Phase1Result> {
        let start = Instant::now();
        let result = self.snapshot.read_cached()?;
        let duration = start.elapsed().as_micros() as u64;

        Ok(Phase1Result {
            resource_counts: result.counts,
            layers: result.layers,
            ecosystem_version: loom_core::ECOSYSTEM_VERSION.to_string(),
            phase_1_duration_us: duration,
        })
    }

    /// Phase 2: Deferred validation + merge (30-50ms for 1000 entries).
    /// Validates snapshot entries, merges conflicts, and syncs new resources from
    /// the filesystem into the snapshot so `loom status` reflects the current state.
    pub fn phase_2(&self) -> anyhow::Result<Phase2Result> {
        let start = Instant::now();

        // 1. Validate entries: check each snapshot entry against the filesystem
        let scanner = scanner::Scanner::new(&self.loom_dir, self.snapshot.clone());
        let validation = scanner.validate_entries()?;

        // 2. Resolve conflicts between native and shared resources
        let merger = merger::ResourceMerger::new(&self.loom_dir);
        let conflicts = merger.resolve_all()?;

        // 3. Scan the filesystem and upsert any new/updated resources into the snapshot.
        //    This ensures `loom status` shows real counts even when resources were added
        //    after `loom init`.
        let snapshot_builder = initializer::SnapshotBuilder::new(&self.loom_dir);
        snapshot_builder.build_initial()?;

        // 4. Collect warnings
        let mut warnings = Vec::new();
        for e in &validation.errors {
            warnings.push(format!("Validation error: {} ({})", e.entry_name, e.reason));
        }
        for c in &conflicts {
            if !c.resolved {
                warnings.push(format!(
                    "Unresolved conflict for '{}': {} strategy needed",
                    c.resource_name,
                    c.strategy.clone().unwrap_or(loom_core::ConflictResolution::NativeFirst)
                ));
            }
        }

        let duration = start.elapsed().as_micros() as u64;
        Ok(Phase2Result {
            entries_validated: validation.entries_validated,
            conflicts_resolved: conflicts.iter().filter(|c| c.resolved).count(),
            entries_updated: validation.entries_updated,
            warnings,
            phase_2_duration_us: duration,
        })
    }

    /// Read live counts from the actual registries (source of truth).
    ///
    /// - Skills → `SkillsEngine` registry (source of truth, not filesystem)
    /// - MCP servers → `McpRegistry` (source of truth, not filesystem)
    /// - Tools → filesystem scan of `tools/` TOML files
    /// - Sessions → snapshot (session manifests live in `sessions/` dir)
    ///
    /// This avoids the snapshot-staleness problem: resources added after
    /// `loom init` (e.g. via `loom mcp add`, `loom skills install`) are
    /// immediately reflected.
    pub fn live_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();

        // Skills — count directories with SKILL.md from filesystem.
        // The SkillRegistry (skills.db) is not populated by `loom skills install`,
        // which only copies files. Registry is unused here.
        let skills_dir = Path::new(&self.loom_dir).join("skills");
        if skills_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                let n = entries.flatten().filter(|e| e.path().is_dir()).count();
                counts.insert("skills".into(), n);
            }
        }

        // MCP servers — query the registry (source of truth, not filesystem snapshot)
        if let Ok(registry) = loom_mcp::McpRegistry::new(&self.loom_dir) {
            counts.insert("mcp_servers".into(), registry.count().unwrap_or(0));
        }

        // Tools — count TOML files directly from filesystem
        let tools_dir = Path::new(&self.loom_dir).join("tools");
        if tools_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&tools_dir) {
                let n = entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                    })
                    .count();
                counts.insert("tools".into(), n);
            }
        }

        // Sessions — count manifests from snapshot (snapshots correctly)
        if let Ok(c) = self.snapshot.count_resources() {
            if let Some(&n) = c.get("sessions") {
                counts.insert("sessions".into(), n);
            }
            if let Some(&n) = c.get("providers") {
                counts.insert("providers".into(), n);
            }
        }

        counts
    }

    /// Run full discovery (Phase 1 + Phase 2).
    pub fn run(&self) -> anyhow::Result<loom_core::DiscoveryResult> {
        let phase1 = self.phase_1()?;
        let phase2 = self.phase_2()?;

        Ok(loom_core::DiscoveryResult {
            ecosystem_version: loom_core::ECOSYSTEM_VERSION.to_string(),
            available_layers: phase1.layers,
            resource_counts: phase1.resource_counts,
            warnings: phase2.warnings,
        })
    }
}

impl HealthCheck for DiscoveryEngine {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let snapshot_ok = self.snapshot.health().is_ok();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-discovery",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: if snapshot_ok { None } else { Some("snapshot check failed".into()) },
            performance_counters: {
                let mut m = std::collections::HashMap::new();
                m.insert("snapshot_entries".into(), self.snapshot.entry_count());
                m
            },
        })
    }
}
