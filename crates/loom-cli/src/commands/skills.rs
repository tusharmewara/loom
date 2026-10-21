use crate::cli::SkillsAction;
use loom_core::SkillEntry;
use loom_skills::lockfile::{self, SkillsLock};
use loom_skills::scan::{Severity, SkillDefect};
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

/// The registered skills and the defects the scan found.
///
/// The registry is the source of truth. The daemon and the MCP server read
/// the same registry, so reading it here keeps one read path instead of a
/// second, divergent one.
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
                println!("{}", serde_json::to_string_pretty(&list_json(&registry_skills, &defects))?);
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
            let name = src
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dst = loom_dir.join("skills").join(src.file_name().unwrap());
            std::fs::create_dir_all(loom_dir.join("skills"))?;
            std::fs::copy(src, &dst)?;
            println!("Installed skill '{name}'");
            update_lock(loom_dir)?;
        }
        SkillsAction::Remove { name } => {
            let path = loom_dir.join("skills").join(name);
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Removed skill '{name}'");
                update_lock(loom_dir)?;
            } else {
                anyhow::bail!("Skill '{name}' not found");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
        // A skill with no description is not registered.
        assert!(!out.contains("no-desc  "), "unregistered skill listed: {out}");
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
        assert!(skills.iter().all(|s| s.name != "no-desc"));
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
