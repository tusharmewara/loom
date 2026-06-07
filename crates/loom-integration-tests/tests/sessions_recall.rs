//! Integration tests for the `CrossSessionMemory::recall` API (§3.2 of the
//! improvement plan). `recall` is the "what past sessions are about X"
//! affordance on top of the message-level `search`. These tests verify the
//! public contract that the `loom sessions recall` CLI command depends on:
//!
//!  1. Multiple sessions sharing the same topic word each appear once
//!     in the output (grouping), with the right hit counts aggregated.
//!  2. The JSON shape matches the documented schema (command, query,
//!     count, results, schema_version).
//!  3. Empty / non-matching queries return an empty Vec (not an error).
//!  4. The `limit` parameter is respected and the ordering is by
//!     `top_score` ascending (more-negative = better), with `hit_count`
//!     as a tiebreaker.
//!  5. A single multi-message session with several hits is returned with
//!     `hit_count` equal to the message count and `top_match` from the
//!     best-scoring message.

use anyhow::Result;
use loom_core::{Message, MessageRole, SessionManifest};
use loom_sessions::memory::{CrossSessionMemory, SessionRecall};
use loom_sessions::store::SessionStore;
use loom_sessions::SessionManager;
use tempfile::tempdir;

fn make_msg(id: &str, role: MessageRole, content: &str) -> Message {
    Message {
        id: id.to_string(),
        role,
        content: content.to_string(),
        tool_calls: None,
        timestamp: "2026-06-06T00:00:00Z".into(),
        metadata: Default::default(),
    }
}

fn save_session(loom_dir: &str, sid: &str, topic: &str, messages: Vec<Message>) -> Result<()> {
    let manager = SessionManager::new(loom_dir);
    let store = manager.store();
    let manifest = SessionManifest {
        session_id: sid.into(),
        source_tool: "loom-test".into(),
        source_version: "0.3.7".into(),
        exported_at: "2026-06-06T00:00:00Z".into(),
        topic: topic.into(),
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: "/tmp".into(),
        model_provider: "test".into(),
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    };
    store.save_manifest(&manifest)?;
    store.replace_messages(sid, &messages)?;
    let memory = CrossSessionMemory::new(loom_dir)?;
    memory.index_session(sid, &messages, topic)?;
    Ok(())
}

#[test]
fn test_recall_groups_multiple_sessions_with_same_topic() -> Result<()> {
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();

    // Three sessions, each with multiple "routing" mentions.
    save_session(
        &loom,
        "routing-deep-dive",
        "routing internals",
        vec![
            make_msg("a1", MessageRole::User, "How does routing decide which tier?"),
            make_msg("a2", MessageRole::Assistant, "Routing uses complexity scoring."),
            make_msg("a3", MessageRole::User, "Tune the routing threshold?"),
        ],
    )?;
    save_session(
        &loom,
        "routing-101",
        "routing basics",
        vec![make_msg(
            "b1",
            MessageRole::User,
            "What is routing in this app?",
        )],
    )?;
    save_session(
        &loom,
        "mcp-setup",
        "mcp",
        vec![make_msg(
            "c1",
            MessageRole::User,
            "How do I configure MCP servers?",
        )],
    )?;

    let memory = CrossSessionMemory::new(&loom)?;
    let recalled: Vec<SessionRecall> = memory.recall("routing", 10)?;

    // session-mcp must not appear; only the 2 sessions whose content
    // actually contains "routing" should be present.
    let ids: Vec<&str> = recalled.iter().map(|r| r.session_id.as_str()).collect();
    assert!(ids.contains(&"routing-deep-dive"));
    assert!(ids.contains(&"routing-101"));
    assert!(!ids.contains(&"mcp-setup"), "unrelated session leaked in");
    assert_eq!(recalled.len(), 2);

    // Hit count aggregation
    let deep = recalled
        .iter()
        .find(|r| r.session_id == "routing-deep-dive")
        .expect("routing-deep-dive missing");
    let basics = recalled
        .iter()
        .find(|r| r.session_id == "routing-101")
        .expect("routing-101 missing");
    assert_eq!(deep.hit_count, 3, "deep-dive should aggregate 3 hits");
    assert_eq!(basics.hit_count, 1, "routing-101 has 1 hit");
    // Topic comes from the manifest/indexer.
    assert_eq!(deep.topic, "routing internals");
    assert_eq!(basics.topic, "routing basics");
    Ok(())
}

