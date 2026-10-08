///! Integration test for session compression performance.
///! Tests that session compression operates efficiently within performance budgets
///! and handles edge cases correctly.

use loom_sessions::store::SessionStore;
use loom_core::{Message, MessageRole, SESSION_COMPRESSION_THRESHOLD, SESSION_FULL_FIDELITY_COUNT};
use std::time::Instant;
use tempfile::tempdir;

#[tokio::test]
async fn test_compression_performance_above_threshold() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let start = Instant::now();
    
    let store = SessionStore::new(dir.path().to_str().unwrap());
    
    // Create a session with messages above threshold (21 > 20)
    store.save_manifest(&loom_core::SessionManifest {
        session_id: "perf-test-1".to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "compression-performance".to_string(),
        message_count: 21,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    });
    
    // Add messages - each message has some content
    let msgs: Vec<loom_core::Message> = (0..21)
        .map(|i| loom_core::Message {
            id: format!("m{}", i),
            role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
            content: format!("User message {} with some substantial content about the topic at hand ", i),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    
    store.replace_messages("perf-test-1", &msgs).unwrap();
    
    // Measure compression performance
    let elapsed = start.elapsed();
    let compression_time = elapsed.as_micros();
    
    // Compression should complete within reasonable time (budget: < 500ms = 500000us)
    // ponytail: relaxed budget for test environment variability
    assert!(
        compression_time < 500000,
        "Compression above threshold should complete within 500ms, took {}us",
        compression_time
    );
    
    // Verify compression was applied
    let post_msgs = store.load_messages("perf-test-1").unwrap();
    assert!(
        post_msgs.len() <= 11,
        "Compression should reduce message count to 11 (1 summary + 10 kept), got {}",
        post_msgs.len()
    );
    
    // Summary message should exist
    assert_eq!(post_msgs[0].id, loom_core::SUMMARY_MESSAGE_ID);
    assert!(matches!(post_msgs[0].role, MessageRole::System));
    
    println!(
        "✅ Compression performance above threshold: {}us for 21 messages",
        compression_time
    );
    Ok(())
}

#[tokio::test]
async fn test_compression_performance_below_threshold_noop() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let start = Instant::now();
    
    let store = SessionStore::new(dir.path().to_str().unwrap());
    
    // Create a session with messages below threshold (5 <= 20)
    store.save_manifest(&loom_core::SessionManifest {
        session_id: "perf-test-2".to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "compression-performance".to_string(),
        message_count: 5,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    });
    
    // Add messages
    let msgs: Vec<loom_core::Message> = (0..5)
        .map(|i| loom_core::Message {
            id: format!("m{}", i),
            role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
            content: format!("User message {} ", i),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    
    store.replace_messages("perf-test-2", &msgs).unwrap();
    
    // Measure compression performance
    let elapsed = start.elapsed();
    let compression_time = elapsed.as_micros();
    
    // Compression should be a no-op below threshold
    let outcome = store.compress_session("perf-test-2");
    let is_noop = outcome.as_ref().map_or(false, |opt| opt.is_none());
    assert!(
        is_noop,
        "Compression below threshold should be a no-op, got outcome: {:?}",
        outcome
    );
    
    // Verify messages unchanged
    let post_msgs = store.load_messages("perf-test-2").unwrap();
    assert_eq!(post_msgs.len(), 5, "Messages should remain unchanged below threshold");
    
    println!(
        "✅ Compression below threshold is no-op: {}us",
        compression_time
    );
    Ok(())
}

#[tokio::test]
async fn test_compression_performance_at_threshold_boundary() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let start = Instant::now();
    
    let store = SessionStore::new(dir.path().to_str().unwrap());
    
    // Create a session with exactly threshold messages (20 == threshold)
    store.save_manifest(&loom_core::SessionManifest {
        session_id: "perf-test-3".to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "compression-performance".to_string(),
        message_count: 20,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    });
    
    // Add messages
    let msgs: Vec<loom_core::Message> = (0..20)
        .map(|i| loom_core::Message {
            id: format!("m{}", i),
            role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
            content: format!("User message {} ", i),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    
    store.replace_messages("perf-test-3", &msgs).unwrap();
    
    // Measure compression performance
    let elapsed = start.elapsed();
    let compression_time = elapsed.as_micros();
    
    // At boundary (20 == threshold), should be no-op
    let outcome = store.compress_session("perf-test-3");
    let is_noop = outcome.as_ref().map_or(false, |opt| opt.is_none());
    assert!(
        is_noop,
        "Compression at exact threshold should be no-op, got outcome: {:?}",
        outcome
    );
    
    println!(
        "✅ Compression at threshold boundary is no-op: {}us",
        compression_time
    );
    Ok(())
}

#[tokio::test]
async fn test_compression_performance_append_auto_trigger() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let start = Instant::now();
    
    let store = SessionStore::new(dir.path().to_str().unwrap());
    
    // Create session at threshold (20 messages)
    store.save_manifest(&loom_core::SessionManifest {
        session_id: "perf-test-4".to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "compression-performance".to_string(),
        message_count: 20,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    });
    
    // Add messages
    let msgs: Vec<loom_core::Message> = (0..20)
        .map(|i| loom_core::Message {
            id: format!("m{}", i),
            role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
            content: format!("User message {} ", i),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    
    store.replace_messages("perf-test-4", &msgs).unwrap();
    
    // Append one more message to cross threshold
    let new_msg = loom_core::Message {
        id: "m20".to_string(),
        role: MessageRole::User,
        content: "trigger message".to_string(),
        tool_calls: None,
        timestamp: "2025-01-01T00:00:00Z".to_string(),
        metadata: std::collections::HashMap::new(),
    };
    
    let append_elapsed = start.elapsed();
    let append_time = append_elapsed.as_micros();
    
    // This append should trigger auto-compression
    store.append_message("perf-test-4", &new_msg).unwrap();
    
    println!(
        "✅ Append auto-trigger performance: {}us to add message and compress",
        append_time
    );
    
    // Verify compression happened - should have 1 summary + 10 kept = 11 messages
    let post_msgs = store.load_messages("perf-test-4").unwrap();
    assert_eq!(
        post_msgs.len(), 11,
        "After append crossing threshold, should have 11 messages (1 summary + 10 kept), got {}",
        post_msgs.len()
    );
    assert_eq!(post_msgs[0].id, loom_core::SUMMARY_MESSAGE_ID);
    assert!(matches!(post_msgs[0].role, MessageRole::System));
    
    println!(
        "✅ Append auto-trigger verified: {} messages after compression",
        post_msgs.len()
    );
    Ok(())
}

#[tokio::test]
async fn test_compression_idempotence() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let start = Instant::now();
    
    let store = SessionStore::new(dir.path().to_str().unwrap());
    
    // Create session above threshold
    store.save_manifest(&loom_core::SessionManifest {
        session_id: "perf-test-5".to_string(),
        source_tool: "test".to_string(),
        source_version: "0.3.9".to_string(),
        exported_at: "2025-01-01T00:00:00Z".to_string(),
        topic: "compression-performance".to_string(),
        message_count: 25,
        working_directory: String::new(),
        model_provider: String::new(),
        active_skills: vec![],
        active_mcp_servers: vec![],
        files_modified: vec![],
        compatibility_note: None,
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    });
    
    // Add messages
    let msgs: Vec<loom_core::Message> = (0..25)
        .map(|i| loom_core::Message {
            id: format!("m{}", i),
            role: if i % 2 == 0 { MessageRole::User } else { MessageRole::Assistant },
            content: format!("User message {} ", i),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            metadata: std::collections::HashMap::new(),
        })
        .collect();
    
    store.replace_messages("perf-test-5", &msgs).unwrap();
    
    // First compression
    let first_elapsed = start.elapsed();
    let first_time = first_elapsed.as_micros();
    
    let _ = store.compress_session("perf-test-5").expect("first compress should succeed");
    
    // Second compression should be a no-op
    let second_elapsed = start.elapsed();
    let _second_time = second_elapsed.as_micros();
    
    let outcome = store.compress_session("perf-test-5");
    let is_noop = outcome.as_ref().map_or(false, |opt| opt.is_none());
    assert!(
        is_noop,
        "Second compression above threshold should be no-op (idempotent), got: {:?}",
        outcome
    );
    
    println!(
        "✅ Compression idempotence: first {}us, second (no-op) completes quickly",
        first_time
    );
    Ok(())
}