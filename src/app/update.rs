//! Key and event routing for the browser and reader.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Effect, Msg, reading_height, reading_width};
use crate::browser::Entry;
use crate::reader::Reader;

impl App {
    /// Apply one message and return effects for the event loop.
    pub fn update(&mut self, msg: Msg) -> Vec<Effect> {
        match msg {
            Msg::Key(key) => self.handle_key(key),
            Msg::Resize { width, height } => {
                self.resize(width, height);
                Vec::new()
            }
            Msg::DirectoryLoaded {
                path,
                entries,
                reselect,
            } => {
                self.apply_listing(path, entries, reselect);
                Vec::new()
            }
            Msg::DirectoryLoadFailed { error } => {
                self.error = Some(error.to_string());
                self.status = None;
                Vec::new()
            }
            Msg::WatcherEvent { dir } => {
                if dir != self.notes_dir {
                    return Vec::new();
                }
                vec![Effect::RefreshDirectory {
                    dir: self.notes_dir.clone(),
                }]
            }
            Msg::NoteChanged { path } => {
                self.apply_note_changed(&path);
                Vec::new()
            }
            Msg::RefreshLoaded { dir, entries } => {
                self.apply_refresh(&dir, entries);
                Vec::new()
            }
            Msg::RefreshFailed { error } => {
                self.refresh_failed(error);
                Vec::new()
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if self.reader.is_some() {
            return self.handle_reader_key(key);
        }
        if self.prompt.is_some() {
            return self.handle_prompt_key(key);
        }
        if self.confirm.is_some() {
            return self.handle_confirm_key(key);
        }

        if is_quit_key(&key) {
            self.should_quit = true;
            return Vec::new();
        }

        if self.help_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help_visible = false;
                self.status = None;
                self.error = None;
            }
            return Vec::new();
        }

        self.status = None;
        self.error = None;

        if self.visual && !is_visual_key(&key) {
            self.commit_visual();
        }

        match key.code {
            KeyCode::Char('?') => self.help_visible = true,
            KeyCode::Esc => {
                if self.cancel_browser() {
                    self.status = Some("cancelled".to_owned());
                }
            }
            KeyCode::Char(' ') => self.toggle_selection(),
            KeyCode::Char('v') => self.toggle_visual(),
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('x') => self.cut_selection(),
            KeyCode::Char('p') => return self.do_paste(),
            KeyCode::Char('d') => self.request_trash(),
            KeyCode::Char('a') => self.open_new_note(),
            KeyCode::Char('A') => self.open_new_folder(),
            KeyCode::Char('R') => self.open_rename(),
            KeyCode::Char('e') => return self.edit_effect(),
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor_by(-1),
            KeyCode::PageDown => self.page_down(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::Char('g') | KeyCode::Home => self.top(),
            KeyCode::Char('G') | KeyCode::End => self.bottom(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                return self.parent_effect();
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                return self.enter_effect();
            }
            _ => {}
        }

        Vec::new()
    }

    fn parent_effect(&self) -> Vec<Effect> {
        if self.notes_dir == self.root {
            return Vec::new();
        }

        let Some(parent) = self.notes_dir.parent() else {
            return Vec::new();
        };

        vec![Effect::LoadDirectory {
            path: parent.to_path_buf(),
            reselect: Some(self.notes_dir.clone()),
        }]
    }

    fn enter_effect(&mut self) -> Vec<Effect> {
        let Some(entry) = self.cursor_entry() else {
            return Vec::new();
        };
        let path = entry.path.clone();

        if entry.is_dir {
            vec![Effect::LoadDirectory {
                path,
                reselect: None,
            }]
        } else {
            self.open_reader(&path);
            Vec::new()
        }
    }

    /// Read and render a note into the reader. Read or render failures stay in
    /// the browser and appear in the message row.
    fn open_reader(&mut self, path: &Path) {
        let open = if self.zellij_painter {
            Reader::open_zellij(
                path,
                reading_width(self.terminal_width),
                reading_height(self.terminal_height),
                self.zellij_cell,
                self.images.clone(),
            )
        } else {
            Reader::open(
                path,
                reading_width(self.terminal_width),
                reading_height(self.terminal_height),
                self.image_picker.clone(),
                self.images.clone(),
            )
        };
        match open {
            Ok(reader) => {
                self.reader = Some(reader);
                self.status = None;
                self.error = None;
            }
            Err(error) => {
                self.status = None;
                self.error = Some(error.to_string());
            }
        }
    }

    /// Reader keys: movement scrolls the rendered note; `e` edits the note and
    /// returns to the reader; `q`/`h`/`esc` return to the browser with the
    /// cursor unchanged. Ctrl-C still quits. A shown `error` clears on the
    /// next non-quit key so the help row returns; `watcher_error` stays until
    /// the next successful refresh.
    fn handle_reader_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return Vec::new();
        }

        if matches!(
            key.code,
            KeyCode::Char('q')
                | KeyCode::Char('h')
                | KeyCode::Esc
                | KeyCode::Left
                | KeyCode::Backspace
        ) {
            self.reader = None;
            return Vec::new();
        }

        self.error = None;
        if matches!(key.code, KeyCode::Char('e')) && key.modifiers.is_empty() {
            let Some(reader) = self.reader.as_ref() else {
                return Vec::new();
            };
            return vec![Effect::OpenEditor {
                path: reader.path().to_path_buf(),
            }];
        }

        let Some(reader) = self.reader.as_mut() else {
            return Vec::new();
        };

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => reader.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => reader.scroll_by(-1),
            KeyCode::PageDown => reader.page_by(1),
            KeyCode::PageUp => reader.page_by(-1),
            KeyCode::Char('g') | KeyCode::Home => reader.top(),
            KeyCode::Char('G') | KeyCode::End => reader.bottom(),
            _ => {}
        }

        Vec::new()
    }

    /// Open the cursor entry in the external editor. Like rename, editing is
    /// restricted to the cursor entry; the event loop runs the editor and
    /// refreshes the listing when it exits.
    fn edit_effect(&mut self) -> Vec<Effect> {
        let Some(entry) = self.cursor_entry() else {
            self.status = Some("nothing selected".to_owned());
            return Vec::new();
        };
        if entry.is_dir {
            self.status = Some("cannot edit a folder".to_owned());
            return Vec::new();
        }
        vec![Effect::OpenEditor {
            path: entry.path.clone(),
        }]
    }

    fn apply_listing(&mut self, path: PathBuf, entries: Vec<Entry>, reselect: Option<PathBuf>) {
        self.cursor = reselect
            .and_then(|wanted| entries.iter().position(|entry| entry.path == wanted))
            .unwrap_or(0);
        self.entries = entries;
        self.notes_dir = path;
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        self.scroll = 0;
        self.status = None;
        self.error = None;
        // A successful navigation doubles as a refresh retry.
        self.watcher_error = None;
        // Selections belong to one listing, but the clipboard survives
        // navigation so entries can be pasted elsewhere.
        self.selection.clear();
        self.visual = false;
        self.anchor = 0;
        self.ensure_cursor_visible();
    }
}

/// Whether `key` belongs to the active visual range or its controls. Any
/// other key commits the range before running its action.
fn is_visual_key(key: &KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char('j')
            | KeyCode::Down
            | KeyCode::Char('k')
            | KeyCode::Up
            | KeyCode::Char('v')
            | KeyCode::Char(' ')
            | KeyCode::Esc
    )
}

fn is_quit_key(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => key.modifiers.is_empty(),
        KeyCode::Char('c') => key.modifiers.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}
