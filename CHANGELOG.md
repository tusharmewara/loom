# Changelog

All notable changes to the Loom project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.0] - 2026-09-16

### Added

- feat(loomd): tracked ACP dispatch runs with disk-persisted registry (`run.status`, restart stale-marking, 30-day retention)
- feat(loomd): single tracked dispatch path for IPC and TCP (headless spawn, per-run logs, session capture, prior-session context)
- feat(loomd): canonical ACP v1 protocol client (`initialize` → `session/new` → `session/prompt`, stop reasons, permission policy)
- feat(cli): `loom acp run` runs a canonical ACP prompt turn and persists an indexed session
- feat(cli): `loom acp agents` lists launchable agents (daemon handlers plus agents.toml entries)
- feat(cli): `loom acp run-status` shows a tracked run's status, exit code, and session
- feat(cli): `loom workflow create --from-plan` imports chief plans as tagged tasks
- feat(cli): `loom workflow create --from-ticket` creates tasks from GitHub/Linear/GitLab/Jira tickets
- feat(cli): `loom workflow draft-plan [--create]` drafts task plans from free text with preview-then-confirm
- feat(cli): `loom workflow remove` deletes a task and its per-phase sidecars
- feat(cli): `loom issues create-pr` opens a pull request via `gh`
- feat(cli): `loom remote exec --capture-session` persists remote output as an indexed session
- feat(tui): Orchestrator intake tab (create task, tracked dispatch, run polling)
- feat(tui): Review tab (task list, detail, dispatch/resume/reassign/remove)
- feat(workflow): chief plan import, deterministic draft plans, task tags

### Changed

- fix(cli): allow hyphen-leading `--title`/`--description` values on `workflow create`/`draft-plan`
- chore(tui): dynamic tab indices; `0` selects Orchestrator, Review via Tab cycling and palette
- docs: corrected ACP/EmbeddedPane gotchas and harness comparison for the above

### Fixed

### Security

## [0.7.2] - 2026-09-01

### Added

### Changed

- Fix: isolate environment-variable tests in child process to prevent flakiness
- chore: fill real source SHA for v0.7.1 homebrew formula

### Fixed

### Security

## [0.7.1] - 2026-08-30

### Added

- feat(cli): add `loom edit` and `loom conflict` CLI commands (P2/5.3, P2/5.7)
- feat(acp): add CLI commands and IPC handler for ACP dispatch server (P1/2.5)
- feat(acp): wire ACP dispatch server into daemon startup (P1/2.5)

### Fixed

- test(core): isolate env-var test in child process to fix flakiness

## [0.7.0] - 2026-08-22

### Added

- feat(ast): AST structural editing with tree-sitter (P1/5.5)
- feat(rules): CLI `loom rules list/show/add/remove/eval` commands (P1/5.6)
- feat(rules): stream rule engine with condition matching and evaluation (P1/5.6)
- feat(conflict): conflict resolution protocol with scan/resolve/bulk operations (P1/5.7)
- feat(adapters): add Kiro adapter (P1/1.2)

## [0.6.1] - 2026-08-22

### Changed

- chore: bump version to 0.6.1

## [0.6.0] - 2026-08-22

### Added

- feat(hindsight): cross-session typed fact memory with retain/recall/reflect (P1/5.4)
- feat(skills): hashline edit system with content-hash anchored edits (P1/5.3)
- feat(acp): peer discovery and secure handshake (P1/2.5)
- feat: debug provider trait and adapter integration
- feat: DAP debugging integration (5.2)
- feat: LSP integration with skill adapters (5.1)
