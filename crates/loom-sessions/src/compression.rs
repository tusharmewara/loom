//! Auto-summarization for session message histories.
//!
//! When a session grows past `SESSION_COMPRESSION_THRESHOLD` messages,
//! `SessionStore::compress_session` folds the oldest messages into a
//! single inline `MessageRole::System` summary (id = `SUMMARY_MESSAGE_ID`)
//! at the head of the message list, and keeps the last
//! `SESSION_FULL_FIDELITY_COUNT` messages verbatim. The manifest's
//! `summarized_at` / `summarized_message_count` fields record the pass
//! so consumers can show "this session has a compressed view of N
//! earlier messages" without scanning `messages.jsonl`.
//!
//! The summarizer is **extractive and deterministic** — no LLM calls.
//! It pulls the first `EXCERPT_CHARS` characters of each dropped
//! message, prefixed with its role, and groups them under turn
//! boundaries (`user` -> `assistant` -> `user` is one "turn"). This is
//! the same approach used by code-review tools and `git log --oneline`:
//! good enough to ground a follow-up, cheap to compute, no network or
//! API-key dependency, and reproducible across runs.
//!
//! A future version can swap in an LLM-based summarizer behind the
//! same function signature; the manifest fields are intentionally
//! generic ("summarized_at" / "summarized_message_count") so a richer
//! summary could carry different metadata later.

use anyhow::Result;
use chrono::Utc;
use loom_core::{Message, MessageRole, SessionManifest, SUMMARY_MESSAGE_ID};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// Number of characters of each dropped message to keep in the
/// extractive summary. Short enough to keep the summary scannable,
/// long enough to convey what was being discussed.
const EXCERPT_CHARS: usize = 200;

/// Maximum total characters the summary message body will hold. Acts
/// as a safety net so a 1000-message session doesn't produce a 100 KB
/// summary blob.
const SUMMARY_MAX_CHARS: usize = 16_000;

/// Keep the most recent `max_messages` lines in a JSONL file; older
/// lines are dropped. Used by the older truncate-only code path
/// (still useful for non-summary truncation if a caller wants it).
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

/// A compressed view of a session: the rolled-up summary message
/// (prepended at the head of the message list) plus the trailing
/// full-fidelity messages. Returned by `build_summary` for callers
/// that want to inspect what `compress_session` would do without
/// actually writing it to disk.
#[derive(Debug, Clone)]
pub struct CompressionOutcome {
    /// The summary message that should be prepended to the message
    /// list, with id `SUMMARY_MESSAGE_ID` and role `System`. The
    /// `metadata` map carries the original-message-count and the
    /// generator version so consumers can decide whether to refresh.
    pub summary_message: Message,
    /// Number of original messages that were folded into the summary
    /// (i.e. the dropped count, not the final count).
    pub folded_count: usize,
    /// The trailing full-fidelity messages that survived the pass.
    pub kept_messages: Vec<Message>,
}

/// Build a compression outcome from a full message list. Pure function
/// — does no I/O. Returns `None` (via `summary_message` left empty)
/// when there is nothing worth compressing (i.e. the session is already
/// short enough to keep verbatim).
///
/// `threshold` and `full_fidelity` mirror the constants in
/// `loom_core::SESSION_COMPRESSION_THRESHOLD` /
/// `SESSION_FULL_FIDELITY_COUNT`; they are parameters so tests can
/// exercise the boundary cases without touching those constants.
pub fn build_summary(
    messages: &[Message],
    threshold: usize,
    full_fidelity: usize,
) -> Option<CompressionOutcome> {
    // Strip any prior summary message from the head before deciding
    // what to compress. Without this, repeatedly calling
    // `compress_session` would re-fold the same content over and over.
    let real_messages: Vec<&Message> = messages
        .iter()
        .filter(|m| m.id != SUMMARY_MESSAGE_ID)
        .collect();

    if real_messages.len() <= threshold {
        return None;
    }
    if full_fidelity >= real_messages.len() {
        return None;
    }

    let split_at = real_messages.len() - full_fidelity;
    let folded: Vec<Message> = real_messages[..split_at].iter().map(|m| (*m).clone()).collect();
    let kept: Vec<Message> = real_messages[split_at..].iter().map(|m| (*m).clone()).collect();

    let summary_text = render_summary(&folded, kept.len());
    let folded_count = folded.len();

    let summary_message = Message {
        id: SUMMARY_MESSAGE_ID.to_string(),
        role: MessageRole::System,
        content: summary_text,
        tool_calls: None,
        timestamp: Utc::now().to_rfc3339(),
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "loom.summary.folded_count".to_string(),
                serde_json::Value::from(folded_count as u64),
            );
            m.insert(
                "loom.summary.full_fidelity_count".to_string(),
                serde_json::Value::from(kept.len() as u64),
            );
            m.insert(
                "loom.summary.generator".to_string(),
                serde_json::Value::from("extractive-v1"),
            );
            m
        },
    };

    Some(CompressionOutcome {
        summary_message,
        folded_count,
        kept_messages: kept,
    })
}

