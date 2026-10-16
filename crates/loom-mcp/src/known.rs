use loom_core::{McpServerEntry, McpTransport, McpTransportType};

/// A well-known MCP server that loom can install on demand.
#[derive(Debug, Clone)]
pub struct KnownMcpServer {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub npm_package: &'static str,
    pub args: &'static [&'static str],
    pub tags: &'static [&'static str],
}

/// All known MCP servers (both recommended and niche).
pub const ALL: &[KnownMcpServer] = &[
    KnownMcpServer {
        id: "filesystem",
        name: "Filesystem",
        description: "Standard filesystem access (read, write, search, directory listing)",
        npm_package: "@modelcontextprotocol/server-filesystem",
        args: &["-y", "@modelcontextprotocol/server-filesystem"],
        tags: &["filesystem", "utilities", "recommended"],
    },
    KnownMcpServer {
        id: "github",
        name: "GitHub",
        description: "GitHub API (PRs, issues, repos, file contents, search)",
        npm_package: "@modelcontextprotocol/server-github",
        args: &["-y", "@modelcontextprotocol/server-github"],
        tags: &["vcs", "github", "recommended"],
    },
    KnownMcpServer {
        id: "playwright",
        name: "Playwright",
        description: "Browser automation for web scraping, screenshots, and E2E testing",
        npm_package: "@modelcontextprotocol/server-playwright",
        args: &["-y", "@modelcontextprotocol/server-playwright"],
        tags: &["browser", "automation", "recommended"],
    },
    KnownMcpServer {
        id: "agentmemory",
        name: "AgentDB Memory",
        description: "Vector memory database — persistent agent memory with recall and search",
        npm_package: "@agentmemory/mcp",
        args: &["-y", "@agentmemory/mcp"],
        tags: &["memory", "vector", "ecosystem", "recommended"],
    },
    KnownMcpServer {
        id: "redis",
        name: "Redis",
        description: "Redis / Valkey key-value store operations",
        npm_package: "@modelcontextprotocol/server-redis",
        args: &["-y", "@modelcontextprotocol/server-redis"],
        tags: &["database", "cache"],
    },
    KnownMcpServer {
        id: "sqlite",
        name: "SQLite",
        description: "SQLite database exploration and queries",
        npm_package: "@modelcontextprotocol/server-sqlite",
        args: &["-y", "@modelcontextprotocol/server-sqlite"],
        tags: &["database", "sql"],
    },
    KnownMcpServer {
        id: "postgres",
        name: "PostgreSQL",
        description: "PostgreSQL database exploration and queries",
        npm_package: "@anthropic/server-postgres",
        args: &["-y", "@anthropic/server-postgres"],
        tags: &["database", "sql"],
    },
    KnownMcpServer {
        id: "brave-search",
        name: "Brave Search",
        description: "Web search via Brave Search API",
        npm_package: "@modelcontextprotocol/server-brave-search",
        args: &["-y", "@modelcontextprotocol/server-brave-search"],
        tags: &["search", "web"],
    },
    KnownMcpServer {
        id: "everart",
        name: "EverArt",
        description: "AI image generation with multiple models (SD, FLUX, etc.)",
        npm_package: "@anthropic/server-everart",
        args: &["-y", "@anthropic/server-everart"],
        tags: &["image", "generation", "ai"],
    },
    KnownMcpServer {
        id: "gdrive",
        name: "Google Drive",
        description: "Google Drive file search, read, and listing",
        npm_package: "@anthropic/server-gdrive",
        args: &["-y", "@anthropic/server-gdrive"],
        tags: &["storage", "google"],
    },
    KnownMcpServer {
        id: "slack",
        name: "Slack",
        description: "Slack workspace messaging and channel management",
        npm_package: "@modelcontextprotocol/server-slack",
        args: &["-y", "@modelcontextprotocol/server-slack"],
        tags: &["communication", "slack"],
    },
    KnownMcpServer {
        id: "mcp-commands",
        name: "MCP Commands",
        description: "Execute arbitrary shell commands (use with caution)",
        npm_package: "@anthropic/server-mcp-commands",
        args: &["-y", "@anthropic/server-mcp-commands"],
        tags: &["system", "utilities"],
    },
    KnownMcpServer {
        id: "sentry",
        name: "Sentry",
        description: "Sentry error tracking and issue management",
        npm_package: "@anthropic/server-sentry",
        args: &["-y", "@anthropic/server-sentry"],
        tags: &["monitoring", "errors"],
    },
    KnownMcpServer {
        id: "cloudflare",
        name: "Cloudflare",
        description: "Cloudflare API for Workers, KV, R2, D1, and DNS management",
        npm_package: "@cloudflare/mcp-server-cloudflare",
        args: &["-y", "@cloudflare/mcp-server-cloudflare"],
        tags: &["cloud", "infrastructure"],
    },
];

/// Convenience: return only servers tagged `"recommended"`.
pub fn recommended() -> Vec<&'static KnownMcpServer> {
    ALL.iter()
        .filter(|s| s.tags.contains(&"recommended"))
        .collect()
}

/// Look up a known server by id (case-insensitive).
pub fn find(id: &str) -> Option<&'static KnownMcpServer> {
    let lower = id.to_lowercase();
    ALL.iter().find(|s| s.id == lower)
}

impl KnownMcpServer {
    /// Build an [`McpServerEntry`] suitable for the registry.
    pub fn to_entry(&self) -> McpServerEntry {
        McpServerEntry {
            id: sanitise_id(self.id),
            name: self.name.to_string(),
            transport: McpTransport {
                transport_type: McpTransportType::Stdio,
                command: Some("npx".to_string()),
                args: self.args.iter().map(|a| a.to_string()).collect(),
                url: None,
                env: Default::default(),
            },
            shared: true,
            tools_filter: vec![],
            health_status: loom_core::HealthStatusStr::Unknown,
            last_health_check: String::new(),
            tags: self.tags.iter().map(|t| t.to_string()).collect(),
            compatible_tools: vec![],
            required_by_skills: vec![],
        }
    }
}

fn sanitise_id(s: &str) -> String {
    s.replace(
        |c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_',
        "-",
    )
}
