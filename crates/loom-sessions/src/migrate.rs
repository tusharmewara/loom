use crate::store::SessionStore;
use anyhow::Result;
use loom_core::SessionManifest;
use std::path::{Path, PathBuf};

/// Migrate legacy `sessions/{id}.json` manifests into `sessions/{id}/manifest.json` + JSONL layout.
pub fn migrate_legacy_sessions(loom_dir: &str) -> Result<usize> {
    let sessions_dir = Path::new(loom_dir).join("sessions");
    if !sessions_dir.exists() {
        return Ok(0);
    }

    let mut migrated = 0;
    for entry in std::fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".json") {
            continue;
        }

        let session_id = name.trim_end_matches(".json").to_string();
        let session_dir = sessions_dir.join(&session_id);
        if session_dir.exists() {
            continue;
        }

        let raw = std::fs::read_to_string(&path)?;
        let manifest: SessionManifest = match serde_json::from_str(&raw) {
            Ok(m) => m,
            Err(_) => continue,
        };

        std::fs::create_dir_all(&session_dir)?;
        let manifest_path = session_dir.join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

        // Legacy messages file: {id}-messages.json (array of Message)
        let legacy_messages = sessions_dir.join(format!("{session_id}-messages.json"));
        if legacy_messages.exists() {
            if let Ok(raw) = std::fs::read_to_string(&legacy_messages) {
                if let Ok(messages) =
                    serde_json::from_str::<Vec<loom_core::Message>>(&raw)
                {
                    let store = SessionStore::new(loom_dir);
                    store.replace_messages(&session_id, &messages)?;
                    let _ = std::fs::remove_file(&legacy_messages);
                }
            }
        }

        let _ = std::fs::remove_file(&path);
        migrated += 1;
        tracing::info!(session_id = %session_id, "migrated legacy session to JSONL layout");
    }

    Ok(migrated)
}

/// Run migration if needed (called from `loom init`).
pub fn migrate_if_needed(loom_dir: &str) -> Result<usize> {
    migrate_legacy_sessions(loom_dir)
}

pub fn legacy_manifest_path(sessions_dir: &Path, id: &str) -> PathBuf {
    sessions_dir.join(format!("{id}.json"))
}
