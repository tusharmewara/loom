use anyhow::{Context, Result};
use comfy_table::Table;
use loom_discovery::DiscoveryEngine;
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct AgenticToolSpec {
    name: &'static str,
    binaries: &'static [&'static str],
}

const AGENTIC_TOOLS: &[AgenticToolSpec] = &[
    AgenticToolSpec {
        name: "loom",
        binaries: &["loom", "loomd"],
    },
    AgenticToolSpec {
        name: "codex",
        binaries: &["codex"],
    },
    AgenticToolSpec {
        name: "claude-code",
        binaries: &["claude"],
    },
    AgenticToolSpec {
        name: "gemini-cli",
        binaries: &["gemini"],
    },
    AgenticToolSpec {
        name: "cursor-cli",
        binaries: &["cursor-agent", "agent"],
    },
    AgenticToolSpec {
        name: "aider",
        binaries: &["aider"],
    },
    AgenticToolSpec {
        name: "opencode",
        binaries: &["opencode"],
    },
    AgenticToolSpec {
        name: "crush",
        binaries: &["crush"],
    },
    AgenticToolSpec {
        name: "amp",
        binaries: &["amp"],
    },
    AgenticToolSpec {
        name: "goose",
        binaries: &["goose"],
    },
];

#[derive(Debug, Clone, serde::Serialize)]
struct AgenticToolHit {
    tool: &'static str,
    binary: String,
    path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
struct McpSourceHit {
    source: String,
    path: PathBuf,
    server_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
struct SkillSourceHit {
    source: String,
    path: PathBuf,
    skill_count: usize,
}

#[derive(Debug, Default, serde::Serialize)]
struct SystemAudit {
    tool_hits: Vec<AgenticToolHit>,
    tool_config_count: usize,
    mcp_sources: Vec<McpSourceHit>,
    skill_sources: Vec<SkillSourceHit>,
    warnings: Vec<String>,
}

impl SystemAudit {
    fn total_mcp_servers(&self) -> usize {
        self.mcp_sources.iter().map(|s| s.server_count).sum()
    }

    fn total_skills(&self) -> usize {
        self.skill_sources.iter().map(|s| s.skill_count).sum()
    }
}

/// `loom scan`
///
/// Runs the two-phase discovery engine and prints a formatted table
/// showing available layers, resource counts, and any warnings.
pub async fn run(loom_dir: &str, json: bool) -> Result<()> {
    // Check that the ecosystem is initialized
    let agents_path = std::path::Path::new(loom_dir);
    if !agents_path.exists() {
        anyhow::bail!("Ecosystem not initialized at ~/.loom/. Run 'loom init' first.");
    }

    let engine = DiscoveryEngine::new(loom_dir);

    // Ensure the snapshot schema exists (idempotent)
    engine
        .snapshot()
        .initialize_schema()
        .context("Failed to initialize snapshot schema")?;

    // Phase 1: fast SQLite snapshot read (for ecosystem version, layers)
    let phase1 = engine.phase_1().context("Phase 1 discovery failed")?;

    // Phase 2: deferred validation + snapshot sync (discovers new resources)
    let phase2 = engine.phase_2().context("Phase 2 discovery failed")?;

    // Compute live counts from registries so display is always accurate,
    // regardless of when the snapshot was last populated.
    let live_counts = engine.live_counts();

    let system_audit = scan_system(agents_path);
    let mut total = 0usize;
    for count in live_counts.values() {
        total += count;
    }

    let mut warnings = phase2.warnings.clone();
    warnings.extend(system_audit.warnings.clone());

    if json {
        print_scan_json(loom_dir, &phase1, &phase2, total, &system_audit, &warnings)?;
        return Ok(());
    }

    // ── Build ecosystem output table ─────────────────────────────
    let mut table = Table::new();
    table.set_header(vec!["Layer", "Count", "Status"]);
    for (layer, count) in &live_counts {
        table.add_row(vec![
            layer.as_str(),
            &count.to_string(),
            if *count > 0 { "active" } else { "empty" },
        ]);
    }
    table.add_row(vec!["ecosystem", &phase1.ecosystem_version, "info"]);

    println!("{table}");
    println!();
    println!("Total resources: {total}");
    println!(
        "Phase 1: {}µs | Phase 2: {}µs | Validated: {} | Conflicts resolved: {}",
        phase1.phase_1_duration_us,
        phase2.phase_2_duration_us,
        phase2.entries_validated,
        phase2.conflicts_resolved,
    );
    print_system_audit(&system_audit);

    if !warnings.is_empty() {
        for w in &warnings {
            eprintln!("⚠ {w}");
        }
    }

    Ok(())
}

/// Build live resource counts from registries (not from the snapshot).
fn print_scan_json(
    loom_dir: &str,
    phase1: &loom_discovery::Phase1Result,
    phase2: &loom_discovery::Phase2Result,
    total_resources: usize,
    system_audit: &SystemAudit,
    warnings: &[String],
) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "command": "scan",
            "loom_dir": loom_dir,
            "ecosystem": {
                "version": phase1.ecosystem_version,
                "resource_counts": phase1.resource_counts,
                "total_resources": total_resources,
                "layers": phase1.layers,
                "phase_1_duration_us": phase1.phase_1_duration_us,
                "phase_2": {
                    "phase_2_duration_us": phase2.phase_2_duration_us,
                    "entries_validated": phase2.entries_validated,
                    "entries_updated": phase2.entries_updated,
                    "conflicts_resolved": phase2.conflicts_resolved
                }
            },
            "system_audit": system_audit,
            "warnings": warnings
        }))?
    );
    Ok(())
}

