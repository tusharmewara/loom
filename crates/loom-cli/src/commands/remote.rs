//! `loom remote` subcommands — manage SSH remote connections.

use crate::cli::RemoteAction;
use loom_remote::connection::{SshConnection, SshAuth, SshConnectionManager};
use loom_remote::sftp;
use std::collections::HashMap;
use std::path::Path;

/// Path within the loom dir where remote connections are stored.
const REMOTE_CONFIG: &str = "config/remote.toml";

pub async fn run(loom_dir: &Path, action: &RemoteAction) -> anyhow::Result<()> {
    let config_path = loom_dir.join(REMOTE_CONFIG);

    match action {
        RemoteAction::List { json } => {
            let manager = load_connections(&config_path)?;
            let connections = manager.list();
            if *json {
                let entries: Vec<serde_json::Value> = connections.iter().map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "host": c.host,
                        "port": c.port,
                        "user": c.user,
                        "jump_host": c.jump_host,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "remote.list",
                    "count": entries.len(),
                    "connections": entries,
                }))?);
            } else {
                if connections.is_empty() {
                    println!("No remote connections configured.");
                    println!("  Add one with: loom remote add <name> --host <host>");
                } else {
                    println!("Remote connections:");
                    for c in connections {
                        let auth = match c.auth {
                            SshAuth::Agent => "ssh-agent",
                            SshAuth::Password(_) => "password",
                            SshAuth::KeyFile(_) => "key file",
                            SshAuth::KeyPem(_) => "key (vault)",
                        };
                        let jump = c.jump_host.as_deref().unwrap_or("-");
                        println!("  {}  {}@{}:{}  auth={}  jump={}", c.name, c.user, c.host, c.port, auth, jump);
                    }
                }
            }
        }
        RemoteAction::Add { name, host, port, user, key, agent, jump } => {
            let auth = if *agent || key.is_none() {
                SshAuth::Agent
            } else if let Some(k) = key {
                SshAuth::KeyFile(k.clone())
            } else {
                SshAuth::Agent
            };
            let conn = SshConnection {
                name: name.clone(),
                host: host.clone(),
                port: *port,
                user: user.clone(),
                auth,
                jump_host: jump.clone(),
            };
            let mut manager = load_connections(&config_path)?;
            manager.add(conn);
            save_connections(&config_path, &manager)?;
            println!("✓ Added remote connection '{}' ({}@{}:{})", name, user, host, port);
        }
        RemoteAction::Remove { name } => {
            let mut manager = load_connections(&config_path)?;
            if manager.remove(name) {
                save_connections(&config_path, &manager)?;
                println!("✓ Removed remote connection '{}'", name);
            } else {
                anyhow::bail!("Remote connection '{}' not found", name);
            }
        }
        RemoteAction::Exec { conn, command } => {
            let manager = load_connections(&config_path)?;
            let connection = manager.get(conn)
                .ok_or_else(|| anyhow::anyhow!("Remote connection '{}' not found", conn))?;
            let mut cmd = connection.ssh_command(command);
            let status = cmd.status()?;
            if !status.success() {
                anyhow::bail!("Remote command exited with status: {}", status);
            }
        }
        RemoteAction::Sync { conn, pull, scp, source, dest } => {
            let manager = load_connections(&config_path)?;
            let connection = manager.get(conn)
                .ok_or_else(|| anyhow::anyhow!("Remote connection '{}' not found", conn))?;
            let use_rsync = !scp;
            if *pull {
                sftp::pull(source, dest, connection, use_rsync)?;
                println!("✓ Synced {} → {} (pull)", source, dest);
            } else {
                sftp::push(source, dest, connection, use_rsync)?;
                println!("✓ Synced {} → {} (push)", source, dest);
            }
        }
    }
    Ok(())
}

fn load_connections(path: &Path) -> anyhow::Result<SshConnectionManager> {
    let mut manager = SshConnectionManager::new();
    if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        let entries: HashMap<String, serde_json::Value> =
            toml::from_str(&raw).unwrap_or_default();
        for (name, val) in entries {
            let host = val.get("host").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let port = val.get("port").and_then(|v| v.as_u64()).unwrap_or(22) as u16;
            let user = val.get("user").and_then(|v| v.as_str()).unwrap_or("root").to_string();
            let auth = match val.get("auth").and_then(|v| v.as_str()) {
                Some("agent") => SshAuth::Agent,
                Some("key_file") => SshAuth::KeyFile(
                    val.get("key_path").and_then(|v| v.as_str()).unwrap_or("").to_string()
                ),
                Some("key_pem") => SshAuth::KeyPem(
                    val.get("key_data").and_then(|v| v.as_str()).unwrap_or("").to_string()
                ),
                _ => SshAuth::Agent,
            };
            let jump_host = val.get("jump_host").and_then(|v| v.as_str()).map(|s| s.to_string());
            manager.add(SshConnection {
                name,
                host,
                port,
                user,
                auth,
                jump_host,
            });
        }
    }
    Ok(manager)
}

fn save_connections(path: &Path, manager: &SshConnectionManager) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut entries = HashMap::new();
    for conn in manager.list() {
        let (auth_type, key_path, key_data) = match &conn.auth {
            SshAuth::Agent => ("agent", None, None),
            SshAuth::Password(_) => ("password", None, None),
            SshAuth::KeyFile(p) => ("key_file", Some(p.clone()), None),
            SshAuth::KeyPem(p) => ("key_pem", None, Some(p.clone())),
        };
        let mut entry = serde_json::json!({
            "host": conn.host,
            "port": conn.port,
            "user": conn.user,
            "auth": auth_type,
        });
        if let Some(kp) = key_path {
            entry["key_path"] = serde_json::Value::String(kp);
        }
        if let Some(kd) = key_data {
            entry["key_data"] = serde_json::Value::String(kd);
        }
        if let Some(jh) = &conn.jump_host {
            entry["jump_host"] = serde_json::Value::String(jh.clone());
        }
        entries.insert(conn.name.clone(), entry);
    }
    let toml_str = toml::to_string_pretty(&entries)?;
    std::fs::write(path, toml_str)?;
    Ok(())
}
