//! Header with the brand, breadcrumb, and listing summary.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui::{text, theme};

const BRAND: &str = "Naskah";
const MIN_SUMMARY_WIDTH: u16 = 40;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (position, total) = app.position();
    let summary = summary(app, position, total);
    let summary_width = UnicodeWidthStr::width(summary.as_str()) as u16;
    let show_summary = area.width >= MIN_SUMMARY_WIDTH && summary_width < area.width;

    let left_width = if show_summary {
        area.width.saturating_sub(summary_width + 2)
    } else {
        area.width
    };
    let left = Rect {
        width: left_width,
        ..area
    };

    let brand_width = UnicodeWidthStr::width(BRAND) as u16;
    let gap = if left_width > brand_width { 2 } else { 0 };
    let breadcrumb_width = left_width.saturating_sub(brand_width + gap) as usize;
    let breadcrumb = text::display_path(app.notes_dir.as_path());
    let breadcrumb = text::truncate_left(&breadcrumb, breadcrumb_width);

    let mut spans = vec![Span::styled(
        text::truncate_right(BRAND, left_width as usize),
        theme::brand(),
    )];
    if !breadcrumb.is_empty() {
        spans.push(Span::raw("  "));
        let (head, tail) = text::split_last_segment(&breadcrumb);
        if !head.is_empty() {
            spans.push(Span::styled(head.to_owned(), theme::breadcrumb_dim()));
        }
        spans.push(Span::styled(tail.to_owned(), theme::breadcrumb()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), left);

    if show_summary {
        let summary_area = Rect {
            x: area.right().saturating_sub(summary_width),
            width: summary_width,
            ..area
        };
        frame.render_widget(
            Paragraph::new(Span::styled(summary, theme::summary())).alignment(Alignment::Right),
            summary_area,
        );
    }
}

fn summary(app: &App, position: usize, total: usize) -> String {
    let mut parts = Vec::new();
    if app.visual {
        parts.push("VISUAL".to_owned());
    }
    if app.clipboard.active() {
        let verb = if app.clipboard.cut { "cut" } else { "copy" };
        parts.push(format!("{verb} {}", app.clipboard.entries.len()));
    }
    if !app.visual && !app.selection.is_empty() {
        parts.push(format!("{} sel", app.selection.len()));
    }
    parts.push(note_label(app.note_count()));
    parts.push(format!("{position}/{total}"));
    parts.join(" · ")
}

fn note_label(count: usize) -> String {
    if count == 1 {
        "1 note".to_owned()
    } else {
        format!("{count} notes")
    }
}
