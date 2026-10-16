//! Integration tests for session auto-summarization (v0.3.9).
//!
//! These tests cover the public surface of the summarization system at
//! three levels:
//!
//!  1. **`SessionStore::compress_session`** — runs the threshold check,
//!     rewrites `messages.jsonl` to a (summary, ...kept) layout, and
//!     updates the manifest's `summarized_at` /
//!     `summarized_message_count` fields.
//!  2. **Idempotence / re-summarization** — a second
//!     `compress_session` after the first must NOT re-fold the summary
//!     line. The first summary gets recognized by id and stripped
//!     before the threshold check.
//!  3. **Auto-summarization on `append_message`** — appending the
//!     message that pushes the count past the threshold must trigger
//!     compression without the caller having to ask.
//!  4. **Cross-store consistency** — the FTS index should be able to
//!     see the compressed view of the session after a manual
//!     `compress_session` + `index_session_with_tags` cycle.

use loom_core::{Message, MessageRole, SessionManifest, SUMMARY_MESSAGE_ID};
use loom_sessions::memory::CrossSessionMemory;
use loom_sessions::store::SessionStore;
use tempfile::tempdir;

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

fn sample_manifest(id: &str, message_count: u32) -> SessionManifest {
    SessionManifest {
        session_id: id.to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "summarization".to_string(),
        message_count,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    }
}

/// Build a session with `n` alternating user/assistant messages, with
/// each message's content being its index — easy to grep for in the
/// summary output.
fn store_with_messages(dir: &std::path::Path, id: &str, n: usize) -> SessionStore {
    let store = SessionStore::new(dir.to_str().unwrap());
    let manifest = sample_manifest(id, n as u32);
    store.save_manifest(&manifest).unwrap();
    let msgs: Vec<Message> = (0..n)
        .map(|i| {
            make_msg(
                &format!("m{i}"),
                if i % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                &format!("message number {i} about topic X"),
            )
        })
        .collect();
    store.replace_messages(id, &msgs).unwrap();
    store
}

#[test]
fn test_compress_session_above_threshold_rewrites() {
    let dir = tempdir().unwrap();
    // Use direct file writes (not replace_messages) so the
    // replace-time auto-trigger doesn't fire and the test exercises
    // compress_session in isolation.
    let store = SessionStore::new(dir.path().to_str().unwrap());
    store.save_manifest(&sample_manifest("s1", 25)).unwrap();
    write_messages_raw(dir.path(), "s1", 25);

    let outcome = store.compress_session("s1").expect("compress");
    let folded = outcome.expect("should have folded");
    assert_eq!(folded, 15);

    let post_msgs = store.load_messages("s1").unwrap();
    assert_eq!(post_msgs.len(), 11);
    assert_eq!(post_msgs[0].id, SUMMARY_MESSAGE_ID);
    assert!(matches!(post_msgs[0].role, MessageRole::System));
    // The summary body should reference message 0 (folded) but not
    // message 24 (kept).
    assert!(post_msgs[0].content.contains("message number 0"));
    assert!(!post_msgs[0].content.contains("message number 24"));
    // Last message should be the original m24 verbatim.
    assert_eq!(post_msgs.last().unwrap().id, "m24");

    // Manifest updated.
    let manifest = store.get("s1").unwrap();
    assert_eq!(manifest.summarized_message_count, Some(15));
    assert_eq!(manifest.message_count, 11);
    assert!(manifest.summarized_at.is_some());
}