/// Render the extractive summary body. Pure function — exposed for
/// testing and for the (future) LLM path that wants to embed extra
/// text inside this same envelope.
fn render_summary(folded: &[Message], full_fidelity_count: usize) -> String {
    // Group messages into "turns" (consecutive same-role runs merge
    // into one excerpt). This mirrors how humans re-summarize a chat
    // transcript: "I asked about X, the assistant said Y" — one turn
    // per role boundary.
    let mut turns: Vec<(MessageRole, String)> = Vec::new();
    for m in folded {
        let role_label = match m.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        };
        let excerpt: String = m
            .content
            .chars()
            .take(EXCERPT_CHARS)
            .collect::<String>()
            .replace('\n', " ");
        let line = format!("[{role_label}] {excerpt}");
        if let Some(last) = turns.last_mut() {
            if last.0 == m.role {
                // Same role as the previous turn — append with a
                // separator. This keeps "user" / "assistant" / "user"
                // a single line of "[user] ...\n  ...\n  ..." rather
                // than three near-identical lines.
                last.1.push_str("\n  ");
                last.1.push_str(&line);
                continue;
            }
        }
        turns.push((m.role, line));
    }

    let mut out = String::new();
    out.push_str(&format!(
        "[loom auto-summary] {} earlier message{} compressed; last {} message{} kept verbatim.\n",
        folded.len(),
        if folded.len() == 1 { "" } else { "s" },
        full_fidelity_count,
        if full_fidelity_count == 1 { "" } else { "s" },
    ));
    for (i, (_role, body)) in turns.iter().enumerate() {
        if out.len() + body.len() + 8 > SUMMARY_MAX_CHARS {
            out.push_str(&format!(
                "\n... ({} more turn{} omitted to fit summary budget)\n",
                turns.len() - i,
                if turns.len() - i == 1 { "" } else { "s" },
            ));
            break;
        }
        out.push_str(&format!("\n#{} {}", i + 1, body));
    }
    out
}

