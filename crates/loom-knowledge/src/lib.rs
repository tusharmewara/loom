//! Knowledge Graph — structured entity/relationship extraction and querying.
//!
//! Stores entities and relationships in SQLite with recursive CTE traversal
//! for subgraph queries. Integrates with session indexing for automatic
//! knowledge extraction.

mod extraction;

use anyhow::Result;
use loom_core::{KnowledgeEntity, RelatedEntity, Relationship, SubgraphResult};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Knowledge graph backed by SQLite with recursive CTE traversal.
pub struct KnowledgeGraph {
    db_path: PathBuf,
}

impl KnowledgeGraph {
    /// Open (or create) the knowledge graph database.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let knowledge_dir = Path::new(loom_dir).join("knowledge");
        std::fs::create_dir_all(&knowledge_dir)?;
        let db_path = knowledge_dir.join("graph.db");

        let kg = Self { db_path };
        kg.initialize()?;
        Ok(kg)
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        Ok(conn)
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL DEFAULT 'concept',
                canonical_name TEXT,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
            CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);

            CREATE TABLE IF NOT EXISTS relationships (
                id TEXT PRIMARY KEY,
                subject_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
                predicate TEXT NOT NULL,
                object_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
                weight REAL NOT NULL DEFAULT 1.0,
                source_session TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rel_subject ON relationships(subject_id);
            CREATE INDEX IF NOT EXISTS idx_rel_object ON relationships(object_id);
            CREATE INDEX IF NOT EXISTS idx_rel_predicate ON relationships(predicate);

            CREATE VIRTUAL TABLE IF NOT EXISTS entity_fts USING fts5(
                name,
                entity_type,
                content='entities',
                content_rowid='rowid'
            );
            ",
        )?;
        Ok(())
    }

    // ─── Entity CRUD ────────────────────────────────────────────────

    /// Add an entity to the graph. Returns the entity ID.
    pub fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        canonical_name: Option<&str>,
        metadata: serde_json::Value,
    ) -> Result<String> {
        let conn = self.conn()?;
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let meta_str = metadata.to_string();

        conn.execute(
            "INSERT INTO entities (id, name, entity_type, canonical_name, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, name, entity_type, canonical_name, meta_str, now],
        )?;

        // Sync FTS
        conn.execute(
            "INSERT INTO entity_fts(rowid, name, entity_type) VALUES (?1, ?2, ?3)",
            params![conn.last_insert_rowid(), name, entity_type],
        )?;

        Ok(id)
    }

    /// Get an entity by ID.
    pub fn get_entity(&self, id: &str) -> Result<Option<KnowledgeEntity>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, entity_type, canonical_name, metadata, created_at
             FROM entities WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            let meta_str: String = row.get(4)?;
            Ok(KnowledgeEntity {
                id: row.get(0)?,
                name: row.get(1)?,
                entity_type: row.get(2)?,
                canonical_name: row.get(3)?,
                metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
                created_at: row.get(5)?,
            })
        })?;

        match rows.next() {
            Some(Ok(entity)) => Ok(Some(entity)),
            _ => Ok(None),
        }
    }

    /// Find an entity by name (case-insensitive exact match).
    pub fn find_entity(&self, name: &str) -> Result<Option<KnowledgeEntity>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, entity_type, canonical_name, metadata, created_at
             FROM entities WHERE LOWER(name) = LOWER(?1)
             LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![name], Self::map_entity)?;
        match rows.next() {
            Some(Ok(e)) => Ok(Some(e)),
            _ => Ok(None),
        }
    }

    /// Search entities by name/type (FTS5).
    pub fn search_entities(&self, query: &str, limit: usize) -> Result<Vec<KnowledgeEntity>> {
        let conn = self.conn()?;
        let sql = "
            SELECT e.id, e.name, e.entity_type, e.canonical_name, e.metadata, e.created_at
            FROM entity_fts fts
            JOIN entities e ON fts.rowid = e.rowid
            WHERE entity_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
        ";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![query, limit as i64], Self::map_entity)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Delete an entity and all its relationships.
    pub fn delete_entity(&self, id: &str) -> Result<bool> {
        let conn = self.conn()?;
        let deleted = conn.execute("DELETE FROM entities WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    // ─── Relationship CRUD ──────────────────────────────────────────

    /// Add a relationship between two entities. Auto-deduplicates by using
    /// INSERT OR IGNORE with the same subject+predicate+object.
    pub fn add_relationship(
        &self,
        subject_id: &str,
        predicate: &str,
        object_id: &str,
        weight: f64,
        source_session: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn()?;

        // Check if relationship already exists (avoid duplicates)
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM relationships
                 WHERE subject_id = ?1 AND predicate = ?2 AND object_id = ?3
                 LIMIT 1",
                params![subject_id, predicate, object_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // Update weight (running average)
            conn.execute(
                "UPDATE relationships SET weight = (weight + ?1) / 2.0 WHERE id = ?2",
                params![weight, id],
            )?;
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO relationships (id, subject_id, predicate, object_id, weight, source_session, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, subject_id, predicate, object_id, weight, source_session, now],
        )?;
        Ok(id)
    }

    /// Get all relationships for an entity (as subject or object).
    pub fn get_relationships(&self, entity_id: &str) -> Result<Vec<Relationship>> {
        let conn = self.conn()?;
        let sql = "
            SELECT r.id, r.subject_id, s.name, r.predicate,
                   r.object_id, o.name, r.weight, r.source_session, r.created_at
            FROM relationships r
            JOIN entities s ON r.subject_id = s.id
            JOIN entities o ON r.object_id = o.id
            WHERE r.subject_id = ?1 OR r.object_id = ?1
            ORDER BY r.weight DESC
        ";
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![entity_id], Self::map_relationship)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.into())
    }

    /// Delete a relationship.
    pub fn delete_relationship(&self, id: &str) -> Result<bool> {
        let conn = self.conn()?;
        let deleted = conn.execute("DELETE FROM relationships WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    // ─── Graph Query ────────────────────────────────────────────────

    /// Query the subgraph around an entity up to a given depth.
    /// Uses recursive CTE for graph traversal.
    pub fn subgraph(&self, entity_id: &str, max_depth: u32) -> Result<SubgraphResult> {
        let entity = self
            .get_entity(entity_id)?
            .ok_or_else(|| anyhow::anyhow!("Entity '{}' not found", entity_id))?;

        let conn = self.conn()?;
        let mut related = Vec::new();

        // Recursive CTE: traverse relationships in both directions.
        // For each visited neighbor, pick the best (highest-weight) relationship
        // back to any already-visited node.
        let sql = "
            WITH RECURSIVE reachable(id, depth) AS (
                SELECT ?1, 0
                UNION
                SELECT DISTINCT
                    CASE WHEN r.subject_id = rv.id THEN r.object_id ELSE r.subject_id END,
                    rv.depth + 1
                FROM relationships r
                JOIN reachable rv ON r.subject_id = rv.id OR r.object_id = rv.id
                WHERE rv.depth < ?2
                  AND CASE WHEN r.subject_id = rv.id THEN r.object_id ELSE r.subject_id END != ?1
            )
            SELECT DISTINCT e.id, e.name, e.entity_type, e.canonical_name, e.metadata, e.created_at,
                   r2.id AS rel_id, r2.subject_id, s.name AS subj_name, r2.predicate,
                   r2.object_id, o.name AS obj_name, r2.weight, r2.source_session, r2.created_at AS rel_created,
                   rv2.depth
            FROM reachable rv2
            JOIN entities e ON rv2.id = e.id
            JOIN relationships r2 ON (r2.subject_id = rv2.id OR r2.object_id = rv2.id)
            JOIN entities s ON r2.subject_id = s.id
            JOIN entities o ON r2.object_id = o.id
            WHERE rv2.depth > 0
            ORDER BY rv2.depth, r2.weight DESC
        ";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![entity_id, max_depth], |row| {
            let meta_str: String = row.get(4)?;
            Ok(RelatedEntity {
                entity: KnowledgeEntity {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    entity_type: row.get(2)?,
                    canonical_name: row.get(3)?,
                    metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
                    created_at: row.get(5)?,
                },
                relationship: Relationship {
                    id: row.get(6)?,
                    subject_id: row.get(7)?,
                    subject_name: row.get(8)?,
                    predicate: row.get(9)?,
                    object_id: row.get(10)?,
                    object_name: row.get(11)?,
                    weight: row.get(12)?,
                    source_session: row.get(13)?,
                    created_at: row.get(14)?,
                },
                depth: row.get(15)?,
            })
        })?;

        // Collect and deduplicate by entity ID (keep the entry with smallest depth)
        for row in rows {
            related.push(row?);
        }
        related.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| {
            b.relationship.weight.partial_cmp(&a.relationship.weight).unwrap_or(std::cmp::Ordering::Equal)
        }));
        related.dedup_by(|a, b| a.entity.id == b.entity.id);

        Ok(SubgraphResult { entity, related })
    }

    /// Simple direct related query: get all entities directly connected via
    /// relationships. More predictable than recursive CTE.
    pub fn related_entities(&self, entity_id: &str) -> Result<Vec<RelatedEntity>> {
        // Verify entity exists
        if self.get_entity(entity_id)?.is_none() {
            anyhow::bail!("Entity '{}' not found", entity_id);
        }

        let conn = self.conn()?;
        let mut related = Vec::new();

        let sql = "
            SELECT e.id, e.name, e.entity_type, e.canonical_name, e.metadata, e.created_at,
                   r.id AS rel_id, r.subject_id, s.name AS subj_name, r.predicate,
                   r.object_id, o.name AS obj_name, r.weight, r.source_session, r.created_at AS rel_created
            FROM relationships r
            JOIN entities e ON (CASE WHEN r.subject_id = ?1 THEN r.object_id ELSE r.subject_id END) = e.id
            JOIN entities s ON r.subject_id = s.id
            JOIN entities o ON r.object_id = o.id
            WHERE r.subject_id = ?1 OR r.object_id = ?1
            ORDER BY r.weight DESC
        ";

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![entity_id], |row| {
            let meta_str: String = row.get(4)?;
            Ok(RelatedEntity {
                entity: KnowledgeEntity {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    entity_type: row.get(2)?,
                    canonical_name: row.get(3)?,
                    metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
                    created_at: row.get(5)?,
                },
                relationship: Relationship {
                    id: row.get(6)?,
                    subject_id: row.get(7)?,
                    subject_name: row.get(8)?,
                    predicate: row.get(9)?,
                    object_id: row.get(10)?,
                    object_name: row.get(11)?,
                    weight: row.get(12)?,
                    source_session: row.get(13)?,
                    created_at: row.get(14)?,
                },
                depth: 1,
            })
        })?;

        for row in rows {
            related.push(row?);
        }

        // Deduplicate by entity ID (keep highest weight relationship)
        related.sort_by(|a, b| {
            b.relationship
                .weight
                .partial_cmp(&a.relationship.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        related.dedup_by(|a, b| a.entity.id == b.entity.id);

        Ok(related)
    }

    /// Get graph statistics.
    pub fn stats(&self) -> Result<GraphStats> {
        let conn = self.conn()?;

        let entity_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?;
        let rel_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM relationships", [], |row| row.get(0))?;

        // Top entity types
        let mut type_counts = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT entity_type, COUNT(*) as cnt FROM entities
                 GROUP BY entity_type ORDER BY cnt DESC LIMIT 10",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?;
            for row in rows {
                type_counts.push(row?);
            }
        }

        // Top predicates
        let mut top_predicates = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT predicate, COUNT(*) as cnt FROM relationships
                 GROUP BY predicate ORDER BY cnt DESC LIMIT 10",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?;
            for row in rows {
                top_predicates.push(row?);
            }
        }

        // Most connected entities
        let mut top_entities = Vec::new();
        {
            let mut stmt = conn.prepare(
                "SELECT e.name, e.entity_type,
                        (SELECT COUNT(*) FROM relationships WHERE subject_id = e.id OR object_id = e.id) AS degree
                 FROM entities e
                 ORDER BY degree DESC
                 LIMIT 10",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })?;
            for row in rows {
                top_entities.push(row?);
            }
        }

        Ok(GraphStats {
            entity_count,
            relationship_count: rel_count,
            type_counts,
            top_predicates,
            top_entities,
        })
    }

    /// Delete all entities and relationships.
    pub fn clear(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch("DELETE FROM relationships; DELETE FROM entities;")?;
        Ok(())
    }

    // ─── Extraction Integration ─────────────────────────────────────

    /// Extract entities and relationships from a batch of messages.
    /// Returns the number of new entities and relationships added.
    pub fn extract_from_messages(
        &self,
        messages: &[loom_core::Message],
        source_session: &str,
    ) -> Result<(usize, usize)> {
        extraction::extract_from_messages(self, messages, source_session)
    }

    // ─── Helpers ────────────────────────────────────────────────────

    fn map_entity(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeEntity> {
        let meta_str: String = row.get(4)?;
        Ok(KnowledgeEntity {
            id: row.get(0)?,
            name: row.get(1)?,
            entity_type: row.get(2)?,
            canonical_name: row.get(3)?,
            metadata: serde_json::from_str(&meta_str).unwrap_or_default(),
            created_at: row.get(5)?,
        })
    }

    fn map_relationship(row: &rusqlite::Row) -> rusqlite::Result<Relationship> {
        Ok(Relationship {
            id: row.get(0)?,
            subject_id: row.get(1)?,
            subject_name: row.get(2)?,
            predicate: row.get(3)?,
            object_id: row.get(4)?,
            object_name: row.get(5)?,
            weight: row.get(6)?,
            source_session: row.get(7)?,
            created_at: row.get(8)?,
        })
    }
}

/// Knowledge graph statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GraphStats {
    pub entity_count: u64,
    pub relationship_count: u64,
    pub type_counts: Vec<(String, u64)>,
    pub top_predicates: Vec<(String, u64)>,
    pub top_entities: Vec<(String, String, u64)>,
}

