# Loom vs Emdash vs Oh-My-Pi: Triple Comparison

**Document Version:** 1.1.0  
**Date:** 2026-05-22  
**Classification:** Technical Analysis

---

## Executive Summary

This document provides a three-way comparison between **Loom**, **Emdash**, and **Oh-My-Pi (omp)**. Each occupies a distinct niche in the agentic development ecosystem, forming a complementary "Macro-Micro" stack when combined.

| Dimension | Loom | Emdash | Oh-My-Pi |
|-----------|------|--------|----------|
| **Language** | Rust | TypeScript/Electron | TypeScript + Rust |
| **Architecture** | Modular daemon + IPC | Electron monolith | TUI + Rust core |
| **Stars** | N/A | 4,569 | 5,977 |
| **License** | MIT | Apache 2.0 | Apache 2.0 |
| **Role** | Platform/Infra | Macro-Orchestrator | Micro-Execution Engine |

### The Macro-Micro Stack

```
┌─────────────────────────────────────────────────────────────────┐
│                    MACRO LEVEL (Emdash)                          │
├─────────────────────────────────────────────────────────────────┤
│  • Issue trackers (Linear, Jira, GitHub, Asana)                 │
│  • Git worktree isolation & environment replication              │
│  • Visual workspace (diff review, PR workflow, CI status)        │
│  • SSH remote development                                       │
│  • Desktop app (Electron + React)                               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    MICRO LEVEL (Oh-My-Pi)                        │
├─────────────────────────────────────────────────────────────────┤
│  • LSP-wired edits (rename, refactor, code actions)              │
│  • DAP debugging (lldb, dlv, debugpy)                           │
│  • Hashline token compression (61% reduction)                    │
│  • Hindsight cross-session memory                               │
│  • Subagent fan-out with typed results                          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    PLATFORM LEVEL (Loom)                         │
├─────────────────────────────────────────────────────────────────┤
│  • MCP server registry & management                              │
│  • Skills system with hot-reload                                 │
│  • Hook automation & scripting                                  │
│  • Credential vault with encryption                             │
│  • Model routing & cost optimization                            │
│  • Event bus (in-process + cross-process)                       │
│  • Session compression & persistence                             │
└─────────────────────────────────────────────────────────────────┘
```

---

## 1. Feature Comparison Matrix

### 1.1 Core Capabilities

