# 🔍 Feature Analysis: Kasetto + Trellis → Loom

> **Date**: 2026-05-22  
> **Purpose**: Comparative analysis of Loom's lineage from Kasetto and Trellis, with feature scoring and improvement roadmap.

---

## Thesis Validation

**The hypothesis that Loom is a synthesis of Kasetto and Trellis holds significant merit.** Loom can be seen as a *marriage* of Kasetto's **declarative skill management** with Trellis's **workflow orchestration and memory**, elevated by a robust Rust-based infrastructure.

### Genetic Comparison Table

| Kasetto DNA | Trellis DNA | Loom Enhancement |
|-------------|-------------|------------------|
| YAML-driven skill/MCP declaration | `.trellis/spec/` progressive specs | TOML-driven tool definitions + event-driven hooks |
| Multi-agent skill sync | Task-centered 4-phase loop (Plan/Implement/Verify/Finish) | `loom-workflow` with 5-phase progression |
| Agent presets (14+ platforms) | Multi-platform support (14 agents) | Built-in adapters for jcode, crush, opencode, goose, claude-code |
| Lock file for reproducibility | Spec/versioning for context | Session compression + full-fidelity snapshots |
| Rust implementation | Node.js + CLI | **Rust + IPC daemon + TUI** |

### Key Insight

- **Kasetto** excels at *local developer setup* — fast, declarative, reproducible
- **Trellis** excels at *project-level workflow guidance* — progressive specs, sub-agents
- **Loom** targets *production team environments* — daemon + IPC + event bus + auth vault + telemetry

---

## 📊 Feature-Level Scoring Matrix

### 1. Declarative Configuration

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Declarative config | ✅ YAML | ⚠️ File-based | ✅ TOML |
| Multi-source config | ✅ | ❌ | ✅ |
| Config inheritance | ✅ | ❌ | ✅ (overlay) |
| Lock file/diffing | ✅ | ❌ | ✅ |
| Remote config via URL | ✅ | ❌ | ❌ |
| **Score** | **4/5** | **2/5** | **4/5** |

### 2. Skill Management

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Skill discovery | ✅ | ⚠️ Manual | ✅ |
| Skill hot-reload | ❌ | ❌ | ✅ |
| Skill registry | ✅ | ❌ | ✅ |
| Multi-format MCP output | ✅ | ❌ | ✅ |
| Local + remote sources | ✅ | ⚠️ Local only | ✅ |
| **Score** | **4/5** | **1/5** | **4/5** |

### 3. Workflow Orchestration

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Multi-phase workflow | ❌ | ✅ (4 phases) | ✅ (5 phases) |
| Auto-injected specs | ❌ | ✅ | ⚠️ (hooks) |
| Task tracking | ❌ | ✅ | ✅ |
| Sub-agent orchestration | ❌ | ✅ | ❌ |
| **Score** | **0/5** | **4/5** | **3/5** |

### 4. Memory & Context

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Project memory (journals) | ❌ | ✅ | ✅ |
| Session persistence | ❌ | ✅ | ✅ |
| Session compression | ❌ | ❌ | ✅ |
| Cross-session context | ❌ | ✅ | ⚠️ |
| **Score** | **0/5** | **3/5** | **3/5** |

### 5. Multi-Agent Support

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Agent presets | ✅ (14+) | ✅ (14+) | ✅ (5 built-in) |
| Agent-agnostic skills | ✅ | ✅ | ✅ |
| Terminal launch | ❌ | ✅ | ✅ |
| Embedded dispatch | ❌ | ✅ | ❌ |
| ACP remote dispatch | ❌ | ❌ | ⚠️ (wired but stub) |
| **Score** | **4/5** | **4/5** | **3/5** |

### 6. Infrastructure

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Daemon architecture | ❌ | ❌ | ✅ |
| IPC protocol | ❌ | ❌ | ✅ (Unix socket) |
| TUI | ⚠️ (basic) | ❌ | ✅ (7 tabs) |
| Event bus | ❌ | ❌ | ✅ (tokio + ZMQ) |
| Health monitoring | ✅ | ❌ | ✅ |
| Telemetry | ❌ | ❌ | ✅ |
| **Score** | **2/5** | **1/5** | **5/5** |

### 7. Auth & Hooks

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Credential vault | ❌ | ❌ | ✅ |
| ACL system | ❌ | ❌ | ✅ |
| Hook system | ❌ | ❌ | ✅ |
| Hook signing | ❌ | ❌ | ✅ |
| Audit logging | ❌ | ❌ | ✅ |
| **Score** | **0/5** | **0/5** | **5/5** |

### 8. Routing & Optimization

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Model tier routing | ❌ | ❌ | ✅ |
| Complexity scoring | ❌ | ❌ | ✅ |
| Cost optimization | ❌ | ❌ | ✅ |
| Manual override | ❌ | ❌ | ✅ |
| **Score** | **0/5** | **0/5** | **4/5** |

### 9. Developer Experience

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Auto-update | ✅ | ❌ | ❌ |
| Self-contained binary | ✅ | ⚠️ (npm) | ✅ |
| Shell completions | ✅ | ⚠️ | ❌ |
| Config doctor | ✅ | ❌ | ✅ |
| **Score** | **3/5** | **1/5** | **2/5** |

### 10. Extensibility

| Feature | Kasetto | Trellis | Loom |
|---------|:-------:|:-------:|:----:|
| Generic adapter | ❌ | ❌ | ✅ |
| TOML-driven plugins | ❌ | ❌ | ✅ |
| FFI support | ❌ | ❌ | ✅ |
| Plugin registry | ✅ | ❌ | ❌ |
| **Score** | **1/5** | **0/5** | **4/5** |

