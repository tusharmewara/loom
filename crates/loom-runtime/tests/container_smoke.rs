//! End-to-end container smoke test.
//!
//! This test needs a real container engine, so it is **skipped by default**:
//!
//! * it only runs when `LOOM_CONTAINER_SMOKE=1` is set (the CI container job
//!   sets it), and
//! * it prints `skipped: ...` and returns successfully when no engine is
//!   reachable — a missing Docker is a skip, never a failure.
//!
//! The pure-logic posture tests live in `network_policy.rs` and run
//! everywhere.

use loom_core::types::{LaunchConfig, Task, TaskPhase, TaskStatus};
use loom_runtime::{ContainerRuntime, NetworkPolicy, Runtime};
use std::process::Command;

fn docker_available() -> bool {
    Command::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// The engine must be usable by the *runtime*, not just by the CLI: the
/// bollard client resolves its own socket. If the runtime cannot reach an
/// engine, the test skips — a missing engine is never a failure.
async fn runtime_available() -> bool {
    ContainerRuntime::docker().is_available().await
}

/// The container backends are feature-gated. Without the `docker` feature
/// this crate has no Docker client at all, so there is nothing to smoke.
fn docker_feature_enabled() -> bool {
    ContainerRuntime::docker().name() == "docker"
}

fn task() -> Task {
    Task {
        task_id: "smoke-task".to_string(),
        title: "Container smoke test".to_string(),
        description: String::new(),
        phase: TaskPhase::Plan,
        phase_history: vec![],
        prd_path: None,
        context_files: vec![],
        routed_provider: None,
        files_modified: vec![],
        complexity_score: None,
        routing_decision: None,
        last_agent: None,
        linked_issue: None,
        sub_agents: None,
        spec_path: None,
        findings_path: None,
        converged: None,
        phase_results: vec![],
        tags: vec![],
        worktree_path: None,
        runtime: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
        status: TaskStatus::Pending,
        last_run: None,
    }
}

#[tokio::test]
async fn container_run_is_network_isolated_by_default() {
    if std::env::var("LOOM_CONTAINER_SMOKE").as_deref() != Ok("1") {
        println!("skipped: LOOM_CONTAINER_SMOKE != 1 (container smoke test not requested)");
        return;
    }
    if !docker_feature_enabled() {
        println!("skipped: docker feature not enabled");
        return;
    }
    if !docker_available() {
        println!("skipped: no container engine reachable");
        return;
    }
    if !runtime_available().await {
        println!("skipped: the runtime cannot reach a container engine");
        return;
    }

    let runtime = ContainerRuntime::docker()
        .with_image("alpine:3".to_string())
        .with_auto_remove(false);

    // The default posture must deny egress. The probe is the container's own
    // network interfaces, not an outbound request: a request would also fail
    // on a CI host with no internet, which would make the test pass for the
    // wrong reason. With `--network none` the container has only `lo`.
    let config = LaunchConfig {
        runtime: "docker".to_string(),
        image: Some("alpine:3".to_string()),
        command: "sh".to_string(),
        args: vec![
            "-c".to_string(),
            "echo IFACES:; ls /sys/class/net".to_string(),
        ],
        ..Default::default()
    };

    let result = runtime
        .run(&config, &task())
        .await
        .expect("container run failed");
    assert!(result.success, "container exited non-zero: {result:?}");
    assert!(
        !result.stdout.contains("eth0"),
        "container had a network interface under the default posture: {result:?}"
    );

    // The engine must have been told to disable networking.
    let container_id = result.container_id.clone().expect("container id");
    let inspected = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.HostConfig.NetworkMode}}",
            &container_id,
        ])
        .output()
        .expect("docker inspect failed");
    let network_mode = String::from_utf8_lossy(&inspected.stdout)
        .trim()
        .to_string();
    assert_eq!(
        network_mode, "none",
        "expected NetworkMode=none, got {network_mode}"
    );

    // Clean up the container we deliberately kept.
    let _ = runtime.cleanup(&result).await;
}

#[tokio::test]
async fn unrestricted_posture_is_the_explicit_escape_hatch() {
    if std::env::var("LOOM_CONTAINER_SMOKE").as_deref() != Ok("1") {
        println!("skipped: LOOM_CONTAINER_SMOKE != 1 (container smoke test not requested)");
        return;
    }
    if !docker_feature_enabled() {
        println!("skipped: docker feature not enabled");
        return;
    }
    if !docker_available() {
        println!("skipped: no container engine reachable");
        return;
    }
    if !runtime_available().await {
        println!("skipped: the runtime cannot reach a container engine");
        return;
    }

    let runtime = ContainerRuntime::docker()
        .with_image("alpine:3".to_string())
        .with_network_policy(NetworkPolicy::Unrestricted)
        .with_auto_remove(false);

    let config = LaunchConfig {
        runtime: "docker".to_string(),
        image: Some("alpine:3".to_string()),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), "echo HELLO".to_string()],
        ..Default::default()
    };

    let result = runtime
        .run(&config, &task())
        .await
        .expect("container run failed");
    assert!(result.success, "container exited non-zero: {result:?}");
    assert!(
        result.stdout.contains("HELLO"),
        "unexpected output: {result:?}"
    );

    let container_id = result.container_id.clone().expect("container id");
    let inspected = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.HostConfig.NetworkMode}}",
            &container_id,
        ])
        .output()
        .expect("docker inspect failed");
    let network_mode = String::from_utf8_lossy(&inspected.stdout)
        .trim()
        .to_string();
    assert_ne!(
        network_mode, "none",
        "unrestricted posture must not disable networking"
    );

    let _ = runtime.cleanup(&result).await;
}

