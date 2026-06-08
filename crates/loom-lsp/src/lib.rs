//! `loom-lsp` — Language Server Protocol integration for Loom.
//!
//! Provides LSP client functionality: server discovery, stdio transport,
//! and common operations (rename, find references, code actions, goto
//! definition, document symbols).
//!
//! ## CLI Usage
//!
//! ```bash
//! loom lsp servers list
//! loom lsp rename <old> <new> --path <file>
//! loom lsp find-references --path <file> --symbol <name>
//! loom lsp code-action --path <file> --range <start>:<end>
//! loom lsp goto-definition --path <file> --line <n> --column <n>
//! loom lsp document-symbols --path <file>
//! ```

pub mod client;
pub mod discover;
pub mod transport;

pub use client::LspClient;
pub use discover::{LanguageServer, discover_servers};
pub use transport::LspTransport;
