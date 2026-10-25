//! Resolve how to launch an ACP agent.
//!
//! Three sources, each with a different role:
//!
//! * the public ACP registry (`cdn.agentclientprotocol.com`) — the
//!   authoritative catalog of installable agents,
//! * the local `config/agents.toml` cache that `loom add-acp-agent`
//!   writes (offline, user-editable),
//! * `PATH` binaries.
//!
//! The registry stores three distribution kinds. Package runners (`npx`,
//! `uvx`) need no manual install and are preferred; `binary` entries are
//! per-platform archives that must already be installed.

use anyhow::{bail, Result};
use serde_json::Value;
use std::path::Path;

use crate::acp_client::{AcpClient, AgentMetadata};

/// Base URL of the public ACP agent registry.
pub const ACP_REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry";

/// Effective registry URL: `config/acp.toml`'s `registry_url` when set,
/// else the built-in default. One owner so the documented setting is
/// actually consulted by every registry fetch site.
pub fn registry_url(loom_dir: &str) -> String {
    loom_core::config::ConfigReader::new(loom_dir)
        .load()
        .ok()
        .and_then(|c| c.acp_registry_url)
        .unwrap_or_else(|| ACP_REGISTRY_URL.to_string())
}

/// A resolved ACP agent launch: the program to spawn plus its arguments
/// and any environment it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AcpLaunch {
    /// Executable to spawn (absolute path, or a runner such as `npx`).
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Environment variables the agent needs.
    pub env: Vec<(String, String)>,
}

/// The runner a distribution is installed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    /// `npx <package> [args]`
    Npx,
    /// `uvx <package> [args]`
    Uvx,
    /// A native executable that must already be on `PATH`.
    Binary,
}

impl Runner {
    pub fn as_str(self) -> &'static str {
        match self {
            Runner::Npx => "npx",
            Runner::Uvx => "uvx",
            Runner::Binary => "binary",
        }
    }
}

/// One installable distribution of a registry agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distribution {
    pub runner: Runner,
    /// npm/uv package name (empty for binaries).
    pub package: String,
    /// Arguments to pass to the agent.
    pub args: Vec<String>,
    /// Environment variables the agent needs.
    pub env: Vec<(String, String)>,
    /// Command inside the archive (binary distributions only).
    pub cmd: String,
}

/// Registry platform key for the current host, e.g. `darwin-aarch64`.
pub fn platform_key() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return None,
    })
}

/// Executable name inside an archive command path (`./pkg/foo` → `foo`).
pub fn exe_name(cmd: &str) -> String {
    cmd.rsplit(['/', '\\'])
        .next()
        .unwrap_or(cmd)
        .trim()
        .to_string()
}

