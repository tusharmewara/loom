//! Pure-logic tests for the container network posture.
//!
//! These run everywhere — no container engine required. The end-to-end
//! container smoke test lives in `container_smoke.rs` and is skipped when no
//! engine is present.

use loom_runtime::{resolve_network_policy, ContainerRuntime, NetworkPolicy, Runtime};

#[test]
fn default_posture_is_no_network() {
    let policy = NetworkPolicy::default();
    assert_eq!(policy, NetworkPolicy::NoNetwork);
    assert_eq!(policy.engine_network_mode(), Some("none"));
    assert_eq!(policy.cli_args(), vec!["--network", "none"]);
}

#[test]
fn container_runtime_defaults_to_no_network() {
    // The runtime's own default posture is the safe one — an unset config
    // must not widen egress.
    assert_eq!(
        ContainerRuntime::docker().network_policy(),
        &NetworkPolicy::NoNetwork
    );
    assert_eq!(
        ContainerRuntime::podman().network_policy(),
        &NetworkPolicy::NoNetwork
    );
}

#[test]
fn no_network_maps_to_no_network_argument() {
    let policy = NetworkPolicy::NoNetwork;
    assert_eq!(policy.engine_network_mode(), Some("none"));
    assert_eq!(policy.cli_args(), vec!["--network", "none"]);
    assert_eq!(policy.name(), "none");
}

#[test]
fn restricted_is_fail_closed() {
    // The engine argument always stays maximum-deny: enforcement of the
    // allowlist happens through the egress proxy the Docker backend
    // composes, and direct egress can never bypass it.
    let policy = NetworkPolicy::parse("restricted:10.0.0.0/8,example.com").unwrap();
    assert_eq!(policy.engine_network_mode(), Some("none"));
    assert_eq!(policy.cli_args(), vec!["--network", "none"]);
    assert_eq!(policy.name(), "restricted");
    assert_eq!(
        policy.allowed_hosts(),
        ["10.0.0.0/8".to_string(), "example.com".to_string()]
    );
}

#[test]
fn unrestricted_is_the_explicit_escape_hatch() {
    // The only posture that leaves the engine's default network in place —
    // and it has to be asked for by name.
    let policy = NetworkPolicy::parse("unrestricted").unwrap();
    assert_eq!(policy, NetworkPolicy::Unrestricted);
    assert_eq!(policy.engine_network_mode(), None);
    assert!(policy.cli_args().is_empty());
    assert_eq!(policy.name(), "unrestricted");
}

#[test]
fn absence_never_means_unrestricted() {
    // The regression this policy exists for: the *absence* of a value used to
    // mean "engine default network" (unrestricted egress).
    for policy in [
        NetworkPolicy::NoNetwork,
        NetworkPolicy::parse("restricted").unwrap(),
    ] {
        assert!(policy.engine_network_mode().is_some());
        assert!(!policy.cli_args().is_empty());
    }
}

#[test]
fn parse_rejects_free_form_network_names() {
    // A network name is not a posture. "bridge" used to be a valid value and
    // silently granted the default network.
    let err = NetworkPolicy::parse("bridge").unwrap_err();
    assert!(err.to_string().contains("unknown network policy"));

    let err = NetworkPolicy::parse("host").unwrap_err();
    assert!(err.to_string().contains("unknown network policy"));

    assert!(NetworkPolicy::parse("").is_err());
}

#[test]
fn parse_accepts_the_three_postures() {
    assert_eq!(
        NetworkPolicy::parse("none").unwrap(),
        NetworkPolicy::NoNetwork
    );
    assert_eq!(
        NetworkPolicy::parse("NONE").unwrap(),
        NetworkPolicy::NoNetwork
    );
    assert_eq!(
        NetworkPolicy::parse("unrestricted").unwrap(),
        NetworkPolicy::Unrestricted
    );
    assert_eq!(
        NetworkPolicy::parse(" restricted ").unwrap(),
        NetworkPolicy::parse("restricted").unwrap()
    );
}

#[test]
fn absent_value_falls_back_to_the_default_posture() {
    let policy = resolve_network_policy(&NetworkPolicy::default(), None).unwrap();
    assert_eq!(policy, NetworkPolicy::NoNetwork);
}

#[test]
fn per_run_value_wins_over_the_runtime_default() {
    // The runtime default is the fallback; an explicit per-run posture wins.
    let resolved = resolve_network_policy(&NetworkPolicy::NoNetwork, Some("unrestricted")).unwrap();
    assert_eq!(resolved, NetworkPolicy::Unrestricted);

    let resolved = resolve_network_policy(&NetworkPolicy::Unrestricted, None).unwrap();
    assert_eq!(resolved, NetworkPolicy::Unrestricted);

    let resolved = resolve_network_policy(&NetworkPolicy::NoNetwork, None).unwrap();
    assert_eq!(resolved, NetworkPolicy::NoNetwork);
}

#[test]
fn an_unknown_per_run_value_is_an_error_not_a_guess() {
    let err = resolve_network_policy(&NetworkPolicy::NoNetwork, Some("bridge")).unwrap_err();
    assert!(err.to_string().contains("unknown network policy"));
}

#[test]
fn runtime_trait_is_reachable_from_outside_the_crate() {
    // The posture lives on the runtime the caller actually gets.
    let runtime = ContainerRuntime::docker().with_network_policy(NetworkPolicy::Unrestricted);
    assert_eq!(runtime.network_policy(), &NetworkPolicy::Unrestricted);
    assert_eq!(runtime.name(), {
        #[cfg(feature = "docker")]
        {
            "docker"
        }
        #[cfg(all(feature = "podman", not(feature = "docker")))]
        {
            "podman"
        }
        #[cfg(all(
            feature = "kubernetes",
            not(feature = "docker"),
            not(feature = "podman")
        ))]
        {
            "kubernetes"
        }
        #[cfg(not(any(feature = "docker", feature = "podman", feature = "kubernetes")))]
        {
            "container"
        }
    });
}
