//! `loom remote` subcommands — manage SSH remote connections.

use crate::cli::RemoteAction;
use loom_auth::vault::CredentialVault;
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
        RemoteAction::Add { name, host, port, user, key, store_in_vault, vault_password, agent, jump } => {
            let auth = if *agent || key.is_none() {
                SshAuth::Agent
            } else if let Some(k) = key {
                SshAuth::KeyFile(k.clone())
            } else {
                SshAuth::Agent
            };

            // If --store-in-vault, write the key material into the vault
            if *store_in_vault {
                if let Some(ref key_path) = key {
                    let key_material = std::fs::read_to_string(key_path)?;
                    let vault_id = format!("ssh-key-{}", name);
                    let mut vault = CredentialVault::new(loom_dir.to_str().unwrap());
                    let password = vault_password.clone().ok_or_else(|| {
                        anyhow::anyhow!("--vault-password is required when using --store-in-vault")
                    })?;
                    vault.unlock(&password)?;
                    vault.set_credential(&vault_id, &key_material)?;
                    println!("  🔑 Stored SSH key in vault as '{vault_id}'");
                }
            }

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
        RemoteAction::Exec { conn, command, capture_session } => {
            let manager = load_connections(&config_path)?;
            let mut connection = manager.get(conn)
                .ok_or_else(|| anyhow::anyhow!("Remote connection '{}' not found", conn))?
                .clone();
            resolve_vault_key(&mut connection, loom_dir)?;
            if *capture_session {
                let mut cmd = connection.ssh_command(command);
                let output = cmd.output()?;
                let exit_code = output.status.code().unwrap_or(-1);
                let (manifest, messages) = build_remote_session(
                    conn,
                    command,
                    exit_code,
                    &String::from_utf8_lossy(&output.stdout),
                    &String::from_utf8_lossy(&output.stderr),
                );
                capture_remote_session(loom_dir, &manifest, &messages);
                if exit_code != 0 {
                    anyhow::bail!("Remote command exited with status: {exit_code}");
                }
            } else {
                let mut cmd = connection.ssh_command(command);
                let status = cmd.status()?;
                if !status.success() {
                    anyhow::bail!("Remote command exited with status: {}", status);
                }
            }
        }
        RemoteAction::Sync { conn, pull, scp, source, dest } => {
            let manager = load_connections(&config_path)?;
            let mut connection = manager.get(conn)
                .ok_or_else(|| anyhow::anyhow!("Remote connection '{}' not found", conn))?
                .clone();
            resolve_vault_key(&mut connection, loom_dir)?;
            let use_rsync = !scp;
            if *pull {
                sftp::pull(source, dest, &connection, use_rsync)?;
                println!("✓ Synced {} → {} (pull)", source, dest);
            } else {
                sftp::push(source, dest, &connection, use_rsync)?;
                println!("✓ Synced {} → {} (push)", source, dest);
            }
        }
    }
    Ok(())
}

