//! Debug adapter discovery — find DAP debuggers on PATH.

/// A discovered debug adapter.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DebugAdapter {
    /// Display name (e.g. "LLDB", "Delve", "DebugPy").
    pub name: String,
    /// Language(s) supported.
    pub languages: Vec<String>,
    /// Command to start the adapter.
    pub command: String,
    /// Arguments to pass.
    pub args: Vec<String>,
    /// Type identifier for the client.
    pub adapter_type: String,
}

/// Known debug adapters and their commands.
type AdapterSpec = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
    &'static [&'static str],
);
const KNOWN_ADAPTERS: &[AdapterSpec] = &[
    ("lldb-dap", &["c", "cpp", "rust", "swift"], &[], "lldb", &["lldb-dap"]),
    ("dlv", &["go"], &["dap"], "go", &["dlv"]),
    ("debugpy", &["python"], &["--listen", "0.0.0.0:5678"], "python", &["debugpy"]),
];

/// Check if a command exists on PATH.
fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Discover available debug adapters on the current PATH.
pub fn discover_adapters() -> Vec<DebugAdapter> {
    let mut adapters = Vec::new();
    for (cmd, langs, args, atype, check_cmds) in KNOWN_ADAPTERS {
        let available = check_cmds.iter().any(|c| command_exists(c));
        if available {
            adapters.push(DebugAdapter {
                name: cmd.to_string(),
                languages: langs.iter().map(|s| s.to_string()).collect(),
                command: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                adapter_type: atype.to_string(),
            });
        }
    }
    adapters
}

/// Resolve a debug adapter for the given language.
pub fn resolve_for_language(language: &str) -> Option<DebugAdapter> {
    let adapters = discover_adapters();
    adapters.into_iter().find(|a| a.languages.iter().any(|l| l == language))
}
