//! Trigger lines: one always-on line per skill, so an agent knows the skill exists.
//!
//! The cue is supplied by the caller through `cue_of` — the caller owns the YAML
//! parse and hands over the *parsed* description. This module never parses YAML
//! and never regexes raw file text: upstream's `autotrigger` regex
//! (`harness2/core/tree.py:192-213`) takes the literal `>` of a folded
//! description, which is the bug this module exists to avoid
//! (see `docs/LOOM_HARNESS2_ANALYSIS.md` §12.3).

use std::collections::BTreeMap;
use std::path::Path;

use crate::scan::{Severity, SkillDefect};
use loom_core::SkillEntry;

/// Maximum total characters of generated trigger text.
pub const TRIGGER_MAX_CHARS: usize = 4_000;

/// Maximum characters of the cue in one trigger line. A longer cue is cut at a
/// word boundary and marked with `…` — the truncation is visible in the line.
pub const CUE_MAX_CHARS: usize = 200;

/// One line of always-on text per skill, so the agent knows the skill exists.
#[derive(Debug, Clone, Default)]
pub struct TriggerSet {
    /// Effective skill name -> the trigger line.
    pub lines: BTreeMap<String, String>,
    /// Skills that got no line: empty cue, a YAML artifact, or over the cap.
    /// Nothing is ever dropped silently.
    pub not_triggered: Vec<String>,
}

/// Build trigger lines for the given skills.
///
/// `cue_of` supplies the "when to use this" phrase for a skill — by default the
/// parsed `description:`. One line per skill, in name order; the total length is
/// capped at [`TRIGGER_MAX_CHARS`] and a line that does not fit is dropped and
/// reported in [`TriggerSet::not_triggered`].
///
/// Only a valid skill gets a line: a skill with a Fatal defect cannot work,
/// so it is not triggered — it is named in [`TriggerSet::not_triggered`] instead
/// of vanishing. A skill with only Warn defects still gets its line.
pub fn trigger_lines(
    skills: &[SkillEntry],
    defects: &[SkillDefect],
    cue_of: impl Fn(&SkillEntry) -> &str,
) -> TriggerSet {
    let mut set = TriggerSet::default();
    let mut total = 0usize;

    let fatal_paths: std::collections::HashSet<String> = defects
        .iter()
        .filter(|d| d.severity == Severity::Fatal)
        .map(|d| d.path.to_string_lossy().into_owned())
        .collect();

    for (name, entry) in pick_one_per_skill(skills) {
        if fatal_paths.contains(&entry.path) {
            set.not_triggered.push(name);
            continue;
        }
        let Some(cue) = clean_cue(cue_of(entry)) else {
            set.not_triggered.push(name);
            continue;
        };
        let line = format!("When {cue}, read {}.", skill_path(entry));
        // +1 for the newline the line occupies in the generated text.
        let cost = line.chars().count() + 1;
        if total + cost > TRIGGER_MAX_CHARS {
            set.not_triggered.push(name);
            continue;
        }
        total += cost;
        set.lines.insert(name, line);
    }

    set
}

/// One entry per effective skill name, dir layout winning over a flat duplicate
/// of the same skill. A `BTreeMap` keeps the output order deterministic.
fn pick_one_per_skill(skills: &[SkillEntry]) -> Vec<(String, &SkillEntry)> {
    let mut chosen: BTreeMap<&str, &SkillEntry> = BTreeMap::new();
    for entry in skills {
        match chosen.get(entry.name.as_str()) {
            Some(existing) if layout_rank(existing) >= layout_rank(entry) => {}
            _ => {
                chosen.insert(entry.name.as_str(), entry);
            }
        }
    }
    chosen
        .into_iter()
        .map(|(n, e)| (n.to_string(), e))
        .collect()
}

/// `1` for the dir layout (`<name>/SKILL.md`), `0` for a flat `<name>.md`.
fn layout_rank(entry: &SkillEntry) -> u8 {
    if is_dir_layout(&entry.path) {
        1
    } else {
        0
    }
}

/// The skill file as the agent will see it, relative to the skills directory.
fn skill_path(entry: &SkillEntry) -> String {
    let path = Path::new(&entry.path);
    if is_dir_layout(&entry.path) {
        let dir = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned());
        match dir {
            Some(dir) => format!("skills/{dir}/SKILL.md"),
            None => format!("skills/{}/SKILL.md", entry.name),
        }
    } else {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty());
        match stem {
            Some(stem) => format!("skills/{stem}.md"),
            None => format!("skills/{}.md", entry.name),
        }
    }
}

/// A path ending in `SKILL.md` is the dir layout; anything else is flat.
fn is_dir_layout(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .map(|n| n == "SKILL.md")
        .unwrap_or(false)
}