/// If the connection uses KeyPem auth (key stored in vault), resolve
/// the key material from the vault and write a temp file for ssh/scp.
fn resolve_vault_key(conn: &mut SshConnection, loom_dir: &Path) -> anyhow::Result<()> {
    if let SshAuth::KeyPem(ref pem) = conn.auth {
        let temp_key = std::env::temp_dir().join(format!(".loom-ssh-{}", conn.name));
        std::fs::write(&temp_key, pem)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temp_key, std::fs::Permissions::from_mode(0o600))?;
        }
        conn.auth = SshAuth::KeyFile(temp_key.to_string_lossy().to_string());
    } else if let SshAuth::KeyFile(ref path) = conn.auth.clone() {
        // If the path doesn't exist, try resolving from the vault
        if !std::path::Path::new(path).exists() {
            let vault_id = format!("ssh-key-{}", conn.name);
            let vault = CredentialVault::new(loom_dir.to_str().unwrap());
            // Vault must be pre-unlocked via `loom auth validate` or we skip silently
            if let Ok(Some(key_material)) = vault.get_credential(&vault_id) {
                conn.auth = SshAuth::KeyPem(key_material);
                return resolve_vault_key(conn, loom_dir);
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

/// Build the manifest + System/User/Assistant messages for a remote
/// command execution. Pure so the shaping is unit-testable.
pub fn build_remote_session(
    conn: &str,
    command: &[String],
    exit_code: i32,
    stdout_tail: &str,
    stderr_tail: &str,
) -> (loom_core::SessionManifest, Vec<loom_core::Message>) {
    use loom_core::{Message, MessageRole, SessionManifest};
    let command_text = command.join(" ");
    let session_id = format!("remote-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let topic: String = command_text.chars().take(80).collect();
    let now = chrono::Utc::now().to_rfc3339();

    let stdout_text = stdout_tail.trim();
    let stderr_text = stderr_tail.trim();
    let mut assistant = if stdout_text.is_empty() {
        "(remote command produced no output)".to_string()
    } else {
        stdout_text.to_string()
    };
    if !stderr_text.is_empty() {
        assistant.push_str("\n\n--- stderr ---\n");
        assistant.push_str(stderr_text);
    }
    if exit_code != 0 {
        assistant.push_str(&format!("\n\n[exited with status {exit_code}]"));
    }

    let messages = vec![
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: format!(
                "Remote execution\n  connection: {conn}\n  command: {command_text}\n  exit_code: {exit_code}"
            ),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: command_text.clone(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: assistant,
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
    ];

    let tags = vec![
        "remote".to_string(),
        "remote-exec".to_string(),
        format!("conn:{conn}"),
        if exit_code == 0 {
            "completed".to_string()
        } else {
            "failed".to_string()
        },
    ];
    let manifest = SessionManifest {
        session_id,
        source_tool: format!("loom-remote/{conn}"),
        source_version: env!("CARGO_PKG_VERSION").to_string(),
        exported_at: now,
        topic,
        message_count: messages.len() as u32,
        active_skills: vec![],
        active_mcp_servers: vec![],
        working_directory: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default(),
        model_provider: format!("remote:{conn}"),
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Remote execution on '{conn}' (exit={exit_code})"
        )),
        tags,
        summarized_at: None,
        summarized_message_count: None,
    };
    (manifest, messages)
}

/// Persist a captured remote session and index it. Best-effort past
/// storage writes, mirroring the other capture paths.
fn capture_remote_session(
    loom_dir: &Path,
    manifest: &loom_core::SessionManifest,
    messages: &[loom_core::Message],
) {
    let loom_dir_str = loom_dir.to_str().unwrap();
    let store = loom_sessions::store::SessionStore::new(loom_dir_str);
    if let Err(e) = store.save_manifest(manifest) {
        eprintln!("  Warning: could not save remote session: {e}");
        return;
    }
    if let Err(e) = store.replace_messages(&manifest.session_id, messages) {
        eprintln!("  Warning: could not save remote session messages: {e}");
        return;
    }
    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir_str) {
        if let Err(e) = memory.index_session_with_tags(
            &manifest.session_id,
            messages,
            &manifest.topic,
            &manifest.tags,
        ) {
            eprintln!("  Warning: memory index failed: {e}");
        }
    }
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(loom_dir_str) {
        if let Err(e) = kg.extract_from_messages(messages, &manifest.session_id) {
            eprintln!("  Warning: knowledge extraction failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_remote_session;

    #[test]
    fn build_remote_session_shapes_messages_manifest_and_tags() {
        let (manifest, messages) = build_remote_session(
            "prod",
            &["uptime".to_string(), "--pretty".to_string()],
            0,
            "12:00 up 10 days",
            "",
        );
        assert_eq!(messages.len(), 3);
        let roles: Vec<String> =
            messages.iter().map(|m| format!("{:?}", m.role)).collect();
        assert_eq!(roles, vec!["System", "User", "Assistant"]);
        assert!(messages[1].content.contains("uptime"));
        assert!(messages[2].content.contains("up 10 days"));
        assert!(manifest.tags.contains(&"remote-exec".to_string()));
        assert!(manifest.tags.contains(&"conn:prod".to_string()));
        assert!(manifest.tags.contains(&"completed".to_string()));
        assert!(manifest.session_id.starts_with("remote-"));
    }

    #[test]
    fn build_remote_session_records_failures() {
        let (manifest, messages) =
            build_remote_session("prod", &["false".to_string()], 1, "", "boom");
        assert!(manifest.tags.contains(&"failed".to_string()));
        assert!(messages[2].content.contains('1'));
        assert!(messages[2].content.contains("boom"));
    }
}