fn print_system_audit(audit: &SystemAudit) {
    let mut summary = Table::new();
    summary.set_header(vec!["System Check", "Count", "Status"]);
    summary.add_row(vec![
        "agentic tools in PATH",
        &audit.tool_hits.len().to_string(),
        if audit.tool_hits.is_empty() {
            "none detected"
        } else {
            "detected"
        },
    ]);
    summary.add_row(vec![
        "tool registry configs",
        &audit.tool_config_count.to_string(),
        if audit.tool_config_count == 0 {
            "none"
        } else {
            "detected"
        },
    ]);
    summary.add_row(vec![
        "mcp config sources",
        &audit.mcp_sources.len().to_string(),
        if audit.mcp_sources.is_empty() {
            "none"
        } else {
            "detected"
        },
    ]);
    summary.add_row(vec![
        "declared mcp servers",
        &audit.total_mcp_servers().to_string(),
        if audit.total_mcp_servers() == 0 {
            "none"
        } else {
            "detected"
        },
    ]);
    summary.add_row(vec![
        "skill sources",
        &audit.skill_sources.len().to_string(),
        if audit.skill_sources.is_empty() {
            "none"
        } else {
            "detected"
        },
    ]);
    summary.add_row(vec![
        "discovered skills",
        &audit.total_skills().to_string(),
        if audit.total_skills() == 0 {
            "none"
        } else {
            "detected"
        },
    ]);

    println!();
    println!("System audit:");
    println!("{summary}");

    if !audit.tool_hits.is_empty() {
        let mut tools = Table::new();
        tools.set_header(vec!["Tool", "Binary", "Path"]);
        for hit in &audit.tool_hits {
            tools.add_row(vec![
                hit.tool,
                hit.binary.as_str(),
                hit.path.to_string_lossy().as_ref(),
            ]);
        }
        println!();
        println!("Detected agentic tools:");
        println!("{tools}");
    }

    if !audit.mcp_sources.is_empty() {
        let mut mcp = Table::new();
        mcp.set_header(vec!["MCP Source", "Servers", "Path"]);
        for source in &audit.mcp_sources {
            mcp.add_row(vec![
                source.source.as_str(),
                &source.server_count.to_string(),
                source.path.to_string_lossy().as_ref(),
            ]);
        }
        println!();
        println!("Detected MCP sources:");
        println!("{mcp}");
    }

    if !audit.skill_sources.is_empty() {
        let mut skills = Table::new();
        skills.set_header(vec!["Skill Source", "Skills", "Path"]);
        for source in &audit.skill_sources {
            skills.add_row(vec![
                source.source.as_str(),
                &source.skill_count.to_string(),
                source.path.to_string_lossy().as_ref(),
            ]);
        }
        println!();
        println!("Detected skill sources:");
        println!("{skills}");
    }
}

