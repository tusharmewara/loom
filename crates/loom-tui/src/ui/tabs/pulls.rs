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

    // Split into list (top 60%) and detail (bottom 40%)
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Percentage(60),
            ratatui::layout::Constraint::Percentage(40),
        ])
        .split(inner);

    frame.render_widget(block, area);

    // ─── PR list ────────────────────────────────────────────────
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
                    let title = pr.pointer("/title").and_then(Value::as_str).unwrap_or("(untitled)");
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
            .title(" PRs ")
            .title_style(Style::default().fg(theme.secondary)),
    );
    frame.render_widget(list_widget, chunks[0]);

    // ─── PR detail panel ─────────────────────────────────────────
    let detail_lines = if let Some(selected_pr) = app.selected_pull() {
        let title = selected_pr.pointer("/title").and_then(Value::as_str).unwrap_or("Untitled");
        let state = selected_pr.pointer("/state").and_then(Value::as_str).unwrap_or("?");
        let id = selected_pr.pointer("/id").and_then(Value::as_str).unwrap_or("?");
        let body = selected_pr.pointer("/body").and_then(Value::as_str).unwrap_or("");
        let labels: Vec<&str> = selected_pr
            .pointer("/labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let assignee = selected_pr.pointer("/assignee").and_then(Value::as_str).unwrap_or("none");
        let url = selected_pr.pointer("/html_url").and_then(Value::as_str).unwrap_or("");

        let mut lines = vec![
            Line::from(Span::styled(
                format!(" {title}"),
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!(" {id}  ({state})",),
                theme.text_dim,
            )),
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
                Style::default().fg(theme.text_dim).add_modifier(Modifier::UNDERLINED),
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
    frame.render_widget(detail, chunks[1]);
}
