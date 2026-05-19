use crate::store::SessionStore;
use anyhow::{Context, Result};
use loom_core::{Message, SessionManifest};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const EXPORT_FORMAT_VERSION: &str = "1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionExport {
    pub format_version: String,
    pub manifest: SessionManifest,
    pub messages: Vec<Message>,
    pub environment: std::collections::HashMap<String, String>,
}

pub fn export_session(store: &SessionStore, session_id: &str) -> Result<SessionExport> {
    let manifest = store
        .get(session_id)
        .with_context(|| format!("session '{session_id}' not found"))?
        .clone();
    let messages = store.load_messages(session_id)?;
    let environment = crate::environment::reconstruct_environment(&manifest);

    Ok(SessionExport {
        format_version: EXPORT_FORMAT_VERSION.to_string(),
        manifest,
        messages,
        environment,
    })
}

pub fn write_export(export: &SessionExport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(export)?;
    std::fs::write(path, raw)?;
    Ok(())
}