| Feature | Loom | Emdash | Oh-My-Pi | Notes |
|---------|:----:|:------:|:--------:|-------|
| **Terminal UI** | TUI (ratatui) | GUI (Electron) | TUI | Loom = CLI, Emdash = Desktop |
| **Multi-Agent** | ❌ | ✅ (27 parallel) | ✅ (subagents) | omp = typed subagents |
| **Git Worktree Isolation** | ❌ | ✅ | ✅ (pi-iso) | omp calls it "workspace isolation" |
| **SSH Remote Dev** | ❌ | ✅ | ❌ | Emdash's standout feature |
| **MCP Server Support** | ✅ | ❌ | ❌ | Loom's core extensibility |
| **Skills System** | ✅ | ❌ | ✅ (skills://) | omp has skill import |
| **Model Routing** | ✅ | ❌ | ✅ (roles) | omp = default/smol/slow/plan |
| **Hook System** | ✅ | ❌ | ✅ (stream rules) | omp = time-traveling rules |
| **Session Management** | ✅ | ❌ | ✅ (Hindsight) | Different approaches |
| **Encrypted Vault** | ✅ | ❌ | ❌ | Loom's security advantage |
| **Issue Tracker Integration** | ❌ | ✅ | ❌ | Major gap for Loom |
| **Provider Count** | 5 | 27 | 40+ | |
| **Tool Count** | 20+ | N/A | 32 | |
| **LSP Integration** | ❌ | ❌ | ✅ (13 ops) | omp's precision advantage |
| **DAP Debugging** | ❌ | ❌ | ✅ (27 ops) | omp's runtime insight |
| **Hashline Edits** | ❌ | ❌ | ✅ | 61% token reduction |
| **AST Editing** | ❌ | ❌ | ✅ | Structural rewrites |
| **Browser Automation** | ❌ | ❌ | ✅ | Stealth Puppeteer |
| **Auto-Update** | ❌ | ✅ | ❌ | Emdash has it |

### 1.2 Technical Capabilities

| Feature | Loom | Emdash | Oh-My-Pi |
|---------|:----:|:------:|:--------:|
| **Rust Core** | ✅ | ❌ | ✅ (~27k LoC) |
| **Native Binaries** | ✅ | ❌ (Electron) | ✅ |
| **Plugin Architecture** | ✅ (TOML) | ❌ | ✅ (YAML) |
| **Daemon Mode** | ✅ | ❌ | ❌ |
| **IPC Protocol** | ✅ (Unix socket) | ❌ | ✅ (ACP stdio) |
| **Event Bus** | ✅ (tokio + ZMQ) | ❌ | ❌ |
| **Health Monitoring** | ✅ | ❌ | ⚠️ (basic) |
| **Telemetry** | ✅ | ⚠️ (PostHog opt-out) | ❌ |

---

## 2. Tool & Provider Ecosystem

### 2.1 Provider Support

```
Emdash (27): Amp, Auggie, Autohand, Charm, Claude Code, Cline, Codebuff,
             Codex, Continue, Cursor, Devin, Droid, Gemini, GitHub Copilot,
             Goose, Hermes, Jules, Junie, Kilocode, Kimi, Kiro, Letta,
             Mistral Vibe, OpenCode, Pi, Qwen Code, Rovo Dev

Oh-My-Pi (40+): Anthropic, OpenAI, OpenAI Codex, Google Gemini, Google,
                xAI, Mistral, Groq, Cerebras, Fireworks, Together, HF,
                NVIDIA, OpenRouter, Cursor, Copilot, GitLab Duo, Kimi,
                Moonshot, MiniMax, Qwen, Z.AI, Xiaomi, NanoGPT, Venice,
                Kilo, ZenMux, Ollama, LM Studio, llama.cpp, vLLM, + more

Loom (5): jcode, crush, opencode, goose, claude-code (via adapters)
          + MCP servers (via loom-mcp)
```

### 2.2 Tool Comparison (Oh-My-Pi's 32 Tools)

| Category | Tools | Loom Equivalent |
|----------|-------|-----------------|
| **Files** | read, write, edit, ast_edit, ast_grep | loom-skills |
| **Search** | search, find | loom-discovery |
| **Runtime** | bash, eval (Python/JS), recipe, ssh | loom-adapters |
| **Code Intel** | lsp (13 ops), debug (27 ops) | ❌ (gap) |
| **Coordination** | task, irc, todo_write, job, ask | loom-workflow |
| **Web** | browser, web_search (14 providers) | ❌ |
| **GitHub** | gh CLI wrapper | loom-sessions |
| **Memory** | checkpoint, rewind, retain, recall, reflect | loom-sessions (Hindsight) |
| **Internal** | pr://, issue://, agent://, skill://, conflict:// | loom-ipc |

---

## 3. Architecture Deep Dive

### 3.1 Loom: Platform Architecture

```
loom-cli (20+ commands)
         │
         ▼
loom-ipc (Unix socket JSON-line)
         │
         ├─────────────────────────────────────────┐
         │                                         │
         ▼                                         ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ loom-event-bus  │  │    loom-mcp    │  │    loom-auth    │
│ (tokio + ZMQ)   │  │    (pool)      │  │    (vault)      │
└─────────────────┘  └─────────────────┘  └─────────────────┘
         │                                         │
         ▼                                         ▼
┌─────────────────┐                      ┌─────────────────┐
│   loom-skills   │                      │ loom-sessions   │
│ (hot-reload)    │                      │ (compression)   │
└─────────────────┘                      └─────────────────┘
         │                                         │
         ▼                                         ▼
┌─────────────────┐                      ┌─────────────────┐
│ loom-workflow   │                      │  loom-routing   │
│ (5-phase task)  │                      │ (complexity)     │
└─────────────────┘                      └─────────────────┘
```

**Strengths:**
- Rust performance
- Daemon resilience
- MCP ecosystem
- Encrypted security

### 3.2 Emdash: Desktop Orchestrator

```
┌──────────────────────────────────────────────────────────────────┐
│                         Electron Renderer                        │
│  React 19 + Tailwind + Monaco + XTerm + Mermaid + React Query   │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│                         Electron Main                            │
│  SQLite | SSH2 (remote) | node-pty (terminal) | Worktree mgmt  │
└──────────────────────────────────────────────────────────────────┘
```

**Strengths:**
- Visual workflow
- Issue tracker integration
- SSH remote
- Git worktree management

### 3.3 Oh-My-Pi: Execution Engine

```
┌──────────────────────────────────────────────────────────────────┐
│                            omp TUI                               │
│  ratatui + crossterm + keyboard handling + tool cards           │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│                    TypeScript SDK / RPC                          │
│  createAgentSession, ModelRegistry, SessionManager              │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│                    Rust Core (~27k LoC)                          │
├──────────────────────────────────────────────────────────────────┤
│  shell (brush) │ grep │ lsp │ debug (DAP) │ ast │ fs_cache      │
│  highlight │ pty │ glob │ workspace │ iso │ prof │ ps │ tokens │
└──────────────────────────────────────────────────────────────────┘
```

**Strengths:**
- LSP precision (rename, refactor, code actions)
- DAP debugging (lldb, dlv, debugpy)
- Hashline token compression
- Hindsight memory
- Subagent typed results

---

## 4. Memory & Session Models

### 4.1 Comparison

| Aspect | Loom | Emdash | Oh-My-Pi |
|--------|------|--------|----------|
| **Session Persistence** | ✅ SQLite | ❌ | ✅ Hindsight banks |
| **Compression** | ✅ (after 20 msgs) | ❌ | ❌ |
| **Cross-Session Context** | ⚠️ | ❌ | ✅ |
| **Progressive Summarization** | ❌ | ❌ | ✅ (checkpoint/rewind) |
| **Typed Results** | ❌ | ❌ | ✅ (subagent output) |
| **Schema Validation** | ❌ | ❌ | ✅ |
| **Conflict Resolution** | ❌ | ❌ | ✅ (conflict://N) |

### 4.2 Hindsight vs Session Compression

**Loom's Approach:**
```rust
// Session compression after threshold
COMPRESSION_THRESHOLD = 20  // messages
FULL_FIDELITY_COUNT = 10    // uncompressed first N
```

**Oh-My-Pi's Hindsight:**
```typescript
// Mid-run memory retention
retain("fact: this module handles auth")  // queue durable facts

// Cross-session memory
recall("how does auth work here?")        // search Hindsight bank

// Synthesis
reflect("summarize the auth flow")         // ask Hindsight to answer
```

---

## 5. Code Mutation Precision

### 5.1 Edit Quality Comparison

| Aspect | Loom | Emdash | Oh-My-Pi |
|--------|------|--------|----------|
| **Standard Edit** | ✅ | ✅ | ✅ |
| **Hashline Anchor** | ❌ | ❌ | ✅ (61% fewer tokens) |
| **LSP Rename** | ❌ | ❌ | ✅ (full refactor) |
| **AST Edit** | ❌ | ❌ | ✅ (preview before apply) |
| **Stale Detection** | ❌ | ❌ | ✅ |
| **Preview Mode** | ❌ | ❌ | ✅ (resolve/accept) |

### 5.2 Token Efficiency

**Oh-My-Pi's Hashline System:**
```
Traditional: "Edit lines 45-67 to add error handling"
Hashline:    "Edit at hash[a3f2b1] add error handling"

Result: 61% token reduction on edits
```

**Loom's Current Approach:**
```rust
// Standard line-based edit
pub async fn edit_file(&self, path: &Path, from: &str, to: &str) -> Result<()>;
```

---

## 6. Debugging Capabilities

| Aspect | Loom | Emdash | Oh-My-Pi |
|--------|------|--------|----------|
| **Attach to Process** | ❌ | ❌ | ✅ |
| **Breakpoints** | ❌ | ❌ | ✅ |
| **Step Debugging** | ❌ | ❌ | ✅ |
| **Stack Frames** | ❌ | ❌ | ✅ |
| **Variable Inspection** | ❌ | ❌ | ✅ |
| **lldb Support** | ❌ | ❌ | ✅ |
| **dlv (Go)** | ❌ | ❌ | ✅ |
| **debugpy (Python)** | ❌ | ❌ | ✅ |
| **DAP Protocol** | ❌ | ❌ | ✅ (27 ops) |

---

## 7. Provider Closeness Matrix

### 7.1 Conceptual Positioning

```
                    Infrastructure ◄─────────────────► Developer Tools
                          │                              │
                          │    ┌─────────┐               │
                          │    │  Loom   │  ◄── Platform/infrastructure
                          │    └─────────┘     (MCP, hooks, vault)
                          │                              │
                          │         ┌─────────┐          │
                          │         │ Emdash  │ ◄── Desktop orchestrator
                          │         └─────────┘    (visual, team, SSH)
                          │                              │
                          │              ┌─────────┐     │
                          │              │ Oh-My-Pi│ ◄── Execution engine
                          │              └─────────┘     (LSP, DAP, precision)
```

### 7.2 Feature Farness

| Distance | Emdash ←→ Oh-My-Pi | Loom ←→ Both |
|----------|-------------------|--------------|
| **Overlap** | ~20% (worktree, agent, CLI) | ~10% (generic agent) |
| **Complement** | ~80% (orchestration ↔ execution) | ~90% (different layers) |
| **Integration** | Emdash wraps omp as "Pi" provider | Loom could wrap both |

---

## 8. Overall Scoring

### 8.1 Category Scores (out of 10)

| Category | Loom | Emdash | Oh-My-Pi |
|----------|-----:|-------:|---------:|
| **Infrastructure** | **10** | 3 | 7 |
| **MCP/Skills** | **9** | 1 | 3 |
| **Auth/Security** | **10** | 2 | 2 |
| **Hooks/Automation** | **9** | 1 | 7 |
| **Session/Memory** | 7 | 2 | **9** |
| **Routing/Cost** | **8** | 1 | 7 |
| **Provider Breadth** | 3 | **10** | **10** |
| **Code Precision** | 2 | 2 | **10** |
| **Debugging** | 1 | 1 | **10** |
| **Issue Trackers** | 1 | **10** | 1 |
| **Visual UI** | 5 | **9** | 4 |
| **Remote Dev** | 1 | **10** | 2 |
| **Token Efficiency** | 3 | 3 | **9** |
| **Subagent Support** | 2 | 5 | **9** |

### 8.2 Weighted Total (Normalized to 100)

| Tool | Score | Weight | Notes |
|------|------:|--------|-------|
| **Oh-My-Pi** | **78** | 25% | Best execution, weakest orchestration |
| **Loom** | **72** | 30% | Best platform, needs execution layer |
| **Emdash** | **68** | 20% | Best UX, needs depth |

### 8.3 Radar Chart Summary

```
                 ┌─ Infrastructure ─┐
                ╱                    ╲
               ╱                      ╲
              │    Oh-My-Pi          │
              │        ●             │  Provider
              │                      │   Breadth
              │  Loom                │
              │    ●                 │
              │                      │
              │              Emdash  │
              │                ●     │
               ╲                     ╱
                ╲                   ╱
                 ╲                 ╱
                  ─── Precision ───

         Loom ●───────────────────────● Emdash
              │                      │
              │      ● Oh-My-Pi      │
              │                      │
         ─────┴──────────────────────┴─────
         Platform                    Execution
```

---

## 9. The Ideal Stack

### 9.1 Stack Composition

```
┌─────────────────────────────────────────────────────────────────┐
│                    EMdash (Macro-Orchestrator)                  │
│  Issue Trackers │ Worktree Mgmt │ Visual UI │ SSH │ CI/CD     │
└─────────────────────────────────────────────────────────────────┘
                              │
                    (runs as provider)
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                  Oh-My-Pi (Micro-Execution)                     │
│  LSP Edits │ DAP Debug │ Hashline │ Hindsight │ Subagents     │
└─────────────────────────────────────────────────────────────────┘
                              │
                    (protocol bridge)
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Loom (Platform Layer)                      │
│  MCP Registry │ Skills │ Hooks │ Vault │ Routing │ Telemetry   │
└─────────────────────────────────────────────────────────────────┘
```

### 9.2 What Each Tool Excels At

| Task | Use | Tool |
|------|-----|------|
| Connect Linear/Jira tickets | Visual workflow | **Emdash** |
| Create isolated worktrees | Branch management | **Emdash** |
| Debug C++ segfault | Attach lldb | **Oh-My-Pi** |
| Rename function across codebase | LSP rename | **Oh-My-Pi** |
| Token-efficient batch edits | Hashline | **Oh-My-Pi** |
| Cross-session memory | Hindsight | **Oh-My-Pi** |
| Register MCP server | Tool extension | **Loom** |
| Route by complexity | Model tiering | **Loom** |
| Encrypted credentials | Team sharing | **Loom** |
| Hook on event | Automation | **Loom** |
| Visual diff review | PR merge | **Emdash** |
| Subagent fan-out | Parallel tasks | **Both** |

---

## 10. Gaps Summary

### 10.1 Loom Gaps (from this analysis)

| Gap | Source | Priority |
|-----|--------|----------|
| LSP integration | Oh-My-Pi | 🔴 High |
| DAP debugging | Oh-My-Pi | 🔴 High |
| Hashline edits | Oh-My-Pi | 🟡 Medium |
| Hindsight memory | Oh-My-Pi | 🟡 Medium |
| SSH remote dev | Emdash | 🟡 Medium |
| Issue trackers | Emdash | 🔴 High |
| Visual UI | Emdash | 🟢 Low |
| Subagent typing | Oh-My-Pi | 🟡 Medium |

### 10.2 Emdash Gaps

| Gap | Source | Priority |
|-----|--------|----------|
| MCP support | Loom | 🔴 High |
| Encrypted vault | Loom | 🟡 Medium |
| Hook system | Loom | 🟡 Medium |
| Token optimization | Oh-My-Pi | 🟡 Medium |
| Debugger integration | Oh-My-Pi | 🟡 Medium |
| Hindsight memory | Oh-My-Pi | 🟡 Medium |

### 10.3 Oh-My-Pi Gaps

| Gap | Source | Priority |
|-----|--------|----------|
| Visual workspace | Emdash | 🟡 Medium |
| Issue tracker integration | Emdash | 🟡 Medium |
| SSH remote support | Emdash | 🟡 Medium |
| MCP ecosystem | Loom | 🟢 Low |
| Encrypted vault | Loom | 🟢 Low |
| Auto-update | Emdash | 🟢 Low |

---

## Appendix A: Feature Checklist

### Code Mutation

| Feature | Loom | Emdash | Oh-My-Pi |
|---------|:----:|:------:|:--------:|
| Standard edit | ✅ | ✅ | ✅ |
| Hashline anchors | ❌ | ❌ | ✅ |
| LSP rename | ❌ | ❌ | ✅ |
| LSP code actions | ❌ | ❌ | ✅ |
| AST structural edit | ❌ | ❌ | ✅ |
| Preview before apply | ❌ | ❌ | ✅ |
| Stale anchor detection | ❌ | ❌ | ✅ |

### Debugging

| Feature | Loom | Emdash | Oh-My-Pi |
|---------|:----:|:------:|:--------:|
| No debugger | ✅ | ✅ | ❌ |
| Attach to process | ❌ | ❌ | ✅ |
| Breakpoints | ❌ | ❌ | ✅ |
| Stack frames | ❌ | ❌ | ✅ |
| lldb | ❌ | ❌ | ✅ |
| dlv (Go) | ❌ | ❌ | ✅ |
| debugpy (Python) | ❌ | ❌ | ✅ |

### Memory

| Feature | Loom | Emdash | Oh-My-Pi |
|---------|:----:|:------:|:--------:|
| Session persistence | ✅ | ❌ | ✅ |
| Compression | ✅ | ❌ | ❌ |
| Cross-session context | ⚠️ | ❌ | ✅ |
| Hindsight banks | ❌ | ❌ | ✅ |
| retain/recall/reflect | ❌ | ❌ | ✅ |
| checkpoint/rewind | ❌ | ❌ | ✅ |

---

## Appendix B: Data Sources

- **Emdash:** GitHub API, README.md (4,569 stars, YC W26)
- **Oh-My-Pi:** GitHub API, README.md (5,977 stars, ~27k Rust LoC)
- **Loom:** Local codebase analysis
- **Analysis Date:** 2026-05-22

---

## Appendix C: Terminology

| Term | Definition |
|------|------------|
| **ACP** | Agent Communication Protocol (Oh-My-Pi's editor bridge) |
| **DAP** | Debug Adapter Protocol |
| **Hashline** | Content-hash anchored edits (reduces tokens 61%) |
| **Hindsight** | Oh-My-Pi's cross-session memory system |
| **LSP** | Language Server Protocol |
| **Macro-Micro** | Emdash (orchestration) + omp (execution) stack |
| **pi-iso** | Oh-My-Pi's workspace isolation layer |

---

*Document generated by Loom team analysis.*