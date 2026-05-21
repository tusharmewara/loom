# Loom – The Shared Agentic Ecosystem

Loom is a unified agentic ecosystem that enables seamless collaboration between multiple AI tools (jcode, crush, claude-code, opencode, goose, etc.) while sharing skills, sessions, and MCP servers.

## 🚀 Quick Start

### Prerequisites
- Rust 1.70+ (via rustup)
- Cargo workspace
- Unix-like environment (Linux/macOS)

### Installation
1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/loom.git
   cd loom
   ```

2. Build the workspace:
   ```bash
   cargo build --workspace --release
   ```

3. Initialize the ecosystem:
   ```bash
    ./loom/target/release/loom init
   ```

4. Start the daemon:
   ```bash
   ./loomd/target/release/loomd --agents-dir ~/.loom
   ```

5. Run a tool (e.g., jcode):
   ```bash
   jcode --loom-agent ~/.loom
   ```

### Using Loom
- **Cross‑tool sessions**: Start a session in jcode, export it, and continue in claude‑code.
- **Shared skills**: Write a skill once, use it from any tool.
- **MCP servers**: Register MCP servers once, share across tools.
- **Auth vault**: Store credentials securely and share them across tools.
- **Hooks framework**: Intercept and modify tool behavior with custom scripts.

---

## 🏗️ Architecture

Loom is built as a Rust workspace with the following crates:

| Crate | Purpose |
|-------|---------|
| `loom-core` | Core types, constants, and schemas |
| `loom` | Command‑line interface for Loom |
| `loom-discovery` | Discovery and registration of tools and resources |
| `loom-event-bus` | In‑process and cross‑process event bus |
| `loom-mcp` | MCP server registry, pool, and health checking |
| `loom-skills` | Skill registry, loader, adapter, and hot‑reload |
| `loom-sessions` | Session export/import, storage, and migration |
| `loom-auth` | Encrypted credential vault and ACL |
| `loom-hooks` | Hook registry, execution, and audit logging |
| `loom-workflow` | Task management and orchestration |
| `loom-routing` | Complexity scoring and intelligent routing |
| `loom-generic-adapter` | TOML‑based adapter for tool configs |
| `loom-telemetry` | Performance monitoring and logging |
| `loom-ffi` | Foreign function interface for external tools |
| `loom-ipc` | Inter‑process communication primitives |
| `loom-integration-tests` | End‑to‑end integration tests |
| `loom-adapters` | Tool‑specific adapters (jcode, crush, claude‑code, opencode, goose) |

Each crate is independently versioned but shares a common workspace version (`2.0.0`).

---

## 🔧 Key Features

- **Shared Skills**: Write a skill once, use it across all tools.
- **Session Portability**: Export a session from one tool and import it into another.
- **MCP Server Sharing**: Register MCP servers once and share them across tools.
- **Secure Auth**: Store credentials in an encrypted vault with ACLs.
- **Hook System**: Intercept tool events with custom scripts.
- **Workflow Orchestration**: Manage tasks across multiple tools.
- **Intelligent Routing**: Route tasks to the most appropriate tool based on complexity.

---

## 📦 Installation

### From Source
```bash
git clone https://github.com/yourusername/loom.git
cd loom
cargo build --workspace --release
```

### Pre‑built Binaries
Pre‑built binaries for Linux and macOS are available on the [GitHub Releases](https://github.com/yourusername/loom/releases) page.

### Docker
```bash
docker pull loom/loom:latest
docker run -it loom/loom:latest
```

---

## 🤝 Contributing

Contributions are welcome! Please read the [CONTRIBUTING.md](CONTRIBUTING.md) guide for details on how to set up a development environment, run tests, and submit patches.

---

## 📄 License

Loom is licensed under the MIT License. See the `LICENSE` file for details.

---

## 🏗️ Architecture Diagram

```
┌─────────────────┐
│   External     │
│   Tools (jcode,│
│   crush, etc.) │
└───────────┬───┘
            │ ACP (Loom)
            ▼
┌─────────────────┐
│   Loom Core     │
│   ┌─────────┐   │
│   │ Discovery│   │
│   │  &       │   │
│   │  Registry│   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │  MCP    │   │
│   │  Bridge │   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │ Skills  │   │
│   │  Engine │   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │ Session │   │
│   │  Manager│   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │   Auth  │   │
│   │  Fabric │   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │  Hooks  │   │
│   │ Framework│ │
│   └─────────┘   │
│   ┌─────────┐   │
│   │ Workflow│   │
│   │ Engine  │   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │  Routing│   │
│   │  Engine │   │
│   └─────────┘   │
│   ┌─────────┐   │
│   │  Adapters│ │
│   │ (Tool‑   │ │
│   │ specific)│ │
│   └─────────┘   │
└───────────┬───┘
            │ ACP (Loom)
            ▼
┌─────────────────┐
│   External      │
│   Tools (jcode, │
│   crush, etc.)  │
└─────────────────┘
```

---

## 📦 Version History

- **2.0.0** (current): Initial release with full agentic ecosystem.
- **1.0.0**: MVP with basic MCP and skill sharing.

---

## 📄 License

MIT License. See LICENSE file for details.