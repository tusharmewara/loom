//! loom-runtime — Pluggable runtime backends for task execution.
//!
//! Provides a `Runtime` trait with implementations for:
//! - **ProcessRuntime** — runs commands directly on the host (current behavior)
//! - **ContainerRuntime** — runs commands inside Docker containers
//!
//! The runtime is selected via `LaunchConfig.runtime` (default: "process").
//!
//! # Network posture
//!
//! Container isolation is **opt-in**: the default runtime stays `process`, so
//! nothing changes for runs that never ask for a container. Once a run *does*
//! select a container backend, the **default network posture is
//! [`NetworkPolicy::NoNetwork`]** — the container gets no network at all.
//! Unrestricted egress is an explicit opt-in, never the absence of a setting.
//!
//! | Posture | Engine argument | Meaning |
//! |---|---|---|
//! | [`NetworkPolicy::NoNetwork`] (default) | `--network none` | no egress |
//! | [`NetworkPolicy::Restricted`] non-empty | internal user-defined network + egress-proxy sidecar | egress **only** through the allowlisted proxy (Docker backend) |
//! | [`NetworkPolicy::Restricted`] empty | `--network none` | deny-all, diagnostic logged |
//! | [`NetworkPolicy::Unrestricted`] | *(no override)* | engine default network, full egress |
//!
//! The posture is selected per run from the tool's `[launch] network` value
//! (`none` | `restricted` | `unrestricted`); free-form network names are
//! rejected — a name is not a posture.
//!
//! `restricted` **non-empty** is enforced: the Docker backend launches a
//! `loom-egress-proxy` sidecar on a user-defined `--internal` network (no
//! NAT, no direct egress for the workload), injects
//! `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY=localhost,127.0.0.1` into the
//! workload, and attaches the proxy to the default bridge for its own
//! upstream connections. The proxy starts from `LOOM_EGRESS_PROXY_IMAGE`
//! (default `loom-egress-proxy:latest`) and a failing proxy fails the
//! dispatch — never a silent widening of egress. The workload's engine
//! network is the internal bridge itself: no default-route NAT, so direct
//! egress is impossible even without the proxy.
//!
//! Enforcement is per backend: the Docker backend composes the proxy itself
//! and is unconditionally fail-closed. The Podman and Kubernetes backends do
//! not compose the proxy yet (see docs/decisions/0002-egress-allowlist-proxy.md):
//! `restricted` non-empty stays deny-all there — fail-closed, but the
//! allowlist is not honored until the per-backend wiring lands. Kubernetes
//! additionally has no guaranteed isolation — its `NetworkPolicy` object is
//! enforced only when the cluster's CNI implements NetworkPolicy, and there
//! is no capability check.

use anyhow::{Context, Result};
use async_trait::async_trait;
use loom_core::types::Task;
use loom_core::HealthCheck;
use loom_telemetry::TelemetryCollector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

    /// Engine port the egress proxy listens on. The Docker wiring points
    /// `HTTP_PROXY` at it on the internal network; `loom-egress-proxy`'s
    /// `LOOM_EGRESS_PORT` defaults to the same value.
    pub const EGRESS_PROXY_PORT: u16 = 3128;

    /// Re-export LaunchConfig for consumers.
    pub use loom_core::types::LaunchConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub container_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub image: String,
    pub cmd: Vec<String>,
    pub env: HashMap<String, String>,
    pub working_dir: String,
    pub volumes: Vec<VolumeMount>,
    /// Network posture for this run. The engine argument is derived from it:
    /// Docker/Podman get `--network none` unless the posture is
    /// [`NetworkPolicy::Unrestricted`], Kubernetes gets a `NetworkPolicy`
    /// object.
    pub network_policy: NetworkPolicy,
    pub resource_limits: Option<ResourceLimits>,
    pub labels: HashMap<String, String>,
}

/// Per-run network posture for container execution.
///
/// This is a **policy**, not a network name: the old `Option<String>` field
/// conflated "which network" with "may this run reach the network at all",
/// which made the *absence* of a value mean "host networking" — the opposite
/// of isolation.
///
/// The default is [`NetworkPolicy::NoNetwork`]. Unrestricted egress is
/// reachable only by asking for it explicitly.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicy {
    /// No network at all — the container cannot reach anything.
    /// The default posture for untrusted runs.
    #[default]
    NoNetwork,
    /// Hosts the run is allowed to reach.
    ///
    /// Non-empty: enforced — a loom-egress-proxy sidecar carries egress over
    /// the user-defined internal network (Docker backend; other backends:
    /// see [`Runtime::run`] and docs/decisions/0002-egress-allowlist-proxy.md).
    /// Empty: the posture stays [`NetworkPolicy::NoNetwork`] — deny-all, with
    /// a diagnostic (see [`NetworkPolicy::diagnostic`]) because the operator
    /// named zero hosts.
    Restricted {
        allowed_hosts: Vec<String>,
    },
    /// Unrestricted egress on the engine's default network. Explicit opt-in.
    Unrestricted,
}

impl NetworkPolicy {
    /// Parse the policy from a tool's `[launch] network` value.
    ///
    /// Accepts `none` / `restricted` / `unrestricted` (case-insensitive), and
    /// `restricted:<host>[,<host>...]` to name the allowlist. Anything else is
    /// an error — free-form network names are not postures.
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        let (name, hosts) = match value.split_once(':') {
            Some((name, hosts)) => (name.trim(), hosts.trim()),
            None => (value, ""),
        };
        match name.to_ascii_lowercase().as_str() {
            "none" => Ok(NetworkPolicy::NoNetwork),
            "restricted" => Ok(NetworkPolicy::Restricted {
                allowed_hosts: parse_host_list(hosts),
            }),
            "unrestricted" => Ok(NetworkPolicy::Unrestricted),
            other => anyhow::bail!(
                "unknown network policy '{other}' (expected: none | restricted | unrestricted)"
            ),
        }
    }

    /// The container engine's network argument for this posture.
    ///
    /// `None` means "do not pass a network argument" — the engine's default
    /// network applies. `Some("none")` means "no network at all".
    pub fn engine_network_mode(&self) -> Option<&'static str> {
        match self {
            // The engine argument only. The egress *enforcement* (allowlist
            // proxy) is composed by the Docker backend when this posture is
            // non-empty; the engine argument stays a hard deny so direct
            // egress can never bypass the proxy.
            NetworkPolicy::NoNetwork | NetworkPolicy::Restricted { .. } => Some("none"),
            NetworkPolicy::Unrestricted => None,
        }
    }

    /// Container-engine CLI arguments for this posture.
    ///
    /// Empty for [`NetworkPolicy::Unrestricted`] — the engine's default
    /// network is left in place rather than overridden.
    pub fn cli_args(&self) -> Vec<String> {
        match self.engine_network_mode() {
            Some(mode) => vec!["--network".to_string(), mode.to_string()],
            None => Vec::new(),
        }
    }

    /// Short name of the posture (used in labels and log lines).
    pub fn name(&self) -> &'static str {
        match self {
            NetworkPolicy::NoNetwork => "none",
            NetworkPolicy::Restricted { .. } => "restricted",
            NetworkPolicy::Unrestricted => "unrestricted",
        }
    }

    /// Hosts the posture allows, if any.
    pub fn allowed_hosts(&self) -> &[String] {
        match self {
            NetworkPolicy::Restricted { allowed_hosts } => allowed_hosts,
            _ => &[],
        }
    }

    /// The diagnostic this posture needs, if any.
    ///
    /// `Restricted` with a **non-empty** allowlist is enforced as-is (the
    /// Docker backend runs the egress proxy sidecar), so there is nothing to
    /// warn about. Restricted with an **empty** allowlist still needs one:
    /// `restricted:` asks for "these hosts" and names none — the honest
    /// reading of the resulting posture is "deny all egress", and saying
    /// only "restricted" would leave the operator guessing whether the list
    /// was lost or empty.
    ///
    /// `None` otherwise.
    pub fn diagnostic(&self) -> Option<String> {
        match self {
            NetworkPolicy::Restricted { allowed_hosts } if allowed_hosts.is_empty() => {
                Some("restricted with an empty allowlist; resolved to deny-all (posture=none)".to_string())
            }
            _ => None,
        }
    }
}

