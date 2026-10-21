//! The one place skills are read: one walk over a skills directory that
//! handles both layouts (`<name>/SKILL.md` and flat `<name>.md`), parses each
//! frontmatter with a real YAML parser, and reports every defect it finds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use loom_core::SkillEntry;
use serde_yaml::Value;
use sha2::{Digest, Sha256};

/// Which layout a skill was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `<name>/SKILL.md`
    Dir,
    /// flat `<name>.md`
    Flat,
}

impl Layout {
    /// The layout implied by the `dir_name` argument of
    /// [`validate_skill_file`]: `Some` means layout A, `None` layout B.
    fn from_dir_name(dir_name: Option<&str>) -> Self {
        match dir_name {
            Some(_) => Layout::Dir,
            None => Layout::Flat,
        }
    }
}

/// Whether a defect refuses the skill or merely reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The skill cannot work: it will be ignored by the agent.
    Fatal,
    /// The skill can work, but something is inconsistent.
    Warn,
}

/// One defect in one skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefect {
    /// The offending file.
    pub path: PathBuf,
    /// Whether the skill is refused or merely reported.
    pub severity: Severity,
    /// Stable machine-readable code, e.g. `"missing_description"`.
    pub code: &'static str,
    /// Human-readable reason, naming the file and the missing field.
    pub message: String,
}

/// The result of one walk: the skills found, and every defect found.
#[derive(Debug, Clone, Default)]
pub struct SkillScan {
    /// The skills that could be read, as registry rows.
    pub skills: Vec<SkillEntry>,
    /// Every defect in the tree.
    pub defects: Vec<SkillDefect>,
}

/// Walk `skills_dir` once, handling both layouts.
pub fn scan_skill_tree(skills_dir: &Path) -> SkillScan {
    let mut scan = SkillScan::default();

    let mut entries: Vec<PathBuf> = match std::fs::read_dir(skills_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => return scan,
    };
    entries.sort();

    // (effective name, path, parsed frontmatter) for every skill file on disk.
    let mut found: Vec<(String, PathBuf, Option<Frontmatter>)> = Vec::new();

    for path in &entries {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let dir_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let (defects, info) = inspect(&skill_md, &dir_name, Layout::Dir);
            scan.defects.extend(defects);
            let name = effective_name(&info, &dir_name);
            found.push((name, skill_md, info));
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let (defects, info) = inspect(path, &stem, Layout::Flat);
            scan.defects.extend(defects);
            let name = effective_name(&info, &stem);
            found.push((name, path.clone(), info));
        }
    }

    // Duplicate effective names: one flat `<name>.md` and one `<name>/SKILL.md`.
    let mut by_name: BTreeMap<&str, Vec<&Path>> = BTreeMap::new();
    for (name, path, _) in &found {
        by_name.entry(name.as_str()).or_default().push(path);
    }
    for (name, paths) in by_name {
        if paths.len() < 2 {
            continue;
        }
        let listed: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        for path in &paths[1..] {
            scan.defects.push(SkillDefect {
                path: (*path).to_path_buf(),
                severity: Severity::Warn,
                code: "duplicate_name",
                message: format!(
                    "duplicate skill name `{name}`: {} resolve to it",
                    listed.join(" and ")
                ),
            });
        }
    }

    // One entry per effective name: the directory layout wins over a flat
    // duplicate of the same skill, so a consumer iterating `scan.skills`
    // never double-counts a name. The collision is reported as a defect.
    let mut chosen: BTreeMap<&str, &(String, PathBuf, Option<Frontmatter>)> = BTreeMap::new();
    for entry in &found {
        match chosen.get(entry.0.as_str()) {
            Some(existing) if layout_rank(&existing.1) >= layout_rank(&entry.1) => {}
            _ => {
                chosen.insert(entry.0.as_str(), entry);
            }
        }
    }
    for (name, path, info) in chosen.into_values() {
        if let Some(info) = info {
            scan.skills.push(skill_entry(name, path, info));
        }
    }

    scan
}

/// `1` for the dir layout (`<name>/SKILL.md`), `0` for a flat `<name>.md`.
fn layout_rank(path: &Path) -> u8 {
    if path.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
        1
    } else {
        0
    }
}

/// Validate every skill in `skills_dir` (both layouts).
/// Returns *all* defects, not just the first.
pub fn validate_skill_tree(skills_dir: &Path) -> Vec<SkillDefect> {
    scan_skill_tree(skills_dir).defects
}

