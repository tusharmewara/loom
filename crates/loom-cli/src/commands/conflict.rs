//! `loom conflict` — Merge Conflict Detection and Resolution CLI.
//!
//! Scans files for Git-style conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
//! and resolves them using the chosen strategy.

use crate::cli::ConflictAction;
use loom_conflict::{
    resolve_bulk, resolve_file, resolve_file_in_place, scan_content, scan_directory, scan_file,
    Conflict, ResolutionStrategy,
};
use serde_json::json;

pub async fn run(loom_dir: &str, action: &ConflictAction) -> anyhow::Result<()> {
    match action {
        ConflictAction::Scan { path, json } => handle_scan(loom_dir, path, *json).await,
        ConflictAction::Resolve {
            file,
            strategy,
            write,
            json,
        } => handle_resolve(loom_dir, file, strategy, *write, *json).await,
        ConflictAction::ResolveBulk { files, strategy } => {
            handle_resolve_bulk(loom_dir, files, strategy).await
        }
        ConflictAction::Show { file, json } => handle_show(loom_dir, file, *json).await,
    }
}

/// Parse a strategy string, returning a strategy or an error.
fn parse_strategy(s: &str) -> Option<ResolutionStrategy> {
    match s.to_lowercase().replace('-', "_").as_str() {
        "ours" => Some(ResolutionStrategy::Ours),
        "theirs" => Some(ResolutionStrategy::Theirs),
        "both_ours_theirs" | "both" => Some(ResolutionStrategy::BothOursTheirs),
        "both_theirs_ours" => Some(ResolutionStrategy::BothTheirsOurs),
        _ => None,
    }
}

/// Print a human-readable summary of conflicts.
fn print_conflicts(conflicts: &[Conflict], prefix: &str) {
    if conflicts.is_empty() {
        println!("{prefix}No conflicts found.");
        return;
    }
    println!("{prefix}Found {} conflict(s):", conflicts.len());
    for (i, c) in conflicts.iter().enumerate() {
        println!(
            "  {}. {}:{}\n     Ours:   {} ({})\n     Theirs: {} ({})",
            i + 1,
            c.file,
            c.start_line,
            c.our_ref,
            preview(&c.ours, 60),
            c.their_ref,
            preview(&c.theirs, 60),
        );
    }
}

fn preview(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    if trimmed.len() <= max_chars {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..max_chars])
    }
}

/// Scan a file or directory for conflicts.
async fn handle_scan(_loom_dir: &str, path: &str, json_output: bool) -> anyhow::Result<()> {
    let resolved_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", _loom_dir, path.trim_start_matches("./"))
    };

    let conflicts = if std::path::Path::new(&resolved_path).is_dir() {
        scan_directory(&resolved_path).map_err(|e| anyhow::anyhow!("Scan failed: {e}"))?
    } else {
        scan_file(&resolved_path).map_err(|e| anyhow::anyhow!("Scan failed: {e}"))?
    };

    if json_output {
        let items: Vec<serde_json::Value> = conflicts
            .iter()
            .map(|c| {
                json!({
                    "file": c.file,
                    "start_line": c.start_line,
                    "our_ref": c.our_ref,
                    "their_ref": c.their_ref,
                    "ours": c.ours,
                    "theirs": c.theirs,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "path": resolved_path,
                "conflict_count": conflicts.len(),
                "conflicts": items,
            }))?
        );
    } else {
        let label = if std::path::Path::new(&resolved_path).is_dir() {
            format!("Directory {}", resolved_path)
        } else {
            format!("File {}", resolved_path)
        };
        println!("Scanning {label}");
        print_conflicts(&conflicts, "  ");
    }
    Ok(())
}

