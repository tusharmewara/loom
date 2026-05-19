//! Tracing setup utilities.
//!
//! Re-exports and convenience wrappers for the `tracing` and `tracing-subscriber` crates.
//! Provides JSON formatting for production and pretty-print for development.

/// Initialize tracing with default (human-readable) format.
pub use tracing_subscriber;

/// Initialize tracing with JSON output format.
pub fn init_json() {
    tracing_subscriber::fmt()
        .json()
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_level(true)
        .init();
}

/// Initialize tracing with pretty-print format.
pub fn init_pretty() {
    tracing_subscriber::fmt()
        .pretty()
        .with_target(true)
        .with_thread_ids(true)
        .init();
}