/// Validate one skill file. `dir_name` is the expected name for layout A
/// (`<dir>/SKILL.md`), `None` for layout B (flat `<name>.md`).
pub fn validate_skill_file(path: &Path, dir_name: Option<&str>) -> Vec<SkillDefect> {
    let layout = Layout::from_dir_name(dir_name);
    let expected = match dir_name {
        Some(d) => d.to_string(),
        None => path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string(),
    };
    inspect(path, &expected, layout).0
}

/// The `name:` in a skill file's frontmatter, if it has one.
///
/// The one YAML parser in the tree does the reading; a caller that needs the
/// effective name of a file it is about to install asks here rather than
/// parsing the file a second way.
pub fn frontmatter_name(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let (yaml, _) = split_frontmatter(&content).ok()?;
    let yaml = yaml?;
    if yaml.trim().is_empty() {
        return None;
    }
    let fields: BTreeMap<String, Value> = serde_yaml::from_str(yaml).ok()?;
    fields
        .get("name")
        .and_then(value_as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// ─── internals ───────────────────────────────────────────────────────

/// The parsed frontmatter of one skill.
#[derive(Debug, Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    tags: Vec<String>,
    ecosystem_compatible: Vec<String>,
    ecosystem_requires: Vec<String>,
    version: Option<String>,
}

fn inspect(path: &Path, expected: &str, layout: Layout) -> (Vec<SkillDefect>, Option<Frontmatter>) {
    let mut defects = Vec::new();

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        // Unreadable file: nothing to validate. The walk reports it as absent.
        Err(_) => return (defects, None),
    };

    let (yaml, body) = match split_frontmatter(&content) {
        Ok(v) => v,
        Err(msg) => {
            defects.push(defect(
                path,
                Severity::Fatal,
                "invalid_frontmatter",
                format!("{msg} in {}", path.display()),
            ));
            return (defects, None);
        }
    };

    let fields: BTreeMap<String, Value> = match yaml {
        Some(y) if !y.trim().is_empty() => match serde_yaml::from_str(y) {
            Ok(m) => m,
            Err(e) => {
                defects.push(defect(
                    path,
                    Severity::Fatal,
                    "invalid_frontmatter",
                    format!("frontmatter is not valid YAML in {}: {e}", path.display()),
                ));
                return (defects, None);
            }
        },
        _ => BTreeMap::new(),
    };

    let fm = Frontmatter {
        name: fields.get("name").and_then(value_as_str),
        description: fields.get("description").and_then(value_as_str),
        tags: fields.get("tags").map(string_list).unwrap_or_default(),
        ecosystem_compatible: fields
            .get("ecosystem_compatible")
            .map(string_list)
            .unwrap_or_default(),
        ecosystem_requires: fields
            .get("ecosystem_requires")
            .map(string_list)
            .unwrap_or_default(),
        version: fields.get("version").and_then(value_as_str),
    };

    let name = fm.name.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let description = fm
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if description.is_none() {
        defects.push(defect(
            path,
            Severity::Fatal,
            "missing_description",
            format!(
                "{} has no `description:` in its frontmatter — the skill is dropped SILENTLY at load",
                path.display()
            ),
        ));
    }

    match name {
        None => defects.push(defect(
            path,
            Severity::Warn,
            "missing_name",
            format!(
                "{} has no `name:` in its frontmatter — the file/directory name `{expected}` is used instead",
                path.display()
            ),
        )),
        Some(n) if n != expected => defects.push(defect(
            path,
            match layout {
                Layout::Dir => Severity::Fatal,
                Layout::Flat => Severity::Warn,
            },
            "name_mismatch",
            format!(
                "{}: frontmatter `name: {n}` disagrees with the {} name `{expected}`",
                path.display(),
                match layout {
                    Layout::Dir => "directory",
                    Layout::Flat => "file",
                }
            ),
        )),
        Some(_) => {}
    }

    if body.trim().is_empty() {
        defects.push(defect(
            path,
            Severity::Warn,
            "empty_body",
            format!("{} has no content after its frontmatter", path.display()),
        ));
    }

    // Referenced scripts must exist next to the skill. Only the directory
    // layout has a directory to check: a flat file has none, so a
    // `scripts/…` reference in it is not a missing script.
    if layout == Layout::Dir {
        let root = path.parent().unwrap_or(Path::new("."));
        for rel in referenced_scripts(body) {
            if !root.join(&rel).exists() {
                defects.push(defect(
                    path,
                    Severity::Warn,
                    "missing_script",
                    format!(
                        "{} references `{rel}`, which does not exist in {}",
                        path.display(),
                        root.display()
                    ),
                ));
            }
        }
    }

    (defects, Some(fm))
}

