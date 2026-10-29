//! `loom export --plain-files` — write all skills, sessions, MCP config,
//! credentials, and analytics to plain files in `{loom_dir}/export/`.
//! Works without the daemon. Addresses the lock-in fear (repeated in panel
//! trust/need answers): context is never locked in if Loom shuts down.
//!
//! All files are copied as-is (SQLite databases are portable files).
//! The daemon should be stopped before running this to avoid copying
//! partially-written databases.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

/// Files and directories to copy from `{loom_dir}` to `{loom_dir}/export/`.
/// Each entry is (relative source path, description).
const EXPORT_ENTRIES: &[(&str, &str)] = &[
    ("license.json", "Ed25519-signed license key (org, tier, seats, expiry)"),
    ("org/membership.json", "Cached org membership (roster, tier, humans, machines)"),
    ("telemetry/usage.db", "Usage ledger (per-run token/cost accounting)"),
    ("control-plane/analytics.db", "Aggregated org analytics (no per-run rows)"),
    ("control-plane/audit.db", "Structured audit log (actor, action, resource, outcome)"),
    ("knowledge/graph.db", "Knowledge graph (entities, relationships, FTS5)"),
    ("skills/skills.db", "Shared skills registry"),
    ("mcp_registry.db", "MCP server registry"),
];

/// Run `loom export --plain-files`.
pub fn run(loom_dir: &str, plain_files: bool) -> Result<()> {
    if !plain_files {
        return Err(anyhow::anyhow!("only --plain-files is supported"));
    }

    let src = Path::new(loom_dir);
    let dst = src.join("export");

    fs::create_dir_all(&dst)
        .with_context(|| format!("creating export dir {:?}", dst))?;

    let mut exported: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for (rel_path, description) in EXPORT_ENTRIES {
        let src_path = src.join(rel_path);
        if !src_path.exists() {
            skipped.push(format!("{rel_path} ({description}) — not found"));
            continue;
        }

        let dst_path = dst.join(rel_path);
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating dir {:?}", parent))?;
        }

        fs::copy(&src_path, &dst_path).with_context(|| {
            format!("copying {rel_path} to {:?}", dst_path)
        })?;
        exported.push(format!("{rel_path} ({description})"));
    }

    // Print results
    println!("Exported {} entries to {:?}", exported.len(), dst);
    for entry in &exported {
        println!("  ✓ {entry}");
    }
    if !skipped.is_empty() {
        println!("Skipped {} entries (not found):", skipped.len());
        for entry in &skipped {
            println!("  - {entry}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn test_loom_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let loom_dir = dir.path().to_path_buf();
        // Create the directory structure
        fs::create_dir_all(loom_dir.join("org")).unwrap();
        fs::create_dir_all(loom_dir.join("telemetry")).unwrap();
        fs::create_dir_all(loom_dir.join("control-plane")).unwrap();
        fs::create_dir_all(loom_dir.join("knowledge")).unwrap();
        fs::create_dir_all(loom_dir.join("skills")).unwrap();
        (dir, loom_dir)
    }

    #[test]
    fn export_creates_export_dir() {
        let (_dir, loom_dir) = test_loom_dir();
        // Create some files to export
        fs::write(loom_dir.join("license.json"), "test-license").unwrap();
        fs::write(loom_dir.join("org/membership.json"), "{\"org\":{\"id\":\"test\"}}").unwrap();

        run(loom_dir.to_str().unwrap(), true).unwrap();

        let export_dir = loom_dir.join("export");
        assert!(export_dir.exists());
        assert!(export_dir.join("license.json").exists());
        assert!(export_dir.join("org/membership.json").exists());
    }

    #[test]
    fn export_skips_missing_files() {
        let (_dir, loom_dir) = test_loom_dir();
        // Don't create any files

        run(loom_dir.to_str().unwrap(), true).unwrap();

        let export_dir = loom_dir.join("export");
        assert!(export_dir.exists());
        // Should not panic when no files exist
    }

    #[test]
    fn export_rejects_non_plain_files() {
        let (_dir, loom_dir) = test_loom_dir();
        let result = run(loom_dir.to_str().unwrap(), false);
        assert!(result.is_err());
    }
}
