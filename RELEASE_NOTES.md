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

- docs: added review record links to ROADMAP/STATUS
- docs: gated acp_live test behind LOOM_ACP_LIVE=1
- docs: replaced documentation cards

### Changed

- documentation: updated review process and test gating notes
