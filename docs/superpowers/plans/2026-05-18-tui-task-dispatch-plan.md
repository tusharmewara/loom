# TUI Task Dispatch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add task selection and agent dispatch to the Loom TUI — select a workflow task, choose a dispatch method (Terminal Launch or ACP), and show feedback.

**Architecture:** New `dispatch.rs` module with `Dispatcher` trait + `TerminalLaunch` and `ACPDispatch` implementations. Task selection unified with scroll on Logs/Sessions tabs. Dispatch menu overlay (same pattern as daemon action palette).

**Tech Stack:** Rust, ratatui, crossterm, tokio, serde_json

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/loom-tui/src/dispatch.rs` | Dispatcher trait, TerminalLaunch, ACPDispatch, EmbeddedPane stub |
| `crates/loom-tui/src/adapter.rs` | Add `dispatch_task_acp()` method for daemon IPC |
| `crates/loom-tui/src/app.rs` | Add dispatch state fields, selection methods, dispatch execution |
| `crates/loom-tui/src/ui.rs` | Render dispatch menu overlay, highlight selected task, update help text |
| `crates/loom-tui/src/main.rs` | Wire up `a`, `Enter`, `j/k` keybindings for dispatch flow |

---

### Task 1: Dispatcher Trait + TerminalLaunch

**Files:**
- Create: `crates/loom-tui/src/dispatch.rs`
- Modify: `crates/loom-tui/src/main.rs:1` (add `mod dispatch;`)
- Test: `crates/loom-tui/src/dispatch.rs` (inline unit tests)

- [ ] **Step 1: Create dispatch.rs with Dispatcher trait and TerminalLaunch**

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchResult {
    pub success: bool,
    pub message: String,
    pub detail: Option<String>,
}

pub trait Dispatcher: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn shortcut(&self) -> char;
    fn available(&self) -> bool;
    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult;
}

pub struct TerminalLaunch;

impl TerminalLaunch {
    fn find_agent_binary() -> Option<String> {
        let candidates = ["opencode", "claude-code", "jcode", "crush"];
        for bin in &candidates {
            if Self::command_exists(bin) {
                return Some(bin.to_string());
            }
        }
        None
    }

    fn command_exists(cmd: &str) -> bool {
        std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn find_terminal_launcher() -> Option<(&'static str, &'static str)> {
        if cfg!(target_os = "macos") {
            if Self::command_exists("open") {
                Some(("open", "-a Terminal"))
            } else {
                None
            }
        } else {
            for cmd in &["x-terminal-emulator", "gnome-terminal", "xterm"] {
                if Self::command_exists(cmd) {
                    return Some((cmd, ""));
                }
            }
            None
        }
    }
}

impl Dispatcher for TerminalLaunch {
    fn name(&self) -> &'static str {
        "Terminal Launch"
    }

    fn description(&self) -> &'static str {
        "Open a new terminal window with the agent"
    }

    fn shortcut(&self) -> char {
        't'
    }

    fn available(&self) -> bool {
        Self::find_agent_binary().is_some() && Self::find_terminal_launcher().is_some()
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        let agent = match Self::find_agent_binary() {
            Some(a) => a,
            None => {
                return DispatchResult {
                    success: false,
                    message: "No agent binary found on PATH (opencode, claude-code, jcode, crush)".to_string(),
                    detail: None,
                };
            }
        };

        let (launcher, arg) = match Self::find_terminal_launcher() {
            Some(l) => l,
            None => {
                return DispatchResult {
                    success: false,
                    message: "No terminal emulator found".to_string(),
                    detail: None,
                };
            }
        };

        let task_id = task.pointer("/task_id").and_then(Value::as_str).unwrap_or("");
        let title = task.pointer("/title").and_then(Value::as_str).unwrap_or("");

        let command = format!(
            "cd {} && loom-cli workflow show {} && exec {}",
            std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
            task_id,
            agent
        );

        let mut cmd = std::process::Command::new(launcher);
        if cfg!(target_os = "macos") {
            cmd.arg(arg).arg("--args").arg("/bin/bash").arg("-c").arg(&command);
        } else {
            cmd.arg("-e").arg(format!("bash -c '{}'", command));
        }

        match cmd.spawn() {
            Ok(child) => DispatchResult {
                success: true,
                message: format!("Opened {} in terminal (pid {})", agent, child.id()),
                detail: Some(format!("Task: {}", title)),
            },
            Err(e) => DispatchResult {
                success: false,
                message: format!("Failed to launch terminal: {}", e),
                detail: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_launch_name() {
        let dispatch = TerminalLaunch;
        assert_eq!(dispatch.name(), "Terminal Launch");
        assert_eq!(dispatch.shortcut(), 't');
    }

    #[test]
    fn test_command_exists_missing() {
        assert!(!TerminalLaunch::command_exists("definitely-not-a-real-binary-xyz"));
    }

    #[test]
    fn test_dispatch_result_serialization() {
        let result = DispatchResult {
            success: true,
            message: "ok".to_string(),
            detail: Some("detail".to_string()),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("message"));
    }
}
```

