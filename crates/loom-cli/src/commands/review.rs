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
pub async fn run(loom_dir: &str, action: ReviewAction) -> Result<()> {
    let store = ReviewStore::new(&PathBuf::from(loom_dir))?;
    match action {
        ReviewAction::Run {
            session_id,
            seats,
            lenses,
            dry_run,
        } => {
            let seat_list = seats_arg(seats);
            let run =
                run_review_pipeline(loom_dir, &session_id, &seat_list, &lenses, dry_run).await?;
            println!("{}", write_report(&run, &run.findings)?);
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

/// Seat list from the `--seats` count: the MVP cross-family defaults.
/// Seats that fail to resolve are skipped with a warning at dispatch time.
fn seats_arg(count: usize) -> Vec<String> {
    const DEFAULT_SEATS: [&str; 2] = ["opencode", "crush"];
    DEFAULT_SEATS
        .iter()
        .take(count.clamp(1, DEFAULT_SEATS.len()))
        .map(|s| s.to_string())
        .collect()
}

const REVIEW_TIMEOUT_SECS: u64 = 300;

/// The full MVP review pipeline (pillar 5): load the session, dispatch a
/// blind reviewer per seat over ACP, parse findings, dedupe, verify
/// patch-backed findings in a fresh worktree, and persist the run.
async fn run_review_pipeline(
    loom_dir: &str,
    session_id: &str,
    seats: &[String],
    lenses: &Option<String>,
    dry_run: bool,
) -> Result<ReviewRun> {
    let mut run = ReviewRun::new(format!("review-{session_id}"), session_id.to_string());
    run.seats = seats.to_vec();
    run.lenses = lenses
        .as_deref()
        .map(|s| s.split(',').map(str::trim).map(str::to_string).collect())
        .unwrap_or_default();

    // ── Plan: session context (never the agent's reasoning — blind review) ──
    let sessions = loom_sessions::store::SessionStore::new(loom_dir);
    let manifest = sessions
        .get(session_id)
        .ok_or_else(|| anyhow::anyhow!("session {session_id} not found; index or import it first"))?;
    let messages = sessions.load_messages(session_id)?;
    let intent = messages
        .iter()
        .find(|m| m.role == loom_core::MessageRole::User)
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let summary = messages
        .iter()
        .rev()
        .find(|m| m.role == loom_core::MessageRole::Assistant)
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let prompt = build_review_prompt(&manifest.topic, &intent, &summary, &manifest.files_modified);

    run.status = RunStatus::Planning;
    let store = ReviewStore::new(&PathBuf::from(loom_dir))?;
    store.save_plan(&run.id, &run)?;

    if dry_run {
        println!(
            "dry run: {} seat(s) {:?} over session {session_id} (topic: {:?}, prompt {} chars)",
            seats.len(),
            seats,
            manifest.topic,
            prompt.chars().count()
        );
        return Ok(run);
    }

    // ── Seating + review: one ACP dispatch per seat ──
    run.status = RunStatus::Reviewing;
    store.save_plan(&run.id, &run)?;
    let mut all_findings: Vec<loom_review::Finding> = Vec::new();
    for seat in seats {
        print!("  seat {seat}: ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        match dispatch_reviewer(loom_dir, seat, &prompt, &manifest.working_directory).await {
            Ok(text) => {
                let seat_findings = parse_findings(&text, seat);
                println!("{} finding(s)", seat_findings.len());
                all_findings.extend(seat_findings);
            }
            Err(e) => {
                println!("skipped ({e})");
            }
        }
    }

    // ── Dedup: one mechanism, one finding ──
    let mut merged = loom_review::dedup_findings(all_findings);
    if merged.is_empty() {
        println!("no findings reported by any seat");
    }

    // ── Verify: worktree evidence pass over patch-backed findings ──
    run.status = RunStatus::Verifying;
    store.save_plan(&run.id, &run)?;
    let repo_root = std::path::PathBuf::from(&manifest.working_directory);
    let testable = repo_root.join("Cargo.toml").exists();
    let runner = loom_review::EvidenceRunner::new(repo_root, "cargo test".into(), None);
    for f in &mut merged {
        if f.patch.is_some() && testable {
            match runner.verify(f, "HEAD").await {
                Ok(v) => {
                    f.state = v.state;
                    f.suite = Some(v.suite);
                }
                Err(e) => {
                    f.state = loom_review::EvidenceState::CouldNotRun;
                    f.suite = Some("n/a".into());
                    eprintln!("  verify {} failed: {e}", f.id);
                }
            }
        } else if f.patch.is_some() {
            f.state = loom_review::EvidenceState::CouldNotRun;
            f.suite = Some("n/a".into());
        }
    }

    // ── Report ──
    run.status = RunStatus::Done;
    run.findings = merged.clone();
    store.save_plan(&run.id, &run)?;
    store.append_findings(&run.id, &merged)?;
    let report = write_report(&run, &merged)?;
    let path = store.write_report(&run.id, &report)?;
    run.report_path = Some(path.display().to_string());
    store.save_plan(&run.id, &run)?;
    println!("report: {}", path.display());
    Ok(run)
}

/// Build the blind-review prompt: intent + summary + files, strict JSON out.
fn build_review_prompt(
    topic: &str,
    intent: &str,
    summary: &str,
    files_modified: &[String],
) -> String {
    let trunc = |s: &str, max: usize| {
        if s.chars().count() > max {
            format!("{}…", s.chars().take(max).collect::<String>())
        } else {
            s.to_string()
        }
    };
    format!(
        "You are a code reviewer. Review the work described below — the code \
itself is on disk in the current working directory. Read the files, then \
report only defects you are confident about.\n\n\
Session topic: {topic}\n\
User intent:\n{}\n\
Agent summary of what it did:\n{}\n\
Files touched: {}\n\n\
Respond with ONLY a JSON array (no prose, no markdown fence) of findings:\n\
[{{\"id\":\"f1\",\"title\":\"...\",\"reason\":\"...\",\"patch\":\"unified diff or empty string\",\"files\":[\"path\"],\"confidence\":0.9}}]\n\
An empty array [] means no findings.",
        trunc(intent, 2000),
        trunc(summary, 2000),
        if files_modified.is_empty() {
            "(none recorded)".to_string()
        } else {
            files_modified.join(", ")
        },
    )
}

/// Dispatch one reviewer seat over ACP and return its text answer.
/// Agent launch resolution is async; the protocol round-trip is blocking,
/// so it runs on a blocking thread.
async fn dispatch_reviewer(
    loom_dir: &str,
    seat: &str,
    prompt: &str,
    cwd: &str,
) -> anyhow::Result<String> {
    let launch = loom_sessions::acp_launch::resolve(loom_dir, seat).await?;
    let config = loom_sessions::acp_protocol::AcpClientConfig {
        program: launch.program.clone(),
        args: launch.args.clone(),
        cwd: Some(cwd.to_string()),
        timeout_secs: REVIEW_TIMEOUT_SECS,
        env: launch.env.clone(),
    };
    let prompt = prompt.to_string();
    let transcript =
        tokio::task::spawn_blocking(move || {
            loom_sessions::acp_protocol::AcpProtocolClient::run_prompt(&config, &prompt)
        })
        .await??;
    Ok(transcript.agent_text)
}

/// Extract the findings JSON array from an agent answer. Accepts a
/// ```json fence or a bare array; no JSON at all yields zero findings.
fn parse_findings(text: &str, seat: &str) -> Vec<loom_review::Finding> {
    let inner = if let Some(i) = text.find("```json") {
        let start = i + 7;
        let end = text[start..].find("```").map(|e| start + e).unwrap_or(text.len());
        &text[start..end]
    } else {
        match (text.find('['), text.rfind(']')) {
            (Some(a), Some(b)) if b > a => &text[a..=b],
            _ => return Vec::new(),
        }
    };
    let Ok(values) = serde_json::from_str::<serde_json::Value>(inner.trim()) else {
        return Vec::new();
    };
    let Some(arr) = values.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let title = v.get("title")?.as_str()?.to_string();
            let reason = v.get("reason")?.as_str()?.to_string();
            let patch = v
                .get("patch")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string);
            let files = v
                .get("files")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let id = v
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{seat}-{}", uuid_like()));
            Some(loom_review::Finding::new(
                id,
                title,
                reason,
                patch,
                files,
                loom_review::EvidenceState::Unverified,
                seat.to_string(),
            ))
        })
        .collect()
}

/// Cheap unique-enough suffix for findings the agent did not id.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn review_run_missing_session_errors() {
        let result = run(
            "/tmp/loom-review-test-missing",
            ReviewAction::Run {
                session_id: "no-such-session".into(),
                seats: 2,
                lenses: None,
                dry_run: true,
            },
        )
        .await;
        assert!(result.is_err(), "missing session must be a clear error");
    }
}