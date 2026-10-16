//! `loom acp run`: one canonical ACP prompt turn against an agent
//! program, persisted as a Loom session.

use loom_sessions::acp_protocol::{build_acp_session, AcpClientConfig, AcpProtocolClient};
use std::path::Path;

/// Run one prompt turn against a canonical ACP agent and persist the
/// transcript as a Loom session (indexed into memory + knowledge).
#[allow(clippy::too_many_arguments)]
pub async fn run_acp_prompt(
    loom_dir: &Path,
    program: &str,
    args: &[String],
    prompt: &str,
    cwd: Option<&str>,
    timeout_secs: u64,
    json_output: bool,
    runtime: Option<String>,
) -> anyhow::Result<()> {
    if let Some(ref rt) = runtime {
        return run_acp_via_runtime(
            loom_dir,
            program,
            args,
            prompt,
            cwd,
            timeout_secs,
            json_output,
            rt,
        )
        .await;
    }
    run_acp_prompt_direct(
        loom_dir,
        program,
        args,
        prompt,
        cwd,
        timeout_secs,
        json_output,
    )
    .await
}

/// `--runtime` is accepted for CLI compatibility only: ACP over
/// container runtimes was never implemented (the old path prepared and
/// ran a container, discarded the result, and fell back to direct
/// execution anyway). Warn and run directly.
// ponytail: real ACP-in-runtime needs stdio wiring through the runtime
// abstraction; build it here when containerized agents are actually wanted.
#[allow(clippy::too_many_arguments)]
async fn run_acp_via_runtime(
    loom_dir: &Path,
    program: &str,
    args: &[String],
    prompt: &str,
    cwd: Option<&str>,
    timeout_secs: u64,
    json_output: bool,
    runtime: &str,
) -> anyhow::Result<()> {
    eprintln!(
        "Warning: runtime '{runtime}' does not support the ACP protocol yet; running directly"
    );
    run_acp_prompt_direct(
        loom_dir,
        program,
        args,
        prompt,
        cwd,
        timeout_secs,
        json_output,
    )
    .await
}

fn program_basename(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
        .to_string()
}

async fn run_acp_prompt_direct(
    loom_dir: &Path,
    program: &str,
    args: &[String],
    prompt: &str,
    cwd: Option<&str>,
    timeout_secs: u64,
    json_output: bool,
) -> anyhow::Result<()> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("prompt must not be empty");
    }
    let mut config_args = args.to_vec();
    if program == "cline" {
        config_args.insert(0, "--acp".to_string());
    }
    let config = AcpClientConfig {
        program: program.to_string(),
        args: config_args,
        cwd: cwd.map(str::to_string),
        timeout_secs,
    };
    let transcript = AcpProtocolClient::run_prompt(&config, prompt)?;

    let agent_name = program_basename(program);
    let (manifest, messages) = build_acp_session(&transcript, prompt, program, None);
    let loom_dir_str = loom_dir.to_str().unwrap();
    let store = loom_sessions::store::SessionStore::new(loom_dir_str);
    store.save_manifest(&manifest)?;
    store.replace_messages(&manifest.session_id, &messages)?;
    if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir_str) {
        if let Err(e) = memory.index_session_with_tags(
            &manifest.session_id,
            &messages,
            &manifest.topic,
            &manifest.tags,
        ) {
            eprintln!("  Warning: memory index failed: {e}");
        }
    }
    if let Ok(kg) = loom_knowledge::KnowledgeGraph::new(loom_dir_str) {
        if let Err(e) = kg.extract_from_messages(&messages, &manifest.session_id) {
            eprintln!("  Warning: knowledge extraction failed: {e}");
        }
    }

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": manifest.session_id,
                "agent_session_id": transcript.session_id,
                "stop_reason": transcript.stop_reason,
                "tool_calls": transcript.tool_calls,
            }))?
        );
        return Ok(());
    }
    println!(
        "ACP run completed (agent={agent_name}, stop={})",
        transcript.stop_reason
    );
    println!("  Session: {}", manifest.session_id);
    if transcript.agent_text.trim().is_empty() {
        println!("  (agent produced no text output)");
    } else {
        println!("--- agent output ---");
        println!("{}", transcript.agent_text.trim());
    }
    if !transcript.tool_calls.is_empty() {
        println!("  Tools: {}", transcript.tool_calls.join(", "));
    }
    Ok(())
}
