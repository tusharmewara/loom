//! Language server discovery — find LSP servers on PATH.

/// A discovered language server.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LanguageServer {
    /// Language ID (e.g. "rust", "python", "typescript").
    pub language: String,
    /// Command to start the server.
    pub command: String,
    /// Arguments to pass.
    pub args: Vec<String>,
    /// File extensions this server handles.
    pub file_extensions: Vec<String>,
}

/// Known language servers and their commands.
const KNOWN_SERVERS: &[(&str, &str, &[&str], &[&str])] = &[
    ("rust", "rust-analyzer", &[], &["rs"]),
    ("python", "pyright-langserver", &["--stdio"], &["py"]),
    ("python", "pylsp", &[], &["py"]),
    (
        "typescript",
        "typescript-language-server",
        &["--stdio"],
        &["ts", "tsx"],
    ),
    (
        "javascript",
        "typescript-language-server",
        &["--stdio"],
        &["js", "jsx"],
    ),
    ("go", "gopls", &[], &["go"]),
];

/// Check if a command exists on PATH.
fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Discover available language servers on the current PATH.
pub fn discover_servers() -> Vec<LanguageServer> {
    let mut servers = Vec::new();
    for (lang, cmd, args, exts) in KNOWN_SERVERS {
        if command_exists(cmd) {
            servers.push(LanguageServer {
                language: lang.to_string(),
                command: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                file_extensions: exts.iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    servers
}
