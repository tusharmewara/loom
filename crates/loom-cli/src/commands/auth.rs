use crate::cli::AuthAction;
use loom_auth::audit::AuditLogger;
use loom_auth::export::{
    build_export, load_or_create_signing_key, parse_public_key, public_key_hex, sign_export,
    verify_export, AuditExport, ExportVerdict,
};
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
        AuthAction::Export {
            output,
            json,
            print_public_key,
        } => {
            cmd_export(loom_dir, output.as_deref(), *json, *print_public_key)?;
        }
        AuthAction::Verify {
            path,
            json,
            public_key,
        } => {
            cmd_verify(loom_dir, path.as_deref(), *json, public_key.as_deref())?;
        }
    }
    Ok(())
}

/// Export the audit chain to a portable, verifiable JSON file.
///
/// The retention window comes from the merged config
/// (`[security] audit_retention_days`); entries older than the window are
/// dropped from the export. The on-disk chain is never rewritten.
///
/// The document is signed with the machine's per-machine Ed25519 key
/// (`<loom-dir>/auth/export-signing.key`, generated on first use). A consumer
/// that pins the matching public key can prove the export came from this
/// machine; the public key can be printed with `--print-public-key`.
fn cmd_export(
    loom_dir: &Path,
    output: Option<&str>,
    json: bool,
    print_public_key: bool,
) -> anyhow::Result<()> {
    if print_public_key {
        // Printing the key must not produce an export.
        let key = load_or_create_signing_key(loom_dir)?;
        let pubkey = public_key_hex(&key);
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "auth.export-public-key",
                    "public_key": pubkey,
                    "algorithm": "ed25519",
                }))?
            );
        } else {
            println!("{pubkey}");
        }
        return Ok(());
    }

    let retention_days = retention_days(loom_dir);
    let logger = AuditLogger::new(loom_dir.to_str().unwrap());
    let mut doc = build_export(&logger, retention_days)?;

    // Sign the document with the machine key. A failure to sign is a hard
    // failure: an unsigned export would only be self-consistent, and shipping
    // it silently would downgrade the trust model.
    let signing_key = load_or_create_signing_key(loom_dir)?;
    sign_export(&mut doc, &signing_key);

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
                "signed": doc.signature.is_some(),
                "public_key": doc.signature.as_ref().map(|s| s.public_key.clone()),
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
        if let Some(sig) = &doc.signature {
            println!("  signed:          yes (ed25519)");
            println!("  public key:      {}", sig.public_key);
            println!("  pin it with:     loom auth verify <file> --public-key <hex|file>");
        }
    }
    Ok(())
}

/// Build the JSON payload of `loom auth verify`.
///
/// The shape keeps the keys an earlier consumer already read
/// (`schema_version`, `command`, `path`, `intact`, `entries`, `problems` and
/// the four check flags) and adds the single `verdict` plus `verifies`, so
/// nothing breaks silently. The check flags are only present for an export
/// document — a local-chain verification has no digest, signature or chain
/// head to report.
fn verify_payload(
    path: Option<&str>,
    verdict: ExportVerdict,
    entries: usize,
    checks: Option<&loom_auth::export::ExportChecks>,
    problems: &[String],
    notes: &[String],
) -> anyhow::Result<serde_json::Value> {
    let mut payload = serde_json::json!({
        "schema_version": 1,
        "command": "auth.verify",
        "path": path,
        // What was verified: an export document, or the local on-disk chain.
        "verifies": if checks.is_some() { "export" } else { "local_chain" },
        "verdict": verdict,
        "intact": matches!(verdict, ExportVerdict::Intact),
        "entries": entries,
        "problems": problems,
    });
    if let Some(checks) = checks {
        if let serde_json::Value::Object(map) = serde_json::to_value(checks)? {
            for (k, v) in map {
                payload[k] = v;
            }
        }
    }
    if !notes.is_empty() {
        payload["notes"] = serde_json::to_value(notes)?;
    }
    Ok(payload)
}