fn json_strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn json_env(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn toml_strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn toml_env(value: Option<&toml::Value>) -> Vec<(String, String)> {
    value
        .and_then(toml::Value::as_table)
        .map(|tbl| {
            tbl.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Choose the distribution to install for a registry entry.
///
/// Deterministic preference: package runners first (`npx`, then `uvx`),
/// because they install on demand; `binary` distributions need the
/// executable already on `PATH`.
pub fn pick_distribution(entry: &AgentMetadata) -> Option<Distribution> {
    let map = entry.distribution.as_object()?;
    for runner in [Runner::Npx, Runner::Uvx] {
        if let Some(spec) = map.get(runner.as_str()) {
            return Some(Distribution {
                runner,
                package: spec
                    .get("package")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                args: json_strings(spec.get("args")),
                env: json_env(spec.get("env")),
                cmd: String::new(),
            });
        }
    }
    let binary = map.get("binary")?;
    let key = platform_key()?;
    let target = binary.get(key)?;
    Some(Distribution {
        runner: Runner::Binary,
        package: String::new(),
        args: json_strings(target.get("args")),
        env: json_env(target.get("env")),
        cmd: target
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

impl Distribution {
    /// Turn the distribution into a launch spec.
    ///
    /// Binary distributions resolve to an absolute path when the
    /// executable is on `PATH`; otherwise the bare name is returned so
    /// the caller can report a helpful "not installed" error.
    pub fn to_launch(&self) -> AcpLaunch {
        match self.runner {
            Runner::Npx => AcpLaunch {
                program: "npx".to_string(),
                args: once(&self.package)
                    .into_iter()
                    .chain(self.args.iter().cloned())
                    .collect(),
                env: self.env.clone(),
            },
            Runner::Uvx => AcpLaunch {
                program: "uvx".to_string(),
                args: once(&self.package)
                    .into_iter()
                    .chain(self.args.iter().cloned())
                    .collect(),
                env: self.env.clone(),
            },
            Runner::Binary => {
                let name = exe_name(&self.cmd);
                let program = loom_core::find_binary(&name)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(name);
                AcpLaunch {
                    program,
                    args: self.args.clone(),
                    env: self.env.clone(),
                }
            }
        }
    }

    /// Render the distribution as a `config/agents.toml` entry.
    pub fn to_toml_entry(&self) -> toml::Table {
        let mut entry = toml::Table::new();
        entry.insert(
            "runner".to_string(),
            toml::Value::String(self.runner.as_str().to_string()),
        );
        if !self.package.is_empty() {
            entry.insert(
                "package".to_string(),
                toml::Value::String(self.package.clone()),
            );
        }
        if self.runner == Runner::Binary && !self.cmd.is_empty() {
            entry.insert("cmd".to_string(), toml::Value::String(exe_name(&self.cmd)));
        }
        if !self.args.is_empty() {
            entry.insert(
                "args".to_string(),
                toml::Value::Array(
                    self.args
                        .iter()
                        .map(|a| toml::Value::String(a.clone()))
                        .collect(),
                ),
            );
        }
        if !self.env.is_empty() {
            let mut env = toml::Table::new();
            for (k, v) in &self.env {
                env.insert(k.clone(), toml::Value::String(v.clone()));
            }
            entry.insert("env".to_string(), toml::Value::Table(env));
        }
        entry
    }
}

fn once(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        vec![value.to_string()]
    }
}

/// Read a launch spec for `agent_id` from `config/agents.toml`.
///
/// Returns `None` when the agent is not registered locally.
pub fn from_agents_toml(loom_dir: &str, agent_id: &str) -> Option<AcpLaunch> {
    let path = Path::new(loom_dir).join("config").join("agents.toml");
    let raw = std::fs::read_to_string(path).ok()?;
    let table: toml::Table = toml::from_str(&raw).ok()?;
    let entry = table.get(agent_id)?.as_table()?;
    Some(launch_from_entry(agent_id, entry))
}

/// Build a launch spec from one `config/agents.toml` entry.
pub fn launch_from_entry(agent_id: &str, entry: &toml::Table) -> AcpLaunch {
    let runner = entry.get("runner").and_then(|v| v.as_str()).unwrap_or("");
    let package = entry.get("package").and_then(|v| v.as_str()).unwrap_or("");
    let args = toml_strings(entry.get("args"));
    let env = toml_env(entry.get("env"));
    match runner {
        "npx" | "npm" => AcpLaunch {
            program: "npx".to_string(),
            args: once(package).into_iter().chain(args).collect(),
            env,
        },
        "uvx" => AcpLaunch {
            program: "uvx".to_string(),
            args: once(package).into_iter().chain(args).collect(),
            env,
        },
        "pip" | "pipx" => AcpLaunch {
            program: "pipx".to_string(),
            args: once("run")
                .into_iter()
                .chain(once(package))
                .chain(args)
                .collect(),
            env,
        },
        "binary" => {
            let cmd = entry
                .get("cmd")
                .and_then(|v| v.as_str())
                .unwrap_or(agent_id);
            let name = exe_name(cmd);
            let program = loom_core::find_binary(&name)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or(name);
            AcpLaunch { program, args, env }
        }
        _ => AcpLaunch {
            program: agent_id.to_string(),
            args,
            env,
        },
    }
}

/// Look an agent up in the ACP registry (config-overridable URL).
pub async fn registry_entry(loom_dir: &str, agent_id: &str) -> Result<AgentMetadata> {
    let client = AcpClient::new(&registry_url(loom_dir), std::time::Duration::from_secs(0)).await?;
    let agents = client.discover_agents().await?;
    agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| anyhow::anyhow!("Agent `{agent_id}` not found in ACP registry"))
}

/// Resolve a launch spec for `agent_id` from the ACP registry.
pub async fn from_registry(loom_dir: &str, agent_id: &str) -> Result<AcpLaunch> {
    let entry = registry_entry(loom_dir, agent_id).await?;
    let distribution = pick_distribution(&entry).ok_or_else(|| {
        anyhow::anyhow!("Agent `{agent_id}` has no distribution available for this platform")
    })?;
    Ok(distribution.to_launch())
}

/// Resolve a launch spec for `agent_id`, preferring the local
/// `config/agents.toml` entry so the common case stays offline.
///
/// Falls back to the public registry when the agent is not registered
/// locally, and finally to a bare `PATH` executable.
///
/// A locally registered entry that carries no arguments (the shape
/// `loom add-acp-agent` used to write) is completed from the registry,
/// because most agents need a mode flag such as `acp` or `--acp` before
/// they speak the protocol at all.
pub async fn resolve(loom_dir: &str, agent_id: &str) -> Result<AcpLaunch> {
    // An explicit path is taken as-is.
    if agent_id.contains('/') || agent_id.contains('\\') {
        return Ok(AcpLaunch {
            program: agent_id.to_string(),
            args: Vec::new(),
            env: Vec::new(),
        });
    }

    let local = from_agents_toml(loom_dir, agent_id);
    let registry = from_registry(loom_dir, agent_id).await.ok();

    match (local, registry) {
        (Some(mut launch), Some(registry)) => {
            if launch.args.is_empty() && !registry.args.is_empty() {
                launch.args = registry.args;
                launch.env.extend(registry.env);
            }
            Ok(launch)
        }
        (Some(launch), None) => Ok(launch),
        (None, Some(launch)) => Ok(launch),
        (None, None) => match loom_core::find_binary(agent_id) {
            Ok(path) => Ok(AcpLaunch {
                program: path.to_string_lossy().to_string(),
                args: Vec::new(),
                env: Vec::new(),
            }),
            Err(_) => anyhow::bail!(
                "Agent '{agent_id}' is not registered and not on PATH. \
                 Register it with: loom add-acp-agent {agent_id}"
            ),
        },
    }
}

/// Verify that a launch spec can actually be started.
pub fn ensure_runnable(launch: &AcpLaunch) -> Result<()> {
    if launch.program.is_empty() {
        bail!("no ACP agent program resolved");
    }
    if launch.program.contains('/') || launch.program.contains('\\') {
        return Ok(());
    }
    if loom_core::find_binary(&launch.program).is_ok() {
        return Ok(());
    }
    bail!(
        "ACP agent '{}' is not installed (no '{}' on PATH)",
        launch.program,
        launch.program
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_url_prefers_config_and_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("acp.toml"),
            "registry_url = \"https://example.test/registry\"\n",
        )
        .unwrap();
        assert_eq!(
            registry_url(dir.path().to_str().unwrap()),
            "https://example.test/registry"
        );
        // No overlay → the built-in default.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(registry_url(empty.path().to_str().unwrap()), ACP_REGISTRY_URL);
    }

    fn metadata(distribution: Value) -> AgentMetadata {
        AgentMetadata {
            id: "test-agent".to_string(),
            name: "Test".to_string(),
            description: "test".to_string(),
            auth_methods: Vec::new(),
            distribution,
            platforms: Vec::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn picks_npx_over_binary() {
        let entry = metadata(json!({
            "binary": {"darwin-aarch64": {"archive": "https://x/y.tar.gz", "cmd": "./foo", "args": ["acp"]}},
            "npx": {"package": "foo-pkg@1.0.0", "args": ["--acp"]},
        }));
        let dist = pick_distribution(&entry).unwrap();
        assert_eq!(dist.runner, Runner::Npx);
        assert_eq!(dist.package, "foo-pkg@1.0.0");
        assert_eq!(dist.args, vec!["--acp".to_string()]);
    }

    #[test]
    fn picks_uvx_when_there_is_no_npx() {
        let entry = metadata(json!({
            "uvx": {"package": "fast-agent-acp==0.10.1", "args": ["-x"], "env": {"FAST_AGENT_MODEL": "codexplan"}},
        }));
        let dist = pick_distribution(&entry).unwrap();
        assert_eq!(dist.runner, Runner::Uvx);
        assert_eq!(
            dist.env,
            vec![("FAST_AGENT_MODEL".to_string(), "codexplan".to_string())]
        );
        let launch = dist.to_launch();
        assert_eq!(launch.program, "uvx");
        assert_eq!(
            launch.args,
            vec!["fast-agent-acp==0.10.1".to_string(), "-x".to_string()]
        );
    }

    #[test]
    fn binary_distribution_keeps_its_acp_arguments() {
        let platform = platform_key().expect("test host has a known platform key");
        let entry = metadata(json!({
            "binary": {
                platform: {"archive": "https://x/y.tar.gz", "cmd": "./opencode", "args": ["acp"]},
            },
        }));
        let dist = pick_distribution(&entry).unwrap();
        assert_eq!(dist.runner, Runner::Binary);
        assert_eq!(dist.args, vec!["acp".to_string()]);
        assert_eq!(dist.cmd, "./opencode");
    }

    #[test]
    fn binary_distribution_is_unavailable_on_other_platforms() {
        let platform = platform_key().expect("test host has a known platform key");
        let other = if platform == "linux-x86_64" {
            "darwin-arm64-not-real"
        } else {
            "linux-x86_64-not-real"
        };
        let entry = metadata(json!({
            "binary": {other: {"archive": "https://x/y.tar.gz", "cmd": "./foo"}},
        }));
        // A binary archive for another platform must never be picked.
        assert!(pick_distribution(&entry).is_none());
    }

    #[test]
    fn toml_entry_round_trips_into_a_launch() {
        let entry = metadata(json!({
            "npx": {"package": "cline@3.0.73", "args": ["--acp"]},
        }));
        let dist = pick_distribution(&entry).unwrap();
        let table = dist.to_toml_entry();
        let launch = launch_from_entry("cline", &table);
        assert_eq!(launch.program, "npx");
        assert_eq!(
            launch.args,
            vec!["cline@3.0.73".to_string(), "--acp".to_string()]
        );
    }

    #[test]
    fn legacy_npm_runner_still_resolves_to_npx() {
        let mut table = toml::Table::new();
        table.insert("runner".to_string(), toml::Value::String("npm".into()));
        table.insert("package".to_string(), toml::Value::String("my-pkg".into()));
        let launch = launch_from_entry("my-agent", &table);
        assert_eq!(launch.program, "npx");
        assert_eq!(launch.args, vec!["my-pkg".to_string()]);
    }

    #[test]
    fn legacy_pip_runner_still_resolves_to_pipx() {
        let mut table = toml::Table::new();
        table.insert("runner".to_string(), toml::Value::String("pip".into()));
        table.insert("package".to_string(), toml::Value::String("my-pkg".into()));
        let launch = launch_from_entry("my-agent", &table);
        assert_eq!(launch.program, "pipx");
        assert_eq!(launch.args, vec!["run".to_string(), "my-pkg".to_string()]);
    }

    #[test]
    fn exe_name_strips_archive_paths() {
        assert_eq!(exe_name("./opencode"), "opencode");
        assert_eq!(exe_name("./dist-package/cursor-agent"), "cursor-agent");
        assert_eq!(exe_name("./goose-package\\goose.exe"), "goose.exe");
    }
}
