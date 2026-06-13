# Loom Comprehensive Improvement Roadmap

**Sources:** 
- loom-vs-emdash.md (Emdash comparison)
- kasetto-trellis-analysis.md (Kasetto/Trellis lineage analysis)
- loom-emdash-omp-comparison.md (Triple comparison with Oh-My-Pi)
- Internal codebase analysis

**Date:** 2026-06-07  
**Goal:** Increase feature score from 76% to 92%+

---

## 📊 Current State

| Category | Score | Target |
|----------|:-----:|:------:|
| Infrastructure | 100% | ✅ |
| Auth & Hooks | 100% | ✅ |
| Routing | 80% | +20% |
| Skill Management | 80% | +20% |
| Config | 100% | ✅ |
| Extensibility | 90% | +10% |
| Multi-Agent | 85% | +15% |
| Workflow | 80% | +20% |
| Memory | 85% | +15% |
| Developer Experience | 75% | +25% |
| Issue Tracking | 100% | ✅ |

---
## ✅ Completed Recently

### 0.3 Multi-Agent Parallel Execution

**Priority:** P0 (2-week task — ✅ DONE)

**What:** Created `loom-parallel` crate with `AgentPool` (bounded-concurrency agent executor) and `WorktreeManager` (git worktree isolation). Full CLI integration with `loom exec --parallel`.

**Implementation:**
- `crates/loom-parallel/` — `AgentPool` with semaphore-bounded concurrency, `WorktreeManager` for git worktree create/cleanup
- `loom exec --parallel --pool-size N` — runs all specified tools concurrently
- `loom exec --no-isolation` — skip git worktree creation
- Progress summary with timing and success/failure counts
- Sequential mode unchanged (existing behavior preserved)

**CLI Usage:**
```bash
loom exec --parallel --pool-size 3 goose crush jcode
loom exec --parallel --no-isolation -- opencode goose
```

---

### 0.2 GitHub Issues Integration

**Priority:** P0 (3-day task — ✅ DONE)

**What:** Created `loom-trackers` crate with generic `IssueTracker` trait + GitHub REST client. Full CLI integration with `loom issues list/show/create-branch`. Integrated with workflow dispatch via `--link-issue`.