---

## 📈 Overall Scores Summary

| Metric | Kasetto | Trellis | Loom |
|--------|:-------:|:-------:|:----:|
| **Total Points** | 18/50 | 15/50 | **33/50** |
| **Percentage** | 36% | 30% | **66%** |

### Category Breakdown

```
Infrastructure:     Loom ████████████ 100%  |  Kasetto ████ 40%  |  Trellis ██ 20%
Auth & Hooks:       Loom ████████████ 100%  |  Kasetto ░░░░  0%  |  Trellis ░░░░  0%
Routing:            Loom ████████░░░  80%  |  Kasetto ░░░░  0%  |  Trellis ░░░░  0%
Skill Management:   Loom ████████░░░  80%  |  Kasetto ████████ 80% | Trellis █░░░  20%
Config:             Loom ████████░░░  80%  |  Kasetto ████████ 80% | Trellis ██░░  40%
Extensibility:      Loom ████████░░░  80%  |  Kasetto █░░░░ 20%  |  Trellis ░░░░  0%
Multi-Agent:        Loom ██████░░░░░  60%  |  Kasetto ████████ 80% | Trellis ████████ 80%
Workflow:           Loom ██████░░░░░  60%  |  Kasetto ░░░░░  0%  |  Trellis ████████ 80%
Memory:             Loom ██████░░░░░  60%  |  Kasetto ░░░░░  0%  |  Trellis ██████░ 60%
DevEx:              Loom ██░░░░░░░░  40%  |  Kasetto ██████░ 60%  |  Trellis █░░░░ 20%
```

---

## 🛠️ Suggested Improvements for Loom

### High Priority (Score Impact: +15-20%)

| # | Improvement | Origin | Complexity | Issue |
|---|-------------|--------|------------|-------|
| 1 | **Sub-agent orchestration** — Implement the Trellis-style plan/implement/verify sub-agents | Trellis | High | - |
| 2 | **Remote config URL support** — Fetch `kasetto.yaml`-style configs via HTTPS | Kasetto | Low | - |
| 3 | **Auto-update system** — `loom self update` with SHA verification | Kasetto | Medium | - |
| 4 | **Shell completions** — bash/zsh/fish integration | Kasetto | Low | #known-gotcha |
| 5 | **ACP dispatch wiring** — Implement actual remote agent dispatch | Trellis | High | #known-gotcha |
| 6 | **Embedded pane dispatch** — TUI embedded agent execution | Trellis | High | #known-gotcha |
| 7 | **Cross-session memory** — Persistent context across sessions beyond current | Trellis | Medium | - |
| 8 | **Plugin registry** — Online catalog like Kasetto's | Kasetto | Medium | - |

### Medium Priority (Score Impact: +8-12%)

| # | Improvement | Origin | Complexity |
|---|-------------|--------|------------|
| 9 | **Lock file for reproducibility** — Export deterministic skill/MCP snapshots | Kasetto |
| 10 | **Config inheritance (`extends:`)** — Overlay system like Kasetto | Kasetto |
| 11 | **Platform presets** — Pre-configured agent setups (like Kasetto) | Kasetto |
| 12 | **Spec templates** — Starter templates for `.loom/spec/` | Trellis |
| 13 | **Session branching** — Fork a session from a checkpoint | — |
| 14 | **Team sync** — Share configs via git/HTTP | Kasetto |

### Low Priority / Nice-to-Have (Score Impact: +5-8%)

| # | Improvement | Origin | Complexity |
|---|-------------|--------|------------|
| 15 | **Interactive TUI browser** — `kst list`-style skill explorer | Kasetto |
| 16 | **Webhook triggers** — HTTP callbacks on workflow events | — |
| 17 | **Scoop/Homebrew install** — Package managers | Kasetto |
| 18 | **Config schema validation** — JSON Schema for TOML | — |
| 19 | **IDE extensions** — VS Code/Neovim integration | — |
| 20 | **GraphQL API** — Query state beyond IPC socket | — |

---

## 🎯 Quick Wins (Implement First)

### 1. Add Shell Completions (Kasetto Feature)

```toml
# Add to Cargo.toml for loom-cli
[dependencies]
clap_complete = "4.5"
```

```rust
// In cli.rs
use clap_complete::{Generator, Shell};

fn generate_completions<G: Generator>(gen: G, cmd: &mut Command) {
    clap_complete::generate(gen, cmd, "loom", &mut Vec::new());
}
```

### 2. Remote Config URL (Kasetto Feature)

```rust
// In loom-core/src/config.rs
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    // Existing fields...
    
    /// Remote config URL (HTTPS) - similar to Kasetto's remote config
    #[serde(default)]
    pub source: Option<String>,
}

impl Config {
    pub async fn load_from_url(url: &str) -> Result<Self> {
        let resp = reqwest::get(url).await?;
        let text = resp.text().await?;
        toml::from_str(&text).map_err(Into::into)
    }
}
```

### 3. Lock File for Reproducibility (Kasetto Feature)

```rust
// In loom-skills/src/lockfile.rs
#[derive(Serialize, Deserialize)]
pub struct LockFile {
    pub version: String,
    pub skills: HashMap<String, SkillLock>,
    pub mcp_servers: HashMap<String, McpLock>,
    pub checksum: String,
}
```

---

## 📚 References

- **Kasetto**: https://github.com/pivoshenko/kasetto
- **Trellis**: https://github.com/mindfold-ai/Trellis
- **Loom**: https://github.com/agentic-ai-foundation/loom (internal)

---

## Changelog

| Date | Change |
|------|--------|
| 2026-05-22 | Initial analysis document created |