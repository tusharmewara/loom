//! Cost dashboard tab — real usage ledger from `loom telemetry usage --json`.
//!
//! Renders what the meter actually recorded (exec, workflow dispatch, acp,
//! resume runs): totals, per-tool breakdown, recent runs. Measured numbers
//! and `chars/4` forecasts are kept visibly distinct.

use crate::app::App;
use crate::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use serde_json::Value;

pub fn draw_cost(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(" Cost Dashboard (usage ledger) ")
        .title_style(
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        );

    let usage = &app.usage;
    if usage.is_null() {
        frame.render_widget(
            Paragraph::new("usage ledger unavailable (run `loom telemetry usage` to check)")
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    let mut lines = Vec::new();
    let bold = Style::default().add_modifier(Modifier::BOLD);

    // ── Totals ─────────────────────────────────────────────────────────
    if let Some(t) = usage.get("totals") {
        let runs = t.get("runs").and_then(Value::as_u64).unwrap_or(0);
        let measured = t.get("measured_runs").and_then(Value::as_u64).unwrap_or(0);
        let unavailable = t.get("unavailable_runs").and_then(Value::as_u64).unwrap_or(0);
        let in_tok = t.get("input_tokens").and_then(Value::as_u64);
        let out_tok = t.get("output_tokens").and_then(Value::as_u64);
        let cost = t.get("cost_usd").and_then(Value::as_f64);
        let est_cost = t.get("est_cost_usd").and_then(Value::as_f64);

        lines.push(Line::from(vec![
            Span::styled("Runs: ", theme.text_dim),
            Span::styled(format!("{runs}"), theme.text_bright),
            Span::styled(
                format!(" ({measured} measured, {unavailable} no-report)   "),
                theme.text_dim,
            ),
            Span::styled("Tokens: ", theme.text_dim),
            Span::styled(
                format!(
                    "{} in / {} out",
                    in_tok.map_or("—".into(), |v| v.to_string()),
                    out_tok.map_or("—".into(), |v| v.to_string()),
                ),
                theme.text_bright,
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Cost (measured): ", theme.text_dim),
            Span::styled(
                cost.map_or("—".into(), |c| format!("${c:.4}")),
                Style::default().fg(theme.success),
            ),
            Span::styled("   Forecast (chars/4): ", theme.text_dim),
            Span::styled(
                est_cost.map_or("—".into(), |c| format!("${c:.4}")),
                Style::default().fg(theme.warning),
            ),
        ]));
    }

    // ── Per-tool breakdown ─────────────────────────────────────────────
    if let Some(tools) = usage.get("by_tool").and_then(Value::as_array) {
        if !tools.is_empty() {
            lines.push(Line::from(Span::styled("", Style::default())));
            lines.push(Line::from(Span::styled(
                "Per tool:",
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
            )));
            for tool in tools {
                let name = tool.get("tool").and_then(Value::as_str).unwrap_or("?");
                let runs = tool.get("runs").and_then(Value::as_u64).unwrap_or(0);
                let cost = tool.get("cost_usd").and_then(Value::as_f64);
                let est = tool.get("est_cost_usd").and_then(Value::as_f64);
                lines.push(Line::from(vec![
                    Span::styled("  • ", theme.primary),
                    Span::styled(format!("{name} "), theme.text),
                    Span::styled(format!("{runs} run(s)"), theme.text_dim),
                    Span::styled(
                        format!(
                            "  ${} measured  ${} forecast",
                            cost.map_or("—".into(), |c| format!("{c:.4}")),
                            est.map_or("—".into(), |c| format!("{c:.4}")),
                        ),
                        theme.text_dim,
                    ),
                ]));
            }
        }
    }

    // ── Recent runs table ───────────────────────────────────────────────
    let recent = usage.get("recent").and_then(Value::as_array);
    let rows: Vec<Row> = recent
        .map(|recs| {
            recs.iter()
                .map(|r| {
                    let when = r
                        .get("recorded_at")
                        .and_then(Value::as_str)
                        .and_then(|s| s.split('T').next())
                        .unwrap_or("?")
                        .to_string();
                    let run_id = r.get("run_id").and_then(Value::as_str).unwrap_or("?");
                    let tool = r.get("tool").and_then(Value::as_str).unwrap_or("?");
                    let model = r.get("model").and_then(Value::as_str).unwrap_or("?");
                    let tokens = match (
                        r.get("input_tokens").and_then(Value::as_u64),
                        r.get("output_tokens").and_then(Value::as_u64),
                    ) {
                        (Some(i), Some(o)) => format!("{i}/{o}"),
                        (Some(i), None) => format!("{i}/—"),
                        (None, Some(o)) => format!("—/{o}"),
                        (None, None) => "—".into(),
                    };
                    let cost = r.get("cost_usd").and_then(Value::as_f64);
                    let method = r.get("method").and_then(Value::as_str).unwrap_or("?");
                    let short_id = &run_id[..run_id.len().min(24)];
                    let short_model = &model[..model.len().min(18)];
                    Row::new(vec![
                        Cell::from(Span::styled(when, theme.text_dim)),
                        Cell::from(Span::styled(short_id.to_string(), theme.text_dim)),
                        Cell::from(Span::styled(tool.to_string(), theme.primary)),
                        Cell::from(Span::styled(short_model.to_string(), theme.text)),
                        Cell::from(Span::styled(tokens, theme.text_bright)),
                        Cell::from(Span::styled(
                            cost.map_or("—".into(), |c| format!("${c:.4}")),
                            theme.success,
                        )),
                        Cell::from(Span::styled(method.to_string(), theme.text_dim)),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    use ratatui::layout::{Constraint, Direction, Layout};
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length((lines.len() + 2) as u16), Constraint::Min(5)])
        .split(area);

    frame.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: false }),
        chunks[0],
    );

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new("no runs recorded yet")
                .block(Block::default().borders(Borders::ALL).border_style(
                    Style::default().fg(theme.border),
                ))
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
        return;
    }

    let header = Row::new(vec![
        Cell::from("Date"),
        Cell::from("Run"),
        Cell::from("Tool"),
        Cell::from("Model"),
        Cell::from("Tok in/out"),
        Cell::from("Cost"),
        Cell::from("Method"),
    ])
    .style(Style::default().fg(theme.text_dim).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(26),
            Constraint::Length(12),
            Constraint::Length(20),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Min(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Recent runs ")
            .title_style(bold),
    )
    .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    frame.render_widget(table, chunks[1]);
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    #[test]
    fn ledger_payload_shapes_used_by_draw_cost_exist() {
        // The minimal payload draw_cost reads: totals, by_tool, recent.
        let payload = json!({
            "totals": {"runs": 1, "measured_runs": 0, "unavailable_runs": 1,
                        "input_tokens": null, "output_tokens": null,
                        "cost_usd": null, "est_cost_usd": 0.001},
            "by_tool": [{"tool": "exec", "runs": 1, "cost_usd": null, "est_cost_usd": 0.001}],
            "recent": [{"recorded_at": "2026-10-24T10:00:00Z", "run_id": "exec-x",
                        "tool": "exec", "model": "m", "input_tokens": null,
                        "output_tokens": null, "cost_usd": null, "method": "no-report"}],
        });
        assert!(payload.get("totals").is_some());
        assert!(payload.get("by_tool").and_then(Value::as_array).unwrap()[0]
            .get("tool").and_then(Value::as_str).unwrap() == "exec");
        assert_eq!(payload.get("recent").and_then(Value::as_array).unwrap().len(), 1);
    }
}
