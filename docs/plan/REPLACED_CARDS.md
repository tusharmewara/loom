# Replaced Board Cards

This file documents task-board cards that were replaced during the Phase A and Pillars 1–4 work. The original cards failed (typically on context exhaustion / max-tokens) and were archived; narrowed replacements were created and completed successfully.

## P1 · Embedded-pane dispatch (original vs replacement)

- **Original:** `a561d8f2-c1e4-43de-9a9f-771260f83ce8` (archived) — "P1 · Embedded-pane dispatch (retry, narrowed) — ship it or delete it"
- **Replacement:** Same ID `a561d8f2` (the replacement was created after the original was archived)
- **Why:** The original card failed 3 times on "agent turn ended without completing: max-tokens" producing no branch commits. The replacement was narrowed to only read 3 specific line ranges (`dispatch.rs:300-340`, `dispatch.rs:545-575`, `lib.rs:250-290`), explicitly defaulted to option (b) "DELETE" (the fake dispatcher), and added a test asserting the command is actually spawned.
- **Outcome:** The replacement succeeded. Branch `p1/embedded-pane` commit `1dcd0f0` deleted the fake `EmbeddedPane` dispatcher in `crates/loom-tui/src/dispatch.rs`, made `App::dispatch_embedded_pane` the single owner (now resolves the agent from the full task record since `loom workflow list --json` carries no `last_agent`, falling back to `TerminalLaunch::find_agent_binary`), and added `app::tests::embedded_pane_dispatch_attempts_the_workflow_dispatch` which uses a shell stub as the loom binary.

## P2 · Audit export F1 (original vs replacement)

- **Original:** `63ba2c3d-6635-466b-810c-c6bf5b1dd2ed` (archived) — "P2-F1 · Make the audit export self-verifying — recomputable chain + Ed25519 anchor"
- **Replacement:** `e785648a-4979-4d83-817d-021a99e75787` — "P2-F1b · Make the audit export self-verifying — recomputable chain + Ed25519 anchor"
- **Why:** The original card failed on "max-tokens" and committed nothing (branch `p2/audit-export` remained at 45a6c27 with a clean worktree). The replacement explicitly carried the user-chosen design (recomputable chain over normalized outcome + Ed25519 anchor, three verdicts `intact`/`tampered`/`unanchored`, workspace deps only), required 6 mandated tests, and specified working in the existing worktree with early commits.
- **Outcome:** The replacement succeeded. Branch `p2/audit-export` commits `670b2d9` + `b93af49` + `0f3c1d1` + `c8ce2f5` implemented the full design: normalised-outcome chain with chain version (old scheme 1 = raw result, new scheme 2 = normalised outcome), per-machine Ed25519 signing key at `{loom_dir}/auth/export-signing.key`, `loom auth export --print-public-key`, `loom auth verify <file> [--public-key]` printing one verdict, and 7 new tests (3 attack + 3 correct + legacy scheme compatibility + raw-result-never-in-export).

## Why this matters

The board's execution model does not allow `task_board_update` on an already-executed card (prompt/title/description are frozen after a run starts). When a card fails on context exhaustion with no committed work, the only path forward is to `archive` it and create a replacement with a narrower scope that fits within the model's context window. This is expected behaviour for complex work — not a bug in the process.