- [ ] **Step 2: Register dispatch module in main.rs**

Add `mod dispatch;` to `crates/loom-tui/src/main.rs:1`:

```rust
mod adapter;
mod app;
mod dispatch;
mod ui;
```

- [ ] **Step 3: Build and run tests**

```bash
cargo test -p loom-tui
```

Expected: 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/loom-tui/src/dispatch.rs crates/loom-tui/src/main.rs
git commit -m "feat(tui): add Dispatcher trait and TerminalLaunch implementation"
```

---

### Task 2: ACPDispatch + EmbeddedPane Stub

**Files:**
- Modify: `crates/loom-tui/src/dispatch.rs` (append ACPDispatch + EmbeddedPane)
- Modify: `crates/loom-tui/src/adapter.rs` (add `dispatch_task_acp` method)

- [ ] **Step 1: Add ACPDispatch implementation to dispatch.rs**

Append to `crates/loom-tui/src/dispatch.rs` (before `#[cfg(test)]`):

```rust
pub struct ACPDispatch {
    daemon_reachable: bool,
}

impl ACPDispatch {
    pub fn new(daemon_reachable: bool) -> Self {
        Self { daemon_reachable }
    }
}

impl Dispatcher for ACPDispatch {
    fn name(&self) -> &'static str {
        "ACP Dispatch"
    }

    fn description(&self) -> &'static str {
        "Dispatch via daemon ACP protocol"
    }

    fn shortcut(&self) -> char {
        'a'
    }

    fn available(&self) -> bool {
        self.daemon_reachable
    }

    fn dispatch(&self, task: &serde_json::Value) -> DispatchResult {
        DispatchResult {
            success: false,
            message: "ACP dispatch requires daemon IPC (not yet wired)".to_string(),
            detail: Some("Use Terminal Launch for now".to_string()),
        }
    }
}

pub struct EmbeddedPane;

impl Dispatcher for EmbeddedPane {
    fn name(&self) -> &'static str {
        "Embedded Pane"
    }

    fn description(&self) -> &'static str {
        "Run agent inline in TUI (future)"
    }

    fn shortcut(&self) -> char {
        'e'
    }

    fn available(&self) -> bool {
        false
    }

    fn dispatch(&self, _task: &serde_json::Value) -> DispatchResult {
        DispatchResult {
            success: false,
            message: "Embedded Pane is not yet implemented".to_string(),
            detail: None,
        }
    }
}
```

- [ ] **Step 2: Build and verify**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 3: Commit**

```bash
git add crates/loom-tui/src/dispatch.rs
git commit -m "feat(tui): add ACPDispatch and EmbeddedPane stub"
```

---

### Task 3: App State for Task Selection + Dispatch Menu

**Files:**
- Modify: `crates/loom-tui/src/app.rs` (add fields + methods)

- [ ] **Step 1: Add dispatch state fields to App struct**

Modify `crates/loom-tui/src/app.rs` — add fields after `scroll_offsets`:

```rust
pub task_selection_index: usize,
pub dispatch_menu_open: bool,
pub dispatch_menu_selected: usize,
```

- [ ] **Step 2: Initialize new fields in App::new()**

Add to the `Self { ... }` block in `App::new()`:

```rust
task_selection_index: 0,
dispatch_menu_open: false,
dispatch_menu_selected: 0,
```

- [ ] **Step 3: Add task selection methods to App impl**

Append these methods to the `impl App` block (before the closing `}`):

