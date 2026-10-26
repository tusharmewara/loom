use crate::cli::PluginAction;
use loom_core::plugin_registry::PluginRegistry;
use loom_core::ConfigReader;
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &PluginAction) -> anyhow::Result<()> {
    let registry = PluginRegistry::new(loom_dir.to_str().unwrap());

    match action {
        PluginAction::List { json } => {
            let plugins = registry.list()?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "plugin.list",
                        "count": plugins.len(),
                        "plugins": plugins,
                    }))?
                );
            } else {
                if plugins.is_empty() {
                    println!("No plugins installed.");
                } else {
                    println!("Installed plugins:");
                    for p in &plugins {
                        let version = &p.version;
                        let desc = p.description.as_deref().unwrap_or("(no description)");
                        println!("  {} v{}  — {}", p.name, version, desc);
                    }
                    println!("\n{} plugin(s) installed.", plugins.len());
                }
            }
        }
        PluginAction::Show { name, json } => {
            let plugin = registry.get(name)?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "plugin.show",
                        "plugin": plugin,
                    }))?
                );
            } else {
                println!("Plugin: {}", plugin.name);
                println!("  Version:     {}", plugin.version);
                if let Some(d) = &plugin.description {
                    println!("  Description: {}", d);
                }
                if let Some(a) = &plugin.author {
                    println!("  Author:      {}", a);
                }
                if let Some(h) = &plugin.homepage {
                    println!("  Homepage:    {}", h);
                }
                if let Some(r) = &plugin.repository {
                    println!("  Repository:  {}", r);
                }
                if let Some(u) = &plugin.tool_toml_url {
                    println!("  Tool URL:    {}", u);
                }
                if let Some(c) = &plugin.checksum {
                    println!("  Checksum:    {}", c);
                }
                if let Some(t) = &plugin.installed_at {
                    println!("  Installed:   {}", t);
                }
            }
        }
        PluginAction::Search { query, json } => {
            let results = registry.search(query)?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "plugin.search",
                        "query": query,
                        "count": results.len(),
                        "results": results,
                    }))?
                );
            } else {
                if results.is_empty() {
                    println!("No plugins found matching '{}'", query);
                } else {
                    println!("Plugin search results for '{}':", query);
                    for p in &results {
                        let desc = p.description.as_deref().unwrap_or("(no description)");
                        println!("  {} v{}  — {}", p.name, p.version, desc);
                    }
                }
            }
        }
        PluginAction::Install {
            name,
            url,
            version,
            verify,
        } => {
            println!("Installing plugin '{}' from {}...", name, url);
            // security.require_signature defaults true (ADR 0005: verify is
            // the default path); install/update refuse unsigned content
            // with the opt-out named. `ConfigReader` embeds built-in
            // defaults, so the fallback also defaults true on load failure.
            let require_sig = ConfigReader::new(loom_dir.to_str().unwrap())
                .load()
                .map(|c| c.security.require_signature)
                .unwrap_or(true);
            let manifest =
                registry.install_from_url(name, url, version.as_deref(), require_sig)?;
            // Verification + pinning happened inside the registry gate
            // before anything was written; the flag remains for verbose
            // confirmation on opted-out installs.
            if manifest.signature.is_some() {
                println!("  ✓ Signature verified");
            } else if *verify {
                println!(
                    "  ⚠ Plugin unsigned — installed under verification opt-out ([security] require_signature = false)"
                );
            }
            println!(
                "✓ Plugin '{}' v{} installed",
                manifest.name, manifest.version
            );
        }
        PluginAction::Register { name, path } => {
            let tool_path = Path::new(path);
            if !tool_path.exists() {
                anyhow::bail!("Tool TOML file not found: {}", path);
            }
            println!("Registering plugin '{}' from {}...", name, path);
            let manifest = registry.register_local(name, tool_path)?;
            println!(
                "✓ Plugin '{}' v{} registered",
                manifest.name, manifest.version
            );
        }
        PluginAction::Update { name } => {
            println!("Updating plugin '{}'...", name);
            let require_sig = ConfigReader::new(loom_dir.to_str().unwrap())
                .load()
                .map(|c| c.security.require_signature)
                .unwrap_or(true);
            let manifest = registry.update(name, require_sig)?;
            println!(
                "✓ Plugin '{}' updated to v{}",
                manifest.name, manifest.version
            );
        }
        PluginAction::Remove { name } => {
            let plugin = registry.get(name)?;
            println!("Removing plugin '{}' v{}...", plugin.name, plugin.version);
            registry.remove(name)?;
            println!("✓ Plugin '{}' removed", name);
        }
    }
    Ok(())
}