fn parse_host_list(hosts: &str) -> Vec<String> {
    hosts
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .collect()
}

/// Resolve the effective network posture for a single run.
///
/// Precedence: an explicit per-run value (`LaunchConfig::network`, i.e. the
/// tool's `[launch] network`) wins over the runtime's default posture. An
/// absent value keeps the runtime default — [`NetworkPolicy::default()`], which
/// is [`NetworkPolicy::NoNetwork`].
///
/// An unrecognised value is an **error**: a free-form network name is not a
/// posture, and guessing would silently widen egress.
pub fn resolve_network_policy(
    runtime_default: &NetworkPolicy,
    requested: Option<&str>,
) -> Result<NetworkPolicy> {
    let policy = match requested {
        Some(value) => NetworkPolicy::parse(value)?,
        None => runtime_default.clone(),
    };
    // Postures that need a diagnostic still say so; `restricted` non-empty
    // is now enforced through the egress proxy, so it is down to the
    // empty-allowlist edge only.
    if let Some(diagnostic) = policy.diagnostic() {
        tracing::warn!(
            posture = policy.name(),
            requested = requested.unwrap_or("(runtime default)"),
            "container network posture diagnostic: {diagnostic}"
        );
    }
    Ok(policy)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_shares: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub cpu_quota: Option<i64>,
    pub cpu_period: Option<u64>,
}

/// Trait for executing commands in different runtime environments.
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Unique runtime identifier (e.g., "process", "docker").
    fn name(&self) -> &'static str;

    /// Whether this runtime is available on the current system.
    async fn is_available(&self) -> bool;

    /// Execute a command with the given launch configuration.
    ///
    /// The `launch_config` comes from the tool's TOML `[launch]` section or CLI args.
    /// The `task` provides context (task_id, worktree_path, etc.).
    async fn run(&self, config: &LaunchConfig, task: &Task) -> Result<RunResult>;

    /// Prepare the runtime (pull images, create networks, etc.).
    async fn prepare(&self, config: &LaunchConfig) -> Result<()> {
        let _ = config;
        Ok(())
    }

    /// Cleanup after execution (remove containers, networks, etc.).
    async fn cleanup(&self, _result: &RunResult) -> Result<()> {
        Ok(())
    }

    /// Get a unique identifier for this runtime instance.
    fn id(&self) -> String {
        format!("{}-{}", self.name(), &Uuid::new_v4().to_string()[..8])
    }
}

/// Runtime registry for managing multiple backends.
pub struct RuntimeRegistry {
    runtimes: HashMap<String, Box<dyn Runtime>>,
    default: String,
}

impl RuntimeRegistry {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            default: "process".to_string(),
        }
    }

    pub fn register(&mut self, runtime: Box<dyn Runtime>) -> Result<()> {
        let name = runtime.name().to_string();
        if self.runtimes.contains_key(&name) {
            anyhow::bail!("Runtime '{}' already registered", name);
        }
        self.runtimes.insert(name.clone(), runtime);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Runtime> {
        self.runtimes.get(name).map(|b| b.as_ref())
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut (dyn Runtime + '_)> {
        match self.runtimes.get_mut(name) {
            Some(boxed) => Some(boxed.as_mut()),
            None => None,
        }
    }

    pub fn default(&self) -> Option<&dyn Runtime> {
        self.get(&self.default)
    }

    pub fn set_default(&mut self, name: &str) -> Result<()> {
        if self.runtimes.contains_key(name) {
            self.default = name.to_string();
            Ok(())
        } else {
            anyhow::bail!("Runtime '{}' not registered", name)
        }
    }

    pub fn list(&self) -> Vec<String> {
        self.runtimes.keys().cloned().collect()
    }
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        let mut registry = Self::new();
        // Register built-in runtimes
        let _ = registry.register(Box::new(ProcessRuntime::new()));
        #[cfg(feature = "docker")]
        {
            let _ = registry.register(Box::new(ContainerRuntime::docker()));
        }
        #[cfg(feature = "podman")]
        {
            let _ = registry.register(Box::new(ContainerRuntime::podman()));
        }
        registry
    }
}

impl HealthCheck for RuntimeRegistry {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        let uptime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Ok(loom_core::HealthStatus {
            crate_name: "loom-runtime",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: uptime,
            last_error: None,
            performance_counters: {
                let mut h = std::collections::HashMap::new();
                h.insert("runtimes_registered".into(), self.runtimes.len() as u64);
                h.insert("default_runtime".into(), 0);
                h
            },
        })
    }
}

/// Process runtime — executes commands directly on the host (current behavior).
pub struct ProcessRuntime {
    work_dir: Option<PathBuf>,
}

impl ProcessRuntime {
    pub fn new() -> Self {
        Self { work_dir: None }
    }

    pub fn with_work_dir(mut self, dir: PathBuf) -> Self {
        self.work_dir = Some(dir);
        self
    }
}

impl Default for ProcessRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Runtime for ProcessRuntime {
    fn name(&self) -> &'static str {
        "process"
    }

    async fn is_available(&self) -> bool {
        true // Process runtime is always available
    }

    async fn run(&self, config: &LaunchConfig, task: &Task) -> Result<RunResult> {
        use std::process::Command;

        let cwd = self
            .work_dir
            .clone()
            .or_else(|| config.working_dir.clone().map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let mut cmd = Command::new(&config.command);
        cmd.current_dir(&cwd);
        cmd.args(&config.args);
        cmd.envs(&config.env);

        // Set worktree path if available
        if let Some(wt) = &task.worktree_path {
            cmd.env("LOOM_WORKTREE", wt);
        }
        cmd.env("LOOM_TASK_ID", &task.task_id);
        cmd.env("LOOM_TASK_TITLE", &task.title);

        let output = cmd
            .output()
            .with_context(|| format!("Failed to execute command: {}", config.command))?;

        Ok(RunResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code(),
            container_id: None,
        })
    }
}

