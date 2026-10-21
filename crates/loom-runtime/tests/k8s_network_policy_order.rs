//! The Kubernetes NetworkPolicy must be created BEFORE the Job.
//!
//! A `NetworkPolicy` selects Pods by label, not by the Pod existing. Creating
//! the policy after the Job leaves a window in which the Job's Pod is already
//! running with the cluster's default egress — the posture is not in force
//! yet when it matters.
//!
//! The test points the Kubernetes client at a local fake API server and
//! records the order of the requests the runtime makes. It asserts the
//! *order*, not the formatting, and it fails if the policy is not created at
//! all — a test that passed when the feature was removed would prove
//! nothing.

#![cfg(feature = "kubernetes")]

use loom_runtime::kubernetes_client::KubernetesClient;
use loom_runtime::{ContainerSpec, NetworkPolicy};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The requests the fake API server saw, in order: the request path and the
/// request body.
type Requests = Arc<Mutex<Vec<(String, String)>>>;

/// A fake API server that records the path of every request and answers with
/// the request body, so the client gets a well-formed object back.
async fn start_fake_api_server() -> (String, Requests) {
    let requests: Requests = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    let recorded = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let Ok(n) = socket.read(&mut buf).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let head = request.split("\r\n\r\n").next().unwrap_or("").to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let content_length = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
                    .and_then(|l| l.split(':').nth(1))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                let mut body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, b)| b.as_bytes().to_vec())
                    .unwrap_or_default();
                while body.len() < content_length {
                    let mut more = vec![0u8; 4096];
                    match socket.read(&mut more).await {
                        Ok(0) | Err(_) => break,
                        Ok(m) => body.extend_from_slice(&more[..m]),
                    }
                }
                let body_text = String::from_utf8_lossy(&body).to_string();
                recorded
                    .lock()
                    .expect("requests lock")
                    .push((path, body_text));

                let head = format!(
                    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            });
        }
    });

    (format!("http://{addr}"), requests)
}

/// A minimal spec, so the runtime can label the Pod.
fn spec(policy: NetworkPolicy) -> ContainerSpec {
    ContainerSpec {
        image: "alpine:3".to_string(),
        cmd: vec!["echo".to_string(), "hi".to_string()],
        env: HashMap::new(),
        working_dir: "/tmp".to_string(),
        volumes: vec![],
        network_policy: policy,
        resource_limits: None,
        labels: HashMap::from([("loom.task_id".to_string(), "order-task".to_string())]),
    }
}

#[tokio::test]
async fn the_network_policy_is_created_before_the_job() {
    let (server_url, requests) = start_fake_api_server().await;

    let config: kube::Config = kube::Config::new(server_url.parse().expect("uri"));
    let client = kube::Client::try_from(config).expect("kube client");
    let kubernetes = KubernetesClient::with_client(client, "default".to_string());

    let spec = spec(NetworkPolicy::NoNetwork);

    let job_name = kubernetes.create_job(&spec).await.expect("create job");

    let seen = requests.lock().expect("requests lock").clone();

    let policy_index = seen
        .iter()
        .position(|(path, _)| path.contains("networkpolicies"))
        .unwrap_or_else(|| {
            panic!("the NetworkPolicy was never created; requests: {seen:?}");
        });
    let job_index = seen
        .iter()
        .position(|(path, _)| path.contains("/jobs"))
        .unwrap_or_else(|| {
            panic!("the Job was never created; requests: {seen:?}");
        });

    assert!(
        policy_index < job_index,
        "the NetworkPolicy must be created before the Job, but the order was {seen:?}"
    );

    // The policy is named after the Job, so `remove_container` can find it
    // again — and the Job it was created for is the one that was just made.
    let policy_body = &seen[policy_index].1;
    assert!(
        policy_body.contains(&job_name),
        "the NetworkPolicy must be named after the Job ({job_name}): {seen:?}"
    );
    // …and it must select the Pod by the task label, not by anything else.
    assert!(
        policy_body.contains("loom.task_id"),
        "the NetworkPolicy must select the Pod by label: {seen:?}"
    );
}

#[tokio::test]
async fn an_unrestricted_posture_creates_no_policy_at_all() {
    // The other half of the contract: the policy is not created when the
    // posture does not need one. Without this, the ordering assertion above
    // could be satisfied by a policy that is always created.
    let (server_url, requests) = start_fake_api_server().await;

    let config: kube::Config = kube::Config::new(server_url.parse().expect("uri"));
    let client = kube::Client::try_from(config).expect("kube client");
    let kubernetes = KubernetesClient::with_client(client, "default".to_string());

    let spec = spec(NetworkPolicy::Unrestricted);

    kubernetes.create_job(&spec).await.expect("create job");

    let seen = requests.lock().expect("requests lock").clone();
    assert!(
        !seen
            .iter()
            .any(|(path, _)| path.contains("networkpolicies")),
        "an unrestricted posture must not create a NetworkPolicy: {seen:?}"
    );
    assert!(
        seen.iter().any(|(path, _)| path.contains("/jobs")),
        "the Job must still be created: {seen:?}"
    );
}
