use crate::cli::SkillsAction;
use loom_ipc::IpcRequest;
use loom_skills::lockfile::{self, SkillsLock};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
// The Phase A validator (card A1, `crates/loom-skills/src/scan.rs`) has not
// landed yet, so the defect check below is local. It follows the shape the A1
// API is specified to take — `validate_skill_file(path, dir_name)` returning
// defects that carry a path, a stable code and a message — so swapping in
// `loom_skills::scan::validate_skill_file` later is a one-line change.
// ---------------------------------------------------------------------------

/// How serious a defect is: `Fatal` refuses the install, `Warn` does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Severity {
    Fatal,
    Warn,
}

/// One defect in one skill file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillDefect {
    path: PathBuf,
    severity: Severity,
    /// Stable machine-readable code, e.g. `missing_description`.
    code: &'static str,
    /// Human-readable reason naming the file and the field.
    message: String,
}

/// Why a skill's frontmatter could not be used.
#[derive(Debug)]
enum FrontmatterError {
    /// The file has no `---` block at all.
    Absent,
    /// The block is present but unusable.
    Invalid(String),
}

/// Parse the `---` fenced frontmatter of a skill file into its top-level keys.
///
/// Handles the styles the live corpus uses: plain scalars, folded (`>`),
/// literal (`|`) and quoted. Unknown keys are ignored, not rejected.
fn parse_frontmatter(content: &str) -> Result<BTreeMap<String, String>, FrontmatterError> {
    let mut lines = content.lines();
    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return Err(FrontmatterError::Absent),
    }
    let mut block = Vec::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        block.push(line);
    }
    if !closed {
        return Err(FrontmatterError::Invalid(
            "unterminated `---` frontmatter fence".into(),
        ));
    }
    Ok(parse_yaml_block(&block))
}

/// Read the top-level `key: value` pairs out of a YAML block.
fn parse_yaml_block(block: &[&str]) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut i = 0;
    while i < block.len() {
        let line = block[i];
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') || line.starts_with(char::is_whitespace) {
            i += 1;
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = key.trim().to_string();
        let value = rest.trim();
        // The indented lines that follow belong to this key.
        let mut cont: Vec<&str> = Vec::new();
        i += 1;
        while i < block.len()
            && (block[i].trim().is_empty() || block[i].starts_with(char::is_whitespace))
        {
            if !block[i].trim().is_empty() {
                cont.push(block[i].trim());
            }
            i += 1;
        }
        if matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            // Block scalar: `>` folds the lines into one, `|` keeps the breaks.
            let joined = if value.starts_with('>') {
                cont.join(" ")
            } else {
                cont.join("\n")
            };
            map.insert(key, joined);
            continue;
        }
        // A plain scalar may also continue on the next line(s).
        let mut scalar = plain_scalar(value);
        if !cont.is_empty() {
            let tail = cont.join(" ");
            scalar = if scalar.is_empty() {
                tail
            } else {
                format!("{scalar} {tail}")
            };
        }
        map.insert(key, scalar);
    }
    map
}

/// A plain YAML scalar: quoted values lose their quotes, a trailing `#`
/// comment is dropped.
fn plain_scalar(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 {
        let bytes = v.as_bytes();
        let quote = bytes[0];
        if (quote == b'"' || quote == b'\'') && bytes[v.len() - 1] == quote {
            let inner = &v[1..v.len() - 1];
            return if quote == b'"' {
                inner
                    .replace("\\\"", "\"")
                    .replace("\\n", "\n")
                    .replace("\\\\", "\\")
            } else {
                inner.replace("''", "'")
            };
        }
    }
    match v.find(" #") {
        Some(idx) => v[..idx].trim().to_string(),
        None => v.to_string(),
    }
}

/// Check one skill file for defects. `dir_name` is the expected name for the
/// directory layout (`<name>/SKILL.md`); `None` for a flat `.md` file, whose
/// file stem is the expected name.
fn validate_skill_content(path: &Path, content: &str, dir_name: Option<&str>) -> Vec<SkillDefect> {
    let mut defects = Vec::new();
    let fm = match parse_frontmatter(content) {
        Ok(fm) => fm,
        Err(FrontmatterError::Absent) => {
            defects.push(SkillDefect {
                path: path.to_path_buf(),
                severity: Severity::Fatal,
                code: "missing_description",
                message: format!(
                    "{} has no frontmatter, so it has no 'description:' — the agent ignores the skill",
                    path.display()
                ),
            });
            defects.push(SkillDefect {
                path: path.to_path_buf(),
                severity: Severity::Warn,
                code: "missing_name",
                message: format!(
                    "{} has no 'name:' — the directory/file name is used instead",
                    path.display()
                ),
            });
            return defects;
        }
        Err(FrontmatterError::Invalid(msg)) => {
            defects.push(SkillDefect {
                path: path.to_path_buf(),
                severity: Severity::Fatal,
                code: "invalid_frontmatter",
                message: format!("{}: {msg}", path.display()),
            });
            return defects;
        }
    };

    match fm.get("description").map(|d| d.trim()) {
        None | Some("") => defects.push(SkillDefect {
            path: path.to_path_buf(),
            severity: Severity::Fatal,
            code: "missing_description",
            message: format!(
                "{} has no 'description:' in its frontmatter — the agent drops the skill silently",
                path.display()
            ),
        }),
        Some(_) => {}
    }

    match fm.get("name").map(|n| n.trim()).filter(|n| !n.is_empty()) {
        None => defects.push(SkillDefect {
            path: path.to_path_buf(),
            severity: Severity::Warn,
            code: "missing_name",
            message: format!(
                "{} has no 'name:' — the directory/file name is used instead",
                path.display()
            ),
        }),
        Some(name) => {
            let expected = dir_name.or_else(|| path.file_stem().and_then(|s| s.to_str()));
            if let Some(expected) = expected {
                if name != expected {
                    let kind = if dir_name.is_some() {
                        "directory"
                    } else {
                        "file"
                    };
                    defects.push(SkillDefect {
                        path: path.to_path_buf(),
                        // The directory name is the addressing key for every
                        // consumer, so a mismatch there is fatal; a flat file
                        // has no directory to contradict.
                        severity: if dir_name.is_some() {
                            Severity::Fatal
                        } else {
                            Severity::Warn
                        },
                        code: "name_mismatch",
                        message: format!(
                            "{} declares name '{name}' but its {kind} name is '{expected}'",
                            path.display()
                        ),
                    });
                }
            }
        }
    }
    defects
}

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

    let content = std::fs::read_to_string(&skill_file)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", skill_file.display()))?;

    let defects = validate_skill_content(&skill_file, &content, dir_name.as_deref());
    let fm_name = parse_frontmatter(&content)
        .ok()
        .and_then(|fm| fm.get("name").map(|n| n.trim().to_string()))
        .filter(|n| !n.is_empty());
    let name = fm_name.unwrap_or_else(|| fallback.clone());

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

