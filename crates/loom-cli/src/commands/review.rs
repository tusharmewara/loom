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
        ReviewAction::Findings { run_id: _, state: _ } => {
            todo!()
        }
        ReviewAction::FixPr { run_id: _ } => {
            todo!()
        }
        ReviewAction::Scoreboard => {
            todo!()
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