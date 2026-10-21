//! Preset tool adapters built on [`loom_generic_adapter::GenericAdapter`].
//!
//! | Module | Tool |
//! |--------|------|
//! | `jcode` | jcode CLI |
//! | `crush` | Crush (HTTP control plane) |
//! | `claude_code` | Claude Code (`~/.claude/`) |
//! | `opencode` | OpenCode |
//! | `goose` | Goose |
//! | `codex` | Codex |
//! | `cursor-cli` | Cursor CLI |
//! | `gemini-cli` | Gemini CLI |
//! | `aider` | Aider |
//! | `kiro` | Kiro (VS Code fork) |

pub mod aider;
pub mod claude_code;
pub mod codex;
pub mod crush;
pub mod cursor_cli;
pub mod gemini_cli;
pub mod github_copilot;
pub mod goose;
pub mod jcode;
pub mod juno;
pub mod kiro;
pub mod opencode;

use anyhow::Result;
use loom_core::{DebugAdapterInfo, LspServerInfo, RoutingDecision};
use loom_generic_adapter::config::{DebugSection, LspSection, ToolToml};
use loom_skills::edit::make_replace_edit;
use loom_skills::{validate_skill_tree, Severity, SkillDefect};
use std::path::Path;

/// Shared adapter operations for ecosystem-registered tools.
pub trait LoomToolAdapter: Send + Sync {
    fn tool_name(&self) -> &'static str;
    fn default_toml(&self, home: &str) -> ToolToml;
    fn apply_routing_decision(&self, decision: &RoutingDecision) -> Result<()>;
    fn sync_from_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport>;

    /// Push Loom-managed resources (MCP servers, skills) into this tool's native
    /// filesystem locations so the tool can use them without `loom exec` wrapping.
    ///
    /// The default implementation uses [`sync_mcp_to_native`] and [`sync_skills_to_native`].
    fn sync_to_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport> {
        let mut report = SyncReport::default();

        // Push shared MCP servers into the tool's native config
        if let Some(mcp) = self.default_toml(home).mcp {
            if let Some(ref config_path) = mcp.config_path {
                match sync_mcp_to_native(loom_dir, config_path) {
                    Ok(n) => {
                        report.mcp_servers_synced = n;
                        if n > 0 {
                            report.notes.push(format!(
                                "synced {n} shared MCP server(s) to {}",
                                config_path
                            ));
                        }
                    }
                    Err(e) => report
                        .notes
                        .push(format!("MCP sync to {} failed: {e}", config_path)),
                }
            }
        }

        // Push shared skills into the tool's native skills directory
        if let Some(skills) = self.default_toml(home).skills {
            if let Some(ref native_dir) = skills.native_dir {
                match sync_skills_to_native(loom_dir, native_dir) {
                    Ok(skill_report) => {
                        report.skills_synced = skill_report.skills_synced;
                        report.notes.extend(skill_report.notes);
                        if skill_report.skills_synced > 0 {
                            report.notes.push(format!(
                                "synced {} shared skill(s) to {}",
                                skill_report.skills_synced, native_dir
                            ));
                        }
                    }
                    Err(e) => report
                        .notes
                        .push(format!("skills sync to {} failed: {e}", native_dir)),
                }
            }
        }

        Ok(report)
    }

    /// Returns LSP server configuration for this adapter, if any.
    fn lsp_config(&self, home: &str) -> Option<LspSection> {
        self.default_toml(home).lsp
    }

