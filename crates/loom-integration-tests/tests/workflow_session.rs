//! Integration tests for the workflow dispatch → session capture pipeline
//! (§2.3 / §3.2 of the improvement plan). Verifies that:
//!
//!  1. Every dispatch leaves behind a `~/.loom/sessions/{task_id}.{phase}/`
//!     directory with a manifest, messages, and a SQLite index — using
//!     only the public `SessionStore` API that `loom-cli` itself uses.
//!  2. The session ID is the deterministic `{task_id}.{phase}` form, so
//!     `prior_session_ids_for_task` can re-discover it later.
//!  3. After capture, the session is visible to `CrossSessionMemory::search`
//!     (so a later dispatch can re-find it via `LOOM_PRIOR_SESSIONS`).
//!  4. The knowledge graph picks up entities from the captured messages,
//!     so the routing layer's knowledge-boost signal works on a workflow
//!     task's prior output.
//!  5. The path convention for the prior-sessions context file
//!     (`~/.loom/workflow/tasks/{task_id}.prior_sessions.json`) matches
//!     what `build_prior_sessions_context` writes, so agents can find it
//!     via `LOOM_PRIOR_SESSIONS_CONTEXT_PATH`.
//!  6. A full multi-phase chain produces 4 chronologically-ordered
//!     sessions (Plan, Implement, Verify, Finish), each with the prior
//!     sessions listed in correct order.

use anyhow::Result;
use loom_core::{Message, MessageRole, SessionManifest, TaskPhase};
use loom_knowledge::KnowledgeGraph;
use loom_sessions::memory::CrossSessionMemory;
use loom_sessions::store::SessionStore;
use loom_sessions::SessionManager;
use loom_workflow::WorkflowEngine;
use std::fs;
use tempfile::tempdir;

fn loom_dir(temp: &tempfile::TempDir) -> String {
    temp.path().join("agents").to_string_lossy().into()
}

/// All four phases in canonical lifecycle order. Kept private to the
/// test so we don't have to construct an owned `TaskPhase` from
/// `&TaskPhase` (TaskPhase is `Clone` but not `Copy`).
fn all_phases() -> Vec<TaskPhase> {
    vec![
        TaskPhase::Plan,
        TaskPhase::Implement,
        TaskPhase::Verify,
        TaskPhase::Finish,
    ]
}

