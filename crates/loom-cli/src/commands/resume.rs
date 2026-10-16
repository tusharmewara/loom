use loom_core::config::ConfigReader;
use loom_knowledge::KnowledgeGraph;
use loom_sessions::SessionManager;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

const DEFAULT_PROVIDER: &str = "openai";

pub async fn resume_session(
    loom_dir: &Path,
    session_id: &str,
    cli_provider: Option<&str>,
    cli_model: Option<&str>,
    cli_agent: Option<&str>,
    inject_memory: bool,
    inject_knowledge: bool,
) -> anyhow::Result<()> {
    let loom_dir_str = loom_dir.to_str().unwrap();
    let manager = SessionManager::new(loom_dir_str);
    let store = manager.store();

    let manifest = store
        .get(session_id)
        .ok_or_else(|| anyhow::anyhow!("Session '{session_id}' not found"))?;
    let mut messages = store.load_messages(session_id)?;

    if messages.is_empty() {
        anyhow::bail!("Session '{session_id}' has no messages to resume from");
    }

    // Inject cross-session memory context if requested
    if inject_memory {
        if let Ok(memory) = loom_sessions::memory::CrossSessionMemory::new(loom_dir_str) {
            let relevant = memory.search(&manifest.topic, 5).unwrap_or_default();
            if !relevant.is_empty() {
                let context_lines: Vec<String> = relevant
                    .iter()
                    .map(|hit| {
                        format!(
                            "[session:{} role:{}] {}",
                            &hit.session_id[..hit.session_id.len().min(8)],
                            hit.role,
                            hit.content
                        )
                    })
                    .collect();
                let context_msg = format!(
                    "Relevant context from other sessions:\n{}",
                    context_lines.join("\n")
                );
                messages.insert(
                    0,
                    loom_core::Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: loom_core::MessageRole::System,
                        content: context_msg,
                        tool_calls: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        metadata: Default::default(),
                    },
                );
                println!(
                    "  Injected {} relevant context messages from cross-session memory",
                    relevant.len()
                );
            }
        }
    }

    // Inject knowledge graph context if requested
    if inject_knowledge {
        if let Ok(kg) = KnowledgeGraph::new(loom_dir_str) {
            let topic = &manifest.topic;
            let recent_msgs: Vec<&str> = messages
                .iter()
                .rev()
                .take(5)
                .map(|m| m.content.as_str())
                .collect();
            let search_terms: Vec<String> = {
                let words: Vec<String> = std::iter::once(topic.clone())
                    .chain(recent_msgs.iter().map(|s| (*s).to_string()))
                    .collect();
                words
                    .iter()
                    .flat_map(|s| s.split_whitespace())
                    .filter(|w| w.len() > 3)
                    .map(|w| w.to_lowercase())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .take(10)
                    .collect()
            };

            let mut entity_info = Vec::new();
            for term in &search_terms {
                if let Ok(Some(entity)) = kg.get_entity(term) {
                    if let Ok(related) = kg.related_entities(&entity.id) {
                        let degree = related.len();
                        if !related.is_empty() {
                            let rels: Vec<String> = related
                                .iter()
                                .map(|r| {
                                    format!(
                                        "  - {} --[{}]--> {} (weight: {:.1})",
                                        r.entity.name,
                                        r.relationship.predicate,
                                        r.relationship.object_name,
                                        r.relationship.weight
                                    )
                                })
                                .collect();
                            entity_info.push(format!(
                                "Entity: {} (type: {}, degree: {})\n{}",
                                entity.name,
                                entity.entity_type,
                                degree,
                                rels.join("\n")
                            ));
                        } else {
                            entity_info.push(format!(
                                "Entity: {} (type: {}, degree: {})",
                                entity.name, entity.entity_type, degree
                            ));
                        }
                    }
                }
            }

            if !entity_info.is_empty() {
                let context_msg = format!(
                    "Knowledge graph context relevant to this session:\n{}",
                    entity_info.join("\n\n")
                );
                messages.insert(
                    0,
                    loom_core::Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: loom_core::MessageRole::System,
                        content: context_msg,
                        tool_calls: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        metadata: Default::default(),
                    },
                );
                println!(
                    "  Injected {} knowledge graph entities relevant to session topic",
                    entity_info.len()
                );
            } else {
                println!("  No knowledge graph entities found for session topic");
            }
        }
    }

    let config = ConfigReader::new(loom_dir_str).load().ok();
    let routing = config.as_ref().and_then(|c| c.routing.as_ref());

    let provider = resolve_provider(cli_provider, routing);
    let model = resolve_model(cli_model, routing, &provider);

    if let Some(agent) = cli_agent {
        return launch_agent_with_session_context(
            loom_dir_str,
            session_id,
            agent,
            &manifest,
            &messages,
            &provider,
            &model,
        )
        .await;
    }

    println!("=== Resuming session {} ===", session_id);
    println!("  Tool:     {}", manifest.source_tool);
    println!("  Topic:    {}", manifest.topic);
    println!("  Provider: {}", provider);
    println!("  Model:    {}", model);
    println!("  Messages: {}", messages.len());
    if let (Some(ts), Some(folded)) = (
        manifest.summarized_at.as_deref(),
        manifest.summarized_message_count,
    ) {
        println!(
            "  Summary:  {} earlier message{} compressed (at {})",
            folded,
            if folded == 1 { "" } else { "s" },
            ts
        );
    }
    println!();

    let api_key = resolve_api_key(&provider)?;
    let provider_messages = convert_messages(&messages);

    if let Some(last) = messages.last() {
        println!("--- Last message ---");
        match last.role {
            loom_core::MessageRole::User => println!("You: {}", last.content),
            loom_core::MessageRole::Assistant => println!("Assistant: {}", last.content),
            _ => println!("{:?}: {}", last.role, last.content),
        }
        println!();
    }

    println!("--- Interactive resume (type your message, /exit to quit) ---");

    let mut all_messages = provider_messages;
    loop {
        print!(">>> ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            break;
        }
        let input = input.trim().to_string();

        if input.is_empty() || input == "/exit" || input == "/quit" {
            break;
        }

        all_messages.push(serde_json::json!({
            "role": "user",
            "content": input,
        }));

        let response = send_to_provider(&provider, &model, &api_key, &all_messages).await?;

        println!("\n{}", response.text);
        if response.input_tokens > 0 || response.output_tokens > 0 {
            println!(
                "\n[Tokens: {} in / {} out]",
                response.input_tokens, response.output_tokens
            );
        }

        all_messages.push(serde_json::json!({
            "role": "assistant",
            "content": response.text,
        }));
    }

    Ok(())
}

