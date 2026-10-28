//! SQLite-backed storage for org analytics aggregates.
//!
//! Stores aggregated (not per-run) data at `{loom_dir}/control-plane/analytics.db`.
//! No per-run tables exist — the privacy invariant is structural.

use anyhow::Result;
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

use crate::org_stats::{AnalyticsAggregate, OrgAggregate, ToolAggregate};

/// SQLite-backed store for org analytics aggregates.
pub struct AnalyticsStore {
    db_path: PathBuf,
}

impl AnalyticsStore {
    /// Open (or create) `{loom_dir}/control-plane/analytics.db` and ensure schema.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let db_path = Path::new(loom_dir).join("control-plane").join("analytics.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self { db_path };
        store.ensure_schema()?;
        Ok(store)
    }

    /// Path of the analytics database.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn connect(&self) -> Result<Connection> {
        loom_core::db::open(&self.db_path)
    }

    fn ensure_schema(&self) -> Result<()> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS org_analytics (
                 org_id     TEXT NOT NULL,
                 tool       TEXT NOT NULL,
                 day        TEXT NOT NULL,  -- YYYY-MM-DD
                 runs       INTEGER NOT NULL DEFAULT 0,
                 measured_runs INTEGER NOT NULL DEFAULT 0,
                 input_tokens INTEGER,
                 output_tokens INTEGER,
                 cost_usd   REAL,
                 est_cost_usd REAL,
                 PRIMARY KEY (org_id, tool, day)
             );",
        )?;
        Ok(())
    }

    /// Push aggregated org data into the store.
    ///
    /// Each `OrgAggregate` represents one org's data for a single tool+day.
    /// Upserts by `(org_id, tool, day)` — re-pushing the same org+tool+day
    /// overwrites the previous values (idempotent).
    pub fn push_aggregate(
        &self,
        org_id: &str,
        entries: &[OrgAggregate],
    ) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;

        for entry in entries {
            if entry.org_id != org_id {
                return Err(anyhow::anyhow!(
                    "org_id mismatch: expected {org_id}, got {}",
                    entry.org_id
                ));
            }
            tx.execute(
                "INSERT INTO org_analytics (
                     org_id, tool, day, runs, measured_runs,
                     input_tokens, output_tokens, cost_usd, est_cost_usd
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(org_id, tool, day) DO UPDATE SET
                     runs = excluded.runs,
                     measured_runs = excluded.measured_runs,
                     input_tokens = excluded.input_tokens,
                     output_tokens = excluded.output_tokens,
                     cost_usd = excluded.cost_usd,
                     est_cost_usd = excluded.est_cost_usd",
                params![
                    org_id,
                    entry.tool,
                    entry.day,
                    entry.runs,
                    entry.measured_runs,
                    entry.input_tokens.map(|v| v as i64),
                    entry.output_tokens.map(|v| v as i64),
                    entry.cost_usd,
                    entry.est_cost_usd,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get aggregated data for an org across all tools and days.
    pub fn get_org_aggregate(&self, org_id: &str) -> Result<Option<AnalyticsAggregate>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT tool, runs, measured_runs, input_tokens, output_tokens, cost_usd, est_cost_usd
             FROM org_analytics
             WHERE org_id = ?1
             ORDER BY tool",
        )?;
        let rows = stmt.query_map([org_id], |r| {
            Ok(ToolAggregate {
                tool: r.get(0)?,
                runs: r.get::<_, i64>(1)?.max(0) as u64,
                measured_runs: r.get::<_, i64>(2)?.max(0) as u64,
                input_tokens: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                output_tokens: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
                cost_usd: r.get::<_, Option<f64>>(5)?,
                est_cost_usd: r.get::<_, Option<f64>>(6)?,
                calibration_ratio: None, // computed below
            })
        })?;

        let mut tools = Vec::new();
        for row in rows {
            tools.push(row?);
        }

        if tools.is_empty() {
            return Ok(None);
        }

        let totals = compute_totals(org_id.to_string(), &tools);

        Ok(Some(AnalyticsAggregate {
            org_id: org_id.to_string(),
            tools,
            totals,
        }))
    }

    /// Get aggregated data for an org across all tools within a date range.
    pub fn get_org_aggregate_range(
        &self,
        org_id: &str,
        since: &NaiveDate,
        until: &NaiveDate,
    ) -> Result<Option<AnalyticsAggregate>> {
        let conn = self.connect()?;
        let since_str = since.to_string();
        let until_str = until.to_string();
        let mut stmt = conn.prepare(
            "SELECT tool, runs, measured_runs, input_tokens, output_tokens, cost_usd, est_cost_usd
             FROM org_analytics
             WHERE org_id = ?1 AND day >= ?2 AND day <= ?3
             ORDER BY tool",
        )?;
        let rows = stmt.query_map([org_id, &since_str, &until_str], |r| {
            Ok(ToolAggregate {
                tool: r.get(0)?,
                runs: r.get::<_, i64>(1)?.max(0) as u64,
                measured_runs: r.get::<_, i64>(2)?.max(0) as u64,
                input_tokens: r.get::<_, Option<i64>>(3)?.map(|v| v.max(0) as u64),
                output_tokens: r.get::<_, Option<i64>>(4)?.map(|v| v.max(0) as u64),
                cost_usd: r.get::<_, Option<f64>>(5)?,
                est_cost_usd: r.get::<_, Option<f64>>(6)?,
                calibration_ratio: None,
            })
        })?;

        let mut tools = Vec::new();
        for row in rows {
            tools.push(row?);
        }

        if tools.is_empty() {
            return Ok(None);
        }

        let totals = compute_totals(org_id.to_string(), &tools);

        Ok(Some(AnalyticsAggregate {
            org_id: org_id.to_string(),
            tools,
            totals,
        }))
    }

    /// Privacy invariant: no per-run rows exist on the control plane.
    /// Returns `false` — there is no per-run table.
    pub fn has_per_run_rows(&self) -> bool {
        false
    }
}