fn scan_system(loom_dir: &Path) -> SystemAudit {
    let mut audit = SystemAudit::default();
    let home = home_dir();

    audit.tool_hits = detect_agentic_tools(None);
    audit.tool_config_count = count_tool_configs(loom_dir, &mut audit.warnings);
    audit.mcp_sources = detect_mcp_sources(loom_dir, home.as_deref(), &mut audit.warnings);
    audit.skill_sources = detect_skill_sources(loom_dir, home.as_deref(), &mut audit.warnings);

    audit
}

fn count_tool_configs(loom_dir: &Path, warnings: &mut Vec<String>) -> usize {
    let tools_dir = loom_dir.join("tools");
    if !tools_dir.exists() {
        return 0;
    }

    let entries = match std::fs::read_dir(&tools_dir) {
        Ok(entries) => entries,
        Err(e) => {
            warnings.push(format!(
                "Failed to read tool registry directory {}: {}",
                tools_dir.display(),
                e
            ));
            return 0;
        }
    };

    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .count()
}

fn detect_agentic_tools(path_override: Option<&OsStr>) -> Vec<AgenticToolHit> {
    let mut hits = Vec::new();

    for spec in AGENTIC_TOOLS {
        let found = spec.binaries.iter().find_map(|binary| {
            find_executable(binary, path_override)
                .map(|path| (binary.to_string(), path))
        });

        if let Some((binary, path)) = found {
            hits.push(AgenticToolHit {
                tool: spec.name,
                binary,
                path,
            });
        }
    }

    hits
}

fn detect_mcp_sources(
    loom_dir: &Path,
    home: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Vec<McpSourceHit> {
    let mut hits = Vec::new();
    let mut seen = BTreeSet::new();

    let mut file_candidates: Vec<(String, PathBuf)> = vec![
        ("loom project".to_string(), loom_dir.join(".mcp.json")),
        ("loom mcp_config".to_string(), loom_dir.join("mcp_config.json")),
    ];

    if let Some(home) = home {
        file_candidates.extend([
            ("codex global".to_string(), home.join(".codex/config.toml")),
            (
                "codex xdg".to_string(),
                home.join(".config/codex/config.toml"),
            ),
            ("claude global".to_string(), home.join(".claude.json")),
            (
                "claude xdg".to_string(),
                home.join(".config/claude/claude.json"),
            ),
            (
                "claude desktop (macos)".to_string(),
                home.join("Library/Application Support/Claude/claude_desktop_config.json"),
            ),
            (
                "cursor workspace".to_string(),
                home.join(".cursor/mcp.json"),
            ),
            (
                "cursor user".to_string(),
                home.join(".config/Cursor/User/mcp.json"),
            ),
            (
                "windsurf".to_string(),
                home.join(".codeium/windsurf/mcp_config.json"),
            ),
            ("gemini".to_string(), home.join(".gemini/settings.json")),
            (
                "project-global".to_string(),
                home.join(".mcp.json"),
            ),
        ]);
    }

    for (label, path) in file_candidates {
        if !path.exists() || !path.is_file() {
            continue;
        }

        let canonical_key = canonical_key(&path);
        if !seen.insert(canonical_key) {
            continue;
        }

        match count_mcp_servers_in_file(&path) {
            Ok(server_count) => hits.push(McpSourceHit {
                source: label,
                path,
                server_count,
            }),
            Err(e) => warnings.push(format!(
                "Failed to parse MCP config {}: {}",
                path.display(),
                e
            )),
        }
    }

    let mut dir_candidates = vec![("loom mcp directory".to_string(), loom_dir.join("mcp"))];
    if let Some(home) = home {
        dir_candidates.extend([
            (
                "global mcp directory".to_string(),
                home.join(".config/mcp"),
            ),
            ("dot-mcp directory".to_string(), home.join(".mcp")),
        ]);
    }

    for (label, dir) in dir_candidates {
        if !dir.exists() || !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                warnings.push(format!("Failed to read MCP dir {}: {}", dir.display(), e));
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            if ext != "json" && ext != "toml" {
                continue;
            }

            let canonical_key = canonical_key(&path);
            if !seen.insert(canonical_key) {
                continue;
            }

            match count_mcp_servers_in_file(&path) {
                Ok(server_count) => hits.push(McpSourceHit {
                    source: label.clone(),
                    path,
                    server_count,
                }),
                Err(e) => warnings.push(format!(
                    "Failed to parse MCP file {}: {}",
                    path.display(),
                    e
                )),
            }
        }
    }

    hits
}

