//! `loom debug` subcommands — Debug Adapter Protocol integration.

use crate::cli::DebugAction;
use loom_debug::client::DapClient;
use loom_debug::discover::{discover_adapters, resolve_for_language};

pub async fn run(action: &DebugAction) -> anyhow::Result<()> {
    match action {
        DebugAction::Adapters { json } => {
            let adapters = discover_adapters();
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "debug.adapters",
                    "count": adapters.len(),
                    "adapters": adapters,
                }))?);
            } else {
                if adapters.is_empty() {
                    println!("No debug adapters found on PATH.");
                    println!("  Install one: lldb-dap (Rust/C/C++), dlv (Go), debugpy (Python)");
                } else {
                    println!("Available debug adapters:");
                    for a in &adapters {
                        let langs = a.languages.join(", ");
                        println!("  {}  ({})  — {}", a.command, langs, a.name);
                    }
                }
            }
        }
        DebugAction::Launch { program, language, args } => {
            let adapter = resolve_for_language(language)
                .ok_or_else(|| anyhow::anyhow!("No debug adapter found for language '{language}'"))?;
            println!("  Using debug adapter: {} ({})", adapter.command, adapter.name);

            let mut client = DapClient::connect_with(&adapter.command, &adapter.args)?;
            client.initialize("loom", &adapter.adapter_type)?;

            let launch_config = serde_json::json!({
                "program": program,
                "args": args,
                "cwd": std::env::current_dir()?.display().to_string(),
                "stopOnEntry": false,
            });
            client.launch(launch_config)?;
            println!("✓ Launched debug session for '{}'", program);

            // Wait for first break
            let stopped = client.wait_for_stopped()?;
            println!("  Stopped: {:?}", stopped);

            // Show initial stack trace
            let thread_id = stopped["body"]["threadId"].as_u64().unwrap_or(1);
            let frames = client.stack_trace(thread_id, None, None)?;
            println!("  Stack frames ({}):", frames.len());
            for (i, frame) in frames.iter().enumerate().take(10) {
                let name = frame["name"].as_str().unwrap_or("<unknown>");
                let source = frame["source"]["path"].as_str().unwrap_or("<unknown>");
                let line = frame["line"].as_u64().unwrap_or(0);
                println!("    #{} {} ({}:{})", i, name, source, line);
            }

            client.disconnect()?;
            println!("✓ Debug session ended");
        }
        DebugAction::Attach { pid, language } => {
            let adapter = resolve_for_language(language)
                .ok_or_else(|| anyhow::anyhow!("No debug adapter found for language '{language}'"))?;
            println!("  Using debug adapter: {} ({})", adapter.command, adapter.name);

            let mut client = DapClient::connect_with(&adapter.command, &adapter.args)?;
            client.initialize("loom", &adapter.adapter_type)?;

            let attach_config = serde_json::json!({
                "processId": pid,
            });
            client.attach(attach_config)?;
            println!("✓ Attached to process {pid}");

            client.configuration_done()?;
            println!("✓ Debug session ready");

            client.disconnect()?;
        }
        DebugAction::Breakpoints { file: _, line: _ } => {
            let adapter = resolve_for_language("rust")
                .or_else(|| resolve_for_language("python"))
                .or_else(|| resolve_for_language("go"))
                .ok_or_else(|| anyhow::anyhow!("No debug adapter found. Install lldb-dap, dlv, or debugpy."))?;

            // We need a running session to set breakpoints
            anyhow::bail!(
                "Breakpoints can only be set in an active debug session. \
                 Use 'loom debug launch' or 'loom debug attach' first.\n\
                 Discovered adapter: {} ({})",
                adapter.command, adapter.name
            );
        }
        DebugAction::Step { direction: _ } => {
            anyhow::bail!(
                "Step operations require an active debug session. \
                 Use 'loom debug launch' or 'loom debug attach' first."
            );
        }
        DebugAction::Evaluate { expression: _ } => {
            anyhow::bail!(
                "Evaluate requires an active debug session. \
                 Use 'loom debug launch' or 'loom debug attach' first."
            );
        }
        DebugAction::Stack => {
            anyhow::bail!(
                "Stack trace requires an active debug session. \
                 Use 'loom debug launch' or 'loom debug attach' first."
            );
        }
    }
    Ok(())
}
