use anyhow::Result;
use loom_core::Message;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Keep the most recent `max_messages` lines in a JSONL file; older lines are dropped.
pub fn compress_jsonl(path: &Path, max_messages: usize) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    if lines.len() <= max_messages {
        return Ok(0);
    }

    let removed = lines.len() - max_messages;
    let kept = &lines[lines.len() - max_messages..];

    let mut out = File::create(path)?;
    for line in kept {
        writeln!(out, "{line}")?;
    }

    Ok(removed)
}

/// Estimate token count for compression heuristics (rough: chars / 4).
pub fn estimate_tokens(messages: &[Message]) -> u32 {
    messages
        .iter()
        .map(|m| (m.content.len() as u32).saturating_add(4) / 4)
        .sum()
}
