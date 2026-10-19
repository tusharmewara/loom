# Discovery Architecture Analysis

**Date:** 2026-05-24
**Status:** Draft

---

## Overview

The ecosystem has **two parallel storage systems** that are not synchronized. This causes `loom status` and `loom scan` to show all zeros.

---

## The Two Storage Systems

### System 1: Live Registries (Source of Truth)

These are the "real" stores — each managed by its own crate:

| Registry | DB Location | Managed By | Has `count()`? |
|---|---|---|---|
| `SkillRegistry` | `~/.loom/skills/skills.db` | `loom-skills` | ✅ yes |
| `McpRegistry` | `~/.loom/mcp_registry.db` | `loom-mcp` | ✅ yes |
| `AuthManager` | `~/.loom/auth/` | `loom-auth` | ✅ (via `list_providers()`) |
| Tools | `~/.loom/tools/*.toml` | Filesystem | ✅ (count TOML files) |
| Sessions | `~/.loom/sessions/` | `loom-sessions` | ✅ (in SnapshotDb) |

**These are populated by user actions** (e.g., `loom init --recommended` writes to `McpRegistry`).

### System 2: Discovery Snapshot (`SnapshotDb`)

**DB Location:** `~/.loom/.cache/snapshot.db`
**Purpose:** Fast cached reads for Phase 1 (designed for <3ms reads, not <5ms)
**Populated by:** `SnapshotBuilder::build_initial()` only
**Tables:** `skills`, `mcp_servers`, `sessions`, `providers`, `tools`

**The snapshot is a cache, not a source of truth.**

---

## Current CLI Data Flow

```
loom status
└── SnapshotDb::count_resources()     ← reads snapshot cache (ALL ZEROS after init)
    └── query: SELECT COUNT(*) FROM skills/mcp_servers/tools/...

loom scan
├── DiscoveryEngine::phase_1()
│   └── SnapshotDb::read_cached()     ← reads snapshot (ALL ZEROS)
└── DiscoveryEngine::phase_2()
    └── Scanner::validate_entries()   ← validates snapshot entries (nothing to validate)
    └── SnapshotBuilder::build_initial()   ← scans filesystem (nothing there either)
    └── Merger::resolve_all()         ← no conflicts to resolve

loom init
└── SnapshotBuilder::build_initial()  ← scans filesystem only
    └── Walk skills/, mcp/, tools/ dirs
    └── Insert into snapshot
    (Does NOT read SkillRegistry or McpRegistry)

loom init --recommended
└── McpRegistry::upsert()            ← writes to ~/.loom/mcp_registry.db ← REGISTERED!
└── SnapshotBuilder::build_initial()  ← scans filesystem (mcp/ is empty)
    (snapshot still empty!)
```

---

## The Root Cause

**MCP servers registered via `loom init --recommended` are stored in `McpRegistry` (`mcp_registry.db`), NOT in the filesystem `mcp/` directory.**

`SnapshotBuilder::build_initial()` only scans the filesystem:

```rust
// Scan mcp/ directory on disk — NOT McpRegistry
let mcp_dir = Path::new(&self.loom_dir).join("mcp");
if mcp_dir.exists() {
    for entry in std::fs::read_dir(&mcp_dir)? { ... }
}
```

`loom init --recommended` writes to `McpRegistry` but **never writes to `mcp/` directory**:

```rust
// In init.rs — this writes to mcp_registry.db, NOT mcp/
registry.upsert(&entry)?;
```

**Result:** 4 MCP servers in `mcp_registry.db` (registered, working), 0 in snapshot.

---

## Module Responsibilities

### `loom-discovery` (SnapshotDb + DiscoveryEngine)

| Module | Responsibility | Owned Data |
|---|---|---|
| `SnapshotDb` | Fast cached reads for Phase 1. Stores lightweight index entries. | `~/.loom/.cache/snapshot.db` |
| `SnapshotBuilder` | Scan filesystem → populate snapshot. Also creates the `~/.loom/` tree. | Snapshot |
| `Scanner` | Validate snapshot entries against filesystem (hash checks, existence) | Reads snapshot + filesystem |
| `Merger` | Resolve conflicts between native and shared layers | No persistent state |
| `Watcher` | Hot-reload on filesystem changes | No persistent state |
| `DiscoveryEngine` | Orchestrates Phase 1 + Phase 2. **Should** be the single entry point for discovery counts. | Owns `SnapshotDb` + `Watcher` |

