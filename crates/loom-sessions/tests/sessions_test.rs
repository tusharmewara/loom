use loom_core::{Message, MessageRole, SessionManifest};
use loom_sessions::{
    export_session, import_session, migrate_if_needed, store::SessionStore, write_export,
};
use tempfile::tempdir;

fn sample_manifest(id: &str) -> SessionManifest {
    SessionManifest {
        session_id: id.to_string(),
        source_tool: "test".into(),
        source_version: "1.0".into(),
        exported_at: "2026-01-01T00:00:00Z".into(),
        topic: "test topic".into(),
        message_count: 1,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: "/tmp".into(),
        model_provider: "openai".into(),
        files_modified: vec![],
        compatibility_note: None,
    }
}

fn sample_message() -> Message {
    Message {
        id: "m1".into(),
        role: MessageRole::User,
        content: "hello".into(),
        tool_calls: None,
        timestamp: "2026-01-01T00:00:00Z".into(),
        metadata: Default::default(),
    }
}

#[test]
fn test_jsonl_roundtrip() {
    let dir = tempdir().unwrap();
    let store = SessionStore::new(dir.path().to_str().unwrap());
    let manifest = sample_manifest("sess-1");
    store.save_manifest(&manifest).unwrap();
    store
        .append_message("sess-1", &sample_message())
        .unwrap();

    let messages = store.load_messages("sess-1").unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "hello");
}

#[test]
fn test_export_import() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let mut store = SessionStore::new(agents);
    store.save_manifest(&sample_manifest("sess-2")).unwrap();
    store.replace_messages("sess-2", &[sample_message()]).unwrap();

    let export = export_session(&store, "sess-2").unwrap();
    let export_path = dir.path().join("export.json");
    write_export(&export, &export_path).unwrap();

    let store2 = SessionStore::new(agents);
    let id = import_session(&store2, &export_path).unwrap();
    assert_eq!(id, "sess-2");
    assert_eq!(store2.load_messages("sess-2").unwrap().len(), 1);
}

#[test]
fn test_legacy_migration() {
    let dir = tempdir().unwrap();
    let agents = dir.path().to_str().unwrap();
    let legacy = dir.path().join("sessions").join("old.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, serde_json::to_string(&sample_manifest("old")).unwrap()).unwrap();

    let n = migrate_if_needed(agents).unwrap();
    assert_eq!(n, 1);

    let store = SessionStore::new(agents);
    assert!(store.get("old").is_some());
    assert!(!legacy.exists());
}