/// Container runtime — executes commands inside Docker containers.
pub struct ContainerRuntime {
    client: Option<Box<dyn ContainerClient>>,
    default_image: String,
    network_policy: NetworkPolicy,
    auto_remove: bool,
}

/// Trait for container client abstraction (bollard for Docker).
#[async_trait]
trait ContainerClient: Send + Sync {
    async fn create_container(&self, spec: &ContainerSpec) -> Result<String>;
    async fn start_container(&self, id: &str) -> Result<()>;
    async fn wait_container(&self, id: &str) -> Result<RunResult>;
    async fn remove_container(&self, id: &str, force: bool) -> Result<()>;
    async fn pull_image(&self, image: &str) -> Result<()>;
}

/// Kubernetes client implementation using the kube-rs API client.
///
/// Tasks run as batch/v1 Jobs in a single Pod. The Job is created via the
/// Kubernetes API (no kubectl required), completion is awaited by polling
/// the Job status, and output is captured from the Pod logs.
#[cfg(feature = "kubernetes")]
pub mod kubernetes_client {
    use super::*;
    use std::time::Duration;
    use uuid::Uuid;

    use k8s_openapi::api::batch::v1::{Job, JobSpec};
    use k8s_openapi::api::core::v1::{
        Container, EmptyDirVolumeSource, EnvVar, Pod, PodSpec, PodTemplateSpec, Volume, VolumeMount,
    };
    use k8s_openapi::api::networking::v1::{NetworkPolicyEgressRule, NetworkPolicySpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
    use kube::api::{Api, DeleteParams, ListParams, LogParams, PostParams};

    use k8s_openapi::api::networking::v1::NetworkPolicy as K8sNetworkPolicy;

    /// Default namespace for task Jobs.
    pub const DEFAULT_NAMESPACE: &str = "default";

    /// Upper bound for waiting on a Job to complete.
    const WAIT_TIMEOUT: Duration = Duration::from_secs(300);

    /// Poll interval when waiting on Job status.
    const WAIT_POLL_INTERVAL: Duration = Duration::from_secs(2);

    pub struct KubernetesClient {
        client: kube::Client,
        namespace: String,
    }

    impl KubernetesClient {
        /// Create a client from the ambient kubeconfig
        /// (`KUBECONFIG` / `~/.kube/config` / in-cluster if running in a Pod).
        pub async fn new() -> Result<Self> {
            let client = kube::Client::try_default().await.context(
                "failed to load Kubernetes client (no reachable cluster or kubeconfig?)",
            )?;
            Ok(Self {
                client,
                namespace: DEFAULT_NAMESPACE.to_string(),
            })
        }

        /// Override the default namespace (e.g. from LaunchConfig).
        pub fn with_namespace(mut self, namespace: String) -> Self {
            self.namespace = namespace;
            self
        }

        /// Build a client against an explicit API server.
        ///
        /// Used by the ordering test, which points the client at a local fake
        /// API server to observe the order of the calls the runtime makes.
        pub fn with_client(client: kube::Client, namespace: String) -> Self {
            Self { client, namespace }
        }

        fn jobs(&self) -> Api<Job> {
            Api::namespaced(self.client.clone(), &self.namespace)
        }

        fn pods(&self) -> Api<Pod> {
            Api::namespaced(self.client.clone(), &self.namespace)
        }

        fn network_policies(&self) -> Api<K8sNetworkPolicy> {
            Api::namespaced(self.client.clone(), &self.namespace)
        }

        async fn find_task_pod(&self, job_name: &str) -> Result<Pod> {
            // The Job controller labels Pods with job-name=<job>.
            let by_label = self
                .pods()
                .list(&ListParams::default().labels(&format!("job-name={}", job_name)))
                .await?;
            if !by_label.items.is_empty() {
                return Ok(by_label.items[0].clone());
            }
            // Fallback: Job Pods are named <job>-<hash>.
            let all = self.pods().list(&ListParams::default()).await?;
            all.items
                .into_iter()
                .find(|p| {
                    p.metadata
                        .name
                        .as_deref()
                        .map(|n| n.starts_with(&format!("{}-", job_name)))
                        .unwrap_or(false)
                })
                .ok_or_else(|| anyhow::anyhow!("no Pod found for Job {}", job_name))
        }

        /// Apply the run's network posture as a Kubernetes `NetworkPolicy`.
        ///
        /// The policy is named after the Job, so `remove_container` can delete
        /// it again. Returns the created policy's name, or `None` when the
        /// posture is [`NetworkPolicy::Unrestricted`] (no policy object is
        /// needed — the Pod keeps the cluster's default egress).
        ///
        /// **This is only as enforced as the cluster's CNI.** A Kubernetes
        /// `NetworkPolicy` is a declarative object; a Pod is only actually
        /// denied egress when the cluster's network plugin implements
        /// NetworkPolicy enforcement. On a cluster whose CNI ignores
        /// `NetworkPolicy` objects, this policy is silently a no-op and the
        /// Pod keeps the cluster's default egress. There is no check for
        /// that here, so the Kubernetes backend is **not** unconditionally
        /// fail-closed the way the Docker and Podman backends are: those two
        /// enforce isolation in the engine itself (`--network none`), while
        /// Kubernetes delegates enforcement to the cluster.
        async fn apply_network_policy(
            &self,
            spec: &ContainerSpec,
            job_name: &str,
            pod_labels: &std::collections::BTreeMap<String, String>,
        ) -> Result<Option<String>> {
            let policy = &spec.network_policy;
            if matches!(policy, NetworkPolicy::Unrestricted) {
                return Ok(None);
            }

            let name = format!("loom-net-{job_name}");

            // Fail-closed, exactly like the Docker/Podman backends: an empty
            // egress list denies all egress. A `Restricted` allowlist is not
            // wired to the egress proxy on this backend yet (follow-up,
            // docs/decisions/0002-egress-allowlist-proxy.md), so it must not
            // widen to full egress — and a hostname is not a CIDR, so it
            // cannot be expressed as an `IPBlock` either. Deferred wiring
            // never widens: until it lands, `restricted` here = deny-all.
            let egress: Vec<NetworkPolicyEgressRule> = Vec::new();
            let k8s_policy = K8sNetworkPolicy {
                metadata: ObjectMeta {
                    name: Some(name.clone()),
                    namespace: Some(self.namespace.clone()),
                    ..Default::default()
                },
                spec: Some(NetworkPolicySpec {
                    pod_selector: LabelSelector {
                        match_labels: Some(pod_labels.clone()),
                        match_expressions: None,
                    },
                    policy_types: Some(vec!["Egress".to_string()]),
                    egress: Some(egress),
                    ingress: None,
                }),
            };

            self.network_policies()
                .create(&PostParams::default(), &k8s_policy)
                .await
                .with_context(|| {
                    format!(
                        "failed to create NetworkPolicy {name} in namespace {}",
                        self.namespace
                    )
                })?;

            Ok(Some(name))
        }
    }

    impl KubernetesClient {
        /// Create the Job for this run, with the network posture applied
        /// before it.
        ///
        /// The order matters: the NetworkPolicy is created first, so the
        /// Job's Pod is already covered by the time the Job controller starts
        /// it.
        pub async fn create_job(&self, spec: &ContainerSpec) -> Result<String> {
            let job_name = format!("loom-task-{}", Uuid::new_v4().simple());

            let env: Vec<EnvVar> = spec
                .env
                .iter()
                .map(|(k, v)| EnvVar {
                    name: k.clone(),
                    value: Some(v.clone()),
                    ..Default::default()
                })
                .collect();

            let volume_mounts: Vec<VolumeMount> = spec
                .volumes
                .iter()
                .map(|v| VolumeMount {
                    name: v.source.clone(),
                    mount_path: v.target.clone(),
                    read_only: Some(v.read_only),
                    ..Default::default()
                })
                .collect();

            let volumes: Vec<Volume> = spec
                .volumes
                .iter()
                .map(|v| Volume {
                    name: v.source.clone(),
                    empty_dir: Some(EmptyDirVolumeSource::default()),
                    ..Default::default()
                })
                .collect();

            let pod_spec = PodSpec {
                containers: vec![Container {
                    name: "task".to_string(),
                    image: Some(spec.image.clone()),
                    command: Some(spec.cmd.clone()),
                    env: if env.is_empty() { None } else { Some(env) },
                    volume_mounts: if volume_mounts.is_empty() {
                        None
                    } else {
                        Some(volume_mounts)
                    },
                    ..Default::default()
                }],
                restart_policy: Some("Never".to_string()),
                volumes: if volumes.is_empty() {
                    None
                } else {
                    Some(volumes)
                },
                ..Default::default()
            };

            // The Pod carries the task label the NetworkPolicy selects on.
            let pod_labels = {
                let mut labels = std::collections::BTreeMap::new();
                labels.insert(
                    "loom.task_id".to_string(),
                    spec.labels
                        .get("loom.task_id")
                        .cloned()
                        .unwrap_or_else(|| job_name.clone()),
                );
                labels
            };

            // Network posture — applied BEFORE the Job. A NetworkPolicy
            // selects Pods by label, not by the Pod existing, so creating the
            // policy first is what makes the posture effective: the Job's Pod
            // is already covered by the time the Job controller starts it.
            // Creating it after the Job leaves a window where the Pod runs
            // with the cluster's default egress.
            if let Some(policy_name) = self
                .apply_network_policy(spec, &job_name, &pod_labels)
                .await?
            {
                tracing::info!(policy = %policy_name, "created Kubernetes NetworkPolicy");
            }

            let job = Job {
                metadata: ObjectMeta {
                    name: Some(job_name.clone()),
                    namespace: Some(self.namespace.clone()),
                    ..Default::default()
                },
                spec: Some(JobSpec {
                    template: PodTemplateSpec {
                        metadata: Some(ObjectMeta {
                            labels: Some(pod_labels.clone()),
                            ..Default::default()
                        }),
                        spec: Some(pod_spec),
                    },
                    backoff_limit: Some(3),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let created = self
                .jobs()
                .create(&PostParams::default(), &job)
                .await
                .with_context(|| {
                    format!(
                        "failed to create Job {} in namespace {}",
                        job_name, self.namespace
                    )
                })?;
            let name = created.metadata.name.unwrap_or(job_name.clone());
            tracing::info!(job = %name, "created Kubernetes Job");

            Ok(name)
        }
    }

    #[async_trait]
    impl ContainerClient for KubernetesClient {
        async fn create_container(&self, spec: &ContainerSpec) -> Result<String> {
            self.create_job(spec).await
        }

        async fn start_container(&self, _id: &str) -> Result<()> {
            // Jobs start their Pod automatically.
            Ok(())
        }

        async fn wait_container(&self, id: &str) -> Result<RunResult> {
            let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
            let mut job = None;
            while tokio::time::Instant::now() < deadline {
                if let Ok(j) = self.jobs().get(id).await {
                    if j.status
                        .as_ref()
                        .map(|s| s.succeeded.is_some() || s.failed.is_some())
                        .unwrap_or(false)
                    {
                        job = Some(j);
                        break;
                    }
                }
                tokio::time::sleep(WAIT_POLL_INTERVAL).await;
            }
            let job = job.ok_or_else(|| {
                anyhow::anyhow!(
                    "Job {id} did not complete within {}s",
                    WAIT_TIMEOUT.as_secs()
                )
            })?;

            let succeeded = job.status.as_ref().and_then(|s| s.succeeded).unwrap_or(0) > 0;

            let pod = self.find_task_pod(id).await?;
            let pod_name = pod.metadata.name.clone().unwrap_or_else(|| id.to_string());
            let stdout = self
                .pods()
                .logs(&pod_name, &LogParams::default())
                .await
                .unwrap_or_default();

            // Best-effort exit code from the Pod's container status.
            let exit_code = pod
                .status
                .as_ref()
                .and_then(|s| s.container_statuses.as_ref())
                .and_then(|cs| cs.first())
                .and_then(|c| c.state.as_ref())
                .and_then(|st| st.terminated.as_ref())
                .map(|t| t.exit_code);

            let stderr = if succeeded {
                String::new()
            } else {
                let mut buf = String::new();
                for line in stdout.lines() {
                    buf.push_str(line);
                    buf.push('\n');
                }
                buf
            };

            Ok(RunResult {
                success: succeeded,
                stdout,
                stderr,
                exit_code,
                container_id: Some(id.to_string()),
            })
        }

        async fn remove_container(&self, id: &str, _force: bool) -> Result<()> {
            // The NetworkPolicy is named after the Job — remove it too, so a
            // finished run leaves no policy object behind.
            let policy_name = format!("loom-net-{id}");
            match self
                .network_policies()
                .delete(&policy_name, &DeleteParams::default())
                .await
            {
                Ok(_) => {}
                Err(kube::Error::Api(err)) if err.code == 404 => {}
                Err(e) => {
                    tracing::warn!(
                        policy = %policy_name,
                        "failed to delete NetworkPolicy: {e}"
                    );
                }
            }

            match self.jobs().delete(id, &DeleteParams::default()).await {
                Ok(_) => Ok(()),
                Err(kube::Error::Api(err)) if err.code == 404 => Ok(()),
                Err(e) => Err(anyhow::anyhow!("failed to delete Job {id}: {e}")),
            }
        }

        async fn pull_image(&self, _image: &str) -> Result<()> {
            // Nodes pull images when the Pod starts; nothing to do client-side.
            Ok(())
        }
    }
}

impl ContainerRuntime {
    pub fn docker() -> Self {
        Self {
            client: None,
            default_image: "loom/agent:latest".to_string(),
            network_policy: NetworkPolicy::default(),
            auto_remove: true,
        }
    }

    pub fn podman() -> Self {
        Self {
            client: None,
            default_image: "loom/agent:latest".to_string(),
            network_policy: NetworkPolicy::default(),
            auto_remove: true,
        }
    }

    #[cfg(feature = "kubernetes")]
    pub fn kubernetes() -> Self {
        Self {
            client: None,
            default_image: "loom/agent:latest".to_string(),
            network_policy: NetworkPolicy::default(),
            auto_remove: true,
        }
    }

    pub fn with_image(mut self, image: String) -> Self {
        self.default_image = image;
        self
    }

    /// Set the network posture for this runtime.
    pub fn with_network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = policy;
        self
    }

    /// The network posture this runtime applies.
    pub fn network_policy(&self) -> &NetworkPolicy {
        &self.network_policy
    }

    pub fn with_auto_remove(mut self, auto_remove: bool) -> Self {
        self.auto_remove = auto_remove;
        self
    }

    async fn init_client(&mut self) -> Result<()> {
        if self.client.is_some() {
            return Ok(());
        }

        #[cfg(feature = "docker")]
        {
            use crate::docker_client::DockerClient;
            let client = DockerClient::new().await?;
            self.client = Some(Box::new(client));
            Ok(())
        }

        #[cfg(all(feature = "podman", not(feature = "docker")))]
        {
            use crate::podman_client::PodmanClient;
            let client = PodmanClient::new().await?;
            self.client = Some(Box::new(client));
            return Ok(());
        }

        #[cfg(all(
            feature = "kubernetes",
            not(feature = "docker"),
            not(feature = "podman")
        ))]
        {
            use crate::kubernetes_client::KubernetesClient;
            let client = KubernetesClient::new().await?;
            self.client = Some(Box::new(client));
            return Ok(());
        }

        #[cfg(not(any(feature = "docker", feature = "podman", feature = "kubernetes")))]
        {
            anyhow::bail!("No container engine feature enabled");
        }
    }
}

