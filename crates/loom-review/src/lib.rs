//! `loom-review` — Verified Review Engine for Loom.
//!
//! When a coding-agent session ends, models from other labs re-do the
//! important part blind; a sandbox labels every finding with what it
//! established (`proven` / `suite_green` / `unverified` / `could_not_run` /
//! `refuted`); one fix PR carries only verified patches; a lesson is written
//! from every proven finding and injected into the next session touching that
//! file.
//!
//! State lives under `~/.loom/review/runs/<id>/`: `plan.json`,
//! `findings.jsonl`, `verify.jsonl`, `report.md`.
//!
//! MVP scope (roadmap pillar 5):
//!   * `loom review run <session-id>` — plan extraction → cross-family
//!     reviewer seats over ACP → findings → dedup → worktree evidence
//!     runner → labels + report.md
//!   * `loom review status | findings | fix-pr | scoreboard`
//!   * Proven-finding lessons via loom-hindsight / --inject-knowledge
//!
//! Not in MVP: adversary seats, container verification, calibration-based
//! seating, write-back automation, perf lens.

mod types;
mod plan;
mod runner;
mod dedup;
mod report;
mod state;

pub use types::*;
pub use plan::ReviewPlan;
pub use runner::EvidenceRunner;
pub use dedup::dedup_findings;
pub use report::write_report;
pub use state::ReviewStore;
