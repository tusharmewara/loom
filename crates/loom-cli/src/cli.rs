use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::commands::telemetry::TelemetryAction;

/// Loom — The Shared Agentic Ecosystem
#[derive(Parser)]
#[command(name = "loom", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Skip automatic update check on startup
    #[arg(long, global = true)]
    pub no_update: bool,
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
        /// Bootstrap from a remote config URL (HTTPS). The URL is written
        /// to `config.toml` as `remote_config_url`, and the content is
        /// fetched and cached immediately. Subsequent runs of any `loom`
        /// command will deep-merge the cached remote config on top of
        /// the local tree.
        #[arg(long, value_name = "URL")]
        source: Option<String>,
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
        /// Verbose output with explanations for each check
        #[arg(long, short)]
        verbose: bool,
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
    /// Runtime management
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },
    /// Event stream rules management
    Rules {
        #[command(subcommand)]
        action: RulesAction,
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
        /// Show diff-style output for violations (used with --verify)
        #[arg(long)]
        diff: bool,
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
    /// Manage the plugin registry (search, install, update, remove)
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Manage remote SSH connections
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },
    /// LSP integration: rename, goto-def, find-refs, code-actions
    Lsp {
        #[command(subcommand)]
        action: LspAction,
    },
    /// Debug Adapter Protocol: launch, attach, breakpoints, step, evaluate
    Debug {
        #[command(subcommand)]
        action: DebugAction,
    },
    /// Manage the knowledge graph (extract, query, add entities, stats)
    Knowledge {
        #[command(subcommand)]
        action: KnowledgeAction,
    },
    /// Content-hash anchored editing (Hashline)
    Edit {
        #[command(subcommand)]
        action: EditAction,
    },
    /// AST structural editing with tree-sitter
    Ast {
        #[command(subcommand)]
        action: AstAction,
    },
    /// Detect and resolve merge conflicts
    Conflict {
        #[command(subcommand)]
        action: ConflictAction,
    },
    /// Manage the ACP dispatch server
    Acp {
        #[command(subcommand)]
        action: AcpAction,
    },
    /// Generate shell completions
    Completions {
        /// Shell type (bash, zsh, fish, powershell, elvish)
        shell: clap_complete::Shell,
    },
    /// Self-update: check for new version and install
    SelfUpdate {
        /// Force re-download even if same version
        #[arg(long)]
        force: bool,
        /// Release source: "github" (default) or "releases.loom.sh"
        #[arg(long, default_value = "github")]
        source: String,
    },
    /// Telemetry metrics and counters
    Telemetry {
        #[command(subcommand)]
        action: TelemetryAction,
    },
}

