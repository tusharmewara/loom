# Loom Exec & Shell Integration

## Overview

Loom provides two ways to integrate with existing agentic tools (opencode, goose, claude-code, etc.):

1. **`loom exec`** - Explicit wrapper command
2. **`loom init-shell`** - Transparent shell integration

## Phase 1: `loom exec` - Wrapper Script

Execute any agentic tool with loom ecosystem integration:

```bash
# Single tool
loom exec opencode
loom exec opencode -- "fix the bug"

# Multiple tools (ecosystem for all, launch last)
loom exec claude-code opencode
loom exec opencode goose --dry-run

# Dry run to inspect env vars
loom exec opencode --dry-run
```

### Multi-Tool Support

When multiple tools are specified, loom enables ecosystem integration for all of them but only launches the last one:

```bash
loom exec claude-code opencode
# ✓ Ecosystem enabled for 'claude-code'
# 🚀 Launching 'opencode' with loom ecosystem...
```

This is useful when:
- You want to share resources between tools
- You're migrating from one tool to another
- You want to test tool compatibility

### Environment Variables Injected

| Variable | Description |
|----------|-------------|
| `LOOM_ENABLED` | Set to `1` when loom is active |
| `LOOM_AGENTS_DIR` | Path to `~/.loom/` ecosystem |
| `LOOM_SKILLS_DIR` | Shared skills directory |
| `LOOM_MCP_DIR` | Shared MCP servers directory |
| `LOOM_SESSIONS_DIR` | Shared sessions directory |
| `LOOM_PROJECT_NAME` | Project name from `.loom` file |
| `LOOM_ROUTING_TIER` | Default routing tier (slm/mid/flagship) |
| `LOOM_ROUTING_STRATEGY` | Routing strategy (balanced/cost/quality) |
| `{TOOL}_SKILLS_DIR` | Tool-specific skills directory |
| `{TOOL}_MCP_CONFIG` | Tool-specific MCP config path |
| `{TOOL}_SESSIONS_DIR` | Tool-specific sessions directory |

## Phase 2: `loom init-shell` - Transparent Integration

Add to your shell config for automatic loom activation:

```bash
# zsh
echo 'eval "$(loom init-shell)"' >> ~/.zshrc

# bash
echo 'eval "$(loom init-shell)"' >> ~/.bashrc

# fish
echo 'loom init-shell | source' >> ~/.config/fish/config.fish
```

After adding, tools work normally but automatically use loom:

```bash
# These now use loom ecosystem automatically
opencode "fix the bug"
goose "add tests"
claude-code "refactor this"
```

### Shell Integration Features

- **Auto-wraps detected tools**: opencode, goose, claude-code, codex, crush, aider
- **Project-level activation**: `.loom` file enables/disables per-project
- **Bypass helper**: `loom-bypass <tool>` runs tool without loom

### Project-Level Activation

Create a `.loom` file in your project root:

```toml
[project]
name = "my-project"
loom_dir = "~/.loom"

[tools]
enabled = ["opencode", "claude-code"]

[routing]
default_tier = "mid"
strategy = "balanced"

[mcp]
shared = ["filesystem", "github"]

[skills]
auto_load = true
```

The shell integration checks for this file on directory changes and activates loom accordingly.

## Bypassing Loom

When you need to run a tool without loom:

```bash
# Using the bypass helper (shell integration)
loom-bypass opencode

# Or use the tool directly without the alias
command opencode
```

## Examples

### Basic Usage

```bash
# Start opencode with loom
loom exec opencode

# Start opencode with a message
loom exec opencode -- "Explain the codebase structure"

# Dry run to see what would happen
loom exec opencode --dry-run
```

### Multi-Tool Workflow

```bash
# Enable ecosystem for claude-code, launch opencode
loom exec claude-code opencode

# Enable ecosystem for multiple tools, launch with args
loom exec opencode goose claude-code -- --help
```

### Project Setup

```bash
# Initialize loom ecosystem
loom init --import

# Register tools
loom thread opencode
loom thread claude-code

# Create project config
cat > .loom << 'EOF'
[project]
name = "my-app"

[tools]
enabled = ["opencode"]
EOF

# Start working
loom exec opencode
```

## Troubleshooting

### Tool not found

```bash
# Check if tool is in PATH
which opencode

# Use absolute path
loom exec /path/to/opencode
```

### Shell integration not working

```bash
# Verify shell type
echo $SHELL

# Regenerate shell script
loom init-shell --shell zsh

# Check if alias is set
alias opencode
```

### Project config not loading

```bash
# Verify .loom file exists
ls -la .loom

# Check TOML syntax
cat .loom

# Test with dry run
loom exec opencode --dry-run
```
