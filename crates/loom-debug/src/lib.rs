//! `loom-debug` — Debug Adapter Protocol integration for Loom.
//!
//! Provides DAP client functionality: debug adapter discovery, stdio transport,
//! and common operations (launch, attach, breakpoints, step, evaluate, etc.).
//!
//! ## CLI Usage
//!
//! ```bash
//! loom debug adapters list
//! loom debug launch --program ./target/debug/myapp --language rust
//! loom debug attach --pid 12345 --language go
//! loom debug breakpoints set src/main.rs:42
//! loom debug step over
//! loom debug evaluate "x->data"
//! loom debug stack
//! ```

pub mod client;
pub mod discover;
pub mod transport;

pub use client::{DapClient, DebugAdapterType};
pub use discover::{discover_adapters, resolve_for_language, DebugAdapter};
pub use transport::DapTransport;
