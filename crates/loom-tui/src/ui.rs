use crate::app::App;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Tabs};
use ratatui::Frame;
use serde_json::Value;

pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    frame.render_widget(
        Paragraph::new("Loom TUI v1 — q quit | r refresh | 1-7 tabs | s scan | d doctor | a dispatch | D actions | ? help"),
        chunks[0],
    );

    let tab_titles = app
        .tabs
        .iter()
        .map(|t| Line::from(Span::raw(*t)))
        .collect::<Vec<_>>();
    let tabs = Tabs::new(tab_titles)
        .select(app.tab_index)
        .block(Block::default().borders(Borders::ALL).title("Views"))
        .style(Style::default())
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[1]);

    let body = match app.tab_index {
        0 => dashboard_body(app),
        1 => daemon_body(app),
        2 => discovery_body(app),
        3 => mcp_body(app),
        4 => skills_body(app),
        5 => sessions_body(app),
        6 => logs_body(app),
        _ => "Unknown tab".to_string(),
    };

    let scroll_offset = app.scroll_offsets.get(app.tab_index).copied().unwrap_or(0);
    let scrolled_body = if scroll_offset > 0 {
        body.lines().skip(scroll_offset).collect::<Vec<_>>().join("\n")
    } else {
        body
    };

    let title = if app.is_task_tab() {
        format!("Details ({}) [↑/↓ select, a dispatch]", app.tabs[app.tab_index])
    } else {
        format!("Details ({}) [scroll ↑/↓]", app.tabs[app.tab_index])
    };
    frame.render_widget(
        Paragraph::new(scrolled_body).block(Block::default().borders(Borders::ALL).title(title)),
        chunks[2],
    );

    let status_spans = build_status_line(app);
    frame.render_widget(
        Paragraph::new(Line::from(status_spans)).block(Block::default().borders(Borders::TOP)),
        chunks[3],
    );

    if app.action_palette_open {
        draw_action_palette(frame, app);
    }

    if app.help_overlay_open {
        draw_help_overlay(frame);
    }

    if app.dispatch_menu_open {
        draw_dispatch_menu(frame, app);
    }
}

fn build_status_line(app: &App) -> Vec<Span<'static>> {
    let mut spans = vec![];

    if app.action_in_progress {
        spans.push(Span::styled(
            "RUNNING... ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
    } else if let Some(result) = &app.last_action_result {
        if result.success {
            spans.push(Span::styled(
                format!("[{}] {} | ", result.action, result.message),
                Style::default().fg(Color::Green),
            ));
        } else {
            spans.push(Span::styled(
                format!("[{}] FAILED: {} | ", result.action, result.message),
                Style::default().fg(Color::Red),
            ));
        }
    }

    if let Some(err) = &app.last_error {
        spans.push(Span::styled(
            format!(
                "Last refresh: {:.1}s ago | ERROR: {}",
                app.last_refresh.elapsed().as_secs_f64(),
                err
            ),
            Style::default().fg(Color::Red),
        ));
    } else {
        spans.push(Span::raw(format!(
            "Last refresh: {:.1}s ago | OK",
            app.last_refresh.elapsed().as_secs_f64()
        )));
    }

    spans
}

fn draw_action_palette(frame: &mut Frame, app: &App) {
    let area = centered_rect(50, 40, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .daemon_actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let style = if i == app.action_palette_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let prefix = if i == app.action_palette_selected { "▸ " } else { "  " };
            ListItem::new(Line::from(Span::styled(
                format!("{}daemon {}", prefix, action),
                style,
            )))
        })
        .collect();

    let palette_block = Block::default()
        .borders(Borders::ALL)
        .title("Daemon Actions (Enter=run, Esc=cancel)");

    let list = List::new(items)
        .block(palette_block)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(list, area);
}

