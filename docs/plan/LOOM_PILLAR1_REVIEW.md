# Pillar 1 review — IPC backoff, backlog reconciliation, embedded-pane dispatch

> **RESOLVED 2026-10-21 — this is the review record, kept as it was written.**
>
> The findings below were raised against the three pillar-1 branches and were
> closed in the merge branch `p1/merge` before it landed on `main`:
>
> | Finding | Severity | Resolution |
> |---|---|---|
> | F1 | BLOCKER | Closed. `docs/loom-improvements.md` now says the same thing `ROADMAP.md` says — the backoff landed, and the single-shot probes stay single-shot but print the same advice. |
> | F2 | BLOCKER | Closed before the merge, in `p1/embedded-pane` commit `c637439`: `App::dispatch_embedded_pane` resolves the agent from the full task record (`loom workflow list --json` carries no `last_agent`) and falls back to a PATH binary. |
> | F3 | MEDIUM | Closed. The deleted `EmbeddedPane` type is no longer named in `VISION.md` or `AGENTS.md`. |
> | F4 | MEDIUM | Closed. `docs/routing-debt.md` records the branch head and the pull-request merge each routing-debt step landed through. |
> | F5 | MEDIUM | Closed. The single-shot probes (`status`, `doctor`, `telemetry`) now route through `crate::ipc_client::probe_daemon` + `stale_socket_advice`; the roadmap names which commands get the backoff and which do not. |
> | F6 | LOW | Moot. The stale "still open on `main`" note described Phase A, which has since landed. |
> | F7, F8, F9 | INFO | F7 is a card-prompt typo (the package is `loom`, not `loom-cli`); F8 is the known worst-case latency of the backoff; F9 is the gitignored `planning/` re-sync, which cannot be a commit. |
>
> The review's own baseline is reproduced below unchanged.


Reviewer: PILLAR-1 REVIEWER (read-only; no fixes applied, nothing merged to `main`, nothing pushed).
Reviewed base: `main` = `b77b5cf` ("docs: harness2 analysis + skill validation proposal").
Review worktree: `.worktrees/p1-review`, branch `p1/review`.
Branches: `p1/ipc-backoff` (`52be61f`), `p1/backlog-reconcile` (`a811a98`), `p1/embedded-pane` (`1dcd0f0`).

Standard applied — ROADMAP.md:17-26, "1. Harden the Core":

> IPC protocol hardening. ~~Versioned request envelope~~, ~~request timeouts~~, ~~panic isolation as a rule~~ — all landed in v0.9.6. Still open: connection-backoff guidance for CLI.

Note on timing: `p1/embedded-pane` was re-landed at 12:26 while this review was in flight. The branch was empty at first inspection; the findings below describe the branch as it now stands (`1dcd0f0`) and were re-verified against it.

---

## Findings (most severe first)

### F1 — BLOCKER: the two documents disagree about the same item after merge — and the merge now collides on it

Merged tree (`.worktrees/p1-review`, `main` + `p1/ipc-backoff` + `p1/backlog-reconcile`):

- `ROADMAP.md:22` — "- **IPC protocol hardening.** … ~~Connection-backoff guidance for CLI~~ — **landed**: the CLI connects with a bounded backoff (8 attempts, 200ms doubling to a 1s ceiling) instead of trusting the socket file's existence, and an unreachable daemon says what happened and what to type (`daemon socket is stale (loomd is not running) — run \`loom daemon restart\``)."
- `docs/loom-improvements.md:23` — "- IPC protocol: … **Still open: connection-backoff guidance for the CLI — the IPC client path has no retry or backoff at all (`crates/loom-ipc/src/client.rs` only wraps the request in a timeout).**"

Same item, opposite truth, in the roadmap and the backlog. The `docs/loom-improvements.md` sentence is not merely stale — it is **false against the merged code**: `crates/loom-ipc/src/client.rs:65-93` defines `connect_with_backoff` (8 attempts, 200 ms base doubling to a 1 s ceiling, 1 s per-attempt timeout) and `crates/loom-cli/src/ipc_client.rs:46-56` calls it.

Origin: `p1/backlog-reconcile` (`a13f26c`) added the "Still open" sentence while standing on `main`, where no backoff existed — true then, false once `p1/ipc-backoff` lands. `p1/ipc-backoff` (`52be61f`) struck the same item in `ROADMAP.md:22`. Neither branch is wrong alone; the *merge* is.

This is also now a **merge conflict**, not just a prose defect — `git merge p1/embedded-pane` into `p1/review` produces:

```
Auto-merging docs/loom-improvements.md
CONFLICT (content): Merge conflict in docs/loom-improvements.md
```

because `p1/embedded-pane` also rewrites the same two bullets of `docs/loom-improvements.md:23-24` (its version of the IPC bullet is `main`'s, without the sentence `p1/backlog-reconcile` added).

**Required:** in the merge, resolve `docs/loom-improvements.md:23` to the landed statement — mirror `ROADMAP.md:22`: bounded backoff (8 attempts, 200 ms → 1 s ceiling) and the cause-and-remedy error. Do not simply delete the clause: the bullet is the backlog's index of the pillar and should record the landed detail, as its sibling bullets do. (I used exactly this wording to test the three-way merge; it builds and passes.)

**Leave alone** the same bullet's panic-isolation sentence — it is true: `crates/loom-ipc/src/server.rs:59-62` runs each handler on a bare thread via `std::thread::scope` and converts a panic into `IpcResponse::error(..., "Internal server error: handler panicked")`.

### F2 — BLOCKER: `p1/embedded-pane` deletes the fake dispatcher but the surviving path can never spawn in production

The branch is now real and takes the DELETE option: the fake `EmbeddedPane` dispatcher is gone (`crates/loom-tui/src/dispatch.rs`, the whole `impl Dispatcher for EmbeddedPane` block and `test_embedded_pane_available`), and `App::dispatch_embedded_pane` (`crates/loom-tui/src/app.rs:703-741`) is the single owner of the path. Good structure. But it cannot work with the data the TUI actually has:

- `app.workflow` is populated from `self.adapter.fetch_workflow_list()` (`crates/loom-tui/src/app.rs:449-451`), which runs `loom workflow list --json` (`crates/loom-tui/src/adapter.rs:57-59` + `run_json` appends `--json`, `adapter.rs:333-336`).
- That JSON carries **no `last_agent`**: `crates/loom-cli/src/commands/workflow.rs:24-29` builds each task summary as only `task_id`, `title`, `phase`, `status`. Verified by running the built CLI:
  ```
  $ loom workflow list --json
  { "command": "workflow.list", "count": 1, "tasks": [ { "phase": "Plan", "status": "Pending",
      "task_id": "b958dae5-…", "title": "probe-agent" } ] }      # no last_agent
  ```
- `dispatch_embedded_pane` reads the agent from `/last_agent` of that very object (`crates/loom-tui/src/app.rs:717`) and bails out at `app.rs:723-725` with "Task {task_id} has no recorded agent to dispatch".

**Proof.** I fed the *exact* payload the CLI produces into the branch's own code path and ran it:

```
PROBE success=false message=Task b958dae5-… has no recorded agent to dispatch
```

So in the real TUI, pressing `e` on a task never spawns anything — the pane reports "no recorded agent to dispatch". The branch's own test (`crates/loom-tui/src/app.rs:2017-2038`) hand-injects `"tasks": [{ "task_id": "task-42", "last_agent": "claude-code" }]` — a payload the CLI never produces — so it proves the spawn *given* an agent, but the production path never reaches the spawn. The test is honest about what it asserts; it is just not the path that runs.

**The data exists elsewhere**, so this is a small fix, not a rework:
- the on-disk task manifest carries it — verified: `~/.loom/workflow/tasks/<id>.json` contains `"last_agent": "goose"` (written by `crates/loom-workflow/src/task_manager.rs:223-225` from the `Task` struct at `crates/loom-core/src/types.rs:679-681`);
- `loom workflow show <id> --json` returns the full task — verified: `"last_agent": "goose"` in the `task` object (`crates/loom-cli/src/commands/workflow.rs:78-92`);
- and `loom workflow dispatch` already resolves `task.last_agent` itself when `--agent` is omitted (`crates/loom-cli/src/commands/workflow.rs:477-486`, erroring only when there is genuinely no agent).

**Required:** source the agent from one of those (simplest: let `loom workflow dispatch` resolve it — i.e. do not pass `--agent` when the TUI does not know one — or fetch `workflow show <id> --json` / read the task manifest), and make the test drive the **real** `loom workflow list --json` payload shape. Until then the branch's headline claim is unproven in production, which also makes the new `VISION.md:75` sentence ("the pane spawns `loom workflow dispatch <task> --agent <agent>` and streams its output") **false as written** — see F3.

### F3 — MEDIUM: the branch leaves three stale references to the type it deleted

`p1/embedded-pane` deletes the `EmbeddedPane` dispatcher but only rewrites one line of `VISION.md`. Left behind:

- `VISION.md:24` — "- `Dispatcher` trait for launching tools from the TUI (TerminalLaunch, ACPDispatch, **EmbeddedPane**)" — names a type that no longer exists.
- `VISION.md:63` — "…`ACPDispatch` and **EmbeddedPane** are wired to the daemon and the embedded pane respectively."
- `AGENTS.md:318` — "10. **EmbeddedPane dispatch** — opens the embedded pane and streams `workflow dispatch` output for the task."
- `AGENTS.md:331` — "Dispatch menu on task tabs: TerminalLaunch …, ACPDispatch …, **EmbeddedPane** (implemented — opens the embedded pane and streams dispatch output)".

So `VISION.md:75` now says "there is no separate `EmbeddedPane` dispatcher" while `VISION.md:24` and `VISION.md:63` list `EmbeddedPane` as one — the file contradicts itself, which is the exact failure mode the finish-or-delete sweep exists to stop. `AGENTS.md` was outside the card's write scope, so this needs a deliberate follow-up rather than a silent skip.

**Required:** update `VISION.md:24` and `VISION.md:63` to drop or re-describe `EmbeddedPane` (e.g. "the embedded pane path"), and correct `AGENTS.md:318` and `AGENTS.md:331` in the same pass (or open a follow-up card naming them). Also, once F2 is fixed, make `VISION.md:75` describe what actually happens, including where the agent comes from.

### F4 — MEDIUM: `p1/backlog-reconcile` attributes the routing debt to branches that do not exist

`docs/routing-debt.md:9-11` (added by `p1/backlog-reconcile`):

> "**What closed it:** all six routing debts below landed on `main` (Steps 0–3 and 5–6 via the `debt/*` branches, merged; Step 4 by re-syncing the Supplement to the shipped constants)."

`git branch --list 'debt/*'` is empty. The merge history contains PR merges only for steps 2–6 (`4d8c9c8` step6, `e152553` step5, `0ee6d58` step4, `11da566` step3, `6991e91` step2). Step 0 landed as `868cf09` ("docs: track ROADMAP.md decision document", a direct commit) and Step 1 as `f4a5f61` ("fix(routing): count chars, not bytes", also a direct commit).

**Required:** correct that sentence to match the history (e.g. "Steps 2–6 via the `debt/step*` branches, merged; Steps 0–1 as direct commits"). The substantive claim — all six debts are landed — is true; only the attribution is wrong.

### F5 — MEDIUM: `ROADMAP.md:22`'s "landed" claim is overbroad — three CLI commands still connect without backoff and print no remedy

The backoff is reached only through `send_daemon_command` → `ensure_daemon` → `connect_with_backoff` (`crates/loom-cli/src/ipc_client.rs:58-65, 24-39, 46-56`). These call sites bypass it and use the raw `loom_ipc::send_command` (single connect, no retry, no advice):

- `crates/loom-cli/src/commands/status.rs:22` — on failure prints "✗ Daemon is not reachable" / "Reason: {err}" (`status.rs:75-82`) — no remedy.
- `crates/loom-cli/src/commands/doctor.rs:198` — on failure prints "Socket exists but IPC failed: {e}" (`doctor.rs:236`) — no remedy.
- `crates/loom-cli/src/commands/telemetry.rs:63` and `:83` — raw connect, no advice.

So "the CLI connects with a bounded backoff … and an unreachable daemon says what happened and what to type" is true for `loom acp *`, `loom memory *`, `loom skills *` (the `send_daemon_command` callers: `crates/loom-cli/src/commands/acp.rs:52,120,133,159,248,408`, `memory.rs:45,124,154`, `skills.rs:22,118`) and for `loom daemon status` (which deliberately probes once — `crates/loom-cli/src/commands/daemon.rs:621`) — but **not** for `loom status`, `loom doctor` and `loom telemetry`.

**Required (or a doc amendment):** either route `status`/`doctor`/`telemetry` through `crate::ipc_client::probe_daemon` and add the `stale_socket_advice` remedy to their unreachable branch, or narrow the `ROADMAP.md:22` claim to the commands that actually get the guidance. `probe_daemon` exists precisely for "status-like commands that report the daemon's state" (`crates/loom-cli/src/ipc_client.rs:67-70`), and `doctor`/`status` are exactly that kind of command.

### F6 — LOW: a "Pillar 3" status note sits under a "Pillar 1" heading

`docs/loom-improvements.md:37` adds:

> "*Status 2026-10-21: still open on `main` — nothing below has landed. `crates/loom-skills/src/scan.rs` and `trigger.rs` do not exist on `main`; they exist only on the unmerged `phase-a/*` branches…*"

The heading it sits under is "### Pillar 1 (candidate slice) — Skill inventory + validation + trigger generation" (`docs/loom-improvements.md:33`); the file's actual "### Pillar 3 — Ecosystem & Protocol" heading is at `:29`. The note's *content* is accurate (verified: `crates/loom-skills/src/scan.rs` and `trigger.rs` do not exist on `main`; they exist only on the unmerged `phase-a/*` branches — `phase-a/scan 12a559b`, `phase-a/trigger 89468ca`), but the label and the section disagree.

**Required:** move the note under the `### Pillar 3 — Ecosystem & Protocol` heading it describes, or retitle it "Status 2026-10-21" without the pillar number.

### F7 — INFORMATIONAL: the card's test command is wrong, the branch is right

Card `fad90e81` says "TESTS `cargo test -p loom-cli`". The package in `crates/loom-cli/Cargo.toml` is named `loom` (`[package] name = "loom"`), so `cargo test -p loom-cli` fails with "package ID specification `loom-cli` did not match any packages". The branch reported and I re-ran `cargo test -p loom` (52 tests). No branch defect; noted so the merge card does not "fix" a correct branch.

### F8 — INFORMATIONAL: the failure path now costs up to ~13 s before the advice appears

`connect_with_backoff` (`crates/loom-ipc/src/client.rs:65-93`) = 8 attempts × up to 1 s connect timeout + 7 sleeps of 200/400/800/1000/1000/1000/1000 ms ⇒ worst case ≈ 13.4 s; guaranteed sleep total 5.4 s. The stale-socket test (`crates/loom-cli/src/ipc_client.rs:161-181`) therefore takes ~5.4 s to surface "daemon socket is stale (loomd is not running) — run `loom daemon restart`". Inherent to the card's own requirement; the *happy* path is unaffected (one connect, no sleep — proven below). Flagged only so the merge card knows the ceiling.

### F9 — INFORMATIONAL: the Supplement re-sync the card asked for is not in any commit (and cannot be)

`p1/backlog-reconcile` (`a811a98`) states this honestly: the Supplement lives under `planning/`, which `.gitignore:1` ignores (`git check-ignore -v planning/Intelligent_Task_Routing_Supplement.md` → matched; `git ls-files planning/` → empty; `git ls-tree -r p1/backlog-reconcile` has no `planning/` path). The working-tree copy on this machine does carry the correction (`planning/Intelligent_Task_Routing_Supplement.md:621` now reads `numeric += keywordScore(signals.keyword_indicators) * 0.35;`, and the band prose at `:60`, `:71`, `:378`, `:633` matches the shipped constants in `crates/loom-routing/src/scorer.rs:157`).

The card cited `Supplement:566` for the stale `0.25`; line 566 in the current file is unrelated (`phase_history: PhaseHistoryEntry[];`) because the file changed underneath the card. The branch handles this correctly by citing the current lines (`:619`, `:621`, `:378`, `:633`, `:71`). No repo-level contradiction exists — the file is untracked, so it is not part of the merged tree. Recorded so the merge card does not look for a commit that cannot exist.

---

## What the branches get RIGHT (no action needed)

**`p1/ipc-backoff` — correct on every point the card set.**

- **Bounded backoff.** `crates/loom-ipc/src/client.rs:12-29` — `CONNECT_ATTEMPTS = 8`, `CONNECT_BACKOFF_BASE = 200ms`, `CONNECT_BACKOFF_MAX = 1s`, `CONNECT_ATTEMPT_TIMEOUT = 1s`; the interval doubles and is capped (`client.rs:83-84`: `delay.saturating_mul(2).min(CONNECT_BACKOFF_MAX)`), so the policy is a fixed, bounded window.
- **Happy path = exactly one connect, no sleep.** `client.rs:74-81` returns on the first successful `timeout(CONNECT_ATTEMPT_TIMEOUT, UnixStream::connect(..))`; the `sleep` is only reached after a failure (`client.rs:82-85`). Proven by a real test, not by construction: `crates/loom-cli/src/ipc_client.rs:136-157` `a_live_daemon_is_used_without_a_retry` counts accepts on a real `UnixListener` and asserts `daemon.connections() == 1` **and** `elapsed < 500ms`; and `crates/loom-ipc/src/client.rs:214-233` `connect_backoff_does_not_wait_for_a_live_daemon` passes a 30 s base delay and asserts `elapsed < 5s`.
- **The error names cause and remedy.** `crates/loom-cli/src/ipc_client.rs:12-16` `stale_socket_advice` → "daemon socket is stale (loomd is not running) — run `loom daemon restart` ({detail})"; `ipc_client.rs:51` → "loomd did not come up — run `loom daemon restart` ({e})". The remedy is a real command: `crates/loom-cli/src/cli.rs:496-497` defines `Restart`, handled at `crates/loom-cli/src/commands/daemon.rs:702-705`.
- **The CLI does not delete the socket.** The branch adds no `remove_file` (`git diff main..p1/ipc-backoff -- crates/loom-cli/src/commands/daemon.rs` contains no removal). The one `remove_file` in the CLI, `crates/loom-cli/src/commands/daemon.rs:90`, is pre-existing on `main`, unchanged by the branch, and runs only in `start_daemon` *after* the single-instance lock probe proves no daemon holds the lock — the daemon's own lifecycle, not the CLI deleting a socket it does not own.
- **One owner for the retry.** The retry loop exists **only** in `crates/loom-ipc/src/client.rs:65-93`. `crates/loom-cli` contains no loop — it only calls `connect_with_backoff` (`crates/loom-cli/src/ipc_client.rs:47`). The other `UnixStream::connect` sites in the tree are pre-existing on `main` and carry no retry: `crates/loom-ipc/src/client.rs:138` (`send_command_inner`, the single-shot path used by `probe_daemon`), `crates/loom-tui/src/dispatch.rs:509` (ACP dispatch), `crates/loom-cli/src/commands/daemon.rs:22` (`daemon_running` probe).
- **No new dependency, no new CLI group, no new crate.** `git diff main..p1/ipc-backoff --stat -- '*Cargo.toml'` is empty; `tempfile.workspace = true` was already in `crates/loom-ipc/Cargo.toml` and `crates/loom-cli/Cargo.toml` `[dev-dependencies]`.
- **Test honesty.** All six new tests use `tempfile::tempdir()` and never touch the real `~/.loom` (every path is under the temp dir; no `loom_dir()` reference in the two changed source files). `connect_backoff_is_bounded` (`client.rs:180-192`) asserts `err.attempts == 3`; `the_default_backoff_covers_a_slow_daemon_start` (`client.rs:199-211`) asserts the policy window ≥ 3 s; `a_stale_socket_says_what_to_do` (`ipc_client.rs:161-181`) writes a stale *regular file* at the socket path and asserts both the cause and the remedy; `a_daemon_that_never_comes_up_says_what_to_do` (`ipc_client.rs:185-213`) asserts the bounded attempt count is reported and the total stays under 15 s. Real assertions, not tautologies. `main` had **zero** tests in either file (`crates/loom-ipc/src/client.rs` was 66 lines with no `mod tests`; `crates/loom-cli/src/ipc_client.rs` had no `mod tests`).

**`p1/embedded-pane` — the right approach, executed cleanly; only the agent source is missing (F2).**

- **The fake dispatcher is gone.** `crates/loom-tui/src/dispatch.rs` — the whole `pub struct EmbeddedPane;` + `impl Dispatcher for EmbeddedPane` block is deleted, along with the tautological `test_embedded_pane_available`. No formatted-success string survives.
- **Exactly one owner of spawn+stream.** `App::dispatch_embedded_pane` (`crates/loom-tui/src/app.rs:703-741`) is the only caller of the adapter's spawn: `git grep -n "stream_background\|stream_command" p1/embedded-pane -- crates/loom-tui/src` returns the two definitions (`crates/loom-tui/src/adapter.rs:394`, `:430`), the one internal call (`adapter.rs:435`) and **one** external caller (`app.rs:728`). The TUI's own duplicate spawn in `crates/loom-tui/src/lib.rs` (the old `KeyCode::Char('e')` block) is removed; both entry points — the dispatch menu's Enter (`lib.rs:180-183`) and the `e` shortcut (`lib.rs:259-265`) — now call `App::dispatch_embedded_pane`. One owner, two callers, no second implementation.
- **The test asserts the work, not a string.** `crates/loom-tui/src/app.rs:2017-2038` spawns against `/bin/echo` and asserts the streamed line contains `workflow dispatch task-42 --agent claude-code` — i.e. it asserts the command actually spawned. The card's "must assert the dispatch path ATTEMPTS the work, not a formatted success string" is satisfied. (Its weakness is the payload, not the assertion — see F2.)
- **The doc is honest about what was not verified.** `docs/loom-improvements.md:24` now says the dispatcher "is now the single owner of the embedded-pane path, covered by a test that asserts the `loom workflow dispatch <task> --agent <agent>` command is actually spawned. **Still open: interactive end-to-end verification of the embedded-pane dispatch path in a live TUI.**" That is exactly the disclosure the card demanded ("If you cannot run the real TUI interactively, say so explicitly with the exact blocker; do NOT claim end-to-end verification").
- **Scope respected.** `git diff --name-only main..p1/embedded-pane` = `VISION.md`, `crates/loom-tui/src/app.rs`, `crates/loom-tui/src/dispatch.rs`, `crates/loom-tui/src/lib.rs`, `docs/loom-improvements.md`. `app.rs` was explicitly in scope ("only if pane state must move" — it must: `dispatch_embedded_pane` owns `embedded_task`/`embedded_log_rx`/`embedded_child`/`embedded_stdin`, `app.rs:729-733`). No `Cargo.toml` change, no new dependency, no new CLI group, no new crate. The `lib.rs` diff also restructures the Enter handler's dispatcher table (the removed `EmbeddedPane` shifted the indices, `lib.rs:184-209`); that is forced by the deletion and is minimal.
- **Tests green at the branch tip**: `cargo test -p loom-tui` → 66 passed, 0 failed; `cargo check -p loom-tui` → exit 0; `cargo clippy -p loom-tui --all-targets` → 0 warnings.

**`p1/backlog-reconcile` — claims verified against the code; the routing-debt record is honest.**

Every `file:line` claim added to `docs/routing-debt.md` was checked on `main` and is exact:

- Step 1 — `crates/loom-routing/src/scorer.rs:83` `let description_length = prompt.chars().count();`; parity test `scorer.rs:291 fn description_length_counts_chars_not_bytes`. ✔
- Step 3 — `crates/loom-core/src/types.rs:175 pub fn derive_level(numeric: f64, thresholds: &ComplexityThresholds) -> ComplexityLevel`, `:188 pub fn derive_tier(level: ComplexityLevel) -> ModelTier`; call sites `crates/loom-routing/src/router.rs:37`, `crates/loom-workflow/src/task_manager.rs:246` and `:405`, `crates/loom-cli/src/commands/routing.rs:179` and `:230`. ✔
- Step 4 — the Supplement lines as listed in F9, consistent with `crates/loom-routing/src/scorer.rs:157` (`numeric += keyword_score * 0.35;`) and `scorer.rs:144` (÷2000 × 0.15). ✔
- Step 5 — `crates/loom-core/src/plugin_registry.rs:9 fn validate_https_url(url: &str) -> Result<()>`, called at `:145` (install) and `:262` (update); tests `plugin_registry.rs:345` and `:357`; `crates/loom-core/src/types.rs:43 pub require_signature: bool`; honoured at `crates/loom-cli/src/commands/plugin.rs:112` and `crates/loom-hooks/src/lib.rs:190` (`if require_signature && hook.signature.is_none()`), with `plugin_registry.rs:57-65` refusing an unsigned plugin. ✔
- Step 6 — `crates/loom-routing/src/outcomes.rs:35 pub struct RoutingOutcomeStore`, wired at `crates/loom-workflow/src/task_manager.rs:53,63` and `crates/loom-cli/src/commands/routing.rs:363,386`. ✔
- Threshold facts — `crates/loom-core/src/types.rs:162-167` (`low_max` 0.35, `high_min` 0.7), `:175` (`derive_level`), `:1099-1105`, test `:1109 fn custom_thresholds_resolve_contradiction_case`; `crates/loom-routing/src/scorer.rs:322` and `:344`. ✔
- `packages/routing-hook/scripts/test_routing_hook.py` has 7 `def test` functions. ✔
- The Pillar-1 daemon-lifecycle bullet is now struck and points at the real file `crates/loomd/tests/daemon_lifecycle.rs` (present on `main`, 8527 bytes). ✔
- `docs/routing-debt.md` keeps the original diagnosis text under the "CLOSED 2026-10-21 — this is a historical record" banner and keeps the decision record (NO on Jev/Laya/Microsoft-Decision-1/Prism) and its flip conditions binding — which is what the card asked for.

**Cross-document scan — everything else agrees.** Pillar-1 bullet sets compared one by one, `ROADMAP.md:21-26` vs `docs/loom-improvements.md:20-27`:

| item | ROADMAP.md | docs/loom-improvements.md | verdict |
|---|---|---|---|
| Daemon lifecycle | closed (`:21`) | closed (`:21`) | agree |
| IPC protocol hardening | backoff **landed** (`:22`) | backoff **still open** (`:23`) | **disagree — F1** |
| Finish-or-delete sweep | embedded-pane verification open (`:23`) | interactive verification open (`:24`, after the branch) | agree |
| Release automation | landed (`:24`) | landed (`:25`) | agree |
| Daemon-lifecycle tests | landed (`:26`) | landed (`:26`) | agree |
| Docs coherence | done (`:25`) | done (`:27`) | agree |

`STATUS.md` and `VISION.md` contain no pillar-1/backoff text, so there is no third document in the loop. `ROADMAP.md:80` is a dated 2026-10-20 sprint-log entry ("Still open: launchd/systemd unit ownership, SIGTERM handling, daemon-lifecycle integration tests") — a historical record of a past sprint, not a live claim, and all three items have since landed; not a contradiction. The only "landed vs still open" collision in the repo is F1.

---

## VERDICT

# FIX FIRST

The approach in all three branches is right; the delivery is not complete. No rework is called for: `p1/ipc-backoff`'s design is correct and should land as-is, `p1/backlog-reconcile`'s reconciliation is accurate apart from two clauses, and `p1/embedded-pane`'s DELETE approach (one owner, no fake dispatcher, honest disclosure of what was not verified interactively) is the shape this review wants.

Required before merge, in order of severity:

1. **F1** — resolve `docs/loom-improvements.md:23` at merge time to the landed backoff statement (the merge conflicts on exactly this bullet; use the `ROADMAP.md:22` wording).
2. **F2** — `p1/embedded-pane`: give the embedded-pane path an agent it can actually obtain (task manifest, `workflow show --json`, or let `loom workflow dispatch` resolve `--agent` itself) and drive the test from the real `loom workflow list --json` payload. Until then the branch removes a lie and replaces it with a path that never spawns, and `VISION.md:75` overstates what works.
3. **F3** — clear the four stale `EmbeddedPane` references (`VISION.md:24`, `VISION.md:63`, `AGENTS.md:318`, `AGENTS.md:331`) and make `VISION.md:75` describe the fixed behaviour.
4. **F4** — `docs/routing-debt.md:9-11`: correct the "via the `debt/*` branches" attribution for Steps 0–1 (they were direct commits, `868cf09` and `f4a5f61`).
5. **F5** — either extend the guidance to `loom status` / `loom doctor` / `loom telemetry` (`commands/status.rs:22`, `commands/doctor.rs:198`, `commands/telemetry.rs:63,83` still use the raw `loom_ipc::send_command` and print no remedy) or narrow the `ROADMAP.md:22` claim to the commands that actually get it.
6. **F6** — move the "Pillar 3" status note (`docs/loom-improvements.md:37`) under the `### Pillar 3 — Ecosystem & Protocol` heading (`:29`) or drop the pillar number from it.

Nothing else blocks. `p1/ipc-backoff` is ship-quality; `p1/backlog-reconcile` is otherwise accurate.

---

## Commands run, and results

| # | command | where | result |
|---|---|---|---|
| 1 | `git worktree add .worktrees/p1-review -b p1/review main` | repo | created at `b77b5cf` |
| 2 | `git merge p1/ipc-backoff` then `git merge p1/backlog-reconcile` | `.worktrees/p1-review` | clean, no conflicts, `4a7cd9b` |
| 3 | `cargo test -p loom -p loom-ipc -p loom-tui` | `.worktrees/p1-review` (2-way merge) | **exit 0** — loom 52, loom-ipc 7, loom-tui 66, 0 failures |
| 4 | `cargo test --workspace` | `.worktrees/p1-review` (2-way merge) | **exit 0** — no failures |
| 5 | `cargo check --workspace` | `.worktrees/p1-review` (2-way merge) | **exit 0** — "Finished dev profile" |
| 6 | `cargo clippy --workspace --all-targets` | `.worktrees/p1-review` (2-way merge) | **exit 0** — 0 warning lines, 0 error lines |
| 7 | `cargo test -p loom -p loom-ipc -p loom-tui` | `main` (**baseline, pre-merge**) | **exit 0** — loom 49, loom-ipc 4, loom-tui 66 → **no pre-existing failures** to separate out |
| 8 | `git merge p1/embedded-pane` | `.worktrees/p1-review` | **CONFLICT** in `docs/loom-improvements.md` (F1) — resolved test-only at `687e6f5` with the F1 wording |
| 9 | `cargo test -p loom -p loom-ipc -p loom-tui` | `.worktrees/p1-review` (3-way merge, conflict resolved) | **exit 0** — loom 52, loom-ipc 7, loom-tui 66, 0 failures |
| 10 | `cargo test --workspace` | `.worktrees/p1-review` (3-way merge) | **exit 0** — 0 failures |
| 11 | `cargo check --workspace` | `.worktrees/p1-review` (3-way merge) | **exit 0** |
| 12 | `cargo clippy --workspace --all-targets` | `.worktrees/p1-review` (3-way merge) | **exit 0** — 0 warning lines, 0 error lines |
| 13 | `cargo test -p loom-tui` | `.worktrees/p1-embedded-v2` (detached at `1dcd0f0`) | **exit 0** — 66 passed, 0 failed (incl. `app::tests::embedded_pane_dispatch_attempts_the_workflow_dispatch`) |
| 14 | `cargo check -p loom-tui` | `.worktrees/p1-embedded-v2` (`1dcd0f0`) | **exit 0** |
| 15 | `cargo clippy -p loom-tui --all-targets` | `.worktrees/p1-embedded-v2` (`1dcd0f0`) | **exit 0** — 0 warning lines |
| 16 | `loom workflow list --json` (built CLI, `LOOM_AGENTS_DIR` = temp dir) | `.worktrees/p1-embedded-v2` | payload has **no `last_agent`** → F2 |
| 17 | `loom workflow show <id> --json` | same | `"last_agent": "goose"` present → the data exists (F2) |
| 18 | task manifest `workflow/tasks/<id>.json` | same | `"last_agent": "goose"` present (F2) |
| 19 | probe test feeding the real list payload to `App::dispatch_embedded_pane` | `.worktrees/p1-embedded-v2` (throwaway, not committed) | `success=false message=Task … has no recorded agent to dispatch` → **F2 proof** |
| 20 | `cargo test -p loom-cli` | `.worktrees/p1-review` | fails: "package ID specification `loom-cli` did not match any packages" — the package is named `loom` (F7) |
| 21 | `git diff --stat main..p1/embedded-pane -- '*Cargo.toml'` | repo | empty — no new dependency on any branch |

Command 7 was run on `main` deliberately, so any failure in the merged runs could be attributed to the branches rather than to the repo. It was green, so every merged result above is attributable to the branches — except the documentation defect F1, which no compiler can see.

Environment note: `timeout` is not available on this macOS host ("timeout: command not found"), so a bounded `cargo check` had to be re-run unbounded. Two throwaway worktrees (`.worktrees/p1-embedded-v2`, `.worktrees/p1-embedded-review`) were removed after use; `.worktrees/` was not touched in `.gitignore`.

---

## Reproducing the findings

```
git -C .worktrees/p1-review log --oneline -1                    # 687e6f5 (3-way merge, test-only conflict resolution)
grep -n "backoff" .worktrees/p1-review/ROADMAP.md .worktrees/p1-review/docs/loom-improvements.md
git -C .worktrees/p1-review merge p1/embedded-pane              # conflicts on docs/loom-improvements.md
git grep -n "stream_background\|stream_command" p1/embedded-pane -- crates/loom-tui/src
git --no-pager show p1/embedded-pane:crates/loom-tui/src/app.rs | sed -n '700,745p'
git --no-pager show p1/embedded-pane:crates/loom-cli/src/commands/workflow.rs | sed -n '18,42p'   # list JSON: no last_agent
git --no-pager show p1/embedded-pane:crates/loom-tui/src/app.rs | sed -n '2010,2045p'            # the test's injected payload
grep -n "EmbeddedPane" .worktrees/p1-review/VISION.md .worktrees/p1-review/AGENTS.md
grep -rn "send_command" .worktrees/p1-review/crates/loom-cli/src/commands/{status,doctor,telemetry}.rs
```

## Notes for the merge card (`589eeb88`)

- Merging all three branches produces a green build and a green test suite, but a tree whose two roadmaps contradict each other (F1) and whose embedded-pane path cannot spawn (F2). Both must be fixed in the merge, not after it.
- `p1/embedded-pane` must not be merged as-is: it deletes the lie but leaves a path that never reaches the spawn.
- Do not touch `.gitignore`; `planning/` stays ignored, and the Supplement re-sync is a working-tree artifact only (F9).