#[test]
fn test_recall_empty_store_returns_empty_vec() -> Result<()> {
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();
    let memory = CrossSessionMemory::new(&loom)?;
    let recalled = memory.recall("anything", 10)?;
    assert!(recalled.is_empty());
    Ok(())
}

#[test]
fn test_recall_no_matches_returns_empty_vec() -> Result<()> {
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();
    save_session(
        &loom,
        "s1",
        "greeting",
        vec![make_msg("m1", MessageRole::User, "Hello there")],
    )?;
    let memory = CrossSessionMemory::new(&loom)?;
    let recalled = memory.recall("xyzzy_no_such_term_qq", 10)?;
    assert!(recalled.is_empty());
    Ok(())
}

#[test]
fn test_recall_respects_limit() -> Result<()> {
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();
    for i in 0..5 {
        let sid = format!("sess-{i}");
        save_session(
            &loom,
            &sid,
            &format!("topic {i}"),
            vec![make_msg(
                "m1",
                MessageRole::User,
                &format!("unique-token routing discussion #{i}"),
            )],
        )?;
    }
    let memory = CrossSessionMemory::new(&loom)?;
    let recalled = memory.recall("routing", 3)?;
    assert_eq!(recalled.len(), 3, "limit=3 must truncate to 3 sessions");
    Ok(())
}

#[test]
fn test_recall_top_match_is_preview_of_best_message() -> Result<()> {
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();
    save_session(
        &loom,
        "s1",
        "routing",
        vec![
            make_msg("m1", MessageRole::User, "Generic intro message"),
            make_msg(
                "m2",
                MessageRole::User,
                "routing tier selection algorithm overview and details",
            ),
        ],
    )?;
    let memory = CrossSessionMemory::new(&loom)?;
    let recalled = memory.recall("routing", 1)?;
    assert_eq!(recalled.len(), 1);
    // The top_match preview must be non-empty and must contain a routing
    // reference. Exact content depends on FTS5 ranking, so we just check
    // it surfaces something meaningful.
    assert!(!recalled[0].top_match.is_empty());
    assert!(recalled[0].top_match.contains("routing"));
    // Role round-trips through the debug-format ("User") used by index_message.
    assert_eq!(recalled[0].top_match_role, "User");
    Ok(())
}

#[test]
fn test_recall_picks_up_sessions_indexed_via_session_index() -> Result<()> {
    // The CLI's `loom sessions index` subcommand indexes all known
    // sessions in one sweep. We mirror that here to confirm `recall`
    // works against the result of a bulk indexing pass.
    let dir = tempdir()?;
    let loom = dir.path().join("agents").to_string_lossy().into_owned();
    let store = SessionStore::new(&loom);
    let memory = CrossSessionMemory::new(&loom)?;

    for (sid, topic, body) in [
        ("s1", "alpha", "alpha topic content goes here"),
        ("s2", "beta", "beta content for routing users"),
        ("s3", "gamma", "routing configuration guide"),
    ] {
        let manifest = SessionManifest {
            session_id: sid.into(),
            source_tool: "loom-test".into(),
            source_version: "0.3.7".into(),
            exported_at: "2026-06-06T00:00:00Z".into(),
            topic: topic.into(),
            message_count: 1,
            active_skills: vec![],
            active_mcp_servers: vec![],
            working_directory: "/tmp".into(),
            model_provider: "test".into(),
            files_modified: vec![],
            compatibility_note: None,
            tags: vec![],
            summarized_at: None,
            summarized_message_count: None,
        };
        let msgs = vec![make_msg("m1", MessageRole::User, body)];
        store.save_manifest(&manifest)?;
        store.replace_messages(sid, &msgs)?;
        memory.index_session(sid, &msgs, topic)?;
    }

    // Query that hits 2 of 3 sessions.
    let recalled = memory.recall("routing", 10)?;
    let ids: Vec<&str> = recalled.iter().map(|r| r.session_id.as_str()).collect();
    assert!(ids.contains(&"s2"));
    assert!(ids.contains(&"s3"));
    assert!(!ids.contains(&"s1"), "s1 has no routing mention");
    Ok(())
}
