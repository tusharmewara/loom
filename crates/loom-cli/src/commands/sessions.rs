use crate::cli::SessionsAction;
use crate::commands::resume;
use loom_sessions::memory::CrossSessionMemory;
use loom_sessions::{export_session, import_session, write_export, SessionManager};
use std::path::Path;

/// Extract entities and relationships from loaded messages into the knowledge graph.
fn extract_to_knowledge(loom_dir: &Path, session_id: &str, messages: &[loom_core::Message]) {
    if messages.is_empty() {
        return;
    }
    match loom_knowledge::KnowledgeGraph::new(loom_dir.to_str().unwrap()) {
        Ok(kg) => {
            match kg.extract_from_messages(messages, session_id) {
                Ok((entities, rels)) => {
                    if entities > 0 || rels > 0 {
                        println!(
                            "  Extracted {} entities, {} relationships into knowledge graph",
                            entities, rels
                        );
                    }
                }
                Err(e) => eprintln!("  Warning: knowledge extraction failed: {e}"),
            }
        }
        Err(e) => eprintln!("  Warning: could not open knowledge graph: {e}"),
    }
}

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

            // Auto-index the exported session into cross-session memory
            if let Ok(memory) = CrossSessionMemory::new(loom_dir.to_str().unwrap()) {
                if let Ok(messages) = store.load_messages(id) {
                    if !messages.is_empty() {
                        let topic = export.manifest.topic.clone();
                        if let Err(e) = memory.index_session(id, &messages, &topic) {
                            eprintln!("  Warning: failed to index session in memory: {e}");
                        } else {
                            println!("  Indexed {} messages into cross-session memory", messages.len());
                        }
                        // Auto-extract entities/relationships into knowledge graph
                        extract_to_knowledge(loom_dir, id, &messages);
                    }
                }
            }
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
            inject_memory,
        } => {
            resume::resume_session(
                loom_dir,
                id,
                provider.as_deref(),
                model.as_deref(),
                agent.as_deref(),
                *inject_memory,
            )
            .await?;
        }
        SessionsAction::Search { query, limit, json } => {
            let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
            let results = memory.search(query, *limit)?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.search",
                    "query": query,
                    "count": results.len(),
                    "results": results,
                }))?);
            } else {
                if results.is_empty() {
                    println!("No results found for query: {query}");
                } else {
                    println!("Cross-session search results for '{}':", query);
                    for (i, hit) in results.iter().enumerate() {
                        println!(
                            "  {}. [{}] {} — {:.120}",
                            i + 1,
                            hit.session_id,
                            hit.role,
                            hit.content.replace('\n', " "),
                        );
                    }
                }
            }
        }
        SessionsAction::Index => {
            let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
            let manifests = store.list();
            let mut total = 0usize;
            for manifest in &manifests {
                if let Ok(messages) = store.load_messages(&manifest.session_id) {
                    if !messages.is_empty() {
                        if let Err(e) = memory.index_session(
                            &manifest.session_id,
                            &messages,
                            &manifest.topic,
                        ) {
                            eprintln!("  Warning: failed to index '{}': {e}", manifest.session_id);
                        } else {
                            total += messages.len();
                        }
                        // Auto-extract entities/relationships into knowledge graph
                        extract_to_knowledge(loom_dir, &manifest.session_id, &messages);
                    }
                }
            }
            println!("Indexed {} messages from {} sessions", total, manifests.len());
        }
        SessionsAction::Branch { id, from, checkpoint } => {
            cmd_branch(loom_dir, store, id, from, *checkpoint)?;
        }
    }
    Ok(())
}

fn cmd_branch(
    loom_dir: &Path,
    store: &loom_sessions::store::SessionStore,
    new_id: &Option<String>,
    from_id: &str,
    checkpoint: Option<usize>,
) -> anyhow::Result<()> {
    // Validate source session exists
    let source_manifest = store
        .get(from_id)
        .ok_or_else(|| anyhow::anyhow!("Source session '{}' not found", from_id))?;

    // Load source messages
    let messages = store.load_messages(from_id)?;
    let total_messages = messages.len();

    // Determine checkpoint
    let cp = checkpoint.unwrap_or(total_messages);
    if cp > total_messages {
        anyhow::bail!(
            "Checkpoint {} exceeds message count {} for session '{}'",
            cp,
            total_messages,
            from_id
        );
    }

    // Truncate at checkpoint
    let branch_messages: Vec<_> = messages.into_iter().take(cp).collect();

    // Generate new session ID
    let branch_id = new_id
        .clone()
        .unwrap_or_else(|| format!("{}-branch-{}", from_id, uuid::Uuid::new_v4().to_string().split('-').next().unwrap()));

    // Create branch manifest (clone source then override)
    let mut branch_manifest = source_manifest.clone();
    branch_manifest.session_id = branch_id.clone();
    branch_manifest.message_count = branch_messages.len() as u32;
    branch_manifest.topic = format!(
        "Branch of '{}' at message {}",
        source_manifest.topic,
        cp,
    );
    branch_manifest.exported_at = chrono::Utc::now().to_rfc3339();

    // Save the new session
    store.save_manifest(&branch_manifest)?;
    store.replace_messages(&branch_id, &branch_messages)?;

    println!("✓ Branched session '{}' from '{}'", branch_id, from_id);
    println!("  Source messages: {} (up to checkpoint {})", cp, cp);
    println!("  Branch messages: {}", branch_messages.len());
    println!("  Topic: {}", branch_manifest.topic);

    // Auto-index into cross-session memory
    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir.to_str().unwrap()) {
        if let Err(e) = memory.index_session(&branch_id, &branch_messages, &branch_manifest.topic) {
            eprintln!("  Warning: failed to index branch in memory: {e}");
        }
    }

    Ok(())
}