```rust
pub fn task_count(&self) -> usize {
    self.workflow
        .pointer("/tasks")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0)
}

pub fn selected_task(&self) -> Option<&Value> {
    let tasks = self.workflow.pointer("/tasks").and_then(Value::as_array)?;
    if self.task_selection_index < tasks.len() {
        Some(&tasks[self.task_selection_index])
    } else {
        None
    }
}

pub fn select_next_task(&mut self) {
    let count = self.task_count();
    if count > 0 {
        self.task_selection_index = (self.task_selection_index + 1) % count;
    }
}

pub fn select_prev_task(&mut self) {
    let count = self.task_count();
    if count > 0 {
        if self.task_selection_index == 0 {
            self.task_selection_index = count - 1;
        } else {
            self.task_selection_index -= 1;
        }
    }
}

pub fn open_dispatch_menu(&mut self) {
    if self.task_count() > 0 {
        self.dispatch_menu_open = true;
        self.dispatch_menu_selected = 0;
    }
}

pub fn close_dispatch_menu(&mut self) {
    self.dispatch_menu_open = false;
}

pub fn dispatch_menu_next(&mut self) {
    if self.dispatch_menu_selected + 1 < 3 {
        self.dispatch_menu_selected += 1;
    }
}

pub fn dispatch_menu_prev(&mut self) {
    if self.dispatch_menu_selected > 0 {
        self.dispatch_menu_selected -= 1;
    }
}

pub fn is_task_tab(&self) -> bool {
    self.tab_index == 5 || self.tab_index == 6
}
```

- [ ] **Step 4: Build**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 5: Commit**

```bash
git add crates/loom-tui/src/app.rs
git commit -m "feat(tui): add task selection and dispatch menu state to App"
```

---

### Task 4: Dispatch Menu Overlay in UI

**Files:**
- Modify: `crates/loom-tui/src/ui.rs` (add dispatch menu rendering + task highlighting)

- [ ] **Step 1: Add dispatch menu overlay function to ui.rs**

Append to `crates/loom-tui/src/ui.rs` (before `fn centered_rect`):

```rust
fn draw_dispatch_menu(frame: &mut Frame, app: &App) {
    let area = centered_rect(50, 45, frame.area());
    frame.render_widget(Clear, area);

    let task = app.selected_task();
    let task_title = task
        .and_then(|t| t.pointer("/title").and_then(Value::as_str))
        .unwrap_or("Unknown task");
    let task_id = task
        .and_then(|t| t.pointer("/task_id").and_then(Value::as_str))
        .unwrap_or("?");

    let dispatchers: [(&str, char, bool); 3] = [
        ("Terminal Launch", 't', true),
        ("ACP Dispatch", 'a', false),
        ("Embedded Pane", 'e', false),
    ];

    let items: Vec<ListItem> = dispatchers
        .iter()
        .enumerate()
        .map(|(i, (name, shortcut, available))| {
            let is_selected = i == app.dispatch_menu_selected;
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if !available {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            let prefix = if is_selected { "▸ " } else { "  " };
            let avail_text = if *available {
                format!("[{}]", shortcut)
            } else {
                "not available".to_string()
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}{}  {}", prefix, name, avail_text),
                style,
            )))
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!("Dispatch Task: {} ({})", task_title, &task_id[..8.min(task_id.len())]));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(list, area);
}
```

- [ ] **Step 2: Wire dispatch menu into draw() function**

In `fn draw()`, add after the help overlay check (around line 72):

```rust
if app.dispatch_menu_open {
    draw_dispatch_menu(frame, app);
}
```

- [ ] **Step 3: Build**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 4: Commit**

```bash
git add crates/loom-tui/src/ui.rs
git commit -m "feat(tui): add dispatch menu overlay rendering"
```

---

### Task 5: Keybindings for Dispatch Flow

**Files:**
- Modify: `crates/loom-tui/src/main.rs` (add dispatch keybindings)

- [ ] **Step 1: Add dispatch menu state handling in run_loop**

In `fn run_loop()`, add a new block after the help_overlay_open check (before action_palette_open):

