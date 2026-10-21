use crate::cli::AuthAction;
use loom_auth::audit::AuditLogger;
use loom_auth::export::{build_export, verify_export, AuditExport};
use loom_auth::AuthManager;
use loom_core::config::ConfigReader;
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
        AuthAction::Export { output, json } => {
            cmd_export(loom_dir, output.as_deref(), *json)?;
        }
        AuthAction::Verify { path, json } => {
            cmd_verify(loom_dir, path.as_deref(), *json)?;
        }
    }
    Ok(())
}

/// Export the audit chain to a portable, verifiable JSON file.
///
/// The retention window comes from the merged config
/// (`[security] audit_retention_days`); entries older than the window are
/// dropped from the export. The on-disk chain is never rewritten.
fn cmd_export(loom_dir: &Path, output: Option<&str>, json: bool) -> anyhow::Result<()> {
    let retention_days = retention_days(loom_dir);
    let logger = AuditLogger::new(loom_dir.to_str().unwrap());
    let doc = build_export(&logger, retention_days)?;

    let out_path = match output {
        Some(p) => Path::new(p).to_path_buf(),
        None => loom_dir.join("auth").join("audit-export.json"),
    };
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(&doc)?;
    std::fs::write(&out_path, &body)
        .map_err(|e| anyhow::anyhow!("failed to write audit export: {e}"))?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "auth.export",
                "path": out_path.to_string_lossy(),
                "entries": doc.entries.len(),
                "dropped_entries": doc.retention.dropped_entries,
                "redacted_entries": doc.redaction.redacted_entries,
                "retention_days": doc.retention.window_days,
                "chain_intact": doc.source.chain_intact,
                "export_digest": doc.export_digest,
            }))?
        );
    } else {
        println!("Audit export written to {}", out_path.display());
        println!("  entries:         {}", doc.entries.len());
        println!(
            "  dropped (older than {retention_days}d): {}",
            doc.retention.dropped_entries
        );
        println!("  redacted:        {}", doc.redaction.redacted_entries);
        println!("  chain intact:    {}", doc.source.chain_intact);
        println!("  export digest:   {}", doc.export_digest);
        if doc.redaction.redacted_entries > 0 {
            println!(
                "  note: the free-text result of {} entr(ies) was redacted from the export",
                doc.redaction.redacted_entries
            );
        }
    }
    Ok(())
}

/// Verify an audit export file, or the local audit chain when no path is given.
fn cmd_verify(loom_dir: &Path, path: Option<&str>, json: bool) -> anyhow::Result<()> {
    let (result, entries) = match path {
        Some(p) => {
            let file = Path::new(p);
            let raw = std::fs::read_to_string(file).map_err(|e| {
                anyhow::anyhow!("failed to read audit export {}: {e}", file.display())
            })?;
            let doc: AuditExport = serde_json::from_str(&raw).map_err(|e| {
                anyhow::anyhow!("failed to parse audit export {}: {e}", file.display())
            })?;
            let v = verify_export(&doc);
            (v, doc.entries.len())
        }
        None => {
            // No file: verify the local chain directly.
            let logger = AuditLogger::new(loom_dir.to_str().unwrap());
            let report = logger.verify_integrity()?;
            (
                loom_auth::export::ExportVerification {
                    intact: report.intact,
                    digest_ok: report.intact,
                    chain_linked: report.intact,
                    chain_head_ok: report.intact,
                    source_chain_intact: report.intact,
                    entries: report.total,
                    problems: report
                        .tampered_at
                        .iter()
                        .map(|t| format!("chain broken at entry {t}"))
                        .collect(),
                },
                report.total,
            )
        }
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "command": "auth.verify",
                "path": path,
                "intact": result.intact,
                "digest_ok": result.digest_ok,
                "chain_linked": result.chain_linked,
                "chain_head_ok": result.chain_head_ok,
                "source_chain_intact": result.source_chain_intact,
                "entries": result.entries,
                "problems": result.problems,
            }))?
        );
    } else {
        if result.intact {
            println!("✓ audit chain intact ({entries} entries)");
        } else {
            println!("✗ audit chain NOT intact ({entries} entries)");
            for problem in &result.problems {
                println!("  - {problem}");
            }
        }
    }

    if !result.intact {
        anyhow::bail!("audit chain verification failed");
    }
    Ok(())
}

