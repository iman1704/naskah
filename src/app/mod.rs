//! Application state and the `Msg`/`Effect` update loop.

pub mod clipboard;
pub mod message;
mod prompt;
pub mod refresh;
mod selection;

mod update;

use std::collections::HashSet;
use std::path::PathBuf;

use crate::browser::Entry;
use crate::images::ImageStore;
use crate::reader::{self, ImagePicker, Reader};
use crate::zellij_images::DEFAULT_CELL;

pub use clipboard::Clipboard;
pub use message::{Effect, Msg};
pub use prompt::{Confirm, Prompt, PromptKind};

/// Rows reserved for the header, message line, and status line.
pub const CHROME_ROWS: u16 = 3;

/// A labeled group in the browser list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Folders,
    Notes,
}

impl Section {
    /// Uppercase label shown above the section.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Folders => "FOLDERS",
            Self::Notes => "NOTES",
        }
    }
}

/// One rendered row of the browser list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRow {
    Section(Section),
    Gap,
    Entry(usize),
}

/// The browser and reader UI state.
#[derive(Debug)]
pub struct App {
    /// Root directory the browser is confined to.
    pub root: PathBuf,
    /// Directory currently being browsed (always `root` or one of its descendants).
    pub notes_dir: PathBuf,
    /// Sorted entries for the current directory.
    pub entries: Vec<Entry>,
    /// Index into `entries`.
    pub cursor: usize,
    /// First rendered list row shown in the viewport.
    pub scroll: usize,
    /// Number of list rows the viewport can show.
    pub viewport: usize,
    /// Current terminal width, used to reflow the reader.
    pub terminal_width: u16,
    /// Current terminal height, used to size the reader.
    pub terminal_height: u16,
    /// The note open in the reader, or `None` while browsing.
    pub reader: Option<Reader>,
    /// Process-wide image store the reader prefetches through. The event loop
    /// drains its completions and refreshes the open reader.
    pub images: ImageStore,
    /// Shared Kitty image picker for the reader, detected once at startup.
    /// `None` keeps the text fallback (unsupported terminals).
    pub image_picker: Option<ImagePicker>,
    /// Whether the reader paints images itself for Zellij, whose Kitty
    /// support does not resolve unicode placeholders.
    pub zellij_painter: bool,
    /// Terminal cell size in pixels for the Zellij painter.
    pub zellij_cell: (u16, u16),
    /// Image ids transmitted by the last Zellij paint, deleted before the
    /// next paint or when the reader closes.
    pub zellij_ids: Vec<u32>,
    /// Transient operation status shown in the status row.
    pub status: Option<String>,
    /// Error shown in the row under the header.
    pub error: Option<String>,
    /// Selected entry paths (`Space` or a committed visual range).
    pub selection: HashSet<PathBuf>,
    /// Whether a `v` visual range is active.
    pub visual: bool,
    /// Cursor index where the visual range started.
    pub anchor: usize,
    /// Entries queued by copy (`y`) or cut (`x`).
    pub clipboard: Clipboard,
    /// Active status-row input for new notes, folders, and renames.
    pub prompt: Option<Prompt>,
    /// Entries waiting for a trash confirmation.
    pub confirm: Option<Confirm>,
    /// Whether the help overlay is open.
    pub help_visible: bool,
    /// The last watcher or refresh failure, shown in the message row while
    /// the last good listing stays on screen. Cleared by the next successful
    /// refresh or navigation.
    pub watcher_error: Option<String>,
    /// Set when the event loop should exit.
    pub should_quit: bool,
}

impl App {
    /// Create a browser for `notes_dir` with an initial listing.
    pub fn new(notes_dir: PathBuf, entries: Vec<Entry>) -> Self {
        Self {
            root: notes_dir.clone(),
            notes_dir,
            entries,
            cursor: 0,
            scroll: 0,
            viewport: 0,
            terminal_width: 80,
            terminal_height: 24,
            reader: None,
            images: ImageStore::shared(),
            image_picker: None,
            zellij_painter: false,
            zellij_cell: DEFAULT_CELL,
            zellij_ids: Vec::new(),
            status: None,
            error: None,
            selection: HashSet::new(),
            visual: false,
            anchor: 0,
            clipboard: Clipboard::default(),
            prompt: None,
            confirm: None,
            help_visible: false,
            watcher_error: None,
            should_quit: false,
        }
    }

    /// List rows with section labels and the gap between groups.
    pub fn rows(&self) -> Vec<ListRow> {
        list_rows(&self.entries)
    }

    /// Update the layout for a terminal of `width` columns and `height` rows.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.terminal_width = width;
        self.terminal_height = height;
        self.viewport = height.saturating_sub(CHROME_ROWS) as usize;
        self.ensure_cursor_visible();