#[derive(Subcommand)]
pub enum PluginAction {
    /// Search registered plugins by name or description
    Search {
        /// Search query
        query: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Install a plugin from a tool TOML URL
    Install {
        /// Plugin name
        name: String,
        /// URL to the tool TOML file
        url: String,
        /// Optional version string
        #[arg(long)]
        version: Option<String>,
        /// Verify Ed25519 signature (requires public_key + signature fields)
        #[arg(long)]
        verify: bool,
    },
    /// Register a plugin from a local tool TOML file
    Register {
        /// Plugin name
        name: String,
        /// Path to the tool TOML file
        path: String,
    },
    /// Update a plugin (re-download its tool TOML)
    Update {
        /// Plugin name
        name: String,
    },
    /// Remove a plugin and its tool TOML
    Remove {
        /// Plugin name
        name: String,
    },
    /// List all installed plugins
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show detail for a specific plugin
    Show {
        name: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum RemoteAction {
    /// List all configured remote connections
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new remote connection
    Add {
        /// Connection name
        name: String,
        /// Remote hostname or IP
        host: String,
        /// SSH port (default 22)
        #[arg(long, default_value = "22")]
        port: u16,
        /// Remote username
        #[arg(long, default_value = "root")]
        user: String,
        /// Path to SSH identity file
        #[arg(long)]
        key: Option<String>,
        /// Store the key in the encrypted credential vault
        #[arg(long)]
        store_in_vault: bool,
        /// Vault password (prompts if not provided and vault is locked)
        #[arg(long)]
        vault_password: Option<String>,
        /// Use ssh-agent (default)
        #[arg(long)]
        agent: bool,
        /// Jump host
        #[arg(long)]
        jump: Option<String>,
    },
    /// Remove a remote connection
    Remove {
        name: String,
    },
    /// Execute a command on a remote host
    Exec {
        /// Connection name
        conn: String,
        /// Command to run
        command: Vec<String>,
        /// Capture output as a Loom session (memory + knowledge indexed)
        #[arg(long)]
        capture_session: bool,
    },
    /// Sync files to/from a remote host
    Sync {
        /// Connection name
        conn: String,
        /// Push local → remote (default), or pull remote → local
        #[arg(long)]
        pull: bool,
        /// Use scp instead of rsync
        #[arg(long)]
        scp: bool,
        /// Local path or remote path
        source: String,
        /// Remote path or local path
        dest: String,
    },
}

#[derive(Subcommand)]
pub enum LspAction {
    /// List available language servers on PATH
    Servers {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Rename a symbol across the workspace
    Rename {
        /// Path to the file containing the symbol
        #[arg(long)]
        path: String,
        /// Line number (0-based)
        #[arg(long)]
        line: u32,
        /// Column number (0-based)
        #[arg(long)]
        column: u32,
        /// New name for the symbol
        new_name: String,
        /// Language server command (default: auto-detect from extension)
        #[arg(long)]
        server: Option<String>,
    },
    /// Find all references to a symbol
    FindReferences {
        /// Path to the file containing the symbol
        #[arg(long)]
        path: String,
        /// Symbol name or line to search
        #[arg(long)]
        symbol: String,
        /// Language server command (default: auto-detect from extension)
        #[arg(long)]
        server: Option<String>,
    },
    /// Execute an LSP operation through a registered tool adapter
    Exec {
        /// Registered tool name (e.g. jcode, goose, claude-code)
        tool: String,
        /// LSP operation to execute: rename, find-references, gotodef, code-action, symbols
        operation: String,
        /// Path to the target file
        #[arg(long)]
        path: String,
        /// Symbol or new-name argument
        #[arg(long)]
        symbol: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: Option<u32>,
        /// Column number (0-based)
        #[arg(long)]
        column: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum DebugAction {
    /// List available debug adapters on PATH
    Adapters {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Launch a program for debugging
    Launch {
        /// Path to the program to debug
        #[arg(long)]
        program: String,
        /// Language of the program (rust, go, python, cpp)
        #[arg(long)]
        language: String,
        /// Arguments to pass to the program
        #[arg(long)]
        args: Vec<String>,
    },
    /// Attach to a running process
    Attach {
        /// Process ID to attach to
        #[arg(long)]
        pid: u32,
        /// Language of the target process (rust, go, python)
        #[arg(long)]
        language: String,
    },
    /// Set breakpoints in a source file (requires active session)
    Breakpoints {
        /// Source file path
        #[arg(long)]
        file: String,
        /// Line numbers
        #[arg(long)]
        line: Vec<u32>,
    },
    /// Step through execution (over, in, out)
    Step {
        /// Direction: over, in, out
        direction: String,
    },
    /// Evaluate an expression in the current debug context
    Evaluate {
        /// Expression to evaluate
        expression: String,
    },
    /// Show current stack trace
    Stack,
    /// Connect to an running debug session (launched via `loom debug launch`) and continue it.
    Continue,
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
        /// Filter to sessions whose tag set intersects any of the given tags.
        /// Repeat the flag to OR multiple tags: `--tag rust --tag go` matches
        /// sessions tagged with either. Sessions with no tags are never
        /// included when this filter is set.
        #[arg(long, value_name = "TAG")]
        tag: Vec<String>,
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
        /// Inject knowledge graph context (entities & relationships)
        #[arg(long)]
        inject_knowledge: bool,
    },
    /// Search across all session messages using full-text search
    Search {
        /// Search query (FTS5 syntax supported)
        query: String,
        /// Maximum results
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Filter to sessions whose tag set intersects any of the given tags
        #[arg(long, value_name = "TAG")]
        tag: Vec<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Recall past sessions that match a query, grouped by session
    /// (best-matching top, hit count + best match per session).
    /// Use this when you want to find prior work relevant to a topic;
    /// use `search` to find individual messages.
    Recall {
        /// Search query (FTS5 syntax supported)
        query: String,
        /// Maximum number of sessions to return
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Filter to sessions whose tag set intersects any of the given tags
        #[arg(long, value_name = "TAG")]
        tag: Vec<String>,
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
    /// Add or remove tags on a session. Tags are stored in the session
    /// manifest and indexed into the cross-session memory store for
    /// filtering `search`/`recall`/`list` results.
    Tag {
        /// Session ID to mutate
        id: String,
        /// Tags to add (repeat the flag to add multiple)
        #[arg(long, value_name = "TAG")]
        add: Vec<String>,
        /// Tags to remove (repeat the flag to remove multiple)
        #[arg(long, value_name = "TAG")]
        remove: Vec<String>,
    },
    /// List all distinct tags in the cross-session memory store along
    /// with the number of sessions that have each tag. Useful for
    /// discovering what tags exist before applying filters.
    Tags {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run (or re-run) auto-summarization on a session. When the
    /// session's message count exceeds the compression threshold
    /// (see `SESSION_COMPRESSION_THRESHOLD` in the source), the older
    /// messages are folded into a single `System`-role summary at
    /// the head of the message list, and the last
    /// `SESSION_FULL_FIDELITY_COUNT` messages are kept verbatim. The
    /// manifest's `summarized_at` / `summarized_message_count` fields
    /// record the pass. Auto-runs on `append_message` once a session
    /// crosses the threshold — this command is the manual escape
    /// hatch and is also useful for back-filling older sessions.
    Summarize {
        /// Session ID to compress
        id: String,
        /// Re-compress even if the session is already below the
        /// threshold. No-op without this flag when there's nothing
        /// to fold.
        #[arg(long)]
        force: bool,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Merge messages from one session into another (appends to destination)
    Merge {
        /// Destination session ID (receives merged messages)
        dest: String,
        /// Source session ID (messages taken from here)
        source: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
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
pub enum RulesAction {
    /// List all rules
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a rule by ID
    Show {
        id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new rule
    Add {
        /// Rule ID (also used as filename)
        id: String,
        /// Human-readable name
        #[arg(long, default_value = "")]
        name: String,
        /// Event type to match (e.g. SkillsAdded)
        #[arg(long)]
        event_type: Option<String>,
        /// Field to compare (e.g. count)
        #[arg(long)]
        field: Option<String>,
        /// Comparison operator: equals, neq, gt, lt, contains, starts-with, ends-with
        #[arg(long)]
        operator: Option<String>,
        /// Value to compare against
        #[arg(long)]
        value: Option<String>,
        /// Log message to emit on match (use this action)
        #[arg(long)]
        action_log: Option<String>,
        /// Suppress the event (use this action)
        #[arg(long)]
        action_suppress: bool,
        /// Rule priority (higher = evaluated first)
        #[arg(long)]
        priority: Option<i32>,
    },
    /// Remove a rule
    Remove {
        id: String,
    },
    /// Evaluate a synthetic event against all rules (test)
    Eval {
        /// Event type name
        #[arg(long, default_value = "RoutingUpdated")]
        event_type: String,
        /// Field name for the synthetic event
        #[arg(long)]
        field: Option<String>,
        /// Field value
        #[arg(long)]
        value: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum RuntimeAction {
    /// List available runtimes
    List {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Set default runtime
    SetDefault {
        /// Runtime name (process, docker)
        name: String,
    },
    /// Show runtime status
    Status,
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
        #[arg(long, short, allow_hyphen_values = true)]
        title: Option<String>,
        /// Task description (used for complexity scoring)
        #[arg(long, short, allow_hyphen_values = true)]
        description: Option<String>,
        /// Create task from a GitHub issue (e.g. "owner/repo#123")
        #[arg(long)]
        from_issue: Option<String>,
        /// Create task from any ticket (github:owner/repo#123,
        /// linear:TEAM-1, gitlab:owner/repo#1, jira:PROJ-1)
        #[arg(long)]
        from_ticket: Option<String>,
        /// Repo for the issue (needed if from_issue is just a number)
        #[arg(long, default_value = "")]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env)
        #[arg(long)]
        token: Option<String>,
        /// Create task from a workflow template
        #[arg(long)]
        template: Option<String>,
        /// Create tasks from a chief plan JSON file
        #[arg(long)]
        from_plan: Option<String>,
        /// Auto-create git worktree for this task
        #[arg(long, default_value_t = true)]
        worktree: bool,
        /// Explicitly disable git worktree creation
        #[arg(long)]
        no_worktree: bool,
    },
    /// Remove a workflow task and its per-phase sidecars
    Remove {
        task_id: String,
    },
    /// Draft a task plan from a goal plus free-text details
    DraftPlan {
        /// Plan goal
        #[arg(long, allow_hyphen_values = true)]
        title: Option<String>,
        /// Free-text details (bullets/numbered lines become tasks)
        #[arg(long, allow_hyphen_values = true)]
        description: Option<String>,
        /// Import the draft as workflow tasks instead of only printing it
        #[arg(long)]
        create: bool,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
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
        /// Runtime backend: process, docker, podman, kubernetes
        #[arg(long)]
        runtime: Option<String>,
        /// Additional args to pass to the agent tool
        #[arg(last = true)]
        tool_args: Vec<String>,
        /// Dry run (print env vars without launching)
        #[arg(long)]
        dry_run: bool,
        /// After agent exits, capture phase result and advance to next phase
        /// (only with --mode sub-agents). Loops until phase is Finish or an
        /// agent exits non-zero. Requires the agent to write a result file
        /// at `~/.loom/workflow/tasks/{task_id}.{phase}.result.json` (or
        /// `.result.md` for a plain summary).
        #[arg(long)]
        auto_advance: bool,
        /// Skip session capture for this dispatch. By default, every dispatch
        /// (single or part of --auto-advance) is saved as a session in
        /// `~/.loom/sessions/{task_id}.{phase}/` and indexed into cross-
        /// session memory and the knowledge graph, so the next phase can
        /// see what prior phases produced.
        #[arg(long)]
        no_capture_session: bool,
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
        /// Max retries per phase before using fallback
        #[arg(long, default_value = "3")]
        max_retries: u8,
        /// Fallback agent when retries exhausted
        #[arg(long)]
        fallback_agent: Option<String>,
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
    /// Retain a durable fact in hindsight memory (cross-session)
    Retain {
        /// The fact content to store
        fact: String,
        /// Type tag (insight, decision, pattern, error, preference, or custom)
        #[arg(long, default_value = "insight")]
        r#type: String,
        /// Source identifier (e.g. tool name)
        #[arg(long, default_value = "cli")]
        source: String,
        /// Importance score 0.0-1.0
        #[arg(long, default_value_t = 0.5)]
        importance: f64,
    },
    /// Search hindsight memory for relevant facts
    Recall {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Synthesize an overview from hindsight memory
    Reflect {
        /// Question or topic to reflect on
        question: String,
    },
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
        /// Provider: github, linear, jira, or gitlab
        #[arg(long, default_value = "github")]
        provider: String,
        /// Jira host URL (e.g. https://mycompany.atlassian.net) or GitLab host (default https://gitlab.com)
        #[arg(long)]
        host: Option<String>,
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
        /// Provider: github, linear, jira, or gitlab
        #[arg(long, default_value = "github")]
        provider: String,
        /// Jira host URL or GitLab host (default https://gitlab.com)
        #[arg(long)]
        host: Option<String>,
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
        /// Provider: github, linear, jira, or gitlab
        #[arg(long, default_value = "github")]
        provider: String,
        /// Jira host URL or GitLab host (default https://gitlab.com)
        #[arg(long)]
        host: Option<String>,
    },
    /// Link a PR URL to an issue (adds a comment)
    LinkPr {
        /// Issue ID (number, UUID, or tracker identifier)
        id: String,
        /// PR URL to link
        pr_url: String,
        /// Repository (owner/repo) — needed if id is just a number for GitHub
        #[arg(long, default_value = "")]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env) or Linear API key
        #[arg(long)]
        token: Option<String>,
        /// Provider: github, linear, jira, or gitlab
        #[arg(long, default_value = "github")]
        provider: String,
        /// Jira host URL or GitLab host (default https://gitlab.com)
        #[arg(long)]
        host: Option<String>,
    },
    /// List pull requests from a repository (GitHub only)
    PullRequests {
        /// Repository (owner/repo)
        #[arg(long)]
        repo: String,
        /// Filter by state: open, closed, all
        #[arg(long, default_value = "open")]
        state: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// GitHub token (defaults to GITHUB_TOKEN env)
        #[arg(long)]
        token: Option<String>,
    },
    /// Show the diff for a pull request (GitHub only)
    Diff {
        /// PR number
        pr: u64,
        /// Repository (owner/repo)
        #[arg(long)]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env)
        #[arg(long)]
        token: Option<String>,
    },
    /// Show CI status for a pull request (GitHub only)
    CiStatus {
        /// PR number
        pr: u64,
        /// Repository (owner/repo)
        #[arg(long)]
        repo: String,
        /// GitHub token (defaults to GITHUB_TOKEN env)
        #[arg(long)]
        token: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a pull request with gh
    CreatePr {
        /// Repository (owner/repo)
        #[arg(long)]
        repo: String,
        /// PR title (default: let gh fill from the branch)
        #[arg(long)]
        title: Option<String>,
        /// PR body (default: let gh fill from commits)
        #[arg(long)]
        body: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the merged config (after deep-merge interpolation)
    Show {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Show the resolved config source chain (which file contributed
        /// each value, in precedence order).
        #[arg(long)]
        extends: bool,
        /// Skip the `extends = [...]` chain (load only `config.toml`
        /// and overlays). Useful for debugging base configs in isolation.
        #[arg(long)]
        no_extend: bool,
        /// Skip applying the cached `remote_config_url` content.
        #[arg(long)]
        no_remote: bool,
    },
    /// Validate the merged config for errors
    Validate {
        /// Skip the `extends = [...]` chain during validation
        #[arg(long)]
        no_extend: bool,
        /// Skip the cached `remote_config_url` content during validation
        #[arg(long)]
        no_remote: bool,
    },
    /// Fetch remote config from URL (HTTPS) with SHA-256 caching
    Fetch {
        /// Remote config URL (overrides config.toml setting)
        #[arg(long)]
        url: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum KnowledgeAction {
    /// Build knowledge graph from indexed sessions
    Build {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Use LLM-based extraction instead of pattern matching
        #[arg(long)]
        llm: bool,
    },
    /// Query subgraph around an entity
    Query {
        /// Entity name or ID to query
        entity: String,
        /// Maximum traversal depth (default: 2)
        #[arg(long, default_value = "2")]
        depth: u32,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show graph statistics
    Stats {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Add an entity manually
    Add {
        /// Entity name
        name: String,
        /// Entity type (tool, concept, language, database, provider, etc.)
        #[arg(long, default_value = "concept")]
        entity_type: String,
        /// Canonical/parent name for grouping
        #[arg(long)]
        canonical: Option<String>,
    },
    /// Add a relationship between two entities
    Relate {
        /// Source entity name
        from: String,
        /// Predicate (e.g., "built_with", "uses", "related_to")
        predicate: String,
        /// Target entity name
        to: String,
        /// Confidence weight (0.0–1.0)
        #[arg(long, default_value = "1.0")]
        weight: f64,
    },
    /// Search entities by name/type
    Search {
        /// Search query
        query: String,
        /// Maximum results (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Clear the entire knowledge graph
    Clear {
        /// Confirm destruction
        #[arg(long, short)]
        force: bool,
    },
}

/// AST structural editing with tree-sitter
#[derive(Subcommand)]
pub enum AstAction {
    /// Parse a file and show its AST tree
    Parse {
        /// File to parse
        file: String,
        /// Language override (rust, python, javascript, go, json)
        #[arg(long)]
        language: Option<String>,
        /// Maximum depth for tree display (default: 3)
        #[arg(long, default_value = "3")]
        max_depth: u32,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Execute a tree-sitter query against the AST
    Query {
        /// File to query
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Tree-sitter query string (e.g. "(function_item name: (identifier) @fn.name)")
        query: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the node at a given position
    NodeAt {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Move a node up (swap with previous sibling)
    MoveUp {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Move a node down (swap with next sibling)
    MoveDown {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Swap two sibling nodes
    Swap {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// First node line (0-based)
        #[arg(long)]
        line_a: usize,
        /// First node column (0-based)
        #[arg(long)]
        column_a: usize,
        /// Second node line (0-based)
        #[arg(long)]
        line_b: usize,
        /// Second node column (0-based)
        #[arg(long)]
        column_b: usize,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Extract a node into a named variable/function
    Extract {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Name for the extracted entity
        name: String,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Wrap a node in a block or construct
    Wrap {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Wrapper type: if, if_true, if_false, loop, while, block, {}, parens, (), brackets, [], try, try_catch, unsafe, or custom with $BODY placeholder
        wrapper: String,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Unwrap a node (remove outer construct, keep inner content)
    Unwrap {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Duplicate a node (insert copy after it)
    Duplicate {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Insert text at a specific position
    Insert {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
        /// Text to insert
        text: String,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Replace text at a range
    Replace {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Start line (0-based)
        #[arg(long)]
        start_line: usize,
        /// Start column (0-based)
        #[arg(long)]
        start_column: usize,
        /// End line (0-based)
        #[arg(long)]
        end_line: usize,
        /// End column (0-based)
        #[arg(long)]
        end_column: usize,
        /// Replacement text
        text: String,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Show the node kind (type) at a position
    KindAt {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Line number (0-based)
        #[arg(long)]
        line: usize,
        /// Column number (0-based)
        #[arg(long)]
        column: usize,
    },
    /// Find all nodes of a specific kind
    FindKind {
        /// File
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Node kind to find (e.g. "function_item", "if_statement")
        kind: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Format source code (simple AST-based formatting)
    Format {
        /// File to format
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Dry-run: show result without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Show AST statistics about a file
    Stats {
        /// File to analyze
        file: String,
        /// Language override
        #[arg(long)]
        language: Option<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

/// Content-hash anchored editing (Hashline).
#[derive(Subcommand)]
pub enum EditAction {
    /// Compute the hashline anchor for a line range
    Hash {
        /// File to compute hash for
        file: String,
        /// Start line (1-based, inclusive)
        #[arg(long)]
        start: usize,
        /// End line (1-based, inclusive)
        #[arg(long)]
        end: usize,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Apply a hashline edit: replace lines with new content
    Apply {
        /// File to edit
        file: String,
        /// Start line (1-based, inclusive) of the target region
        #[arg(long)]
        start: usize,
        /// End line (1-based, inclusive) of the target region
        #[arg(long)]
        end: usize,
        /// Replacement content
        #[arg(long)]
        content: String,
        /// Write changes back to file (default: dry-run, prints diff-ish output)
        #[arg(long)]
        write: bool,
    },
    /// Validate that a hashline anchor still matches the current file content
    Validate {
        /// File to check
        file: String,
        /// SHA-256 hex hash to validate (or just an 8-char prefix for display)
        #[arg(long)]
        hash: String,
        /// Start line (1-based, inclusive)
        #[arg(long)]
        start: usize,
        /// End line (1-based, inclusive)
        #[arg(long)]
        end: usize,
    },
}

/// Detect and resolve merge conflicts.
#[derive(Subcommand)]
pub enum ConflictAction {
    /// Scan a file or directory for merge conflict markers
    Scan {
        /// File or directory path to scan
        path: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Resolve conflicts in a file
    Resolve {
        /// File to resolve
        file: String,
        /// Resolution strategy: ours, theirs, both-ours-theirs, both-theirs-ours
        #[arg(long, default_value = "theirs")]
        strategy: String,
        /// Write resolved content (default: dry-run, prints to stdout)
        #[arg(long)]
        write: bool,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Bulk-resolve conflicts across multiple files
    ResolveBulk {
        /// Files to resolve
        files: Vec<String>,
        /// Resolution strategy: ours, theirs, both-ours-theirs, both-theirs-ours
        #[arg(long, default_value = "theirs")]
        strategy: String,
    },
    /// Show detailed info about conflicts in a file
    Show {
        /// File path
        file: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum AcpAction {
    /// Show ACP dispatch server running status
    Status {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Start the ACP dispatch server
    Start,
    /// Stop the ACP dispatch server
    Stop,
    /// Show detailed ACP dispatch server information
    Show {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// List launchable ACP agents (daemon handlers plus agents.toml entries)
    Agents {
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Run one canonical ACP prompt turn against an agent program
    Run {
        /// Agent program (absolute path or PATH name)
        program: String,
        /// Prompt text to send
        #[arg(long)]
        prompt: String,
        /// Session working directory (default: current dir)
        #[arg(long)]
        cwd: Option<String>,
        /// Per-request timeout in seconds
        #[arg(long, default_value = "120")]
        timeout_secs: u64,
        /// Runtime backend: process, docker, podman, kubernetes
        #[arg(long)]
        runtime: Option<String>,
        /// Extra args for the agent program (after --)
        #[arg(last = true)]
        args: Vec<String>,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a tracked ACP run's status
    RunStatus {
        /// Run ID returned by dispatch
        #[arg(long)]
        run_id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

impl Cli {
    pub async fn execute(self) -> anyhow::Result<()> {
        let loom_dir = crate::loom_dir();
        let agents_path = PathBuf::from(&loom_dir);
        match self.command {
            Command::Init { import, recommended, source } => {
                crate::commands::init::run(import, recommended, source.as_deref(), &loom_dir).await
            }
            Command::Scan { json } => crate::commands::scan::run(&loom_dir, json).await,
            Command::Status { json } => crate::commands::status::run(&loom_dir, json).await,
            Command::Doctor { json, fix, verbose } => crate::commands::doctor::run(&loom_dir, json, fix, verbose).await,
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
            Command::Runtime { action } => crate::commands::runtime::run(&agents_path.to_string_lossy(), &action).await,
            Command::Rules { action } => crate::commands::rules::run(&agents_path, &action).await,
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
            Command::Lock { generate, verify, diff } => {
                crate::commands::lock::run(&loom_dir, generate, verify, diff)
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
            Command::Plugin { action } => {
                crate::commands::plugin::run(&agents_path, &action).await
            }
            Command::Remote { action } => {
                crate::commands::remote::run(&agents_path, &action).await
            }
            Command::Lsp { action } => {
                crate::commands::lsp::run(&action).await
            }
            Command::Debug { action } => {
                crate::commands::debug::run(&action).await
            }
            Command::Knowledge { action } => {
                crate::commands::knowledge::run(&loom_dir, &action).await
            }
            Command::Acp { action } => {
                crate::commands::acp::run(&agents_path, &action).await
            }
            Command::Edit { action } => {
                crate::commands::edit::run(&loom_dir, &action).await
            }
            Command::Ast { action } => {
                crate::commands::ast::run(&action).await
            }
            Command::Telemetry { action } => {
                crate::commands::telemetry::run(&loom_dir, &action).await
            }
            Command::Conflict { action } => {
                crate::commands::conflict::run(&loom_dir, &action).await
            }
            Command::Completions { shell } => {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                clap_complete::generate(shell, &mut cmd, "loom", &mut std::io::stdout());
                Ok(())
            }
            Command::SelfUpdate { force, source } => {
                crate::commands::self_update::run(force, &source).await
            }
        }
    }
}