fn detect_skill_sources(
    loom_dir: &Path,
    home: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Vec<SkillSourceHit> {
    let mut hits = Vec::new();
    let mut seen = BTreeSet::new();

    let mut skill_dir_candidates: Vec<(String, PathBuf)> =
        vec![("loom skills".to_string(), loom_dir.join("skills"))];

    if let Some(home) = home {
        skill_dir_candidates.extend([
            ("codex skills".to_string(), home.join(".codex/skills")),
            ("claude skills".to_string(), home.join(".claude/skills")),
            (
                "claude xdg skills".to_string(),
                home.join(".config/claude/skills"),
            ),
            ("gemini skills".to_string(), home.join(".gemini/skills")),
            ("cursor skills".to_string(), home.join(".cursor/skills")),
            (
                "windsurf skills".to_string(),
                home.join(".codeium/windsurf/skills"),
            ),
        ]);
    }

    for (source, dir) in skill_dir_candidates {
        if !dir.exists() || !dir.is_dir() {
            continue;
        }

        let canonical_key = canonical_key(&dir);
        if !seen.insert(canonical_key) {
            continue;
        }

        match count_skills_in_directory(&dir) {
            Ok(skill_count) => hits.push(SkillSourceHit {
                source,
                path: dir,
                skill_count,
            }),
            Err(e) => warnings.push(format!(
                "Failed to scan skill directory {}: {}",
                dir.display(),
                e
            )),
        }
    }

    hits
}

fn count_skills_in_directory(dir: &Path) -> Result<usize> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?;
    let mut count = 0usize;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() && path.join("SKILL.md").exists() {
            count += 1;
            continue;
        }

        if path.is_file()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("SKILL.md"))
        {
            count += 1;
        }
    }

    Ok(count)
}

fn count_mcp_servers_in_file(path: &Path) -> Result<usize> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if ext == "json" {
        let json: serde_json::Value = serde_json::from_str(&content)
            .with_context(|| format!("invalid JSON in {}", path.display()))?;
        return Ok(count_mcp_servers_in_json_value(&json));
    }

    if ext == "toml" {
        let toml_value: toml::Value = toml::from_str(&content)
            .with_context(|| format!("invalid TOML in {}", path.display()))?;
        return Ok(count_mcp_servers_in_toml_value(&toml_value));
    }

    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
        return Ok(count_mcp_servers_in_json_value(&json));
    }

    let toml_value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("unsupported MCP format in {}", path.display()))?;
    Ok(count_mcp_servers_in_toml_value(&toml_value))
}

fn count_mcp_servers_in_json_value(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Object(map) => {
            let mut total = 0;
            for (key, nested) in map {
                if is_mcp_server_key(key) {
                    if let serde_json::Value::Object(servers) = nested {
                        total += servers.len();
                    }
                }
                total += count_mcp_servers_in_json_value(nested);
            }
            total
        }
        serde_json::Value::Array(items) => items
            .iter()
            .map(count_mcp_servers_in_json_value)
            .sum(),
        _ => 0,
    }
}

fn count_mcp_servers_in_toml_value(value: &toml::Value) -> usize {
    match value {
        toml::Value::Table(map) => {
            let mut total = 0;
            for (key, nested) in map {
                if is_mcp_server_key(key) {
                    if let toml::Value::Table(servers) = nested {
                        total += servers.len();
                    }
                }
                total += count_mcp_servers_in_toml_value(nested);
            }
            total
        }
        toml::Value::Array(items) => items
            .iter()
            .map(count_mcp_servers_in_toml_value)
            .sum(),
        _ => 0,
    }
}

