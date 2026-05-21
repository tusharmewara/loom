use crate::cli::AuthAction;
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
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "auth.list",
                    "count": providers.len(),
                    "providers": providers
                }))?);
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
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
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
                }))?);
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
                            if p.api_base_url.is_empty() { "-" } else { &p.api_base_url },
                            if p.models_endpoint.is_empty() { "-" } else { &p.models_endpoint },
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
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "auth.show",
                        "provider_id": provider_id,
                        "config": config
                    }))?);
                } else {
                    println!("{}", content);
                }
            } else {
                anyhow::bail!("Provider '{}' not found", provider_id);
            }
        }
        AuthAction::Validate { provider_id } => {
            println!("Validating provider '{}'... (stub)", provider_id);
        }
    }
    Ok(())
}