async fn launch_agent_with_session_context(
    loom_dir: &str,
    session_id: &str,
    agent: &str,
    manifest: &loom_core::SessionManifest,
    messages: &[loom_core::Message],
    provider: &str,
    model: &str,
) -> anyhow::Result<()> {
    let home = loom_core::home_dir();

    // Write session messages as a context file the agent can read
    let context_path = std::path::Path::new(loom_dir)
        .join("sessions")
        .join(session_id)
        .join("resume_context.json");
    let context = serde_json::json!({
        "session_id": session_id,
        "source_tool": manifest.source_tool,
        "topic": manifest.topic,
        "message_count": messages.len(),
        "provider": provider,
        "model": model,
        "messages": messages.iter().map(|m| {
            serde_json::json!({
                "role": match m.role {
                    loom_core::MessageRole::User => "user",
                    loom_core::MessageRole::Assistant => "assistant",
                    loom_core::MessageRole::System => "system",
                    loom_core::MessageRole::Tool => "tool",
                },
                "content": m.content,
            })
        }).collect::<Vec<_>>(),
    });
    std::fs::write(&context_path, serde_json::to_string_pretty(&context)?)?;

    let mut env_vars: HashMap<String, String> = HashMap::new();
    env_vars.insert("LOOM_AGENTS_DIR".to_string(), loom_dir.to_string());
    env_vars.insert("LOOM_ENABLED".to_string(), "1".to_string());
    env_vars.insert("LOOM_SESSION_ID".to_string(), session_id.to_string());
    env_vars.insert(
        "LOOM_SESSION_CONTEXT_PATH".to_string(),
        context_path.to_string_lossy().to_string(),
    );
    env_vars.insert("LOOM_ROUTING_PROVIDER".to_string(), provider.to_string());
    env_vars.insert("LOOM_ROUTING_MODEL".to_string(), model.to_string());

    // Set up the agent tool adapter
    if let Some(adapter) = loom_adapters::adapter_for(agent) {
        let toml = adapter.default_toml(&home);

        // Skills
        if let Some(ref skills) = toml.skills {
            if let Some(ref native_dir) = skills.native_dir {
                env_vars.insert("LOOM_SKILLS_DIR".to_string(), format!("{loom_dir}/skills"));
                env_vars.insert(
                    format!("{}_SKILLS_DIR", agent.to_uppercase().replace('-', "_")),
                    native_dir.clone(),
                );
            }
        }

        // MCP
        if let Some(ref mcp) = toml.mcp {
            if let Some(ref config_path) = mcp.config_path {
                env_vars.insert("LOOM_MCP_DIR".to_string(), format!("{loom_dir}/mcp"));
                env_vars.insert(
                    format!("{}_MCP_CONFIG", agent.to_uppercase().replace('-', "_")),
                    config_path.clone(),
                );
            }
        }

        // Sessions
        if let Some(ref sessions) = toml.sessions {
            if let Some(ref store_path) = sessions.store_path {
                env_vars.insert(
                    "LOOM_SESSIONS_DIR".to_string(),
                    format!("{loom_dir}/sessions"),
                );
                env_vars.insert(
                    format!("{}_SESSIONS_DIR", agent.to_uppercase().replace('-', "_")),
                    store_path.clone(),
                );
            }
        }

        // Apply routing decision through the adapter
        let decision = loom_core::RoutingDecision {
            recommended_tier: loom_core::ModelTier::Mid,
            recommended_provider: provider.to_string(),
            recommended_model: model.to_string(),
            reason: "Session resume".to_string(),
            overrides_role_default: true,
            decided_at: chrono::Utc::now().to_rfc3339(),
            cost_estimate: loom_core::CostEstimate::default(),
        };
        adapter.apply_routing_decision(&decision)?;

        // Read back tool-native model env var
        let tool_key = format!("{}_MODEL", agent.to_uppercase().replace('-', "_"));
        if let Ok(val) = std::env::var(&tool_key) {
            env_vars.insert(tool_key, val);
        }
    } else {
        // No adapter — set generic model env var
        env_vars.insert(
            format!("{}_MODEL", agent.to_uppercase().replace('-', "_")),
            model.to_string(),
        );
    }

    // Find the binary
    let binary_path = find_binary(agent)?;

    println!("=== Resuming session {} via {} ===", session_id, agent);
    println!("  Topic:    {}", manifest.topic);
    println!("  Provider: {}", provider);
    println!("  Model:    {}", model);
    println!("  Context:  {}", context_path.display());
    println!();

    // Spawn the agent tool
    let mut cmd = std::process::Command::new(&binary_path);
    cmd.envs(&env_vars);
    cmd.stdin(std::process::Stdio::inherit());
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("Agent tool '{agent}' exited with status: {status}");
    }
    Ok(())
}

