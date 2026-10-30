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

- feat(cli): `loom export --plain-files` — export sessions as plain, tool-agnostic files for lock-in prevention
- docs(readme): add Mermaid architecture diagram + architecture.mmd source
- docs(readme): fix Mermaid architecture diagram syntax
- docs(diagram): update architecture diagram for v0.12.0
- docs(release): update README and homebrew formula for v0.12.0

### Fixed

- fix(security): harden outbound URL validation against SSRF
- fix(security): remediate Trivy + Semgrep SAST findings

### Security

- security: fixes