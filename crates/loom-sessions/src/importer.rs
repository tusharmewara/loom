use crate::exporter::SessionExport;
use crate::store::SessionStore;
use anyhow::{bail, Context, Result};
use std::path::Path;

/// Import a session from a portable export file into the ecosystem store.
pub fn import_session(store: &SessionStore, export_path: &Path) -> Result<String> {
    let raw = std::fs::read_to_string(export_path)
        .with_context(|| format!("read export {}", export_path.display()))?;
    let export: SessionExport = serde_json::from_str(&raw)
        .with_context(|| "parse session export (expected SessionExport JSON)")?;

    if export.format_version != crate::exporter::EXPORT_FORMAT_VERSION {
        tracing::warn!(
            "export format {} differs from supported {}",
            export.format_version,
            crate::exporter::EXPORT_FORMAT_VERSION
        );
    }

    let session_id = export.manifest.session_id.clone();
    store.save_manifest(&export.manifest)?;
    store.replace_messages(&session_id, &export.messages)?;
    Ok(session_id)
}

/// Import from raw JSON string (for IPC/tests).
pub fn import_session_json(store: &SessionStore, raw: &str) -> Result<String> {
    let export: SessionExport = serde_json::from_str(raw).context("parse session export JSON")?;
    if export.manifest.session_id.is_empty() {
        bail!("export manifest missing session_id");
    }
    let session_id = export.manifest.session_id.clone();
    store.save_manifest(&export.manifest)?;
    store.replace_messages(&session_id, &export.messages)?;
    Ok(session_id)
}
