//! Grouped folder and note list with cursor, dates, and empty state.

use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local};
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, ListRow, Section};
use crate::browser::Entry;
use crate::ui::{text, theme};

const PREFIX_WIDTH: usize = 4;
const SHOW_DATES_MIN_WIDTH: u16 = 44;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if app.entries.is_empty() {
        draw_empty(frame, area);
        return;
    }

    let rows = app.rows();
    let visible = rows.iter().skip(app.scroll).take(area.height as usize);
    for (offset, row) in visible.enumerate() {
        let row_area = Rect::new(area.x, area.y.saturating_add(offset as u16), area.width, 1);
        match row {
            ListRow::Section(section) => draw_section(frame, *section, row_area),
            ListRow::Gap => {}
            ListRow::Entry(index) => draw_entry(frame, app, *index, row_area),
        }
    }
}

fn draw_section(frame: &mut Frame, section: Section, area: Rect) {
    let label = format!(" {}", section.label());
    frame.render_widget(Paragraph::new(Span::styled(label, theme::section())), area);
}

fn draw_entry(frame: &mut Frame, app: &App, index: usize, area: Rect) {
    let Some(entry) = app.entries.get(index) else {
        return;
    };
    let is_cursor = index == app.cursor;
    let selected = app.entry_selected(index);
    let state = entry_state(app, index, entry);
    // The cursor owns the first gutter cell, so a selected or queued
    // entry's icon moves to the second cell. Clipboard icons take
    // precedence over the selection mark.
    let (first, second) = if is_cursor {
        ("▌", state.unwrap_or(" "))
    } else {
        (state.unwrap_or(" "), " ")
    };
    let base = if is_cursor {
        theme::cursor_row()
    } else if selected {
        theme::multi_row()
    } else {
        Style::default()
    };
    let gutter_style = if is_cursor {
        base.patch(theme::rail())
    } else if state.is_some() {
        base.patch(theme::marker())
    } else {
        base
    };

    if is_cursor {
        let tint = Rect {
            x: area.x.saturating_add(1),
            y: area.y,
            width: area.width.saturating_sub(1),
            height: 1,
        };
        frame.render_widget(Block::default().style(base), tint);
    }

    let date = date_label(entry, area.width);
    let date_width = date
        .as_ref()
        .map_or(0, |date| UnicodeWidthStr::width(date.as_str()));
    let date_gap = if date.is_some() { 2 } else { 0 };
    let title_width = (area.width as usize).saturating_sub(PREFIX_WIDTH + date_width + date_gap);
    let title = text::truncate_right(&entry_title(entry), title_width);
    let used = PREFIX_WIDTH + UnicodeWidthStr::width(title.as_str()) + date_width + date_gap;
    let padding = (area.width as usize).saturating_sub(used);

    let rail = first;
    let tag = second;
    let title_style = if entry.is_dir {
        if is_cursor || selected {
            base.patch(theme::selected_folder())
        } else {
            base.patch(Style::default().fg(theme::FOLDER))
        }
    } else {
        base
    };

    let mut spans = vec![
        Span::styled(rail, gutter_style),
        Span::styled(tag, gutter_style),
        Span::styled("  ", base),
        Span::styled(title, title_style),
    ];

    if let Some(date) = date {
        spans.push(Span::styled(" ".repeat(padding + date_gap), base));
        spans.push(Span::styled(
            date,
            base.patch(Style::default().fg(theme::DATE)),
        ));
    } else {
        spans.push(Span::styled(" ".repeat(padding), base));
    }

    frame.render_widget(Line::from(spans), area);
}

fn draw_empty(frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("no markdown notes here — ", theme::empty()),
        Span::styled("a new note", theme::empty_key()),
        Span::styled(" · ", theme::empty()),
        Span::styled("A new folder", theme::empty_key()),
    ]);
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), row);
}

fn entry_state(app: &App, index: usize, entry: &Entry) -> Option<&'static str> {
    if app.clipboard.has(&entry.path) {
        Some(if app.clipboard.cut { "✂" } else { "⧉" })
    } else if app.entry_selected(index) {
        Some("✓")
    } else {
        None
    }
}

fn entry_title(entry: &Entry) -> String {
    if entry.is_dir {
        return entry.name.clone();
    }

    Path::new(&entry.name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&entry.name)
        .to_owned()
}

fn date_label(entry: &Entry, width: u16) -> Option<String> {
    if entry.is_dir || width < SHOW_DATES_MIN_WIDTH {
        return None;
    }
    entry.modified.map(relative_date)
}

fn relative_date(modified: SystemTime) -> String {
    relative_date_at(DateTime::<Local>::from(modified), Local::now())
}

fn relative_date_at(note: DateTime<Local>, now: DateTime<Local>) -> String {
    let note_date = note.date_naive();
    let today = now.date_naive();

    match today.signed_duration_since(note_date).num_days() {
        0 => "Today".to_owned(),
        1 => "Yesterday".to_owned(),
        2..=6 => note_date.format("%a").to_string(),
        _ if note_date.year() == today.year() => note_date.format("%b %d").to_string(),
        _ => note_date.format("%Y-%m-%d").to_string(),
    }
}
