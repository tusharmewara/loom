<!--
Release notes template.

How to use:
  1. Replace the comment block under "## Notes" with the release notes, in the
     style used elsewhere in CHANGELOG.md (e.g. "- feat(cli): ..." bullets, or
     "### Added" / "### Fixed" / "### Changed" subsections).
  2. Run: scripts/release.sh <version>
     The content under "## Notes" is copied into CHANGELOG.md as the body of
     the new "## [<version>] - <date>" section. The "## Notes" heading itself
     is dropped.
  3. If "## Notes" is left empty, the script inserts a visible TODO placeholder
     instead of failing the release.

Only the content under "## Notes" is used; this comment block is ignored.
-->

## Notes

### Added

- feat(review): the Verified Review Engine — new `loom-review` crate and `loom review` CLI (`run`, `status`, `findings`, `scoreboard`, `fix-pr`). A review run extracts a plan from a session, seats cross-family reviewer models over ACP, dedups findings, verifies each in an isolated git worktree via `loom-parallel` + `loom-runtime`, and labels every finding with what it established (`proven` / `suite_green` / `unverified` / `could_not_run` / `refuted`). `--seat` picks the agent per seat; lessons are written only for proven findings
- feat(review): `loom review fix-pr` is a real git flow — branch, commit the verified patches, push, and open the PR via `GitHubTracker::create_pr` (`gh pr create`)
- feat(telemetry): `loom exec` and every TUI task dispatch path (terminal launch, ACP dispatch, runtime dispatch) now record runs in the metered usage ledger (unavailable + `chars/4` forecast, provenance labelled)
- feat(auth): `loom auth rotate-password [--vault-password <pw>] --new-password <pw>` re-encrypts every credential in place and invalidates all derived grants

### Fixed

- fix(acp): fail fast when the ACP agent child dies instead of sleeping out the full timeout
- fix(acp): publish the vendored ACP registry and make `registry_url` configuration live
- test(workflow): pin dispatch usage recording and document embedded-pane dispatch inheritance

### Changed

- docs: close the embedded-pane live-TUI e2e item; retire the usage-metering gap list; add MIT LICENSE and badges
