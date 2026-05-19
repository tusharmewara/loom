//! Shared constants for Loom ecosystem paths.

pub const ECOSYSTEM_VERSION: &str = "2.0.0";
pub const AGENTS_DIR: &str = ".loom";
pub const SKILLS_DIR: &str = "skills";
pub const MCP_DIR: &str = "mcp";
pub const SESSIONS_DIR: &str = "sessions";
pub const AUTH_DIR: &str = "auth";
pub const HOOKS_DIR: &str = "hooks";
pub const TOOLS_DIR: &str = "tools";
pub const CACHE_DIR: &str = ".cache";
pub const CONFIG_DIR: &str = "config";

pub const DAEMON_DEFAULT_IDLE_TIMEOUT_MS: u64 = 300_000; // 5 min
pub const MCP_DEFAULT_HEALTH_INTERVAL_MS: u64 = 300_000; // 5 min
pub const HOOK_DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const AUDIT_LOG_MAX_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
pub const SESSION_COMPRESSION_THRESHOLD: usize = 20;
pub const SESSION_FULL_FIDELITY_COUNT: usize = 10;
pub const VAULT_MAX_UNLOCK_ATTEMPTS: u32 = 5;
pub const VAULT_UNLOCK_WINDOW_MS: u64 = 60_000; // 1 min
pub const JOURNAL_ROTATION_LINES: usize = 2000;
