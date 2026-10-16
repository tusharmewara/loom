use crate::cli::AuthAction;
use loom_auth::AuthManager;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProviderMeta {
    provider_id: String,
    name: String,
    auth_method: String,
    api_base_url: String,
    models_endpoint: String,
    status: String,
    last_validated: String,
    supported_tools: Vec<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ProviderFile {
    version: u32,
    providers: Vec<ProviderMeta>,
}

pub async fn run(loom_dir: &Path, action: &AuthAction) -> anyhow::Result<()> {
    match action {
        AuthAction::List { json } => {
            let auth_dir = loom_dir.join("auth");
            let mut providers = Vec::new();
            if auth_dir.exists() {
                for entry in std::fs::read_dir(&auth_dir)? {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().to_string();
                    providers.push(name);
                }
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.list",
                        "count": providers.len(),
                        "providers": providers
                    }))?
                );
            } else {
                if providers.is_empty() {
                    println!("No auth entries found.");
                } else {
                    for p in &providers {
                        println!("{}", p);
                    }
                }
            }
        }
        AuthAction::Providers { json } => {
            let providers_path = loom_dir.join("auth").join("providers.json");
            let entries = if providers_path.exists() {
                let raw = std::fs::read_to_string(&providers_path)?;
                let file: ProviderFile = serde_json::from_str(&raw)?;
                file.providers
            } else {
                vec![]
            };

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.providers",
                        "count": entries.len(),
                        "providers": entries.iter().map(|p| serde_json::json!({
                            "provider_id": p.provider_id,
                            "name": p.name,
                            "status": p.status,
                            "auth_method": p.auth_method,
                            "api_base_url": p.api_base_url,
                            "models_endpoint": p.models_endpoint,
                        })).collect::<Vec<_>>()
                    }))?
                );
            } else {
                if entries.is_empty() {
                    println!("No providers registered. Run `loom thread <tool>` to register a tool with providers.");
                } else {
                    println!("Registered providers ({}):", entries.len());
                    for p in &entries {
                        println!(
                            "  {} ({})  status={}  api={}  models={}",
                            p.provider_id,
                            p.name,
                            p.status,
                            if p.api_base_url.is_empty() {
                                "-"
                            } else {
                                &p.api_base_url
                            },
                            if p.models_endpoint.is_empty() {
                                "-"
                            } else {
                                &p.models_endpoint
                            },
                        );
                    }
                }
            }
        }
        AuthAction::Show { provider_id, json } => {
            let path = loom_dir.join("auth").join(format!("{}.toml", provider_id));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                if *json {
                    let config: toml::Value = toml::from_str(&content)?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "auth.show",
                            "provider_id": provider_id,
                            "config": config
                        }))?
                    );
                } else {
                    println!("{}", content);
                }
            } else {
                anyhow::bail!("Provider '{}' not found", provider_id);
            }
        }
        AuthAction::Validate { provider_id } => {
            cmd_validate(loom_dir, provider_id).await?;
        }
    }
    Ok(())
}

async fn cmd_validate(loom_dir: &Path, provider_id: &str) -> anyhow::Result<()> {
    println!("Validating provider '{}'...", provider_id);

    let managers = AuthManager::new(loom_dir.to_str().unwrap());

    // Look up the provider in the registry
    let provider = managers
        .list_providers()
        .into_iter()
        .find(|p| p.provider_id == provider_id);

    let (api_base_url, models_endpoint) = match &provider {
        Some(p) => {
            println!("  Provider:    {}", p.name);
            println!("  Auth method: {:?}", p.auth_method);
            println!("  API base:    {}", p.api_base_url);
            println!("  Models URL:  {}", p.models_endpoint);
            println!("  Status:      {:?}", p.status);
            (p.api_base_url.clone(), p.models_endpoint.clone())
        }
        None => {
            println!("  ⚠ Provider '{}' not found in registry.", provider_id);
            println!("    Trying environment variables anyway...");
            (String::new(), String::new())
        }
    };

    // Resolve the API key: env var first, then vault
    let api_key = resolve_api_key(provider_id);

    let key_display = api_key
        .as_deref()
        .map(|k| {
            if k.len() > 8 {
                format!("{}...{}", &k[..4], &k[k.len() - 4..])
            } else {
                "****".to_string()
            }
        })
        .unwrap_or_else(|| "none".to_string());

    match &api_key {
        Some(_) => println!("  API key:     {} (via env var)", key_display),
        None => {
            // Try vault
            match managers.vault().get_credential(provider_id) {
                Ok(Some(key)) => {
                    let masked = if key.len() > 8 {
                        format!("{}...{}", &key[..4], &key[key.len() - 4..])
                    } else {
                        "****".to_string()
                    };
                    println!("  API key:     {} (via vault)", masked);
                    // Re-wrap in Some for validation
                    let api_key = Some(key);
                    return do_validate(
                        provider_id,
                        &api_key,
                        &models_endpoint,
                        &api_base_url,
                        &managers,
                    )
                    .await;
                }
                Ok(None) => {
                    println!("  API key:     none found");
                }
                Err(e) => {
                    println!("  API key:     vault locked or error: {e}");
                }
            }
        }
    }

    if api_key.is_none() {
        println!();
        println!("  ✗ No API key found for provider '{}'", provider_id);
        println!(
            "    Set {}_API_KEY environment variable",
            provider_id.to_uppercase()
        );
        println!("    Or add credential to vault via auth import");
        return Ok(());
    }

    do_validate(
        provider_id,
        &api_key,
        &models_endpoint,
        &api_base_url,
        &managers,
    )
    .await
}