/// The name a skill is addressed by: the frontmatter `name:` if present,
/// otherwise the directory name (layout A) or the file stem (layout B).
fn effective_name(fm: &Option<Frontmatter>, fallback: &str) -> String {
    fm.as_ref()
        .and_then(|f| f.name.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn skill_entry(name: &str, path: &Path, fm: &Frontmatter) -> SkillEntry {
    let content_hash = std::fs::read(path)
        .map(|bytes| {
            let mut h = Sha256::new();
            h.update(&bytes);
            format!("{:x}", h.finalize())
        })
        .unwrap_or_default();

    SkillEntry {
        name: name.to_string(),
        description: fm
            .description
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .to_string(),
        path: path.to_string_lossy().to_string(),
        content_hash,
        tags: fm.tags.clone(),
        ecosystem_compatible: fm.ecosystem_compatible.clone(),
        ecosystem_requires: fm.ecosystem_requires.clone(),
        version: fm.version.clone().unwrap_or_else(|| "1.0.0".to_string()),
        last_modified: loom_core::rfc3339_now(),
    }
}

fn defect(path: &Path, severity: Severity, code: &'static str, message: String) -> SkillDefect {
    SkillDefect {
        path: path.to_path_buf(),
        severity,
        code,
        message,
    }
}

/// Split a skill file into its frontmatter block and its body.
///
/// `Ok((None, content))` means the file has no frontmatter at all (legal, but
/// then it has no `description:`). `Err` means the `---` fence is unterminated.
fn split_frontmatter(content: &str) -> Result<(Option<&str>, &str), &'static str> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let first_end = content.find('\n').map_or(content.len(), |i| i + 1);
    if content[..first_end].trim() != "---" {
        return Ok((None, content));
    }

    let mut offset = first_end;
    for line in content[first_end..].split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            let yaml = &content[first_end..offset];
            let body = &content[offset + line.len()..];
            return Ok((Some(yaml), body));
        }
        offset += line.len();
    }
    Err("the `---` frontmatter fence is never closed")
}

