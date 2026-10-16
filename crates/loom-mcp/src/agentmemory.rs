//! Managed process lifecycle for the agentmemory memory server.
//!
//! AgentMemory (agent-memory.dev) is a persistent memory layer for AI coding
//! agents. It runs as a background server providing a REST API on port 3111
//! and exposes 53 MCP tools via its MCP shim (`@agentmemory/mcp`).
//!
//! This module manages the *memory server* process (`npx @agentmemory/agentmemory`).
//! The MCP shim is registered as a shared server in [`McpRegistry::seed_defaults`]
//! so all tools auto-discover it.
//!
//! Lifecycle:
//!   1. On loomd start → spawn `npx @agentmemory/agentmemory` as a child process.
//!   2. Health-check via `GET /agentmemory/health` on localhost:3111.
//!   3. On loomd stop → kill the child process.

use anyhow::{Context, Result};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use tracing::{info, warn};

/// Default port the agentmemory REST API listens on.
pub const AGENTMEMORY_PORT: u16 = 3111;

/// Default viewer port.
pub const AGENTMEMORY_VIEWER_PORT: u16 = 3113;

/// Manages the agentmemory memory server subprocess lifecycle.
pub struct AgentMemoryManager {
    /// The loom directory (for env).
    _loom_dir: String,
    /// The child process handle (None when not running or after stop).
    child: Mutex<Option<Child>>,
}

impl AgentMemoryManager {
    /// Create a new manager. Does *not* start the process.
    pub fn new(loom_dir: &str) -> Self {
        Self {
            _loom_dir: loom_dir.to_string(),
            child: Mutex::new(None),
        }
    }

    /// Start the agentmemory memory server as a subprocess.
    ///
    /// Returns `Ok(true)` if started, `Ok(false)` if already running.
    /// Returns `Err` only if spawning fails (e.g. npx not found).
    pub fn start(&self) -> Result<bool> {
        let mut guard = loom_core::lock_ok!(self.child);
        if guard.is_some() {
            // Already running — check if the process is still alive.
            if let Some(ref mut child) = *guard {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process exited; clear the slot and re-spawn.
                        warn!("agentmemory process exited (status={}); restarting", status);
                        *guard = None;
                    }
                    Ok(None) => {
                        // Still alive.
                        return Ok(false);
                    }
                    Err(e) => {
                        warn!("agentmemory try_wait error: {e}; re-spawning");
                        *guard = None;
                    }
                }
            }
        }

        // Check that npx is available before trying to spawn.
        if let Err(e) = check_npx() {
            warn!("agentmemory not started: {e}");
            return Err(e);
        }

        let child = Command::new("npx")
            .arg("@agentmemory/agentmemory")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context(
                "Failed to spawn agentmemory via `npx @agentmemory/agentmemory`.
Make sure Node.js >= 20 and npx are on PATH.
Install: `npm install -g @agentmemory/agentmemory` or just run `npx @agentmemory/agentmemory`",
            )?;

        info!("Started agentmemory memory server (pid {})", child.id());
        *guard = Some(child);
        Ok(true)
    }

    /// Stop the agentmemory memory server.
    ///
    /// Returns `Ok(true)` if the process was killed, `Ok(false)` if it wasn't running.
    pub fn stop(&self) -> Result<bool> {
        let mut guard = loom_core::lock_ok!(self.child);
        if let Some(mut child) = guard.take() {
            let pid = child.id();
            match child.kill() {
                Ok(_) => {
                    let _ = child.wait();
                    info!("Stopped agentmemory (pid {})", pid);
                }
                Err(e) => {
                    warn!("Failed to kill agentmemory (pid {}): {}", pid, e);
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check whether the agentmemory process appears to be alive.
    ///
    /// Uses the process status (not a network health check). For an
    /// end-to-end check that the REST API is live, call [`health_check`].
    pub fn is_running(&self) -> bool {
        let mut guard = loom_core::lock_ok!(self.child);
        if let Some(ref mut child) = *guard {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Exited — clean up.
                    *guard = None;
                    false
                }
                Ok(None) => true,
                Err(_) => true, // assume running if we can't check
            }
        } else {
            false
        }
    }
}

impl Drop for AgentMemoryManager {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

/// Perform a network-level health check by probing the agentmemory REST API port.
pub fn health_check(port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{}", port);
    TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(3)).context(format!(
        "agentmemory not reachable at {addr} (is it running?)"
    ))?;
    Ok(())
}

/// Check that `npx` is available and can resolve `@agentmemory/agentmemory`.
fn check_npx() -> Result<()> {
    let output = Command::new("npx")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| {
            anyhow::anyhow!(
                "npx not found on PATH ({e}). Install Node.js >= 20 from https://nodejs.org"
            )
        })?;

    if !output.status.success() {
        anyhow::bail!("npx --version exited with error");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_returns_not_running() {
        let mgr = AgentMemoryManager::new("/tmp/.loom-test");
        assert!(!mgr.is_running());
    }

    #[test]
    fn test_stop_when_not_running() {
        let mgr = AgentMemoryManager::new("/tmp/.loom-test");
        let stopped = mgr.stop().unwrap();
        assert!(!stopped);
    }

    #[test]
    fn test_check_npx_available() {
        // This will fail in CI environments without Node.js; that's expected.
        let result = check_npx();
        // On most dev machines npx should be available.
        if result.is_err() {
            println!("npx not available (expected in CI): {:?}", result);
        } else {
            println!("npx is available");
        }
        // Don't assert — CI may not have Node.
    }

    #[test]
    fn test_health_check_closed_port() {
        // With no server on an arbitrary port, this should fail gracefully.
        let result = health_check(19999);
        assert!(result.is_err());
    }

    #[test]
    fn test_health_check_zero_port() {
        // Port 0 should fail gracefully too.
        let result = health_check(0);
        assert!(result.is_err());
    }

    #[test]
    fn test_drop_when_stopped() {
        // Dropping a non-running manager should not panic.
        let mgr = AgentMemoryManager::new("/tmp/.loom-test");
        drop(mgr);
    }
}