```rust
if app.dispatch_menu_open {
    match key.code {
        KeyCode::Esc => app.close_dispatch_menu(),
        KeyCode::Enter => {
            let dispatchers: Vec<Box<dyn crate::dispatch::Dispatcher>> = vec![
                Box::new(crate::dispatch::TerminalLaunch),
                Box::new(crate::dispatch::ACPDispatch::new(
                    app.status.pointer("/daemon/reachable").and_then(Value::as_bool).unwrap_or(false),
                )),
                Box::new(crate::dispatch::EmbeddedPane),
            ];
            if app.dispatch_menu_selected < dispatchers.len() {
                if let Some(task) = app.selected_task() {
                    let result = dispatchers[app.dispatch_menu_selected].dispatch(task);
                    app.last_action_result = Some(crate::adapter::CommandResult {
                        action: dispatchers[app.dispatch_menu_selected].name().to_string(),
                        success: result.success,
                        message: result.message,
                    });
                }
            }
            app.close_dispatch_menu();
        }
        KeyCode::Down | KeyCode::Char('j') => app.dispatch_menu_next(),
        KeyCode::Up | KeyCode::Char('k') => app.dispatch_menu_prev(),
        KeyCode::Char('t') => {
            app.dispatch_menu_selected = 0;
            if let Some(task) = app.selected_task() {
                let dispatcher = crate::dispatch::TerminalLaunch;
                let result = dispatcher.dispatch(task);
                app.last_action_result = Some(crate::adapter::CommandResult {
                    action: dispatcher.name().to_string(),
                    success: result.success,
                    message: result.message,
                });
            }
            app.close_dispatch_menu();
        }
        _ => {}
    }
    continue;
}
```

- [ ] **Step 2: Add `a` keybinding for opening dispatch menu**

In the main `match key.code` block, add after `KeyCode::Char('D')`:

```rust
KeyCode::Char('a') if app.is_task_tab() => app.open_dispatch_menu(),
```

- [ ] **Step 3: Change ↑/↓ behavior for task tabs**

Replace the existing `KeyCode::Down` and `KeyCode::Up` handlers:

```rust
KeyCode::Down => {
    if app.is_task_tab() {
        app.select_next_task();
    } else {
        app.scroll_down();
    }
}
KeyCode::Up => {
    if app.is_task_tab() {
        app.select_prev_task();
    } else {
        app.scroll_up();
    }
}
```

- [ ] **Step 4: Build**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 5: Commit**

```bash
git add crates/loom-tui/src/main.rs
git commit -m "feat(tui): wire up dispatch keybindings (a, Enter, t, j/k)"
```

---

### Task 6: Highlight Selected Task in Logs/Sessions Tabs

**Files:**
- Modify: `crates/loom-tui/src/ui.rs` (update logs_body and sessions_body)

- [ ] **Step 1: Update logs_body to highlight selected task**

Replace the task rendering section in `fn logs_body()` (the `if let Some(tasks)` block):

```rust
if let Some(tasks) = app.workflow.pointer("/tasks").and_then(Value::as_array) {
    for (i, t) in tasks.iter().enumerate() {
        let id = t.pointer("/task_id").and_then(Value::as_str).unwrap_or("?");
        let title = t.pointer("/title").and_then(Value::as_str).unwrap_or("");
        let phase = t.pointer("/phase").and_then(Value::as_str).unwrap_or("?");
        let status = t.pointer("/status").and_then(Value::as_str).unwrap_or("?");
        let prefix = if i == app.task_selection_index && app.tab_index == 6 {
            "▸ "
        } else {
            "  "
        };
        lines.push(format!("{}{} | {} | {} ({})", prefix, &id[..8], title, phase, status));
    }
}
```

- [ ] **Step 2: Update sessions_body to highlight selected session**

Replace the session rendering section in `fn sessions_body()` (the `for s in sess` loop):

```rust
for (i, s) in sess.iter().enumerate() {
    let id = s.pointer("/session_id").and_then(Value::as_str).unwrap_or("?");
    let tool = s.pointer("/source_tool").and_then(Value::as_str).unwrap_or("?");
    let msgs = s.pointer("/message_count").and_then(Value::as_u64).unwrap_or(0);
    let topic = s.pointer("/topic").and_then(Value::as_str).unwrap_or("");
    let exported = s.pointer("/exported_at").and_then(Value::as_str).unwrap_or("");
    let prefix = if i == app.task_selection_index && app.tab_index == 5 {
        "▸ "
    } else {
        "  "
    };
    lines.push(format!("{}─── {} ───", prefix, id));
    lines.push(format!("    Tool: {} | Messages: {}", tool, msgs));
    if !topic.is_empty() {
        lines.push(format!("    Topic: {}", topic));
    }
    if !exported.is_empty() {
        lines.push(format!("    Exported: {}", exported));
    }
}
```

