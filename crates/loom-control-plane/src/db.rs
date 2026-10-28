//! SQLite schema + queries for the control-plane MVP.
//!
//! WAL mode (set in `AppState::new`). One connection, mutex-guarded — fine for
//! the single-process MVP.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS orgs (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            tier TEXT NOT NULL DEFAULT 'free',
            license_key TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS humans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            org_id TEXT NOT NULL,
            email TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'member',
            created_at TEXT NOT NULL,
            FOREIGN KEY(org_id) REFERENCES orgs(id)
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_humans_org_email
            ON humans(org_id, lower(email));
        CREATE TABLE IF NOT EXISTS machine_keys (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            org_id TEXT NOT NULL,
            machine_pub TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL,
            revoked INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY(org_id) REFERENCES orgs(id)
        );
        CREATE TABLE IF NOT EXISTS machine_tokens (
            token TEXT PRIMARY KEY,
            machine_pub TEXT NOT NULL,
            org_id TEXT NOT NULL,
            revoked INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            FOREIGN KEY(machine_pub) REFERENCES machine_keys(machine_pub)
        );
        ",
    )?;
    Ok(())
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn insert_org(conn: &Connection, id: &str, name: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO orgs (id, name, tier, created_at) VALUES (?1, ?2, 'free', ?3)",
        rusqlite::params![id, name, now_rfc3339()],
    )?;
    Ok(())
}

pub fn get_org(conn: &Connection, org_id: &str) -> rusqlite::Result<Option<(String, String, String)>> {
    // returns (name, tier, license_key)
    conn.query_row(
        "SELECT name, tier, COALESCE(license_key, '') FROM orgs WHERE id = ?1",
        rusqlite::params![org_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
    )
    .optional()
}

pub fn human_count(conn: &Connection, org_id: &str) -> rusqlite::Result<u32> {
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM humans WHERE org_id = ?1",
            rusqlite::params![org_id],
            |r| r.get(0),
        )?;
    Ok(count)
}

pub fn insert_human(
    conn: &Connection,
    org_id: &str,
    email: &str,
    role: &str,
) -> rusqlite::Result<u64> {
    Ok(conn.execute(
        "INSERT INTO humans (org_id, email, role, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![org_id, email, role, now_rfc3339()],
    )? as u64)
}

pub fn list_humans(conn: &Connection, org_id: &str) -> rusqlite::Result<Vec<(String, String)>> {
    // (email, role)
    let mut stmt = conn.prepare(
        "SELECT email, role FROM humans WHERE org_id = ?1 ORDER BY email",
    )?;
    let rows = stmt.query_map(rusqlite::params![org_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn delete_human(conn: &Connection, org_id: &str, email: &str) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "DELETE FROM humans WHERE org_id = ?1 AND lower(email) = lower(?2)",
        rusqlite::params![org_id, email],
    )?;
    Ok(changed > 0)
}

pub fn get_license_key(conn: &Connection, org_id: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT license_key FROM orgs WHERE id = ?1 AND license_key IS NOT NULL",
        rusqlite::params![org_id],
        |r| r.get(0),
    )
    .optional()
}

pub fn set_license_key(conn: &Connection, org_id: &str, license_key: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE orgs SET license_key = ?1 WHERE id = ?2",
        rusqlite::params![license_key, org_id],
    )?;
    Ok(())
}

pub fn insert_machine_key(
    conn: &Connection,
    org_id: &str,
    machine_pub: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO machine_keys (org_id, machine_pub, created_at, revoked) VALUES (?1, ?2, ?3, 0)",
        rusqlite::params![org_id, machine_pub, now_rfc3339()],
    )?;
    Ok(())
}

pub fn get_machine_key(conn: &Connection, machine_pub: &str) -> rusqlite::Result<Option<String>> {
    // returns org_id if key exists and not revoked
    conn.query_row(
        "SELECT org_id FROM machine_keys WHERE machine_pub = ?1 AND revoked = 0",
        rusqlite::params![machine_pub],
        |r| r.get(0),
    )
    .optional()
}

pub fn list_machines(conn: &Connection, org_id: &str) -> rusqlite::Result<Vec<(String, bool)>> {
    // (machine_pub, revoked)
    let mut stmt = conn.prepare(
        "SELECT machine_pub, revoked FROM machine_keys WHERE org_id = ?1 ORDER BY machine_pub",
    )?;
    let rows = stmt.query_map(rusqlite::params![org_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i32>(1)? != 0))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn insert_token(
    conn: &Connection,
    token: &str,
    machine_pub: &str,
    org_id: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO machine_tokens (token, machine_pub, org_id, revoked, created_at) VALUES (?1, ?2, ?3, 0, ?4)",
        rusqlite::params![token, machine_pub, org_id, now_rfc3339()],
    )?;
    Ok(())
}

pub fn get_token(conn: &Connection, token: &str) -> rusqlite::Result<Option<(String, String)>> {
    // returns (machine_pub, org_id) if not revoked
    conn.query_row(
        "SELECT machine_pub, org_id FROM machine_tokens WHERE token = ?1 AND revoked = 0",
        rusqlite::params![token],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .optional()
}

pub fn list_machines_for_membership(conn: &Connection, org_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT machine_pub FROM machine_keys WHERE org_id = ?1 AND revoked = 0 ORDER BY machine_pub",
    )?;
    let rows = stmt.query_map(rusqlite::params![org_id], |r| r.get(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}
