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
        /// Explicit seat agent ids (repeatable); overrides `--seats` defaults.
        #[arg(long = "seat")]
        seat: Vec<String>,
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
        /// Local git repository to apply findings in (default: current dir).
        #[arg(long)]
        path: Option<String>,
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
    Dismiss {
        id: String,
        /// Why the lesson is being dismissed.
        #[arg(long)]
        reason: String,
    },
}

/// Execute a review command.
pub async fn run(loom_dir: &str, action: ReviewAction) -> Result<()> {
    let store = ReviewStore::new(&PathBuf::from(loom_dir))?;
    match action {
        ReviewAction::Run {
            session_id,
            seats,
            seat,
            lenses,
            dry_run,
        } => {
            let seat_list = if seat.is_empty() {
                seats_arg(seats)
            } else {
                seat
            };
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
        ReviewAction::FixPr {
            run_id,
            repo,
            path,
        } => {
            let repo_path = path
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            // Graceful credential check: the tracker only constructs with a
            // token; without one the local git work still happens and the
            // manual `gh pr create` command is printed instead.
            let tracker = loom_trackers::github::GitHubTracker::new(None).ok();
            let create_pr = tracker.as_ref().map(|t| {
                move |r: &str, title: &str, body: &str, head: &str, base: &str| {
                    t.create_pr(r, title, body, head, base)
                }
            });
            apply_review_fixes(&store, &run_id, &repo_path, &repo, create_pr.as_ref())
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
                let lessons = store.load_all_lessons()?;
                if lessons.is_empty() {
                    println!("no lessons recorded; run a review with terminal findings first");
                    return Ok(());
                }
                for l in lessons {
                    let reason = l
                        .reason
                        .as_deref()
                        .map(|r| format!(" — {r}"))
                        .unwrap_or_default();
                    println!("{:<5} {} [{}] {} ({})", format!("{:?}", l.outcome).to_lowercase(), l.id, l.state, l.title, reason);
                }
                Ok(())
            }
            LessonAction::Confirm { id } => {
                store.set_lesson_outcome(&id, loom_review::LessonOutcome::Confirmed, None)?;
                println!("lesson confirmed: {id}");
                Ok(())
            }
            LessonAction::Dismiss { id, reason } => {
                store.set_lesson_outcome(
                    &id,
                    loom_review::LessonOutcome::Dismissed,
                    Some(reason),
                )?;
                println!("lesson dismissed: {id}");
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
    // Outcome-gated lessons: one per terminal finding.
    let lessons = loom_review::Lesson::derive(&run.id, &merged);
    if !lessons.is_empty() {
        store.save_lessons(&run.id, &lessons)?;
        println!("lessons: {} pending (loom review lesson list)", lessons.len());
    }
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

/// Apply the verified patch-backed findings of a review run as a real git
/// flow in `repo_path`: branch `fix/<run_id>` from HEAD, one `git apply` per
/// patch, one commit, push to `origin` when it exists, then PR creation via
/// the injected `create_pr` seam (`(repo, title, body, head, base) -> url`).
/// Without the seam (no credentials) the manual `gh pr create` command is
/// printed instead — the local work is never lost to a missing token.
fn apply_review_fixes<F>(
    store: &ReviewStore,
    run_id: &str,
    repo_path: &std::path::Path,
    repo_slug: &str,
    create_pr: Option<&F>,
) -> Result<()>
where
    F: Fn(&str, &str, &str, &str, &str)
        -> Result<String, loom_trackers::models::TrackerError>,
{
    if store.load_run(run_id)?.is_none() {
        anyhow::bail!("run {run_id} not found");
    }
    let findings: Vec<_> = store
        .load_findings(run_id)?
        .into_iter()
        .filter(|f| {
            f.patch.is_some()
                && matches!(
                    f.state,
                    loom_review::EvidenceState::SuiteGreen | loom_review::EvidenceState::Proven
                )
        })
        .collect();
    if findings.is_empty() {
        anyhow::bail!(
            "run {run_id} has no verified patch-backed findings (suite_green/proven); nothing to PR"
        );
    }

    if !repo_path.join(".git").exists() {
        anyhow::bail!("{} is not a git repository (no .git)", repo_path.display());
    }
    let head = format!("fix/{run_id}");
    git_out(repo_path, &["checkout", "-b", &head], None)?;
    for f in &findings {
        let patch = f.patch.as_deref().unwrap_or_default();
        if let Err(e) = git_out(repo_path, &["apply", "-"], Some(patch)) {
            anyhow::bail!("apply patch for finding {} ({}): {e}", f.id, f.title);
        }
    }

    // Stage only what the patches touch so unrelated dirty worktree files
    // never leak into the commit; fall back to -A when the diff headers
    // yield no paths.
    let mut touched: Vec<String> = findings
        .iter()
        .filter_map(|f| f.patch.as_deref())
        .flat_map(patch_paths)
        .collect();
    touched.sort();
    touched.dedup();
    if touched.is_empty() {
        git_out(repo_path, &["add", "-A"], None)?;
    } else {
        let mut add = vec!["add", "--"];
        add.extend(touched.iter().map(String::as_str));
        git_out(repo_path, &add, None)?;
    }

    let body = findings
        .iter()
        .map(|f| format!("- {} ({})", f.title, f.state))
        .collect::<Vec<_>>()
        .join("\n");
    git_out(
        repo_path,
        &[
            "commit",
            "-m",
            &format!("Apply verified findings from review run {run_id}"),
            "-m",
            &body,
        ],
        None,
    )?;
    let sha = git_out(repo_path, &["rev-parse", "HEAD"], None)?;
    println!(
        "branch {head} created at {sha} ({} finding(s) applied)",
        findings.len()
    );

    let remotes = git_out(repo_path, &["remote"], None)?;
    if remotes.lines().any(|r| r == "origin") {
        match git_out(repo_path, &["push", "-u", "origin", &head], None) {
            Ok(_) => println!("pushed {head} to origin"),
            Err(e) => eprintln!("push failed (continuing): {e}"),
        }
    } else {
        println!("no `origin` remote; skipped push (branch {head} is local only)");
    }

    let title = format!("Fixes for review run {run_id}");
    match create_pr {
        Some(f) => match f(repo_slug, &title, &body, &head, "main") {
            Ok(url) => println!("Created PR: {url}"),
            Err(e) => print_pr_fallback(
                repo_slug,
                &head,
                &sha,
                &title,
                &body,
                &format!("PR creation failed: {e}"),
            ),
        },
        None => print_pr_fallback(
            repo_slug,
            &head,
            &sha,
            &title,
            &body,
            "no GitHub credentials (set GITHUB_TOKEN or GH_TOKEN)",
        ),
    }
    Ok(())
}

/// Print the branch, commit sha, and the exact `gh pr create` command the
/// user can run when the automated PR step is unavailable.
fn print_pr_fallback(repo: &str, head: &str, sha: &str, title: &str, body: &str, why: &str) {
    println!("PR not created: {why}");
    println!("  branch: {head} @ {sha}");
    println!(
        "  run: gh pr create --repo {repo} --head {head} --base main --title \"{title}\" --body \"{body}\""
    );
}

/// Repo-relative paths a unified diff touches, from its `a/`/`b/` headers.
/// Skips `/dev/null` so additions and deletions resolve to real paths.
fn patch_paths(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|l| {
            l.strip_prefix("+++ b/")
                .or_else(|| l.strip_prefix("--- a/"))
        })
        .filter(|p| *p != "/dev/null")
        .map(str::to_string)
        .collect()
}

/// Run a git command in `dir` (optionally feeding `stdin`, e.g. a patch to
/// `apply -`); trimmed stdout on success, command + stderr on failure.
fn git_out(dir: &std::path::Path, args: &[&str], stdin: Option<&str>) -> Result<String> {
    use std::io::Write;
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    if stdin.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("git {}: {e}", args.join(" ")))?;
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_review::{EvidenceState, Finding, ReviewRun};
    use loom_trackers::models::TrackerError;
    use std::process::Command;
    use std::sync::Mutex;

    /// PR-creation seam signature as a fn pointer, for `None` stubs.
    type PrFn = fn(&str, &str, &str, &str, &str) -> Result<String, TrackerError>;

    /// Unified diff against `BASE_CONTENT` fixing the sign bug.
    const FIX_PATCH: &str = "--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n-fn add(a: i32, b: i32) -> i32 { a - b }\n+fn add(a: i32, b: i32) -> i32 { a + b }\n";
    const BASE_CONTENT: &str = "fn add(a: i32, b: i32) -> i32 { a - b }\n";
    const FIXED_CONTENT: &str = "fn add(a: i32, b: i32) -> i32 { a + b }\n";

    /// Run git in `dir`, panicking on failure (test setup only).
    fn git_setup(dir: &std::path::Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Temp git repo with one committed `lib.rs`, plus a bare `origin`
    /// remote when `with_remote` (offline: a local bare repo, no network).
    fn init_repo(with_remote: bool) -> (tempfile::TempDir, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), BASE_CONTENT).unwrap();
        git_setup(dir.path(), &["init"]);
        git_setup(dir.path(), &["config", "user.email", "test@loom.dev"]);
        git_setup(dir.path(), &["config", "user.name", "Loom Test"]);
        git_setup(dir.path(), &["add", "-A"]);
        git_setup(dir.path(), &["commit", "-m", "base"]);
        let remote = tempfile::tempdir().unwrap();
        if with_remote {
            git_setup(remote.path(), &["init", "--bare"]);
            git_setup(
                dir.path(),
                &["remote", "add", "origin", remote.path().to_str().unwrap()],
            );
        }
        (dir, remote)
    }

    /// Store seeded with `run_id` + one SuiteGreen patch-backed finding.
    fn seed_run(store: &ReviewStore, run_id: &str) {
        store
            .save_plan(run_id, &ReviewRun::new(run_id.into(), "s".into()))
            .unwrap();
        let f = Finding::new(
            "f1".into(),
            "wrong sign".into(),
            "add subtracts instead of adding".into(),
            Some(FIX_PATCH.into()),
            vec!["lib.rs".into()],
            EvidenceState::SuiteGreen,
            "bugs_security".into(),
        );
        store.append_findings(run_id, &[f]).unwrap();
    }

    #[test]
    fn fix_pr_creates_branch_applies_patch_and_commits() {
        let (repo, _remote) = init_repo(true);
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReviewStore::new(store_dir.path()).unwrap();
        seed_run(&store, "run-1");

        let calls = Mutex::new(Vec::<Vec<String>>::new());
        let pr = |r: &str, t: &str, b: &str, h: &str, base: &str| {
            calls
                .lock()
                .unwrap()
                .push(vec![r.into(), t.into(), b.into(), h.into(), base.into()]);
            Ok::<_, TrackerError>("https://github.com/o/r/pull/1".into())
        };
        apply_review_fixes(&store, "run-1", repo.path(), "o/r", Some(&pr)).unwrap();

        // On the fix branch.
        let branch = git_out(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"], None).unwrap();
        assert_eq!(branch, "fix/run-1");
        // Patched content on disk.
        let content = std::fs::read_to_string(repo.path().join("lib.rs")).unwrap();
        assert_eq!(content, FIXED_CONTENT);
        // Exactly one commit beyond the base (root) commit.
        let root = git_out(repo.path(), &["rev-list", "--max-parents=0", "HEAD"], None).unwrap();
        let count = git_out(repo.path(), &["rev-list", &format!("{root}..HEAD")], None).unwrap();
        assert_eq!(count.lines().count(), 1, "expected exactly one commit");
        // Commit message carries the run id and the finding title.
        let msg = git_out(repo.path(), &["log", "-1", "--pretty=%B"], None).unwrap();
        assert!(msg.contains("Apply verified findings from review run run-1"), "{msg}");
        assert!(msg.contains("wrong sign"), "{msg}");
        // PR seam called with repo/head/base; head/base must not be reordered.
        let c = calls.lock().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0][0], "o/r");
        assert_eq!(c[0][3], "fix/run-1");
        assert_eq!(c[0][4], "main");
        assert!(c[0][1].contains("run-1"), "{}", c[0][1]);
        // Push landed on the bare origin.
        let refs = git_out(repo.path(), &["ls-remote", "origin", "fix/run-1"], None).unwrap();
        assert!(refs.contains("refs/heads/fix/run-1"), "{refs}");
    }

    #[test]
    fn fix_pr_bails_without_verified_findings() {
        let (repo, _remote) = init_repo(false);
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReviewStore::new(store_dir.path()).unwrap();
        store
            .save_plan("r2", &ReviewRun::new("r2".into(), "s".into()))
            .unwrap();
        // Patch-backed but never verified — and a refuted one. Neither applies.
        let unverified = Finding::new(
            "f1".into(),
            "t".into(),
            "r".into(),
            Some(FIX_PATCH.into()),
            vec!["lib.rs".into()],
            EvidenceState::Unverified,
            "seat".into(),
        );
        let refuted = Finding::new(
            "f2".into(),
            "t2".into(),
            "r2".into(),
            Some(FIX_PATCH.into()),
            vec!["lib.rs".into()],
            EvidenceState::Refuted,
            "seat".into(),
        );
        store.append_findings("r2", &[unverified, refuted]).unwrap();

        let never_pr: PrFn = |_: &str, _: &str, _: &str, _: &str, _: &str| {
            panic!("PR step must not run when nothing is verified")
        };
        let err = apply_review_fixes(&store, "r2", repo.path(), "o/r", Some(&never_pr))
            .unwrap_err();
        assert!(
            err.to_string().contains("no verified patch-backed findings"),
            "{err}"
        );
        // Repo untouched: file still has the base content.
        let content = std::fs::read_to_string(repo.path().join("lib.rs")).unwrap();
        assert_eq!(content, BASE_CONTENT);
    }

    #[test]
    fn fix_pr_names_finding_on_apply_failure() {
        let (repo, _remote) = init_repo(false);
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReviewStore::new(store_dir.path()).unwrap();
        store
            .save_plan("r3", &ReviewRun::new("r3".into(), "s".into()))
            .unwrap();
        let bad = Finding::new(
            "f-bad".into(),
            "does not apply".into(),
            "patch expects content that was never committed".into(),
            Some("--- a/lib.rs\n+++ b/lib.rs\n@@ -1 +1 @@\n-fn nope() {}\n+fn yes() {}\n".into()),
            vec!["lib.rs".into()],
            EvidenceState::Proven,
            "bugs_security".into(),
        );
        store.append_findings("r3", &[bad]).unwrap();

        let err = apply_review_fixes(&store, "r3", repo.path(), "o/r", None::<&PrFn>).unwrap_err();
        assert!(err.to_string().contains("f-bad"), "{err}");
        assert!(err.to_string().contains("does not apply"), "{err}");
    }

    #[test]
    fn fix_pr_without_remote_or_credentials_still_succeeds() {
        let (repo, _remote) = init_repo(false);
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReviewStore::new(store_dir.path()).unwrap();
        seed_run(&store, "run-4");

        // No origin remote, no PR seam (credentials unavailable): the local
        // git work must still complete and the command must not fail.
        apply_review_fixes(&store, "run-4", repo.path(), "o/r", None::<&PrFn>).unwrap();

        let branch = git_out(repo.path(), &["rev-parse", "--abbrev-ref", "HEAD"], None).unwrap();
        assert_eq!(branch, "fix/run-4");
        let content = std::fs::read_to_string(repo.path().join("lib.rs")).unwrap();
        assert_eq!(content, FIXED_CONTENT);
        // No remote was ever configured, so nothing could have been pushed.
        assert!(git_out(repo.path(), &["remote"], None).unwrap().is_empty());
    }

    #[test]
    fn fix_pr_rejects_non_git_path() {
        let store_dir = tempfile::tempdir().unwrap();
        let store = ReviewStore::new(store_dir.path()).unwrap();
        seed_run(&store, "run-5");
        let plain_dir = tempfile::tempdir().unwrap();
        let err =
            apply_review_fixes(&store, "run-5", plain_dir.path(), "o/r", None::<&PrFn>).unwrap_err();
        assert!(err.to_string().contains("not a git repository"), "{err}");
    }

    #[tokio::test]
    async fn review_run_missing_session_errors() {
        let result = run(
            "/tmp/loom-review-test-missing",
            ReviewAction::Run {
                session_id: "no-such-session".into(),
                seats: 2,
                seat: Vec::new(),
                lenses: None,
                dry_run: true,
            },
        )
        .await;
        assert!(result.is_err(), "missing session must be a clear error");
    }
}