fn find_binary(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let path = std::path::Path::new(name);
    if path.is_absolute() && path.exists() {
        return Ok(path.to_path_buf());
    }
    let path_env = std::env::var("PATH").map_err(|_| anyhow::anyhow!("PATH not set"))?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if candidate.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&candidate) {
                    if meta.permissions().mode() & 0o111 != 0 {
                        return Ok(candidate);
                    }
                }
            }
            #[cfg(not(unix))]
            {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("Binary '{name}' not found in PATH")
}

fn resolve_provider(cli: Option<&str>, routing: Option<&loom_core::RoutingConfig>) -> String {
    if let Some(v) = cli.filter(|s| !s.is_empty()) {
        return v.to_string();
    }
    if let Ok(v) = std::env::var("LOOM_ROUTING_PROVIDER") {
        if !v.is_empty() {
            return v;
        }
    }
    if let Some(v) = routing.and_then(|r| r.default_provider.as_ref()) {
        if !v.is_empty() {
            return v.clone();
        }
    }
    DEFAULT_PROVIDER.to_string()
}

fn resolve_model(
    cli: Option<&str>,
    routing: Option<&loom_core::RoutingConfig>,
    provider: &str,
) -> String {
    if let Some(v) = cli.filter(|s| !s.is_empty()) {
        return v.to_string();
    }
    if let Some(v) = routing.and_then(|r| r.default_model.as_ref()) {
        if !v.is_empty() {
            return v.clone();
        }
    }
    default_model(provider).to_string()
}

