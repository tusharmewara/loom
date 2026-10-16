use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_pulls(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Pull Requests ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);

    frame.render_widget(block, area);

    // When diff view is active, show PR list on the left and diff on the right
    if app.show_diff_view {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([
                ratatui::layout::Constraint::Percentage(40),
                ratatui::layout::Constraint::Percentage(60),
            ])
            .split(inner);

        draw_pr_list(frame, chunks[0], app, theme, " PRs ");
        draw_diff_view(frame, chunks[1], app, theme);
    } else {
        let chunks = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Percentage(50),
                ratatui::layout::Constraint::Percentage(50),
            ])
            .split(inner);

        draw_pr_list(frame, chunks[0], app, theme, " PRs ");
        if app.pr_ci_status.is_object()
            && !app
                .pr_ci_status
                .get("checks")
                .unwrap_or(&Value::Null)
                .as_array()
                .unwrap_or(&Vec::new())
                .is_empty()
        {
            draw_ci_status(frame, chunks[1], app, theme);
        } else {
            draw_pr_detail(frame, chunks[1], app, theme);
        }
    }
}

fn draw_pr_list(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    app: &App,
    theme: &Theme,
    title: &str,
) {
    let pulls = &app.pulls;
    let items: Vec<ListItem> = if pulls.is_null() || !pulls.is_object() {
        vec![ListItem::new(Line::from(Span::styled(
            "No PR data. Run a daemon with issues integration to see PRs.",
            theme.text_dim,
        )))]
    } else {
        let list = pulls
            .get("issues")
            .and_then(|v| v.as_array())
            .map(|a| a.as_slice())
            .unwrap_or_default();

        if list.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No open PRs found.",
                theme.text_dim,
            )))]
        } else {
            list.iter()
                .enumerate()
                .map(|(i, pr)| {
                    let title = pr
                        .pointer("/title")
                        .and_then(Value::as_str)
                        .unwrap_or("(untitled)");
                    let state = pr.pointer("/state").and_then(Value::as_str).unwrap_or("?");
                    let id = pr.pointer("/id").and_then(Value::as_str).unwrap_or("?");
                    let labels: Vec<&str> = pr
                        .pointer("/labels")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();

                    let is_selected = i == app.pull_selection_index;
                    let style = if is_selected {
                        Style::default()
                            .fg(theme.selection_fg)
                            .bg(theme.selection_bg)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text)
                    };

                    let state_marker = match state {
                        "open" => "🟢",
                        "closed" | "merged" => "🟣",
                        _ => "⚪",
                    };

                    let label_str = if labels.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", labels.join(", "))
                    };

                    ListItem::new(Line::from(Span::styled(
                        format!(" {}  {}{}  {}", state_marker, id, label_str, title),
                        style,
                    )))
                })
                .collect()
        }
    };

    let list_widget = List::new(items).block(
        Block::default()
            .borders(Borders::TOP)
            .title(title)
            .title_style(Style::default().fg(theme.secondary)),
    );
    frame.render_widget(list_widget, area);
}

fn draw_pr_detail(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let detail_lines = if let Some(selected_pr) = app.selected_pull() {
        let title = selected_pr
            .pointer("/title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        let state = selected_pr
            .pointer("/state")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let id = selected_pr
            .pointer("/id")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let body = selected_pr
            .pointer("/body")
            .and_then(Value::as_str)
            .unwrap_or("");
        let labels: Vec<&str> = selected_pr
            .pointer("/labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let assignee = selected_pr
            .pointer("/assignee")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let url = selected_pr
            .pointer("/html_url")
            .and_then(Value::as_str)
            .unwrap_or("");

        let mut lines = vec![
            Line::from(Span::styled(
                format!(" {title}"),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(format!(" {id}  ({state})",), theme.text_dim)),
            Line::from(Span::styled("", Style::default())),
        ];

        if !labels.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" Labels: {}", labels.join(", ")),
                theme.text,
            )));
        }
        lines.push(Line::from(Span::styled(
            format!(" Assignee: {assignee}"),
            theme.text,
        )));
        if !url.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" URL: {url}"),
                Style::default().fg(theme.secondary),
            )));
        }

        if !body.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                " Description:",
                Style::default()
                    .fg(theme.text_dim)
                    .add_modifier(Modifier::UNDERLINED),
            )));
            for line in body.lines().take(8) {
                lines.push(Line::from(Span::styled(line, theme.text)));
            }
            if body.lines().count() > 8 {
                lines.push(Line::from(Span::styled(
                    format!(" ... ({} more lines)", body.lines().count() - 8),
                    theme.text_dim,
                )));
            }
        }

        lines
    } else {
        vec![Line::from(Span::styled(
            " Select a PR to view details.",
            theme.text_dim,
        ))]
    };

    let detail = Paragraph::new(detail_lines)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .title(" Details ")
                .title_style(Style::default().fg(theme.secondary)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(detail, area);
}

fn draw_ci_status(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let mut lines = vec![Line::from(Span::styled(
        " CI Status",
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD),
    ))];

    if let Some(checks) = app
        .pr_ci_status
        .pointer("/checks")
        .and_then(Value::as_array)
    {
        if checks.is_empty() {
            lines.push(Line::from(Span::styled(
                " No CI checks found.",
                theme.text_dim,
            )));
        } else {
            for check in checks {
                let name = check
                    .pointer("/name")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let status = check
                    .pointer("/status")
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let (icon, style) = match status {
                    "success" => ("✅", Style::default().fg(theme.success)),
                    "failure" => ("❌", Style::default().fg(theme.error)),
                    "cancelled" => ("⏹️", Style::default().fg(theme.warning)),
                    "pending" | "queued" | "in_progress" => {
                        ("⏳", Style::default().fg(theme.warning))
                    }
                    "neutral" => ("➖", Style::default().fg(theme.text_dim)),
                    _ => ("❓", Style::default().fg(theme.text_dim)),
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", icon), style),
                    Span::styled(name.to_string(), theme.text),
                ]));
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            " No CI status available.",
            theme.text_dim,
        )));
    }

    let ci = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .title(" CI Status ")
                .title_style(Style::default().fg(theme.secondary)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(ci, area);
}

fn draw_diff_view(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let diff_text = match app.pr_diff {
        Some(ref diff) => diff.clone(),
        None => "No diff loaded. Select a PR and toggle diff view.".to_string(),
    };

    let lines: Vec<Line> = diff_text
        .lines()
        .skip(app.pr_diff_scroll)
        .take(area.height as usize)
        .map(|line| {
            let style = if line.starts_with('+') && !line.starts_with("+++") {
                Style::default().fg(theme.success)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Style::default().fg(theme.error)
            } else if line.starts_with("@@") {
                Style::default().fg(theme.warning)
            } else {
                Style::default().fg(theme.text)
            };
            Line::from(Span::styled(line.to_string(), style))
        })
        .collect();

    let diff = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .title(" Diff ")
                .title_style(Style::default().fg(theme.secondary)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(diff, area);
}