fn draw_help_overlay(frame: &mut Frame) {
    let area = centered_rect(55, 55, frame.area());
    frame.render_widget(Clear, area);

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
        "Daemon Actions (D):".to_string(),
        "  Enter     Execute selected daemon action".to_string(),
        "  Esc       Close palette".to_string(),
        "  j/k       Navigate daemon actions".to_string(),
        "".to_string(),
        "General:".to_string(),
        "  q         Quit TUI".to_string(),
        "  ?         Toggle this help".to_string(),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Help (Esc or ? to close)");

    let text = help_text.join("\n");
    frame.render_widget(
        Paragraph::new(text).block(block),
        area,
    );
}

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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn dashboard_body(app: &App) -> String {
    let daemon = app.status.pointer("/daemon/reachable").and_then(Value::as_bool);
    let daemon_status = app.status.pointer("/daemon/status").and_then(Value::as_str);
    let total = app.status.pointer("/resources/total").and_then(Value::as_u64);
    let resource_counts = app.status.pointer("/resources/counts");
    let doctor_pass = app.doctor.pointer("/summary/pass").and_then(Value::as_u64);
    let doctor_warn = app.doctor.pointer("/summary/warn").and_then(Value::as_u64);
    let doctor_fail = app.doctor.pointer("/summary/fail").and_then(Value::as_u64);
    let tools = app
        .scan
        .pointer("/system_audit/tool_hits")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let skills = app
        .scan
        .pointer("/system_audit/skill_sources")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let mcp_sources = app
        .scan
        .pointer("/system_audit/mcp_sources")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);

    let mut lines = vec![
        "═══ Loom Dashboard ═══".to_string(),
        "".to_string(),
        format!("Daemon: {} ({})", 
            daemon.map(|d| if d { "● RUNNING" } else { "○ STOPPED" }).unwrap_or("?"),
            daemon_status.unwrap_or("unknown")),
        format!("Total resources: {}", total.map(|t| t.to_string()).unwrap_or_else(|| "?".to_string())),
        "".to_string(),
        "Resource counts:".to_string(),
    ];

    if let Some(counts) = resource_counts {
        if let Some(obj) = counts.as_object() {
            for (k, v) in obj {
                lines.push(format!("  {}: {}", k, v));
            }
        }
    }

    lines.push("".to_string());
    lines.push(format!("Doctor: pass={} warn={} fail={}",
        doctor_pass.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string()),
        doctor_warn.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string()),
        doctor_fail.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string())));
    lines.push(format!("System audit: tools={} skill_sources={} mcp_sources={}", tools, skills, mcp_sources));

    if let Some(warnings) = app.scan.pointer("/warnings").and_then(Value::as_array) {
        if !warnings.is_empty() {
            lines.push("".to_string());
            lines.push("Warnings:".to_string());
            for w in warnings {
                if let Some(msg) = w.as_str() {
                    lines.push(format!("  ⚠ {}", msg));
                }
            }
        }
    }

    lines.join("\n")
}

fn daemon_body(app: &App) -> String {
    let running = app.daemon.pointer("/running").and_then(Value::as_bool);
    let status = app.daemon.pointer("/status").and_then(Value::as_str);
    let socket = app.daemon.pointer("/socket_path").and_then(Value::as_str);
    let error = app.daemon.pointer("/error").and_then(Value::as_str);
    let data = app.daemon.pointer("/data");

    let mut lines = vec![
        "═══ Daemon Status ═══".to_string(),
        "".to_string(),
        format!("Running: {}", running.map(|v| if v { "Yes" } else { "No" }).unwrap_or("unknown")),
        format!("Status: {}", status.unwrap_or("unknown")),
        format!("Socket: {}", socket.unwrap_or("unknown")),
        format!("Error: {}", error.unwrap_or("none")),
    ];

    if let Some(d) = data {
        if let Some(subsystems) = d.pointer("/subsystems") {
            lines.push("".to_string());
            lines.push("Subsystems:".to_string());
            if let Some(obj) = subsystems.as_object() {
                for (k, v) in obj {
                    let icon = if v.as_bool().unwrap_or(false) { "✓" } else { "✗" };
                    lines.push(format!("  {} {}", icon, k));
                }
            }
        }
        if let Some(version) = d.pointer("/version").and_then(Value::as_str) {
            lines.push(format!("Version: {}", version));
        }
    }

    if let Some(result) = &app.last_action_result {
        lines.push("".to_string());
        lines.push(format!("Last action: [{}] {} ({})",
            result.action,
            result.message,
            if result.success { "OK" } else { "FAILED" }));
    }

    lines.join("\n")
}

fn discovery_body(app: &App) -> String {
    let counts = app.scan.pointer("/ecosystem/resource_counts");
    let warnings = app
        .scan
        .pointer("/warnings")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let phase1 = app
        .scan
        .pointer("/ecosystem/phase_1_duration_us")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let phase2 = app
        .scan
        .pointer("/ecosystem/phase_2/phase_2_duration_us")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    let mut lines = vec![
        "═══ Discovery Scan ═══".to_string(),
        "".to_string(),
        format!("Phase 1: {}μs", phase1),
        format!("Phase 2: {}μs", phase2),
        format!("Warnings: {}", warnings),
        "".to_string(),
        "Resource counts:".to_string(),
    ];

    if let Some(c) = counts {
        if let Some(obj) = c.as_object() {
            for (k, v) in obj {
                lines.push(format!("  {}: {}", k, v));
            }
        }
    }

    if let Some(tools) = app.scan.pointer("/system_audit/tool_hits").and_then(Value::as_array) {
        lines.push("".to_string());
        lines.push("Detected tools:".to_string());
        for t in tools {
            let name = t.pointer("/tool").and_then(Value::as_str).unwrap_or("?");
            let path = t.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(format!("  {} → {}", name, path));
        }
    }

    if let Some(skill_src) = app.scan.pointer("/system_audit/skill_sources").and_then(Value::as_array) {
        lines.push("".to_string());
        lines.push("Skill sources:".to_string());
        for s in skill_src {
            let source = s.pointer("/source").and_then(Value::as_str).unwrap_or("?");
            let count = s.pointer("/skill_count").and_then(Value::as_u64).unwrap_or(0);
            let path = s.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(format!("  {} ({} skills) → {}", source, count, path));
        }
    }

    if let Some(mcp_src) = app.scan.pointer("/system_audit/mcp_sources").and_then(Value::as_array) {
        lines.push("".to_string());
        lines.push("MCP sources:".to_string());
        for m in mcp_src {
            let source = m.pointer("/source").and_then(Value::as_str).unwrap_or("?");
            let count = m.pointer("/server_count").and_then(Value::as_u64).unwrap_or(0);
            let path = m.pointer("/path").and_then(Value::as_str).unwrap_or("?");
            lines.push(format!("  {} ({} servers) → {}", source, count, path));
        }
    }

    lines.join("\n")
}