#[async_trait]
impl Runtime for ContainerRuntime {
    fn name(&self) -> &'static str {
        #[cfg(feature = "docker")]
        return "docker";
        #[cfg(all(feature = "podman", not(feature = "docker")))]
        return "podman";
        #[cfg(all(
            feature = "kubernetes",
            not(feature = "docker"),
            not(feature = "podman")
        ))]
        return "kubernetes";
        #[cfg(not(any(feature = "docker", feature = "podman", feature = "kubernetes")))]
        return "container";
    }

    async fn is_available(&self) -> bool {
        // Try to initialize client
        let mut runtime = self.clone();
        runtime.init_client().await.is_ok()
    }

    async fn run(&self, config: &LaunchConfig, task: &Task) -> Result<RunResult> {
        let start = std::time::Instant::now();
        let telemetry = TelemetryCollector::new("");

        let mut runtime = self.clone();
        runtime.init_client().await?;

        let client = runtime
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Container client not initialized"))?;

        // Build container spec from launch config
        // (`resolve_network_policy` logs the posture-downgrade diagnostic.)
        let policy = resolve_network_policy(&runtime.network_policy, config.network.as_deref())?;
        let spec = build_container_spec(config, task, &runtime.default_image, policy)?;

        // Pull image if needed
        client.pull_image(&spec.image).await?;
        telemetry.record_container_pull(true);

        // Create and start container
        let container_id = client.create_container(&spec).await?;
        client.start_container(&container_id).await?;

        // Wait for completion
        let mut result = client.wait_container(&container_id).await?;
        result.container_id = Some(container_id.clone());

        // Cleanup if auto_remove
        if self.auto_remove {
            client.remove_container(&container_id, true).await?;
        }

        let duration_us = start.elapsed().as_micros() as f64;
        let runtime_name = match config.runtime.as_str() {
            "docker" => "docker",
            "podman" => "podman",
            "kubernetes" => "kubernetes",
            _ => "container",
        };
        telemetry.record_container_run(runtime_name, result.success, duration_us);

        Ok(result)
    }

    async fn prepare(&self, _config: &LaunchConfig) -> Result<()> {
        let mut runtime = self.clone();
        runtime.init_client().await?;

        let client = runtime
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Container client not initialized"))?;

        // Pull default image
        client.pull_image(&runtime.default_image).await
    }

    async fn cleanup(&self, result: &RunResult) -> Result<()> {
        if let Some(container_id) = &result.container_id {
            let mut runtime = self.clone();
            runtime.init_client().await?;
            if let Some(client) = runtime.client.as_ref() {
                client.remove_container(container_id, true).await?;
            }
        }
        Ok(())
    }
}