async fn do_validate(
    provider_id: &str,
    api_key: &Option<String>,
    models_endpoint: &str,
    api_base_url: &str,
    managers: &AuthManager,
) -> anyhow::Result<()> {
    let key = match api_key {
        Some(k) => k,
        None => {
            println!("  ✗ No API key available");
            return Ok(());
        }
    };

    // Determine the endpoint to test
    let test_url = if !models_endpoint.is_empty() {
        models_endpoint.to_string()
    } else if !api_base_url.is_empty() {
        format!("{}/v1/models", api_base_url.trim_end_matches('/'))
    } else {
        // Fallback: try common endpoints based on provider ID
        match provider_id {
            "openai" | "gpt" => "https://api.openai.com/v1/models".to_string(),
            "anthropic" | "claude" => "https://api.anthropic.com/v1/messages".to_string(),
            _ => {
                println!("  ⚠ No endpoint configured for this provider");
                println!("    Set models_endpoint or api_base_url in provider config");
                return Ok(());
            }
        }
    };

    println!("  Testing:     {}", test_url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let response = match provider_id {
        "anthropic" | "claude" => {
            client
                .get(&test_url)
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
        }
        _ => client.get(&test_url).bearer_auth(key).send().await,
    };

    println!();
    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                println!("  ✓ Validation successful (HTTP {})", status.as_u16());
                println!("  ✓ API key is valid");
                // Try to update provider status
                if managers.providers().get(provider_id).is_some() {
                    let _ = managers
                        .providers()
                        .update_status(provider_id, loom_core::ProviderStatus::Active);
                    // Update last_validated
                    let now = chrono::Utc::now().to_rfc3339();
                    if let Some(mut entry) = managers.providers().get(provider_id) {
                        entry.last_validated = now;
                        let _ = managers.providers().register(entry);
                    }
                    let _ = managers.providers().save();
                    println!("  ✓ Provider status updated to Active");
                }
            } else {
                let body = resp.text().await.unwrap_or_default();
                println!("  ✗ Validation failed (HTTP {})", status.as_u16());
                if !body.is_empty() {
                    let snippet = &body[..body.len().min(200)];
                    println!("    Response: {}", snippet);
                }
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    println!("    → API key is invalid or expired");
                }
            }
        }
        Err(e) => {
            if e.is_timeout() {
                println!("  ✗ Request timed out after 15s");
                println!("    → Endpoint may be unreachable or network issue");
            } else if e.is_connect() {
                println!("  ✗ Connection failed: {}", e);
                println!("    → Check API endpoint URL or network connectivity");
            } else {
                println!("  ✗ Request failed: {}", e);
            }
        }
    }

    Ok(())
}

fn resolve_api_key(provider_id: &str) -> Option<String> {
    // Try specific env vars based on provider ID
    let env_var_names = match provider_id {
        "openai" | "gpt" | "gpt-4" | "gpt-3.5" => {
            vec!["OPENAI_API_KEY".to_string()]
        }
        "anthropic" | "claude" => {
            vec!["ANTHROPIC_API_KEY".to_string()]
        }
        "deepseek" => {
            vec!["DEEPSEEK_API_KEY".to_string()]
        }
        "google" | "gemini" => {
            vec!["GOOGLE_API_KEY".to_string(), "GEMINI_API_KEY".to_string()]
        }
        _ => {
            // Try provider-specific and generic
            vec![
                format!("{}_API_KEY", provider_id.to_uppercase().replace('-', "_")),
                format!("{}_API_KEY", provider_id.to_uppercase()),
            ]
        }
    };

    // Try each env var
    for var_name in &env_var_names {
        if let Ok(val) = std::env::var(var_name) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }

    // Also check LOOM_VAULT_KEY or similar generic fallbacks
    // (handled by vault lookup in the caller)
    None
}