/// Verify an audit export file, or the local audit chain when no path is given.
///
/// A pinned public key (`--public-key <hex|file>`) is what turns the export
/// from *self-consistent* into *provably from this machine*. Without one the
/// verdict is `unanchored` — never `intact`.
fn cmd_verify(
    loom_dir: &Path,
    path: Option<&str>,
    json: bool,
    public_key: Option<&str>,
) -> anyhow::Result<()> {
    let pinned = match public_key {
        Some(spec) => Some(parse_public_key(spec)?),
        None => None,
    };

    let (verdict, entries, checks, problems, notes) = match path {
        Some(p) => {
            let file = Path::new(p);
            let raw = std::fs::read_to_string(file).map_err(|e| {
                anyhow::anyhow!("failed to read audit export {}: {e}", file.display())
            })?;
            let doc: AuditExport = serde_json::from_str(&raw).map_err(|e| {
                anyhow::anyhow!("failed to parse audit export {}: {e}", file.display())
            })?;
            let v = verify_export(&doc, pinned.as_ref());
            (
                v.verdict,
                v.entries,
                Some(v.checks),
                v.problems,
                v.notes,
            )
        }
        None => {
            // No file: verify the local chain directly. This checks the
            // on-disk chain, not an export — the export-specific checks do
            // not apply and are not reported.
            let logger = AuditLogger::new(loom_dir.to_str().unwrap());
            let report = logger.verify_integrity()?;
            let problems = report
                .tampered_at
                .iter()
                .map(|t| format!("chain broken at entry {t}"))
                .collect::<Vec<_>>();
            let verdict = if report.intact {
                ExportVerdict::Intact
            } else {
                ExportVerdict::Tampered
            };
            (
                verdict,
                report.total,
                None,
                problems,
                Vec::new(),
            )
        }
    };

    if json {
        let payload = verify_payload(path, verdict, entries, checks.as_ref(), &problems, &notes)?;
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        match verdict {
            ExportVerdict::Intact => {
                if checks.is_some() {
                    println!("✓ audit export INTACT ({entries} entries)");
                    println!("  chain re-derived, signature verified against the pinned key");
                } else {
                    println!("✓ audit chain intact ({entries} entries)");
                }
            }
            ExportVerdict::Unanchored => {
                println!("⚠ audit export UNANCHORED ({entries} entries)");
                println!("  the exported fields re-derive the chain where they allow it —");
                println!("  but no public key was pinned, so the document is NOT proven to come from this machine");
            }
            ExportVerdict::Tampered => {
                if checks.is_some() {
                    println!("✗ audit export TAMPERED ({entries} entries)");
                } else {
                    println!("✗ audit chain TAMPERED ({entries} entries)");
                }
            }
        }
        for problem in &problems {
            println!("  - {problem}");
        }
        for note in &notes {
            // The unanchored reason is already spelled out above; printing it
            // twice adds nothing.
            if note.starts_with("no public key was pinned") {
                continue;
            }
            println!("  note: {note}");
        }
    }

    if verdict == ExportVerdict::Tampered {
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
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();

        assert!(out.exists(), "export file must be written");
        let raw = std::fs::read_to_string(&out).unwrap();
        let doc: AuditExport = serde_json::from_str(&raw).unwrap();
        assert_eq!(doc.entries.len(), 3);
        assert!(doc.signature.is_some(), "export must be signed");

        // A re-imported export verifies as intact against the pinned key.
        let key = public_key_hex(&load_or_create_signing_key(loom_dir).unwrap());
        cmd_verify(loom_dir, Some(out.to_str().unwrap()), true, Some(&key)).unwrap();
    }

    #[test]
    fn test_verify_without_pinned_key_is_unanchored() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        seed_chain(loom_dir);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();

        // No pinned key: the document is self-consistent, but not anchored.
        // It must still verify (not be reported as tampered).
        cmd_verify(loom_dir, Some(out.to_str().unwrap()), true, None).unwrap();
    }

    /// The JSON shape is a contract: an existing consumer reads `intact`,
    /// `entries`, `problems` and the four check flags. They must survive, and
    /// the new `verdict` must be there next to them.
    #[test]
    fn test_verify_json_keeps_the_old_keys_and_adds_the_verdict() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        seed_chain(loom_dir);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();

        let doc: AuditExport =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        // Verify without a pinned key: the verdict is `unanchored`, but the
        // document is still consistent.
        let v = loom_auth::export::verify_export(&doc, None);
        let payload = verify_payload(
            Some(out.to_str().unwrap()),
            v.verdict,
            v.entries,
            Some(&v.checks),
            &v.problems,
            &v.notes,
        )
        .unwrap();

        assert_eq!(payload["schema_version"], 1);
        assert_eq!(payload["command"], "auth.verify");
        assert_eq!(payload["verdict"], "unanchored");
        assert_eq!(payload["intact"], false);
        assert_eq!(payload["entries"], 3);
        assert_eq!(payload["verifies"], "export");
        // The old flags are still there, at the top level.
        for key_name in [
            "digest_ok",
            "chain_linked",
            "chain_head_ok",
            "source_chain_intact",
        ] {
            assert!(
                payload.get(key_name).is_some(),
                "the old key '{key_name}' must survive: {payload}"
            );
        }
        assert!(payload.get("notes").is_some());
    }

    /// The local-chain mode must not report export-specific checks (review
    /// F10: four flags that are really one boolean).
    #[test]
    fn test_verify_json_local_chain_omits_the_export_checks() {
        let payload = verify_payload(None, ExportVerdict::Intact, 7, None, &[], &[]).unwrap();
        assert_eq!(payload["verifies"], "local_chain");
        assert_eq!(payload["verdict"], "intact");
        for key_name in ["digest_ok", "chain_linked", "chain_head_ok", "source_chain_intact"] {
            assert!(
                payload.get(key_name).is_none(),
                "the local chain has no '{key_name}': {payload}"
            );
        }
    }

    #[test]
    fn test_verify_rejects_tampered_export() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path();
        seed_chain(loom_dir);

        let out = dir.path().join("out.json");
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();

        // Tamper with an exported field.
        let raw = std::fs::read_to_string(&out).unwrap();
        let tampered = raw.replace("provider-1", "provider-9");
        assert_ne!(raw, tampered);
        std::fs::write(&out, tampered).unwrap();

        let key = public_key_hex(&load_or_create_signing_key(loom_dir).unwrap());
        let err =
            cmd_verify(loom_dir, Some(out.to_str().unwrap()), true, Some(&key)).unwrap_err();
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
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();
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
        cmd_export(loom_dir, Some(out.to_str().unwrap()), true, false).unwrap();
        let raw = std::fs::read_to_string(&out).unwrap();
        let doc: AuditExport = serde_json::from_str(&raw).unwrap();

        assert_eq!(doc.retention.window_days, 30);
        assert_eq!(doc.retention.policy, "drop");
        assert_eq!(doc.retention.dropped_entries, 1);
        assert_eq!(doc.entries.len(), 1);
        assert_eq!(doc.entries[0].requesting_tool, "new-tool");

        // The truncated export is still a valid artifact: it verifies.
        let key = public_key_hex(&load_or_create_signing_key(loom_dir).unwrap());
        cmd_verify(loom_dir, Some(out.to_str().unwrap()), true, Some(&key)).unwrap();
    }
}
