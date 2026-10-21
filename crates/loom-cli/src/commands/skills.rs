use crate::cli::SkillsAction;
use loom_core::SkillEntry;
use loom_skills::lockfile::{self, SkillsLock};
use loom_skills::scan::{self, Severity, SkillDefect};
use std::path::Path;

fn update_lock(loom_dir: &Path) -> anyhow::Result<()> {
    let engine = loom_skills::SkillsEngine::new(loom_dir.to_str().unwrap())?;
    let registry_skills = engine.registry().list();
    let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
    let mut lock = SkillsLock::load(&lock_path)?.unwrap_or_default();
    for skill in &registry_skills {
        lock.lock_skill(skill);
    }
    lock.save(&lock_path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Install / remove.
//
// Validation is `loom_skills::scan` — the one validator and the one YAML
// parser in the tree. A Fatal defect refuses the install and leaves the
// skills directory untouched.
// ---------------------------------------------------------------------------

/// A skill name must be a single path segment.
fn is_safe_name(name: &str) -> bool {
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

/// Install a skill from a directory (`<name>/SKILL.md`) or a flat `.md` file.
///
/// Returns the installed skill's name. A Fatal defect refuses the install and
/// leaves the skills directory untouched.
fn install_skill(loom_dir: &Path, src: &Path) -> anyhow::Result<String> {
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

/// Remove an installed skill: the flat `<name>.md` file or the whole `<name>/`
/// directory tree. Nothing is left behind.
fn remove_skill(loom_dir: &Path, name: &str) -> anyhow::Result<()> {
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

// ---------------------------------------------------------------------------
// Inventory.
//
// The registry is the source of truth. The daemon and the MCP server read
// the same registry, so reading it here keeps one read path instead of a
// second, divergent one.
// ---------------------------------------------------------------------------

/// The registered skills and the defects the scan found.
fn inventory(loom_dir: &Path) -> anyhow::Result<(Vec<SkillEntry>, Vec<SkillDefect>)> {
    let engine = loom_skills::SkillsEngine::new(loom_dir.to_str().unwrap())?;
    let skills = engine.registry().list();
    let defects = engine.registry().last_scan().defects;
    Ok((skills, defects))
}

/// The human-readable listing: every registered skill, then every defect the
/// scan found. A duplicate is a warning — the collision is reported, never
/// silently resolved.
fn list_lines(skills: &[SkillEntry], defects: &[SkillDefect]) -> String {
    let mut out = String::new();
    if skills.is_empty() {
        out.push_str("No skills found.\n");
    } else {
        for skill in skills {
            out.push_str(&format!("{}  {}\n", skill.name, skill.description));
        }
    }
    for defect in defects {
        let tag = match defect.severity {
            Severity::Fatal => "✗",
            Severity::Warn => "⚠",
        };
        out.push_str(&format!("  {tag} [{}] {}\n", defect.code, defect.message));
    }
    out
}

/// The JSON listing. `count` and `skills` keep their shape: the TUI reads
/// them.
fn list_json(skills: &[SkillEntry], defects: &[SkillDefect]) -> serde_json::Value {
    let defects: Vec<serde_json::Value> = defects
        .iter()
        .map(|d| {
            serde_json::json!({
                "path": d.path.to_string_lossy(),
                "severity": match d.severity {
                    Severity::Fatal => "fatal",
                    Severity::Warn => "warn",
                },
                "code": d.code,
                "message": d.message,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": 1,
        "command": "skills.list",
        "count": skills.len(),
        "skills": skills,
        "defects": defects,
    })
}

pub async fn run(loom_dir: &Path, action: &SkillsAction) -> anyhow::Result<()> {
    match action {
        SkillsAction::List { json, locked } => {
            let (registry_skills, defects) = inventory(loom_dir)?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list_json(&registry_skills, &defects))?
                );
            } else {
                print!("{}", list_lines(&registry_skills, &defects));
            }
            if *locked {
                let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
                match SkillsLock::load(&lock_path) {
                    Ok(Some(lock)) => {
                        if lock.is_clean(&registry_skills) {
                            println!("  ✓ Lock file clean");
                        } else {
                            let violations = lock.verify_against(&registry_skills);
                            for v in &violations {
                                match v.kind {
                                    lockfile::LockViolationKind::Missing => {
                                        println!(
                                            "  ✗ {}: in lock but removed from registry",
                                            v.name
                                        );
                                    }
                                    lockfile::LockViolationKind::VersionMismatch => {
                                        println!(
                                            "  ✗ {}: version mismatch (expected {}, actual {})",
                                            v.name, v.expected, v.actual
                                        );
                                    }
                                    lockfile::LockViolationKind::ContentChanged => {
                                        println!("  ✗ {}: content hash changed", v.name);
                                    }
                                    lockfile::LockViolationKind::Unlocked => {
                                        println!("  ✗ {}: not in lock file", v.name);
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => println!("  ℹ No lock file found (run `loom lock --generate`)"),
                    Err(e) => eprintln!("  ⚠ Failed to read lock file: {e}"),
                }
            }
        }
        SkillsAction::Show { name, json } => {
            let (registry_skills, defects) = inventory(loom_dir)?;

            if let Some(skill) = registry_skills.into_iter().find(|s| &s.name == name) {
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "skills.show",
                            "name": name,
                            "skill": skill
                        }))?
                    );
                } else {
                    println!("{}", serde_json::to_string_pretty(&skill)?);
                }
                return Ok(());
            }

            // Not registered: it may be on disk but refused for a defect.
            // Name the defect instead of claiming the skill does not exist.
            if let Some(defect) = defects
                .iter()
                .find(|d| d.path.to_string_lossy().contains(name.as_str()))
            {
                anyhow::bail!("Skill '{name}' is not registered: {}", defect.message);
            }
            anyhow::bail!("Skill '{name}' not found");
        }
        SkillsAction::Install { path } => {
            let src = Path::new(path);
            if !src.exists() {
                anyhow::bail!("Path does not exist: {path}");
            }
            let name = install_skill(loom_dir, src)?;
            println!("Installed skill '{name}'");
            update_lock(loom_dir)?;
        }
        SkillsAction::Remove { name } => {
            remove_skill(loom_dir, name)?;
            println!("Removed skill '{name}'");
            update_lock(loom_dir)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// A skill directory holding a SKILL.md with the given frontmatter.
    fn make_dir_skill(root: &Path, dir_name: &str, frontmatter: &str) -> PathBuf {
        let dir = root.join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("{frontmatter}\n# {dir_name}\n\nBody.\n"),
        )
        .unwrap();
        dir
    }

    const GOOD_FM: &str = "---\nname: myskill\ndescription: Use when testing skills.\n---";

    #[test]
    fn install_dir_layout_copies_the_tree() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(tmp.path(), "myskill", GOOD_FM);
        std::fs::create_dir_all(src.join("scripts")).unwrap();
        std::fs::write(src.join("scripts").join("run.sh"), "#!/bin/sh\n").unwrap();

        let name = install_skill(&loom_dir, &src).unwrap();
        assert_eq!(name, "myskill");
        assert!(loom_dir.join("skills/myskill/SKILL.md").is_file());
        assert!(loom_dir.join("skills/myskill/scripts/run.sh").is_file());
    }

    #[test]
    fn install_flat_file_lands_as_md() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = tmp.path().join("flat.md");
        std::fs::write(
            &src,
            "---\nname: flat\ndescription: Use when testing.\n---\n\nBody.\n",
        )
        .unwrap();

        let name = install_skill(&loom_dir, &src).unwrap();
        assert_eq!(name, "flat");
        assert!(loom_dir.join("skills/flat.md").is_file());
    }

    #[test]
    fn install_without_description_is_refused() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(tmp.path(), "nodesc", "---\nname: nodesc\n---");

        let err = install_skill(&loom_dir, &src).unwrap_err().to_string();
        assert!(err.contains("missing_description"), "err: {err}");
        assert!(err.contains("description"), "err: {err}");
        assert!(err.contains("nodesc/SKILL.md"), "err: {err}");
        assert!(!loom_dir.join("skills/nodesc").exists());
    }

    #[test]
    fn install_with_name_mismatch_shows_both_values() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(
            tmp.path(),
            "dirname",
            "---\nname: othername\ndescription: Use when testing.\n---",
        );

        let err = install_skill(&loom_dir, &src).unwrap_err().to_string();
        assert!(err.contains("name_mismatch"), "err: {err}");
        assert!(err.contains("othername"), "err: {err}");
        assert!(err.contains("dirname"), "err: {err}");
        assert!(!loom_dir.join("skills/othername").exists());
        assert!(!loom_dir.join("skills/dirname").exists());
    }

    #[test]
    fn install_then_remove_dir_layout_leaves_nothing_behind() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(tmp.path(), "myskill", GOOD_FM);

        install_skill(&loom_dir, &src).unwrap();
        assert!(loom_dir.join("skills/myskill").is_dir());

        remove_skill(&loom_dir, "myskill").unwrap();
        assert!(!loom_dir.join("skills/myskill").exists());
        assert!(!loom_dir.join("skills/myskill.md").exists());
    }

    #[test]
    fn remove_flat_skill() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = tmp.path().join("flat.md");
        std::fs::write(
            &src,
            "---\nname: flat\ndescription: Use when testing.\n---\n\nBody.\n",
        )
        .unwrap();

        install_skill(&loom_dir, &src).unwrap();
        remove_skill(&loom_dir, "flat").unwrap();
        assert!(!loom_dir.join("skills/flat.md").exists());
    }

    #[test]
    fn remove_missing_skill_reports_cleanly() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        std::fs::create_dir_all(loom_dir.join("skills")).unwrap();

        let err = remove_skill(&loom_dir, "nope").unwrap_err().to_string();
        assert!(err.contains("nope"), "err: {err}");
    }

    #[test]
    fn install_refuses_to_overwrite_existing_skill() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(tmp.path(), "myskill", GOOD_FM);

        install_skill(&loom_dir, &src).unwrap();
        let err = install_skill(&loom_dir, &src).unwrap_err().to_string();
        assert!(err.contains("already exists"), "err: {err}");
    }

    #[test]
    fn flat_name_mismatch_warns_but_installs() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = tmp.path().join("flat.md");
        std::fs::write(
            &src,
            "---\nname: othername\ndescription: Use when testing.\n---\n\nBody.\n",
        )
        .unwrap();

        let name = install_skill(&loom_dir, &src).unwrap();
        assert_eq!(name, "othername");
        assert!(loom_dir.join("skills/othername.md").is_file());
    }

    #[test]
    fn missing_name_falls_back_to_directory_or_stem() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = make_dir_skill(
            tmp.path(),
            "noname",
            "---\ndescription: Use when testing.\n---",
        );

        let name = install_skill(&loom_dir, &src).unwrap();
        assert_eq!(name, "noname");
        assert!(loom_dir.join("skills/noname/SKILL.md").is_file());
    }

    #[test]
    fn invalid_yaml_is_refused() {
        let tmp = tempdir().unwrap();
        let loom_dir = tmp.path().join("loom");
        let src = tmp.path().join("badyaml.md");
        std::fs::write(
            &src,
            "---\nname: badyaml\ndescription: [unclosed\n---\n\nBody.\n",
        )
        .unwrap();

        let err = install_skill(&loom_dir, &src).unwrap_err().to_string();
        assert!(err.contains("invalid_frontmatter"), "err: {err}");
        assert!(!loom_dir.join("skills/badyaml.md").exists());
    }

    #[test]
    fn unterminated_fence_is_invalid_frontmatter() {
        let tmp = tempdir().unwrap();
        let src = tmp.path().join("fence.md");
        std::fs::write(&src, "---\nname: x\ndescription: y\n").unwrap();

        let defects = scan::validate_skill_file(&src, None);
        assert!(defects
            .iter()
            .any(|d| d.code == "invalid_frontmatter" && d.severity == Severity::Fatal));
    }

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nBody.\n")
    }

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// A mixed-layout tree: one directory skill, one flat skill, one skill
    /// with no description, and one name that exists in both layouts.
    fn mixed_tree(loom_dir: &Path) {
        let skills = loom_dir.join("skills");
        write(
            &skills.join("dir-skill/SKILL.md"),
            &skill_md("dir-skill", "\"A skill in a directory\""),
        );
        write(
            &skills.join("flat-skill.md"),
            &skill_md("flat-skill", "\"A skill in a flat file\""),
        );
        write(
            &skills.join("both/SKILL.md"),
            &skill_md("both", "\"The directory copy\""),
        );
        write(&skills.join("both.md"), &skill_md("both", "\"The flat copy\""));
        // No frontmatter at all: the live `gdrive` shape.
        write(
            &skills.join("no-desc/SKILL.md"),
            "# No Description\n\nBody.\n",
        );
    }

    #[test]
    fn list_shows_every_skill() -> anyhow::Result<()> {
        let dir = tempdir()?;
        mixed_tree(dir.path());

        let (skills, defects) = inventory(dir.path())?;
        let out = list_lines(&skills, &defects);

        // Both layouts are listed.
        assert!(out.contains("dir-skill"), "dir skill missing from: {out}");
        assert!(out.contains("flat-skill"), "flat skill missing from: {out}");
        // The description is shown, not just the name.
        assert!(out.contains("A skill in a directory"));
        assert!(out.contains("A skill in a flat file"));
        Ok(())
    }

    #[test]
    fn list_reports_a_description_less_skill() -> anyhow::Result<()> {
        let dir = tempdir()?;
        mixed_tree(dir.path());

        let (skills, defects) = inventory(dir.path())?;
        let out = list_lines(&skills, &defects);

        assert!(
            out.contains("missing_description"),
            "description-less skill not reported: {out}"
        );
        // Validation annotates, it does not remove: the skill is still listed.
        assert!(
            skills.iter().any(|s| s.name == "no-desc"),
            "description-less skill dropped from the inventory"
        );
        Ok(())
    }

    #[test]
    fn list_reports_a_duplicate_pair() -> anyhow::Result<()> {
        let dir = tempdir()?;
        mixed_tree(dir.path());

        let (skills, defects) = inventory(dir.path())?;
        let out = list_lines(&skills, &defects);

        assert!(
            out.contains("duplicate_name"),
            "duplicate pair not reported: {out}"
        );
        // The name column is UNIQUE: one row for the colliding name.
        assert_eq!(skills.iter().filter(|s| s.name == "both").count(), 1);
        Ok(())
    }

    #[test]
    fn list_json_keeps_the_count_and_skills_keys() -> anyhow::Result<()> {
        let dir = tempdir()?;
        mixed_tree(dir.path());

        let (skills, defects) = inventory(dir.path())?;
        let v = list_json(&skills, &defects);

        // The TUI reads /count and /skills.
        assert_eq!(v["count"].as_u64().unwrap(), skills.len() as u64);
        assert!(v["skills"].is_array());
        assert!(v["defects"].is_array());
        Ok(())
    }
}