**Implementation:**
- `crates/loom-trackers/` — `IssueTracker` trait, `GitHubTracker` backend, models, error types
- `loom issues list --repo owner/repo` — list issues with state/label filters
- `loom issues show <id>` — show issue details (number or owner/repo#number)
- `loom issues create-branch <id>` — create local git branch from issue
- `loom workflow dispatch <task-id> --link-issue owner/repo#123` — link issue to task, injected as `LOOM_LINKED_ISSUE` env var
- Token from `GITHUB_TOKEN` or `GH_TOKEN` env var, or `--token` flag

---

### 0.1 Memory MCP Integration (agentmemory)

**Priority:** P0 (3-day task — ✅ DONE)

**What:** Integrated [AgentMemory](https://agent-memory.dev) (19.6K ⭐) as a shared ecosystem MCP server. 53 MCP tools for persistent memory with 95.2% R@5 recall. Replaces the need for a custom hindsight memory system.

**Implementation:**
- `AgentMemoryManager` in `crates/loom-mcp/src/agentmemory.rs` — managed subprocess lifecycle for `npx @agentmemory/agentmemory`
- Seeded as default shared MCP server in `registry.rs` (`seed_defaults()`) — auto-registered for all tools
- Auto-start in `loomd` on daemon startup (graceful fallback if npx unavailable)
- IPC commands: `memory.status`, `memory.start`, `memory.stop`
- CLI: `loom memory status/start/stop`
- Health checks via TCP port 3111

**Impact:** Every tool gets persistent memory (recall, search, store) without per-tool configuration. Memory score in table below updated to reflect ecosystem-wide persistent memory.

---

## 🎯 Phase 1: Quick Wins (Week 1-2)

> **Low effort, high visibility improvements**

### 1.1 Shell Completions ⭐ FROM KASETTO

**Priority:** P0 (2-day task)

**Status:** ✅ DONE — `loom completions bash|zsh|fish|powershell|elvish` generates full CLI tab-completions. `loom init-shell` provides shell integration scripts (env vars, tool wrappers). Missing: README documentation.

**Action Items:**
- [x] Add shell integration script (`loom init-shell`) — generates bash/zsh/fish wrappers + env setup
- [x] Add `clap_complete` to `loom-cli/Cargo.toml`
- [x] Generate bash completions (`loom completions bash`)
- [x] Generate zsh completions (`loom completions zsh`)
- [x] Generate fish completions (`loom completions fish`)
- [x] Add `loom init-shell` command
- [x] Document in README

**Verification:**
```bash
loom completions bash > /etc/bash_completion.d/loom
source /etc/bash_completion.d/loom
loom <TAB>  # Should show commands
```

---

### 1.2 More CLI Provider Adapters ⭐ FROM EMDAHS

**Priority:** P0 (1-week task)

**Current:** 10 providers (aider, claude-code, codex, crush, cursor, gemini, goose, jcode, opencode)  
**Target:** 14+ providers

**New Adapters to Add:**

| Provider | Source | Priority |
|----------|--------|----------|
| Kiro/AWS | Emdash | P1 |
| Juno | Emdash | P1 |
| GitHub Copilot | Emdash | P2 |
| Jules | Emdash | P2 |

**Action Items:**
- [x] Create adapter skeleton for Claude Code
- [x] Implement Claude Code adapter (verify CLI, exec, auth)
- [x] Create adapter skeleton for Codex
- [x] Implement Codex adapter
- [x] Create adapter skeleton for Cursor
- [x] Implement Cursor adapter
- [x] Create adapter skeleton for Gemini CLI
- [x] Implement Gemini CLI adapter
- [x] Create adapter skeleton for Aider
- [x] Implement Aider adapter
- [x] Add `providers list-known` command (superseded by `loom mcp list-known`)
- [x] Add `providers install <name>` command (superseded by `loom mcp install`)
- [x] Update README with new providers

---

### 1.3 GitHub Issues Integration ⭐ FROM EMDAHS

**Priority:** P0 (3-day task — ✅ DONE, see §0.2)

**Action Items:**
- [x] Create `loom-trackers` crate skeleton
- [x] Implement GitHub Issues REST client
- [x] Add `issues list` command (reads from configured repo)
- [x] Add `issues show <id>` command
- [x] Add `issues create-branch <id>` command
- [x] Integrate with `loom workflow dispatch`
- [x] Document GitHub token setup

**CLI Usage:**
```bash
loom issues list --repo owner/repo --state open
loom issues show 123
loom workflow dispatch <task-id> --link-issue 123
```

---

### 1.4 Lock File for Reproducibility ⭐ FROM KASETTO

**Priority:** P1 (2-day task)

**Action Items:**
- [x] Create `LockFile` struct in `loom-skills`
- [x] Add `skills.lock` generation on `loom skills install`
- [x] Implement diff output on lock file changes (`loom lock --verify --diff`)
- [x] Add `--locked` flag to prevent updates (`loom skills list --locked`)
- [x] Add `loom lock` command to regenerate
- [x] Document lock file usage

---

## 🚀 Phase 2: Core Features (Week 3-6)

> **High impact, moderate effort**

### 2.1 Multi-Agent Parallel Execution ⭐ FROM EMDAHS

**Priority:** P0 (2-week task — ✅ DONE, see §0.3)

**Status:** ✅ Complete — `loom-parallel` crate with `AgentPool`, `WorktreeManager`, `loom exec --parallel --pool-size N --no-isolation`.


**Action Items:**
- [x] Create `loom-parallel` crate
- [x] Implement `AgentPool::execute_all(tasks)` — run multiple agents
- [x] Implement `AgentPool::execute_round_robin(tasks)` — sequential with context
- [x] Implement `WorktreeManager` for git worktree isolation
- [x] Add `exec --parallel` flag to CLI
- [x] Add `exec --pool-size N` option
- [x] Add progress reporting for parallel execution
- [x] Implement result aggregation
- [x] Add cleanup on completion/error

**CLI Usage:**
```bash
loom exec --parallel --pool-size 3 goose crush jcode
loom exec --parallel -- git-analyze --files $(git diff --name-only)
```

---

### 2.2 Issue Tracker Ecosystem ⭐ FROM EMDAHS

**Priority:** P0 (3-week task)

**Target Trackers:**
| Tracker | Priority | API Type |
|---------|----------|----------|
| GitHub Issues | P0 | REST |
| Linear | P1 | GraphQL |
| Jira | P2 | REST |
| GitLab | P2 | REST |
| Asana | P3 | REST |

**Action Items:**
- [x] Implement `IssueTracker` trait
- [x] Add GitHub Issues (complete)
- [x] Add Linear GraphQL client (implemented as `loom-trackers::linear::LinearTracker`)
- [x] Add Jira REST client (implemented as `loom-trackers::jira::JiraTracker` with `--provider jira --host`)
- [x] Add GitLab Issues client (implemented as `loom-trackers::gitlab::GitLabTracker` with `--provider gitlab --host`)
- [x] Add `issues` CLI subcommand group (as `loom issues`, not `loom trackers`)
- [x] Integrate with workflow creation
- [x] Auto-create branch from issue
- [x] Link PR to issue on completion

**Code Structure:**
```rust
pub trait IssueTracker {
    fn name(&self) -> &str;
    fn fetch_issue(&self, id: &str) -> Result<Issue>;
    fn list_issues(&self, query: &IssueQuery) -> Result<Vec<Issue>>;
    fn create_branch(&self, issue: &Issue) -> Result<String>;
    fn link_pr(&self, issue_id: &str, pr_url: &str) -> Result<()>;
}
```

---

### 2.3 Sub-Agent Orchestration ⭐ FROM TRELLIS

**Priority:** P1 (2-week task)

**Gap:** Loom has 5-phase workflow but no sub-agent execution. Trellis implements Plan/Implement/Verify/Finish sub-agents.

**Status:** 🟡 PARTIALLY ADDRESSED — Phase result chain (`PostPhase` hooks, auto-advance, session capture per phase) was added. Workflow `dispatch` passes task title + description as prompt. Sub-agent orchestration (multi-agent within a single phase) remains unimplemented.

**Proposed Design:**

```rust
// Extend loom-workflow
pub struct SubAgentConfig {
    pub phase: WorkflowPhase,
    pub agent: AgentType,
    pub prompt_template: String,
    pub max_retries: u8,
    pub fallback_agent: Option<AgentType>,
}

// New dispatch modes
pub enum DispatchMode {
    Single,                    // Current behavior
    SubAgents {                // Trellis-style
        plan: AgentType,
        implement: AgentType,
        verify: AgentType,
        finish: AgentType,
    },
    Parallel { max: usize },   // Emdash-style
}
```

**Action Items:**
- [x] Add `SubAgentConfig` to workflow phase (plan/implement/verify/finish agents, max_retries, fallback_agent)
- [x] Implement auto-prompt injection per phase (title + description → prompt, prior sessions as LOOM_PRIOR_SESSIONS)
- [x] Add `dispatch --mode sub-agents` option
- [x] Add result passing between phases (PhaseResult chain, capture_dispatch_session)
- [x] Add retry logic with fallback agents (per-agent retry count, fallback agent on exhaustion)
- [x] Add `dispatch --mode parallel` option (via `loom exec --parallel`)

---

### 2.4 SSH Remote Development ⭐ FROM EMDAHS

**Priority:** P1 (2-week task)

**Gap:** Emdash supports SSH + SFTP + remotePTY. Loom has no remote capabilities.

**Architecture:**

```
┌─────────────────────────────────────────────────────────────┐
│                    loom-remote (new crate)                  │
├─────────────────────────────────────────────────────────────┤
│  SshConnectionManager                                        │
│  ├── connections: HashMap<String, SshConnection>            │
│  └── default_auth: SshAuth                                   │
├─────────────────────────────────────────────────────────────┤
│  SshConnection                                               │
│  ├── host: String                                            │
│  ├── port: u16                                              │
│  ├── sftp: SftpSession                                      │
│  ├── pty: Option<PtySession>                                │
│  └── auth: SshAuth                                           │
└─────────────────────────────────────────────────────────────┘
```

**Action Items:**
- [x] Create `loom-remote` crate
- [x] Add SSH connection manager (SshConnection, SshAuth, SshConnectionManager)
- [x] Implement SFTP file sync (sftp::push/pull via rsync/scp)
- [x] Implement remote PTY execution (via ssh command)
- [x] Add `remote connections` CLI subcommand (`loom remote list/add/remove`)
- [x] Add `remote exec --host <name> <command>`
- [x] Add `remote sync --push` and `--pull`
- [x] Support SSH agent and key auth (Agent, KeyFile, KeyPem)
- [x] Store credentials in vault (re-use `loom-auth`) — `loom remote add --store-in-vault --vault-password` stores SSH keys in the encrypted credential vault; `remote exec`/`sync` auto-resolve vault keys

**CLI Usage:**
```bash
loom remote add my-server --host server.com --user dev
loom remote connections list
loom remote exec my-server "ls -la"
loom remote sync --push ./dist/
loom exec --remote my-server goose "analyze logs"
```

---

### 2.5 ACP Dispatch Implementation ⭐ FROM TRELLIS (Stubs → Real)

**Priority:** P1 (1-week task) — ✅ COMPLETE

**Status:** ✅ DONE — `AcpDispatchServer` crate (~665 lines) with Ed25519 handshake auth, JSON-line dispatch protocol, health endpoint. Daemon integration with auto-start in `Loomd::run()`, default handler for local agent binary dispatch. IPC handler commands (`acp.status`, `acp.start`, `acp.stop`). CLI commands (`loom acp status/start/stop/show`). Key persistence across restarts. Peer discovery (`AcpPeer`, `PeerDiscovery`) and `AcpClient` for ACP registry integration.

**Action Items:**
- [x] Define ACP protocol in `loom-core`
- [x] Implement `AcpClient` with HTTP/WebSocket
- [x] Implement `AcpServer` in `loomd` (acp.dispatch IPC command + `dispatch_acp_agent` with native/npm/pip runners)
- [x] Add `dispatch --acp <agent-id>` to CLI
- [ ] Add peer discovery
- [ ] Implement secure handshake
- [x] Add `--dry-run` for testing

---

## ⚡ Phase 3: Experience Enhancements (Week 7-10)

> **Medium impact, medium effort**

### 3.1 Embedded Pane Dispatch ⭐ FROM TRELLIS (Stubs → Real)

**Priority:** P1 (1-week task)

**Gap:** Known gotcha #10 — "EmbeddedPane dispatch — explicit 'not yet implemented'."

**Status:** 🟡 Embedded pane now has split-view layout (task context left 45%, output log right 55%), output log with color-coded lines, scrollable with j/k. Still missing: real-time streaming from process stdout, input/interrupt capability.

**Action Items:**
- [x] Implement embedded agent panel in TUI
- [x] Add dispatch --embedded option
- [x] Add split-view with task context
- [x] Add real-time output streaming (from agent process stdout via TUI embedded pane — spawns agent in background, drains stdout into scrollable log, auto-refreshes every 1s)
- [x] Add input/interrupt capability (Ctrl-C to kill, `i` to type input, Enter to send stdin to child process)

---

### 3.2 Cross-Session Memory ⭐ FROM TRELLIS

**Priority:** P2 (1-week task)

**Status:** 🟡 PARTIALLY ADDRESSED by agentmemory MCP integration (see §0.1). AgentMemory provides vector-based persistent memory with 53 MCP tools. Custom Hindsight-style operations (retain/recall/reflect) not yet exposed through Loom CLI but available via MCP tools.

**Gap:** Loom has session persistence but limited cross-session context.

**Action Items:**
- [x] Implement session indexing on completion (v0.3.5)
- [x] Add session search by pattern/task (v0.3.5)
- [x] Implement context injection from similar sessions (v0.3.5)
- [x] Add `sessions recall <query>` command (v0.3.7) — grouped session-level recall on top of message-level search
- [x] Add session tagging/categorization (v0.3.8) — `tags` field on `SessionManifest`, `CrossSessionMemory` `session_tags` table, `loom sessions tag <id> --add/--remove`, `loom sessions tags` (facets), `--tag` filter on `list`/`search`/`recall`, automatic workflow-context tags on dispatch captures
- [x] Implement auto-summarization (v0.3.9) — `summarized_at` + `summarized_message_count` fields on `SessionManifest`, extractive summarizer in `loom-sessions` `compression` module (turn-merged role-prefixed excerpts, 200-char cap per dropped message), `SessionStore::compress_session`, auto-trigger on `append_message`/`replace_message` once message count exceeds `SESSION_COMPRESSION_THRESHOLD` (20), last `SESSION_FULL_FIDELITY_COUNT` (10) messages kept verbatim, `loom sessions summarize <id> [--force]` CLI, `loom sessions list` summary banner, resume-time summary disclosure, IPC `sessions.summarize` command with re-index into the cross-session memory store

---

### 3.3 Config Inheritance (extends:) ⭐ FROM KASETTO

**Priority:** P2 (3-day task)

**Action Items:**
- [x] Add `extends` field to TOML config
- [x] Implement deep merge (not replace)
- [x] Add `--no-extend` override (`--no-extend` on `loom config show` /
      `loom config validate`, `LOOM_NO_EXTEND=1` env var)
- [x] Document precedence rules (5-level: defaults < extends < local <
      overlays < remote; see `AGENTS.md` "Config Precedence" section)

**Example:**
```toml
# ~/.loom/config.base.toml
[skills]
registry_url = "https://registry.loom.sh"
auto_update = true

# ~/.loom/config.toml
extends = "~/.loom/config.base.toml"

[routing]
default_tier = "slm"
```

```bash
# View the resolved source chain
loom config show --extends
# → prints each layer (defaults, extends, local, overlays, remote) in
#   precedence order with the keys each layer contributed.

# Skip the extends chain for a single command
loom config show --extends --no-extend
LOOM_NO_EXTEND=1 loom routing evaluate "..."  # same effect, process-wide
```

---

### 3.4 Remote Config URL Support ⭐ FROM KASETTO

**Priority:** P2 (2-day task)

**Action Items:**
- [x] Add `remote_config_url` field to config
- [x] Implement HTTPS fetch with caching (`loom config fetch` writes to
      `~/.loom/.cache/remote_config/{sha256}.toml` + `index.json`)
- [x] Cached remote is **deep-merged** on top of the local+overlays tree
      (see "Config Precedence" in `AGENTS.md`)
- [x] Add `--source <url>` to `loom init` (rejects non-HTTPS schemes)
- [x] Document security considerations (HTTPS-only, cache key is
      sha256(url), no auto-refresh — run `loom config fetch` to update)
- [x] SHA-256 integrity check on URL key (path in cache is
      `~/.loom/.cache/remote_config/{sha256(url)}.toml`)

---

### 3.5 Plugin Registry ⭐ FROM KASETTO

**Priority:** P2 (1-week task)

**Action Items:**
- [x] Create `loom-registry` crate (reuses `loom-core::plugin_registry`)
- [x] Define plugin manifest schema
- [x] Implement registry index at registry.loom.sh
- [x] Add `loom plugin search <query>`
- [x] Add `loom plugin install <name>`
- [x] Add `loom plugin update <name>`
- [x] Add signature verification (`loom plugin install --verify`)

---

### 3.6 Session Branching ⭐ NEW

**Priority:** P3 (3-day task)

**Status:** ✅ DONE — `loom sessions branch <id> -f <from> -c <checkpoint>`. Creates forked session at given checkpoint (message line number) or full copy if no checkpoint. New session inherits source metadata with fresh ID. Remaining: merge capability, doc.

**Action Items:**
- [x] Add session checkpoint creation
- [x] Implement session fork
- [x] Add `sessions branch <id> --from <checkpoint>`
- [x] Add merge capability (`loom sessions merge <dest> <source>`)
- [x] Document use case: experiment branches (see `loom sessions branch --help`)

---

## 🔧 Phase 4: Polish (Week 11-12)

> **Lower impact, but important for DX**

### 4.1 Auto-Update System ⭐ FROM KASETTO

**Priority:** P2 (1-week task)

**Action Items:**
- [x] Add update check on startup (24h cache, `--no-update` flag)
- [x] Implement SHA verification (downloaded binary checksum)
- [x] Add `loom self update` command
- [x] Add `--no-update` flag (global, suppresses startup check)
- [x] Support self-update from releases.loom.sh (`--source releases.loom.sh`)

---

### 4.2 Visual PR Review (TUI Extension) ⭐ FROM EMDAHS

**Priority:** P3 (1-week task)

**Status:** 🟡 PR tab added (tab index 8, key `9` to jump). Shows PR list with state indicators, selection, detail panel with description. Missing: diff viewer, inline commenting, CI status display.

**Action Items:**
- [x] Add PR tab to TUI
- [ ] Implement diff viewer (side-by-side, uses `loom issues list` + detail)
- [ ] Add inline commenting
- [ ] Integrate with GitHub API
- [ ] Add CI status display

---

### 4.3 Config Doctor Improvements ⭐ FROM KASETTO

**Priority:** P3 (2-day task)

**Action Items:**
- [x] Add interactive fix mode (`loom doctor --fix`)
- [x] Add `--fix` flag to auto-correct
- [x] Add configuration validation on save (`loom config validate`)
- [x] Add verbose mode with explanations (`loom doctor --verbose`)

---

### 4.4 IDE Extensions ⭐ NEW

**Priority:** P3 (out of scope for core team)

**Considerations:**
- VS Code extension
- Neovim plugin (using `nvim-tree-sitter` or LSP)

**Note:** Could be community-driven after core stabilizes.

---

## 🆕 Phase 5: Oh-My-Pi Precision Features (Week 13-16)

> **Code mutation precision, debugging, and token optimization**

Oh-My-Pi's ~27k lines of Rust core provide capabilities that Loom lacks. These are high-value additions that would make Loom the most capable platform.

### 5.1 LSP Integration 🔴 HIGH PRIORITY

**Gap:** Oh-My-Pi has 13 LSP operations. Loom has zero. This means omp can do proper renames, refactors, and code actions.

**Priority:** P0 (2-week task)

**Architecture:**
```rust
// New crate: loom-lsp
pub struct LspClient {
    transport: LspTransport,
    capabilities: ServerCapabilities,
}

impl LspClient {
    // Rename symbol across project
    pub async fn rename(&self, params: RenameParams) -> Result<WorkspaceEdit>;
    
    // Find references
    pub async fn find_references(&self, params: ReferenceParams) -> Result<Vec<Location>>;
    
    // Execute code action
    pub async fn code_action(&self, params: CodeActionParams) -> Result<Vec<Command>>;
    
    // Go to definition
    pub async fn goto_definition(&self, params: TextDocumentPositionParams) -> Result<Location>;
    
    // Document symbols
    pub async fn document_symbols(&self, params: DocumentSymbolParams) -> Result<Vec<SymbolInformation>>;
    
    // Formatting
    pub async fn format_document(&self, params: DocumentFormattingParams) -> Result<Vec<TextEdit>>;
}
```

**Action Items:**
- [x] Create `loom-lsp` crate
- [x] Implement LSP transport (stdio, Content-Length framing, JSON-RPC)
- [x] Add server discovery (rust-analyzer, pyright, pylsp, typescript-language-server, gopls)
- [x] Implement `rename` operation
- [x] Implement `find_references`
- [x] Implement `code_action`
- [x] Implement `goto_definition`
- [x] Implement `document_symbols`
- [x] Add `lsp` subcommand group
- [x] Add `lsp rename <old> <new>` CLI command
- [x] Integrate with skill adapters

**CLI Usage:**
```bash
loom lsp servers list
loom lsp rename formatBytes format_file_size
loom lsp find-references --path src/main.rs --symbol MyStruct
```

---

### 5.2 DAP Debugging 🔴 HIGH PRIORITY

**Gap:** Oh-My-Pi has 27 DAP operations (lldb, dlv, debugpy). Loom has no debugging.

**Priority:** P0 (2-week task)

**Supported Debuggers:**
| Debugger | Languages | Priority |
|----------|----------|----------|
| lldb | C, C++, Rust, Swift | P0 |
| dlv | Go | P1 |
| debugpy | Python | P1 |
| vscode-node-debug2 | JavaScript, TypeScript | P2 |

**Architecture:**
```rust
// New crate: loom-debug
pub struct DebugSession {
    adapter: Box<dyn DebugAdapter>,
    breakpoints: HashMap<Source, Vec<Breakpoint>>,
    threads: Vec<Thread>,
    stack_frames: Vec<StackFrame>,
}

pub trait DebugAdapter {
    async fn initialize(&mut self) -> Result<Capabilities>;
    async fn launch(&mut self, config: &LaunchConfig) -> Result<()>;
    async fn attach(&mut self, pid: u32) -> Result<()>;
    async fn set_breakpoints(&mut self, source: &str, lines: Vec<u32>) -> Result<Vec<Breakpoint>>;
    async fn step_over(&mut self) -> Result<()>;
    async fn step_in(&mut self) -> Result<()>;
    async fn step_out(&mut self) -> Result<()>;
    async fn pause(&mut self) -> Result<()>;
    async fn resume(&mut self) -> Result<()>;
    async fn evaluate(&mut self, expr: &str) -> Result<String>;
    async fn stack_trace(&mut self) -> Result<Vec<StackFrame>>;
    async fn variables(&mut self, frame_id: u64) -> Result<Vec<Variable>>;
}
```

**Action Items:**
- [x] Create `loom-debug` crate
- [x] Implement Debug Adapter Protocol (DAP) client
- [x] Implement lldb adapter
- [x] Implement dlv adapter (Go)
- [x] Implement debugpy adapter (Python)
- [x] Add `debug` subcommand group
- [x] Add `debug attach <pid>` command
- [x] Add `debug breakpoints set <file>:<line>` command
- [ ] Add `debug step [over|in|out]` commands
- [x] Add `debug eval <expression>` command
- [x] Add `debug stack` command
- [x] Integrate with skill adapters (run test → debug on failure)

**CLI Usage:**
```bash
loom debug attach 12345
loom debug breakpoints set src/main.rs:42
loom debug step over
loom debug eval x->data
loom debug stack
```

---

### 5.3 Hashline Edit System 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi uses content-hash anchored edits (61% fewer tokens). Loom uses line-based.

**Priority:** P1 (1-week task)

**Status:** ✅ DONE — `HashlineAnchor`, `HashlineEdit`, `InsertionPoint` structs in `loom-skills::edit` (~497 lines). Content-hash generation, validation, and fallback to line-range for stale files. CLI via `loom edit hash/apply/validate` commands in `loom-cli`.

**Action Items:**
- [x] Add `HashlineAnchor` struct to `loom-skills`
- [x] Implement content-hash generation
- [x] Implement anchor validation before edit
- [x] Implement fallback to line-range for stale files
- [x] Add `edit --hashline` flag
- [ ] Integrate with skill adapter execution
- [x] Add stale detection and warning

---

### 5.4 Hindsight Memory System 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi's Hindsight = cross-session memory with retain/recall/reflect. Loom has session persistence but no cross-session context building.

**Priority:** P1 (1-week task) — ✅ COMPLETE

**Status:** ✅ DONE — `loom-hindsight` crate (~484 lines) with typed fact storage, semantic search, retain/recall/reflect operations. Full CLI integration via `loom memory retain/recall/reflect` commands. Integrated with cross-session memory store.

**CLI Usage:**
```bash
loom memory retain "This module handles OAuth2 authentication flow"
loom memory recall "how does auth work in this codebase?"
loom memory reflect "summarize the architecture of this project"
```

---

### 5.5 AST Structural Editing 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi has ast_edit with preview. Loom has simple file replacement.

**Priority:** P1 (1-week task)

**Action Items:**
- [ ] Add `ast_edit` module using tree-sitter
- [ ] Implement pattern matching (ast-grep)
- [ ] Implement structural rewrite
- [ ] Add preview mode before apply
- [ ] Add `ast-edit` command to CLI
- [ ] Add `--dry-run` flag

---

### 5.6 Time-Traveling Stream Rules 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi injects rules mid-stream when regex matches. Loom's hooks are event-based.

**Priority:** P2 (3-day task) — ✅ DONE

**Status:** ✅ DONE — `loom-rules` crate (~1166 lines) with `StreamRule` engine, regex-based injection, condition matching, retry logic. Full CLI via `loom rules list/show/add/remove/eval` commands. IPC handler integration.

**Action Items:**
- [x] Add `StreamRule` type
- [x] Implement regex-based injection
- [x] Add retry from injection point
- [x] Add `rules inject --pattern <regex> --rule <content>`
- [x] Document use case: prevent known bad patterns

---

### 5.7 Conflict Resolution Protocol 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi handles merge conflicts via `conflict://N` scheme.

**Priority:** P2 (2-day task)

**Status:** ✅ DONE — `loom-conflict` crate (~526 lines) with merge conflict detection, @theirs/@ours/@base resolution markers, `ConflictResolver` type. CLI via `loom conflict scan/resolve/resolve-bulk/show` commands in `loom-cli`.

**Action Items:**
- [x] Detect merge conflicts
- [x] Implement @theirs/@ours/@base resolution
- [x] Add `conflict resolve --theirs` commands
- [x] Add `conflict://*` bulk resolution

---

### 5.8 ACP (Agent Communication Protocol) 🟡 MEDIUM PRIORITY

**Gap:** Oh-My-Pi has ACP for editor integration. Loom's ACP is stub-only.

**Priority:** P1 (see Phase 2, Section 2.5) — ✅ COMPLETE (via server crate + daemon + CLI)

---

## 📋 Consolidated Action Items

### P0 (Critical — Ship in Week 1-2)

- [x] Shell completions (bash, zsh, fish)
- [x] Claude Code adapter
- [x] Codex adapter
- [x] GitHub Issues integration
- [x] Lock file generation
- [x] README documentation

### P1 (Important — Ship in Week 3-6)

- [x] Multi-agent parallel execution
- [x] Agent pool with worktree isolation
- [x] Linear tracker integration (implemented as `loom-trackers::linear::LinearTracker` with GraphQL API)
- [x] SSH remote connections (implemented as `loom-remote` crate with full CLI)
- [x] Sub-agent orchestration (Trellis-style) — SubAgentConfig, retry, fallback, auto-advance
- [x] ACP dispatch implementation (server crate + daemon integration + CLI)
- [x] Embedded pane dispatch
- [x] Cursor adapter
- [x] Kiro/AWS adapter
- [x] **LSP integration** (NEW — loom-lsp crate with 6 ops, CLI, server discovery)
- [x] **DAP debugging** (NEW — loom-debug crate + CLI with Adapters/Launch/Attach/Breakpoints/Step/Evaluate/Stack)
- [x] **Hindsight memory** (NEW — loom-hindsight crate + CLI via `loom memory retain/recall/reflect`)
- [x] **AST structural editing** (NEW — loom-ast crate, tree-sitter structural operations, 29 tests)
- [x] **Hashline edits** (NEW — core in loom-skills::edit, CLI pending)

### P2 (Enhancement — Ship in Week 9-12)

- [x] Cross-session memory
- [x] Config inheritance
- [x] Remote config URL
- [x] Plugin registry
- [x] Session branching
- [x] Auto-update system (`loom self-update`, startup check, `--no-update`)
- [ ] Juno adapter
- [ ] GitHub Copilot adapter
- [x] Jira tracker integration (implemented as `loom-trackers::jira::JiraTracker`)
- [x] **Stream rules** (NEW — loom-rules crate, CLI via `loom rules`)
- [x] **Conflict resolution** (NEW — loom-conflict crate, CLI via `loom conflict`)
- [x] **Hashline Edit System** (NEW — loom-skills::edit, CLI via `loom edit`)

### P3 (Polish — Ship in Week 11-12)

- [x] Visual PR review in TUI (tab 9, list + detail panel)
- [x] Config doctor improvements (`--verbose`, `--fix`, explanations)
- [ ] IDE extensions (community)

---

## 📈 Impact Projection

| Phase | Features Added | Score Improvement | New Score |
|-------|----------------|-------------------|-----------|
| Phase 1 | Shell completions, 2 adapters, GitHub, lockfile | +8% | 79% |
| Phase 2 | Trackers, SSH, sub-agents, ACP server | +13% | 92% |
| Phase 3 | Auto-update, Juno, Copilot, Jira | +5% | 97% |
| Phase 4 | Visual, polish | +3% | 100% |
| Phase 5 | LSP, DAP, Hashline, Hindsight, AST, Rules, Conflicts | +15% | 115%* |

*Score can exceed 100% as Phase 5 adds capabilities beyond inspiration projects

**Phase 5 Impact:** After Phase 5, Loom would have:
- Debugging (currently 0% → 33%)
- Code precision (currently 20% → 80%)
- Memory system (currently 60% → 90%)
- Token efficiency (currently 30% → 70%)

**Target: Phase 2 completion = 92% feature parity with inspiration projects**

**Current Status (post Phase 5):** All P0 and P1 items complete. Remaining gaps (P2):
- Juno and GitHub Copilot adapters

---

## 🔗 Cross-References

### From loom-vs-emdash.md

| Emdash Feature | Loom Equivalent | Status |
|----------------|-----------------|--------|
| 27 CLI providers | 5 adapters | In Progress (Phase 1) |
| Parallel execution | None | Phase 2 |
| SSH remote dev | None | Phase 2 |
| Issue trackers | None | Phase 1-2 |
| Visual PR review | None | Phase 4 |
| Git worktree isolation | None | Phase 2 |
| GitHub/Jira/Linear | None | Phase 1-2 |

### From kasetto-trellis-analysis.md

| Kasetto/Trellis Feature | Loom Equivalent | Status |
|-------------------------|-----------------|--------|
| Shell completions | None | Phase 1 |
| Auto-update | None | Phase 3 |
| Sub-agent orchestration | 5-phase workflow | Phase 2 |
| Cross-session memory | Session persistence | Phase 3 |
| ACP dispatch | Stub only | Phase 2 |
| Embedded pane | "Not implemented" | Phase 2 |
| Config inheritance | Overlay only | Phase 3 |
| Remote config URL | None | Phase 3 |
| Lock file | None | Phase 1 |
| Plugin registry | TOML-driven generic | Phase 3 |

### From loom-emdash-omp-comparison.md (Oh-My-Pi)

| Oh-My-Pi Feature | Loom Equivalent | Status |
|-------------------|-----------------|--------|
| LSP integration (13 ops) | None | Phase 5 |
| DAP debugging (27 ops) | None | Phase 5 |
| Hashline edits (61% tokens) | loom edit | Phase 5 ✅ |
| Hindsight memory | Session compression | Phase 5 |
| AST structural editing | Simple replace | Phase 5 |
| Time-traveling rules | Event hooks | Phase 5 |
| Conflict resolution | loom conflict | Phase 5 ✅ |
| Preview mode (resolve) | Direct apply | Phase 5 |
| Browser automation | None | Future |
| web_search (14 providers) | None | Future |

---

## 🗺️ Milestone Timeline

```
Week 1-2:  ┌─────────────────────────┐
           │ Phase 1 Quick Wins      │
           │ • Shell completions     │
           │ • Claude Code adapter  │
           │ • GitHub Issues        │
           └─────────────────────────┘

Week 3-4:  ┌─────────────────────────┐
           │ Phase 2a Core           │
           │ • Multi-agent pool      │
           │ • Linear tracker        │
           │ • Codex adapter         │
           └─────────────────────────┘

Week 5-6:  ┌─────────────────────────┐
           │ Phase 2b Remote          │
           │ • SSH connections       │
           │ • Sub-agent orchestr.   │
           │ • ACP dispatch          │
           └─────────────────────────┘

Week 7-10: ┌─────────────────────────┐
           │ Phase 3 Enhancements     │
           │ • Config inheritance    │
           │ • Cross-session memory  │
           │ • Plugin registry       │
           └─────────────────────────┘

Week 11-12:┌─────────────────────────┐
           │ Phase 4 Polish          │
           │ • Auto-update          │
           │ • Visual PR review     │
           │ • Config doctor        │
           └─────────────────────────┘

Week 13-16:┌─────────────────────────┐
           │ Phase 5 Precision        │
           │ • LSP integration      │
           │ • DAP debugging        │
           │ • Hashline/Hindsight   │
           │ • AST editing          │
           └─────────────────────────┘
```

---

## Appendix: Known Gotchas Addressed

| # | Gotcha | Resolution | Phase |
|---|--------|------------|-------|
| #9 | ACP dispatch not wired | Implement AcpClient/Server | Phase 2 |
| #10 | EmbeddedPane not implemented | Implement embedded panel | Phase 2 |
| #5 | Vault lockout | Keep as-is (security feature) | — |
| #7 | TUI keybindings | Document, don't change | — |
| #8 | Adapter binaries PATH check | Keep as-is | — |

---

## Appendix: New Crates Required

| Crate | Purpose | Phase |
|-------|---------|-------|
| `loom-lsp` | LSP client for rename/refactor/code actions | Phase 5 |
| `loom-debug` | DAP client for lldb/dlv/debugpy | Phase 5 |
| `loom-parallel` | Agent pool & worktree isolation | Phase 2 |
| `loom-trackers` | Issue tracker clients (GitHub, Linear, Jira) | Phase 1-2 |
| `loom-remote` | SSH/SFTP connection manager | Phase 2 |
| `loom-hindsight` | Cross-session memory system | Phase 5 |

---

## Appendix: Dependency Hierarchy

```
loom-core
    │
    ├── loom-lsp (NEW) ──────────► loom-skills
    ├── loom-debug (NEW) ─────────► loom-workflow
    ├── loom-parallel (NEW) ───────► loom-adapters, loom-workflow
    ├── loom-trackers (NEW) ───────► loom-workflow, loom-auth
    ├── loom-remote (NEW) ─────────► loom-auth (vault)
    └── loom-hindsight (NEW) ──────► loom-sessions
```

---

*Roadmap generated from loom-vs-emdash.md, kasetto-trellis-analysis.md, and loom-emdash-omp-comparison.md*