/// The scalar value of a YAML node, if it has one.
fn value_as_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn string_list(v: &Value) -> Vec<String> {
    match v {
        Value::Sequence(items) => items.iter().filter_map(value_as_str).collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// The `scripts/…` paths a skill body references.
///
/// ponytail: a reference is taken to be a file when the token carries an
/// extension, so prose like "the scripts/ directory" is not reported. Extend
/// if the corpus grows script references without extensions.
fn referenced_scripts(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(idx) = rest.find("scripts/") {
        let after = &rest[idx + "scripts/".len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || "\"'`)]>,;|".contains(c))
            .unwrap_or(after.len());
        let token = after[..end].trim_end_matches('.');
        rest = &after[end..];
        if token.is_empty() || token.contains('*') || token.contains('?') || !token.contains('.') {
            continue;
        }
        let rel = format!("scripts/{token}");
        if !out.contains(&rel) {
            out.push(rel);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn codes(defects: &[SkillDefect]) -> Vec<&'static str> {
        defects.iter().map(|d| d.code).collect()
    }

    #[test]
    fn missing_description_is_fatal() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("nosdesc/SKILL.md"),
            "---\nname: nosdesc\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(codes(&defects).contains(&"missing_description"));
        let d = defects
            .iter()
            .find(|d| d.code == "missing_description")
            .unwrap();
        assert_eq!(d.severity, Severity::Fatal);
        assert!(d.message.contains("nosdesc/SKILL.md"));
    }

    #[test]
    fn empty_description_counts_as_missing() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("empty-desc/SKILL.md"),
            "---\nname: empty-desc\ndescription: \"   \"\n---\n\n# Body\n",
        );
        assert!(codes(&validate_skill_tree(dir.path())).contains(&"missing_description"));
    }

    #[test]
    fn name_mismatch_layout_a_is_fatal() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("dirname/SKILL.md"),
            "---\nname: other\ndescription: d\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects.iter().find(|d| d.code == "name_mismatch").unwrap();
        assert_eq!(d.severity, Severity::Fatal);
        assert!(d.message.contains("other"));
        assert!(d.message.contains("dirname"));
    }

    #[test]
    fn name_mismatch_layout_b_is_warn() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("file.md"),
            "---\nname: other\ndescription: d\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects.iter().find(|d| d.code == "name_mismatch").unwrap();
        assert_eq!(d.severity, Severity::Warn);
    }

    #[test]
    fn quoted_name_is_not_a_mismatch() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("graphflow/SKILL.md"),
            "---\nname: \"graphflow\"\ndescription: d\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(!codes(&defects).contains(&"name_mismatch"));
    }

    #[test]
    fn folded_description_is_parsed() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("folded/SKILL.md"),
            "---\nname: folded\ndescription: >\n  Use when the task\n  mentions X or Y.\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(!codes(&defects).contains(&"missing_description"));
        let scan = scan_skill_tree(dir.path());
        assert_eq!(scan.skills.len(), 1);
        assert_eq!(
            scan.skills[0].description,
            "Use when the task mentions X or Y."
        );
    }

    #[test]
    fn literal_description_is_parsed() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("literal/SKILL.md"),
            "---\nname: literal\ndescription: |\n  line one\n  line two\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(!codes(&defects).contains(&"missing_description"));
        let scan = scan_skill_tree(dir.path());
        // A literal block keeps its line breaks; the trailing newline is trimmed.
        assert_eq!(scan.skills[0].description, "line one\nline two");
    }

    #[test]
    fn quoted_description_is_parsed() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("quoted/SKILL.md"),
            "---\nname: quoted\ndescription: \"Use when the user asks for it\"\n---\n\n# Body\n",
        );
        let scan = scan_skill_tree(dir.path());
        assert_eq!(scan.skills[0].description, "Use when the user asks for it");
    }

    #[test]
    fn folded_literal_and_quoted_descriptions_parse_identically() {
        let dir = tempdir().unwrap();
        // The three YAML description styles, one expected text.
        write(
            &dir.path().join("folded/SKILL.md"),
            "---\nname: folded\ndescription: >\n  Use when the task\n  mentions X or Y.\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("literal/SKILL.md"),
            "---\nname: literal\ndescription: |\n  Use when the task mentions X or Y.\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("quoted/SKILL.md"),
            "---\nname: quoted\ndescription: \"Use when the task mentions X or Y.\"\n---\n\n# Body\n",
        );

        let scan = scan_skill_tree(dir.path());
        assert_eq!(scan.skills.len(), 3);
        for skill in &scan.skills {
            assert_eq!(
                skill.description, "Use when the task mentions X or Y.",
                "{} parsed differently",
                skill.name
            );
        }
    }

    #[test]
    fn unknown_frontmatter_keys_do_not_invalidate() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("extra/SKILL.md"),
            "---\nname: extra\ndescription: d\ntriggers:\n  - \"use extra\"\nargument-hint: \"[a|b]\"\nlicense: MIT\n---\n\n# Body\n",
        );
        assert!(validate_skill_tree(dir.path()).is_empty());
    }

    #[test]
    fn invalid_yaml_is_fatal() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("broken/SKILL.md"),
            "---\nname: broken\ndescription: [unclosed\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects
            .iter()
            .find(|d| d.code == "invalid_frontmatter")
            .unwrap();
        assert_eq!(d.severity, Severity::Fatal);
    }

    #[test]
    fn unterminated_fence_is_invalid_frontmatter() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("unterminated/SKILL.md"),
            "---\nname: unterminated\ndescription: d\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects
            .iter()
            .find(|d| d.code == "invalid_frontmatter")
            .unwrap();
        assert_eq!(d.severity, Severity::Fatal);
        assert!(d.message.contains("never closed"));
    }

    #[test]
    fn no_frontmatter_at_all_is_reported_as_missing_description() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("gdrive/SKILL.md"),
            "# Google Drive\n\nUpload and download files.\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(codes(&defects).contains(&"missing_description"));
        assert!(!codes(&defects).contains(&"invalid_frontmatter"));
    }

    #[test]
    fn empty_body_is_warn() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("hollow/SKILL.md"),
            "---\nname: hollow\ndescription: d\n---\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects.iter().find(|d| d.code == "empty_body").unwrap();
        assert_eq!(d.severity, Severity::Warn);
    }

    #[test]
    fn missing_script_is_reported() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("opendesign/SKILL.md"),
            "---\nname: opendesign\ndescription: d\n---\n\nRun `node \"$SKILL_DIR/scripts/list-design-templates.mjs\"`.\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects.iter().find(|d| d.code == "missing_script").unwrap();
        // A missing script is reported, not fatal: it must not block an
        // install of an otherwise working skill.
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.message.contains("scripts/list-design-templates.mjs"));
    }

    #[test]
    fn a_flat_skill_has_no_script_directory_to_check() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("flat.md"),
            "---\nname: flat\ndescription: d\n---\n\nRun `scripts/thing.sh`.\n",
        );
        let defects = validate_skill_tree(dir.path());
        assert!(!codes(&defects).contains(&"missing_script"));
    }

    #[test]
    fn existing_script_is_not_reported() {
        let dir = tempdir().unwrap();
        write(&dir.path().join("ok/scripts/run.sh"), "#!/bin/sh\n");
        write(
            &dir.path().join("ok/SKILL.md"),
            "---\nname: ok\ndescription: d\n---\n\nRun `scripts/run.sh`.\n",
        );
        assert!(!codes(&validate_skill_tree(dir.path())).contains(&"missing_script"));
    }

    #[test]
    fn duplicate_flat_dir_pair_is_reported() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("dup/SKILL.md"),
            "---\nname: dup\ndescription: d\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("dup.md"),
            "---\nname: dup\ndescription: d\n---\n\n# Body\n",
        );
        let defects = validate_skill_tree(dir.path());
        let d = defects.iter().find(|d| d.code == "duplicate_name").unwrap();
        assert_eq!(d.severity, Severity::Warn);
        assert!(d.message.contains("dup"));
    }

    #[test]
    fn dir_and_flat_together_in_one_tree() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("one/SKILL.md"),
            "---\nname: one\ndescription: d\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("two.md"),
            "---\nname: two\ndescription: d\n---\n\n# Body\n",
        );
        let scan = scan_skill_tree(dir.path());
        assert_eq!(scan.skills.len(), 2);
        let mut names: Vec<&str> = scan.skills.iter().map(|s| s.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["one", "two"]);
    }

    #[test]
    fn nested_md_inside_a_skill_dir_is_not_a_skill() {
        let dir = tempdir().unwrap();
        // The `ui-craft` shape: a skill dir that also holds a copy of its own
        // markdown. The nested copy is not a skill of its own.
        write(
            &dir.path().join("ui-craft/SKILL.md"),
            "---\nname: ui-craft\ndescription: d\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("ui-craft/ui-craft.md"),
            "---\nname: ui-craft\ndescription: d\n---\n\n# Body\n",
        );
        let scan = scan_skill_tree(dir.path());
        assert_eq!(scan.skills.len(), 1);
        assert_eq!(scan.skills[0].name, "ui-craft");
        assert!(!codes(&scan.defects).contains(&"duplicate_name"));
    }

    #[test]
    fn scan_skills_emits_registry_ready_entries() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ready/SKILL.md");
        write(
            &path,
            "---\nname: ready\ndescription: Use when testing.\n---\n\n# Body\n",
        );
        let scan = scan_skill_tree(dir.path());
        let entry = &scan.skills[0];
        assert_eq!(entry.name, "ready");
        assert_eq!(entry.description, "Use when testing.");
        assert_eq!(entry.path, path.to_string_lossy());
        assert!(!entry.version.is_empty(), "the version is synthesized");
        assert_eq!(entry.content_hash.len(), 64, "SHA-256 of the file bytes");
    }

    #[test]
    fn empty_tree_returns_no_defects() {
        let dir = tempdir().unwrap();
        assert!(validate_skill_tree(dir.path()).is_empty());
        assert!(scan_skill_tree(dir.path()).skills.is_empty());
    }

    #[test]
    fn missing_tree_has_no_defects() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(validate_skill_tree(&missing).is_empty());
    }

    #[test]
    fn validate_skill_file_checks_one_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("solo/SKILL.md");
        write(&path, "---\nname: other\ndescription: d\n---\n\n# Body\n");
        let defects = validate_skill_file(&path, Some("solo"));
        assert!(codes(&defects).contains(&"name_mismatch"));
        let defects = validate_skill_file(&path, None);
        assert!(codes(&defects).contains(&"name_mismatch"));
    }

    #[test]
    fn validate_skill_tree_returns_every_defect() {
        let dir = tempdir().unwrap();
        write(
            &dir.path().join("a/SKILL.md"),
            "---\nname: b\n---\n\n# Body\n",
        );
        write(
            &dir.path().join("c.md"),
            "---\nname: d\ndescription: e\n---\n",
        );
        let defects = validate_skill_tree(dir.path());
        // a: missing_description + name_mismatch; c: empty_body + name_mismatch
        let mut found: Vec<&str> = codes(&defects);
        found.sort();
        assert_eq!(
            found,
            vec![
                "empty_body",
                "missing_description",
                "name_mismatch",
                "name_mismatch"
            ],
            "every defect is returned, not just the first"
        );
    }
}
