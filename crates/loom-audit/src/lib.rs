//! `loom-audit` — structured audit logging for the control plane.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Option<i64>,
    pub timestamp: DateTime<Utc>,
    pub actor_type: ActorType,
    pub actor_id: String,
    pub action: String,
    pub resource: Option<String>,
    pub outcome: Outcome,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActorType {
    Human,
    Machine,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AuditFilter {
    pub actor_type: Option<ActorType>,
    pub actor_id: Option<String>,
    pub action: Option<String>,
    pub resource: Option<String>,
    pub outcome: Option<Outcome>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub struct AuditLogger {
    conn: Mutex<Connection>,
}

impl AuditLogger {
    pub fn new(loom_dir: &str) -> Result<Self> {
        let db_path = Path::new(loom_dir).join("control-plane").join("audit.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = loom_core::db::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                actor_type TEXT NOT NULL,
                actor_id TEXT NOT NULL,
                action TEXT NOT NULL,
                resource TEXT,
                outcome TEXT NOT NULL CHECK (outcome IN ('success','failure')),
                details TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_audit_actor ON audit_log(actor_type, actor_id);
            CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn log(&self, entry: AuditEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO audit_log (timestamp, actor_type, actor_id, action, resource, outcome, details)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.timestamp.to_rfc3339(),
                format!("{:?}", entry.actor_type).to_lowercase(),
                entry.actor_id,
                entry.action,
                entry.resource,
                format!("{:?}", entry.outcome).to_lowercase(),
                entry.details.map(|v| v.to_string()),
            ],
        )?;
        Ok(())
    }

    pub fn query(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT id, timestamp, actor_type, actor_id, action, resource, outcome, details FROM audit_log WHERE 1=1".to_string();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(at) = filter.actor_type {
            sql.push_str(" AND actor_type = ?");
            params_vec.push(Box::new(format!("{:?}", at).to_lowercase()));
        }
        if let Some(ai) = filter.actor_id {
            sql.push_str(" AND actor_id = ?");
            params_vec.push(Box::new(ai));
        }
        if let Some(ac) = filter.action {
            sql.push_str(" AND action = ?");
            params_vec.push(Box::new(ac));
        }
        if let Some(r) = filter.resource {
            sql.push_str(" AND resource = ?");
            params_vec.push(Box::new(r));
        }
        if let Some(o) = filter.outcome {
            sql.push_str(" AND outcome = ?");
            params_vec.push(Box::new(format!("{:?}", o).to_lowercase()));
        }
        if let Some(s) = filter.since {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(s.to_rfc3339()));
        }
        if let Some(u) = filter.until {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(u.to_rfc3339()));
        }
        sql.push_str(" ORDER BY timestamp DESC");
        if let Some(l) = filter.limit {
            sql.push_str(&format!(" LIMIT {}", l));
        }
        if let Some(o) = filter.offset {
            sql.push_str(&format!(" OFFSET {}", o));
        }

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(&param_refs[..], |row| {
            Ok(AuditEntry {
                id: Some(row.get(0)?),
                timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(1)?)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                actor_type: match row.get::<_, String>(2)?.as_str() {
                    "human" => ActorType::Human,
                    "machine" => ActorType::Machine,
                    _ => ActorType::System,
                },
                actor_id: row.get(3)?,
                action: row.get(4)?,
                resource: row.get(5)?,
                outcome: match row.get::<_, String>(6)?.as_str() {
                    "success" => Outcome::Success,
                    _ => Outcome::Failure,
                },
                details: row.get::<_, Option<String>>(7)?.and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_logger() -> AuditLogger {
        let dir = tempdir().unwrap();
        AuditLogger::new(dir.path().to_str().unwrap()).unwrap()
    }

    #[test]
    fn log_and_query() {
        let logger = test_logger();
        let entry = AuditEntry {
            id: None,
            timestamp: Utc::now(),
            actor_type: ActorType::Human,
            actor_id: "admin@example.com".into(),
            action: "sso_config_update".into(),
            resource: Some("org/acme".into()),
            outcome: Outcome::Success,
            details: Some(serde_json::json!({"method": "oidc"})),
        };
        logger.log(entry.clone()).unwrap();

        let entries = logger.query(Default::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor_id, "admin@example.com");
        assert_eq!(entries[0].action, "sso_config_update");
    }

    #[test]
    fn filter_by_actor() {
        let logger = test_logger();
        logger.log(AuditEntry {
            id: None,
            timestamp: Utc::now(),
            actor_type: ActorType::Human,
            actor_id: "alice".into(),
            action: "login".into(),
            resource: None,
            outcome: Outcome::Success,
            details: None,
        }).unwrap();
        logger.log(AuditEntry {
            id: None,
            timestamp: Utc::now(),
            actor_type: ActorType::Machine,
            actor_id: "machine-1".into(),
            action: "heartbeat".into(),
            resource: None,
            outcome: Outcome::Success,
            details: None,
        }).unwrap();

        let mut filter = AuditFilter::default();
        filter.actor_type = Some(ActorType::Human);
        let entries = logger.query(filter).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].actor_id, "alice");
    }
}