use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Message, MessageRole};
    use tempfile::tempdir;

    fn make_msg(id: &str, role: MessageRole, content: &str) -> Message {
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
    fn test_add_and_get_entity() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let id = kg
            .add_entity("Loom", "tool", None, serde_json::json!({"version": "2.1"}))
            .unwrap();

        let entity = kg.get_entity(&id).unwrap().unwrap();
        assert_eq!(entity.name, "Loom");
        assert_eq!(entity.entity_type, "tool");
        assert_eq!(entity.metadata["version"], "2.1");
    }

    #[test]
    fn test_find_entity_case_insensitive() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        kg.add_entity("Rust", "language", None, serde_json::json!({}))
            .unwrap();
        let entity = kg.find_entity("rust").unwrap().unwrap();
        assert_eq!(entity.name, "Rust");
    }

    #[test]
    fn test_add_relationship_and_subgraph() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let loom_id = kg.add_entity("Loom", "tool", None, serde_json::json!({})).unwrap();
        let rust_id = kg.add_entity("Rust", "language", None, serde_json::json!({})).unwrap();
        let sqlite_id = kg.add_entity("SQLite", "database", None, serde_json::json!({})).unwrap();

        kg.add_relationship(&loom_id, "built_with", &rust_id, 1.0, None)
            .unwrap();
        kg.add_relationship(&loom_id, "uses", &sqlite_id, 1.0, Some("session-1"))
            .unwrap();

        let subgraph = kg.subgraph(&loom_id, 1).unwrap();
        assert_eq!(subgraph.entity.name, "Loom");
        assert_eq!(subgraph.related.len(), 2);
    }

    #[test]
    fn test_search_entities() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        kg.add_entity("PostgreSQL", "database", None, serde_json::json!({}))
            .unwrap();
        kg.add_entity("Python", "language", None, serde_json::json!({}))
            .unwrap();

        // FTS5 needs * for prefix matching; exact token works too
        let results = kg.search_entities("PostgreSQL", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "PostgreSQL");

        // Prefix search
        let results = kg.search_entities("Post*", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_stats() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let a = kg.add_entity("A", "test", None, serde_json::json!({})).unwrap();
        let b = kg.add_entity("B", "test", None, serde_json::json!({})).unwrap();
        kg.add_relationship(&a, "connected", &b, 1.0, None).unwrap();

        let stats = kg.stats().unwrap();
        assert_eq!(stats.entity_count, 2);
        assert_eq!(stats.relationship_count, 1);
    }

    #[test]
    fn test_extract_from_messages() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let msgs = vec![
            make_msg(
                "m1",
                MessageRole::User,
                "How do I configure MCP servers in Loom?",
            ),
            make_msg(
                "m2",
                MessageRole::Assistant,
                "You can use the loom mcp add command to register servers.",
            ),
        ];

        // Check it runs without error (some entities may be extracted)
        let result = kg.extract_from_messages(&msgs, "session-test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_subgraph_depth() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let a = kg.add_entity("A", "test", None, serde_json::json!({})).unwrap();
        let b = kg.add_entity("B", "test", None, serde_json::json!({})).unwrap();
        let c = kg.add_entity("C", "test", None, serde_json::json!({})).unwrap();

        kg.add_relationship(&a, "knows", &b, 1.0, None).unwrap();
        kg.add_relationship(&b, "knows", &c, 1.0, None).unwrap();

        // Depth 1: only B
        let sg1 = kg.subgraph(&a, 1).unwrap();
        let names: Vec<&str> = sg1.related.iter().map(|r| r.entity.name.as_str()).collect();
        assert!(names.contains(&"B"), "B should be in depth-1 subgraph");
        assert_eq!(sg1.related.len(), 1, "depth 1 should have exactly 1 entity");

        // Depth 2: B and C
        let sg2 = kg.subgraph(&a, 2).unwrap();
        let names2: Vec<&str> = sg2.related.iter().map(|r| r.entity.name.as_str()).collect();
        assert!(names2.contains(&"B"));
        assert!(names2.contains(&"C"));
    }

    #[test]
    fn test_delete_entity_cascades() {
        let dir = tempdir().unwrap();
        let kg = KnowledgeGraph::new(dir.path().to_str().unwrap()).unwrap();

        let a = kg.add_entity("A", "test", None, serde_json::json!({})).unwrap();
        let b = kg.add_entity("B", "test", None, serde_json::json!({})).unwrap();
        kg.add_relationship(&a, "knows", &b, 1.0, None).unwrap();

        assert!(kg.delete_entity(&a).unwrap());

        // B still exists, but cascade should have deleted the relationship
        let subgraph = kg.subgraph(&b, 1).unwrap();
        assert!(subgraph.related.is_empty());
    }
}
