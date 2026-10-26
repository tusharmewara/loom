//! Skill installation — moved out of the CLI so every caller (CLI, daemon, FFI
//! `loom_register_skill`) registers through the same path.
//!
//! Validation is [`crate::scan`] — the one validator and the one YAML parser
//! in the tree. A Fatal defect refuses the install and leaves the skills
//! directory untouched.

use crate::lockfile::{self, SkillsLock};
use crate::scan::{self, Severity, SkillDefect};
use crate::SkillsEngine;
use loom_core::SkillEntry;
use std::path::Path;

/// A skill name must be a single path segment.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Copy a directory tree, creating the destination as needed.
fn copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Rewrite the lock file so it matches the registry exactly: every installed
/// skill is locked, and a skill that is no longer installed is unlocked.
pub fn update_lock(loom_dir: &Path, skills: &[SkillEntry]) -> anyhow::Result<()> {
    let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
    let mut lock = SkillsLock::load(&lock_path)?.unwrap_or_default();
    for skill in skills {
        lock.lock_skill(skill);
    }
    let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let stale: Vec<String> = lock
        .skills
        .keys()
        .filter(|name| !names.contains(&name.as_str()))
        .cloned()
        .collect();
    for name in stale {
        lock.unlock_skill(&name);
    }
    lock.save(&lock_path)?;
    Ok(())
}

/// The name a skill is addressed by: the frontmatter `name:` if present,
/// otherwise the directory name (layout A) or the file stem (layout B).
fn effective_name(path: &Path, dir_name: Option<&str>, fallback: &str) -> String {
    if let Some(name) = scan::frontmatter_name(path) {
        return name;
    }
    match dir_name {
        Some(dir) => dir.to_string(),
        None => path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(fallback)
            .to_string(),
    }
}

/// Install a skill from a directory (`<name>/SKILL.md`) or a flat `.md` file.
///
/// Returns the installed skill's name. A Fatal defect refuses the install and
/// leaves the skills directory untouched.
pub fn install_skill(loom_dir: &Path, src: &Path) -> anyhow::Result<String> {
    let skills_dir = loom_dir.join("skills");
    std::fs::create_dir_all(&skills_dir)?;

    let (skill_file, dir_name, fallback, is_dir_layout) = if src.is_dir() {
        let skill_md = src.join("SKILL.md");
        if !skill_md.is_file() {
            anyhow::bail!(
                "'{}' is a directory without a SKILL.md — not a skill directory",
                src.display()
            );
        }
        let dir_name = src
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        (skill_md, Some(dir_name.clone()), dir_name, true)
    } else {
        if src.extension().and_then(|e| e.to_str()) != Some("md") {
            anyhow::bail!("'{}' is not a .md file — not a skill", src.display());
        }
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        (src.to_path_buf(), None, stem, false)
    };

    let defects = scan::validate_skill_file(&skill_file, dir_name.as_deref());
    let name = effective_name(&skill_file, dir_name.as_deref(), &fallback);

    let fatals: Vec<&SkillDefect> = defects
        .iter()
        .filter(|d| d.severity == Severity::Fatal)
        .collect();
    if !fatals.is_empty() {
        let detail = fatals
            .iter()
            .map(|d| format!("{} [{}]", d.message, d.code))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::bail!("Refusing to install skill '{name}': {detail}");
    }
    for warn in defects.iter().filter(|d| d.severity == Severity::Warn) {
        eprintln!("warning: {} [{}]", warn.message, warn.code);
    }

    if !is_safe_name(&name) {
        anyhow::bail!("Refusing to install skill with invalid name '{name}'");
    }

    // A skill is addressed by name and tracked in the lock file, so replacing
    // one silently would swap the user's skill out from under the agent.
    let dst_dir = skills_dir.join(&name);
    let dst_file = skills_dir.join(format!("{name}.md"));
    let existing = if dst_dir.exists() {
        Some(dst_dir.clone())
    } else if dst_file.exists() {
        Some(dst_file.clone())
    } else {
        None
    };
    if let Some(existing) = existing {
        anyhow::bail!(
            "Skill '{name}' already exists at {} — refusing to overwrite it",
            existing.display()
        );
    }

    if is_dir_layout {
        copy_dir(src, &dst_dir)?;
    } else {
        std::fs::copy(src, &dst_file)?;
    }
    Ok(name)
}

/// Install a skill AND register it: the registry is re-read from disk, the new
/// skill is upserted, and the lock file follows the registry.
///
/// This is the full `loom skills install` flow — called by the CLI and the FFI
/// `loom_register_skill` export so both land in `loom scan`.
pub fn register_skill(loom_dir: &Path, src: &Path) -> anyhow::Result<String> {
    let name = install_skill(loom_dir, src)?;
    let entry = scan::scan_skill_tree(&loom_dir.join("skills"))
        .skills
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| anyhow::anyhow!("installed skill '{name}' is not on disk"))?;
    let engine = SkillsEngine::new(loom_dir.to_str().unwrap())?;
    engine.registry().upsert(entry)?;
    let skills = engine.registry().list();
    update_lock(loom_dir, &skills)?;
    Ok(name)
}

/// Remove an installed skill from disk: the flat `<name>.md` file or the whole
/// `<name>/` directory tree. Nothing is left behind.
pub fn remove_skill(loom_dir: &Path, name: &str) -> anyhow::Result<()> {
    if !is_safe_name(name) {
        anyhow::bail!("Invalid skill name '{name}'");
    }
    let skills_dir = loom_dir.join("skills");
    let dir = skills_dir.join(name);
    let file = skills_dir.join(format!("{name}.md"));
    if dir.is_dir() {
        std::fs::remove_dir_all(&dir)?;
    } else if dir.exists() {
        std::fs::remove_file(&dir)?;
    } else if file.exists() {
        std::fs::remove_file(&file)?;
    } else {
        anyhow::bail!("Skill '{name}' not found in {}", skills_dir.display());
    }
    Ok(())
}

/// Remove a skill AND sync the registry and lock file
/// (the full `loom skills remove` flow).
pub fn unregister_skill(loom_dir: &Path, name: &str) -> anyhow::Result<()> {
    remove_skill(loom_dir, name)?;
    let engine = SkillsEngine::new(loom_dir.to_str().unwrap())?;
    let skills = engine.registry().list();
    update_lock(loom_dir, &skills)?;
    Ok(())
}
