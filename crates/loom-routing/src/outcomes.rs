//! Persisted routing outcome telemetry (routing-debt.md Step 6).
//!
//! `metrics.rs` counts in-process AtomicU64s — nothing survives a restart,
//! and nothing connects a routing decision to how the task actually went.
//! This store persists the (signals → decision → outcome) triple the
//! routing Supplement promised: one row per task-bound routing decision
//! (signals as JSON, numeric/level/tier, resolved model, token estimate),
//! closed later by an outcome —
//!
//! * `completed`  — the task reached Finish without the decision being
//!   overridden (advance_phase closes all of the task's open rows);
//! * `overridden` — a manual `loom routing override` rejected the tier
//!   the heuristic picked. This is the misroute evidence a future
//!   model-based scorer needs before it may replace the heuristic.
//!
//! Token estimates are recorded so the actual-vs-estimate overrun outcome
//! can be derived once usage metering (ROADMAP.md) starts recording
//! reality; until then estimates are the only leg that exists.
//!
//! The <10ms routing-decision budget is untouched: every write happens
//! after the decision is made, on the task/CLI path rather than inside
//! the scorer or router. Telemetry failures are the callers' choice to
//! ignore — record_* returns Result so callers can warn and move on.

use anyhow::Result;
use loom_core::{ComplexityScore, RoutingDecision};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// SQLite-backed store of routing decisions and their outcomes.
///
/// One row per task-bound routing decision; outcome filled in later.
/// Connection-per-call like `loom-knowledge`'s graph store — writes are
/// task-granular, so open cost is irrelevant next to the task work.
pub struct RoutingOutcomeStore {
    db_path: PathBuf,
}

/// A decision row as read back for reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionRow {
    pub recorded_at: String,
    pub source: String,
    pub task_id: String,
    pub phase: Option<String>,
    pub numeric: f64,
    pub level: String,
    pub tier: String,
    pub model: Option<String>,
    pub est_input_tokens: Option<i64>,
    pub est_output_tokens: Option<i64>,
    pub signals_json: String,
    pub outcome: Option<String>,
    pub outcome_at: Option<String>,
    pub outcome_detail: Option<String>,
}

impl RoutingOutcomeStore {
    /// Open (or create) `{loom_dir}/telemetry/routing.db` and ensure schema.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let telemetry_dir = Path::new(loom_dir).join("telemetry");
        std::fs::create_dir_all(&telemetry_dir)?;
        let db_path = telemetry_dir.join("routing.db");

