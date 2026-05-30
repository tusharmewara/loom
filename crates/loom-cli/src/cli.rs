use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Loom — The Shared Agentic Ecosystem
#[derive(Parser)]
#[command(name = "loom", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize the ~/.loom/ ecosystem directory
    Init {
        /// Scan for existing resources and build the initial snapshot
        #[arg(long, short)]
        import: bool,
        /// Install recommended MCP servers (filesystem, github, playwright, agentmemory)
        #[arg(long, short)]
        recommended: bool,
    },
    /// Scan the ecosystem and display discovered resources
    Scan {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Check ecosystem and daemon health status
    Status {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run deep diagnostics for daemon, service, and persistence health
    Doctor {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Auto-fix detected issues (creates missing dirs, inits missing files)
        #[arg(long, short)]
        fix: bool,
    },
    /// Register or update a tool in the ecosystem
    Thread {
        /// Tool name (jcode, crush, claude-code, or custom)
        tool: String,
        /// Apply routing decision to the tool environment
        #[arg(long)]
        apply_routing: Option<String>,
    },
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Manage skills
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Manage sessions
    Sessions {
        #[command(subcommand)]
        action: SessionsAction,
    },
    /// Manage authentication providers
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Manage hooks
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
    },
    /// Manage workflow tasks
    Workflow {
        #[command(subcommand)]
        action: WorkflowAction,
    },
    /// Model routing configuration and evaluation
    Routing {
        #[command(subcommand)]
        action: RoutingAction,
    },
    /// Execute agentic tool(s) with loom ecosystem integration
    Exec {
        /// Tool name(s) to enable ecosystem for (last one is launched, or all in parallel)
        #[arg(required = true)]
        tools: Vec<String>,
        /// Arguments passed to the launched tool (after --)
        #[arg(last = true)]
        args: Vec<String>,
        /// Dry run: show env vars without executing
        #[arg(long)]
        dry_run: bool,
        /// Run all tools in parallel (requires --pool-size)
        #[arg(long)]
        parallel: bool,
        /// Maximum concurrent tools for parallel execution (default: 4)
        #[arg(long, default_value = "4")]
        pool_size: usize,
        /// Skip git worktree isolation in parallel mode
        #[arg(long)]
        no_isolation: bool,
    },
    /// Generate shell integration script for transparent loom activation
    InitShell {
        /// Shell type (bash, zsh, fish)
        #[arg(long, default_value = "auto")]
        shell: String,
    },
    /// Sync Loom-managed resources to tool-native filesystem locations
    Sync {
        /// Sync only this specific tool (jcode, crush, claude-code, opencode, goose)
        #[arg(long)]
        tool: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Add an agentic tool from the ACP registry (cdn.agentclientprotocol.com)
    AddAcpAgent {
        agent_id: String,
    },
    /// Launch the terminal UI dashboard
    #[cfg(feature = "tui")]
    Tui,
    /// Manage skill lock file for reproducibility
    Lock {
        /// Generate lock file from current registry state
        #[arg(long, short)]
        generate: bool,
        /// Verify installed skills match the lock file
        #[arg(long, short)]
        verify: bool,
    },
    /// Manage AgentMemory persistent memory server
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Manage issue tracker integration (GitHub Issues)
    Issues {
        #[command(subcommand)]
        action: IssuesAction,
    },
    /// Show/validate merged config
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
pub enum DaemonAction {
    /// Start the loomd background process
    Start,
    /// Stop the loomd background process
    Stop,
    /// Show daemon status
    Status {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Restart the loomd background process
    Restart,
    /// Install loomd as a user service (launchd/systemd)
    InstallService,
    /// Uninstall loomd user service
    UninstallService,
}

#[derive(Subcommand)]
pub enum McpAction {
    /// List registered MCP servers
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show an MCP server configuration
    Show {
        id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new MCP server stub
    Add { name: String },
    /// Remove an MCP server
    Remove { id: String },
    /// Install a known MCP server (npm-based)
    Install {
        name: String,
        /// Also pre-warm the npm cache (slower now, blazing fast later)
        #[arg(long, short)]
        prewarm: bool,
    },
    /// Install all recommended MCP servers
    InstallRecommended {
        /// Also pre-warm the npm cache
        #[arg(long, short)]
        prewarm: bool,
    },
    /// List known MCP servers available for install
    ListKnown {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the Loom MCP server (stdio-based, for MCP-compatible tools)
    Serve,
}

#[derive(Subcommand)]
pub enum SkillsAction {
    /// List skills (via daemon when running)
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Verify against lock file
        #[arg(long)]
        locked: bool,
    },
    /// Show a skill by name
    Show {
        name: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Install a skill from a file path
    Install { path: String },
    /// Remove a skill by name
    Remove { name: String },
}

#[derive(Subcommand)]
pub enum SessionsAction {
    /// List session manifests
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a session manifest
    Show {
        id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Export a session to a portable JSON bundle
    Export {
        id: String,
        /// Output file path (default: sessions/exports/{id}.loom.json)
        #[arg(long, short)]
        output: Option<String>,
    },
    /// Import a session from a portable export file
    Import { path: String },
    /// Resume a session interactively with an LLM provider or launch an agent tool
    Resume {
        id: String,
        /// LLM provider (openai, anthropic, or custom endpoint URL).
        /// Falls back to LOOM_ROUTING_PROVIDER env var, then
        /// routing.default_provider in config.
        #[arg(long, short)]
        provider: Option<String>,
        /// Model name (e.g. gpt-4, claude-sonnet-4-20250514).
        /// Falls back to routing.default_model in config, then
        /// per-provider defaults.
        #[arg(long, short)]
        model: Option<String>,
        /// Agent tool to launch with session context (e.g. "opencode", "goose").
        /// When set, launches the tool instead of the interactive REPL.
        #[arg(long)]
        agent: Option<String>,
        /// Inject cross-session context from memory store
        #[arg(long)]
        inject_memory: bool,
    },
    /// Search across all session messages using full-text search
    Search {
        /// Search query (FTS5 syntax supported)
        query: String,
        /// Maximum results
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Index all sessions into the cross-session memory store
    Index,
    /// Branch a session at a checkpoint, creating a forked copy
    Branch {
        /// New session ID for the branch (default: auto-generated)
        id: Option<String>,
        /// Source session ID to branch from
        #[arg(long, short)]
        from: String,
        /// Message line number to branch at (0-based). If omitted, branches at end.
        #[arg(long, short)]
        checkpoint: Option<usize>,
    },
}

#[derive(Subcommand)]
pub enum AuthAction {
    /// List auth entries in the auth directory
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List registered LLM providers from the provider registry
    Providers {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a provider configuration
    Show {
        provider_id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate a provider credential
    Validate { provider_id: String },
}

#[derive(Subcommand)]
pub enum HooksAction {
    /// List registered hooks
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a hook configuration
    Show {
        id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Register a hook from a file path
    Register { path: String },
    /// Remove a hook
    Remove { id: String },
    /// Run hooks for an event (parallel execution)
    Run {
        /// Event type: pre-tool-use, post-tool-use, session-start
        #[arg(long)]
        event: String,
        /// Tool name invoking the action
        #[arg(long, default_value = "unknown")]
        tool: String,
        /// Optional matcher string (e.g. command being run)
        #[arg(long)]
        matcher: Option<String>,
    },
    /// Verify hook signatures in the registry
    Verify,
}

#[derive(Subcommand)]
pub enum WorkflowAction {
    /// List workflow tasks
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a task
    Show {
        task_id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new task
    Create {
        /// Task title
        #[arg(long, short)]
        title: Option<String>,
        /// Task description (used for complexity scoring)
        #[arg(long, short)]
        description: Option<String>,
        /// Create task from a GitHub issue (e.g. "owner/repo#123")
        #[arg(long)]
        from_issue: Option<String>,
        /// Repo for the issue (needed if from_issue is just a number)
        #[arg(long, default_value = "")]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env)
        #[arg(long)]
        token: Option<String>,
        /// Create task from a workflow template
        #[arg(long)]
        template: Option<String>,
    },
    /// Advance a task to the next phase
    Advance {
        task_id: String,
        /// Post a comment on the linked GitHub issue
        #[arg(long)]
        update_issue: bool,
        /// Custom comment text (with --update-issue)
        #[arg(long)]
        comment: Option<String>,
    },
    /// Assign a routed provider to a task (with optional overrides)
    Assign {
        task_id: String,
        /// Override the provider (e.g., "anthropic", "openai")
        #[arg(long)]
        provider: Option<String>,
        /// Override the model (e.g., "claude-opus-4", "gpt-4o")
        #[arg(long)]
        model: Option<String>,
        /// Set the agent tool for this task (e.g., "opencode", "goose")
        #[arg(long)]
        agent: Option<String>,
    },
    /// Dispatch a task to an agent tool (e.g., "opencode", "goose").
    /// Defaults to the last agent that worked on the task.
    /// With --mode sub-agents, uses the per-phase agent configuration.
    Dispatch {
        task_id: String,
        /// Agent tool to dispatch to (default: task.last_agent)
        #[arg(long)]
        agent: Option<String>,
        /// Link an issue to this task (e.g. "owner/repo#123")
        #[arg(long)]
        link_issue: Option<String>,
        /// Also create a git branch from the linked issue
        #[arg(long)]
        create_branch: bool,
        /// Dispatch mode: single (default) or sub-agents
        #[arg(long, default_value = "single")]
        mode: String,
        /// Additional args to pass to the agent tool
        #[arg(last = true)]
        tool_args: Vec<String>,
        /// Dry run (print env vars without launching)
        #[arg(long)]
        dry_run: bool,
    },
    /// Configure per-phase sub-agents for a task
    SetSubAgents {
        task_id: String,
        /// Agent for the Plan phase (e.g. "opencode")
        #[arg(long)]
        plan: Option<String>,
        /// Agent for the Implement phase (e.g. "goose")
        #[arg(long)]
        implement: Option<String>,
        /// Agent for the Verify phase (e.g. "jcode")
        #[arg(long)]
        verify: Option<String>,
        /// Agent for the Finish phase (e.g. "crush")
        #[arg(long)]
        finish: Option<String>,
    },
    /// Manage workflow templates
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub enum TemplateAction {
    /// List available workflow templates
    List,
    /// Show a template's configuration
    Show {
        name: String,
    },
    /// Create or update a workflow template
    Create {
        name: String,
        /// Template description
        #[arg(long)]
        description: String,
        /// Default task title (supports {issue_title} placeholder)
        #[arg(long)]
        default_title: Option<String>,
        /// Default task description
        #[arg(long)]
        default_description: Option<String>,
        /// Default agent tool
        #[arg(long)]
        default_agent: Option<String>,
        /// Default LLM provider
        #[arg(long)]
        default_provider: Option<String>,
        /// Default model
        #[arg(long)]
        default_model: Option<String>,
        /// Agent for Plan phase
        #[arg(long)]
        sub_plan: Option<String>,
        /// Agent for Implement phase
        #[arg(long)]
        sub_implement: Option<String>,
        /// Agent for Verify phase
        #[arg(long)]
        sub_verify: Option<String>,
        /// Agent for Finish phase
        #[arg(long)]
        sub_finish: Option<String>,
    },
    /// Remove a workflow template
    Remove { name: String },
}

#[derive(Subcommand)]
pub enum RoutingAction {
    /// Show routing configuration and model catalog
    Show {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Evaluate routing for a prompt (score + decision)
    Evaluate {
        prompt: String,
        #[arg(long)]
        json: bool,
    },
    /// Show only the complexity score for a prompt
    Score {
        prompt: String,
        #[arg(long)]
        json: bool,
    },
    /// Get routing recommendation for an existing task
    Recommend {
        task_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Override routing decision for a task
    Override {
        task_id: String,
        /// Tier to force (slm, mid, flagship)
        tier: String,
        /// Reason for override
        #[arg(long)]
        reason: Option<String>,
    },
    /// Manage model catalog
    Catalog {
        /// Refresh catalog from providers
        #[arg(long)]
        refresh: bool,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum MemoryAction {
    /// Show agentmemory server status
    Status {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the agentmemory memory server
    Start,
    /// Stop the agentmemory memory server
    Stop,
}

#[derive(Subcommand)]
pub enum IssuesAction {
    /// List issues from the configured repository
    List {
        /// Repository (owner/repo, or team ID for Linear)
        #[arg(long)]
        repo: String,
        /// Filter by state: open, closed, all
        #[arg(long, default_value = "open")]
        state: Option<String>,
        /// Filter by labels (comma-separated)
        #[arg(long)]
        labels: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// GitHub token (defaults to GITHUB_TOKEN env) or Linear API key (--provider linear)
        #[arg(long)]
        token: Option<String>,
        /// Provider: github or linear
        #[arg(long, default_value = "github")]
        provider: String,
    },
    /// Show a single issue
    Show {
        /// Issue ID (number, UUID, or tracker identifier)
        id: String,
        /// Repository (owner/repo) — needed if id is just a number for GitHub
        #[arg(long, default_value = "")]
        repo: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// GitHub token (defaults to GITHUB_TOKEN env) or Linear API key
        #[arg(long)]
        token: Option<String>,
        /// Provider: github or linear
        #[arg(long, default_value = "github")]
        provider: String,
    },
    /// Create a git branch from an issue
    CreateBranch {
        /// Issue ID (number, UUID, or tracker identifier)
        id: String,
        /// Repository (owner/repo) — needed if id is just a number for GitHub
        #[arg(long, default_value = "")]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env) or Linear API key
        #[arg(long)]
        token: Option<String>,
        /// Provider: github or linear
        #[arg(long, default_value = "github")]
        provider: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the merged config (after deep-merge interpolation)
    Show {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Validate the merged config for errors
    Validate,
    /// Fetch remote config from URL (HTTPS) with SHA-256 caching
    Fetch {
        /// Remote config URL (overrides config.toml setting)
        #[arg(long)]
        url: Option<String>,
    },
}

impl Cli {
    pub async fn execute(self) -> anyhow::Result<()> {
        let loom_dir = crate::loom_dir();
        let agents_path = PathBuf::from(&loom_dir);
        match self.command {
            Command::Init { import, recommended } => {
                crate::commands::init::run(import, recommended, &loom_dir).await
            }
            Command::Scan { json } => crate::commands::scan::run(&loom_dir, json).await,
            Command::Status { json } => crate::commands::status::run(&loom_dir, json).await,
            Command::Doctor { json, fix } => crate::commands::doctor::run(&loom_dir, json, fix).await,
            Command::Thread {
                tool,
                apply_routing,
            } => crate::commands::thread::run(&tool, apply_routing.as_deref(), &loom_dir).await,
            Command::Daemon { action } => crate::commands::daemon::run(&agents_path, &action).await,
            Command::Mcp { action } => crate::commands::mcp::run(&agents_path, &action).await,
            Command::Skills { action } => crate::commands::skills::run(&agents_path, &action).await,
            Command::Sessions { action } => crate::commands::sessions::run(&agents_path, &action).await,
            Command::Auth { action } => crate::commands::auth::run(&agents_path, &action).await,
            Command::Hooks { action } => crate::commands::hooks::run(&agents_path, &action).await,
            Command::Workflow { action } => crate::commands::workflow::run(&agents_path, &action).await,
            Command::Routing { action } => crate::commands::routing::run(&agents_path, &action).await,
            Command::Exec {
                tools,
                args,
                dry_run,
                parallel,
                pool_size,
                no_isolation,
            } => {
                crate::commands::exec::run(
                    &tools,
                    &args,
                    dry_run,
                    parallel,
                    pool_size,
                    no_isolation,
                    &loom_dir,
                )
                .await
            }
            Command::InitShell { shell } => crate::commands::init_shell::run(&shell),
            Command::Sync { tool, json } => {
                crate::commands::sync::run(tool.as_deref(), &loom_dir, json).await
            }

            Command::AddAcpAgent { agent_id } => {
                crate::commands::tools_acp::AddArgs {
                    agent_id,
                }
                .handle(&loom_dir)
                .await
            }
            #[cfg(feature = "tui")]
            Command::Tui => loom_tui::run().await,
            Command::Lock { generate, verify } => {
                crate::commands::lock::run(&loom_dir, generate, verify)
            }
            Command::Memory { action } => {
                crate::commands::memory::run(&agents_path, &action).await
            }
            Command::Issues { action } => {
                crate::commands::issues::run(&agents_path, &action).await
            }
            Command::Config { action } => {
                crate::commands::config::run(&loom_dir, &action).await
            }
        }
    }
}
