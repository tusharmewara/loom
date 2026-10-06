use crate::cli::AcpAction;
use crate::ipc_client;
use loom_ipc::IpcRequest;
use serde_json::json;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &AcpAction) -> anyhow::Result<()> {
    match action {
        AcpAction::Status { json: json_output } => handle_status(loom_dir, *json_output).await,
        AcpAction::Start => handle_start(loom_dir).await,
        AcpAction::Stop => handle_stop(loom_dir).await,
        AcpAction::Show { json: json_output } => handle_show(loom_dir, *json_output).await,
        AcpAction::Agents { json: json_output } => handle_agents(loom_dir, *json_output).await,
        AcpAction::RunStatus { run_id, json: json_output } => {
            handle_run_status(loom_dir, run_id, *json_output).await
        }
        AcpAction::Run {
            program,
            prompt,
            cwd,
            timeout_secs,
            runtime,
            args,
            json: json_output,
        } => {
            crate::commands::acp_run::run_acp_prompt(
                loom_dir,
                program,
                args,
                prompt,
                cwd.as_deref(),
                *timeout_secs,
                *json_output,
                runtime.clone(),
            )
            .await
        }
    }
}

fn get_data(resp: &loom_ipc::IpcResponse) -> serde_json::Value {
    resp.data.clone().unwrap_or(serde_json::Value::Null)
}

async fn handle_status(loom_dir: &Path, json_output: bool) -> anyhow::Result<()> {
    let req = IpcRequest::new("acp.status").with_params(json!({}));
    let resp = ipc_client::send_daemon_command(loom_dir, req).await?;

    if resp.status == "error" {
        let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("ACP status check failed: {err}");
    }

    let data = get_data(&resp);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        let running = data.get("running").and_then(|v| v.as_bool()).unwrap_or(false);
        if running {
            let addr = data
                .get("listen_addr")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let pub_key = data
                .get("public_key")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let uptime = data.get("uptime_secs").and_then(|v| v.as_u64()).unwrap_or(0);
            let handler_count = data
                .get("handlers_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let has_default = data
                .get("has_default_handler")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            println!("ACP Dispatch Server: RUNNING");
            println!("  Listen Address:   {addr}");
            let key_preview = if pub_key.len() > 16 {
                format!(
                    "{}..{}",
                    &pub_key[..8],
                    &pub_key[pub_key.len().saturating_sub(8)..]
                )
            } else {
                pub_key.to_string()
            };
            println!("  Public Key:        {key_preview}");
            println!("  Uptime:            {uptime}s");
            println!("  Handlers:          {handler_count} registered");
            println!("  Default Handler:   {has_default}");
        } else {
            let msg = data
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("ACP dispatch server not running");
            println!("ACP Dispatch Server: STOPPED");
            println!("  {msg}");
            println!("  (Auto-starts with loomd)");
        }
    }
    Ok(())
}

async fn handle_start(loom_dir: &Path) -> anyhow::Result<()> {
    let req = IpcRequest::new("acp.start");
    let resp = ipc_client::send_daemon_command(loom_dir, req).await?;

    if resp.status == "error" {
        let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("ACP start failed: {err}");
    }

    println!("ACP dispatch server started");
    Ok(())
}

async fn handle_stop(loom_dir: &Path) -> anyhow::Result<()> {
    let req = IpcRequest::new("acp.stop");
    let resp = ipc_client::send_daemon_command(loom_dir, req).await?;

    if resp.status == "error" {
        let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("ACP stop failed: {err}");
    }

    let data = get_data(&resp);
    let stopped = data.get("stopped").and_then(|v| v.as_bool()).unwrap_or(false);
    if stopped {
        println!("ACP dispatch server stopped");
    } else {
        let msg = data
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        println!("ACP dispatch server: {msg}");
    }
    Ok(())
}

