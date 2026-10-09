//! Hindsight Memory — cross-session typed fact storage with recall and reflection.
//!
//! Hindsight stores **typed facts** that survive across sessions, unlike
//! session compression which only preserves message history.
//!
//! ## Operations
//!
//! - **Retain** — store a durable fact with type metadata
//! - **Recall** — search the fact bank by text similarity (FTS5)
//! - **Reflect** — synthesize an overview from matching facts
//!
//! ## Fact Types
//!
//! Facts carry a type tag that groups related knowledge:
//! - `insight` — general observations
//! - `decision` — architectural/design decisions
//! - `pattern` — reusable code patterns
//! - `error` — errors and solutions
//! - `preference` — user preferences

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

// ─── Types ───────────────────────────────────────────────────────────

/// A typed fact stored in the hindsight memory bank.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Unique fact identifier.
    pub id: String,
    /// The factual content.
    pub content: String,
    /// Type tag (insight, decision, pattern, error, preference, or custom).
    pub fact_type: String,
    /// Source identifier (e.g. session ID, tool name).
    pub source: String,
    /// Importance score 0.0–1.0.
    pub importance: f64,
    /// When this fact was created.
    pub created_at: DateTime<Utc>,
}

/// A query result from the fact bank.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactMatch {
    /// The matching fact.
    pub fact: Fact,
    /// BM25 / FTS relevance score (higher = more relevant).
    pub score: f64,
}

/// The retrospective memory bank — persists typed facts across sessions.
pub struct Hindsight {
    db: Mutex<Connection>,
}

impl Hindsight {
    /// Open (or create) the hindsight store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open hindsight DB at {}", path.display()))?;

