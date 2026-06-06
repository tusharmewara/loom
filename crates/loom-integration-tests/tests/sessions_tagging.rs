//! Integration tests for session tagging (v0.3.8).
//!
//! These tests cover the public surface of the tagging system at three
//! levels:
//!
//!  1. **`SessionStore`** tag operations — set/add/remove persist to
//!     the manifest file, normalize whitespace + dedupe, and preserve
//!     order on read-back.
//!  2. **`CrossSessionMemory`** tag operations — `set_session_tags`,
//!     `add_session_tag`, `remove_session_tag`, `session_tags`,
//!     `tag_facets`, and the `search_with_tags`/`recall_with_tags`
//!     filters.
//!  3. **Cross-store consistency** — when the manifest is updated, the
//!     memory index can be rebuilt (via `index_session_with_tags`) and
//!     the filter will see the same set of sessions.

use loom_core::{Message, MessageRole, SessionManifest};
use loom_sessions::memory::CrossSessionMemory;
use loom_sessions::store::{normalize_tags, merge_tag_sets, SessionStore};
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

fn sample_manifest(id: &str, topic: &str, tags: Vec<String>) -> SessionManifest {
    SessionManifest {
        session_id: id.to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.8".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        message_count: 1,
        topic: topic.to_string(),
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags,
    }
}

fn store_with_session(
    dir: &std::path::Path,
    id: &str,
    topic: &str,
    tags: Vec<String>,
) -> SessionStore {
    let store = SessionStore::new(dir.to_str().unwrap());
    let manifest = sample_manifest(id, topic, tags);
    store.save_manifest(&manifest).unwrap();
    store
        .replace_messages(
            id,
            &[make_msg("m1", MessageRole::User, "hello world")],
        )
        .unwrap();
    store
}

#[test]
fn test_normalize_helpers_trim_and_dedupe() {
    let normalized = normalize_tags(&[
        "  alpha  ".into(),
        "alpha".into(),
        "".into(),
        "beta".into(),
    ]);
    assert_eq!(normalized, vec!["alpha".to_string(), "beta".to_string()]);
}

#[test]
fn test_merge_tag_sets_preserves_existing_order() {
    let merged = merge_tag_sets(&["a".into(), "b".into()], &["c".into(), "d".into()]);
    assert_eq!(
        merged,
        vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string()
        ]
    );
}

#[test]
fn test_set_tags_persists_to_manifest() {
    let dir = tempdir().unwrap();
    let store = store_with_session(dir.path(), "s1", "rust", vec![]);
    store
        .set_tags("s1", &["language:rust".into(), "topic:memory".into()])
        .unwrap();

    // Read back via a fresh store (forces a manifest file read).
    let store2 = SessionStore::new(dir.path().to_str().unwrap());
    let m = store2.get("s1").unwrap();
    assert_eq!(
        m.tags,
        vec!["language:rust".to_string(), "topic:memory".to_string()]
    );
}

#[test]
fn test_set_tags_replaces_existing() {
    let dir = tempdir().unwrap();
    let store = store_with_session(dir.path(), "s1", "topic", vec!["old".into()]);
    store.set_tags("s1", &["new1".into(), "new2".into()]).unwrap();
    let m = store.get("s1").unwrap();
    assert_eq!(m.tags, vec!["new1".to_string(), "new2".to_string()]);
}