        let resized = self
            .reader
            .as_mut()
            .map(|reader| reader.resize(reading_width(width), reading_height(height)));
        if let Some(Err(error)) = resized {
            self.reader = None;
            self.status = None;
            self.error = Some(error.to_string());
        }
    }

    /// Re-render the open reader so newly loaded images appear.
    ///
    /// The event loop calls this once after draining a batch of image
    /// completions. A render failure closes the reader and reports in the
    /// browser message row, like a resize failure.
    pub fn refresh_reader_images(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        if let Err(error) = reader.refresh_images() {
            self.reader = None;
            self.status = None;
            self.error = Some(error.to_string());
        }
    }

    /// Number of markdown notes in the current listing.
    pub fn note_count(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.is_dir).count()
    }

    /// One-based cursor position and total entry count.
    pub fn position(&self) -> (usize, usize) {
        if self.entries.is_empty() {
            (0, 0)
        } else {
            (self.cursor + 1, self.entries.len())
        }
    }

    fn cursor_entry(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    fn move_cursor_by(&mut self, delta: isize) {
        if self.entries.is_empty() {
            self.cursor = 0;
            return;
        }

        let last = self.entries.len() - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last as isize) as usize;
        self.ensure_cursor_visible();
    }

    fn page_down(&mut self) {
        self.move_cursor_by(self.viewport.max(1) as isize);
    }

    fn page_up(&mut self) {
        self.move_cursor_by(-(self.viewport.max(1) as isize));
    }

    fn top(&mut self) {
        self.cursor = 0;
        self.scroll = 0;
        self.ensure_cursor_visible();
    }

    fn bottom(&mut self) {
        self.cursor = self.entries.len().saturating_sub(1);
        self.ensure_cursor_visible();
    }

    fn cursor_row(&self) -> Option<usize> {
        self.rows()
            .iter()
            .position(|row| *row == ListRow::Entry(self.cursor))
    }

    fn ensure_cursor_visible(&mut self) {
        if self.viewport == 0 {
            self.scroll = 0;
            return;
        }

        let Some(row) = self.cursor_row() else {
            self.scroll = 0;
            return;
        };

        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + self.viewport {
            self.scroll = row + 1 - self.viewport;
        }

        let max_scroll = self.rows().len().saturating_sub(self.viewport);
        self.scroll = self.scroll.min(max_scroll);
    }
}

/// Reading column width for a terminal `width` cells wide.
pub fn reading_width(width: u16) -> u16 {
    width.clamp(1, reader::MAX_WIDTH)
}

/// Reading viewport height for a terminal `height` rows tall.
pub fn reading_height(height: u16) -> usize {
    height.saturating_sub(reader::CHROME_ROWS) as usize
}

fn list_rows(entries: &[Entry]) -> Vec<ListRow> {
    let folders: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.is_dir)
        .map(|(index, _)| index)
        .collect();
    let notes: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.is_dir)
        .map(|(index, _)| index)
        .collect();

    let mut rows = Vec::with_capacity(entries.len() + 3);
    if !folders.is_empty() {
        rows.push(ListRow::Section(Section::Folders));
        rows.extend(folders.into_iter().map(ListRow::Entry));
        if !notes.is_empty() {
            rows.push(ListRow::Gap);
        }
    }
    if !notes.is_empty() {
        rows.push(ListRow::Section(Section::Notes));
        rows.extend(notes.into_iter().map(ListRow::Entry));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            is_dir,
            modified: None,
        }
    }

    fn app(entries: Vec<Entry>) -> App {
        App::new(PathBuf::from("/notes"), entries)
    }

    #[test]
    fn rows_include_sections_and_the_gap() {
        let app = app(vec![entry("folder", true), entry("note.md", false)]);

        assert_eq!(
            app.rows(),
            [
                ListRow::Section(Section::Folders),
                ListRow::Entry(0),
                ListRow::Gap,
                ListRow::Section(Section::Notes),
                ListRow::Entry(1),
            ]
        );
    }

    #[test]
    fn rows_omit_an_empty_group() {
        let app = app(vec![entry("note.md", false)]);

        assert_eq!(
            app.rows(),
            [ListRow::Section(Section::Notes), ListRow::Entry(0)]
        );
    }

    #[test]
    fn scroll_accounts_for_section_labels_and_the_gap() {
        let mut app = app(vec![
            entry("one", true),
            entry("two", true),
            entry("first.md", false),
            entry("second.md", false),
        ]);
        app.resize(80, 6);

        app.bottom();

        assert_eq!(app.cursor, 3);
        assert_eq!(app.scroll, 4);
    }
}
