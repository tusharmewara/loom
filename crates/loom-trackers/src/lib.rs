//! `loom-trackers` — Issue tracker integration for Loom.
//!
//! Provides a generic [`IssueTracker`] trait and backends for GitHub Issues
//! (and in the future GitLab, Linear, Jira, etc.).

pub mod github;
pub mod models;
mod tracker;

pub use tracker::IssueTracker;
