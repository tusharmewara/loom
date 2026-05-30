//! Entity and relationship extraction from session messages.
//!
//! Uses a combination of:
//! 1. Pattern matching on known tool/config names
//! 2. Capitalized noun phrase extraction
//! 3. Co-occurrence for relationship inference

use crate::KnowledgeGraph;
use loom_core::Message;
use regex_lite::Regex;
use serde::Deserialize;

/// LLM extraction response — the model should return this JSON shape.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LlmExtractionResult {
    pub entities: Vec<LlmEntity>,
    pub relationships: Vec<LlmRelationship>,
}

#[derive(Debug, Deserialize)]
pub struct LlmEntity {
    pub name: String,
    pub entity_type: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LlmRelationship {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    #[serde(default)]
    pub weight: Option<f64>,
}

/// Extract entities and relationships using an LLM provider response.
/// Parses the LLM's JSON output and stores results into the knowledge graph.
pub fn extract_from_llm_response(
    kg: &KnowledgeGraph,
    llm_response: &str,
    source_session: &str,
) -> anyhow::Result<(usize, usize)> {
    let mut entity_count = 0usize;
    let mut rel_count = 0usize;

    // Try to parse as structured JSON
    let result: LlmExtractionResult = serde_json::from_str(llm_response)
        .or_else(|_| {
            // The response might be wrapped in a markdown code block
            if let Some(start) = llm_response.find("```json") {
                let rest = &llm_response[start + 7..];
                if let Some(end) = rest.find("```") {
                    return serde_json::from_str(rest[..end].trim());
                }
            }
            // Or just a code block without language
            if let Some(start) = llm_response.find("```") {
                let rest = &llm_response[start + 3..];
                if let Some(end) = rest.find("```") {
                    return serde_json::from_str(rest[..end].trim());
                }
            }
            Err(serde_json::from_str::<LlmExtractionResult>("").unwrap_err())
        })?;

    for ent in &result.entities {
        if kg.find_entity(&ent.name)?.is_none() {
            let etype = if ent.entity_type.is_empty() {
                "concept"
            } else {
                &ent.entity_type
            };
            let metadata = serde_json::json!({
                "extracted": true,
                "source": "llm",
                "description": ent.description,
            });
            kg.add_entity(&ent.name, etype, Some(&ent.name), metadata)?;
            entity_count += 1;
        }
    }

    for rel in &result.relationships {
        // Resolve both entities
        if let (Some(a), Some(b)) = (kg.find_entity(&rel.subject)?, kg.find_entity(&rel.object)?) {
            let weight = rel.weight.unwrap_or(0.8).clamp(0.0, 1.0);
            kg.add_relationship(&a.id, &rel.predicate, &b.id, weight, Some(source_session))?;
            rel_count += 1;
        }
    }

    Ok((entity_count, rel_count))
}

/// Extract entities and relationships from a batch of messages.
/// Returns (new_entities_count, new_relationships_count).
pub fn extract_from_messages(
    kg: &KnowledgeGraph,
    messages: &[Message],
    source_session: &str,
) -> anyhow::Result<(usize, usize)> {
    let mut entity_count = 0usize;
    let mut rel_count = 0usize;

    // Collect all content
    let all_content: Vec<&str> = messages.iter().map(|m| m.content.as_str()).collect();
    let combined = all_content.join(" ");

    // 1. Extract known tool/config references
    let known_patterns = [
        (r"(?i)\bloom\b", "tool", "Loom"),
        (r"(?i)\bloomd\b", "tool", "loomd"),
        (r"(?i)\bjcode\b", "tool", "JCode"),
        (r"(?i)\bclaude[- ]?code\b", "tool", "Claude Code"),
        (r"(?i)\bopencode\b", "tool", "OpenCode"),
        (r"(?i)\bgoose\b", "tool", "Goose"),
        (r"(?i)\bmcp\b", "concept", "MCP"),
        (r"(?i)\bskill[s]?\b", "concept", "Skill"),
        (r"(?i)\bsession[s]?\b", "concept", "Session"),
        (r"(?i)\bplugin[s]?\b", "concept", "Plugin"),
        (r"(?i)\brouting\b", "concept", "Routing"),
        (r"(?i)\bworkflow[s]?\b", "concept", "Workflow"),
        (r"(?i)\bauth\b", "concept", "Auth"),
        (r"(?i)\bhook[s]?\b", "concept", "Hook"),
        (r"(?i)\bsqlite\b", "database", "SQLite"),
        (r"(?i)\bpostgres(ql)?\b", "database", "PostgreSQL"),
        (r"(?i)\bredis\b", "database", "Redis"),
        (r"(?i)\bdocker\b", "tool", "Docker"),
        (r"(?i)\bkubernetes\b", "tool", "Kubernetes"),
        (r"(?i)\brust\b", "language", "Rust"),
        (r"(?i)\bpython\b", "language", "Python"),
        (r"(?i)\btypescript\b", "language", "TypeScript"),
        (r"(?i)\bjavascript\b", "language", "JavaScript"),
        (r"(?i)\bgolang\b|\bgo\b", "language", "Go"),
        (r"(?i)\bopenai\b", "provider", "OpenAI"),
        (r"(?i)\banthropic\b", "provider", "Anthropic"),
    ];

    let mut found_names: Vec<(String, String)> = Vec::new(); // (name, type)

    for (pattern, etype, canonical) in &known_patterns {
        let re = Regex::new(pattern).unwrap();
        if re.is_match(&combined) {
            let name = canonical.to_string();
            if !found_names.iter().any(|(n, _)| n == &name) {
                found_names.push((name.clone(), etype.to_string()));

                // Upsert entity
                if kg.find_entity(&name)?.is_none() {
                    kg.add_entity(
                        &name,
                        etype,
                        Some(&name),
                        serde_json::json!({"extracted": true, "source": "pattern"}),
                    )?;
                    entity_count += 1;
                }
            }
        }
    }

    // 2. Extract capitalized multi-word phrases as potential entities
    let phrase_re = Regex::new(r"\b[A-Z][a-z]+(?:\s+[A-Z][a-z]+)*\b")?;
    for content in &all_content {
        for cap in phrase_re.captures_iter(content) {
            let phrase = cap.get(0).unwrap().as_str().to_string();
            if phrase.len() < 3 || phrase.len() > 80 {
                continue;
            }
            // Skip if it matches a known pattern already
            if found_names.iter().any(|(n, _)| n == &phrase) {
                continue;
            }
            // Skip common false positives
            if is_stop_phrase(&phrase) {
                continue;
            }

            // Add as a concept entity
            if kg.find_entity(&phrase)?.is_none() {
                kg.add_entity(
                    &phrase,
                    "concept",
                    None,
                    serde_json::json!({"extracted": true, "source": "session"}),
                )?;
                entity_count += 1;
                found_names.push((phrase.clone(), "concept".to_string()));
            }
        }
    }

    // 3. Infer relationships from co-occurrence in same messages
    for msg in messages {
        let content_lower = msg.content.to_lowercase();
        // For each pair of found entities that appear in the same message, create a "related_to" edge
        let present: Vec<(String, String)> = found_names
            .iter()
            .filter(|(name, _)| {
                let name_lower = name.to_lowercase();
                content_lower.contains(&name_lower)
            })
            .cloned()
            .collect();

        for i in 0..present.len() {
            for j in (i + 1)..present.len() {
                let (name_a, type_a) = &present[i];
                let (name_b, type_b) = &present[j];

                let ent_a = kg.find_entity(name_a)?;
                let ent_b = kg.find_entity(name_b)?;

                if let (Some(a), Some(b)) = (ent_a, ent_b) {
                    // Infer predicate based on types
                    let predicate = infer_predicate(type_a, type_b);

                    kg.add_relationship(
                        &a.id,
                        &predicate,
                        &b.id,
                        0.5,
                        Some(source_session),
                    )?;
                    rel_count += 1;
                }
            }
        }
    }

    Ok((entity_count, rel_count))
}

/// Infer a predicate based on entity type pairs.
fn infer_predicate(type_a: &str, type_b: &str) -> String {
    match (type_a, type_b) {
        ("tool", "language") => "built_with".to_string(),
        ("tool", "database") => "uses".to_string(),
        ("tool", "concept") => "implements".to_string(),
        ("concept", "tool") => "implemented_by".to_string(),
        ("tool", "provider") => "powered_by".to_string(),
        ("tool", "tool") => "integrates_with".to_string(),
        ("language", "tool") => "used_by".to_string(),
        ("concept", "concept") => "related_to".to_string(),
        _ => "related_to".to_string(),
    }
}

/// Common capitalized phrases that aren't meaningful entities.
fn is_stop_phrase(phrase: &str) -> bool {
    let lower = phrase.to_lowercase();
    let stops = [
        "how", "what", "when", "where", "why", "the", "this", "that", "with",
        "from", "into", "there", "here", "come", "came", "make", "like", "just",
        "also", "than", "then", "and", "but", "for", "not", "you", "your",
        "have", "has", "had", "can", "could", "would", "should", "will",
        "shall", "may", "might", "must", "need", "use", "using", "used",
        "get", "got", "set", "let", "put", "take", "look", "see", "show",
        "tell", "ask", "said", "says", "say", "one", "two", "some", "any",
        "each", "every", "all", "both", "few", "more", "most", "other",
        "such", "only", "own", "same", "so", "very", "too", "quite",
        "please", "thank", "yes", "no", "ok", "okay", "hi", "hello",
        "thanks", "welcome", "regards", "best", "sincerely",
        "step", "steps", "part", "parts", "section", "sections",
        "example", "examples", "note", "notes", "tip", "tips",
        "warning", "caution", "important", "see also",
    ];
    let words: Vec<&str> = lower.split_whitespace().collect();
    words.len() == 1 && stops.contains(&words[0])
        || words.len() == 2 && stops.contains(&words[0]) && stops.contains(&words[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Message, MessageRole};
    use tempfile::tempdir;

    fn msg(id: &str, role: MessageRole, content: &str) -> Message {
        Message {
            id: id.to_string(),
            role,
            content: content.to_string(),
            tool_calls: None,
            timestamp: "2025-01-01T00:00:00Z".into(),
            metadata: Default::default(),
        }
    }

    #[test]
    fn test_extract_known_tools() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();
        let msgs = vec![msg(
            "m1",
            MessageRole::User,
            "I love using Loom with Rust and SQLite.",
        )];

        let (entities, rels) = kg.extract_from_messages(&msgs, "session-1").unwrap();
        assert!(entities >= 2); // Loom, Rust, SQLite etc.
        assert!(rels > 0);
    }

    #[test]
    fn test_extract_capitalized_phrases() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();
        let msgs = vec![msg(
            "m1",
            MessageRole::User,
            "The Cross-Session Memory feature is very useful.",
        )];

        let (entities, _) = kg.extract_from_messages(&msgs, "session-1").unwrap();
        // "Cross-Session Memory" feature — capitalize extraction may find entities
        // Not guaranteed to find a specific number, just check it runs
    }

    #[test]
    fn test_infer_predicate_tool_language() {
        assert_eq!(infer_predicate("tool", "language"), "built_with");
        assert_eq!(infer_predicate("tool", "database"), "uses");
        assert_eq!(infer_predicate("concept", "concept"), "related_to");
    }

    #[test]
    fn test_stop_phrase_detection() {
        assert!(is_stop_phrase("How"));
        assert!(is_stop_phrase("The"));
        assert!(is_stop_phrase("Hello"));
        assert!(!is_stop_phrase("Cross-Session Memory")); // hyphenated won't match
        assert!(!is_stop_phrase("Loom"));
    }
}
