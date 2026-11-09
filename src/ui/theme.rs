//! Style tokens for the browser UI.
//!
//! Palette: Tokyo Night // WOPR - WarGames Edition
//! (see `wargames-tokyonight-color-palette.md`).

use ratatui::style::{Color, Modifier, Style};

/// Cursor rail and interactive accents (Terminal Cyan / Markdown headings).
pub const ACCENT: Color = Color::Rgb(0x5a, 0xc8, 0xff);
/// Folder titles (Terminal Blue).
pub const FOLDER: Color = Color::Rgb(0x3d, 0xa9, 0xff);
/// Relative dates (Comments).
pub const DATE: Color = Color::Rgb(0x3a, 0x4a, 0x6d);
/// Group labels such as `FOLDERS` (Comments).
pub const SECTION: Color = Color::Rgb(0x3a, 0x4a, 0x6d);
/// Header summary text, dimmed (Comments).
pub const SUMMARY: Color = Color::Rgb(0x3a, 0x4a, 0x6d);
/// Breadcrumb path, bright final segment (Editor Foreground).
pub const BREADCRUMB: Color = Color::Rgb(0xa8, 0xb6, 0xd4);
/// Breadcrumb parent directories, dimmed (Comments).
pub const BREADCRUMB_DIM: Color = Color::Rgb(0x3a, 0x4a, 0x6d);
/// Operation status text (Terminal Green).
pub const STATUS: Color = Color::Rgb(0x5c, 0xff, 0xc6);
/// Error text (Terminal Red).
pub const ERROR: Color = Color::Rgb(0xff, 0x5c, 0x6c);
/// Empty-state text (Markdown Text).
pub const EMPTY: Color = Color::Rgb(0x8a, 0x9a, 0xbf);
/// Empty-state key hints (Markdown links).
pub const EMPTY_KEY: Color = Color::Rgb(0x5c, 0xff, 0xc6);
/// Help bar and overlay descriptions (Markdown Text).
pub const HELP_TEXT: Color = Color::Rgb(0x8a, 0x9a, 0xbf);
/// Help key hints (Terminal Cyan).
pub const HELP_KEY: Color = Color::Rgb(0x5a, 0xc8, 0xff);
/// Help overlay border and title (Comments).
pub const HELP_BORDER: Color = Color::Rgb(0x3a, 0x4a, 0x6d);
/// Cursor row background (Terminal Black navy tint, softer than grey).
pub const CURSOR_BG: Color = Color::Rgb(0x1e, 0x27, 0x42);
/// Cursor row foreground (Terminal White).
pub const CURSOR_FG: Color = Color::Rgb(0xd6, 0xe2, 0xff);
/// Selected (but not cursor) row background (Editor Background night).
pub const MULTI_BG: Color = Color::Rgb(0x0a, 0x0f, 0x1e);
/// Folder titles on the cursor or a selected row (Terminal Cyan).
pub const SELECTED_FOLDER: Color = Color::Rgb(0x5a, 0xc8, 0xff);
/// Selection and clipboard gutter markers (Terminal Green).
pub const MARKER: Color = Color::Rgb(0x5c, 0xff, 0xc6);
/// Visual-mode and confirmation hints (Terminal Yellow).
pub const WARNING: Color = Color::Rgb(0xff, 0xcc, 0x66);
/// Dim prompt placeholder text (Comments).
pub const PLACEHOLDER: Color = Color::Rgb(0x3a, 0x4a, 0x6d);

pub fn brand() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn breadcrumb() -> Style {
    Style::default().fg(BREADCRUMB)
}

pub fn breadcrumb_dim() -> Style {
    Style::default().fg(BREADCRUMB_DIM)
}

pub fn summary() -> Style {
    Style::default().fg(SUMMARY)
}

pub fn section() -> Style {
    Style::default().fg(SECTION).add_modifier(Modifier::BOLD)
}

pub fn rail() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn marker() -> Style {
    Style::default().fg(MARKER).add_modifier(Modifier::BOLD)
}

pub fn multi_row() -> Style {
    Style::default().bg(MULTI_BG).add_modifier(Modifier::BOLD)
}

pub fn warning() -> Style {
    Style::default().fg(WARNING).add_modifier(Modifier::BOLD)
}

pub fn placeholder() -> Style {
    Style::default().fg(PLACEHOLDER)
}

pub fn prompt_cursor() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

pub fn cursor_row() -> Style {
    Style::default()
        .fg(CURSOR_FG)
        .bg(CURSOR_BG)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_folder() -> Style {
    Style::default().fg(SELECTED_FOLDER)
}

pub fn date() -> Style {
    Style::default().fg(DATE)
}

pub fn status() -> Style {
    Style::default().fg(STATUS)
}

pub fn error() -> Style {
    Style::default().fg(ERROR)
}

pub fn empty() -> Style {
    Style::default().fg(EMPTY)
}

pub fn empty_key() -> Style {
    Style::default().fg(EMPTY_KEY).add_modifier(Modifier::BOLD)
}

pub fn help_text() -> Style {
    Style::default().fg(HELP_TEXT)
}

pub fn help_key() -> Style {
    Style::default().fg(HELP_KEY)
}

pub fn help_border() -> Style {
    Style::default().fg(HELP_BORDER)
}

pub fn help_title() -> Style {
    Style::default().fg(HELP_KEY).add_modifier(Modifier::BOLD)
}
