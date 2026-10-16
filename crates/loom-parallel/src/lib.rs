//! `loom-parallel` — Concurrent agent execution and git worktree isolation.
//!
//! Provides [`AgentPool`] for running multiple agent tools concurrently
//! (with configurable concurrency limits) and [`WorktreeManager`] for
//! isolating parallel agents in separate git worktrees.
//!
//! Independent by design: no `loom-core` dependency. This crate is a pure
//! stdlib library (no paths, locks needing poisoning recovery, time,
//! or SQLite), so adopting loom-core would be coupling without benefit.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use loom_parallel::{AgentPool, AgentTask};
//!
//! # async fn example() {
//! let pool = AgentPool::new(4);
//! let results = pool
//!     .execute_all(vec![
//!         AgentTask::new("opencode", "/usr/local/bin/opencode")
//!             .args(vec!["--message".into(), "fix bug".into()])
//!             .env("LOOM_ENABLED", "1"),
//!     ])
//!     .await;
//! for r in &results {
//!     println!("{}: {}", r.name, if r.success() { "OK" } else { "FAILED" });
//! }
//! # }
//! ```

mod pool;
mod worktree;

pub use pool::{AgentPool, AgentResult, AgentTask};
pub use worktree::WorktreeManager;