        let store = Self { db_path };
        store.conn()?.execute_batch(
            // No synthetic id column: SQLite's implicit rowid is the ordering
            // and "latest" key, and nothing external needs to reference a row.
            "CREATE TABLE IF NOT EXISTS routing_decisions (
                recorded_at TEXT NOT NULL,
                source TEXT NOT NULL,
                task_id TEXT NOT NULL,
                phase TEXT,
                numeric REAL NOT NULL,
                level TEXT NOT NULL,
                tier TEXT NOT NULL,
                model TEXT,
                est_input_tokens INTEGER,
                est_output_tokens INTEGER,
                signals_json TEXT NOT NULL,
                outcome TEXT,
                outcome_at TEXT,
                outcome_detail TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_rd_task ON routing_decisions(task_id);
            CREATE INDEX IF NOT EXISTS idx_rd_outcome ON routing_decisions(outcome);",
        )?;
        Ok(store)
    }

    fn conn(&self) -> Result<Connection> {
        loom_core::db::open(&self.db_path)
    }

    /// Append a routing decision with its signals.
    ///
    /// `source` names the writer (`create_task`, `advance_phase`). Ephemeral
    /// CLI paths (`evaluate`/`score`) deliberately don't record: no task, no
    /// outcome can ever attach, and outcome-less rows would pollute the
    /// misroute analysis this store exists for.
    pub fn record_decision(
        &self,
        source: &str,
        task_id: &str,
        phase: Option<&str>,
        score: &ComplexityScore,
        decision: Option<&RoutingDecision>,
    ) -> Result<()> {
        let signals_json = serde_json::to_string(&score.signals)?;
        self.conn()?.execute(
            "INSERT INTO routing_decisions
                (recorded_at, source, task_id, phase, numeric, level, tier, model,
                 est_input_tokens, est_output_tokens, signals_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                loom_core::rfc3339_now(),
                source,
                task_id,
                phase,
                score.numeric,
                format!("{:?}", score.level),
                format!("{:?}", score.model_tier),
                decision.map(|d| d.recommended_model.clone()),
                decision.map(|d| d.cost_estimate.input_tokens_estimated as i64),
                decision.map(|d| d.cost_estimate.output_tokens_estimated as i64),
                signals_json,
            ],
        )?;
        Ok(())
    }

    /// Mark the task's latest open decision `overridden` (manual tier
    /// override) and return how many rows were touched.
    ///
    /// Only the latest open row: earlier phase decisions already steered
    /// completed phases and close normally at completion.
    pub fn record_override(&self, task_id: &str, from_tier: &str, to_tier: &str) -> Result<usize> {
        let now = loom_core::rfc3339_now();
        let detail = serde_json::json!({"from_tier": from_tier, "to_tier": to_tier}).to_string();
        let n = self.conn()?.execute(
            "UPDATE routing_decisions
             SET outcome = 'overridden', outcome_at = ?1, outcome_detail = ?2
             WHERE rowid = (
                 SELECT rowid FROM routing_decisions
                 WHERE task_id = ?3 AND outcome IS NULL
                 ORDER BY rowid DESC LIMIT 1
             )",
            params![now, detail, task_id],
        )?;
        Ok(n)
    }

    /// Mark all of the task's open decisions `completed` (task reached
    /// Finish) and return how many rows were closed.
    pub fn record_completion(&self, task_id: &str) -> Result<usize> {
        let n = self.conn()?.execute(
            "UPDATE routing_decisions
             SET outcome = 'completed', outcome_at = ?1
             WHERE task_id = ?2 AND outcome IS NULL",
            params![loom_core::rfc3339_now(), task_id],
        )?;
        Ok(n)
    }

    /// Most recent decisions, newest first — the `loom routing outcomes`
    /// report.
    pub fn recent_decisions(&self, limit: usize) -> Result<Vec<DecisionRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT recorded_at, source, task_id, phase, numeric, level, tier, model,
                    est_input_tokens, est_output_tokens, signals_json, outcome, outcome_at, outcome_detail
             FROM routing_decisions
             ORDER BY rowid DESC
             LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(DecisionRow {
                    recorded_at: r.get(0)?,
                    source: r.get(1)?,
                    task_id: r.get(2)?,
                    phase: r.get(3)?,
                    numeric: r.get(4)?,
                    level: r.get(5)?,
                    tier: r.get(6)?,
                    model: r.get(7)?,
                    est_input_tokens: r.get(8)?,
                    est_output_tokens: r.get(9)?,
                    signals_json: r.get(10)?,
                    outcome: r.get(11)?,
                    outcome_at: r.get(12)?,
                    outcome_detail: r.get(13)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// (outcome, count) pairs plus `("open", n)` for undecided rows —
    /// the summary line of the report.
    pub fn outcome_counts(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT COALESCE(outcome, 'open'), COUNT(*)
             FROM routing_decisions
             GROUP BY outcome
             ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{ComplexityLevel, ComplexitySignals, ModelTier};

    fn store() -> (tempfile::TempDir, RoutingOutcomeStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = RoutingOutcomeStore::new(dir.path().to_str().unwrap()).unwrap();
        (dir, s)
    }

    fn score(numeric: f64) -> ComplexityScore {
        ComplexityScore {
            level: ComplexityLevel::Medium,
            numeric,
            model_tier: ModelTier::Mid,
            scored_at: "2026-05-17T00:00:00Z".to_string(),
            signals: ComplexitySignals::default(),
        }
    }

    #[test]
    fn record_and_read_back() {
        let (_dir, s) = store();
        s.record_decision("create_task", "t1", Some("Plan"), &score(0.5), None)
            .unwrap();
        let rows = s.recent_decisions(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "create_task");
        assert_eq!(rows[0].task_id, "t1");
        assert_eq!(rows[0].phase.as_deref(), Some("Plan"));
        assert!((rows[0].numeric - 0.5).abs() < f64::EPSILON);
        assert_eq!(rows[0].level, "Medium");
        assert_eq!(rows[0].tier, "Mid");
        assert_eq!(rows[0].outcome, None);
        // The signals leg of the triple is persisted as JSON.
        assert!(serde_json::from_str::<serde_json::Value>(&rows[0].signals_json).is_ok());
    }

    #[test]
    fn override_closes_latest_open_only() {
        let (_dir, s) = store();
        s.record_decision("create_task", "t1", Some("Plan"), &score(0.5), None)
            .unwrap();
        s.record_decision("advance_phase", "t1", Some("Implement"), &score(0.6), None)
            .unwrap();
        s.record_decision("create_task", "t2", Some("Plan"), &score(0.4), None)
            .unwrap();

        let n = s.record_override("t1", "Mid", "Flagship").unwrap();
        assert_eq!(n, 1, "only the latest open row for t1 closes");

        let rows = s.recent_decisions(10).unwrap();
        let t1_latest = rows
            .iter()
            .find(|r| r.task_id == "t1" && r.phase.as_deref() == Some("Implement"))
            .unwrap();
        assert_eq!(t1_latest.outcome.as_deref(), Some("overridden"));
        assert!(t1_latest
            .outcome_detail
            .clone()
            .unwrap()
            .contains("Flagship"));
        let t1_first = rows
            .iter()
            .find(|r| r.task_id == "t1" && r.phase.as_deref() == Some("Plan"))
            .unwrap();
        assert_eq!(t1_first.outcome, None, "earlier phase decision stays open");
        let t2 = rows.iter().find(|r| r.task_id == "t2").unwrap();
        assert_eq!(t2.outcome, None, "other tasks untouched");
    }

    #[test]
    fn completion_closes_all_open_rows_for_task() {
        let (_dir, s) = store();
        s.record_decision("create_task", "t1", Some("Plan"), &score(0.5), None)
            .unwrap();
        s.record_decision("advance_phase", "t1", Some("Implement"), &score(0.6), None)
            .unwrap();

        let n = s.record_completion("t1").unwrap();
        assert_eq!(n, 2);

        let counts = s.outcome_counts().unwrap();
        assert_eq!(counts, vec![("completed".to_string(), 2)]);
        assert!(s
            .recent_decisions(10)
            .unwrap()
            .iter()
            .all(|r| r.outcome.as_deref() == Some("completed")));
    }

    #[test]
    fn override_then_completion_leaves_override_standing() {
        let (_dir, s) = store();
        s.record_decision("create_task", "t1", Some("Plan"), &score(0.5), None)
            .unwrap();
        s.record_override("t1", "Mid", "Flagship").unwrap();
        let n = s.record_completion("t1").unwrap();
        assert_eq!(n, 0, "nothing open left to close");
        assert_eq!(
            s.outcome_counts().unwrap(),
            vec![("overridden".to_string(), 1)]
        );
    }

    #[test]
    fn unknown_task_override_is_noop() {
        let (_dir, s) = store();
        assert_eq!(s.record_override("missing", "Mid", "Slm").unwrap(), 0);
        assert_eq!(s.record_completion("missing").unwrap(), 0);
    }
}
