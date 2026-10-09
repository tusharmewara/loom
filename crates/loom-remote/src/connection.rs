//! SSH connection management.

use std::collections::HashMap;
use std::process::Command as StdCommand;

/// Authentication method for an SSH connection.
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// Use ssh-agent (default).
    Agent,
    /// Password-based auth (not recommended; use keys).
    Password(String),
    /// Key file on disk.
    KeyFile(String),
    /// Key from vault (PEM string).
    KeyPem(String),
}

/// A stored SSH connection configuration.
#[derive(Debug, Clone)]
pub struct SshConnection {
    /// User-defined name for this connection.
    pub name: String,
    /// Remote hostname or IP.
    pub host: String,
    /// SSH port (default 22).
    pub port: u16,
    /// Remote username.
    pub user: String,
    /// Auth method.
    pub auth: SshAuth,
    /// Optional jump host.
    pub jump_host: Option<String>,
}

impl SshConnection {
    /// Build an SSH command that connects to this host.
    pub fn ssh_command(&self, remote_args: &[String]) -> StdCommand {
        let mut cmd = StdCommand::new("ssh");
        cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
        cmd.arg("-o").arg("UserKnownHostsFile=/dev/null");
        cmd.arg("-p").arg(self.port.to_string());

        // Add key if file-based
        if let SshAuth::KeyFile(ref path) = self.auth {
            cmd.arg("-i").arg(path);
        }

        // Jump host
        if let Some(ref jump) = self.jump_host {
            cmd.arg("-J").arg(jump);
        }

        cmd.arg(format!("{}@{}", self.user, self.host));
        cmd.args(remote_args);
        cmd
    }

    /// Build SCP-style target string (user@host:path).
    pub fn scp_target(&self, remote_path: &str) -> String {
        format!("{}@{}:{}", self.user, self.host, remote_path)
    }

    /// Build a simple `ssh user@host` string for display.
    pub fn display(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }

    /// Attempt to verify the connection by running a remote command.
    pub fn check(&self) -> Result<(), String> {
        let output = self
            .ssh_command(&[String::from("echo"), String::from("ok")])
            .output()
            .map_err(|e| format!("Failed to spawn ssh: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("SSH check failed: {}", stderr.trim()))
        }
    }
}

/// Manages a collection of SSH connections.
#[derive(Debug, Default)]
pub struct SshConnectionManager {
    connections: HashMap<String, SshConnection>,
}

impl SshConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    pub fn add(&mut self, conn: SshConnection) {
        self.connections.insert(conn.name.clone(), conn);
    }

    pub fn get(&self, name: &str) -> Option<&SshConnection> {
        self.connections.get(name)
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.connections.remove(name).is_some()
    }

    pub fn list(&self) -> Vec<&SshConnection> {
        let mut vec: Vec<&SshConnection> = self.connections.values().collect();
        vec.sort_by_key(|c| &c.name);
        vec
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }
}
