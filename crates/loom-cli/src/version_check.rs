//! CLI ↔ daemon version-skew detection.
//!
//! The daemon reports its version in the `health.check` payload
//! (`data.version`). A CLI talking to a daemon built from a different
//! revision can hit missing/malformed fields in other commands, so the
//! mismatch has to be surfaced instead of silently reported as healthy.

/// Compare the daemon-reported version with the CLI's own version.
///
/// Returns `None` when the versions match, otherwise a human-readable
/// message describing the skew (including the "daemon reported no version"
/// case, which is reported as unknown rather than as a match).
pub(crate) fn version_skew(daemon_version: Option<&str>, cli_version: &str) -> Option<String> {
    let cli = cli_version.trim();
    match daemon_version.map(str::trim).filter(|v| !v.is_empty()) {
        Some(daemon) if daemon == cli => None,
        Some(daemon) => Some(format!(
            "Version mismatch: CLI {cli}, daemon {daemon} — restart the daemon: loom daemon restart"
        )),
        None => Some(format!(
            "Daemon version unknown (CLI {cli}) — daemon reported no version, it may be an older build. Restart the daemon: loom daemon restart"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::version_skew;

    #[test]
    fn test_version_skew_same_version_is_none() {
        assert_eq!(version_skew(Some("0.9.6"), "0.9.6"), None);
    }

    #[test]
    fn test_version_skew_different_version_reports_both() {
        let msg = version_skew(Some("0.9.3"), "0.9.6").expect("mismatch must be reported");
        assert!(msg.contains("0.9.6"), "missing CLI version in: {msg}");
        assert!(msg.contains("0.9.3"), "missing daemon version in: {msg}");
    }

    #[test]
    fn test_version_skew_missing_daemon_version_is_unknown() {
        let msg = version_skew(None, "0.9.6").expect("unknown version must be reported");
        assert!(
            msg.to_lowercase().contains("unknown"),
            "message should say unknown: {msg}"
        );
    }
}