/// Write `n` user messages to `messages.jsonl` for `sid` without
/// touching the SQLite index. The store's `load_messages` reads the
/// JSONL directly, so the missing index entries don't affect the
/// test path that exercises compression. (We only need to bypass
/// `replace_messages`'s own auto-trigger — the index is rebuilt
/// inside `compress_session` via `replace_messages`.)
fn write_messages_raw(dir: &std::path::Path, sid: &str, n: usize) {
    let path = dir.join("sessions").join(sid).join("messages.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(&path).unwrap();
    use std::io::Write;
    for i in 0..n {
        let m = make_msg(
            &format!("m{i}"),
            if i % 2 == 0 {
                MessageRole::User
            } else {
                MessageRole::Assistant
            },
            &format!("message number {i} about topic X"),
        );
        writeln!(f, "{}", serde_json::to_string(&m).unwrap()).unwrap();
    }
}

#[test]
fn test_compress_session_below_threshold_is_noop() {
    let dir = tempdir().unwrap();
    let store = store_with_messages(dir.path(), "s1", 5);
    let outcome = store.compress_session("s1").expect("compress");
    assert!(outcome.is_none(), "below threshold -> no compression");
    // Messages unchanged.
    let post = store.load_messages("s1").unwrap();
    assert_eq!(post.len(), 5);
    let manifest = store.get("s1").unwrap();
    assert!(manifest.summarized_at.is_none());
    assert!(manifest.summarized_message_count.is_none());
}

#[test]
fn test_compress_session_at_threshold_boundary_is_noop() {
    let dir = tempdir().unwrap();
    // 20 == threshold -> no compression (the threshold check is
    // strictly greater than).
    let store = store_with_messages(dir.path(), "s1", 20);
    let outcome = store.compress_session("s1").expect("compress");
    assert!(outcome.is_none());
}

#[test]
fn test_compress_session_is_idempotent() {
    let dir = tempdir().unwrap();
    let store = store_with_messages(dir.path(), "s1", 25);
    let _ = store.compress_session("s1").expect("first compress");
    // Second call should see 11 messages (1 summary + 10 kept) and
    // consider the threshold based on the *non-summary* count, which
    // is 10. 10 <= 20 -> no-op.
    let outcome2 = store.compress_session("s1").expect("second compress");
    assert!(outcome2.is_none(), "second compress should be a no-op");
    // Message count and manifest should be unchanged from the first pass.
    let post = store.load_messages("s1").unwrap();
    assert_eq!(post.len(), 11);
    let manifest = store.get("s1").unwrap();
    assert_eq!(manifest.summarized_message_count, Some(15));
}

#[test]
fn test_append_message_auto_triggers_compression() {
    let dir = tempdir().unwrap();
    // Pre-fill with threshold + 1 messages via direct writes that
    // bypass the auto-trigger (replace_messages triggers at the end,
    // but here we use save + append to verify append's own path).
    // To isolate the append path, build a 20-message session that is
    // *not* yet over threshold, then append one more.
    let store = SessionStore::new(dir.path().to_str().unwrap());
    store.save_manifest(&sample_manifest("s1", 20)).unwrap();
    let msgs: Vec<Message> = (0..20)
        .map(|i| make_msg(&format!("m{i}"), MessageRole::User, &format!("msg {i}")))
        .collect();
    // Save the file with 20 messages but no compression trigger.
    {
        let dir_path = dir.path();
        let path = dir_path.join("sessions").join("s1").join("messages.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        for m in &msgs {
            use std::io::Write;
            writeln!(f, "{}", serde_json::to_string(m).unwrap()).unwrap();
        }
    }
    // The next append should cross the threshold (21 > 20) and auto-fire.
    let new_msg = make_msg("m20", MessageRole::User, "trigger");
    store.append_message("s1", &new_msg).expect("append");
    let post = store.load_messages("s1").unwrap();
    // 1 summary + 10 full-fidelity = 11
    assert_eq!(
        post.len(),
        11,
        "auto-compress should leave summary + 10 kept"
    );
    assert_eq!(post[0].id, SUMMARY_MESSAGE_ID);
    let manifest = store.get("s1").unwrap();
    assert!(manifest.summarized_at.is_some());
    assert_eq!(manifest.summarized_message_count, Some(11));
}

#[test]
fn test_compress_then_index_drops_folded_messages_from_search() {
    // After compression + re-index, the FTS view should NOT return
    // hits from the (now-folded) original messages. Search should
    // only return hits from the kept messages + the summary.
    let dir = tempdir().unwrap();
    let store = SessionStore::new(dir.path().to_str().unwrap());
    store.save_manifest(&sample_manifest("s1", 25)).unwrap();
    // Build the raw messages with the kangaroo/platypus content so
    // the summary body itself contains "kangaroo".
    let path = dir
        .path()
        .join("sessions")
        .join("s1")
        .join("messages.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let mut f = std::fs::File::create(&path).unwrap();
        use std::io::Write;
        for i in 0..25 {
            let content = if i < 15 {
                format!("folded message number {i} about kangaroos")
            } else {
                format!("kept message number {i} about platypuses")
            };
            let m = make_msg(&format!("m{i}"), MessageRole::User, &content);
            writeln!(f, "{}", serde_json::to_string(&m).unwrap()).unwrap();
        }
    }
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();

    // Pre-condition: the original 25-message set is indexable, and a
    // search for a word that appears in the (later-folded) range
    // hits.
    let pre_msgs = store.load_messages("s1").unwrap();
    memory
        .index_session_with_tags("s1", &pre_msgs, "summarization", &[])
        .unwrap();
    let before = memory.search("kangaroo", 50).unwrap();
    assert!(
        before.len() >= 15,
        "before compress: kangaroo should hit all 15 folded messages, got {}",
        before.len()
    );

    // Compress + re-index with the post-compression message set.
    store.compress_session("s1").expect("compress");
    let post = store.load_messages("s1").unwrap();
    assert_eq!(post.len(), 11);
    assert!(post[0].content.contains("kangaroo"));
    memory
        .index_session_with_tags("s1", &post, "summarization", &[])
        .unwrap();

    // The summary message gets indexed (its content is the rendered
    // extractive view, which still mentions "kangaroo"), and the 10
    // kept messages get indexed. The 15 folded messages no longer
    // contribute as separate rows. So the search should return
    // exactly 1 hit (the summary line).
    let after = memory.search("kangaroo", 10).unwrap();
    assert_eq!(
        after.len(),
        1,
        "after compress: only summary should carry the folded content, got {} hits",
        after.len()
    );
    // Sanity: kept-range word is still there, in the 10 kept
    // messages. The messages contain "platypuses"; the porter
    // stemmer reduces that to "platypus" so both forms match.
    let plat = memory.search("platypuses", 50).unwrap();
    assert_eq!(plat.len(), 10, "kept messages should still be searchable");
}

#[test]
fn test_summary_message_role_and_metadata() {
    let dir = tempdir().unwrap();
    let store = store_with_messages(dir.path(), "s1", 25);
    store.compress_session("s1").expect("compress");
    let post = store.load_messages("s1").unwrap();
    let summary = &post[0];
    assert_eq!(summary.id, SUMMARY_MESSAGE_ID);
    assert!(matches!(summary.role, MessageRole::System));
    let meta = &summary.metadata;
    assert_eq!(
        meta.get("loom.summary.folded_count")
            .unwrap()
            .as_u64()
            .unwrap(),
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
        meta.get("loom.summary.generator")
            .unwrap()
            .as_str()
            .unwrap(),
        "extractive-v1"
    );
}

#[test]
fn test_replace_messages_above_threshold_auto_compresses() {
    let dir = tempdir().unwrap();
    let store = SessionStore::new(dir.path().to_str().unwrap());
    store.save_manifest(&sample_manifest("s1", 0)).unwrap();
    let msgs: Vec<Message> = (0..30)
        .map(|i| make_msg(&format!("m{i}"), MessageRole::User, &format!("msg {i}")))
        .collect();
    // replace_messages itself triggers compression at the end.
    store.replace_messages("s1", &msgs).expect("replace");
    let post = store.load_messages("s1").unwrap();
    assert_eq!(post.len(), 11, "replace-time compression should fire");
    assert_eq!(post[0].id, SUMMARY_MESSAGE_ID);
    let manifest = store.get("s1").unwrap();
    assert_eq!(manifest.summarized_message_count, Some(20));
    assert_eq!(manifest.message_count, 11);
}

#[test]
fn test_replace_messages_below_threshold_does_not_compress() {
    let dir = tempdir().unwrap();
    let store = SessionStore::new(dir.path().to_str().unwrap());
    store.save_manifest(&sample_manifest("s1", 0)).unwrap();
    let msgs: Vec<Message> = (0..5)
        .map(|i| make_msg(&format!("m{i}"), MessageRole::User, &format!("msg {i}")))
        .collect();
    store.replace_messages("s1", &msgs).expect("replace");
    let post = store.load_messages("s1").unwrap();
    assert_eq!(post.len(), 5);
    assert_ne!(post[0].id, SUMMARY_MESSAGE_ID);
    let manifest = store.get("s1").unwrap();
    assert!(manifest.summarized_at.is_none());
}

#[test]
fn test_manifest_round_trips_with_summarized_fields() {
    // The serde defaults must keep pre-v0.3.9 manifests loadable and
    // must let the new fields round-trip cleanly.
    let dir = tempdir().unwrap();
    let store = store_with_messages(dir.path(), "s1", 25);
    store.compress_session("s1").expect("compress");
    // Read the manifest back from disk and verify fields survive.
    let manifest = store.get("s1").unwrap();
    let raw = std::fs::read_to_string(dir.path().join("sessions").join("s1").join("manifest.json"))
        .unwrap();
    let parsed: SessionManifest = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        parsed.summarized_message_count,
        manifest.summarized_message_count
    );
    assert_eq!(parsed.summarized_at, manifest.summarized_at);
    // Pre-v0.3.9 manifests (without these fields) must still load
    // — synthesize one on disk and read it.
    let dir2 = tempdir().unwrap();
    let store2 = SessionStore::new(dir2.path().to_str().unwrap());
    store2.save_manifest(&sample_manifest("legacy", 3)).unwrap();
    let legacy = store2.get("legacy").unwrap();
    assert!(legacy.summarized_at.is_none());
    assert!(legacy.summarized_message_count.is_none());
}
