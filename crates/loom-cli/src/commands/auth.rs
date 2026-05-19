use crate::cli::AuthAction;
use std::path::Path;

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
                    println!("No auth providers configured.");
                } else {
                    for p in &providers {
                        println!("{}", p);
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
