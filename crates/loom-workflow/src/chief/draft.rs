//! Deterministic draft plans from free-text goal + details.
//!
//! A draft is a *proposal*, never an implicit work order: the caller
//! previews it first and imports it explicitly. No network, no
//! credentials, no LLM — pure line-marker heuristics, so output is
//! stable and unit-testable.
//!
//! Rules:
//! - Non-empty lines starting with `-`, `*`, `•`, or a number followed
//!   by `.`/`)` are task candidates (stripped text must be ≥3 chars).
//! - Each candidate becomes one task; titles truncate at 120 chars.
//! - At most 12 tasks; further candidates merge into the last task.
//! - Non-candidate prose is preserved under "Notes:" on the first task.
//! - Zero candidates → a single task carrying the whole details.

use crate::chief::plan::{PlanTask, TaskPlan};
use anyhow::Result;

/// Maximum tasks per draft; overflow merges into the last task.
pub const MAX_DRAFT_TASKS: usize = 12;
/// Minimum stripped marker text to count as a task candidate.
const MIN_CANDIDATE_CHARS: usize = 3;
/// Title truncation width (chars, not bytes).
const MAX_TITLE_CHARS: usize = 120;

/// Strip one list marker (`-`, `*`, `•`, `N.` / `N)`) from a line.
/// Returns `None` for unmarked lines.
fn strip_marker(line: &str) -> Option<&str> {
    let text = line.trim_start();
    for marker in ["- ", "* ", "• "] {
        if let Some(rest) = text.strip_prefix(marker) {
            return Some(rest);
        }
    }
    let mut digits_end = 0;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            digits_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if digits_end > 0 {
        let rest = &text[digits_end..];
        if let Some(stripped) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(stripped);
        }
    }
    None
}

fn truncate_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_TITLE_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = text.chars().take(MAX_TITLE_CHARS - 1).collect();
    out.push('…');
    out
}

/// Draft a [`TaskPlan`] from a goal plus free-text details.
/// Errors on empty goal or empty details.
pub fn draft_plan(goal: &str, details: &str) -> Result<TaskPlan> {
    let goal = goal.trim();
    if goal.is_empty() {
        anyhow::bail!("goal must be non-empty");
    }
    let lines: Vec<&str> = details
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        anyhow::bail!("details must be non-empty");
    }

    let mut candidates: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for line in &lines {
        match strip_marker(line) {
            Some(rest) if rest.trim().chars().count() >= MIN_CANDIDATE_CHARS => {
                candidates.push(rest.trim().to_string());
            }
            _ => notes.push((*line).to_string()),
        }
    }

    if candidates.is_empty() {
        return Ok(TaskPlan {
            goal: goal.to_string(),
            tasks: vec![PlanTask {
                id: "t1".to_string(),
                title: goal.to_string(),
                description: lines.join("\n"),
                depends_on: vec![],
                agent: None,
                acceptance: vec![],
            }],
        });
    }

    let mut tasks: Vec<PlanTask> = Vec::new();
    for candidate in &candidates {
        if tasks.len() >= MAX_DRAFT_TASKS {
            let last = tasks.last_mut().unwrap();
            last.description
                .push_str(&format!("\nAlso:\n- {candidate}"));
            continue;
        }
        let id = format!("t{}", tasks.len() + 1);
        tasks.push(PlanTask {
            id,
            title: truncate_title(candidate),
            description: format!("{candidate}\n\nContext: {goal}"),
            depends_on: vec![],
            agent: None,
            acceptance: vec![],
        });
    }
    if !notes.is_empty() && !tasks.is_empty() {
        tasks[0]
            .description
            .push_str(&format!("\n\nNotes:\n{}", notes.join("\n")));
    }
    Ok(TaskPlan {
        goal: goal.to_string(),
        tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::draft_plan;

    #[test]
    fn draft_plan_splits_bullets_and_numbers() {
        let plan = draft_plan("Goal", "- alpha\n* beta\n1. gamma\n\nplain context").unwrap();
        assert_eq!(plan.goal, "Goal");
        assert_eq!(plan.tasks.len(), 3);
        assert_eq!(plan.tasks[0].id, "t1");
        assert_eq!(plan.tasks[0].title, "alpha");
        assert_eq!(plan.tasks[1].title, "beta");
        assert_eq!(plan.tasks[2].title, "gamma");
        assert!(plan.tasks[0].description.contains("Goal"));
        assert!(plan.tasks[0].description.contains("plain context"));
        assert!(plan.tasks[0].depends_on.is_empty());
    }

    #[test]
    fn draft_plan_ignores_short_and_unmarked_lines() {
        let plan = draft_plan("Goal", "- ok task\n- x\n-  \nprose").unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].title, "ok task");
        assert!(plan.tasks[0].description.contains("prose"));
    }

    #[test]
    fn draft_plan_falls_back_to_single_task() {
        let plan = draft_plan("Goal", "just some prose without markers").unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].title, "Goal");
        assert_eq!(plan.tasks[0].description, "just some prose without markers");
    }

    #[test]
    fn draft_plan_caps_at_twelve_with_overflow_merged() {
        let details = (1..=15)
            .map(|n| format!("- task number {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let plan = draft_plan("Goal", &details).unwrap();
        assert_eq!(plan.tasks.len(), 12);
        assert!(plan.tasks[11].description.contains("task number 15"));
    }

    #[test]
    fn draft_plan_rejects_empty_goal_or_details() {
        assert!(draft_plan("", "details").is_err());
        assert!(draft_plan("   ", "details").is_err());
        assert!(draft_plan("Goal", "").is_err());
        assert!(draft_plan("Goal", "   \n  ").is_err());
    }

    #[test]
    fn draft_plan_truncates_very_long_titles() {
        let long = format!("- {}", "w".repeat(200));
        let plan = draft_plan("Goal", &format!("{long}\n- second")).unwrap();
        assert_eq!(plan.tasks.len(), 2);
        assert!(plan.tasks[0].title.chars().count() <= 120);
    }
}
