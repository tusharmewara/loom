use crate::cli::SessionsAction;
use crate::commands::resume;
use loom_sessions::{export_session, import_session, write_export, SessionManager};
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &SessionsAction) -> anyhow::Result<()> {
    let manager = SessionManager::new(loom_dir.to_str().unwrap());
    let store = manager.store();

    match action {
        SessionsAction::List { json } => {
            let manifests = store.list();
            if *json {
                let sessions: Vec<_> = manifests.iter().map(|m| {
                    serde_json::json!({
                        "session_id": m.session_id,
                        "source_tool": m.source_tool,
                        "message_count": m.message_count,
                        "topic": m.topic,
                        "exported_at": m.exported_at,
                        "source_version": m.source_version,
                        "working_directory": m.working_directory,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.list",
                    "count": sessions.len(),
                    "sessions": sessions
                }))?);
            } else {
                for manifest in &manifests {
                    println!(
                        "{}  tool={}  messages={}  topic={}",
                        manifest.session_id, manifest.source_tool, manifest.message_count, manifest.topic
                    );
                }
                if manifests.is_empty() {
                    println!("No sessions found.");
                }
            }
        }
        SessionsAction::Show { id, json } => {
            let manifest = store
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session '{id}' not found"))?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.show",
                    "session": manifest
                }))?);
            } else {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
            }
        }
        SessionsAction::Export { id, output } => {
            let export = export_session(store, id)?;
            let out_path = output
                .as_ref()
                .map(Path::new)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| {
                    loom_dir
                        .join("sessions")
                        .join("exports")
                        .join(format!("{id}.loom.json"))
                });
            write_export(&export, &out_path)?;
            println!("Exported session '{}' to {}", id, out_path.display());
        }
        SessionsAction::Import { path } => {
            let session_id = import_session(store, Path::new(path))?;
            println!("Imported session '{session_id}'");
        }
        SessionsAction::Resume {
            id,
            provider,
            model,
            agent,
        } => {
            resume::resume_session(
                loom_dir,
                id,
                provider.as_deref(),
                model.as_deref(),
                agent.as_deref(),
            )
            .await?;
        }
    }
    Ok(())
}
