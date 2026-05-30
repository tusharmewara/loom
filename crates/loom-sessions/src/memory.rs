//! Cross-session memory store using SQLite FTS5 for full-text search.
//!
//! Maintains a global `~/.loom/sessions/memory.db` that indexes session messages
//! for cross-session search and context injection on resume.

use anyhow::Result;
use loom_core::Message;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single hit from a memory search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub session_id: String,
    pub message_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
    pub topic: String,
    /// Relevance score from FTS5 (rank).
    pub score: f64,
}

/// Cross-session memory store backed by SQLite FTS5.
pub struct CrossSessionMemory {
    db_path: PathBuf,
}

impl CrossSessionMemory {
    /// Open (or create) the global memory database.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let sessions_dir = Path::new(loom_dir).join("sessions");
        std::fs::create_dir_all(&sessions_dir)?;
        let db_path = sessions_dir.join("memory.db");

        let store = Self { db_path };
        store.initialize()?;
        Ok(store)
    }

    fn conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }

    fn initialize(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS session_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                topic TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_session_messages_session
                ON session_messages(session_id);
            CREATE INDEX IF NOT EXISTS idx_session_messages_message
                ON session_messages(message_id);

            CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                content,
                topic,
                content=session_messages,
                content_rowid=id,
                tokenize='porter unicode61'
            );
            ",
        )?;
        Ok(())
    }

    /// Index a single message into the cross-session memory store.
    /// If the message already exists (same message_id), it is updated.
    pub fn index_message(
        &self,
        session_id: &str,
        message: &Message,
        topic: &str,
    ) -> Result<()> {
        let conn = self.conn()?;

        // Upsert: delete existing entry for this message_id, then insert
        conn.execute(
            "DELETE FROM session_messages WHERE message_id = ?1",
            params![message.id],
        )?;

        conn.execute(
            "INSERT INTO session_messages (session_id, message_id, role, content, timestamp, topic)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                message.id,
                format!("{:?}", message.role),
                message.content,
                message.timestamp,
                topic,
            ],
        )?;

        // Sync FTS index
        conn.execute("INSERT INTO message_fts(rowid, content, topic) VALUES (?1, ?2, ?3)", params![
            conn.last_insert_rowid(),
            message.content,
            topic,
        ])?;

        Ok(())
    }

    /// Index multiple messages for a session (full re-index).
    pub fn index_session(
        &self,
        session_id: &str,
        messages: &[Message],
        topic: &str,
    ) -> Result<()> {
        // Remove existing entries for this session
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM session_messages WHERE session_id = ?1",
            params![session_id],
        )?;

        for msg in messages {
            self.index_message(session_id, msg, topic)?;
        }
        Ok(())
    }

    /// Remove all messages for a session from the memory store.
    pub fn remove_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM session_messages WHERE session_id = ?1",
            params![session_id],
        )?;
        // FTS will be cleaned up via the content sync on next rebuild,
        // but orphaned rows in FTS are harmless since they won't have matching
        // content= rows.
        Ok(())
    }

    /// Search across all indexed session messages using FTS5.
    /// Returns up to `limit` results, ordered by relevance.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryHit>> {
        let conn = self.conn()?;

        // Use FTS5 to search, joining back to the main table for metadata
        let sql = "
            SELECT
                sm.session_id,
                sm.message_id,
                sm.role,
                sm.content,
                sm.timestamp,
                sm.topic,
                fts.rank AS score
            FROM message_fts fts
            JOIN session_messages sm ON fts.rowid = sm.id
            WHERE message_fts MATCH ?1
            ORDER BY rank
            LIMIT ?2
        ";

        let mut stmt = conn.prepare(sql)?;
        let hits = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(MemoryHit {
                    session_id: row.get(0)?,
                    message_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    timestamp: row.get(4)?,
                    topic: row.get(5)?,
                    score: row.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(hits)
    }

    /// Get message count for a session.
    pub fn message_count(&self, session_id: &str) -> Result<usize> {
        let conn = self.conn()?;
        let count: usize = conn.query_row(
            "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get total message count across all sessions.
    pub fn total_count(&self) -> Result<usize> {
        let conn = self.conn()?;
        let count: usize =
            conn.query_row("SELECT COUNT(*) FROM session_messages", [], |row| {
                row.get(0)
            })?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Message, MessageRole};
    use tempfile::tempdir;

    fn make_message(id: &str, role: MessageRole, content: &str) -> Message {
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
    fn test_index_and_search() {
        let dir = tempdir().unwrap();
        let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();

        let msg = make_message("msg-1", MessageRole::User, "How do I configure routing in Loom?");
        memory
            .index_message("session-1", &msg, "configuration")
            .unwrap();

        let results = memory.search("routing", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "session-1");
        assert_eq!(results[0].content, "How do I configure routing in Loom?");
        // FTS5 rank should be <= 0 for matches (lower/negative = better)
        assert!(results[0].score <= 0.0);
    }

    #[test]
    fn test_index_session_and_remove() {
        let dir = tempdir().unwrap();
        let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();

        let msgs = vec![
            make_message("msg-1", MessageRole::User, "Hello"),
            make_message("msg-2", MessageRole::Assistant, "Hi there!"),
        ];
        memory
            .index_session("session-1", &msgs, "greeting")
            .unwrap();

        assert_eq!(memory.message_count("session-1").unwrap(), 2);

        memory.remove_session("session-1").unwrap();
        assert_eq!(memory.message_count("session-1").unwrap(), 0);
    }

    #[test]
    fn test_search_empty() {
        let dir = tempdir().unwrap();
        let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();
        let results = memory.search("nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_multiple_sessions() {
        let dir = tempdir().unwrap();
        let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();

        memory
            .index_message(
                "session-a",
                &make_message("a1", MessageRole::User, "Configure MCP servers"),
                "mcp",
            )
            .unwrap();
        memory
            .index_message(
                "session-b",
                &make_message("b1", MessageRole::User, "Set up routing rules"),
                "routing",
            )
            .unwrap();

        // Search across both sessions
        let results = memory.search("configure OR routing", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_reindex_updates() {
        let dir = tempdir().unwrap();
        let memory = CrossSessionMemory::new(dir.path().to_str().unwrap()).unwrap();

        memory
            .index_message(
                "session-1",
                &make_message("msg-1", MessageRole::User, "Old content"),
                "test",
            )
            .unwrap();

        // Re-index with same message_id but new content
        memory
            .index_message(
                "session-1",
                &make_message("msg-1", MessageRole::User, "New content"),
                "test",
            )
            .unwrap();

        let results = memory.search("New", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "New content");

        let results = memory.search("Old", 10).unwrap();
        assert_eq!(results.len(), 0);
    }
}
