use crate::cli::MemoryAction;
use loom_hindsight::Hindsight;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &MemoryAction) -> anyhow::Result<()> {
    match action {
        MemoryAction::Status { json } => handle_status(loom_dir, *json).await,
        MemoryAction::Start => handle_start(loom_dir).await,
        MemoryAction::Stop => handle_stop(loom_dir).await,
        MemoryAction::Retain {
            fact,
            r#type,
            source,
            importance,
        } => handle_retain(loom_dir, fact, r#type, source, *importance),
        MemoryAction::Recall { query, limit, json } => {
            handle_recall(loom_dir, query, *limit, *json)
        }
        MemoryAction::Reflect { question } => handle_reflect(loom_dir, question),
    }
}

// ─── AgentMemory helpers ─────────────────────────────────────────────

async fn handle_status(loom_dir: &Path, json: bool) -> anyhow::Result<()> {
    let socket_path = loom_dir.join("loomd.sock");
    if !socket_path.exists() {
        if json {
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
            let running = data
                .get("running")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let port = data.get("port").and_then(|v| v.as_u64()).unwrap_or(3111);
            let viewer_port = data
                .get("viewer_port")
                .and_then(|v| v.as_u64())
                .unwrap_or(3113);

            if json {
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
            if json {
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
            if json {
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

async fn handle_start(loom_dir: &Path) -> anyhow::Result<()> {
    let socket_path = loom_dir.join("loomd.sock");
    if !socket_path.exists() {
        anyhow::bail!("loomd is not running. Start it with `loom daemon start`");
    }

    let req = loom_ipc::IpcRequest::new("memory.start");
    match crate::ipc_client::send_daemon_command(loom_dir, req).await {
        Ok(resp) if resp.status == "ok" => {
            let data = resp.data.unwrap_or(serde_json::Value::Null);
            let started = data
                .get("started")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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

async fn handle_stop(loom_dir: &Path) -> anyhow::Result<()> {
    let socket_path = loom_dir.join("loomd.sock");
    if !socket_path.exists() {
        anyhow::bail!("loomd is not running. Start it with `loom daemon start`");
    }

    let req = loom_ipc::IpcRequest::new("memory.stop");
    match crate::ipc_client::send_daemon_command(loom_dir, req).await {
        Ok(resp) if resp.status == "ok" => {
            let data = resp.data.unwrap_or(serde_json::Value::Null);
            let stopped = data
                .get("stopped")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
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

// ─── Hindsight helpers ───────────────────────────────────────────────

fn hindsight_path(loom_dir: &Path) -> std::path::PathBuf {
    loom_dir.join("hindsight.db")
}

fn open_hindsight(loom_dir: &Path) -> anyhow::Result<Hindsight> {
    let path = hindsight_path(loom_dir);
    Hindsight::open(&path)
}

fn handle_retain(
    loom_dir: &Path,
    fact: &str,
    fact_type: &str,
    source: &str,
    importance: f64,
) -> anyhow::Result<()> {
    let h = open_hindsight(loom_dir)?;
    let id = h.retain(fact, fact_type, source, importance)?;
    println!("✅ Fact retained (id: {id})");
    Ok(())
}

fn handle_recall(loom_dir: &Path, query: &str, limit: usize, json: bool) -> anyhow::Result<()> {
    let h = open_hindsight(loom_dir)?;
    let results = h.recall(query, limit)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "query": query,
                "count": results.len(),
                "results": results,
            }))?
        );
    } else {
        if results.is_empty() {
            println!("No matching facts found.");
            return Ok(());
        }
        println!("Found {} relevant fact(s):\n", results.len());
        for (i, fm) in results.iter().enumerate() {
            println!(
                "{}. [{:.2}] {} [{}] (from: {})",
                i + 1,
                fm.score,
                fm.fact.content,
                fm.fact.fact_type,
                fm.fact.source,
            );
        }
    }
    Ok(())
}

fn handle_reflect(loom_dir: &Path, question: &str) -> anyhow::Result<()> {
    let h = open_hindsight(loom_dir)?;
    let reflection = h.reflect(question)?;
    println!("{reflection}");
    Ok(())
}
