//! `loom-trackers` — Issue tracker integration for Loom.
//!
//! Provides a generic [`IssueTracker`] trait and backends for GitHub Issues,
//! Linear, Jira, and more.

pub mod github;
pub mod gitlab;
pub mod jira;
pub mod linear;
pub mod models;
mod tracker;

pub use github::CiStatus;
pub use tracker::IssueTracker;
