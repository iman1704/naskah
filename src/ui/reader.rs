//! Reader screen: a centered markdown column with a header and help row.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::reader::{self, Reader};
use crate::ui::{text, theme};

const BRAND: &str = "Naskah";
const PERCENT_GAP: u16 = 2;

/// Content column rectangle for a fullscreen area: the centered reading
/// column below the header. Single source of truth for drawing and painting.
pub fn content_rect(area: Rect) -> Rect {
    let width = area.width.min(reader::MAX_WIDTH);
    let x = area.x + (area.width - width) / 2;
    Rect {
        x,
        y: area.y + 1,
        width,
        height: area.height.saturating_sub(reader::CHROME_ROWS),
    }
}

/// Draw the reader over the whole terminal area.
///
/// `error` is the browser `error` or `watcher_error` to show in the help row
/// while the reader is open; the browser message row is hidden behind the
/// reader, so without this watch and editor failures would be invisible.
pub fn draw(frame: &mut Frame, reader: &mut Reader, error: Option<&str>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let [header, _, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    let column = content_rect(area);

    draw_header(
        frame,
        reader,
        Rect {
            x: column.x,
            width: column.width,
            ..header
        },
    );
    draw_content(frame, reader, column);
    draw_help(
        frame,
        error,
        Rect {
            x: column.x,
            width: column.width,
            ..help
        },
    );
}

fn draw_header(frame: &mut Frame, reader: &Reader, area: Rect) {
    if area.width == 0 {
        return;
    }

    let percent = format!("{}%", reader.percent());
    let percent_width = UnicodeWidthStr::width(percent.as_str()) as u16;
    let show_percent = area.width > percent_width + PERCENT_GAP;
    let left_width = if show_percent {
        area.width.saturating_sub(percent_width + PERCENT_GAP)
    } else {
        area.width
    };

    let brand_width = UnicodeWidthStr::width(BRAND) as u16;
    let gap = if left_width > brand_width + 2 { 2 } else { 0 };
    let path_width = left_width.saturating_sub(brand_width + gap) as usize;
    let path = text::truncate_left(&text::display_path(reader.path()), path_width);

    let mut spans = vec![Span::styled(BRAND, theme::brand())];
    if !path.is_empty() {
        spans.push(Span::raw("  "));
        let (head, tail) = text::split_last_segment(&path);
        if !head.is_empty() {
            spans.push(Span::styled(head.to_owned(), theme::breadcrumb_dim()));
        }
        spans.push(Span::styled(tail.to_owned(), theme::breadcrumb()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            width: left_width,
            ..area
        },
    );

    if show_percent {
        let percent_area = Rect {
            x: area.right().saturating_sub(percent_width),
            width: percent_width,
            ..area
        };
        frame.render_widget(
            Paragraph::new(Span::styled(percent, theme::summary())).alignment(Alignment::Right),
            percent_area,
        );
    }
}

fn draw_content(frame: &mut Frame, reader: &mut Reader, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if reader.has_images() {
        reader.draw(frame, area);
        return;
    }

    let scroll = u16::try_from(reader.scroll()).unwrap_or(u16::MAX);
    frame.render_widget(
        Paragraph::new(reader.content().clone()).scroll((scroll, 0)),
        area,
    );
}

fn draw_help(frame: &mut Frame, error: Option<&str>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if let Some(error) = error {
        let message = text::truncate_right(&format!("! {error}"), area.width as usize);
        frame.render_widget(Paragraph::new(Span::styled(message, theme::error())), area);
        return;
    }

    let line = Line::from(vec![
        Span::styled("j/k", theme::help_key()),
        Span::styled(" scroll", theme::help_text()),
        Span::styled("  PgUp/PgDn", theme::help_key()),
        Span::styled(" page", theme::help_text()),
        Span::styled("  g/G", theme::help_key()),
        Span::styled(" top/bottom", theme::help_text()),
        Span::styled("  e", theme::help_key()),
        Span::styled(" edit", theme::help_text()),
        Span::styled("  q/h/esc", theme::help_key()),
        Span::styled(" back", theme::help_text()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
