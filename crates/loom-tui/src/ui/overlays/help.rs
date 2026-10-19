#![allow(clippy::vec_init_then_push)]

use crate::theme::Theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn draw_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_focused))
        .title(" Help ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "Navigation",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  1-0 !@#   ", Style::default().fg(theme.primary)),
        Span::styled("Switch tabs (last two: Tab/→)", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Tab/→     ", Style::default().fg(theme.primary)),
        Span::styled("Next tab", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  S-Tab/←   ", Style::default().fg(theme.primary)),
        Span::styled("Previous tab", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ↑/↓       ", Style::default().fg(theme.primary)),
        Span::styled("Scroll / select tasks", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ←→↑↓/hjkl ", Style::default().fg(theme.primary)),
        Span::styled("Move Kanban selection (Kanban tab)", theme.text),
    ]));
    lines.push(Line::from(Span::styled("", Style::default())));

    lines.push(Line::from(Span::styled(
        "Actions",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  r         ", Style::default().fg(theme.primary)),
        Span::styled("Refresh all data", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  s         ", Style::default().fg(theme.primary)),
        Span::styled("Run ecosystem scan", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  d         ", Style::default().fg(theme.primary)),
        Span::styled("Run doctor checks", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  c         ", Style::default().fg(theme.primary)),
        Span::styled("Create PR from branch (PRs tab)", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  a         ", Style::default().fg(theme.primary)),
        Span::styled("Dispatch selected task", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Enter     ", Style::default().fg(theme.primary)),
        Span::styled("Dispatch Kanban selection (Kanban tab)", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  > / p     ", Style::default().fg(theme.primary)),
        Span::styled("Advance Kanban task to next phase", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  x         ", Style::default().fg(theme.primary)),
        Span::styled("Remove selected task (Review/Kanban)", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  D         ", Style::default().fg(theme.primary)),
        Span::styled("Daemon action palette", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ]         ", Style::default().fg(theme.primary)),
        Span::styled("Next theme", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  [         ", Style::default().fg(theme.primary)),
        Span::styled("Previous theme", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  S         ", Style::default().fg(theme.primary)),
        Span::styled("Toggle sidebar", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Ctrl+P    ", Style::default().fg(theme.primary)),
        Span::styled("Command palette", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Tab       ", Style::default().fg(theme.primary)),
        Span::styled("Next Orchestrator field", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Enter     ", Style::default().fg(theme.primary)),
        Span::styled("Send Orchestrator task", theme.text),
    ]));
    lines.push(Line::from(Span::styled("", Style::default())));

    lines.push(Line::from(Span::styled(
        "General",
        Style::default()
            .fg(theme.secondary)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  q         ", Style::default().fg(theme.primary)),
        Span::styled("Quit TUI", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  ?         ", Style::default().fg(theme.primary)),
        Span::styled("Toggle help", theme.text),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Esc       ", Style::default().fg(theme.primary)),
        Span::styled("Close overlay", theme.text),
    ]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}