/// Resolve conflicts in a single file.
async fn handle_resolve(
    _loom_dir: &str,
    file: &str,
    strategy_str: &str,
    should_write: bool,
    json_output: bool,
) -> anyhow::Result<()> {
    let resolved_path = if file.starts_with('/') {
        file.to_string()
    } else {
        format!("{}/{}", _loom_dir, file.trim_start_matches("./"))
    };

    let strategy = parse_strategy(strategy_str)
        .ok_or_else(|| anyhow::anyhow!(
            "Unknown strategy '{strategy_str}'. Use: ours, theirs, both-ours-theirs, both-theirs-ours"
        ))?;

    if should_write {
        let count = resolve_file_in_place(&resolved_path, strategy)
            .map_err(|e| anyhow::anyhow!("Resolution failed: {e}"))?;

        if json_output {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "file": resolved_path,
                    "strategy": strategy_str,
                    "conflicts_resolved": count,
                }))?
            );
        } else if count == 0 {
            println!("No conflicts found in {resolved_path}");
        } else {
            println!(
                "Resolved {count} conflict(s) in {resolved_path} using \"{strategy_str}\" strategy"
            );
        }
    } else {
        // Dry-run: show what would be resolved
        let content = std::fs::read_to_string(&resolved_path)
            .map_err(|e| anyhow::anyhow!("Cannot read {resolved_path}: {e}"))?;
        let conflicts = scan_content(&resolved_path, &content);

        if conflicts.is_empty() {
            println!("No conflicts found in {resolved_path}");
            return Ok(());
        }

        let resolved = resolve_file(&content, strategy);

        if json_output {
            let items: Vec<serde_json::Value> = conflicts
                .iter()
                .map(|c| {
                    json!({
                        "file": c.file,
                        "start_line": c.start_line,
                        "resolution": strategy.resolve(c),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "file": resolved_path,
                    "strategy": strategy_str,
                    "conflict_count": conflicts.len(),
                    "resolutions": items,
                    "resolved_content": resolved,
                }))?
            );
        } else {
            println!("=== Dry-run: {resolved_path} === (use --write to apply)");
            println!("Strategy: {strategy_str}");
            print_conflicts(&conflicts, "  ");
            println!();
            println!("Resolved content:");
            println!("{resolved}");
        }
    }
    Ok(())
}

/// Bulk-resolve conflicts across multiple files.
async fn handle_resolve_bulk(
    _loom_dir: &str,
    files: &[String],
    strategy_str: &str,
) -> anyhow::Result<()> {
    let strategy = parse_strategy(strategy_str)
        .ok_or_else(|| anyhow::anyhow!(
            "Unknown strategy '{strategy_str}'. Use: ours, theirs, both-ours-theirs, both-theirs-ours"
        ))?;

    // Resolve absolute paths
    let resolved_files: Vec<String> = files
        .iter()
        .map(|f| {
            if f.starts_with('/') {
                f.clone()
            } else {
                format!("{}/{}", _loom_dir, f.trim_start_matches("./"))
            }
        })
        .collect();

    let results = resolve_bulk(&resolved_files, strategy);
    let mut total = 0;
    for (file, result) in &results {
        match result {
            Ok(count) => {
                if *count > 0 {
                    println!("{file}: resolved {count} conflict(s)");
                } else {
                    println!("{file}: no conflicts found");
                }
                total += count;
            }
            Err(e) => {
                eprintln!("{file}: error — {e}");
            }
        }
    }
    println!(
        "Total: {total} conflict(s) resolved across {} file(s)",
        files.len()
    );
    Ok(())
}

/// Show detailed info about conflicts in a file.
async fn handle_show(_loom_dir: &str, file: &str, json_output: bool) -> anyhow::Result<()> {
    let resolved_path = if file.starts_with('/') {
        file.to_string()
    } else {
        format!("{}/{}", _loom_dir, file.trim_start_matches("./"))
    };

    let content = std::fs::read_to_string(&resolved_path)
        .map_err(|e| anyhow::anyhow!("Cannot read {resolved_path}: {e}"))?;
    let conflicts = scan_content(&resolved_path, &content);

    if json_output {
        let items: Vec<serde_json::Value> = conflicts
            .iter()
            .map(|c| {
                json!({
                    "file": c.file,
                    "start_line": c.start_line,
                    "our_ref": c.our_ref,
                    "their_ref": c.their_ref,
                    "ours": c.ours,
                    "theirs": c.theirs,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "file": resolved_path,
                "conflict_count": conflicts.len(),
                "conflicts": items,
            }))?
        );
    } else {
        println!("Conflicts in {resolved_path}");
        println!("═══════════════════════════════");
        if conflicts.is_empty() {
            println!("  No conflicts found.");
            return Ok(());
        }
        for (i, c) in conflicts.iter().enumerate() {
            println!("Conflict #{} at line {}", i + 1, c.start_line);
            println!("  Ours ({}):", c.our_ref);
            for line in c.ours.lines() {
                println!("    {line}");
            }
            println!("  Theirs ({}):", c.their_ref);
            for line in c.theirs.lines() {
                println!("    {line}");
            }
            println!("  Resolution options:");
            println!("    loom conflict resolve {} --strategy ours", file);
            println!("    loom conflict resolve {} --strategy theirs", file);
            println!(
                "    loom conflict resolve {} --strategy both-ours-theirs",
                file
            );
            println!();
        }
    }
    Ok(())
}
