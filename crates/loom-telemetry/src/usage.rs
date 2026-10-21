//! The usage ledger: one row per executed run, per tool.
//!
//! This is the single owner of token/cost accounting in Loom. The routing
//! outcome store (`loom-routing`) records *decisions* — which model was
//! picked and what it was forecast to cost; this store records *reality* —
//! what a run actually consumed. The two are deliberately separate: a
//! decision is made before a run exists, so mixing them would mean writing
//! a measurement into a row that is still open.
//!
//! Every row carries its provenance (`source`): `measured` when the
//! provider or protocol reported the count, `estimated` when a documented
//! heuristic produced it, `unavailable` when nothing reported a count. A
//! row with `unavailable` carries no counts at all — no placeholder, no
//! zero standing in for a real zero.
//!
//! A row is `measured` when the source reported *something* — a token
//! count, a context-window size, or a cost. Those are three different
//! facts, and the ledger keeps them apart: a run that reported only a
//! cost is `measured` but reported no token count, so the token sums
//! over it stay `None` ("unavailable"), never `0`. Only a run whose
//! source reported an input or output count counts toward
//! `measured_runs`.
//!
//! # What is metered today
//!
//! Measured now: `resume` (provider usage), `acp` (protocol usage),
//! `knowledge-extract` (provider usage). `workflow` is recorded with
//! source `unavailable` — it is in the ledger, but it carries no count.
//!
//! **Not** metered yet: `loom exec`, the TUI dispatch path
//! (`loom-tui/src/dispatch.rs`), the TUI Cost tab
//! (`loom-tui/src/ui/tabs/cost.rs`), and multi-turn session cost. The
//! ledger does not pretend to cover them — an absent row is not a
//! zero-cost run.
//!
//! # Estimators
//!
//! The forecast leg (`est_*`) uses `loom_core::estimate_tokens` — a
//! `chars/4` ceiling over the prompt. A second, different `chars/4`
//! estimator exists in `loom-sessions/src/compression.rs` (per-message
//! `(len+4)/4`, floored, summed); it sizes context for compression, not
//! for this ledger, and the two do not agree.
//!
//! Storage is SQLite at `{loom_dir}/telemetry/usage.db`, next to the
//! routing outcome store's database, opened with the ecosystem's shared
//! pragmas (`loom_core::db::open`).

use anyhow::Result;
use loom_core::{UsageCounts, UsageRecord, UsageSource};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

/// SQLite-backed ledger of per-run token and cost usage.
pub struct UsageMeter {
    db_path: PathBuf,
}

/// Totals across every recorded run.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct UsageTotals {
    /// Runs recorded, whatever their source.
    pub runs: u64,
    /// Runs whose source reported a usage figure of any kind (a token
    /// count, a context size, or a cost).
    pub reported_runs: u64,
    /// Runs whose source reported a *token count*. A run that reported only
    /// a cost or only a context size is reported but not measured here —
    /// it has no token count to sum.
    pub measured_runs: u64,
    /// Runs whose source reported nothing.
    pub unavailable_runs: u64,
    /// Input tokens across the runs that reported one. `None` = no run in
    /// this scope reported an input count. Distinct from 0.
    pub input_tokens: Option<u64>,
    /// Output tokens across the runs that reported one. `None` = no run in
    /// this scope reported an output count. Distinct from 0.
    pub output_tokens: Option<u64>,
    /// Cost reported by the source (or priced from measured counts), in USD.
    /// `None` = no run in this scope has a cost. Distinct from $0.
    pub cost_usd: Option<f64>,
    /// Forecast cost (the `chars/4` estimate) for the same runs. `None` =
    /// no forecast could be priced (the catalog does not know the model).
    pub est_cost_usd: Option<f64>,
    /// Forecast input tokens for the same runs.
    pub est_input_tokens: u64,
    /// Forecast output tokens for the same runs.
    pub est_output_tokens: u64,
}

/// Totals for a single tool.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ToolUsage {
    pub tool: String,
    pub runs: u64,
    /// Runs whose source reported a usage figure of any kind.
    pub reported_runs: u64,
    /// Runs whose source reported a token count.
    pub measured_runs: u64,
    pub unavailable_runs: u64,
    /// Input tokens across the runs that reported one. `None` = none did.
    pub input_tokens: Option<u64>,
    /// Output tokens across the runs that reported one. `None` = none did.
    pub output_tokens: Option<u64>,
    /// Cost of the measured runs, in USD. `None` = no run in this scope
    /// has a cost. Distinct from $0.
    pub cost_usd: Option<f64>,
    /// Forecast cost for the same runs. `None` = no forecast could be
    /// priced (the catalog does not know the model).
    pub est_cost_usd: Option<f64>,
}

