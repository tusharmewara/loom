# Changelog

All notable changes to the Loom project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
