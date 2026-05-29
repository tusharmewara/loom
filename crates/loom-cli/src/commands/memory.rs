use crate::cli::MemoryAction;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &MemoryAction) -> anyhow::Result<()> {
    match action {
        MemoryAction::Status { json } => {
            let socket_path = loom_dir.join("loomd.sock");
            if !socket_path.exists() {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "memory.status",
                            "running": false,
                            "error": "daemon not running"
                        }))?
                    );
                } else {
                    println!("loomd is not running — start it with `loom daemon start`");
                }
                return Ok(());
            }

            let req = loom_ipc::IpcRequest::new("memory.status");
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    let data = resp.data.unwrap_or(serde_json::Value::Null);
                    let running = data.get("running").and_then(|v| v.as_bool()).unwrap_or(false);
                    let port = data.get("port").and_then(|v| v.as_u64()).unwrap_or(3111);
                    let viewer_port = data.get("viewer_port").and_then(|v| v.as_u64()).unwrap_or(3113);

                    if *json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": 1,
                                "command": "memory.status",
                                "running": running,
                                "port": port,
                                "viewer_port": viewer_port,
                            }))?
                        );
                    } else {
                        if running {
                            println!("✅ AgentMemory is running");
                            println!("   REST API:  http://localhost:{port}");
                            println!("   Viewer:    http://localhost:{viewer_port}");
                            println!("   Use `loom memory stop` to stop it.");
                        } else {
                            println!("❌ AgentMemory is not running");
                            println!("   Use `loom memory start` to start it.");
                        }
                    }
                    Ok(())
                }
                Ok(resp) => {
                    if *json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": 1,
                                "command": "memory.status",
                                "running": false,
                                "error": resp.error
                            }))?
                        );
                    } else {
                        println!("Daemon responded with status: {}", resp.status);
                    }
                    Ok(())
                }
                Err(e) => {
                    if *json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": 1,
                                "command": "memory.status",
                                "running": false,
                                "error": e.to_string()
                            }))?
                        );
                    } else {
                        println!("Failed to contact daemon: {e}");
                    }
                    Ok(())
                }
            }
        }
        MemoryAction::Start => {
            let socket_path = loom_dir.join("loomd.sock");
            if !socket_path.exists() {
                anyhow::bail!("loomd is not running. Start it with `loom daemon start`");
            }

            let req = loom_ipc::IpcRequest::new("memory.start");
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    let data = resp.data.unwrap_or(serde_json::Value::Null);
                    let started = data.get("started").and_then(|v| v.as_bool()).unwrap_or(false);
                    if started {
                        println!("✅ AgentMemory server started");
                    } else {
                        println!("✓ AgentMemory was already running");
                    }
                    Ok(())
                }
                Ok(resp) => {
                    anyhow::bail!("Daemon returned error: {}", resp.error.unwrap_or_default());
                }
                Err(e) => {
                    anyhow::bail!("Failed to contact daemon: {e}");
                }
            }
        }
        MemoryAction::Stop => {
            let socket_path = loom_dir.join("loomd.sock");
            if !socket_path.exists() {
                anyhow::bail!("loomd is not running. Start it with `loom daemon start`");
            }

            let req = loom_ipc::IpcRequest::new("memory.stop");
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    let data = resp.data.unwrap_or(serde_json::Value::Null);
                    let stopped = data.get("stopped").and_then(|v| v.as_bool()).unwrap_or(false);
                    if stopped {
                        println!("✅ AgentMemory server stopped");
                    } else {
                        println!("✓ AgentMemory was not running");
                    }
                    Ok(())
                }
                Ok(resp) => {
                    anyhow::bail!("Daemon returned error: {}", resp.error.unwrap_or_default());
                }
                Err(e) => {
                    anyhow::bail!("Failed to contact daemon: {e}");
                }
            }
        }
    }
}