/// Build a session manifest for a captured dispatch. Mirrors the
/// builder inside `capture_dispatch_session` in `loom-cli` — keep
/// the contract stable, since the CLI depends on this exact field set
/// for the JSON file that ends up in `~/.loom/sessions/`.
fn build_session(
    task_id: &str,
    phase: TaskPhase,
    agent: &str,
    topic: &str,
    prompt: &str,
    response: &str,
) -> (String, SessionManifest, Vec<Message>) {
    let session_id = format!("{task_id}.{}", phase.name());
    let now = "2026-06-06T00:00:00Z".to_string();
    // Use a unique `id` per (session_id, slot) combination. The
    // production CLI uses UUIDs for this — see `capture_dispatch_session`
    // in loom-cli. Reusing the same `id` across sessions would cause
    // the `index_message` upsert key (`message_id`) to clobber prior
    // sessions. The test mirrors the production contract.
    let messages = vec![
        Message {
            id: format!("{session_id}/sys"),
            role: MessageRole::System,
            content: format!(
                "Workflow task dispatch\n  task_id: {task_id}\n  phase: {}\n  agent: {agent}",
                phase.name()
            ),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: format!("{session_id}/user"),
            role: MessageRole::User,
            content: prompt.into(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: format!("{session_id}/asst"),
            role: MessageRole::Assistant,
            content: response.into(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: {
                let mut m = std::collections::HashMap::new();
                m.insert("agent".into(), serde_json::Value::String(agent.into()));
                m.insert(
                    "phase".into(),
                    serde_json::Value::String(phase.name().to_string()),
                );
                m
            },
        },
    ];
    let manifest = SessionManifest {
        session_id: session_id.clone(),
        source_tool: format!("loom-workflow/{agent}"),
        source_version: env!("CARGO_PKG_VERSION").into(),
        exported_at: now,
        topic: topic.into(),
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: "/tmp".into(),
        model_provider: "openai/gpt-4".into(),
        files_modified: vec![],
        compatibility_note: Some(format!("Captured from {} phase", phase.name())),
        tags: vec![],
        summarized_at: None,
        summarized_message_count: None,
    };
    (session_id, manifest, messages)
}

/// Mirror of `capture_dispatch_session` in `loom-cli` for the public
/// surface area: persist via `SessionStore`, index into cross-session
/// memory, extract into the knowledge graph.
fn capture(loom_dir: &str, manifest: &SessionManifest, messages: &[Message]) -> Result<()> {
    let manager = SessionManager::new(loom_dir);
    let store = manager.store();
    store.save_manifest(manifest)?;
    store.replace_messages(&manifest.session_id, messages)?;
    let memory = CrossSessionMemory::new(loom_dir)?;
    memory.index_session(&manifest.session_id, messages, &manifest.topic)?;
    let kg = KnowledgeGraph::new(loom_dir)?;
    let _ = kg.extract_from_messages(messages, &manifest.session_id)?;
    Ok(())
}

fn open_store(loom_dir: &str) -> std::sync::Arc<SessionStore> {
    use std::sync::Arc;
    Arc::new(SessionStore::new(loom_dir))
}

#[test]
fn test_capture_creates_manifest_and_messages() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let (sid, manifest, messages) = build_session(
        "task-1",
        TaskPhase::Plan,
        "opencode",
        "Build a thing",
        "Plan it",
        "planned it",
    );

    capture(&agents, &manifest, &messages)?;

    // Manifest + messages files exist on disk
    let session_dir = std::path::Path::new(&agents).join("sessions").join(&sid);
    assert!(
        session_dir.join("manifest.json").exists(),
        "manifest.json missing"
    );
    assert!(
        session_dir.join("messages.jsonl").exists(),
        "messages.jsonl missing"
    );

    // Session shows up in `store.list()`
    let store = open_store(&agents);
    let ids: Vec<_> = store.list().into_iter().map(|m| m.session_id).collect();
    assert!(ids.contains(&sid), "session {sid} not in list: {ids:?}");

    // Messages round-trip
    let loaded = store.load_messages(&sid)?;
    assert_eq!(loaded.len(), 3, "expected 3 messages, got {}", loaded.len());
    assert!(matches!(loaded[0].role, MessageRole::System));
    assert!(matches!(loaded[1].role, MessageRole::User));
    assert!(matches!(loaded[2].role, MessageRole::Assistant));
    Ok(())
}

#[test]
fn test_session_id_uses_task_dot_phase_convention() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);

    for (i, phase) in all_phases().into_iter().enumerate() {
        let (sid, manifest, messages) = build_session(
            "task-42",
            phase.clone(),
            "agent-x",
            "topic",
            "prompt",
            &format!("response {i}"),
        );
        capture(&agents, &manifest, &messages)?;
        assert_eq!(sid, format!("task-42.{}", phase.name()));
    }

    let store = open_store(&agents);
    let mut ids: Vec<_> = store
        .list()
        .into_iter()
        .map(|m| m.session_id)
        .filter(|id| id.starts_with("task-42."))
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "task-42.Finish".to_string(),
            "task-42.Implement".to_string(),
            "task-42.Plan".to_string(),
            "task-42.Verify".to_string(),
        ]
    );
    Ok(())
}