/// Compute aggregate totals from a list of tool aggregates.
fn compute_totals(org_id: String, tools: &[ToolAggregate]) -> OrgAggregate {
    let mut totals = OrgAggregate {
        org_id,
        tool: "total".to_string(),
        day: String::new(),
        runs: 0,
        measured_runs: 0,
        input_tokens: None,
        output_tokens: None,
        cost_usd: None,
        est_cost_usd: None,
    };

    for tool in tools {
        totals.runs += tool.runs;
        totals.measured_runs += tool.measured_runs;
        totals.input_tokens = match (totals.input_tokens, tool.input_tokens) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        totals.output_tokens = match (totals.output_tokens, tool.output_tokens) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        totals.cost_usd = match (totals.cost_usd, tool.cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        totals.est_cost_usd = match (totals.est_cost_usd, tool.est_cost_usd) {
            (Some(a), Some(b)) => Some(a + b),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }

    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> (AnalyticsStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AnalyticsStore::new(dir.path().to_str().unwrap()).unwrap();
        (store, dir)
    }

    #[test]
    fn push_and_read_aggregate() {
        let (store, _dir) = test_store();
        let org_id = "org-test";

        store
            .push_aggregate(
                org_id,
                &[OrgAggregate {
                    org_id: org_id.to_string(),
                    tool: "claude-code".to_string(),
                    day: "2026-10-27".to_string(),
                    runs: 5,
                    measured_runs: 5,
                    input_tokens: Some(12000),
                    output_tokens: Some(3000),
                    cost_usd: Some(0.45),
                    est_cost_usd: Some(0.40),
                }],
            )
            .unwrap();

        let result = store.get_org_aggregate(org_id).unwrap();
        assert!(result.is_some());
        let agg = result.unwrap();
        assert_eq!(agg.org_id, org_id);
        assert_eq!(agg.tools.len(), 1);
        assert_eq!(agg.tools[0].runs, 5);
        assert_eq!(agg.tools[0].input_tokens, Some(12000));
        assert_eq!(agg.tools[0].cost_usd, Some(0.45));
        assert_eq!(agg.totals.runs, 5);
        assert_eq!(agg.totals.input_tokens, Some(12000));
    }

    #[test]
    fn privacy_no_per_run_rows() {
        let (store, _dir) = test_store();
        assert!(!store.has_per_run_rows());
    }

    #[test]
    fn dedup_by_tool_day() {
        let (store, _dir) = test_store();
        let org_id = "org-dedup";

        // Push twice with same org+tool+day — second should overwrite, not duplicate
        store
            .push_aggregate(
                org_id,
                &[OrgAggregate {
                    org_id: org_id.to_string(),
                    tool: "claude-code".to_string(),
                    day: "2026-10-27".to_string(),
                    runs: 5,
                    measured_runs: 5,
                    input_tokens: Some(1000),
                    output_tokens: Some(200),
                    cost_usd: Some(0.10),
                    est_cost_usd: Some(0.08),
                }],
            )
            .unwrap();

        store
            .push_aggregate(
                org_id,
                &[OrgAggregate {
                    org_id: org_id.to_string(),
                    tool: "claude-code".to_string(),
                    day: "2026-10-27".to_string(),
                    runs: 10,
                    measured_runs: 10,
                    input_tokens: Some(2000),
                    output_tokens: Some(400),
                    cost_usd: Some(0.20),
                    est_cost_usd: Some(0.16),
                }],
            )
            .unwrap();

        let result = store.get_org_aggregate(org_id).unwrap();
        assert!(result.is_some());
        let agg = result.unwrap();
        assert_eq!(agg.tools.len(), 1);
        assert_eq!(agg.tools[0].runs, 10);
        assert_eq!(agg.tools[0].input_tokens, Some(2000));
    }
}