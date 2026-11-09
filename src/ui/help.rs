//! Centered help overlay with the current key bindings.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::theme;

pub fn draw(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let width = area.width.saturating_sub(4).min(54);
    let height = area.height.saturating_sub(2).min(22);
    if width < 8 || height < 4 {
        return;
    }

    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::help_border())
        .title(Line::from(Span::styled(" Help ", theme::help_title())));

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(help_lines()).block(block), popup);
}

fn help_lines() -> Vec<Line<'static>> {
    vec![
        line("j / ↓", "move down"),
        line("k / ↑", "move up"),
        line("PgDn / PgUp", "page down / up"),
        line("g / G", "top / bottom"),
        line("h / ← / ⌫", "parent folder"),
        line("l / → / enter", "open folder"),
        line("space", "select entry"),
        line("v", "visual range"),
        line("y / x", "copy / cut"),
        line("p", "paste"),
        line("d", "trash"),
        line("a / A", "new note / folder"),
        line("R", "rename"),
        line("esc", "clear selection"),
        line("?", "close help"),
        line("q", "quit"),
    ]
}

fn line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<15}"), theme::help_key()),
        Span::styled(description.to_owned(), theme::help_text()),
    ])
}