impl Clone for ContainerRuntime {
    fn clone(&self) -> Self {
        Self {
            client: None, // Client will be re-initialized
            default_image: self.default_image.clone(),
            network_policy: self.network_policy.clone(),
            auto_remove: self.auto_remove,
        }
    }
}

/// Build container spec from launch config and task.
fn build_container_spec(
    config: &LaunchConfig,
    task: &Task,
    default_image: &str,
    network_policy: NetworkPolicy,
) -> Result<ContainerSpec> {
    let image = config
        .image
        .clone()
        .unwrap_or_else(|| default_image.to_string());

    // Determine working directory
    let working_dir = config
        .working_dir
        .clone()
        .or_else(|| task.worktree_path.clone())
        .unwrap_or_else(|| "/workspace".to_string());

    // Build volume mounts
    let mut volumes = Vec::new();

    // Mount worktree if available
    if let Some(wt) = &task.worktree_path {
        volumes.push(VolumeMount {
            source: wt.clone(),
            target: working_dir.clone(),
            read_only: false,
        });
    }

    // Mount Loom config directory
    if let Ok(loom_dir) = std::env::var("LOOM_DIR") {
        volumes.push(VolumeMount {
            source: loom_dir,
            target: "/.loom".to_string(),
            read_only: true,
        });
    }

    // Build environment
    let mut env = config.env.clone();
    env.insert("LOOM_TASK_ID".to_string(), task.task_id.clone());
    env.insert("LOOM_TASK_TITLE".to_string(), task.title.clone());
    env.insert(
        "LOOM_WORKTREE".to_string(),
        task.worktree_path.clone().unwrap_or_default(),
    );
    env.insert("LOOM_RUNTIME".to_string(), "container".to_string());

    let mut labels = HashMap::new();
    labels.insert("loom.task_id".to_string(), task.task_id.clone());
    labels.insert("loom.task_title".to_string(), task.title.clone());
    labels.insert(
        "loom.network_policy".to_string(),
        network_policy.name().to_string(),
    );
    if !network_policy.allowed_hosts().is_empty() {
        labels.insert(
            "loom.network_allowed_hosts".to_string(),
            network_policy.allowed_hosts().join(","),
        );
    }

    Ok(ContainerSpec {
        image,
        cmd: vec![config.command.clone()]
            .into_iter()
            .chain(config.args.clone())
            .collect(),
        env,
        working_dir,
        volumes,
        network_policy,
        resource_limits: None,
        labels,
    })
}

