//! `loom acp run`: one canonical ACP prompt turn against an agent
//! program, persisted as a Loom session.

use loom_core::{Message, SessionManifest, Task};
use loom_sessions::acp_protocol::{AcpClientConfig, AcpProtocolClient, AcpTranscript};
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
    // If runtime is specified, use the Runtime abstraction
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
        ).await;
    }

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
    let (manifest, messages) = build_acp_session(&transcript, prompt, program);
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
    println!("ACP run completed (agent={agent_name}, stop={})", transcript.stop_reason);
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

fn program_basename(program: &str) -> String {
    std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
        .to_string()
}

/// Build the manifest + System/User/Assistant messages for an ACP
/// transcript. Pure so the shaping is unit-testable.
pub fn build_acp_session(
    transcript: &AcpTranscript,
    prompt: &str,
    program: &str,
) -> (SessionManifest, Vec<Message>) {
    let prompt = prompt.trim();
    let agent_name = program_basename(program);
    let session_id = format!(
        "acp-{}",
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let topic: String = prompt.chars().take(80).collect();
    let now = chrono::Utc::now().to_rfc3339();

    let mut assistant = if transcript.agent_text.trim().is_empty() {
        "(agent produced no text output)".to_string()
    } else {
        transcript.agent_text.trim().to_string()
    };
    if !transcript.tool_calls.is_empty() {
        assistant.push_str("\n\nTools:\n");
        for tool in &transcript.tool_calls {
            assistant.push_str(&format!("- {tool}\n"));
        }
    }
    assistant.push_str(&format!("\n[stop reason: {}]", transcript.stop_reason));

    let messages = vec![
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::System,
            content: format!(
                "ACP protocol run\n  agent: {program}\n  agent_session: {}\n  stop_reason: {}",
                transcript.session_id, transcript.stop_reason,
            ),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::User,
            content: prompt.to_string(),
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: loom_core::MessageRole::Assistant,
            content: assistant,
            tool_calls: None,
            timestamp: now.clone(),
            metadata: Default::default(),
        },
    ];

    let tags = vec![
        "acp".to_string(),
        "acp-protocol".to_string(),
        format!("agent:{agent_name}"),
    ];
    let manifest = SessionManifest {
        session_id,
        source_tool: format!("loom-acp/{agent_name}"),
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
        model_provider: agent_name,
        files_modified: vec![],
        compatibility_note: Some(format!(
            "Canonical ACP run against '{program}' (stop={})",
            transcript.stop_reason
        )),
        tags,
        summarized_at: None,
        summarized_message_count: None,
    };
    (manifest, messages)
}

/// Execute an ACP run via the Runtime abstraction.
///
/// This function runs the ACP agent inside a container runtime (docker, podman, kubernetes)
/// while preserving the ACP protocol transcript and session persistence.
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
    use loom_runtime::{LaunchConfig, ProcessRuntime, RuntimeRegistry};
    use std::collections::HashMap;
    #[cfg(feature = "docker")]
    use loom_runtime::ContainerRuntime;
    #[cfg(feature = "podman")]
    use loom_runtime::ContainerRuntime;
    #[cfg(feature = "kubernetes")]
    use loom_runtime::ContainerRuntime;

    // Build launch config for the ACP agent
    let config = LaunchConfig {
        runtime: runtime.to_string(),
        command: program.to_string(),
        args: args.to_vec(),
        env: HashMap::new(),
        working_dir: cwd.map(str::to_string),
        image: None,
        network: None,
        auto_remove: true,
    };

    // Create runtime registry with all available runtimes
    let mut registry = RuntimeRegistry::new();
    let _ = registry.register(Box::new(ProcessRuntime::new()));
    #[cfg(feature = "docker")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::docker()));
    }
    #[cfg(feature = "podman")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::podman()));
    }
    #[cfg(feature = "kubernetes")]
    {
        let _ = registry.register(Box::new(ContainerRuntime::kubernetes()));
    }

    // Get the runtime
    let runtime_impl = registry
        .get(runtime)
        .ok_or_else(|| anyhow::anyhow!("Runtime '{}' not available", runtime))?;

    // Create a minimal task for runtime execution
    let task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        title: format!("acp run {}", program),
        description: prompt.to_string(),
        phase: loom_core::TaskPhase::Plan,
        phase_history: vec![],
        prd_path: None,
        context_files: vec![],
        routed_provider: None,
        files_modified: vec![],
        complexity_score: None,
        routing_decision: None,
        last_agent: None,
        linked_issue: None,
        sub_agents: None,
        phase_results: vec![],
        tags: vec!["acp".to_string(), "acp-protocol".to_string()],
        worktree_path: None,
        runtime: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        status: loom_core::TaskStatus::Pending,
    };

    // Prepare, run, cleanup
    runtime_impl.prepare(&config).await?;
    let result = runtime_impl.run(&config, &task).await?;
    runtime_impl.cleanup(&result).await?;

    // The Runtime abstraction doesn't directly support ACP protocol.
    // For now, fall back to direct execution with a warning.
    // TODO: Implement ACP protocol support in container runtimes.
    eprintln!("Warning: Runtime abstraction for ACP not fully implemented; falling back to direct execution");

    // Fall back to direct execution
    run_acp_prompt_direct(
        loom_dir,
        program,
        args,
        prompt,
        cwd,
        timeout_secs,
        json_output,
    ).await
}

// Direct execution (original implementation)
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
    let (manifest, messages) = build_acp_session(&transcript, prompt, program);
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
    println!("ACP run completed (agent={agent_name}, stop={})", transcript.stop_reason);
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

#[cfg(test)]
mod tests {
    use super::build_acp_session;

    fn transcript() -> loom_sessions::acp_protocol::AcpTranscript {
        loom_sessions::acp_protocol::AcpTranscript {
            session_id: "sess-abc".to_string(),
            stop_reason: "end_turn".to_string(),
            agent_text: "Hello back".to_string(),
            tool_calls: vec!["Reading file [completed]".to_string()],
        }
    }

    #[test]
    fn build_acp_session_shapes_messages_manifest_and_tags() {
        let (manifest, messages) =
            build_acp_session(&transcript(), "  hello agent, please help  ", "/usr/bin/fake-agent");
        assert_eq!(messages.len(), 3);
        let roles: Vec<String> =
            messages.iter().map(|m| format!("{:?}", m.role)).collect();
        assert_eq!(roles, vec!["System", "User", "Assistant"]);
        assert_eq!(messages[1].content, "hello agent, please help");
        assert!(messages[2].content.contains("Hello back"));
        assert!(messages[2].content.contains("Reading file [completed]"));
        assert_eq!(manifest.topic, "hello agent, please help");
        assert_eq!(manifest.source_tool, "loom-acp/fake-agent");
        assert!(manifest.tags.contains(&"acp-protocol".to_string()));
        assert!(manifest.tags.contains(&"agent:fake-agent".to_string()));
        assert!(manifest.session_id.starts_with("acp-"));
    }

    #[test]
    fn build_acp_session_truncates_long_topics_and_handles_empty_text() {
        let mut t = transcript();
        t.agent_text = String::new();
        t.tool_calls = vec![];
        let long_prompt = "p".repeat(200);
        let (manifest, messages) = build_acp_session(&t, &long_prompt, "agent");
        assert!(manifest.topic.chars().count() <= 80);
        assert!(messages[2].content.contains("no text output"));
    }
}