struct ProviderResponse {
    text: String,
    input_tokens: u64,
    output_tokens: u64,
}

fn default_model(provider: &str) -> &'static str {
    match provider {
        "openai" | "gpt" => "gpt-4",
        "anthropic" | "claude" => "claude-sonnet-4-20250514",
        "deepseek" => "deepseek-chat",
        "google" | "gemini" => "gemini-2.0-flash",
        _ => "gpt-4",
    }
}

fn resolve_api_key(provider: &str) -> anyhow::Result<String> {
    if provider.starts_with("http") {
        return std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .map_err(|_| anyhow::anyhow!("No API key found for custom endpoint. Set OPENAI_API_KEY or ANTHROPIC_API_KEY."));
    }
    match provider {
        "anthropic" | "claude" => std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set")),
        _ => std::env::var("OPENAI_API_KEY").map_err(|_| anyhow::anyhow!("OPENAI_API_KEY not set")),
    }
}

fn provider_endpoint(provider: &str) -> String {
    if provider.starts_with("http") {
        return provider.to_string();
    }
    match provider {
        "openai" | "gpt" | "gpt-4" | "gpt-3.5" => "https://api.openai.com/v1/chat/completions",
        "deepseek" => "https://api.deepseek.com/v1/chat/completions",
        "google" | "gemini" => {
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        }
        _ => "https://api.openai.com/v1/chat/completions",
    }
    .to_string()
}

fn convert_messages(messages: &[loom_core::Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| {
            let role = match msg.role {
                loom_core::MessageRole::User => "user",
                loom_core::MessageRole::Assistant => "assistant",
                loom_core::MessageRole::System => "system",
                loom_core::MessageRole::Tool => "tool",
            };
            serde_json::json!({
                "role": role,
                "content": msg.content,
            })
        })
        .collect()
}

async fn send_to_provider(
    provider: &str,
    model: &str,
    api_key: &str,
    messages: &[serde_json::Value],
) -> anyhow::Result<ProviderResponse> {
    if provider.starts_with("http") {
        return send_openai_compatible(provider, api_key, model, messages).await;
    }
    match provider {
        "anthropic" | "claude" => send_anthropic(api_key, model, messages).await,
        _ => send_openai_compatible(provider, api_key, model, messages).await,
    }
}

async fn send_openai_compatible(
    provider: &str,
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> anyhow::Result<ProviderResponse> {
    let endpoint = provider_endpoint(provider);
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
    });

    let resp: serde_json::Value = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response: {}", resp))?
        .to_string();

    let usage = &resp["usage"];
    let input_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
    let output_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);

    Ok(ProviderResponse {
        text,
        input_tokens,
        output_tokens,
    })
}

async fn send_anthropic(
    api_key: &str,
    model: &str,
    messages: &[serde_json::Value],
) -> anyhow::Result<ProviderResponse> {
    let client = reqwest::Client::new();

    let system = messages
        .iter()
        .filter(|m| m["role"] == "system")
        .map(|m| m["content"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let chat_messages: Vec<_> = messages
        .iter()
        .filter(|m| m["role"] != "system")
        .map(|m| {
            serde_json::json!({
                "role": m["role"],
                "content": m["content"],
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 4096,
        "messages": chat_messages,
    });

    if !system.is_empty() {
        body["system"] = serde_json::json!(system);
    }

    let resp: serde_json::Value = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = resp["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No content in response: {}", resp))?
        .to_string();

    let usage = &resp["usage"];
    let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);

    Ok(ProviderResponse {
        text,
        input_tokens,
        output_tokens,
    })
}