// Docker client implementation using bollard
#[cfg(feature = "docker")]
mod docker_client {
    use super::*;
    use bollard::container::{
        Config, CreateContainerOptions, LogOutput, RemoveContainerOptions, StartContainerOptions,
        WaitContainerOptions,
    };
    use bollard::image::CreateImageOptions;
    use bollard::models::{HostConfig, Mount, MountTypeEnum};
    use bollard::Docker;
    use futures_util::stream::TryStreamExt;

    pub struct DockerClient {
        client: Docker,
    }

    impl DockerClient {
        pub async fn new() -> Result<Self> {
            // DOCKER_HOST wins, exactly like the docker CLI: `unix://…`
            // selects a different socket (colima, rootless Podman), any
            // `tcp://`/`http://…` an HTTP endpoint. Without it, the built-in
            // socket default applies so CI (and a vanilla Docker Desktop)
            // works unchanged.
            let client = match std::env::var("DOCKER_HOST") {
                Ok(host) if host.starts_with("unix://") => {
                    let path = host
                        .strip_prefix("unix://")
                        .ok_or_else(|| anyhow::anyhow!("invalid DOCKER_HOST unix path: {host}"))?;
                    Docker::connect_with_socket(path, 120, bollard::API_DEFAULT_VERSION)
                }
                Ok(host) => Docker::connect_with_http(&host, 120, bollard::API_DEFAULT_VERSION),
                Err(_) => Docker::connect_with_socket_defaults(),
            }
            .context("Failed to connect to Docker daemon")?;
            // Test connection
            client.ping().await.context("Docker daemon not reachable")?;
            Ok(Self { client })
        }

        /// Compose the egress half of a `restricted` (non-empty) run:
        ///
        /// 1. a **user-defined `--internal` network** — Docker installs no
        ///    NAT/masquerade for it, so the workload cannot reach the
        ///    internet even though it has an interface;
        /// 2. the **egress proxy sidecar**, attached to that network *and*
        ///    to the default bridge (its only route upstream), started from
        ///    `LOOM_EGRESS_PROXY_IMAGE` with the run's allowlist;
        ///
        /// Ready is proven, not assumed: the proxy prints `EGRESS listening`
        /// when it is serving, and this polls the container logs for that
        /// line. Every failure fails the dispatch with a clear error —
        /// never a fallback to unisolated egress.
        async fn start_egress(
            &self,
            short: &str,
            allowed_hosts: &[String],
        ) -> Result<EgressEcosystem> {
            let (network, proxy_name) = EgressEcosystem::names(short);

            self.client
                .create_network(bollard::network::CreateNetworkOptions {
                    name: network.clone(),
                    driver: "bridge".to_string(),
                    // The whole point: no NAT, no default route to anywhere.
                    internal: true,
                    attachable: true,
                    ..Default::default()
                })
                .await
                .with_context(|| format!("failed to create internal egress network {network}"))?;

            // Everything from here on must unwind cleanly: the proxy steps
            // clean up the sidecar container on failure, this call cleans up
            // the network — a failed dispatch never leaks egress state.
            match self
                .start_proxy(&network, &proxy_name, allowed_hosts)
                .await
            {
                Ok(proxy_id) => {
                    tracing::info!(
                        network = %network,
                        proxy = %proxy_name,
                        allowed_hosts = allowed_hosts.join(","),
                        "restricted egress enforced via proxy"
                    );
                    Ok(EgressEcosystem {
                        network,
                        proxy_name,
                        proxy_id,
                    })
                }
                Err(e) => {
                    self.client.remove_network(&network).await.ok();
                    Err(e)
                }
            }
        }

