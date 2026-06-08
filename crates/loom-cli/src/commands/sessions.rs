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
        SessionsAction::List { json, tag } => {
            let manifests = store.list();
            // Apply tag filter to the manifest list. We keep the
            // memory's index as the source of truth for which sessions
            // match — the manifest's `tags` field may be stale relative
            // to the memory store if the user only edited tags via the
            // `tag` command without re-running `sessions index`.
            let filtered: Vec<loom_core::SessionManifest> = if tag.is_empty() {
                manifests
            } else {
                let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
                let allowed = memory.session_ids_with_tags(Some(tag))?;
                let allowed = allowed.unwrap_or_default();
                let allowed_set: std::collections::HashSet<String> =
                    allowed.into_iter().collect();
                manifests
                    .into_iter()
                    .filter(|m| allowed_set.contains(&m.session_id))
                    .collect()
            };
            if *json {
                let sessions: Vec<_> = filtered.iter().map(|m| {
                    serde_json::json!({
                        "session_id": m.session_id,
                        "source_tool": m.source_tool,
                        "message_count": m.message_count,
                        "topic": m.topic,
                        "tags": m.tags,
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
                for manifest in &filtered {
                    let tag_str = if manifest.tags.is_empty() {
                        String::new()
                    } else {
                        format!("  tags=[{}]", manifest.tags.join(","))
                    };
                    let summary_str = match manifest.summarized_message_count {
                        Some(n) if n > 0 => format!(
                            "  summary=[{} folded, last_summary={}]",
                            n,
                            manifest
                                .summarized_at
                                .as_deref()
                                .unwrap_or("?")
                        ),
                        _ => String::new(),
                    };
                    println!(
                        "{}  tool={}  messages={}  topic={}{}{}",
                        manifest.session_id,
                        manifest.source_tool,
                        manifest.message_count,
                        manifest.topic,
                        tag_str,
                        summary_str
                    );
                }
                if filtered.is_empty() {
                    if tag.is_empty() {
                        println!("No sessions found.");
                    } else {
                        println!("No sessions match tag filter: {}", tag.join(","));
                    }
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
                        if let Err(e) = memory.index_session_with_tags(
                            id,
                            &messages,
                            &topic,
                            &export.manifest.tags,
                        ) {
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
            inject_knowledge,
        } => {
            resume::resume_session(
                loom_dir,
                id,
                provider.as_deref(),
                model.as_deref(),
                agent.as_deref(),
                *inject_memory,
                *inject_knowledge,
            )
            .await?;
        }
        SessionsAction::Search { query, limit, json, tag } => {
            let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
            let filter_slice: Option<Vec<String>> = if tag.is_empty() {
                None
            } else {
                Some(tag.clone())
            };
            let results = memory.search_with_tags(query, *limit, filter_slice.as_deref())?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.search",
                    "query": query,
                    "tag_filter": tag,
                    "count": results.len(),
                    "results": results,
                }))?);
            } else {
                if results.is_empty() {
                    if tag.is_empty() {
                        println!("No results found for query: {query}");
                    } else {
                        println!("No results found for query '{query}' with tag filter: {}", tag.join(","));
                    }
                } else {
                    let header = if tag.is_empty() {
                        format!("Cross-session search results for '{}':", query)
                    } else {
                        format!("Cross-session search results for '{}' (tags: {}):", query, tag.join(","))
                    };
                    println!("{header}");
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
        SessionsAction::Recall { query, limit, json, tag } => {
            let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
            let filter_slice: Option<Vec<String>> = if tag.is_empty() {
                None
            } else {
                Some(tag.clone())
            };
            let results = memory.recall_with_tags(query, *limit, filter_slice.as_deref())?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.recall",
                    "query": query,
                    "tag_filter": tag,
                    "count": results.len(),
                    "results": results,
                }))?);
            } else {
                if results.is_empty() {
                    if tag.is_empty() {
                        println!("No sessions recalled for query: {query}");
                    } else {
                        println!("No sessions recalled for query '{query}' with tag filter: {}", tag.join(","));
                    }
                } else {
                    let header = if tag.is_empty() {
                        format!("Recalled sessions for '{}':", query)
                    } else {
                        format!("Recalled sessions for '{}' (tags: {}):", query, tag.join(","))
                    };
                    println!("{header}");
                    for (i, r) in results.iter().enumerate() {
                        let preview = r.top_match.replace('\n', " ");
                        let topic = if r.topic.is_empty() {
                            "(no topic)".to_string()
                        } else {
                            format!("topic=\"{}\"", r.topic)
                        };
                        println!(
                            "  {}. [{}]  {} hits  {}  top[{:.3}]: {:.200}",
                            i + 1,
                            r.session_id,
                            r.hit_count,
                            topic,
                            r.top_score,
                            preview,
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
                        // Pass manifest tags into the memory index so
                        // `list --tag`, `search --tag`, `recall --tag`
                        // work without requiring a separate `tag` step.
                        if let Err(e) = memory.index_session_with_tags(
                            &manifest.session_id,
                            &messages,
                            &manifest.topic,
                            &manifest.tags,
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
        SessionsAction::Tag { id, add, remove } => {
            // Validate the session exists before mutating.
            let mut manifest = store
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session '{id}' not found"))?
                .clone();

            // Re-derive the new manifest tags by applying the requested
            // adds/removes through the same normalization the store
            // helper uses. This keeps the in-memory tag set in sync
            // with what the user typed.
            let current: Vec<String> = manifest.tags.clone();
            let mut next: Vec<String> = current
                .into_iter()
                .filter(|t| !remove.iter().any(|r| r.trim() == t))
                .collect();
            for t in add {
                let trimmed = t.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if !next.iter().any(|x| x == trimmed) {
                    next.push(trimmed.to_string());
                }
            }
            let added: Vec<String> = next
                .iter()
                .filter(|t| !manifest.tags.contains(t))
                .cloned()
                .collect();
            let removed: Vec<String> = remove
                .iter()
                .filter(|t| manifest.tags.contains(t))
                .cloned()
                .collect();

            manifest.tags = next.clone();
            store.save_manifest(&manifest)?;

            // Mirror the new tag set into the cross-session memory
            // store so `--tag` filters on search/recall/list see it
            // immediately. We do a full replace (not a diff) so this
            // command is idempotent and self-healing.
            if let Ok(memory) = CrossSessionMemory::new(loom_dir.to_str().unwrap()) {
                if let Err(e) = memory.set_session_tags(id, &next) {
                    eprintln!("  Warning: failed to update tags in memory index: {e}");
                }
            }

            // Report what actually changed.
            if added.is_empty() && removed.is_empty() {
                println!("No changes. Current tags for '{id}': [{}]", next.join(","));
            } else {
                if !added.is_empty() {
                    println!("Added: [{}]", added.join(","));
                }
                if !removed.is_empty() {
                    println!("Removed: [{}]", removed.join(","));
                }
                println!("Current tags for '{id}': [{}]", next.join(","));
            }
        }
        SessionsAction::Tags { json } => {
            let memory = CrossSessionMemory::new(loom_dir.to_str().unwrap())?;
            let facets = memory.tag_facets()?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.tags",
                    "count": facets.len(),
                    "tags": facets.into_iter().map(|(t, n)| serde_json::json!({
                        "tag": t,
                        "session_count": n,
                    })).collect::<Vec<_>>()
                }))?);
            } else {
                if facets.is_empty() {
                    println!("No tags indexed yet. Use `loom sessions tag <id> --add <t>` to add tags.");
                } else {
                    println!("Tags in cross-session memory:");
                    for (tag, n) in &facets {
                        println!("  {:<32}  {} session{}", tag, n, if *n == 1 { "" } else { "s" });
                    }
                }
            }
        }
        SessionsAction::Summarize { id, force, json } => {
            // The store's compress_session is the public entry point;
            // it handles the threshold check, manifest update, and
            // message rewrite atomically. We just translate its
            // `Option<usize>` (Some(folded) / None for no-op) into a
            // useful user-facing message.
            let pre_count = store
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session '{id}' not found"))?
                .message_count;
            if pre_count == 0 {
                anyhow::bail!("Session '{id}' has no messages to summarize");
            }
            if !force && pre_count as usize <= loom_core::SESSION_COMPRESSION_THRESHOLD {
                if *json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "sessions.summarize",
                        "session_id": id,
                        "status": "skipped",
                        "reason": format!(
                            "message_count ({pre_count}) is at or below threshold ({})",
                            loom_core::SESSION_COMPRESSION_THRESHOLD
                        ),
                        "message_count": pre_count,
                    }))?);
                } else {
                    println!(
                        "Session '{id}' has {pre_count} messages (threshold: {}). No compression needed. Use --force to override.",
                        loom_core::SESSION_COMPRESSION_THRESHOLD
                    );
                }
                return Ok(());
            }
            let outcome = store.compress_session(id)?;
            let post = store
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("Session '{id}' disappeared after compress"))?;
            // Re-index the session into the cross-session memory so
            // the FTS view reflects the compressed state. Without
            // this, search would still return hits from the
            // (now-folded) original messages.
            if let Ok(memory) = CrossSessionMemory::new(loom_dir.to_str().unwrap()) {
                if let Ok(msgs) = store.load_messages(id) {
                    if let Err(e) =
                        memory.index_session_with_tags(id, &msgs, &post.topic, &post.tags)
                    {
                        eprintln!("  Warning: failed to re-index compressed session in memory: {e}");
                    }
                }
            }
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "sessions.summarize",
                    "session_id": id,
                    "status": if outcome.is_some() { "compressed" } else { "no_op" },
                    "folded_count": outcome,
                    "pre_compress_count": pre_count,
                    "post_compress_count": post.message_count,
                    "summarized_at": post.summarized_at,
                    "summarized_message_count": post.summarized_message_count,
                }))?);
            } else {
                match outcome {
                    Some(folded) => {
                        // The new message_count includes the summary
                        // message itself, so `kept` is one less. The
                        // number of full-fidelity messages equals the
                        // `SESSION_FULL_FIDELITY_COUNT` constant for
                        // sessions that triggered the auto-summarizer
                        // (i.e. crossed the threshold by at least one
                        // message); a `--force` run on a session
                        // that's still under the threshold will not
                        // happen because we early-return above, so
                        // `kept` here is reliably
                        // `SESSION_FULL_FIDELITY_COUNT`.
                        let kept = loom_core::SESSION_FULL_FIDELITY_COUNT;
                        println!("Compressed session '{id}': folded {folded} earlier message{} into a single summary, kept {kept} message{} verbatim.",
                            if folded == 1 { "" } else { "s" },
                            if kept == 1 { "" } else { "s" });
                        println!("  summarized_at: {}", post.summarized_at.as_deref().unwrap_or("?"));
                    }
                    None => {
                        println!("No compression needed for '{id}' (message_count={}).", pre_count);
                    }
                }
            }
        }
        SessionsAction::Merge { dest, source, json } => {
            cmd_merge(&loom_dir, &store, dest, source, *json)?
        }
    }
    Ok(())
}