#[test]
fn test_add_tags_is_additive() {
    let dir = tempdir().unwrap();
    let store = store_with_session(dir.path(), "s1", "topic", vec!["a".into()]);
    store.add_tags("s1", &["b".into(), "c".into()]).unwrap();
    let m = store.get("s1").unwrap();
    assert_eq!(m.tags, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    // Adding a duplicate is a no-op.
    store.add_tags("s1", &["b".into()]).unwrap();
    let m = store.get("s1").unwrap();
    assert_eq!(m.tags, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}

#[test]
fn test_remove_tags_drops_specified() {
    let dir = tempdir().unwrap();
    let store = store_with_session(
        dir.path(),
        "s1",
        "topic",
        vec!["a".into(), "b".into(), "c".into()],
    );
    store.remove_tags("s1", &["b".into()]).unwrap();
    let m = store.get("s1").unwrap();
    assert_eq!(m.tags, vec!["a".to_string(), "c".to_string()]);
    // Removing a tag that isn't there is a no-op (not an error).
    store.remove_tags("s1", &["nonexistent".into()]).unwrap();
    let m = store.get("s1").unwrap();
    assert_eq!(m.tags, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn test_set_tags_normalizes_whitespace() {
    let dir = tempdir().unwrap();
    let store = store_with_session(dir.path(), "s1", "topic", vec![]);
    store
        .set_tags(
            "s1",
            &["  alpha  ".into(), "beta".into(), "alpha".into()],
        )
        .unwrap();
    let m = store.get("s1").unwrap();
    // Trimmed, deduped, order preserved.
    assert_eq!(m.tags, vec!["alpha".to_string(), "beta".to_string()]);
}

#[test]
fn test_memory_set_and_list_session_tags() {
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    memory
        .set_session_tags("s1", &["a".into(), "b".into()])
        .unwrap();
    assert_eq!(
        memory.session_tags("s1").unwrap(),
        vec!["a".to_string(), "b".to_string()]
    );
}

#[test]
fn test_memory_add_remove_tag() {
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    assert!(memory.add_session_tag("s1", "foo").unwrap());
    assert!(!memory.add_session_tag("s1", "foo").unwrap());
    assert!(memory.remove_session_tag("s1", "foo").unwrap());
    assert!(memory.session_tags("s1").unwrap().is_empty());
}

#[test]
fn test_memory_search_with_tag_filter_isolates_sessions() {
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    memory
        .index_session_with_tags(
            "rust-sess",
            &[make_msg("m1", MessageRole::User, "borrowing rules")],
            "rust",
            &["language:rust".into()],
        )
        .unwrap();
    memory
        .index_session_with_tags(
            "go-sess",
            &[make_msg("m2", MessageRole::User, "borrowing patterns")],
            "go",
            &["language:go".into()],
        )
        .unwrap();
    let rust_hits = memory
        .search_with_tags("borrowing", 10, Some(&["language:rust".into()]))
        .unwrap();
    assert_eq!(rust_hits.len(), 1);
    assert_eq!(rust_hits[0].session_id, "rust-sess");
}

#[test]
fn test_memory_recall_with_tag_filter() {
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    memory
        .index_session_with_tags(
            "rust-sess",
            &[make_msg(
                "m1",
                MessageRole::User,
                "rust ownership and borrowing",
            )],
            "rust",
            &["language:rust".into()],
        )
        .unwrap();
    memory
        .index_session_with_tags(
            "go-sess",
            &[make_msg("m2", MessageRole::User, "ownership semantics in go")],
            "go",
            &["language:go".into()],
        )
        .unwrap();
    let rust_recalls = memory
        .recall_with_tags("ownership", 10, Some(&["language:rust".into()]))
        .unwrap();
    assert_eq!(rust_recalls.len(), 1);
    assert_eq!(rust_recalls[0].session_id, "rust-sess");
}

#[test]
fn test_tag_facets_counts_sessions_per_tag() {
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    memory.set_session_tags("s1", &["a".into()]).unwrap();
    memory
        .set_session_tags("s2", &["a".into(), "b".into()])
        .unwrap();
    memory.set_session_tags("s3", &["a".into()]).unwrap();
    let facets = memory.tag_facets().unwrap();
    assert_eq!(facets[0], ("a".to_string(), 3));
    assert!(facets.iter().any(|(t, _)| t == "b"));
    assert_eq!(facets.iter().find(|(t, _)| t == "b").unwrap().1, 1);
}

#[test]
fn test_index_session_with_tags_writes_to_both_indexes() {
    // Round-trip: store creates session, memory index sees both
    // messages and tags. A subsequent `search_with_tags` filter must
    // be able to recover the session.
    let dir = tempdir().unwrap();
    let _store = store_with_session(
        dir.path(),
        "round-trip",
        "topic",
        vec!["alpha".into(), "beta".into()],
    );
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    let store2 = SessionStore::new(dir.path().to_str().unwrap());
    let msgs = store2.load_messages("round-trip").unwrap();
    memory
        .index_session_with_tags(
            "round-trip",
            &msgs,
            "topic",
            &["alpha".into(), "beta".into()],
        )
        .unwrap();
    let tags = memory.session_tags("round-trip").unwrap();
    assert_eq!(tags, vec!["alpha".to_string(), "beta".to_string()]);
    let count = memory.message_count("round-trip").unwrap();
    assert_eq!(count, 1);
}

#[test]
fn test_remove_session_drops_tags_too() {
    // This is the cross-store cleanup invariant: deleting from one
    // side has to also clear the other side, or `--tag` filters
    // would surface dead sessions.
    let dir = tempdir().unwrap();
    let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
    memory
        .index_session_with_tags(
            "s1",
            &[make_msg("m1", MessageRole::User, "hello")],
            "t",
            &["a".into(), "b".into()],
        )
        .unwrap();
    assert_eq!(memory.session_tags("s1").unwrap().len(), 2);
    memory.remove_session("s1").unwrap();
    assert!(memory.session_tags("s1").unwrap().is_empty());
    assert_eq!(memory.message_count("s1").unwrap(), 0);
}