- [ ] **Step 3: Update detail pane title for task tabs**

In `fn draw()`, replace the title line:

```rust
let title = if app.is_task_tab() {
    format!("Details ({}) [↑/↓ select, a dispatch]", app.tabs[app.tab_index])
} else {
    format!("Details ({}) [scroll ↑/↓]", app.tabs[app.tab_index])
};
```

- [ ] **Step 4: Build**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 5: Commit**

```bash
git add crates/loom-tui/src/ui.rs
git commit -m "feat(tui): highlight selected tasks in Logs/Sessions tabs"
```

---

### Task 7: Update Help Overlay + Header Bar

**Files:**
- Modify: `crates/loom-tui/src/ui.rs` (update help text and header)

- [ ] **Step 1: Update header bar text**

In `fn draw()`, change the header line:

```rust
frame.render_widget(
    Paragraph::new("Loom TUI v1 — q quit | r refresh | 1-7 tabs | s scan | d doctor | a dispatch | D actions | ? help"),
    chunks[0],
);
```

- [ ] **Step 2: Update help overlay text**

In `fn draw_help_overlay()`, update the help_text vec:

```rust
let help_text = vec![
    "═══ Keybindings ═══".to_string(),
    "".to_string(),
    "Navigation:".to_string(),
    "  1-7       Switch tabs".to_string(),
    "  Tab/→     Next tab".to_string(),
    "  Shift-Tab/←  Previous tab".to_string(),
    "  ↑/↓       Scroll content / select tasks".to_string(),
    "".to_string(),
    "Actions:".to_string(),
    "  r         Refresh all data".to_string(),
    "  s         Run ecosystem scan".to_string(),
    "  d         Run doctor checks".to_string(),
    "  a         Dispatch selected task (Logs/Sessions)".to_string(),
    "  D         Open daemon action palette".to_string(),
    "".to_string(),
    "Dispatch Menu:".to_string(),
    "  Enter     Execute selected dispatcher".to_string(),
    "  t         Quick: Terminal Launch".to_string(),
    "  Esc       Close menu".to_string(),
    "  j/k       Navigate dispatchers".to_string(),
    "".to_string(),
    "General:".to_string(),
    "  q         Quit TUI".to_string(),
    "  ?         Toggle this help".to_string(),
];
```

- [ ] **Step 3: Build**

```bash
cargo build -p loom-tui
```

Expected: clean build

- [ ] **Step 4: Commit**

```bash
git add crates/loom-tui/src/ui.rs
git commit -m "feat(tui): update help overlay and header for dispatch feature"
```

---

### Task 8: Install + Smoke Test

**Files:**
- No file changes

- [ ] **Step 1: Build release and install**

```bash
cargo build --release -p loom-tui && cp target/release/loom-tui /usr/local/bin/loom-tui
```

- [ ] **Step 2: Verify daemon is running**

```bash
loom-cli daemon status --json
```

Expected: `"running": true`

- [ ] **Step 3: Verify workflow tasks exist**

```bash
loom-cli workflow list --json
```

Expected: at least one task in the list

- [ ] **Step 4: Manual smoke test**

```bash
loom-tui
```

In the TUI:
1. Press `7` to go to Logs tab
2. Press `↑/↓` to select a task (should see `▸` prefix)
3. Press `a` to open dispatch menu
4. Verify menu shows Terminal Launch, ACP Dispatch, Embedded Pane
5. Press `Esc` to close
6. Press `?` to verify help shows dispatch keybindings
7. Press `q` to quit

---

### Task 9: Clippy + Final Commit

- [ ] **Step 1: Run clippy**

```bash
cargo clippy -p loom-tui -- -D warnings
```

Expected: clean

- [ ] **Step 2: Run tests**

```bash
cargo test -p loom-tui
```

Expected: all tests pass

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "feat(tui): complete task dispatch feature with TerminalLaunch"
```
