use anyhow::Result;
use loom_core::{Message, SessionManifest, SESSION_COMPRESSION_THRESHOLD, SESSION_FULL_FIDELITY_COUNT};
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

    /// Replace the full tag set for a session. Returns the new tag set,
    /// deduplicated and whitespace-trimmed (preserves original ordering
    /// of first occurrence). Fails if the session does not exist.
    pub fn set_tags(&self, session_id: &str, tags: &[String]) -> Result<Vec<String>> {
        let mut manifest = self
            .read_manifest(session_id)
            .map_err(|_| anyhow::anyhow!("session '{session_id}' not found"))?;
        manifest.tags = normalize_tags(tags);
        self.save_manifest(&manifest)?;
        Ok(manifest.tags)
    }

    /// Add one or more tags to a session, returning the new full set.
    /// Existing tags are preserved; duplicates (after normalization) are
    /// collapsed. No-op if `tags` is empty.
    pub fn add_tags(&self, session_id: &str, tags: &[String]) -> Result<Vec<String>> {
        let mut manifest = self
            .read_manifest(session_id)
            .map_err(|_| anyhow::anyhow!("session '{session_id}' not found"))?;
        let merged = merge_tag_sets(&manifest.tags, tags);
        manifest.tags = merged.clone();
        self.save_manifest(&manifest)?;
        Ok(merged)
    }

    /// Remove one or more tags from a session, returning the new full set.
    /// Removal is case- and whitespace-sensitive (tags are matched
    /// verbatim after the same normalization `add_tags` applies).
    pub fn remove_tags(&self, session_id: &str, tags: &[String]) -> Result<Vec<String>> {
        let mut manifest = self
            .read_manifest(session_id)
            .map_err(|_| anyhow::anyhow!("session '{session_id}' not found"))?;
        let to_remove: std::collections::HashSet<String> = normalize_tags(tags).into_iter().collect();
        manifest.tags = manifest
            .tags
            .iter()
            .filter(|t| !to_remove.contains(*t))
            .cloned()
            .collect();
        self.save_manifest(&manifest)?;
        Ok(manifest.tags)
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

        // Auto-summarize on replace too. The append path triggers
        // this on its own, but `replace_messages` is the entry point
        // for imports and branch operations, which would otherwise
        // silently leave oversized histories uncompressed.
        if messages.len() > SESSION_COMPRESSION_THRESHOLD {
            if let Err(e) = self.compress_session(session_id) {
                tracing::warn!("auto-summarization failed for '{session_id}': {e}");
            }
        }

        Ok(())
    }

    /// Append one message to JSONL and update the offset index.
    /// If the session's total message count crosses
    /// `SESSION_COMPRESSION_THRESHOLD` after the append, automatically
    /// runs `compress_session` to fold the older history into a
    /// summary. This makes summarization a transparent side effect of
    /// writing — callers don't need to remember to call compress
    /// explicitly.
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

        // Auto-summarize when we cross the threshold. The cost of the
        // check is one length read on a small file plus an O(n) scan
        // inside build_summary if it fires; on a session below the
        // threshold it's just a single `load_messages` we already
        // would do for the offset bookkeeping. We only auto-trigger on
        // append, not on import (imports go through replace_messages,
        // which is also a natural compression point — see below).
        if let Ok(msgs) = self.load_messages(session_id) {
            if msgs.len() > SESSION_COMPRESSION_THRESHOLD {
                if let Err(e) = self.compress_session(session_id) {
                    tracing::warn!("auto-summarization failed for '{session_id}': {e}");
                }
            }
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

    /// Auto-summarize a session. If the session has more than
    /// `SESSION_COMPRESSION_THRESHOLD` messages, fold the older
    /// history into a single `MessageRole::System` summary at the
    /// head of the message list, keeping the last
    /// `SESSION_FULL_FIDELITY_COUNT` messages verbatim. Updates the
    /// manifest's `summarized_at` / `summarized_message_count` fields
    /// in place.
    ///
    /// Returns `Some(folded_count)` when compression actually ran, and
    /// `None` when the session was below the threshold (a no-op
    /// observable to callers — e.g. the CLI uses it to print
    /// "no compression needed" vs "compressed N messages").
    ///
    /// The original full message list is **not** preserved on disk
    /// after this call — that's the point. Callers that need an
    /// audit trail should export the session first (via
    /// `export_session`).
    pub fn compress_session(&self, session_id: &str) -> Result<Option<usize>> {
        let mut manifest = self
            .read_manifest(session_id)
            .map_err(|_| anyhow::anyhow!("session '{session_id}' not found"))?;
        let messages = self.load_messages(session_id)?;
        if let Some(new_messages) = crate::compression::plan_compression(
            &mut manifest,
            &messages,
            SESSION_COMPRESSION_THRESHOLD,
            SESSION_FULL_FIDELITY_COUNT,
        ) {
            let folded = manifest
                .summarized_message_count
                .ok_or_else(|| anyhow::anyhow!("plan_compression did not record folded count"))?
                as usize;
            self.replace_messages(session_id, &new_messages)?;
            self.save_manifest(&manifest)?;
            Ok(Some(folded))
        } else {
            Ok(None)
        }
    }
}

/// Normalize a tag list: trim whitespace, drop empties, dedupe
/// preserving first-occurrence order. Public so callers (CLI, IPC)
/// can reuse the same rules the store applies internally. Tags are
/// stored verbatim (no case folding) — the CLI does not enforce a
/// case convention.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(tags.len());
    for t in tags {
        let trimmed = t.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Merge two tag sets: existing tags first (in their original order),
/// then new tags (normalized, deduped, skipping anything already in
/// existing). Preserves the ordering of the existing set, which makes
/// `add_tags` commutative-friendly for the most common case. Public
/// so CLI / IPC callers can compute the post-add set without a
/// round-trip through the store.
pub fn merge_tag_sets(existing: &[String], additions: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = existing.iter().cloned().collect();
    let mut out: Vec<String> = existing.to_vec();
    for t in normalize_tags(additions) {
        if seen.insert(t.clone()) {
            out.push(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tags_trims_and_dedupes() {
        let input = vec!["foo".into(), "  bar  ".into(), "foo".into(), "".into(), "  ".into()];
        let out = normalize_tags(&input);
        assert_eq!(out, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn test_normalize_tags_preserves_order() {
        let input = vec!["z".into(), "a".into(), "m".into()];
        assert_eq!(
            normalize_tags(&input),
            vec!["z".to_string(), "a".to_string(), "m".to_string()]
        );
    }

    #[test]
    fn test_merge_tag_sets_appends_new() {
        let existing = vec!["a".into(), "b".into()];
        let additions = vec!["c".into(), "d".into()];
        let merged = merge_tag_sets(&existing, &additions);
        assert_eq!(
            merged,
            vec!["a".to_string(), "b".to_string(), "c".to_string(), "d".to_string()]
        );
    }

    #[test]
    fn test_merge_tag_sets_dedupes_overlap() {
        let existing = vec!["a".into(), "b".into()];
        let additions = vec!["b".into(), "c".into(), "  a  ".into()];
        let merged = merge_tag_sets(&existing, &additions);
        assert_eq!(merged, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }
}
