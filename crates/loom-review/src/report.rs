//! Write a human-readable review report (`report.md`).

use crate::types::{Finding, ReviewRun};
use anyhow::Result;
use std::fmt::Write;

/// Compose a markdown report from a run and its findings. Returns the report text.
pub fn write_report(run: &ReviewRun, findings: &[Finding]) -> Result<String> {
    let mut s = String::new();
    writeln!(s, "# Review: {}", run.id)?;
    writeln!(s)?;
    writeln!(s, "Session: {}", run.session_id)?;
    writeln!(s, "Status: {:?}", run.status)?;

    if findings.is_empty() {
        writeln!(s)?;
        writeln!(s, "## Findings")?;
        writeln!(s)?;
        writeln!(s, "No findings.")?;
    } else {
        writeln!(s)?;
        writeln!(s, "## Findings")?;
        for f in findings {
            writeln!(s)?;
            writeln!(s, "### {}", f.title)?;
            writeln!(s, "- **id**: {}", f.id)?;
            writeln!(s, "- **state**: {}", f.state.label())?;
            writeln!(s, "- **confidence**: {:.2}", f.confidence)?;
            writeln!(s, "- **lens**: {}", f.lens)?;
            if !f.files.is_empty() {
                writeln!(s, "- **files**: {}", f.files.join(", "))?;
            }
            if let Some(suite) = &f.suite {
                writeln!(s, "- **suite**: {}", suite)?;
            }
            writeln!(s)?;
            writeln!(s, "{}", f.reason)?;
            if let Some(patch) = &f.patch {
                writeln!(s)?;
                writeln!(s, "```diff")?;
                writeln!(s, "{}", patch)?;
                writeln!(s, "```")?;
            }
        }
    }

    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_renders() {
        let run = ReviewRun {
            id: "r1".into(),
            session_id: "s1".into(),
            status: crate::types::RunStatus::Done,
            created_at: "".into(),
            updated_at: "".into(),
            seats: vec![],
            lenses: vec![],
            findings: vec![],
            report_path: None,
        };
        let rep = write_report(&run, &[]).expect("report");
        assert!(rep.contains("# Review: r1"));
        assert!(rep.contains("Session: s1"));
    }
}