        /// Pull (or find locally), create, bridge-attach, start, and
        /// wait-for-readiness the proxy sidecar. On failure, removes the
        /// sidecar; the network is removed by the caller.
        async fn start_proxy(
            &self,
            network: &str,
            proxy_name: &str,
            allowed_hosts: &[String],
        ) -> Result<String> {
            let image = std::env::var("LOOM_EGRESS_PROXY_IMAGE")
                .unwrap_or_else(|_| "loom-egress-proxy:latest".to_string());
            // A locally built/tagged image is enough; only pull from a
            // registry when it does not exist locally.
            if self.client.inspect_image(&image).await.is_err() {
                self.pull_image(&image)
                    .await
                    .with_context(|| format!("egress proxy image '{image}' unavailable"))?;
            }

            let mut labels = HashMap::new();
            labels.insert("loom.egress_role".to_string(), "proxy".to_string());
            labels.insert("loom.network_policy".to_string(), "restricted".to_string());
            labels.insert(
                "loom.network_allowed_hosts".to_string(),
                allowed_hosts.join(","),
            );

            let proxy = Config {
                image: Some(image.clone()),
                env: Some(vec![format!(
                    "LOOM_ALLOWED_HOSTS={}",
                    allowed_hosts.join(",")
                )]),
                labels: Some(labels),
                host_config: Some(HostConfig {
                    network_mode: Some(network.to_string()),
                    auto_remove: Some(false),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let created = self
                .client
                .create_container(
                    Some(CreateContainerOptions {
                        name: proxy_name.clone(),
                        platform: None,
                    }),
                    proxy,
                )
                .await
                .with_context(|| {
                    format!("failed to create egress proxy container {proxy_name} (image '{image}')")
                })?;
            let proxy_id = created.id;

            let outcome = async {
                // The internal network is NAT-free, so the proxy itself
                // would have no egress either. The default bridge is its
                // upstream.
                self.client
                    .connect_network(
                        "bridge",
                        bollard::network::ConnectNetworkOptions {
                            container: proxy_name.clone(),
                            endpoint_config: bollard::models::EndpointSettings::default(),
                        },
                    )
                    .await
                    .with_context(|| {
                        format!("failed to attach egress proxy {proxy_name} to the bridge network")
                    })?;

                self.client
                    .start_container(&proxy_id, None::<StartContainerOptions<String>>)
                    .await
                    .with_context(|| {
                        format!("failed to start egress proxy container {proxy_name}")
                    })?;

                self.wait_for_listening(&proxy_id)
                    .await
                    .with_context(|| format!("egress proxy {proxy_name} did not become ready"))?;

                Ok::<String, anyhow::Error>(proxy_id.clone())
            }
            .await;

            if outcome.is_err() {
                // Best-effort undo of the sidecar before bailing.
                self.client
                    .remove_container(
                        &proxy_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await
                    .ok();
            }
            outcome
        }

        /// Poll the proxy's own log output for its ready line.
        async fn wait_for_listening(&self, proxy_id: &str) -> Result<()> {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
            loop {
                let mut stream = self.client.logs(
                    proxy_id,
                    Some(bollard::container::LogsOptions::<String> {
                        stdout: true,
                        stderr: true,
                        ..Default::default()
                    }),
                );
                while let Some(log) = stream.try_next().await? {
                    let line = match log {
                        LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                            String::from_utf8_lossy(&message).into_owned()
                        }
                        _ => continue,
                    };
                    if line.contains("EGRESS listening") {
                        return Ok(());
                    }
                }
                if tokio::time::Instant::now() > deadline {
                    anyhow::bail!(
                        "the egress proxy never printed its ready line within 10s (image broken or crashed?)"
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }

    /// The egress composition for a `restricted` (non-empty) run: the
    /// user-defined internal network both containers share, and the proxy
    /// sidecar that is the only route off it.
    #[derive(Debug)]
    struct EgressEcosystem {
        network: String,
        proxy_name: String,
        proxy_id: String,
    }

    impl EgressEcosystem {
        fn names(short: &str) -> (String, String) {
            (format!("loom-net-{short}"), format!("loom-{short}-proxy"))
        }

        /// Best-effort removal of the sidecar and the network. Used after a
        /// successful run (via `remove_container`, which derives the same
        /// names from the workload's) and on mid-setup failures.
        async fn teardown(&self, client: &bollard::Docker) {
            if let Err(e) = client
                .remove_container(
                    &self.proxy_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
            {
                tracing::warn!(proxy = %self.proxy_name, "failed to remove egress proxy container: {e}");
            }
            if let Err(e) = client.remove_network(&self.network).await {
                tracing::warn!(network = %self.network, "failed to remove egress network: {e}");
            }
        }
    }

    #[async_trait]
    impl ContainerClient for DockerClient {
        async fn create_container(&self, spec: &ContainerSpec) -> Result<String> {
            let short = &Uuid::new_v4().to_string()[..8];
            // Restricted non-empty ⇒ the workload talks only to the egress
            // proxy, over an internal user-defined network. Any failure here
            // fails the dispatch loudly — it must never degrade into *more*
            // egress than the posture allows.
            let ecosystem = match &spec.network_policy {
                NetworkPolicy::Restricted { allowed_hosts }
                    if !allowed_hosts.is_empty() =>
                {
                    Some(
                        self.start_egress(short, allowed_hosts)
                            .await
                            .context("restricted egress setup (internal network + allowlist proxy)")?,
                    )
                }
                _ => None,
            };

            let mut spec = spec.clone();
            let network_mode = match &ecosystem {
                Some(egress) => {
                    spec.env.insert(
                        "HTTP_PROXY".to_string(),
                        format!("http://{}:{}", egress.proxy_name, crate::EGRESS_PROXY_PORT),
                    );
                    spec.env.insert(
                        "HTTPS_PROXY".to_string(),
                        format!("http://{}:{}", egress.proxy_name, crate::EGRESS_PROXY_PORT),
                    );
                    // Loopback traffic is the workload's own; honored by
                    // clients, never routed to the proxy.
                    spec.env
                        .insert("NO_PROXY".to_string(), "localhost,127.0.0.1".to_string());
                    Some(egress.network.clone())
                }
                None => spec.network_policy.engine_network_mode().map(String::from),
            };

            let config = Config {
                image: Some(spec.image.clone()),
                cmd: Some(spec.cmd.clone()),
                env: Some(
                    spec.env
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect(),
                ),
                working_dir: Some(spec.working_dir.clone()),
                labels: Some(spec.labels.clone()),
                host_config: Some(HostConfig {
                    mounts: Some(
                        spec.volumes
                            .iter()
                            .map(|v| Mount {
                                target: Some(v.target.clone()),
                                source: Some(v.source.clone()),
                                typ: Some(MountTypeEnum::BIND),
                                read_only: Some(v.read_only),
                                ..Default::default()
                            })
                            .collect(),
                    ),
                    network_mode,
                    // The runtime owns the container lifecycle (`auto_remove`
                    // on `ContainerRuntime`): the daemon deleting the
                    // container on exit would take the logs with it and make
                    // the runtime's own cleanup 404.
                    auto_remove: Some(false),
                    cpu_shares: spec
                        .resource_limits
                        .as_ref()
                        .and_then(|r| r.cpu_shares.map(|v| v as i64)),
                    memory: spec
                        .resource_limits
                        .as_ref()
                        .and_then(|r| r.memory_bytes.map(|v| v as i64)),
                    cpu_quota: spec.resource_limits.as_ref().and_then(|r| r.cpu_quota),
                    cpu_period: spec
                        .resource_limits
                        .as_ref()
                        .and_then(|r| r.cpu_period.map(|v| v as i64)),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let options = CreateContainerOptions {
                name: format!("loom-{short}"),
                platform: None,
            };

            match self.client.create_container(Some(options), config).await {
                Ok(response) => Ok(response.id),
                Err(e) => {
                    // Undo whatever partial egress state exists: a failed
                    // dispatch must not leak the sidecar or the network.
                    if let Some(egress) = &ecosystem {
                        egress.teardown(&self.client).await;
                    }
                    Err(anyhow::anyhow!("Failed to create container: {e}"))
                }
            }
        }

        async fn start_container(&self, id: &str) -> Result<()> {
            self.client
                .start_container(id, None::<StartContainerOptions<String>>)
                .await
                .context("Failed to start container")?;
            Ok(())
        }

        async fn wait_container(&self, id: &str) -> Result<RunResult> {
            let mut stream = self
                .client
                .wait_container(id, None::<WaitContainerOptions<String>>);
            let mut exit_code: Option<i32> = None;
            while let Some(item) = stream.try_next().await? {
                exit_code = Some(item.status_code as i32);
            }

            // Get logs
            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut log_stream = self.client.logs(
                id,
                Some(bollard::container::LogsOptions::<String> {
                    stdout: true,
                    stderr: true,
                    follow: true,
                    ..Default::default()
                }),
            );
            while let Some(log) = log_stream.try_next().await? {
                match log {
                    LogOutput::StdOut { message } => {
                        stdout.push_str(&String::from_utf8_lossy(&message))
                    }
                    LogOutput::StdErr { message } => {
                        stderr.push_str(&String::from_utf8_lossy(&message))
                    }
                    _ => {}
                }
            }

            Ok(RunResult {
                success: exit_code == Some(0),
                stdout,
                stderr,
                exit_code,
                container_id: None,
            })
        }

        async fn remove_container(&self, id: &str, force: bool) -> Result<()> {
            // Egress teardown first, derived from the workload's name: the
            // sidecar container and the internal network are named
            // deterministically from it. Best-effort — a 404 anywhere means
            // the piece is already gone.
            if let Ok(inspected) = self
                .client
                .inspect_container(id, None::<bollard::container::InspectContainerOptions>)
                .await
            {
                if let Some(name) = inspected.name.as_deref().map(|n| n.trim_start_matches('/')) {
                    if let Some(short) = name.strip_prefix("loom-") {
                        let (network, proxy_name) = EgressEcosystem::names(short);
                        match self
                            .client
                            .remove_container(
                                &proxy_name,
                                Some(RemoveContainerOptions { force, ..Default::default() }),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(
                                bollard::errors::Error::DockerResponseServerError {
                                    status_code: 404,
                                    ..
                                },
                            ) => {}
                            Err(e) => {
                                tracing::warn!(proxy = %proxy_name, "failed to remove egress proxy container: {e}")
                            }
                        }
                        match self.client.remove_network(&network).await {
                            Ok(_) => {}
                            Err(e) if e.to_string().contains("not found") => {}
                            Err(e) => {
                                tracing::warn!(network = %network, "failed to remove egress network: {e}")
                            }
                        }
                    }
                }
            }

            let options = RemoveContainerOptions {
                force,
                ..Default::default()
            };
            match self.client.remove_container(id, Some(options)).await {
                Ok(_) => Ok(()),
                // Already gone — the runtime asked for removal, so this is
                // success, not a failure.
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => Ok(()),
                Err(e) => Err(anyhow::anyhow!("Failed to remove container: {e}")),
            }
        }

        async fn pull_image(&self, image: &str) -> Result<()> {
            let options = CreateImageOptions {
                from_image: image,
                ..Default::default()
            };
            let mut stream = self.client.create_image(Some(options), None, None);
            while let Some(_item) = stream.try_next().await? {
                // Consume stream
            }
            Ok(())
        }
    }
}

/// Podman client implementation using Podman CLI
// ponytail: cfg mirrors the only call site (ContainerRuntime::connect picks docker when
// both features are on, leaving this module dead under --all-features)
#[cfg(all(feature = "podman", not(feature = "docker")))]
mod podman_client {
    use super::*;
    use std::process::Command;
    use uuid::Uuid;

    pub struct PodmanClient;

    impl PodmanClient {
        pub async fn new() -> Result<Self> {
            // Check if podman is available
            let output = Command::new("podman")
                .arg("version")
                .output()
                .context("Podman not found in PATH")?;
            if !output.status.success() {
                anyhow::bail!("Podman not available");
            }
            Ok(Self)
        }
    }

    #[async_trait]
    impl ContainerClient for PodmanClient {
        async fn create_container(&self, spec: &ContainerSpec) -> Result<String> {
            let mut args = vec![
                "run".to_string(),
                "-d".to_string(),
                "--name".to_string(),
                format!("loom-{}", Uuid::new_v4().to_string()[..8].to_string()),
            ];

            // Add volumes
            for vol in &spec.volumes {
                let mut vol_arg = format!("{}:{}", vol.source, vol.target);
                if vol.read_only {
                    vol_arg.push_str(":ro");
                }
                args.push("-v".to_string());
                args.push(vol_arg);
            }

            // Add environment variables
            for (k, v) in &spec.env {
                args.push("-e".to_string());
                args.push(format!("{}={}", k, v));
            }

            // Working directory
            args.push("-w".to_string());
            args.push(spec.working_dir.clone());

            // Network posture
            args.extend(spec.network_policy.cli_args());

            // Labels
            for (k, v) in &spec.labels {
                args.push("--label".to_string());
                args.push(format!("{}={}", k, v));
            }

            // Image and command
            args.push(spec.image.clone());
            args.extend(spec.cmd.clone());

            let output = Command::new("podman")
                .args(&args)
                .output()
                .context("Failed to execute podman run")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("podman run failed: {}", stderr);
            }

            let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(container_id)
        }

        async fn start_container(&self, id: &str) -> Result<()> {
            let output = Command::new("podman")
                .args(["start", id])
                .output()
                .context("Failed to start container")?;
            if !output.status.success() {
                anyhow::bail!(
                    "Failed to start container: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(())
        }

        async fn wait_container(&self, id: &str) -> Result<RunResult> {
            let output = Command::new("podman")
                .args(["wait", id])
                .output()
                .context("Failed to wait for container")?;

            let exit_code = String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<i32>()
                .ok();

            let logs_output = Command::new("podman")
                .args(["logs", id])
                .output()
                .context("Failed to get container logs")?;

            let stdout = String::from_utf8_lossy(&logs_output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&logs_output.stderr).to_string();

            Ok(RunResult {
                success: exit_code == Some(0),
                stdout,
                stderr,
                exit_code,
                container_id: None,
            })
        }

        async fn remove_container(&self, id: &str, force: bool) -> Result<()> {
            let mut args = vec!["rm".to_string()];
            if force {
                args.push("-f".to_string());
            }
            args.push(id.to_string());

            let output = Command::new("podman")
                .args(&args)
                .output()
                .context("Failed to remove container")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to remove container: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(())
        }

        async fn pull_image(&self, image: &str) -> Result<()> {
            let output = Command::new("podman")
                .args(["pull", image])
                .output()
                .context("Failed to pull image")?;

            if !output.status.success() {
                anyhow::bail!(
                    "Failed to pull image: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(())
        }
    }
}

/// Kubernetes client implementation using kubectl CLI
#[cfg(feature = "kubernetes")]
#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::types::LaunchConfig;

    #[test]
    fn test_runtime_registry_default() {
        let registry: RuntimeRegistry = Default::default();
        assert!(registry.get("process").is_some());
    }

    #[test]
    fn test_process_runtime_name() {
        let rt = ProcessRuntime::new();
        assert_eq!(rt.name(), "process");
    }

    #[cfg(feature = "docker")]
    #[test]
    fn test_container_runtime_name_docker() {
        assert_eq!(ContainerRuntime::docker().name(), "docker");
    }

    #[cfg(all(feature = "podman", not(feature = "docker")))]
    #[test]
    fn test_container_runtime_name_podman() {
        assert_eq!(ContainerRuntime::podman().name(), "podman");
    }

    #[test]
    fn test_runtime_registry_register() {
        let mut registry = RuntimeRegistry::new();
        registry.register(Box::new(ProcessRuntime::new())).unwrap();
        assert!(registry.get("process").is_some());
    }

    #[test]
    fn test_build_container_spec() {
        let config = LaunchConfig {
            command: "echo".to_string(),
            args: vec!["hello".to_string()],
            env: HashMap::new(),
            working_dir: Some("/workspace".to_string()),
            image: Some("test:latest".to_string()),
            ..Default::default()
        };
        let task = Task {
            task_id: "test-123".to_string(),
            title: "Test Task".to_string(),
            description: "".to_string(),
            phase: loom_core::TaskPhase::Plan,
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
            worktree_path: Some("/tmp/worktree".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            status: loom_core::TaskStatus::Pending,
            runtime: None,
            last_run: None,
        };

        let spec = build_container_spec(&config, &task, "default:latest", NetworkPolicy::default())
            .unwrap();
        assert_eq!(spec.image, "test:latest");
        assert_eq!(spec.cmd, vec!["echo", "hello"]);
        assert_eq!(spec.working_dir, "/workspace");
        assert!(!spec.volumes.is_empty());
        // An unset network value must not widen egress.
        assert_eq!(spec.network_policy, NetworkPolicy::NoNetwork);
        assert_eq!(
            spec.labels.get("loom.network_policy").map(String::as_str),
            Some("none")
        );
    }
}
