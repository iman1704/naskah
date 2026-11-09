//! Rendering for the browser and reader.

pub mod browser;
pub mod header;
pub mod help;
pub mod layout;
pub mod prompt;
pub mod reader;
pub mod text;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;

/// Draw one frame: the reader when a note is open, otherwise the browser.
pub fn draw(frame: &mut Frame, app: &mut App) {
    if let Some(reader) = app.reader.as_mut() {
        let error = app.error.as_deref().or(app.watcher_error.as_deref());
        reader::draw(frame, reader, error, frame.area());
        return;
    }

    let areas = layout::split(frame.area());

    header::draw(frame, app, areas.header);
    draw_message(frame, app, areas.message);
    browser::draw(frame, app, areas.list);
    draw_status(frame, app, areas.status);

    if app.help_visible {
        help::draw(frame, frame.area());
    }
}

fn draw_message(frame: &mut Frame, app: &App, area: Rect) {
    let message = app.error.as_ref().or(app.watcher_error.as_ref());
    let Some(message) = message else {
        return;
    };
    let line = Line::from(vec![
        Span::styled("! ", theme::error()),
        Span::styled(message.clone(), theme::error()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(prompt) = &app.prompt {
        prompt::draw_prompt(frame, prompt, area);
        return;
    }
    if let Some(confirm) = &app.confirm {
        prompt::draw_confirm(frame, confirm, area);
        return;
    }
    if app.visual {
        frame.render_widget(
            Paragraph::new(Span::styled(
                " VISUAL · j/k extend · v commit · esc cancel",
                theme::warning(),
            )),
            area,
        );
        return;
    }
    if let Some(status) = &app.status {
        frame.render_widget(
            Paragraph::new(Span::styled(status.clone(), theme::status())),
            area,
        );
        return;
    }

    frame.render_widget(Paragraph::new(help_bar()), area);
}

fn help_bar() -> Line<'static> {
    Line::from(vec![
        Span::styled("? help", theme::help_key()),
        Span::styled("  j/k move", theme::help_text()),
        Span::styled("  h/l parent/open", theme::help_text()),
        Span::styled("  g/G top/bottom", theme::help_text()),
        Span::styled("  q quit", theme::help_text()),
    ])
}