pub async fn run(loom_dir: &Path, action: &SkillsAction) -> anyhow::Result<()> {
    match action {
        SkillsAction::List { json, locked } => {
            let req = IpcRequest::new("skills.list");
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    if let Some(data) = resp.data {
                        if let Some(skills) = data.get("skills") {
                            if *json {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&serde_json::json!({
                                        "schema_version": 1,
                                        "command": "skills.list",
                                        "count": skills.as_array().map(|a| a.len()).unwrap_or(0),
                                        "skills": skills
                                    }))?
                                );
                            } else {
                                println!("{}", serde_json::to_string_pretty(skills)?);
                            }
                            return Ok(());
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("Daemon unavailable ({e}); listing local skills dir"),
            }

            let skills_dir = loom_dir.join("skills");
            let mut skill_names = Vec::new();
            if skills_dir.exists() {
                for entry in std::fs::read_dir(&skills_dir)? {
                    let name = entry?.file_name().to_string_lossy().to_string();
                    skill_names.push(name.clone());
                }
            }
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "skills.list",
                        "count": skill_names.len(),
                        "skills": skill_names
                    }))?
                );
            } else {
                if skill_names.is_empty() {
                    println!("No skills found.");
                } else {
                    for name in &skill_names {
                        println!("{}", name);
                    }
                }
            }
            if *locked {
                let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
                match SkillsLock::load(&lock_path) {
                    Ok(Some(lock)) => {
                        let loom_str = loom_dir.to_str().unwrap();
                        if let Ok(engine) = loom_skills::SkillsEngine::new(loom_str) {
                            let skills = engine.registry().list();
                            if lock.is_clean(&skills) {
                                println!("  ✓ Lock file clean");
                            } else {
                                let violations = lock.verify_against(&skills);
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
                    }
                    Ok(None) => println!("  ℹ No lock file found (run `loom lock --generate`)"),
                    Err(e) => eprintln!("  ⚠ Failed to read lock file: {e}"),
                }
            }
        }
        SkillsAction::Show { name, json } => {
            let req =
                IpcRequest::new("skills.show").with_params(serde_json::json!({ "name": name }));
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    if let Some(data) = resp.data {
                        if *json {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "command": "skills.show",
                                    "name": name,
                                    "skill": data
                                }))?
                            );
                        } else {
                            println!("{}", serde_json::to_string_pretty(&data)?);
                        }
                        return Ok(());
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("Daemon unavailable ({e})"),
            }

            let path = loom_dir.join("skills").join(name);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                if *json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "command": "skills.show",
                            "name": name,
                            "content": content
                        }))?
                    );
                } else {
                    println!("{}", content);
                }
            } else {
                anyhow::bail!("Skill '{name}' not found");
            }
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
    fn frontmatter_styles_all_parse() {
        let folded = parse_frontmatter("---\nname: a\ndescription: >\n  one\n  two\n---").unwrap();
        assert_eq!(folded.get("description").unwrap(), "one two");

        let literal = parse_frontmatter("---\nname: b\ndescription: |\n  one\n  two\n---").unwrap();
        assert_eq!(literal.get("description").unwrap(), "one\ntwo");

        let quoted =
            parse_frontmatter("---\nname: \"c\"\ndescription: \"quoted value\"\n---").unwrap();
        assert_eq!(quoted.get("name").unwrap(), "c");
        assert_eq!(quoted.get("description").unwrap(), "quoted value");
    }

    #[test]
    fn unterminated_fence_is_invalid_frontmatter() {
        let defects = validate_skill_content(
            Path::new("/tmp/x/SKILL.md"),
            "---\nname: x\ndescription: y\n",
            Some("x"),
        );
        assert!(defects
            .iter()
            .any(|d| d.code == "invalid_frontmatter" && d.severity == Severity::Fatal));
    }
}