fn mcp_body(app: &App) -> String {
    let count = app.mcp.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    let servers = app.mcp.pointer("/servers").and_then(Value::as_array);

    let mut lines = vec![
        "═══ MCP Servers ═══".to_string(),
        "".to_string(),
        format!("Registered: {}", count),
    ];

    if let Some(srvs) = servers {
        if srvs.is_empty() {
            lines.push("".to_string());
            lines.push("No MCP servers registered in ~/.agents/mcp/".to_string());
            lines.push("".to_string());
            lines.push("Tip: Add MCP server configs or import from tool configs".to_string());
        } else {
            for s in srvs {
                lines.push("".to_string());
                let id = s.pointer("/id").and_then(Value::as_str).unwrap_or("?");
                let name = s.pointer("/name").and_then(Value::as_str).unwrap_or("?");
                lines.push(format!("─── {} ───", id));
                lines.push(format!("Name: {}", name));
                if let Some(transport) = s.pointer("/transport") {
                    lines.push(format!("Transport: {}", pretty_json_compact(transport)));
                }
            }
        }
    }

    lines.join("\n")
}

fn skills_body(app: &App) -> String {
    let count = app.skills.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    let skills = app.skills.pointer("/skills").and_then(Value::as_array);

    let mut lines = vec![
        "═══ Skills ═══".to_string(),
        "".to_string(),
        format!("Registered: {}", count),
    ];

    if let Some(sk) = skills {
        if sk.is_empty() {
            lines.push("".to_string());
            lines.push("No skills in daemon registry.".to_string());
            lines.push("".to_string());
            lines.push("Check ~/.agents/skills/ for local skills.".to_string());
        } else {
            for s in sk {
                lines.push("".to_string());
                if let Some(name) = s.as_str() {
                    lines.push(format!("  • {}", name));
                } else if let Some(name) = s.pointer("/name").and_then(Value::as_str) {
                    let source = s.pointer("/source").and_then(Value::as_str).unwrap_or("");
                    lines.push(format!("  • {} ({})", name, source));
                }
            }
        }
    }

    lines.join("\n")
}

fn sessions_body(app: &App) -> String {
    let count = app.sessions.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    let sessions = app.sessions.pointer("/sessions").and_then(Value::as_array);

    let mut lines = vec![
        "═══ Sessions ═══".to_string(),
        "".to_string(),
        format!("Stored: {}", count),
    ];

    if let Some(sess) = sessions {
        if sess.is_empty() {
            lines.push("".to_string());
            lines.push("No sessions stored yet.".to_string());
            lines.push("".to_string());
            lines.push("Sessions are exported/imported via:".to_string());
            lines.push("  loom-cli sessions export <id>".to_string());
            lines.push("  loom-cli sessions import <path>".to_string());
        } else {
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
        }
    }

    lines.join("\n")
}

fn logs_body(app: &App) -> String {
    let mut lines = vec![
        "═══ Logs / Events ═══".to_string(),
        "".to_string(),
    ];

    lines.push("Recent activity:".to_string());
    lines.push(format!("  Last refresh: {:.1}s ago", app.last_refresh.elapsed().as_secs_f64()));

    if let Some(result) = &app.last_action_result {
        lines.push(format!("  Last command: [{}] {}", result.action, result.message));
    }

    if let Some(err) = &app.last_error {
        lines.push(format!("  Last error: {}", err));
    }

    lines.push("".to_string());
    lines.push("Workflow tasks:".to_string());
    let task_count = app.workflow.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    lines.push(format!("  Active: {}", task_count));

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

    lines.push("".to_string());
    lines.push("Hooks:".to_string());
    let hook_count = app.hooks.pointer("/count").and_then(Value::as_u64).unwrap_or(0);
    lines.push(format!("  Registered: {}", hook_count));

    if let Some(hooks) = app.hooks.pointer("/hooks").and_then(Value::as_array) {
        for h in hooks {
            let id = h.pointer("/id").and_then(Value::as_str).unwrap_or("?");
            let event = h.pointer("/event_type").and_then(Value::as_str).unwrap_or("?");
            let path = h.pointer("/script_path").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("  {} | {} | {}", id, event, path));
        }
    }

    lines.join("\n")
}

fn pretty_json_compact(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