### `loom-skills` (SkillRegistry)

| Module | Responsibility | Owned Data |
|---|---|---|
| `SkillRegistry` | CRUD for skills. In-memory cache + SQLite persistence. | `~/.loom/skills/skills.db` |

### `loom-mcp` (McpRegistry)

| Module | Responsibility | Owned Data |
|---|---|---|
| `McpRegistry` | CRUD for MCP server entries. JSON serialization of `McpServerEntry`. | `~/.loom/mcp_registry.db` |

### `loom-cli` Commands

| Command | Current Behavior | Should Do |
|---|---|---|
| `loom init` | Creates tree + `build_initial()` (filesystem scan) | Same |
| `loom init --recommended` | Registers MCP servers in `McpRegistry` | Same |
| `loom status` | Reads `SnapshotDb::count_resources()` | Read live registries |
| `loom scan` | Reads Phase 1 (snapshot) + runs Phase 2 | Same Phase 1/2, but use live counts for output |

---

## Design Question: Snapshot vs Registries

### Option A: Snapshot is a filesystem-only cache
- Snapshot stores resources found in filesystem dirs (`skills/`, `mcp/`, `tools/`)
- CLI reads live registries for counts
- Phase 2 validates filesystem resources (hash checks, etc.)
- **Problem:** MCP servers registered via API/CLI never appear in `mcp/` dir

### Option B: Snapshot mirrors live registries (bidirectional sync)
- Phase 2 syncs from live registries → snapshot (not filesystem)
- Phase 2 syncs from snapshot → filesystem (optional)
- Snapshot always reflects real state
- **This is the correct approach.**

### Option C: Remove snapshot entirely, read live registries
- Simplest fix
- No sync needed
- **Downside:** Slower (registries may be slower than snapshot for bulk reads)

**Recommended: Option B** — Phase 2 should query live registries, not the filesystem.

---

## Recommended Fix

### DiscoveryEngine gets a `live_counts()` method

```rust
impl DiscoveryEngine {
    /// Read accurate resource counts from live registries.
    /// This is the canonical source of truth for CLI counts.
    pub fn live_counts(&self) -> anyhow::Result<HashMap<String, usize>> {
        let mut counts = HashMap::new();
        
        // Skills — from SkillRegistry
        let skill_registry = SkillRegistry::new(&self.loom_dir)?;
        counts.insert("skills".into(), skill_registry.count());
        
        // MCP servers — from McpRegistry
        let mcp_registry = McpRegistry::new(&self.loom_dir)?;
        counts.insert("mcp_servers".into(), mcp_registry.count()?);
        
        // Tools — from filesystem (tool TOMLs)
        counts.insert("tools".into(), Self::count_tools(&self.loom_dir));
        
        // Sessions — from SnapshotDb (session manifests)
        counts.insert("sessions".into(), self.snapshot.count_sessions()?);
        
        // Auth providers — from SnapshotDb (providers table)
        counts.insert("providers".into(), self.snapshot.count_providers()?);
        
        Ok(counts)
    }
}
```

### CLI commands use `live_counts()` instead of Phase 1

```rust
// status.rs — use live_counts() instead of phase_1.resource_counts
let engine = DiscoveryEngine::new(loom_dir);
let counts = engine.live_counts()?;

// scan.rs — use live_counts() for display, keep Phase 1/2 for validation
let engine = DiscoveryEngine::new(loom_dir);
let counts = engine.live_counts()?;
let phase2 = engine.phase_2()?; // still validates + syncs snapshot
```

### Phase 2 syncs from live registries

```rust
// In phase_2():
// Instead of: snapshot_builder.build_initial()  // scans filesystem
// Use: snapshot_builder.sync_from_registries()  // queries live registries
```

---

## Summary of Changes Needed

1. **`DiscoveryEngine::live_counts()`** — new method querying live registries
2. **`status.rs`** — use `live_counts()` instead of `SnapshotDb::count_resources()`
3. **`scan.rs`** — use `live_counts()` for display counts (Phase 1 still OK for layers)
4. **`Phase 2`** — change to sync from live registries, not filesystem
5. **`SnapshotBuilder`** — add `sync_from_registries()` method

---

## Key Insight

> `build_initial()` scans the **filesystem**. But many resources (MCP servers from `--recommended`) are only in **registries**. The snapshot is designed as a cache, but it can only cache what's on the filesystem. The fix is to make Phase 2 query the **live registries** to populate the snapshot, so both paths are accurate.