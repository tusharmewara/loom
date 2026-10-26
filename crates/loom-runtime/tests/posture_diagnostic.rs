//! The posture diagnostic.
//!
//! `restricted` with a **non-empty** allowlist is enforced as-is (the Docker
//! backend composes the egress proxy), so it produces no diagnostic — a
//! diagnostic on every such run would be noise operators learn to ignore
//! (see `an_enforced_posture_is_not_reported_as_a_downgrade`). The
//! **empty** allowlist is different: `restricted:` asks for "these hosts"
//! and names none, so the honest reading of the resulting posture is
//! "deny all egress", and that has to be said explicitly.

use loom_runtime::{resolve_network_policy, NetworkPolicy};
use loom_telemetry::logging::tracing_subscriber;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Captures what the subscriber writes, so the test can see the log line.
#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("capture lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Resolve `requested` with a subscriber installed, and return what was
/// logged while it happened.
fn resolve_with_capture(requested: &str) -> String {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || Capture(sink.clone()))
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        resolve_network_policy(&NetworkPolicy::NoNetwork, Some(requested)).expect("resolve");
    });

    let bytes = buf.lock().expect("capture lock").clone();
    String::from_utf8(bytes).expect("log output is utf-8")
}

#[test]
fn an_enforced_posture_is_not_reported_as_a_downgrade() {
    // A named allowlist is enforced through the egress proxy — saying
    // "downgraded" or "not enforced" here would be a lie.
    let logged = resolve_with_capture("restricted:api.example.com");
    assert!(
        !logged.contains("deny-all"),
        "an enforced allowlist must not be reported as deny-all: {logged}"
    );
    assert!(
        !logged.contains("posture diagnostic"),
        "an enforced allowlist must not produce a diagnostic: {logged}"
    );
}

#[test]
fn an_empty_allowlist_is_reported_too() {
    // `restricted` with no host list and `restricted:` with an empty one are
    // the same request, and both mean "no hosts" — which must not be silent.
    for requested in ["restricted", "restricted:", "restricted: , "] {
        let logged = resolve_with_capture(requested);
        assert!(
            logged.contains("empty"),
            "an empty allowlist must still be reported ({requested}): {logged}"
        );
        assert!(
            logged.contains("deny-all"),
            "the empty-allowlist diagnostic must state the real posture: {logged}"
        );
    }
}

#[test]
fn a_posture_that_was_asked_for_is_not_reported_as_a_downgrade() {
    // `none` and `unrestricted` resolve to themselves — there is nothing to
    // warn about, and a warning on every run would train operators to ignore
    // them.
    for requested in ["none", "unrestricted"] {
        let logged = resolve_with_capture(requested);
        assert!(
            !logged.contains("deny-all"),
            "{requested} must not produce a posture diagnostic: {logged}"
        );
    }
}

#[test]
fn the_diagnostic_exists_on_the_posture_itself() {
    // The diagnostic belongs to the posture, so it is reachable without an
    // engine — and it is absent for the postures that need none.
    assert!(NetworkPolicy::NoNetwork.diagnostic().is_none());
    assert!(NetworkPolicy::Unrestricted.diagnostic().is_none());
    assert!(
        NetworkPolicy::parse("restricted:api.example.com")
            .unwrap()
            .diagnostic()
            .is_none(),
        "an enforced allowlist needs no diagnostic"
    );

    let empty = NetworkPolicy::parse("restricted")
        .unwrap()
        .diagnostic()
        .expect("an empty allowlist needs a diagnostic");
    assert!(empty.contains("empty"));
    assert!(empty.contains("deny-all"));
}

#[test]
fn an_unknown_posture_is_still_an_error_and_never_a_downgrade() {
    // A free-form network name must not resolve to anything, and must not
    // resolve to "unrestricted" either.
    let err = resolve_network_policy(&NetworkPolicy::NoNetwork, Some("bridge")).unwrap_err();
    assert!(err.to_string().contains("unknown network policy"));

    // The fail-closed default is untouched by the diagnostic.
    let policy = resolve_network_policy(&NetworkPolicy::default(), None).unwrap();
    assert_eq!(policy, NetworkPolicy::NoNetwork);
    assert_eq!(policy.cli_args(), vec!["--network", "none"]);
}