impl UsageMeter {
    /// Open (or create) `{loom_dir}/telemetry/usage.db` and ensure schema.
    pub fn new(loom_dir: &str) -> Result<Self> {
        let db_path = Path::new(loom_dir).join("telemetry").join("usage.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let meter = Self { db_path };
        meter.ensure_schema()?;
        Ok(meter)
    }

    /// Where the ledger lives.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    fn connect(&self) -> Result<Connection> {
        let conn = loom_core::db::open(&self.db_path)?;
        Ok(conn)
    }

    fn ensure_schema(&self) -> Result<()> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_runs (
                 recorded_at     TEXT NOT NULL,
                 run_id          TEXT NOT NULL,
                 tool            TEXT NOT NULL,
                 model           TEXT NOT NULL,
                 source          TEXT NOT NULL,
                 method          TEXT NOT NULL,
                 input_tokens    INTEGER,
                 output_tokens   INTEGER,
                 context_tokens  INTEGER,
                 cost_usd        REAL,
                 est_input_tokens  INTEGER,
                 est_output_tokens INTEGER,
                 est_cost_usd      REAL
             );
             CREATE INDEX IF NOT EXISTS idx_usage_runs_tool ON usage_runs(tool);
             CREATE INDEX IF NOT EXISTS idx_usage_runs_recorded ON usage_runs(recorded_at);",
        )?;
        Ok(())
    }

    /// Record one run. Counts that the source did not report stay `NULL` —
    /// the ledger never stores a fabricated count.
    pub fn record(&self, record: &UsageRecord) -> Result<()> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO usage_runs (
                 recorded_at, run_id, tool, model, source, method,
                 input_tokens, output_tokens, context_tokens, cost_usd,
                 est_input_tokens, est_output_tokens, est_cost_usd
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                record.recorded_at,
                record.run_id,
                record.tool,
                record.model,
                record.source.as_str(),
                record.method,
                record.input_tokens.map(|v| v as i64),
                record.output_tokens.map(|v| v as i64),
                record.context_tokens.map(|v| v as i64),
                record.cost_usd,
                record.est_input_tokens.map(|v| v as i64),
                record.est_output_tokens.map(|v| v as i64),
                record.est_cost_usd,
            ],
        )?;
        Ok(())
    }

    /// Record one executed run: what the source reported, plus the
    /// `chars/4` forecast for the same prompt.
    ///
    /// The cost is the source's own number when it gave one; otherwise it
    /// is derived from the *measured* token counts and the catalog price.
    /// With neither, the cost stays unavailable — never the estimate.
    #[allow(clippy::too_many_arguments)]
    pub fn record_run(
        &self,
        run_id: &str,
        tool: &str,
        model: &str,
        counts: UsageCounts,
        method: &str,
        prompt_chars: usize,
        prices: Option<(f64, f64)>,
    ) -> Result<UsageRecord> {
        let mut record = UsageRecord::from_counts(run_id, tool, model, counts, method);
        if record.cost_usd.is_none() {
            if let (Some(input), Some(output)) = (record.input_tokens, record.output_tokens) {
                if let Some((input_cost_per_1k, output_cost_per_1k)) = prices {
                    record.cost_usd = loom_core::measured_cost_usd(
                        Some(input),
                        Some(output),
                        input_cost_per_1k,
                        output_cost_per_1k,
                    );
                }
            }
        }
        let record = record.with_estimate(prompt_chars, prices);
        self.record(&record)?;
        Ok(record)
    }

    /// Totals across every recorded run.
    ///
    /// The token sums are `SUM(input_tokens)` with NO `COALESCE`: a run
    /// that reported no count leaves the column NULL, and a scope where
    /// NO run reported a count sums to NULL — "unavailable", not 0. A
    /// `COALESCE` here would manufacture a zero that reads like a
    /// measurement.
    pub fn totals(&self) -> Result<UsageTotals> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT
                 COUNT(*),
                 SUM(CASE WHEN source = 'measured' THEN 1 ELSE 0 END),
                 SUM(CASE WHEN source = 'unavailable' THEN 1 ELSE 0 END),
                 SUM(CASE WHEN input_tokens IS NOT NULL OR output_tokens IS NOT NULL THEN 1 ELSE 0 END),
                 SUM(input_tokens),
                 SUM(output_tokens),
                 SUM(cost_usd),
                 SUM(est_cost_usd),
                 SUM(COALESCE(est_input_tokens, 0)),
                 SUM(COALESCE(est_output_tokens, 0))
             FROM usage_runs",
        )?;
        let mut totals = UsageTotals::default();
        let row = stmt.query_row([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<f64>>(7)?,
                r.get::<_, Option<i64>>(8)?,
                r.get::<_, Option<i64>>(9)?,
            ))
        })?;
        totals.runs = row.0.max(0) as u64;
        totals.reported_runs = row.1.unwrap_or(0).max(0) as u64;
        totals.unavailable_runs = row.2.unwrap_or(0).max(0) as u64;
        totals.measured_runs = row.3.unwrap_or(0).max(0) as u64;
        totals.input_tokens = row.4.map(|v| v.max(0) as u64);
        totals.output_tokens = row.5.map(|v| v.max(0) as u64);
        totals.cost_usd = row.6;
        totals.est_cost_usd = row.7;
        totals.est_input_tokens = row.8.unwrap_or(0).max(0) as u64;
        totals.est_output_tokens = row.9.unwrap_or(0).max(0) as u64;
        Ok(totals)
    }

    /// Totals grouped by tool — the per-tool half of the accounting.
    ///
    /// Same nullable rule as [`UsageMeter::totals`]: no `COALESCE` on the
    /// token sums, so a tool whose runs all reported no count reads
    /// "unavailable" rather than "0/0".
    pub fn by_tool(&self) -> Result<Vec<ToolUsage>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT
                 tool,
                 COUNT(*),
                 SUM(CASE WHEN source = 'measured' THEN 1 ELSE 0 END),
                 SUM(CASE WHEN source = 'unavailable' THEN 1 ELSE 0 END),
                 SUM(CASE WHEN input_tokens IS NOT NULL OR output_tokens IS NOT NULL THEN 1 ELSE 0 END),
                 SUM(input_tokens),
                 SUM(output_tokens),
                 SUM(cost_usd),
                 SUM(est_cost_usd)
             FROM usage_runs
             GROUP BY tool
             ORDER BY tool",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ToolUsage {
                tool: r.get(0)?,
                runs: r.get::<_, i64>(1)?.max(0) as u64,
                reported_runs: r.get::<_, Option<i64>>(2)?.unwrap_or(0).max(0) as u64,
                unavailable_runs: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
                measured_runs: r.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
                input_tokens: r.get::<_, Option<i64>>(5)?.map(|v| v.max(0) as u64),
                output_tokens: r.get::<_, Option<i64>>(6)?.map(|v| v.max(0) as u64),
                cost_usd: r.get::<_, Option<f64>>(7)?,
                est_cost_usd: r.get::<_, Option<f64>>(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The most recent runs, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<UsageRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT recorded_at, run_id, tool, model, source, method,
                    input_tokens, output_tokens, context_tokens, cost_usd,
                    est_input_tokens, est_output_tokens, est_cost_usd
             FROM usage_runs
             ORDER BY rowid DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(UsageRecord {
                recorded_at: r.get(0)?,
                run_id: r.get(1)?,
                tool: r.get(2)?,
                model: r.get(3)?,
                source: UsageSource::from_str_lossy(&r.get::<_, String>(4)?),
                method: r.get(5)?,
                input_tokens: r.get::<_, Option<i64>>(6)?.map(|v| v.max(0) as u64),
                output_tokens: r.get::<_, Option<i64>>(7)?.map(|v| v.max(0) as u64),
                context_tokens: r.get::<_, Option<i64>>(8)?.map(|v| v.max(0) as u64),
                cost_usd: r.get::<_, Option<f64>>(9)?,
                est_input_tokens: r.get::<_, Option<i64>>(10)?.map(|v| v.max(0) as u64),
                est_output_tokens: r.get::<_, Option<i64>>(11)?.map(|v| v.max(0) as u64),
                est_cost_usd: r.get::<_, Option<f64>>(12)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter() -> (UsageMeter, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let meter = UsageMeter::new(dir.path().to_str().unwrap()).unwrap();
        (meter, dir)
    }

    #[test]
    fn records_measured_runs_per_tool() {
        let (meter, _dir) = meter();
        let mut rec = UsageRecord::measured("run-1", "acp", "claude", 1200, 300, "provider");
        rec.cost_usd = Some(0.42);
        meter.record(&rec).unwrap();
        meter
            .record(&UsageRecord::measured(
                "run-2", "acp", "claude", 800, 100, "provider",
            ))
            .unwrap();
        meter
            .record(&UsageRecord::measured(
                "run-3", "resume", "gpt-4o", 10, 5, "provider",
            ))
            .unwrap();

        let totals = meter.totals().unwrap();
        assert_eq!(totals.runs, 3);
        assert_eq!(totals.measured_runs, 3);
        assert_eq!(totals.input_tokens, Some(2010));
        assert_eq!(totals.output_tokens, Some(405));
        assert!((totals.cost_usd.unwrap() - 0.42).abs() < 1e-9);

        let by_tool = meter.by_tool().unwrap();
        assert_eq!(by_tool.len(), 2);
        let acp = by_tool.iter().find(|t| t.tool == "acp").unwrap();
        assert_eq!(acp.runs, 2);
        assert_eq!(acp.input_tokens, Some(2000));
    }

    #[test]
    fn unavailable_run_stores_no_counts() {
        let (meter, _dir) = meter();
        meter
            .record(&UsageRecord::unavailable("run-x", "acp", "agent"))
            .unwrap();

        let totals = meter.totals().unwrap();
        assert_eq!(totals.runs, 1);
        assert_eq!(totals.unavailable_runs, 1);
        assert_eq!(totals.measured_runs, 0);
        // No count at all — not 1000, not zero.
        assert_eq!(totals.input_tokens, None);
        assert_eq!(totals.output_tokens, None);
        // And no cost: unknown is not zero.
        assert_eq!(totals.cost_usd, None);

        let rows = meter.recent(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, UsageSource::Unavailable);
        assert_eq!(rows[0].input_tokens, None);
        assert_eq!(rows[0].output_tokens, None);
    }

    /// A run that reported only a context size and a cost did NOT report a
    /// token count. The aggregate must say so (None), not 0 — a zero reads
    /// like a measurement of zero tokens.
    #[test]
    fn context_and_cost_only_run_leaves_the_token_sum_unavailable() {
        let (meter, _dir) = meter();
        let counts = loom_core::UsageCounts {
            context_tokens: Some(4321),
            cost_usd: Some(0.0123),
            ..Default::default()
        };
        let rec = loom_core::UsageRecord::from_counts(
            "run-c",
            "acp",
            "claude",
            counts,
            "acp:session-context-cost",
        );
        // The source reported something, so the run is "reported"...
        assert_eq!(rec.source, loom_core::UsageSource::Measured);
        // ...but no token count was reported.
        assert!(!rec.reported_token_count());
        meter.record(&rec).unwrap();

        let totals = meter.totals().unwrap();
        assert_eq!(totals.runs, 1);
        assert_eq!(totals.reported_runs, 1);
        // The token-count bit is separate: this run is not a token measurement.
        assert_eq!(totals.measured_runs, 0);
        assert_eq!(totals.input_tokens, None);
        assert_eq!(totals.output_tokens, None);
        // The cost is real and survives.
        assert!((totals.cost_usd.unwrap() - 0.0123).abs() < 1e-9);

        let by_tool = meter.by_tool().unwrap();
        assert_eq!(by_tool.len(), 1);
        assert_eq!(by_tool[0].input_tokens, None);
        assert_eq!(by_tool[0].output_tokens, None);
        assert_eq!(by_tool[0].measured_runs, 0);
        assert_eq!(by_tool[0].reported_runs, 1);
    }

    #[test]
    fn estimate_leg_is_stored_next_to_the_measurement() {
        let (meter, _dir) = meter();
        let rec = UsageRecord::measured("run-1", "acp", "claude", 100, 20, "provider")
            .with_estimate(400, Some((0.003, 0.015)));
        meter.record(&rec).unwrap();

        let rows = meter.recent(1).unwrap();
        assert_eq!(rows[0].input_tokens, Some(100));
        assert_eq!(rows[0].est_input_tokens, Some(100));
        assert_eq!(rows[0].est_output_tokens, Some(25));
        assert!(rows[0].est_cost_usd.unwrap() > 0.0);
    }
}