/// Apply `build_summary` to a manifest in place: returns the new
/// `messages.jsonl` body (summary message + trailing full-fidelity
/// messages) and updates the manifest's `summarized_at` /
/// `summarized_message_count` fields. Pure — does not write to disk.
pub fn plan_compression(
    manifest: &mut SessionManifest,
    messages: &[Message],
    threshold: usize,
    full_fidelity: usize,
) -> Option<Vec<Message>> {
    if let Some(outcome) = build_summary(messages, threshold, full_fidelity) {
        manifest.summarized_at = Some(outcome.summary_message.timestamp.clone());
        manifest.summarized_message_count = Some(outcome.folded_count as u32);
        let mut new_messages = Vec::with_capacity(1 + outcome.kept_messages.len());
        new_messages.push(outcome.summary_message);
        new_messages.extend(outcome.kept_messages);
        // `message_count` is the total we now hold, not the original
        // session size. The full history is recoverable from
        // `summarized_message_count + kept_count` if anyone needs it.
        manifest.message_count = new_messages.len() as u32;
        Some(new_messages)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Message, MessageRole};

    fn make_msg(id: &str, role: MessageRole, content: &str) -> Message {
        Message {
            id: id.to_string(),
            role,
            content: content.to_string(),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn test_below_threshold_is_no_op() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "hi"))
            .collect();
        assert!(build_summary(&msgs, 20, 10).is_none());
    }

    #[test]
    fn test_at_threshold_boundary_is_no_op() {
        let msgs: Vec<Message> = (0..20)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "hi"))
            .collect();
        // 20 <= threshold(20) -> no compression
        assert!(build_summary(&msgs, 20, 10).is_none());
    }

    #[test]
    fn test_just_over_threshold_compresses() {
        let msgs: Vec<Message> = (0..21)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "hi"))
            .collect();
        let outcome = build_summary(&msgs, 20, 10).expect("should compress");
        // 21 - 10 (full fidelity) = 11 folded
        assert_eq!(outcome.folded_count, 11);
        assert_eq!(outcome.kept_messages.len(), 10);
        // Summary message id is the marker, role System
        assert_eq!(outcome.summary_message.id, SUMMARY_MESSAGE_ID);
        assert!(matches!(outcome.summary_message.role, MessageRole::System));
    }

    #[test]
    fn test_recompress_strips_prior_summary() {
        // Build a session that already has a summary prepended + 15
        // real messages. After build_summary, only the 15 real
        // messages should be considered (the prior summary is just
        // a header line at this point).
        let mut msgs = vec![make_msg(SUMMARY_MESSAGE_ID, MessageRole::System, "old summary")];
        for i in 0..15 {
            msgs.push(make_msg(
                &format!("m{i}"),
                MessageRole::User,
                &format!("message {i}"),
            ));
        }
        // 16 total, 1 is the prior summary. 16 <= 20 threshold so no
        // compression.
        assert!(build_summary(&msgs, 20, 10).is_none());

        // Now grow to 25 — should compress. The prior summary line
        // is filtered out before the threshold check, so the
        // outcome's folded_count = 25 - 10 = 15, NOT 16.
        for i in 15..25 {
            msgs.push(make_msg(
                &format!("m{i}"),
                MessageRole::User,
                &format!("message {i}"),
            ));
        }
        let outcome = build_summary(&msgs, 20, 10).expect("should compress at 25");
        assert_eq!(outcome.folded_count, 15);
        assert_eq!(outcome.kept_messages.len(), 10);
    }

    #[test]
    fn test_full_fidelity_larger_than_session_is_no_op() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "hi"))
            .collect();
        // Even though the session is small, the threshold check
        // catches this first.
        assert!(build_summary(&msgs, 2, 10).is_none());
    }

    #[test]
    fn test_turn_merging_collapses_consecutive_same_role() {
        // 4 messages, threshold=2, full_fidelity=2 -> 2 folded, 2 kept.
        // The folded pair is the first two user messages, which should
        // merge into a single "[user] ..." turn. The kept pair is the
        // assistant + user at the end, which should appear verbatim.
        let msgs = vec![
            make_msg("a", MessageRole::User, "first"),
            make_msg("b", MessageRole::User, "second"),
            make_msg("c", MessageRole::Assistant, "reply"),
            make_msg("d", MessageRole::User, "follow-up"),
        ];
        let outcome = build_summary(&msgs, 2, 2).expect("should compress");
        let body = &outcome.summary_message.content;
        // The two folded user messages should merge into one turn.
        assert!(body.contains("[user] first"));
        assert!(body.contains("second")); // merged into same turn
        assert_eq!(body.matches("#1 ").count(), 1);
        // The "kept" messages are NOT in the summary body — only the
        // folded ones are.
        assert!(!body.contains("reply"));
        assert!(!body.contains("follow-up"));
    }

    #[test]
    fn test_summary_metadata_records_counts() {
        let msgs: Vec<Message> = (0..25)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "x"))
            .collect();
        let outcome = build_summary(&msgs, 20, 10).expect("compress");
        let meta = &outcome.summary_message.metadata;
        assert_eq!(
            meta.get("loom.summary.folded_count").unwrap().as_u64().unwrap(),
            15
        );
        assert_eq!(
            meta.get("loom.summary.full_fidelity_count")
                .unwrap()
                .as_u64()
                .unwrap(),
            10
        );
        assert_eq!(
            meta.get("loom.summary.generator").unwrap().as_str().unwrap(),
            "extractive-v1"
        );
    }

    #[test]
    fn test_plan_compression_updates_manifest() {
        let mut manifest = SessionManifest {
            session_id: "s1".into(),
            source_tool: "test".into(),
            source_version: "0.3.9".into(),
            exported_at: "2025-01-01T00:00:00Z".into(),
            topic: "rust".into(),
            message_count: 0,
            working_directory: String::new(),
            model_provider: String::new(),
            active_skills: vec![],
            active_mcp_servers: vec![],
            files_modified: vec![],
            compatibility_note: None,
            tags: vec![],
            summarized_at: None,
            summarized_message_count: None,
        };
        let msgs: Vec<Message> = (0..25)
            .map(|i| make_msg(&format!("m{i}"), MessageRole::User, "x"))
            .collect();
        let plan = plan_compression(&mut manifest, &msgs, 20, 10).expect("should plan");
        assert_eq!(plan.len(), 11); // 1 summary + 10 kept
        assert_eq!(manifest.summarized_message_count, Some(15));
        assert_eq!(manifest.message_count, 11);
        assert!(manifest.summarized_at.is_some());
    }

    #[test]
    fn test_estimate_tokens() {
        let msgs = vec![make_msg("a", MessageRole::User, "hello world")];
        // 11 chars + 4 = 15 chars; /4 = 3 tokens (rounded down)
        assert_eq!(estimate_tokens(&msgs), 3);
    }
}