    /// Returns LSP servers this adapter supports.
    fn lsp_servers(&self, home: &str) -> Vec<LspServerInfo> {
        self.lsp_config(home)
            .map(|cfg| {
                cfg.servers
                    .into_iter()
                    .map(|s| LspServerInfo {
                        language: s.language,
                        command: s.command,
                        args: s.args,
                        file_extensions: s.file_extensions,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the best LSP server for a given file extension.
    fn resolve_lsp_server(&self, extension: &str, home: &str) -> Option<LspServerInfo> {
        self.lsp_servers(home)
            .into_iter()
            .find(|s| s.file_extensions.iter().any(|e| e == extension))
    }

    /// Returns debug adapter configuration for this adapter, if any.
    fn debug_config(&self, home: &str) -> Option<DebugSection> {
        self.default_toml(home).debug
    }

    /// Returns debug adapters this adapter supports.
    fn debug_adapters(&self, home: &str) -> Vec<DebugAdapterInfo> {
        self.debug_config(home)
            .map(|cfg| {
                cfg.adapters
                    .into_iter()
                    .map(|a| DebugAdapterInfo {
                        language: a.language,
                        command: a.command,
                        args: a.args,
                        name: a.name,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve the best debug adapter for a given language.
    fn resolve_debug_adapter(&self, language: &str, home: &str) -> Option<DebugAdapterInfo> {
        self.debug_adapters(home)
            .into_iter()
            .find(|a| a.language == language)
    }

    /// Apply a hashline edit to a file, with stale detection and fallback.
    /// Returns the edited file content (caller writes to disk).
    fn apply_hashline_edit(
        &self,
        file_path: &str,
        start: usize,
        end: usize,
        new_content: &str,
        context_lines: &str,
    ) -> Result<Option<String>> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Cannot read {file_path}: {e}"))?;
        let edit = make_replace_edit(&content, start, end, new_content, context_lines)?;
        edit.apply(&content)
            .map_err(|e| anyhow::anyhow!("Hashline edit failed: {e}"))
    }
}

/// Write shared MCP servers from Loom's registry into a tool-native MCP config file.
/// Merges with any existing config — tool-specific servers are preserved.
pub fn sync_mcp_to_native(loom_dir: &str, native_config_path: &str) -> Result<usize> {
    let path = Path::new(native_config_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Read shared MCP servers from Loom's registry
    let mgr = loom_mcp::McpManager::new(loom_dir)?;
    let registry = mgr.registry();
    let shared_servers = registry.filter_shared(true)?;

    if shared_servers.is_empty() {
        return Ok(0);
    }

    // Read existing config (if any)
    let mut existing: serde_json::Value = if path.exists() {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
    } else {
        serde_json::Value::Object(Default::default())
    };

    // Merge shared servers under a "loom_shared" key so they're clearly namespaced
    let obj = existing.as_object_mut().unwrap();
    let mut loom_servers = serde_json::Map::new();
    for entry in &shared_servers {
        let server_obj = serde_json::json!({
            "command": entry.transport.command,
            "args": entry.transport.args,
            "env": entry.transport.env,
        });
        loom_servers.insert(entry.id.clone(), server_obj);
    }

    if !loom_servers.is_empty() {
        // Write into a "loom_shared" key so the tool can distinguish loom-managed servers
        // Also write into mcpServers if it doesn't already have that entry
        let mcp_servers = obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        if let Some(mcp_obj) = mcp_servers.as_object_mut() {
            for (id, server) in &loom_servers {
                // Only add if not already defined by the tool (tool-specific takes precedence)
                mcp_obj.entry(id).or_insert_with(|| server.clone());
            }
        }
    }

    // Write back
    let content = serde_json::to_string_pretty(&existing)?;
    std::fs::write(path, content)?;

    Ok(shared_servers.len())
}

/// Symlink Loom's shared skills directory into a tool-native skills directory.
/// Existing skill files in the tool's directory are preserved (loom skills are merged in).
///
/// A skill with a Fatal defect (missing `description:`, invalid frontmatter, a
/// name that disagrees with its directory, a missing referenced script) is NOT
/// linked: the agent would ignore it silently. The skip is named in the
/// report notes, by path and defect code.
pub fn sync_skills_to_native(loom_dir: &str, native_skills_dir: &str) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let loom_skills = Path::new(loom_dir).join("skills");
    if !loom_skills.exists() {
        return Ok(report);
    }

    let native_dir = Path::new(native_skills_dir);
    std::fs::create_dir_all(native_dir)?;

    let defects = validate_skill_tree(&loom_skills);

    let mut count = 0;
    for entry in std::fs::read_dir(&loom_skills)? {
        let entry = entry?;
        let name = entry.file_name();
        let loom_skill_path = entry.path();

        // Only process directories with SKILL.md
        if !loom_skill_path.is_dir() {
            continue;
        }
        let skill_file = loom_skill_path.join("SKILL.md");
        if !skill_file.exists() {
            continue;
        }

        let native_skill_path = native_dir.join(&name);

        // Validate before linking: a Fatal defect refuses the skill, and a
        // stale link made before the gate existed is pulled.
        let (linkable, notes) = skill_link_decision(
            &defects,
            &skill_file,
            &name.to_string_lossy(),
            Some((&native_skill_path, &loom_skill_path)),
        );
        report.notes.extend(notes);
        if !linkable {
            continue;
        }

        // If the tool already has this skill, skip (native takes precedence)
        if native_skill_path.exists() {
            continue;
        }

        // Create a symlink (Unix) or copy (fallback)
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(&loom_skill_path, &native_skill_path).is_ok() {
                count += 1;
            }
        }
        #[cfg(not(unix))]
        {
            if copy_dir_recursive(&loom_skill_path, &native_skill_path).is_ok() {
                count += 1;
            }
        }
    }

    report.skills_synced = count;
    Ok(report)
}

/// The defects of every skill file in `skills_dir`.
///
/// One walk, one rule: both the layout-A path (`sync_skills_to_native`) and the
/// layout-B path (`inject_shared_skills`) ask this question.
pub fn skill_defects_by_file(skills_dir: &Path) -> Vec<SkillDefect> {
    validate_skill_tree(skills_dir)
}

/// Decide whether one skill file may be linked into a native tool directory.
///
/// Returns `(linkable, notes)`. A Fatal defect (missing `description:`, invalid
/// frontmatter, a name that disagrees with its directory) refuses the skill and
/// names the reason; a Warn never blocks the sync but is still reported.
///
/// `stale_link` is the `(native_path, loom_source_path)` pair of the link this
/// sync would have made. When the skill is refused, a link Loom itself made
/// earlier is removed — otherwise the defecting skill keeps reaching the tool
/// through the old link.
pub fn skill_link_decision(
    defects: &[SkillDefect],
    file: &Path,
    name: &str,
    stale_link: Option<(&Path, &Path)>,
) -> (bool, Vec<String>) {
    let mut notes = Vec::new();
    let mut linkable = true;

    for d in defects.iter().filter(|d| d.path == file) {
        match d.severity {
            Severity::Fatal => {
                linkable = false;
                notes.push(format!(
                    "skipped skill '{name}': {} — {}",
                    d.code, d.message
                ));
            }
            Severity::Warn => notes.push(format!("skill '{name}': {} — {}", d.code, d.message)),
        }
    }

    if !linkable {
        if let Some((native_path, source)) = stale_link {
            remove_stale_link(native_path, source, name, &mut notes);
        }
    }

    (linkable, notes)
}

/// Remove a link Loom itself made to a skill that is now refused.
///
/// Only a symlink pointing at the loom source is removed; a skill the user
/// placed in the native directory is never touched.
fn remove_stale_link(native_path: &Path, source: &Path, name: &str, notes: &mut Vec<String>) {
    #[cfg(unix)]
    {
        let is_link = std::fs::symlink_metadata(native_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        if !is_link {
            return;
        }
        let points_at_source = std::fs::read_link(native_path)
            .map(|t| t == source)
            .unwrap_or(false)
            || std::fs::canonicalize(native_path)
                .ok()
                .zip(std::fs::canonicalize(source).ok())
                .map(|(a, b)| a == b)
                .unwrap_or(false);
        if points_at_source && std::fs::remove_file(native_path).is_ok() {
            notes.push(format!(
                "removed the stale link to refused skill '{name}' at {}",
                native_path.display()
            ));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (native_path, source, name, notes);
    }
}

#[cfg(not(unix))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

pub fn write_tool_registration(
    adapter: &dyn LoomToolAdapter,
    loom_dir: &str,
    home: &str,
) -> Result<()> {
    let tools_dir = std::path::Path::new(loom_dir).join("tools");
    std::fs::create_dir_all(&tools_dir)?;
    let path = tools_dir.join(format!("{}.toml", adapter.tool_name()));
    let toml = toml::to_string_pretty(&adapter.default_toml(home))?;
    std::fs::write(&path, toml)?;
    tracing::info!(tool = adapter.tool_name(), path = %path.display(), "registered tool");
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    pub skills_imported: usize,
    pub skills_synced: usize,
    pub mcp_servers_imported: usize,
    pub mcp_servers_synced: usize,
    pub providers_imported: usize,
    pub notes: Vec<String>,
}

pub fn adapter_for(tool: &str) -> Option<Box<dyn LoomToolAdapter>> {
    match tool {
        "jcode" => Some(Box::new(jcode::JcodeAdapter)),
        "crush" => Some(Box::new(crush::CrushAdapter)),
        "claude-code" | "claude_code" => Some(Box::new(claude_code::ClaudeCodeAdapter)),
        "opencode" => Some(Box::new(opencode::OpencodeAdapter)),
        "goose" => Some(Box::new(goose::GooseAdapter)),
        "codex" => Some(Box::new(codex::CodexAdapter)),
        "cursor-cli" | "cursor" => Some(Box::new(cursor_cli::CursorCliAdapter)),
        "gemini-cli" | "gemini" => Some(Box::new(gemini_cli::GeminiCliAdapter)),
        "kiro" => Some(Box::new(kiro::KiroAdapter)),
        "juno" => Some(Box::new(juno::JunoAdapter)),
        "github-copilot" | "copilot" => Some(Box::new(github_copilot::GitHubCopilotAdapter)),
        "aider" => Some(Box::new(aider::AiderAdapter)),
        _ => None,
    }
}

pub fn supported_tools() -> &'static [&'static str] {
    &[
        "jcode",
        "crush",
        "claude-code",
        "opencode",
        "goose",
        "codex",
        "cursor",
        "gemini-cli",
        "aider",
        "kiro",
        "juno",
        "github-copilot",
    ]
}

pub fn sync_tools_dir(loom_dir: &str) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let mgr = loom_mcp::McpManager::new(loom_dir)?;
    let registry = mgr.registry();
    let n = loom_mcp::import::import_from_tools_dir(std::path::Path::new(loom_dir), &|entry| {
        registry.upsert(&entry)?;
        Ok(())
    })?;
    report.mcp_servers_imported = n;
    if n > 0 {
        report
            .notes
            .push(format!("imported {n} MCP server(s) from tools/"));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// A skill with frontmatter the agent can actually load.
    fn valid_skill(name: &str) -> String {
        format!(
            "---\nname: {name}\ndescription: >\n  Does {name} things.\n---\n\n# {name}\n\nBody.\n"
        )
    }

    #[test]
    fn valid_dir_skill_is_linked() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        write(
            &loom.join("skills").join("alpha").join("SKILL.md"),
            &valid_skill("alpha"),
        );

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert_eq!(report.skills_synced, 1);
        assert!(native.join("alpha").exists());
    }

    #[test]
    fn fatal_skill_is_not_linked_and_is_named() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        // No description: — the agent drops the skill silently at load.
        write(
            &loom.join("skills").join("gdrive").join("SKILL.md"),
            "# Google Drive\n\nUpload and download files.\n",
        );

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert_eq!(report.skills_synced, 0);
        assert!(!native.join("gdrive").exists());
        let note = report
            .notes
            .iter()
            .find(|n| n.contains("gdrive"))
            .expect("the skip must be named in the notes");
        assert!(note.contains("missing_description"), "note: {note}");
        assert!(note.contains("gdrive/SKILL.md"), "note: {note}");
    }

    #[test]
    fn warn_does_not_block_the_link_but_is_reported() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        // No `name:` — a Warn, the directory name is used instead.
        write(
            &loom.join("skills").join("beta").join("SKILL.md"),
            "---\ndescription: Does beta things.\n---\n\n# Beta\n\nBody.\n",
        );

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert_eq!(report.skills_synced, 1);
        assert!(native.join("beta").exists());
        assert!(
            report.notes.iter().any(|n| n.contains("missing_name")),
            "notes: {:?}",
            report.notes
        );
    }

    #[test]
    fn one_fatal_skill_does_not_abort_the_rest_of_the_sync() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        write(
            &loom.join("skills").join("broken").join("SKILL.md"),
            "# No frontmatter\n",
        );
        write(
            &loom.join("skills").join("good").join("SKILL.md"),
            &valid_skill("good"),
        );

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert_eq!(report.skills_synced, 1);
        assert!(native.join("good").exists());
        assert!(!native.join("broken").exists());
    }

    #[test]
    fn a_stale_link_to_a_refused_skill_is_removed() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        let skill_dir = loom.join("skills").join("gdrive");
        write(
            &skill_dir.join("SKILL.md"),
            "# Google Drive\n\nNo frontmatter.\n",
        );

        // A link a previous (ungated) sync made.
        fs::create_dir_all(&native).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&skill_dir, native.join("gdrive")).unwrap();
        assert!(native.join("gdrive").exists());

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert_eq!(report.skills_synced, 0);
        assert!(
            !native.join("gdrive").exists(),
            "the stale link to a refused skill must be gone"
        );
        assert!(
            report.notes.iter().any(|n| n.contains("removed")),
            "notes: {:?}",
            report.notes
        );
    }

    #[test]
    fn a_native_skill_the_user_placed_is_never_removed() {
        let scratch = tempfile::tempdir().unwrap();
        let loom = scratch.path().join("loom");
        let native = scratch.path().join("native");
        write(
            &loom.join("skills").join("gdrive").join("SKILL.md"),
            "# Google Drive\n\nNo frontmatter.\n",
        );
        // The tool's own copy — not a link Loom made.
        write(
            &native.join("gdrive").join("SKILL.md"),
            "# Google Drive\n\nTool copy.\n",
        );

        let report =
            sync_skills_to_native(loom.to_str().unwrap(), native.to_str().unwrap()).unwrap();

        assert!(
            native.join("gdrive").exists(),
            "the tool's own skill was removed"
        );
        assert!(report
            .notes
            .iter()
            .any(|n| n.contains("missing_description")));
    }
}