async fn handle_show(loom_dir: &Path, json_output: bool) -> anyhow::Result<()> {    let req = IpcRequest::new("acp.status").with_params(json!({}));
    let resp = ipc_client::send_daemon_command(loom_dir, req).await?;

    if resp.status == "error" {
        let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("ACP show failed: {err}");
    }

    let data = get_data(&resp);

    // Show full server info including handler list
    if json_output {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        let running = data.get("running").and_then(|v| v.as_bool()).unwrap_or(false);
        if running {
            let addr = data
                .get("listen_addr")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let pub_key = data
                .get("public_key")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let uptime = data.get("uptime_secs").and_then(|v| v.as_u64()).unwrap_or(0);
            let handler_count = data
                .get("handlers_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let has_default = data
                .get("has_default_handler")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let handlers: Vec<String> = data
                .get("handlers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            println!("ACP Dispatch Server Details");
            println!("══════════════════════════════");
            println!("  Status:           RUNNING");
            println!("  Listen Address:   {addr}");
            println!("  Public Key:       {pub_key}");
            println!("  Uptime:           {uptime}s");
            println!("  Handlers:         {handler_count}");
            if !handlers.is_empty() {
                for h in &handlers {
                    println!("    - {h}");
                }
            }
            println!("  Default Handler:  {has_default}");
        } else {
            println!("ACP Dispatch Server: STOPPED");
        }
    }
    Ok(())
}

/// List launchable ACP agents: daemon-registered handlers merged with
/// local `config/agents.toml` entries (what `loom add-acp-agent`
/// writes). The daemon half is best-effort so the command still works
/// while loomd is stopped.
async fn handle_agents(loom_dir: &Path, json_output: bool) -> anyhow::Result<()> {
    let mut agents_toml: Vec<String> = Vec::new();
    let agents_path = loom_dir.join("config").join("agents.toml");
    if agents_path.exists() {
        if let Ok(raw) = std::fs::read_to_string(&agents_path) {
            if let Ok(table) = toml::from_str::<toml::Table>(&raw) {
                let mut keys: Vec<String> = table.keys().cloned().collect();
                keys.sort();
                agents_toml = keys;
            }
        }
    }

    let mut handlers: Vec<String> = Vec::new();
    let mut has_default_handler = false;
    let mut daemon_reachable = false;
    let req = IpcRequest::new("acp.status").with_params(json!({}));
    if let Ok(resp) = ipc_client::send_daemon_command(loom_dir, req).await {
        if resp.status != "error" {
            daemon_reachable = true;
            let data = get_data(&resp);
            handlers = data
                .get("handlers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::trim))
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            has_default_handler = data
                .get("has_default_handler")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "running": daemon_reachable,
                "handlers": handlers,
                "agents_toml": agents_toml,
                "has_default_handler": has_default_handler,
            }))?
        );
        return Ok(());
    }

    let mut merged = handlers.clone();
    for agent in &agents_toml {
        if !merged.iter().any(|a| a == agent) {
            merged.push(agent.clone());
        }
    }
    merged.sort();
    if merged.is_empty() {
        println!("No ACP agents found.");
        println!("Register one with: loom add-acp-agent <agent_id>");
    } else {
        println!("Launchable ACP agents:");
        for agent in &merged {
            println!("  - {agent}");
        }
    }
    Ok(())
}

/// Show a tracked ACP run's status (exit code and captured session,
// recorded by the daemon when a tracked `acp.dispatch` completes).
async fn handle_run_status(
    loom_dir: &Path,
    run_id: &str,
    json_output: bool,
) -> anyhow::Result<()> {
    if run_id.trim().is_empty() {
        anyhow::bail!("run_id must not be empty");
    }
    let req = IpcRequest::new("run.status").with_params(json!({ "run_id": run_id }));
    let resp = ipc_client::send_daemon_command(loom_dir, req).await?;

    if resp.status == "error" {
        let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
        anyhow::bail!("run status check failed: {err}");
    }

    let data = get_data(&resp);
    if json_output {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let status = data.get("status").and_then(|v| v.as_str()).unwrap_or("unknown");
    println!("Run {}", data.get("run_id").and_then(|v| v.as_str()).unwrap_or(run_id));
    println!("  Status:  {status}");
    println!(
        "  Task:    {}",
        data.get("task_id").and_then(|v| v.as_str()).unwrap_or("unknown")
    );
    println!(
        "  Agent:   {}",
        data.get("agent_id").and_then(|v| v.as_str()).unwrap_or("unknown")
    );
    match data.get("exit_code").and_then(|v| v.as_i64()) {
        Some(code) => println!("  Exit:    {code}"),
        None => println!("  Exit:    (still running)"),
    }
    match data.get("session_id").and_then(|v| v.as_str()) {
        Some(session) => println!("  Session: {session}"),
        None => println!("  Session: (none captured yet)"),
    }
    Ok(())
}
