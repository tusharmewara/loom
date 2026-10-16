//! `loom edit` — Hashline Edit System CLI.
//!
//! Content-hash anchored editing: compute hashline anchors, apply edits
//! with stale detection, and validate references.

use crate::cli::EditAction;
use loom_skills::edit::{make_replace_edit, HashlineAnchor};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::io::Write;

pub async fn run(loom_dir: &str, action: &EditAction) -> anyhow::Result<()> {
    match action {
        EditAction::Hash {
            file,
            start,
            end,
            json,
        } => handle_hash(loom_dir, file, *start, *end, *json).await,
        EditAction::Apply {
            file,
            start,
            end,
            content,
            write,
        } => handle_apply(loom_dir, file, *start, *end, content, *write).await,
        EditAction::Validate {
            file,
            hash,
            start,
            end,
        } => handle_validate(loom_dir, file, hash, *start, *end).await,
    }
}

/// Read file content, returning (path_buf, content).
fn read_file(loom_dir: &str, file: &str) -> anyhow::Result<(std::path::PathBuf, String)> {
    let path = if file.starts_with('/') {
        std::path::PathBuf::from(file)
    } else {
        std::path::Path::new(loom_dir).join(file)
    };
    let content =
        std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("Cannot read {path:?}: {e}"))?;
    Ok((path, content))
}

/// Compute a hashline anchor hash for a line range and display it.
async fn handle_hash(
    _loom_dir: &str,
    file: &str,
    start: usize,
    end: usize,
    json_output: bool,
) -> anyhow::Result<()> {
    let (path, content) = read_file(_loom_dir, file)?;

    // Get just the target lines for the hash
    let target = loom_skills::edit::get_lines(&content, start, end)
        .ok_or_else(|| anyhow::anyhow!("Invalid line range {start}-{end}"))?;

    let context_snippet = content
        .lines()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
        .collect::<Vec<_>>()
        .join(" ");

    let anchor = HashlineAnchor::from_content(
        target.as_bytes(),
        Some(loom_skills::edit::LineRange { start, end }),
        &context_snippet,
    );

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "file": path.to_string_lossy(),
                "content_hash": anchor.content_hash,
                "hash_prefix": anchor.hash_prefix(8),
                "line_range": { "start": start, "end": end },
                "valid": anchor.validate(&content),
            }))?
        );
    } else {
        println!("Hashline Anchor for {}", path.display());
        println!(
            "  Hash:       hash[{}]  (full: {})",
            anchor.hash_prefix(8),
            anchor.content_hash
        );
        println!("  Line Range: lines {start}–{end}");
        println!("  Valid:      {}", anchor.validate(&content));
        println!();
        println!(
            "  Usage:  loom edit apply {} --start {} --end {} --content \"...\"",
            file, start, end
        );
        println!(
            "  Usage:  loom edit validate {} --hash {} --start {} --end {}",
            file,
            anchor.hash_prefix(8),
            start,
            end
        );
    }
    Ok(())
}

/// Apply a hashline edit (replace lines with new content).
async fn handle_apply(
    _loom_dir: &str,
    file: &str,
    start: usize,
    end: usize,
    new_content: &str,
    write: bool,
) -> anyhow::Result<()> {
    let (path, content) = read_file(_loom_dir, file)?;

    // Build context snippet from the target lines
    let context_lines = content
        .lines()
        .skip(start.saturating_sub(1))
        .take(end.saturating_sub(start).saturating_add(1))
        .collect::<Vec<_>>()
        .join(" ");

    // Create hashline edit
    let edit = make_replace_edit(&content, start, end, new_content, &context_lines)
        .map_err(|e| anyhow::anyhow!("Failed to create edit: {e}"))?;

    // Validate first
    if !edit.validate(&content) {
        anyhow::bail!(
            "Stale anchor: file content at lines {start}-{end} no longer matches.\n\
             Expected hash[{}] — file likely changed since recording.\n\
             Re-run `loom edit hash {}` to get the current anchor.",
            edit.anchor.hash_prefix(8),
            file
        );
    }

    // Apply
    let result = edit
        .apply(&content)
        .map_err(|e| anyhow::anyhow!("Edit failed: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("Could not find anchor content in file"))?;

    if write {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(result.as_bytes())?;
        println!("Applied hashline edit to {}", path.display());
        println!(
            "  hash[{}] lines {start}-{end} → replacement ({} chars)",
            edit.anchor.hash_prefix(8),
            new_content.len()
        );
    } else {
        // Dry-run: show diff-ish output
        let old_lines: Vec<&str> = content.lines().collect();
        let new_lines: Vec<&str> = result.lines().collect();

        println!("=== Dry-run: {} === (use --write to apply)", path.display());
        println!(
            "Hashline: hash[{}] lines {start}-{end}",
            edit.anchor.hash_prefix(8)
        );
        if old_lines.len() == new_lines.len() {
            for (i, (old, new)) in old_lines.iter().zip(new_lines.iter()).enumerate() {
                if old != new {
                    println!("  \x1b[31m- L{}: {}\x1b[0m", i + 1, old);
                    println!("  \x1b[32m+ L{}: {}\x1b[0m", i + 1, new);
                }
            }
        } else {
            println!(
                "  File size changed: {} lines → {} lines",
                old_lines.len(),
                new_lines.len()
            );
            println!("{}", result);
        }
    }
    Ok(())
}

/// Validate that a hashline still matches.
async fn handle_validate(
    _loom_dir: &str,
    file: &str,
    hash: &str,
    start: usize,
    end: usize,
) -> anyhow::Result<()> {
    let (_path, content) = read_file(_loom_dir, file)?;

    // Get text at line range
    let target = loom_skills::edit::get_lines(&content, start, end)
        .ok_or_else(|| anyhow::anyhow!("Invalid line range {start}-{end}"))?;

    let actual_hash = hex::encode(Sha256::digest(target.as_bytes()));
    let expected_full = if hash.len() == 64 {
        hash.to_string()
    } else {
        // Prefix match
        actual_hash
            .starts_with(hash)
            .then(|| actual_hash.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Hash mismatch: expected prefix hash[{hash}] but actual hash is hash[{}]",
                    &actual_hash[..hash.len().min(actual_hash.len())]
                )
            })?
    };

    if actual_hash == expected_full {
        println!(
            "✅ Valid: hash[{}] matches lines {start}-{end}",
            &actual_hash[..8]
        );
    } else {
        anyhow::bail!(
            "Stale: expected hash[{expected_full}] but found hash[{actual_hash}] at lines {start}-{end}"
        );
    }
    Ok(())
}