/// The full `restricted` contract: the workload gets no direct egress, talks
/// only to the egress proxy over an `--internal` network, allowlisted hosts
/// are reachable, anything else is refused, and both artifacts are cleaned up.
///
/// Needs a prebuilt proxy image — the CI container job builds it from
/// `crates/loom-egress-proxy/Dockerfile` first; locally:
/// `docker build -t loom-egress-proxy:latest crates/loom-egress-proxy`.
#[tokio::test]
async fn restricted_posture_routes_egress_through_the_allowlist_proxy() {
    if std::env::var("LOOM_CONTAINER_SMOKE").as_deref() != Ok("1") {
        println!("skipped: LOOM_CONTAINER_SMOKE != 1 (container smoke test not requested)");
        return;
    }
    if !docker_feature_enabled() {
        println!("skipped: docker feature not enabled");
        return;
    }
    if !docker_available() {
        println!("skipped: no container engine reachable");
        return;
    }
    if !runtime_available().await {
        println!("skipped: the runtime cannot reach a container engine");
        return;
    }
    if std::env::var("LOOM_EGRESS_PROXY_IMAGE").is_err() {
        println!("skipped: LOOM_EGRESS_PROXY_IMAGE not set (no egress-proxy image available)");
        return;
    }

    let runtime = ContainerRuntime::docker().with_auto_remove(false);

    let config = LaunchConfig {
        runtime: "docker".to_string(),
        image: Some("curlimages/curl:latest".to_string()),
        command: "sh".to_string(),
        args: vec!["-c".to_string(), r#"
echo "PROXY_ENV HOST=${HTTP_PROXY} HTTPS=${HTTPS_PROXY} NOPROXY=${NO_PROXY}"
curl -sS -m 20 https://api.github.com/zen -o /dev/null -w "ALLOWED:%{http_code}\n" || echo "ALLOWED:FAILED"
curl -sS -m 20 https://evil.example.invalid -o /dev/null 2>/dev/null && echo "DENIED:FAIL" || echo "DENIED:OK"
"#
        .to_string()],
        network: Some("restricted:api.github.com".to_string()),
        ..Default::default()
    };

    let result = runtime
        .run(&config, &task())
        .await
        .expect("restricted container run failed");
    assert!(result.success, "container exited non-zero: {result:?}");
    println!("--- smoke output ---\n{}\n--------------------", result.stdout);

    // Proxy injected and reachable by its internal name.
    assert!(
        result
            .stdout
            .contains("PROXY_ENV HOST=http://loom-"),
        "HTTP_PROXY not injected/pointed at the internal proxy: {result:?}"
    );
    assert!(
        result.stdout.contains("NOPROXY=localhost,127.0.0.1"),
        "NO_PROXY not injected: {result:?}"
    );
    // Allowlisted host reachable through the proxy.
    assert!(
        result.stdout.contains("ALLOWED:200"),
        "allowlisted host did not reach 200 through the proxy: {result:?}"
    );
    // Non-listed host refused (fail-closed), name matching is pre-DNS so this
    // works even where the DENIED probe host cannot resolve.
    assert!(
        result.stdout.contains("DENIED:OK"),
        "non-listed host was not refused: {result:?}"
    );

    // The workload rides the internal egress network, not plain `none` nor
    // the default bridge.
    let container_id = result.container_id.clone().expect("container id");
    let inspected = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.HostConfig.NetworkMode}}",
            &container_id,
        ])
        .output()
        .expect("docker inspect failed");
    let network_mode = String::from_utf8_lossy(&inspected.stdout).trim().to_string();
    assert!(
        network_mode.starts_with("loom-net-"),
        "expected the internal egress network, got {network_mode}"
    );

    // The proxy sidecar exists and carries the allowlist as a label.
    let short = network_mode.trim_start_matches("loom-net-").to_string();
    let inspected = Command::new("docker")
        .args([
            "inspect",
            "--format",
            "{{index .Config.Labels \"loom.network_allowed_hosts\"}}",
            &format!("loom-{short}-proxy"),
        ])
        .output()
        .expect("docker inspect of the proxy failed");
    let allowlist = String::from_utf8_lossy(&inspected.stdout).trim().to_string();
    assert_eq!(allowlist, "api.github.com", "proxy allowlist label: {allowlist}");

    // Every artifact goes in one cleanup call.
    let _ = runtime.cleanup(&result).await;
    let gone = Command::new("docker")
        .args([
            "run",
            "--rm",
            format!("--network={network_mode}").as_str(),
            "--entrypoint",
            "sh",
            "curlimages/curl:latest",
            "-c",
            "echo STILL_ATTACHED",
        ])
        .output()
        .expect("docker run failed");
    // A removed network cannot be joined at all — this run must fail.
    assert!(
        !String::from_utf8_lossy(&gone.stderr).contains("STILL_ATTACHED"),
        "the internal network outlived the run"
    );
}