fn cmd_merge(
    _loom_dir: &Path,
    store: &loom_sessions::store::SessionStore,
    dest: &str,
    source: &str,
    json: bool,
) -> anyhow::Result<()> {
    // Validate both sessions exist
    let _dest_manifest = store
        .get(dest)
        .ok_or_else(|| anyhow::anyhow!("Destination session '{dest}' not found"))?;
    let _source_manifest = store
        .get(source)
        .ok_or_else(|| anyhow::anyhow!("Source session '{source}' not found"))?;

    // Load source messages
    let source_msgs = store.load_messages(source)?;
    if source_msgs.is_empty() {
        anyhow::bail!("Source session '{source}' has no messages to merge");
    }

    let source_count = source_msgs.len();

    // Append each source message to the destination session
    for msg in &source_msgs {
        store.append_message(dest, msg)?;
    }

    let dest_after = store
        .get(dest)
        .map(|m| m.message_count)
        .unwrap_or(0);

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "command": "sessions.merge",
            "dest": dest,
            "source": source,
            "source_messages_merged": source_count,
            "dest_message_count_after": dest_after,
        }))?);
    } else {
        println!("✓ Merged {source_count} message(s) from '{source}' into '{dest}'");
        println!("  '{dest}' now has {dest_after} message(s)");
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
    // Inherit tags from the source session so a branch stays filterable
    // under the same tags. If you branch with the intent of starting a
    // new topic, `loom sessions tag` can remove or change them later.
    let branch_tags = source_manifest.tags.clone();
    branch_manifest.tags = branch_tags.clone();

    // Save the new session
    store.save_manifest(&branch_manifest)?;
    store.replace_messages(&branch_id, &branch_messages)?;

    println!("✓ Branched session '{}' from '{}'", branch_id, from_id);
    println!("  Source messages: {} (up to checkpoint {})", cp, cp);
    println!("  Branch messages: {}", branch_messages.len());
    println!("  Topic: {}", branch_manifest.topic);

    // Auto-index into cross-session memory
    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir.to_str().unwrap()) {
        if let Err(e) = memory.index_session_with_tags(
            &branch_id,
            &branch_messages,
            &branch_manifest.topic,
            &branch_tags,
        ) {
            eprintln!("  Warning: failed to index branch in memory: {e}");
        }
    }

    Ok(())
}