/// The configured audit retention window, in days.
///
/// Read from the merged config (`[security] audit_retention_days`) so the
/// policy lives in exactly one place. A config that cannot be read falls
/// back to the built-in default.
fn retention_days(loom_dir: &Path) -> u32 {
    ConfigReader::new(loom_dir.to_str().unwrap())
        .load()
        .map(|c| c.security.audit_retention_days)
        .unwrap_or(loom_core::types::DEFAULT_AUDIT_RETENTION_DAYS)
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

#[cfg(test)]
mod tests {
    use super::*;
    use loom_auth::audit::AuditLogger;

    /// Seed an audit chain with one read, one write and one denial.
    fn seed_chain(loom_dir: &Path) {
        let logger = AuditLogger::new(loom_dir.to_str().unwrap());
        logger.log_read("tool-a", "provider-1", true).unwrap();
        logger.log_write("tool-b", "provider-2", true).unwrap();
        logger.log_denied_access("tool-c", "provider-3", "acl denied");
    }

    #[test]
    fn test_export_writes_file_and_verify_accepts_it() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        seed_chain(loom_dir);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true).unwrap();

        assert!(out.exists(), "export file must be written");
        let raw = std::fs::read_to_string(&out).unwrap();
        let doc: AuditExport = serde_json::from_str(&raw).unwrap();
        assert_eq!(doc.entries.len(), 3);

        // A re-imported export verifies as intact.
        cmd_verify(loom_dir, Some(out.to_str().unwrap()), true).unwrap();
    }

    #[test]
    fn test_verify_rejects_tampered_export() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        seed_chain(loom_dir);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true).unwrap();

        // Tamper with an exported field.
        let raw = std::fs::read_to_string(&out).unwrap();
        let tampered = raw.replace("provider-1", "provider-9");
        assert_ne!(raw, tampered);
        std::fs::write(&out, tampered).unwrap();

        let err = cmd_verify(loom_dir, Some(out.to_str().unwrap()), true).unwrap_err();
        assert!(
            err.to_string().contains("verification failed"),
            "tampered export must fail verification: {err}"
        );
    }

    #[test]
    fn test_export_never_writes_secret_values() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        let logger = AuditLogger::new(loom_dir.to_str().unwrap());
        let secret = "sk-live-SUPERSECRETVALUE";
        logger.log_denied_access("tool-a", "provider-1", secret);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true).unwrap();
        let raw = std::fs::read_to_string(&out).unwrap();
        assert!(
            !raw.contains(secret),
            "secret must never reach the export: {raw}"
        );
    }

    #[test]
    fn test_retention_days_comes_from_config() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();

        // Default: 365 days.
        assert_eq!(retention_days(loom_dir), 365);

        // Configured: 30 days.
        std::fs::write(
            loom_dir.join("config.toml"),
            "[security]\naudit_retention_days = 30\n",
        )
        .unwrap();
        assert_eq!(retention_days(loom_dir), 30);
    }

    #[test]
    fn test_export_drops_entries_past_the_retention_window() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();

        // An old entry, then a fresh one.
        let logger = AuditLogger::new(loom_dir.to_str().unwrap());
        let old = loom_core::AuditEntry {
            timestamp: "2020-01-01T00:00:00Z".to_string(),
            requesting_tool: "old-tool".to_string(),
            provider_id: "provider-1".to_string(),
            access_type: "read".to_string(),
            result: "granted".to_string(),
            prev_hash: None,
            entry_hash: None,
        };
        logger.log(&old).unwrap();
        logger.log_read("new-tool", "provider-2", true).unwrap();

        std::fs::write(
            loom_dir.join("config.toml"),
            "[security]\naudit_retention_days = 30\n",
        )
        .unwrap();

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true).unwrap();
        let raw = std::fs::read_to_string(&out).unwrap();
        let doc: AuditExport = serde_json::from_str(&raw).unwrap();

        assert_eq!(doc.retention.window_days, 30);
        assert_eq!(doc.retention.policy, "drop");
        assert_eq!(doc.retention.dropped_entries, 1);
        assert_eq!(doc.entries.len(), 1);
        assert_eq!(doc.entries[0].requesting_tool, "new-tool");
    }
}
