pub mod adapter;
pub mod app;
pub mod dispatch;
pub mod theme;
pub mod ui;

use crate::adapter::LoomCliAdapter;
use crate::app::App;
use crate::dispatch::Dispatcher;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;
use std::io;
use std::time::{Duration, Instant};

pub async fn run() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let adapter = LoomCliAdapter::new("loom");
    let mut app = App::new(adapter);
    app.refresh_safe().await;

    let result = run_loop(&mut terminal, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    terminal.clear()?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let refresh_interval = Duration::from_secs(1);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.help_overlay_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') => app.close_help(),
                        _ => {}
                    }
                    continue;
                }

                if app.dispatch_menu_open {
                    match key.code {
                        KeyCode::Esc => app.close_dispatch_menu(),
                        KeyCode::Enter => {
                            let dispatchers: Vec<Box<dyn crate::dispatch::Dispatcher>> = vec![
                                Box::new(crate::dispatch::TerminalLaunch),
                                Box::new(crate::dispatch::ACPDispatch::new(
                                    app.status
                                        .pointer("/daemon/reachable")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false),
                                )),
                                Box::new(crate::dispatch::EmbeddedPane),
                            ];
                            if app.dispatch_menu_selected < dispatchers.len() {
                                if let Some(task) = app.selected_task() {
                                    let result =
                                        dispatchers[app.dispatch_menu_selected].dispatch(task);
                                    app.last_action_result = Some(crate::adapter::CommandResult {
                                        action: dispatchers[app.dispatch_menu_selected]
                                            .name()
                                            .to_string(),
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

                if app.action_palette_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('D') => app.close_action_palette(),
                        KeyCode::Enter => app.execute_selected_action().await,
                        KeyCode::Down | KeyCode::Char('j') => app.action_palette_next(),
                        KeyCode::Up | KeyCode::Char('k') => app.action_palette_prev(),
                        _ => {}
                    }
                    continue;
                }

                if app.command_palette_open {
                    match key.code {
                        KeyCode::Esc => {
                            app.close_command_palette();
                        }
                        KeyCode::Enter => {
                            if let Some(cmd_name) = app.command_palette_selected_name() {
                                let should_continue = app.execute_named_command(&cmd_name).await;
                                app.close_command_palette();
                                if !should_continue {
                                    break;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let max = app.command_palette_count();
                            if max > 0 {
                                app.command_palette_selected = (app.command_palette_selected + 1) % max;
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let max = app.command_palette_count();
                            if max > 0 {
                                app.command_palette_selected = if app.command_palette_selected == 0 {
                                    max - 1
                                } else {
                                    app.command_palette_selected - 1
                                };
                            }
                        }
                        KeyCode::Backspace => {
                            app.command_palette_backspace();
                        }
                        KeyCode::Delete => {
                            app.command_palette_backspace();
                        }
                        KeyCode::Char(c) if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' => {
                            app.command_palette_append(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('r') => app.refresh_safe().await,
                    KeyCode::Char('s') => app.run_scan_command().await,
                    KeyCode::Char('d') => app.run_doctor_command().await,
                    KeyCode::Char('?') => app.toggle_help(),
                    KeyCode::Tab | KeyCode::Right => app.next_tab(),
                    KeyCode::BackTab | KeyCode::Left => app.prev_tab(),
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
                    KeyCode::Char('1') => app.set_tab_from_digit(0),
                    KeyCode::Char('2') => app.set_tab_from_digit(1),
                    KeyCode::Char('3') => app.set_tab_from_digit(2),
                    KeyCode::Char('4') => app.set_tab_from_digit(3),
                    KeyCode::Char('5') => app.set_tab_from_digit(4),
                    KeyCode::Char('6') => app.set_tab_from_digit(5),
                    KeyCode::Char('7') => app.set_tab_from_digit(6),
                    KeyCode::Char('D') => app.toggle_action_palette(),
                    KeyCode::Char('a') if app.is_task_tab() => app.open_dispatch_menu(),
                    KeyCode::Char(']') => app.next_theme(),
                    KeyCode::Char('[') => app.prev_theme(),
                    KeyCode::Char('S') => app.toggle_sidebar(),
                    KeyCode::Char('p') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                        app.toggle_command_palette();
                    }
                    _ => {}
                }
            }
        }

        if last_tick.elapsed() >= refresh_interval {
            app.refresh_safe().await;
            last_tick = Instant::now();
        }
    }

    Ok(())
}
