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
    /// Initialize the ~/.agents/ ecosystem directory
    Init {
        /// Scan for existing resources and build the initial snapshot
        #[arg(long, short)]
        import: bool,
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
        /// Tool name(s) to enable ecosystem for (last one is launched)
        #[arg(required = true)]
        tools: Vec<String>,
        /// Arguments passed to the launched tool (after --)
        #[arg(last = true)]
        args: Vec<String>,
        /// Dry run: show env vars without executing
        #[arg(long)]
        dry_run: bool,
    },
    /// Generate shell integration script for transparent loom activation
    InitShell {
        /// Shell type (bash, zsh, fish)
        #[arg(long, default_value = "auto")]
        shell: String,
    },
    /// Add an agentic tool from the ACP registry (requires integration-acp feature)
    #[cfg(feature = "integration-acp")]
    AddAcpAgent {
        agent_id: String,
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
}

#[derive(Subcommand)]
pub enum SkillsAction {
    /// List skills (via daemon when running)
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
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
}

#[derive(Subcommand)]
pub enum AuthAction {
    /// List auth provider entries
    List {
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
    },
    /// Advance a task to the next phase
    Advance { task_id: String },
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

impl Cli {
    pub async fn execute(self) -> anyhow::Result<()> {
        let agents_dir = crate::agents_dir();
        let agents_path = PathBuf::from(&agents_dir);
        match self.command {
            Command::Init { import } => crate::commands::init::run(import, &agents_dir).await,
            Command::Scan { json } => crate::commands::scan::run(&agents_dir, json).await,
            Command::Status { json } => crate::commands::status::run(&agents_dir, json).await,
            Command::Doctor { json } => crate::commands::doctor::run(&agents_dir, json).await,
            Command::Thread {
                tool,
                apply_routing,
            } => crate::commands::thread::run(&tool, apply_routing.as_deref(), &agents_dir).await,
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
            } => crate::commands::exec::run(&tools, &args, dry_run, &agents_dir).await,
            Command::InitShell { shell } => crate::commands::init_shell::run(&shell),

            #[cfg(feature = "integration-acp")]
            Command::AddAcpAgent { agent_id } => {
                crate::commands::tools_acp::AddArgs {
                    agent_id,
                }
                .handle(&agents_dir)
                .await
            }
        }
    }
}
