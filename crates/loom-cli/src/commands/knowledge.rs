use crate::cli::KnowledgeAction;
use loom_knowledge::KnowledgeGraph;
use std::path::Path;

pub async fn run(loom_dir: &str, action: &KnowledgeAction) -> anyhow::Result<()> {
    match action {
        KnowledgeAction::Build { json, llm } => {
            let kg = KnowledgeGraph::new(loom_dir)?;

            // Scan sessions directory for indexed sessions
            let sessions_dir = Path::new(loom_dir).join("sessions");
            let mut total_entities = 0usize;
            let mut total_rels = 0usize;
            let mut session_count = 0usize;

            if *llm {
                // LLM-based extraction — batch all session content per session
                if sessions_dir.exists() {
                    for entry in std::fs::read_dir(&sessions_dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                            || path.extension().and_then(|e| e.to_str()) == Some("json")
                        {
                            let content = std::fs::read_to_string(&path)?;
                            let session_id = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let messages = parse_session_content(&content);
                            if messages.is_empty() {
                                continue;
                            }

                            // Combine messages into an LLM prompt
                            let combined: String = messages
                                .iter()
                                .map(|m| m.content.as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let prompt = build_extraction_prompt(&combined);

                            match call_llm_for_extraction(&prompt).await {
                                Ok(response) => {
                                    match kg.extract_from_llm_response(&response, &session_id) {
                                        Ok((e, r)) => {
                                            total_entities += e;
                                            total_rels += r;
                                            session_count += 1;
                                            tracing::info!(
                                                "[{}] LLM extracted: {} entities, {} relationships",
                                                &session_id[..session_id.len().min(12)],
                                                e, r
                                            );
                                        }
                                        Err(parse_err) => {
                                            tracing::warn!(
                                                "[{}] Parse warning: {}. Raw: {}",
                                                &session_id[..session_id.len().min(12)],
                                                parse_err,
                                                &response[..response.len().min(200)]
                                            );
                                        }
                                    }
                                }
                                Err(llm_err) => {
                                    tracing::warn!(
                                        "[{}] LLM call failed: {}",
                                        &session_id[..session_id.len().min(12)],
                                        llm_err
                                    );
                                }
                            }
                        }
                    }
                }
            } else {
                for entry in std::fs::read_dir(&sessions_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                        || path.extension().and_then(|e| e.to_str()) == Some("json")
                    {
                        let content = std::fs::read_to_string(&path)?;
                        let session_id = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Parse messages from the session file
                        let messages = parse_session_content(&content);
                        if messages.is_empty() {
                            continue;
                        }

                        let (e, r) = kg.extract_from_messages(&messages, &session_id)?;
                        total_entities += e;
                        total_rels += r;
                        session_count += 1;
                    }
                }

                // Also try session directories with subfolders
                if session_count == 0 {
                    // Check for session data files in nested structure
                    if let Ok(dir) = std::fs::read_dir(&sessions_dir) {
                        for entry in dir.flatten() {
                            let path = entry.path();
                            if path.is_dir() {
                                for sub in std::fs::read_dir(&path)? {
                                    let sub = sub?;
                                    if sub.path().extension().and_then(|e| e.to_str()) == Some("jsonl")
                                        || sub
                                            .path()
                                            .extension()
                                            .and_then(|e| e.to_str())
                                            == Some("json")
                                    {
                                        let content = std::fs::read_to_string(sub.path())?;
                                        let session_id = path
                                            .file_name()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown")
                                            .to_string();
                                        let messages = parse_session_content(&content);
                                        if messages.is_empty() {
                                            continue;
                                        }
                                        let (e, r) =
                                            kg.extract_from_messages(&messages, &session_id)?;
                                        total_entities += e;
                                        total_rels += r;
                                        session_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "knowledge.build",
                        "sessions_processed": session_count,
                        "entities_added": total_entities,
                        "relationships_added": total_rels,
                    }))?
                );
            } else {
                println!("✅ Knowledge graph built from {session_count} sessions");
                println!("   Entities added: {total_entities}");
                println!("   Relationships added: {total_rels}");
            }
        }

        KnowledgeAction::Query {
            entity,
            depth,
            json,
        } => {
            let kg = KnowledgeGraph::new(loom_dir)?;

            // Try as exact ID first, then as name
            let entity_id = if let Some(e) = kg.get_entity(entity)? {
                e.id
            } else if let Some(e) = kg.find_entity(entity)? {
                e.id
            } else {
                anyhow::bail!("Entity '{}' not found in knowledge graph", entity);
            };

            let subgraph = kg.subgraph(&entity_id, *depth)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "knowledge.query",
                        "entity": {
                            "id": subgraph.entity.id,
                            "name": subgraph.entity.name,
                            "type": subgraph.entity.entity_type,
                        },
                        "related": subgraph.related.iter().map(|r| serde_json::json!({
                            "entity": {
                                "id": r.entity.id,
                                "name": r.entity.name,
                                "type": r.entity.entity_type,
                            },
                            "relationship": {
                                "predicate": r.relationship.predicate,
                                "weight": r.relationship.weight,
                            },
                            "depth": r.depth,
                        })).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!("📊 Knowledge: {}", subgraph.entity.name);
                println!("   Type: {}", subgraph.entity.entity_type);
                println!(
                    "   Connections: {}",
                    subgraph.related.len()
                );
                println!();
                for r in &subgraph.related {
                    let arrow = if r.relationship.subject_id == subgraph.entity.id {
                        format!("── {} ──>", r.relationship.predicate)
                    } else {
                        format!("<── {} ──", r.relationship.predicate)
                    };
                    println!(
                        "   {} {} {} (depth {})",
                        r.entity.name, arrow, r.entity.entity_type, r.depth
                    );
                }
            }
        }

        KnowledgeAction::Stats { json } => {
            let kg = KnowledgeGraph::new(loom_dir)?;
            let stats = kg.stats()?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "knowledge.stats",
                        "entity_count": stats.entity_count,
                        "relationship_count": stats.relationship_count,
                        "type_counts": stats.type_counts.iter().map(|(t, c)| serde_json::json!({"type": t, "count": c})).collect::<Vec<_>>(),
                        "top_predicates": stats.top_predicates.iter().map(|(p, c)| serde_json::json!({"predicate": p, "count": c})).collect::<Vec<_>>(),
                        "top_entities": stats.top_entities.iter().map(|(n, t, d)| serde_json::json!({"name": n, "type": t, "degree": d})).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                println!("📊 Knowledge Graph Statistics");
                println!("   Entities: {}", stats.entity_count);
                println!("   Relationships: {}", stats.relationship_count);
                println!();
                if !stats.type_counts.is_empty() {
                    println!("   By type:");
                    for (t, c) in &stats.type_counts {
                        println!("     {t}: {c}");
                    }
                }
                if !stats.top_predicates.is_empty() {
                    println!();
                    println!("   Top predicates:");
                    for (p, c) in &stats.top_predicates {
                        println!("     {p}: {c}");
                    }
                }
                if !stats.top_entities.is_empty() {
                    println!();
                    println!("   Most connected:");
                    for (n, t, d) in &stats.top_entities {
                        println!("     {n} ({t}): {d} connections");
                    }
                }
            }
        }

        KnowledgeAction::Add {
            name,
            entity_type,
            canonical,
        } => {
            let kg = KnowledgeGraph::new(loom_dir)?;

            // Check if already exists
            if let Some(existing) = kg.find_entity(name)? {
                println!("⚠ Entity '{}' already exists (id: {})", name, existing.id);
                return Ok(());
            }

            let id = kg.add_entity(name, entity_type, canonical.as_deref(), serde_json::json!({"manual": true}))?;
            println!("✅ Added entity '{}' (type: {}) with id: {}", name, entity_type, id);
        }

        KnowledgeAction::Relate {
            from,
            predicate,
            to,
            weight,
        } => {
            let kg = KnowledgeGraph::new(loom_dir)?;

            let from_id = resolve_entity(&kg, from)?;
            let to_id = resolve_entity(&kg, to)?;

            let rel_id = kg.add_relationship(&from_id, predicate, &to_id, *weight, Some("manual"))?;
            println!("✅ Added relationship: {} --{}--> {} (id: {})", from, predicate, to, rel_id);
        }

        KnowledgeAction::Search { query, limit, json } => {
            let kg = KnowledgeGraph::new(loom_dir)?;
            let results = kg.search_entities(query, *limit)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "knowledge.search",
                        "query": query,
                        "results": results.iter().map(|e| serde_json::json!({
                            "id": e.id,
                            "name": e.name,
                            "type": e.entity_type,
                        })).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                if results.is_empty() {
                    println!("No results found for '{query}'");
                } else {
                    println!("🔍 Search results for '{query}':");
                    for e in &results {
                        println!("   {} ({})", e.name, e.entity_type);
                    }
                }
            }
        }

        KnowledgeAction::Clear { force } => {
            if !*force {
                anyhow::bail!("Use --force to clear the entire knowledge graph");
            }
            let kg = KnowledgeGraph::new(loom_dir)?;
            kg.clear()?;
            println!("✅ Knowledge graph cleared");
        }
    }

    Ok(())
}

