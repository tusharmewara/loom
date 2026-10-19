mod layout;
pub mod overlays;
mod tabs;
pub mod widgets;

use crate::app::App;
use crate::theme::Theme;
use layout::compute_layout;
use ratatui::Frame;
use widgets::{draw_sidebar, draw_status_bar, draw_tab_bar};

pub use layout::UILayout;

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = &app.theme;
    let show_sidebar = app.sidebar_visible;
    let layout = compute_layout(frame.area(), show_sidebar);

    draw_tab_bar(frame, layout.top_bar, app, theme);

    draw_content_body(frame, layout.content, app, theme);

    if let Some(sidebar_area) = layout.sidebar {
        draw_sidebar(frame, sidebar_area, app, theme);
    }

    draw_status_bar(frame, layout.status_bar, app, theme);

    if app.help_overlay_open {
        let centered = overlays::centered_rect(50, 55, frame.area());
        overlays::draw_help_overlay(frame, centered, theme);
    }

    if app.action_palette_open {
        overlays::draw_action_palette(frame, frame.area(), app, theme);
    }

    if app.dispatch_menu_open {
        overlays::draw_dispatch_menu(frame, frame.area(), app, theme);
    }

    if app.command_palette_open {
        overlays::draw_command_palette(frame, frame.area(), app, theme, &app.command_palette_query);
    }

    if app.embedded_task.is_some() {
        overlays::draw_embedded_pane(frame, frame.area(), app, theme);
    }
}

fn draw_content_body(frame: &mut Frame, area: ratatui::layout::Rect, app: &App, theme: &Theme) {
    match app.tab_index {
        0 => tabs::draw_dashboard(frame, area, app, theme),
        1 => tabs::draw_daemon(frame, area, app, theme),
        2 => tabs::draw_discovery(frame, area, app, theme),
        3 => tabs::draw_mcp(frame, area, app, theme),
        4 => tabs::draw_skills(frame, area, app, theme),
        5 => tabs::draw_sessions(frame, area, app, theme),
        6 => tabs::draw_logs(frame, area, app, theme),
        7 => tabs::draw_knowledge(frame, area, app, theme),
        8 => tabs::draw_pulls(frame, area, app, theme),
        9 => tabs::draw_runtime(frame, area, app, theme),
        10 => tabs::draw_kanban(frame, area, app, theme),
        11 => tabs::draw_graph(frame, area, app, theme),
        12 => tabs::draw_cost(frame, area, app, theme),
        i if Some(i) == app.orchestrator_tab_index() => {
            tabs::draw_orchestrator(frame, area, app, theme)
        }
        i if Some(i) == app.review_tab_index() => tabs::draw_review(frame, area, app, theme),
        _ => {}
    }
}
