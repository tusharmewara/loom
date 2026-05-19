use anyhow::Result;
use loom_core::{Message, SessionManifest};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub struct SessionStore {
    loom_dir: String,
    sessions_dir: PathBuf,
}

impl SessionStore {
    pub fn new(loom_dir: &str) -> Self {
        let sessions_dir = Path::new(loom_dir).join("sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);
        Self {
            loom_dir: loom_dir.to_string(),
            sessions_dir,
        }
    }

    pub fn loom_dir(&self) -> &str {
        &self.loom_dir
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions_dir
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(session_id)
    }

    fn manifest_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("manifest.json")
    }

    fn messages_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("messages.jsonl")
    }

    fn index_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("index.db")
    }

    fn legacy_manifest_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(format!("{session_id}.json"))
    }

    fn open_index(&self, session_id: &str) -> Result<Connection> {
        let path = self.index_path(session_id);
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS message_offsets (
                line_number INTEGER PRIMARY KEY,
                byte_offset INTEGER NOT NULL,
                message_id TEXT NOT NULL,
                role TEXT NOT NULL
            );
            "#,
        )?;
        Ok(conn)
    }

    /// List all known session manifests (legacy + directory layout).
    pub fn list(&self) -> Vec<SessionManifest> {
        let mut by_id: HashMap<String, SessionManifest> = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(id) = path.file_name().and_then(|n| n.to_str()) {
                        if let Ok(m) = self.read_manifest(id) {
                            by_id.insert(m.session_id.clone(), m);
                        }
                    }
                } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".json") && !name.contains('-') {
                        let _id = name.trim_end_matches(".json");
                        if let Ok(raw) = std::fs::read_to_string(&path) {
                            if let Ok(m) = serde_json::from_str::<SessionManifest>(&raw) {
                                by_id.insert(m.session_id.clone(), m);
                            }
                        }
                    }
                }
            }
        }

        let mut manifests: Vec<_> = by_id.into_values().collect();
        manifests.sort_by(|a, b| a.session_id.cmp(&b.session_id));
        manifests
    }

    pub fn get(&self, session_id: &str) -> Option<SessionManifest> {
        self.read_manifest(session_id).ok()
    }

    fn read_manifest(&self, session_id: &str) -> Result<SessionManifest> {
        let new_path = self.manifest_path(session_id);
        if new_path.exists() {
            let raw = std::fs::read_to_string(new_path)?;
            return Ok(serde_json::from_str(&raw)?);
        }
        let legacy = self.legacy_manifest_path(session_id);
        if legacy.exists() {
            let raw = std::fs::read_to_string(legacy)?;
            return Ok(serde_json::from_str(&raw)?);
        }
        anyhow::bail!("session '{session_id}' not found")
    }

    pub fn save_manifest(&self, manifest: &SessionManifest) -> Result<()> {
        let dir = self.session_dir(&manifest.session_id);
        std::fs::create_dir_all(&dir)?;
        let path = self.manifest_path(&manifest.session_id);
        std::fs::write(path, serde_json::to_string_pretty(manifest)?)?;
        Ok(())
    }

    /// Load all messages for a session from JSONL (falls back to empty).
    pub fn load_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let path = self.messages_path(session_id);
        if !path.exists() {
            return Ok(vec![]);
        }

        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            messages.push(serde_json::from_str(&line)?);
        }
        Ok(messages)
    }

    /// Replace the full message history for a session.
    pub fn replace_messages(&self, session_id: &str, messages: &[Message]) -> Result<()> {
        let dir = self.session_dir(session_id);
        std::fs::create_dir_all(&dir)?;

        let path = self.messages_path(session_id);
        let mut file = File::create(&path)?;
        let index = self.open_index(session_id)?;
        index.execute("DELETE FROM message_offsets", [])?;

        let mut offset: i64 = 0;
        for (line_no, message) in messages.iter().enumerate() {
            let line = serde_json::to_string(message)?;
            writeln!(file, "{line}")?;
            index.execute(
                "INSERT INTO message_offsets (line_number, byte_offset, message_id, role)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    line_no as i64,
                    offset,
                    message.id,
                    format!("{:?}", message.role)
                ],
            )?;
            offset += (line.len() + 1) as i64;
        }

        if let Some(manifest) = self.get(session_id) {
            let mut updated = manifest;
            updated.message_count = messages.len() as u32;
            self.save_manifest(&updated)?;
        }

        Ok(())
    }

    /// Append one message to JSONL and update the offset index.
    pub fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        let dir = self.session_dir(session_id);
        std::fs::create_dir_all(&dir)?;

        let path = self.messages_path(session_id);
        let line = serde_json::to_string(message)?;
        let offset = if path.exists() {
            std::fs::metadata(&path)?.len() as i64
        } else {
            0
        };

        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(file, "{line}")?;

        let index = self.open_index(session_id)?;
        let line_no: i64 = index.query_row(
            "SELECT COALESCE(MAX(line_number), -1) + 1 FROM message_offsets",
            [],
            |row| row.get(0),
        )?;

        index.execute(
            "INSERT INTO message_offsets (line_number, byte_offset, message_id, role)
             VALUES (?1, ?2, ?3, ?4)",
            params![line_no, offset, message.id, format!("{:?}", message.role)],
        )?;

        if let Some(mut manifest) = self.get(session_id) {
            manifest.message_count = manifest.message_count.saturating_add(1);
            self.save_manifest(&manifest)?;
        }

        Ok(())
    }

    /// Read a message by line number using the SQLite offset index.
    pub fn get_message_at_line(
        &self,
        session_id: &str,
        line_number: usize,
    ) -> Result<Option<Message>> {
        let index = self.open_index(session_id)?;
        let offset: Option<i64> = index
            .query_row(
                "SELECT byte_offset FROM message_offsets WHERE line_number = ?1",
                params![line_number as i64],
                |row| row.get(0),
            )
            .ok();

        let Some(offset) = offset else {
            return Ok(None);
        };

        let path = self.messages_path(session_id);
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        let mut current_offset = 0i64;

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line)?;
            if bytes == 0 {
                break;
            }
            if current_offset == offset {
                return Ok(Some(serde_json::from_str(line.trim())?));
            }
            current_offset += bytes as i64;
        }

        Ok(None)
    }

    pub fn compress_history(&self, session_id: &str, max_messages: usize) -> Result<usize> {
        let path = self.messages_path(session_id);
        let removed = crate::compression::compress_jsonl(&path, max_messages)?;
        if removed > 0 {
            // Rebuild index after compression
            let messages = self.load_messages(session_id)?;
            self.replace_messages(session_id, &messages)?;
        }
        Ok(removed)
    }
}
