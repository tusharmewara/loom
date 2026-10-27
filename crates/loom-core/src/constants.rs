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
/// Env var read by loomd to configure the idle-shutdown timeout in
/// milliseconds. `0` disables idle shutdown entirely — the installed
/// service unit sets it to 0 so the supervisor owns the daemon lifecycle
/// instead of the daemon exiting (and being restarted) on its own.
pub const DAEMON_IDLE_TIMEOUT_ENV: &str = "LOOM_IDLE_TIMEOUT_MS";
pub const MCP_DEFAULT_HEALTH_INTERVAL_MS: u64 = 300_000; // 5 min
pub const HOOK_DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const AUDIT_LOG_MAX_SIZE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB
pub const SESSION_COMPRESSION_THRESHOLD: usize = 20;
pub const SESSION_FULL_FIDELITY_COUNT: usize = 10;
pub const VAULT_MAX_UNLOCK_ATTEMPTS: u32 = 5;
pub const VAULT_UNLOCK_WINDOW_MS: u64 = 60_000; // 1 min
pub const JOURNAL_ROTATION_LINES: usize = 2000;

/// Ed25519 public key that verifies Loom license keys. The matching private
/// key lives offline on the maintainer machine (`~/.loom-admin/license.key`,
/// mode 0600) — see `docs/decisions/0004-license-mint-custody.md`. This is the
/// only non-self-hostable check in the license gate.
pub const LICENSE_VERIFY_PUBLIC_KEY: &str =
    "4ea0613e45f74574c22bdd1c4a8a14e35f26ae1208d5fa2f0579c18c0ee53da0";

/// The one refusal text every license-gated (Team) surface returns. Shared so
/// the CLI, the daemon and the doctor cannot drift.
pub const LICENSE_REQUIRED_MSG: &str =
    "requires a Loom Team license — run `loom license activate`";