/// Turn a raw cue into a single-line sentence fragment, or `None` when there is
/// no usable cue.
fn clean_cue(raw: &str) -> Option<String> {
    let s = strip_quotes(raw.trim());
    // A folded (`>`) or literal (`|`) description keeps its line breaks; the
    // trigger line is one line, so collapse the whitespace.
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() || is_yaml_artifact(&s) {
        return None;
    }
    let s = s
        .trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\''])
        .trim();
    if s.is_empty() {
        return None;
    }
    Some(truncate_cue(s))
}

/// Strip one layer of surrounding quotes.
fn strip_quotes(s: &str) -> &str {
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// A YAML block indicator is not a cue — upstream's regex emits it verbatim.
fn is_yaml_artifact(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| matches!(c, '>' | '|' | '-' | '+'))
}

/// Cut an over-long cue at a word boundary. The `…` marks the cut: the cue is
/// never truncated mid-word, which would produce a false cue.
fn truncate_cue(cue: &str) -> String {
    if cue.chars().count() <= CUE_MAX_CHARS {
        return cue.to_string();
    }
    let head: String = cue.chars().take(CUE_MAX_CHARS).collect();
    let cut = match head.rfind(' ') {
        Some(i) => head[..i].trim_end(),
        None => head.trim_end(),
    };
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, description: &str, path: &str) -> SkillEntry {
        SkillEntry {
            name: name.to_string(),
            description: description.to_string(),
            path: path.to_string(),
            content_hash: String::new(),
            tags: Vec::new(),
            ecosystem_compatible: Vec::new(),
            ecosystem_requires: Vec::new(),
            version: "0.1.0".to_string(),
            last_modified: String::new(),
        }
    }

    fn dir_entry(name: &str, description: &str) -> SkillEntry {
        entry(name, description, &format!("/skills/{name}/SKILL.md"))
    }

    fn flat_entry(name: &str, description: &str) -> SkillEntry {
        entry(name, description, &format!("/skills/{name}.md"))
    }

    fn desc_of(e: &SkillEntry) -> &str {
        &e.description
    }

    /// The trigger set for skills with no defects.
    fn lines(skills: &[SkillEntry], cue_of: impl Fn(&SkillEntry) -> &str) -> TriggerSet {
        trigger_lines(skills, &[], cue_of)
    }

    #[test]
    fn a_fatal_skill_is_named_but_not_triggered() {
        let bad = dir_entry("bad", "cue");
        let defect = SkillDefect {
            path: bad.path.clone().into(),
            severity: Severity::Fatal,
            code: "name_mismatch",
            message: "mismatch".to_string(),
        };
        let set = trigger_lines(&[bad, dir_entry("ok", "cue")], &[defect], desc_of);
        assert!(!set.lines.contains_key("bad"));
        assert!(set.not_triggered.contains(&"bad".to_string()));
        assert!(set.lines.contains_key("ok"));
    }

    #[test]
    fn a_warn_only_skill_still_gets_a_line() {
        let ok = dir_entry("ok", "cue");
        let defect = SkillDefect {
            path: ok.path.clone().into(),
            severity: Severity::Warn,
            code: "missing_script",
            message: "missing script".to_string(),
        };
        let set = trigger_lines(&[ok], &[defect], desc_of);
        assert!(set.lines.contains_key("ok"));
        assert!(set.not_triggered.is_empty());
    }

    #[test]
    fn plain_description_yields_the_expected_cue() {
        let set = lines(&[dir_entry("a", "Use when writing Rust code")], desc_of);
        assert_eq!(
            set.lines.get("a").map(String::as_str),
            Some("When Use when writing Rust code, read skills/a/SKILL.md.")
        );
        assert!(set.not_triggered.is_empty());
    }

    #[test]
    fn folded_description_yields_the_joined_text_never_the_indicator() {
        // What a YAML parse of `description: >` yields: the folded lines joined.
        let folded = "Forces the laziest solution that actually works, simplest, shortest, \
                      most minimal. Use on ANY coding task.";
        let set = lines(&[dir_entry("ponytail", folded)], desc_of);
        let line = set.lines.get("ponytail").unwrap();
        assert!(line.contains("Forces the laziest solution that actually works"));
        assert!(line.contains("Use on ANY coding task"));
        assert!(!line.contains('>'));
        assert_eq!(line.matches('\n').count(), 0);
    }

    #[test]
    fn literal_description_yields_the_text_never_the_indicator() {
        // What a YAML parse of `description: |` yields: the text with newlines.
        let literal = "Bring the Open Design catalogue into any agent session.\nPick a brand spec.";
        let set = lines(&[dir_entry("open-design", literal)], desc_of);
        let line = set.lines.get("open-design").unwrap();
        assert!(line.contains("Bring the Open Design catalogue into any agent session."));
        assert!(line.contains("Pick a brand spec"));
        assert!(!line.contains('|'));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn quoted_description_keeps_the_text_without_quotes() {
        let set = lines(
            &[dir_entry(
                "q",
                "\"UI design and implementation for real product surfaces\"",
            )],
            desc_of,
        );
        let line = set.lines.get("q").unwrap();
        assert!(line.starts_with("When UI design and implementation for real product surfaces,"));
        assert!(!line.contains('"'));
    }

    #[test]
    fn empty_description_is_not_triggered() {
        let set = lines(
            &[dir_entry("empty", "   "), dir_entry("ok", "Use me")],
            desc_of,
        );
        assert!(!set.lines.contains_key("empty"));
        assert_eq!(set.not_triggered, vec!["empty".to_string()]);
        assert!(set.lines.contains_key("ok"));
    }

    #[test]
    fn yaml_indicator_is_never_emitted_as_a_cue() {
        // Upstream's regex would yield ">" here.
        let set = lines(&[dir_entry("folded", ">"), dir_entry("lit", "|")], desc_of);
        assert!(set.lines.is_empty());
        assert_eq!(
            set.not_triggered,
            vec!["folded".to_string(), "lit".to_string()]
        );
    }

    #[test]
    fn cap_boundary_is_exact() {
        let cue = "x".repeat(CUE_MAX_CHARS);
        let skills: Vec<SkillEntry> = (0..40)
            .map(|i| flat_entry(&format!("s{i:02}"), &cue))
            .collect();

        let set = lines(&skills, desc_of);
        assert!(!set.not_triggered.is_empty(), "cap must drop something");
        assert_eq!(
            set.lines.len() + set.not_triggered.len(),
            skills.len(),
            "every skill is either triggered or named"
        );
        for name in &set.not_triggered {
            assert!(!set.lines.contains_key(name));
        }

        let total: usize = set.lines.values().map(|l| l.chars().count() + 1).sum();
        assert!(total <= TRIGGER_MAX_CHARS);

        // The first dropped skill did not fit: adding it would exceed the cap.
        let dropped = &set.not_triggered[0];
        let entry = skills.iter().find(|s| &s.name == dropped).unwrap();
        let cost = format!("When {}, read {}.", entry.description, skill_path(entry))
            .chars()
            .count()
            + 1;
        assert!(total + cost > TRIGGER_MAX_CHARS);
    }

    #[test]
    fn long_description_is_cut_at_a_word_boundary() {
        let long = format!("{} alpha", "word ".repeat(200));
        let set = lines(&[dir_entry("long", &long)], desc_of);
        let line = set.lines.get("long").unwrap();
        let cue = line
            .strip_prefix("When ")
            .and_then(|l| l.split_once(", read "))
            .unwrap()
            .0;
        assert!(cue.ends_with('…'), "truncation is visible: {cue}");
        let cut = cue.trim_end_matches('…');
        assert!(!cut.ends_with("wor"), "cue is not cut mid-word: {cut}");
        // Word boundary: the cut point is a space, so the last word is whole.
        assert!(long.starts_with(cut));
        assert!(cut.chars().count() <= CUE_MAX_CHARS);
    }

    #[test]
    fn every_valid_skill_gets_a_line() {
        let valid = vec![
            dir_entry("alpha", "cue"),
            dir_entry("beta", "cue"),
            flat_entry("gamma", "cue"),
        ];
        let bad = dir_entry("delta", "cue");
        let defect = SkillDefect {
            path: bad.path.clone().into(),
            severity: Severity::Fatal,
            code: "name_mismatch",
            message: "mismatch".to_string(),
        };
        let mut all = valid.clone();
        all.push(bad);
        let set = trigger_lines(&all, &[defect], desc_of);
        assert_eq!(set.lines.len(), 3, "one line per valid skill");
        for skill in &valid {
            assert!(set.lines.contains_key(&skill.name), "{}", skill.name);
        }
        assert!(set.not_triggered.contains(&"delta".to_string()));
    }

    #[test]
    fn trigger_lines_are_deterministic() {
        let a = flat_entry("alpha", "first");
        let b = flat_entry("beta", "second");
        let c = flat_entry("gamma", "third");
        let set = lines(&[c.clone(), a.clone(), b.clone()], desc_of);
        let keys: Vec<&str> = set.lines.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["alpha", "beta", "gamma"]);
        let shuffled = lines(&[b, a, c], desc_of);
        assert_eq!(set.lines, shuffled.lines);
    }

    #[test]
    fn path_follows_the_layout_the_skill_uses() {
        let set = lines(
            &[flat_entry("flatone", "cue"), dir_entry("dirone", "cue")],
            desc_of,
        );
        assert_eq!(
            set.lines.get("flatone").map(String::as_str),
            Some("When cue, read skills/flatone.md.")
        );
        assert_eq!(
            set.lines.get("dirone").map(String::as_str),
            Some("When cue, read skills/dirone/SKILL.md.")
        );
    }

    #[test]
    fn dir_layout_wins_over_a_flat_duplicate() {
        let set = lines(
            &[flat_entry("ponytail", "cue"), dir_entry("ponytail", "cue")],
            desc_of,
        );
        assert_eq!(set.lines.len(), 1);
        assert_eq!(
            set.lines.get("ponytail").map(String::as_str),
            Some("When cue, read skills/ponytail/SKILL.md.")
        );
        assert!(set.not_triggered.is_empty());
    }
}
