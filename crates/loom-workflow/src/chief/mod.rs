//! Chief orchestration core (LOOM_CHIEF_ORCHESTRATION_PROPOSAL.md, P0 slice).
//!
//! Pure-logic pieces that let `loom orchestrate` decompose an ask via a Chief
//! planner harness and fan the resulting tasks out across worker harnesses in
//! isolated git worktrees. No agents are spawned from this module — it is the
//! contract core consumed by the executor layers.

pub mod dag;
pub mod draft;
pub mod harness;
pub mod import;
pub mod manifest;
pub mod plan;
pub mod scheduler;

pub use dag::TaskDag;
pub use draft::draft_plan;
pub use harness::{HarnessRole, HarnessSelection, HarnessSelector};
pub use import::import_plan;
pub use manifest::{WorktreeEntry, WorktreeManifest};
pub use plan::{PlanParser, PlanTask, TaskPlan};
pub use scheduler::{build_dispatch, DispatchEntry};
