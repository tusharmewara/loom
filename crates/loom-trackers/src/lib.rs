//! `loom-trackers` — Issue tracker integration for Loom.
//!
//! Provides a generic [`IssueTracker`] trait and backends for GitHub Issues,
//! Linear, and more.

pub mod github;
pub mod linear;
pub mod models;
mod tracker;

pub use tracker::IssueTracker;