fn is_mcp_server_key(key: &str) -> bool {
    matches!(key, "mcp_servers" | "mcpServers" | "servers")
}

fn find_executable(binary: &str, path_override: Option<&OsStr>) -> Option<PathBuf> {
    let path_env = path_override
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var_os("PATH"))?;

    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(binary);
        if is_executable(&candidate) {
            return Some(candidate);
        }

        #[cfg(windows)]
        {
            let pathext = std::env::var_os("PATHEXT")
                .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            for ext in std::env::split_paths(&pathext) {
                let ext = ext.to_string_lossy();
                let ext = ext.trim_matches(';');
                if ext.is_empty() {
                    continue;
                }
                let candidate = dir.join(format!("{binary}{ext}"));
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn canonical_key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init;
    use loom_discovery::registry::SnapshotDb;

    #[tokio::test]
    async fn test_scan_empty_ecosystem() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Initialize first
        init::run(false, false, loom_dir).await.unwrap();

        // Scan should succeed with empty counts
        run(loom_dir, false).await.unwrap();

        // Verify by checking snapshot directly
        let db = SnapshotDb::new(loom_dir);
        let counts = db.count_resources().unwrap();
        assert!(counts.values().all(|c| *c == 0));
    }

    #[tokio::test]
    async fn test_scan_with_resources() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Pre-populate resources
        let skill_dir = dir.path().join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(skill_dir.join("SKILL.md"), "# Test Skill").unwrap();
        std::fs::create_dir_all(dir.path().join("tools")).unwrap();
        std::fs::write(
            dir.path().join("tools").join("cursor.toml"),
            "[tool]\nname = \"cursor\"\nversion = \"0.45.0\"\n",
        )
        .unwrap();

        // Init with import
        init::run(true, false, loom_dir).await.unwrap();

        // Scan should find resources
        run(loom_dir, false).await.unwrap();

        // Verify counts
        let db = SnapshotDb::new(loom_dir);
        let counts = db.count_resources().unwrap();
        assert_eq!(*counts.get("skills").unwrap_or(&0), 1);
        assert_eq!(*counts.get("tools").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_scan_without_init_proceeds_with_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let loom_dir = dir.path().to_str().unwrap();

        // Should succeed because scan creates schema on the fly
        run(loom_dir, false).await.unwrap();

        // Snapshot should have schema with empty counts
        let db = SnapshotDb::new(loom_dir);
        let counts = db.count_resources().unwrap();
        assert!(counts.values().all(|c| *c == 0));
    }

    #[test]
    fn test_count_mcp_servers_json_nested() {
        let value = serde_json::json!({
            "mcpServers": {
                "filesystem": {"command": "npx"},
                "github": {"url": "https://example.com"}
            },
            "projects": {
                "/repo": {
                    "mcpServers": {
                        "postgres": {"command": "uvx"}
                    }
                }
            }
        });

        assert_eq!(count_mcp_servers_in_json_value(&value), 3);
    }

    #[test]
    fn test_count_mcp_servers_toml_nested() {
        let value: toml::Value = toml::from_str(
            r#"
            [mcp_servers]
            filesystem = { command = "npx" }

            [profile.default.mcp_servers]
            github = { command = "node" }
            "#,
        )
        .unwrap();

        assert_eq!(count_mcp_servers_in_toml_value(&value), 2);
    }

    #[test]
    fn test_detect_agentic_tools_from_path_override() {
        let dir = tempfile::tempdir().unwrap();
        let codex_path = dir.path().join("codex");
        std::fs::write(&codex_path, "#!/bin/sh\necho codex\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&codex_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&codex_path, perms).unwrap();
        }

        let hits = detect_agentic_tools(Some(dir.path().as_os_str()));
        assert!(hits.iter().any(|hit| hit.tool == "codex"));
    }
}
