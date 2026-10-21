//! The posture-downgrade diagnostic.
//!
//! `restricted` resolves to the `none` engine posture because the allowlist
//! is not enforced yet. The operator asked for one thing and got another, so
//! the runtime has to say so — a silent downgrade reads like the allowlist is
//! in force. The empty-allowlist case is the same problem in a worse shape:
//! `restricted:` asks for "these hosts" and names none.

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
fn a_restricted_allowlist_is_reported_as_not_enforced() {
    let logged = resolve_with_capture("restricted:api.example.com");

    // The operator must be able to tell, from the log, that the allowlist is
    // not in force yet.
    assert!(
        logged.contains("not enforced"),
        "the diagnostic must say the allowlist is not enforced: {logged}"
    );
    // …and which hosts were asked for.
    assert!(
        logged.contains("api.example.com"),
        "the diagnostic must name the requested hosts: {logged}"
    );
    // …and that the posture actually in force is the total-isolation one.
    assert!(
        logged.contains("posture=none") || logged.contains("none"),
        "the diagnostic must state the resolved posture: {logged}"
    );
}

#[test]
fn an_empty_allowlist_is_reported_too() {
    // `restricted` with no host list and `restricted:` with an empty one are
    // the same request, and both mean "no hosts" — which must not be silent.
    for requested in ["restricted", "restricted:", "restricted: , "] {
        let logged = resolve_with_capture(requested);
        assert!(
            logged.contains("not enforced"),
            "an empty allowlist must still be reported ({requested}): {logged}"
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
            !logged.contains("not enforced"),
            "{requested} must not produce a downgrade diagnostic: {logged}"
        );
    }
}

#[test]
fn the_diagnostic_exists_on_the_posture_itself() {
    // The diagnostic belongs to the posture, so it is reachable without an
    // engine — and it is absent for the postures that need none.
    assert!(NetworkPolicy::NoNetwork.diagnostic().is_none());
    assert!(NetworkPolicy::Unrestricted.diagnostic().is_none());

    let diagnostic = NetworkPolicy::parse("restricted:api.example.com")
        .unwrap()
        .diagnostic()
        .expect("restricted needs a diagnostic");
    assert!(diagnostic.contains("not enforced"));
    assert!(diagnostic.contains("api.example.com"));

    let empty = NetworkPolicy::parse("restricted")
        .unwrap()
        .diagnostic()
        .expect("an empty allowlist needs a diagnostic");
    assert!(empty.contains("not enforced"));
    assert!(empty.contains("empty"));
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