/// Resolve an entity by name (search + exact match) or return as an ID.
fn resolve_entity(kg: &KnowledgeGraph, name_or_id: &str) -> anyhow::Result<String> {
    // Try as ID first
    if let Some(e) = kg.get_entity(name_or_id)? {
        return Ok(e.id);
    }
    // Try as name
    if let Some(e) = kg.find_entity(name_or_id)? {
        return Ok(e.id);
    }
    anyhow::bail!("Entity '{}' not found. Use `loom knowledge add` first.", name_or_id)
}

/// Parse session content (JSONL or JSON array) into loom_core::Message slices.
fn parse_session_content(content: &str) -> Vec<loom_core::Message> {
    // Try JSONL first (one JSON object per line)
    if content.contains('\n') {
        let mut messages = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<loom_core::Message>(line) {
                messages.push(msg);
            }
        }
        if !messages.is_empty() {
            return messages;
        }
    }

    // Try JSON array
    if let Ok(msgs) = serde_json::from_str::<Vec<loom_core::Message>>(content) {
        return msgs;
    }

    Vec::new()
}

/// Build an LLM prompt for entity/relationship extraction from session content.
fn build_extraction_prompt(content: &str) -> String {
    format!(
        r#"You are a knowledge extraction system. Analyze the following conversation and extract:
1. Entities — important tools, concepts, languages, databases, providers, frameworks, architecture patterns
2. Relationships — meaningful connections between entities (e.g., "built_with", "uses", "implements", "related_to", "powered_by", "integrates_with", "depends_on")

Return ONLY valid JSON in this exact format (no markdown, no explanation):
{{
  "entities": [
    {{"name": "EntityName", "entity_type": "tool|concept|language|database|provider|framework|pattern", "description": "brief description of what this entity is"}}
  ],
  "relationships": [
    {{"subject": "EntityA", "predicate": "built_with", "object": "EntityB", "weight": 0.9}}
  ]
}}

Conversation content:
---
{}
---
"#,
        content
    )
}

/// Call an LLM provider for extraction.
/// Uses OPENAI_API_KEY (default) or ANTHROPIC_API_KEY.
/// Returns the raw response text.
async fn call_llm_for_extraction(prompt: &str) -> anyhow::Result<String> {
    // Try Anthropic first, fallback to OpenAI
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        return call_anthropic_extraction(&api_key, prompt).await;
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow::anyhow!("Set OPENAI_API_KEY or ANTHROPIC_API_KEY for LLM extraction"))?;
    call_openai_extraction(&api_key, prompt).await
}

async fn call_openai_extraction(api_key: &str, prompt: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.1,
        "max_tokens": 2000,
    });

    let resp: serde_json::Value = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in OpenAI response"))?
        .to_string();
    Ok(text)
}

async fn call_anthropic_extraction(api_key: &str, prompt: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 2000,
        "messages": [
            {"role": "user", "content": prompt}
        ],
    });

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in Anthropic response"))?
        .to_string();
    Ok(text)
}