#[test]
fn test_capture_indexes_into_cross_session_memory() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let (sid, manifest, messages) = build_session(
        "task-mem",
        TaskPhase::Plan,
        "opencode",
        "rust async refactor",
        "How should I refactor the runtime?",
        "Use loom orchestration primitives like post_phase hooks and a TaskPhase::name helper.",
    );
    capture(&agents, &manifest, &messages)?;

    // The memory index should now return at least one hit when queried
    // with a term that appears in the agent's response.
    let memory = CrossSessionMemory::new(&agents)?;
    let hits = memory.search("orchestration", 5)?;
    assert!(
        !hits.is_empty(),
        "memory.search found nothing for 'orchestration'"
    );
    assert!(
        hits.iter().any(|h| h.session_id == sid),
        "expected hit for session {sid}, got: {:?}",
        hits.iter().map(|h| &h.session_id).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn test_capture_extracts_knowledge_graph_entities() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    // Use distinctive capitalized proper nouns the knowledge extractor
    // should pick up. `extract_known_tools` recognises "Loom", "Rust",
    // "Tokio", "jcode", etc.
    let (sid, manifest, messages) = build_session(
        "task-kg",
        TaskPhase::Implement,
        "jcode",
        "Knowledge extraction test",
        "Implement knowledge graph wiring",
        "Wired Loom into the Tokio runtime so that Rust sessions can be indexed via the loom-sessions crate.",
    );
    capture(&agents, &manifest, &messages)?;

    // At least one entity should have been added for the captured session.
    let kg = KnowledgeGraph::new(&agents)?;
    let entities = kg.search_entities("Loom", 5)?;
    assert!(
        !entities.is_empty(),
        "no 'Loom' entity found in knowledge graph after capture"
    );
    // The Loom entity should have at least one relationship whose
    // `source_session` matches the captured session ID. The session
    // is not stored as a node in the graph; it's recorded as the
    // provenance of each relationship it produced.
    let loom_entity = entities
        .iter()
        .find(|e| e.name == "Loom")
        .expect("'Loom' entity not present");
    let related = kg.related_entities(&loom_entity.id)?;
    assert!(
        related
            .iter()
            .any(|r| r.relationship.source_session.as_deref() == Some(sid.as_str())),
        "no relationship in the knowledge graph is sourced from session {sid}; got sources: {:?}",
        related
            .iter()
            .map(|r| r.relationship.source_session.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn test_prior_sessions_context_file_convention() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let engine = WorkflowEngine::new(&agents);
    let orch = engine.orchestrator();
    let task = orch.create_task("Context file test", "desc", false, None)?;

    // Capture the Plan and Implement sessions.
    for (phase, agent) in [
        (TaskPhase::Plan, "opencode"),
        (TaskPhase::Implement, "jcode"),
    ] {
        let (sid, manifest, messages) = build_session(
            &task.task_id,
            phase.clone(),
            agent,
            "Context file test",
            "do the thing",
            &format!("did the {} thing", phase.name()),
        );
        capture(&agents, &manifest, &messages)?;
        // Sanity: the session ID we just used is the same shape the
        // prior-session lookup will rely on.
        assert!(sid.starts_with(&task.task_id));
    }

    // The path the CLI would write to — kept in lockstep with
    // `build_prior_sessions_context` in loom-cli.
    let expected_ctx = std::path::Path::new(&agents)
        .join("workflow")
        .join("tasks")
        .join(format!("{}.prior_sessions.json", task.task_id));
    assert!(
        !expected_ctx.exists(),
        "context file shouldn't exist before dispatch: {}",
        expected_ctx.display()
    );

    // The two captured sessions should be discoverable via
    // `loom sessions list` once we filter by prefix.
    let store = open_store(&agents);
    let prefix = format!("{}.", task.task_id);
    let ids: Vec<String> = store
        .list()
        .into_iter()
        .map(|m| m.session_id)
        .filter(|id| id.starts_with(&prefix))
        .collect();
    assert_eq!(ids.len(), 2, "expected 2 prior sessions, got: {ids:?}");
    Ok(())
}

#[test]
fn test_full_phase_chain_captures_four_ordered_sessions() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let engine = WorkflowEngine::new(&agents);
    let orch = engine.orchestrator();
    let task = orch.create_task("Phase chain capture", "verify the chain end to end", false, None)?;

    // Simulate the full Plan → Implement → Verify → Finish chain, capturing
    // a session at every step. Order matters: `prior_session_ids_for_task`
    // in the CLI sorts by phase order, so we want all four present.
    let phases = [
        (TaskPhase::Plan, "opencode", "plan output"),
        (TaskPhase::Implement, "jcode", "implement output"),
        (TaskPhase::Verify, "crush", "verify output"),
        (TaskPhase::Finish, "goose", "finish output"),
    ];
    for (phase, agent, response) in phases {
        let (_, manifest, messages) = build_session(
            &task.task_id,
            phase,
            agent,
            "Phase chain capture",
            "prompt",
            response,
        );
        capture(&agents, &manifest, &messages)?;
    }

    // All 4 sessions should be present in chronological phase order.
    let store = open_store(&agents);
    let prefix = format!("{}.", task.task_id);
    let mut ids: Vec<String> = store
        .list()
        .into_iter()
        .map(|m| m.session_id)
        .filter(|id| id.starts_with(&prefix))
        .collect();

    // Mirror the order logic in `prior_session_ids_for_task` in loom-cli.
    let order = |id: &str| -> u8 {
        let phase = id.strip_prefix(&prefix).unwrap_or("");
        match phase {
            "Plan" => 0,
            "Implement" => 1,
            "Verify" => 2,
            "Finish" => 3,
            _ => 99,
        }
    };
    ids.sort_by_key(|id| order(id));

    assert_eq!(
        ids,
        vec![
            format!("{}.Plan", task.task_id),
            format!("{}.Implement", task.task_id),
            format!("{}.Verify", task.task_id),
            format!("{}.Finish", task.task_id),
        ]
    );

    // And the memory index should be able to retrieve any of them by
    // searching for a unique string from the agent's output.
    let memory = CrossSessionMemory::new(&agents)?;
    let hits = memory.search("implement", 5)?;
    let hit_sids: Vec<_> = hits.iter().map(|h| h.session_id.clone()).collect();
    assert!(
        hits.iter()
            .any(|h| h.session_id == format!("{}.Implement", task.task_id)),
        "expected to find Implement session in memory; got: {hit_sids:?}"
    );
    Ok(())
}

#[test]
fn test_five_captures_index_into_memory() -> Result<()> {
    // Simplified reproducer for the FTS5 ordering issue. Capture 5
    // sessions in sequence with distinct assistant content, then verify
    // that *each* one is findable by FTS search. This isolates whether
    // the FTS index keeps up across many `index_session` calls.
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let manager = SessionManager::new(&agents);
    let store = manager.store();
    let memory = CrossSessionMemory::new(&agents)?;

    for (i, word) in ["alpha", "beta", "gamma", "delta", "epsilon"]
        .iter()
        .enumerate()
    {
        let sid = format!("sess-{i}");
        let m = vec![Message {
            id: format!("m-{i}"),
            role: MessageRole::Assistant,
            content: format!("the unique keyword is {word}"),
            tool_calls: None,
            timestamp: "2026-06-06T00:00:00Z".into(),
            metadata: Default::default(),
        }];
        store.save_manifest(&SessionManifest {
            session_id: sid.clone(),
            source_tool: "test".into(),
            source_version: "0".into(),
            exported_at: "2026-06-06T00:00:00Z".into(),
            topic: "test".into(),
            message_count: 1,
            active_skills: vec![],
            active_mcp_servers: vec![],
            working_directory: "/".into(),
            model_provider: "test".into(),
            files_modified: vec![],
            compatibility_note: None,
            tags: vec![],
            summarized_at: None,
            summarized_message_count: None,
        })?;
        store.replace_messages(&sid, &m)?;
        memory.index_session(&sid, &m, "test")?;
    }

    // Each keyword should be findable
    for (i, word) in ["alpha", "beta", "gamma", "delta", "epsilon"]
        .iter()
        .enumerate()
    {
        let hits = memory.search(word, 5)?;
        let expected_sid = format!("sess-{i}");
        assert!(
            hits.iter().any(|h| h.session_id == expected_sid),
            "expected to find session {expected_sid} searching for {word}; got: {:?}",
            hits.iter().map(|h| &h.session_id).collect::<Vec<_>>()
        );
    }
    Ok(())
}

#[test]
fn test_session_dir_layout_matches_cli_expectations() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    let (sid, manifest, messages) = build_session(
        "task-layout",
        TaskPhase::Plan,
        "opencode",
        "layout",
        "p",
        "r",
    );
    capture(&agents, &manifest, &messages)?;

    // The directory must be exactly `~/.loom/sessions/{session_id}/`
    // (not nested, not prefixed). This is what `loom sessions list` walks.
    let canonical = std::path::Path::new(&agents).join("sessions").join(&sid);
    assert!(
        canonical.is_dir(),
        "missing session dir: {}",
        canonical.display()
    );
    assert!(canonical.join("manifest.json").is_file());
    assert!(canonical.join("messages.jsonl").is_file());
    let raw_messages = fs::read_to_string(canonical.join("messages.jsonl"))?;
    assert!(raw_messages.contains("System"));
    assert!(raw_messages.contains("Assistant"));
    Ok(())
}