        // Enable WAL for concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Create tables if they don't exist
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id          TEXT PRIMARY KEY,
                content     TEXT NOT NULL,
                fact_type   TEXT NOT NULL DEFAULT 'insight',
                source      TEXT NOT NULL DEFAULT '',
                importance  REAL NOT NULL DEFAULT 0.5,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_facts_type ON facts(fact_type);
            CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source);
            CREATE INDEX IF NOT EXISTS idx_facts_importance ON facts(importance);",
        )?;

        // Create FTS5 virtual table for full-text search
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
                content,
                fact_type,
                content='facts',
                content_rowid='rowid',
                tokenize='porter unicode61'
            );",
        )?;

        // Populate FTS index if empty (e.g. after schema creation)
        conn.execute_batch(
            "INSERT OR IGNORE INTO facts_fts(rowid, content, fact_type)
             SELECT rowid, content, fact_type FROM facts;",
        )?;

        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    /// Open the in-memory store (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id          TEXT PRIMARY KEY,
                content     TEXT NOT NULL,
                fact_type   TEXT NOT NULL DEFAULT 'insight',
                source      TEXT NOT NULL DEFAULT '',
                importance  REAL NOT NULL DEFAULT 0.5,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_facts_type ON facts(fact_type);
            CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source);
            CREATE INDEX IF NOT EXISTS idx_facts_importance ON facts(importance);
            CREATE VIRTUAL TABLE IF NOT EXISTS facts_fts USING fts5(
                content,
                fact_type,
                content='facts',
                content_rowid='rowid',
                tokenize='porter unicode61'
            );",
        )?;
        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    /// Retain a fact — stores it durably.
    ///
    /// Returns the generated fact ID.
    pub fn retain(
        &self,
        content: &str,
        fact_type: &str,
        source: &str,
        importance: f64,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let importance = importance.clamp(0.0, 1.0);

        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO facts (id, content, fact_type, source, importance, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, content, fact_type, source, importance, now],
        )?;

        // Sync to FTS index
        db.execute(
            "INSERT INTO facts_fts(rowid, content, fact_type)
             VALUES ((SELECT rowid FROM facts WHERE id = ?1), ?2, ?3)",
            params![id, content, fact_type],
        )?;

        tracing::debug!(%id, %fact_type, %importance, "hindsight: retained fact");
        Ok(id)
    }

    /// Recall facts matching the query — returns up to `limit` results
    /// ordered by FTS relevance (BM25).
    pub fn recall(&self, query: &str, limit: usize) -> Result<Vec<FactMatch>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let limit = limit.min(100) as i64;
        let db = self.db.lock().unwrap();

        // Build a FTS query: make multi-word queries match all terms
        let fts_query = query
            .split_whitespace()
            .filter(|t| t.len() >= 2)
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let sql = "SELECT f.id, f.content, f.fact_type, f.source, f.importance, f.created_at,
                    bm25(facts_fts, 0.0, 10.0, 5.0) AS score
             FROM facts_fts
             JOIN facts f ON f.rowid = facts_fts.rowid
             WHERE facts_fts MATCH ?1
             ORDER BY score
             LIMIT ?2".to_string();

        let mut stmt = db.prepare(&sql)?;
        let results = stmt
            .query_map(params![fts_query, limit], |row| {
                Ok(FactMatch {
                    fact: Fact {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        fact_type: row.get(2)?,
                        source: row.get(3)?,
                        importance: row.get(4)?,
                        created_at: row
                            .get::<_, String>(5)?
                            .parse::<DateTime<Utc>>()
                            .unwrap_or_else(|_| Utc::now()),
                    },
                    score: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Reflect on a topic — returns a synthesized text summary from
    /// matching facts. This uses template-based synthesis (no LLM call).
    pub fn reflect(&self, question: &str) -> Result<String> {
        let facts = self.recall(question, 10)?;

        if facts.is_empty() {
            return Ok("No relevant facts found in hindsight memory.".to_string());
        }

        let mut types: Vec<String> = Vec::new();
        let mut insights = Vec::new();
        let mut decisions = Vec::new();
        let mut patterns = Vec::new();
        let mut errors = Vec::new();
        let mut other: Vec<(&str, &str)> = Vec::new();

        for fm in &facts {
            if !types.contains(&fm.fact.fact_type) {
                types.push(fm.fact.fact_type.clone());
            }
            match fm.fact.fact_type.as_str() {
                "insight" => insights.push(&fm.fact.content),
                "decision" => decisions.push(&fm.fact.content),
                "pattern" => patterns.push(&fm.fact.content),
                "error" => errors.push(&fm.fact.content),
                t => other.push((t, &fm.fact.content)),
            }
        }

        let mut parts: Vec<String> = Vec::new();
        parts.push(format!(
            "Reflection on \"{question}\": found {} relevant fact(s).",
            facts.len()
        ));
        parts.push(format!("Fact types: {}", types.join(", ")));

        if !insights.is_empty() {
            parts.push("Insights:".to_string());
            for i in insights {
                parts.push(format!("  • {i}"));
            }
        }
        if !decisions.is_empty() {
            parts.push("Decisions:".to_string());
            for d in decisions {
                parts.push(format!("  • {d}"));
            }
        }
        if !patterns.is_empty() {
            parts.push("Patterns:".to_string());
            for p in patterns {
                parts.push(format!("  • {p}"));
            }
        }
        if !errors.is_empty() {
            parts.push("Errors & Solutions:".to_string());
            for e in errors {
                parts.push(format!("  • {e}"));
            }
        }
        for (typ, content) in other {
            parts.push(format!("[{typ}]: {content}"));
        }

        Ok(parts.join("\n"))
    }

    /// Delete a fact by ID.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let db = self.db.lock().unwrap();
        let affected = db.execute(
            "DELETE FROM facts WHERE id = ?1",
            params![id],
        )?;
        // Delete from FTS as well (the sync trigger handles it, but also do explicitly)
        db.execute(
            "DELETE FROM facts_fts WHERE rowid = (SELECT rowid FROM facts WHERE id = ?1)",
            params![id],
        )?;
        Ok(affected > 0)
    }

    /// Count total facts in the store.
    pub fn count(&self) -> Result<u64> {
        let db = self.db.lock().unwrap();
        let count: u64 = db.query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))?;
        Ok(count)
    }

    /// List facts filtered by type, with optional limit.
    pub fn list_by_type(&self, fact_type: &str, limit: usize) -> Result<Vec<Fact>> {
        let limit = limit.min(100) as i64;
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, content, fact_type, source, importance, created_at
             FROM facts
             WHERE fact_type = ?1
             ORDER BY importance DESC, created_at DESC
             LIMIT ?2",
        )?;
        let results = stmt
            .query_map(params![fact_type, limit], |row| {
                Ok(Fact {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    fact_type: row.get(2)?,
                    source: row.get(3)?,
                    importance: row.get(4)?,
                    created_at: row
                        .get::<_, String>(5)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(results)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retain_and_recall() {
        let h = Hindsight::open_in_memory().unwrap();

        let id = h
            .retain("The API uses OAuth2 for authentication", "decision", "test", 0.8)
            .unwrap();
        assert!(!id.is_empty());

        let results = h.recall("authentication", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fact.id, id);
        assert!(results[0].score >= 0.0);
    }

    #[test]
    fn test_recall_empty_query() {
        let h = Hindsight::open_in_memory().unwrap();
        h.retain("test fact", "insight", "test", 0.5).unwrap();
        let results = h.recall("", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_recall_no_match() {
        let h = Hindsight::open_in_memory().unwrap();
        h.retain("the sky is blue", "insight", "test", 0.5).unwrap();
        let results = h.recall("quantum physics", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_importance_clamping() {
        let h = Hindsight::open_in_memory().unwrap();
        let _id = h.retain("important fact", "insight", "test", 1.5).unwrap();
        let results = h.recall("important", 10).unwrap();
        assert!((results[0].fact.importance - 1.0).abs() < 0.01);

        let _id2 = h.retain("unimportant", "insight", "test", -0.5).unwrap();
        let results = h.recall("unimportant", 10).unwrap();
        assert!((results[0].fact.importance - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_reflect_with_mixed_types() {
        let h = Hindsight::open_in_memory().unwrap();
        h.retain("Use Rust for performance-critical paths", "decision", "arch", 0.9)
            .unwrap();
        h.retain("Connection pooling improves throughput by 3x", "insight", "bench", 0.7)
            .unwrap();
        h.retain("Factory pattern for creating protocol handlers", "pattern", "dev", 0.6)
            .unwrap();

        let reflection = h.reflect("Rust performance").unwrap();
        assert!(reflection.contains("found 1 relevant fact(s)"));
        assert!(reflection.contains("Decisions"));
        assert!(!reflection.contains("pattern"));
    }

    #[test]
    fn test_reflect_no_results() {
        let h = Hindsight::open_in_memory().unwrap();
        let reflection = h.reflect("nonsense").unwrap();
        assert!(reflection.contains("No relevant facts found"));
    }

    #[test]
    fn test_delete() {
        let h = Hindsight::open_in_memory().unwrap();
        let id = h.retain("delete me", "insight", "test", 0.5).unwrap();
        assert!(h.delete(&id).unwrap());
        assert!(!h.delete(&id).unwrap());
        assert_eq!(h.recall("delete", 10).unwrap().len(), 0);
    }

    #[test]
    fn test_count() {
        let h = Hindsight::open_in_memory().unwrap();
        assert_eq!(h.count().unwrap(), 0);
        h.retain("fact 1", "insight", "test", 0.5).unwrap();
        h.retain("fact 2", "decision", "test", 0.5).unwrap();
        assert_eq!(h.count().unwrap(), 2);
    }

    #[test]
    fn test_list_by_type() {
        let h = Hindsight::open_in_memory().unwrap();
        h.retain("insight fact", "insight", "test", 0.7).unwrap();
        h.retain("decision fact", "decision", "test", 0.9).unwrap();

        let decisions = h.list_by_type("decision", 10).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].fact_type, "decision");

        let insights = h.list_by_type("insight", 10).unwrap();
        assert_eq!(insights.len(), 1);
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hindsight.db");

        {
            let h = Hindsight::open(&path).unwrap();
            h.retain("persistent fact", "insight", "test", 0.5).unwrap();
            assert_eq!(h.count().unwrap(), 1);
        }

        // Re-open
        {
            let h = Hindsight::open(&path).unwrap();
            assert_eq!(h.count().unwrap(), 1);
            let results = h.recall("persistent", 10).unwrap();
            assert_eq!(results.len(), 1);
        }
    }

    #[test]
    fn test_fts_query_with_short_words_is_ignored() {
        // Single-character tokens should be filtered out
        let h = Hindsight::open_in_memory().unwrap();
        h.retain("longer meaningful content", "insight", "test", 0.5)
            .unwrap();
        let results = h.recall("a", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_retain_with_custom_type() {
        let h = Hindsight::open_in_memory().unwrap();
        let _id = h.retain("custom typed fact", "my-custom-type", "my-tool", 0.5).unwrap();
        let results = h.list_by_type("my-custom-type", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "my-tool");
    }
}
