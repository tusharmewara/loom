//! `loom-remote` — SSH remote development for Loom.
//!
//! Provides SSH connection management, SFTP file sync, and remote PTY
//! execution, integrated with the Loom credential vault for secure key
//! storage.
//!
//! ## Usage
//!
//! ```bash
//! loom remote add my-server --host server.com --user dev
//! loom remote connections list
//! loom remote exec my-server "ls -la"
//! loom remote sync --push ./dist/ my-server:/var/www/
//! loom exec --remote my-server -- goose "analyze logs"
//! ```

mod auth;
pub mod connection;
pub mod sftp;

pub use auth::{load_ssh_key, SshKeySource};
pub use connection::{SshAuth, SshConnection, SshConnectionManager};
