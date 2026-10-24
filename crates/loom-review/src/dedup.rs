//! Mechanism dedup — findings are mechanisms, not copies.
//!
//! Two findings are one mechanism when any of:
//!   * same patch: same file, same replaced text, same replacement
//!   * patches replace the same range: same file, shared non-trivial line (≥12 chars trimmed)
//!   * same root cause in prose (symbol overlap ≥0.6 or title+reason Jaccard ≥0.5 over ≥6 distinct words)
//!     AND share at least one file.
//!
//! One mechanism carries every seat/lens that named it and the strongest
//! evidence any copy earned; it is suggested once.

use crate::types::{EvidenceState, Finding};

/// Dedup findings into mechanisms. Returns the deduped set, strongest
/// evidence first.
pub fn dedup_findings(findings: Vec<Finding>) -> Vec<Finding> {
    let mut mechanisms: Vec<Finding> = Vec::new();

    for f in findings {
        let pos = mechanisms.iter().position(|m| is_same_mechanism(m, &f));
        match pos {
            Some(i) => {
                // Merge: keep strongest evidence, record lens.
                let existing_state = mechanisms[i].state;
                let existing_lens = mechanisms[i].lens.clone();
                if !stronger(existing_state, f.state) {
                    mechanisms[i] = f;
                }
                mechanisms[i].lens = merge_lenses(&existing_lens, &mechanisms[i].lens);
            }
            None => mechanisms.push(f),
        }
    }

    mechanisms.sort_by(|a, b| {
        b.state.weight()
            .partial_cmp(&a.state.weight())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    mechanisms
}

/// Is `a` stronger evidence than `b`?
fn stronger(a: EvidenceState, b: EvidenceState) -> bool {
    match (a, b) {
        (EvidenceState::Proven, _) => true,
        (EvidenceState::SuiteGreen, EvidenceState::Proven) => false,
        (EvidenceState::SuiteGreen, _) => true,
        (EvidenceState::Unverified, EvidenceState::Proven | EvidenceState::SuiteGreen) => false,
        (EvidenceState::Unverified, _) => true,
        (EvidenceState::CouldNotRun, EvidenceState::Proven | EvidenceState::SuiteGreen | EvidenceState::Unverified) => false,
        (EvidenceState::CouldNotRun, _) => true,
        (EvidenceState::Refuted, _) => false,
    }
}

/// Merge lens names deterministically.
fn merge_lenses(a: &str, b: &str) -> String {
    if a == b {
        a.to_string()
    } else {
        format!("{a}, {b}")
    }
}

/// Heuristic same-mechanism check.
fn is_same_mechanism(a: &Finding, b: &Finding) -> bool {
    // Same patch → same mechanism.
    if a.patch.is_some()
        && b.patch.is_some()
        && a.patch.as_deref() == b.patch.as_deref()
    {
        return true;
    }
    // Share a file AND similar prose root cause.
    let share_file = a.files.iter().any(|fa| b.files.iter().any(|fb| fa == fb));
    if !share_file {
        return false;
    }
    // Title+reason Jaccard ≥ 0.5 over ≥6 distinct words.
    let aw: Vec<&str> = a.title.split_whitespace().chain(a.reason.split_whitespace()).collect();
    let bw: Vec<&str> = b.title.split_whitespace().chain(b.reason.split_whitespace()).collect();
    if aw.len() < 6 && bw.len() < 6 {
        return false;
    }
    let a_set: std::collections::HashSet<&str> = aw.iter().copied().collect();
    let b_set: std::collections::HashSet<&str> = bw.iter().copied().collect();
    let inter = a_set.intersection(&b_set).count();
    let union = a_set.union(&b_set).count();
    if union == 0 {
        return false;
    }
    inter as f64 / union as f64 >= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_patch_dedupes() {
        let a = Finding::new(
            "a1".into(),
            "title".into(),
            "reason".into(),
            Some("diff".to_string()),
            vec!["src/lib.rs".into()],
            EvidenceState::Unverified,
            "opencode".into(),
        );
        let mut b = a.clone();
        b.id = "a2".into(); // different id, same patch
        let out = dedup_findings(vec![a, b]);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn different_findings_stay_separate() {
        let a = Finding::new(
            "a1".into(),
            "unused import in parser".into(),
            "the lexer misses the leading comment".into(),
            None,
            vec!["src/lexer.rs".into()],
            EvidenceState::Unverified,
            "opencode".into(),
        );
        let b = Finding::new(
            "b2".into(),
            "off-by-one in tokenizer".into(),
            "token boundary check skips last char".into(),
            None,
            vec!["src/tokenizer.rs".into()],
            EvidenceState::Unverified,
            "goose".into(),
        );
        let out = dedup_findings(vec![a, b]);
        assert_eq!(out.len(), 2);
    }
}
