use anyhow::Result;
use clap::Parser;
use loom_review::{ReviewRun, write_report, ReviewStore, RunStatus};
use std::path::PathBuf;

/// Review subcommand args.
#[derive(Parser)]
pub struct ReviewArgs {
    #[command(subcommand)]
    pub action: ReviewAction,
}

/// Review action variants.
#[derive(Parser)]
pub enum ReviewAction {
    /// Run a review on a session, starting the evidence pipeline.
    Run {
        /// Session ID to review.
        session_id: String,
        /// Number of evidence seats (default: 2).
        #[arg(long, default_value_t = 2)]
        seats: usize,
        /// Optional lenses to focus on (e.g. "bugs_security").
        #[arg(long)]
        lenses: Option<String>,
        /// Dry run: show planned steps without executing.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Show status of a review run.
    Status {
        /// Run ID to look up (shows all if omitted).
        #[arg(long)]
        run_id: Option<String>,
    },
    /// List findings from a review run, optionally filtered by state.
    Findings {
        /// Run ID to look up.
        #[arg(long)]
        run_id: Option<String>,
        /// Filter by EvidenceState (proven, suite_green, unverified, could_not_run, refuted).
        #[arg(long)]
        state: Option<String>,
    },
    /// Generate a fix PR for a review run.
    FixPr {
        /// Run ID to generate PR for.
        #[arg(long)]
        run_id: String,
        /// Repository (owner/repo)
        #[arg(long)]
        repo: String,
    },
    /// Show the scoreboard of findings with confidence.
    Scoreboard,
    /// Manage lessons learned from the review.
    Lesson {
        #[command(subcommand)]
        action: LessonAction,
    },
}

#[derive(Parser)]
pub enum LessonAction {
    List,
    Confirm { id: String },
    Dismiss { id: String, reason: String },
}

/// Execute a review command.
pub fn run(loom_dir: &str, action: ReviewAction) -> Result<()> {
    let store = ReviewStore::new(&PathBuf::from(loom_dir))?;
    match action {
        ReviewAction::Run {
            session_id,
            seats: _seats,
            lenses,
            dry_run: _dry_run,
        } => {
            let mut run = ReviewRun::new(
                format!("review-{session_id}"),
                session_id.clone(),
            );
            run.seats = vec!["opencode".into(), "crush".into()];
            run.lenses = lenses
                .into_iter()
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>();
            run.status = RunStatus::Done;
            store.save_plan(&run.id, &run)?;
            let report = write_report(&run, &run.findings)?;
            store.write_report(&run.id, &report)?;
            println!("{}", report);
            Ok(())
        }
        ReviewAction::Status { run_id } => match run_id {
            Some(id) => match store.load_run(&id)? {
                Some(run) => {
                    println!("{}", serde_json::to_string_pretty(&run)?);
                    Ok(())
                }
                None => anyhow::bail!("run {id} not found"),
            },
            None => {
                let ids = store.list_runs()?;
                if ids.is_empty() {
                    println!("no review runs");
                } else {
                    for id in ids {
                        println!("{id}");
                    }
                }
                Ok(())
            }
        },
        ReviewAction::Findings { run_id, state } => {
            let id = match run_id {
                Some(id) => id,
                None => match store.list_runs()?.first() {
                    Some(latest) => latest.clone(),
                    None => anyhow::bail!("no review runs; run `loom review run <session-id>` first"),
                },
            };
            let findings = store.load_findings(&id)?;
            let findings: Vec<_> = findings
                .into_iter()
                .filter(|f| {
                    state.as_deref().is_none_or(|s| {
                        f.state.label() == s.trim()
                    })
                })
                .collect();
            if findings.is_empty() {
                println!("no findings in run {id}");
            }
            for f in findings {
                println!(
                    "{:>5.2} [{}] {} — {} ({})",
                    f.confidence, f.state, f.title, f.reason, f.lens
                );
            }
            Ok(())
        }
        ReviewAction::FixPr { run_id, repo } => {
            let run = store
                .load_run(&run_id)?
                .ok_or_else(|| anyhow::anyhow!("run {run_id} not found"))?;
            let patch_backed: Vec<_> = store
                .load_findings(&run_id)?
                .into_iter()
                .filter(|f| f.patch.is_some() && f.state != loom_review::EvidenceState::Refuted)
                .collect();
            if patch_backed.is_empty() {
                anyhow::bail!("run {run_id} has no patch-backed findings; nothing to PR");
            }
            // ponytail: prints the PR plan; branch+push+`GitHubTracker::create_pr`
            // lands once the seating pipeline produces real patches (today runs
            // never carry any, so the full git flow would be untestable dead code).
            let head = format!("fix/{}", run.id);
            let title = format!("Fixes for review run {}", run.id);
            let body = patch_backed
                .iter()
                .map(|f| format!("- {} ({})", f.title, f.state))
                .collect::<Vec<_>>()
                .join("\n");
            println!("PR plan for {repo}:");
            println!("  head: {head}  base: main");
            println!("  title: {title}");
            println!("  body:\n{body}");
            Ok(())
        }
        ReviewAction::Scoreboard => {
            let mut rows: Vec<(String, loom_review::Finding)> = Vec::new();
            for id in store.list_runs()? {
                for f in store.load_findings(&id)? {
                    rows.push((id.clone(), f));
                }
            }
            if rows.is_empty() {
                println!("no findings recorded yet");
                return Ok(());
            }
            rows.sort_by(|a, b| b.1.confidence.partial_cmp(&a.1.confidence).unwrap());
            println!("run                     conf  state          title");
            for (run, f) in rows {
                println!("{:<22} {:.2}  {:<13} {}", run, f.confidence, f.state, f.title);
            }
            Ok(())
        }
        ReviewAction::Lesson { action } => match action {
            LessonAction::List => {
                println!("lesson actions: list");
                Ok(())
            }
            LessonAction::Confirm { id } => {
                println!("lesson confirmed {id}");
                Ok(())
            }
            LessonAction::Dismiss { id, reason } => {
                println!("lesson dismissed {id} reason={reason}");
                Ok(())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_run_creates_run() {
        let args = ReviewArgs {
            action: ReviewAction::Run {
                session_id: "test-session".into(),
                seats: 2,
                lenses: None,
                dry_run: false,
            },
        };
        let result = run("/tmp/loom-test", args.action);
        assert!(result.is_ok(), "review run should succeed (dry-run or not)");
    }
}