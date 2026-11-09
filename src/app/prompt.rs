//! New note/folder/rename prompts, trash confirmation, and the paste and
//! trash flows they trigger.
//!
//! Prompts live in the status row; validation errors appear under the header
//! and keep the prompt open. Filesystem work runs synchronously and refreshes
//! through `Effect::LoadDirectory`, which clears the selection but leaves the
//! clipboard to the path fixups below.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{App, Clipboard, Effect};
use crate::browser::Entry;
use crate::files;

/// Which input the status row is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    NewNote,
    NewFolder,
    Rename,
}

/// An active status-row input. `cursor` is a byte index into `value`, always
/// on a character boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    /// Rename source; empty for new notes and folders.
    pub target: PathBuf,
    pub value: String,
    pub cursor: usize,
}

/// Entries waiting for a `y`/`n` trash confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub entries: Vec<Entry>,
}

impl Prompt {
    fn new(kind: PromptKind, target: PathBuf, value: String) -> Self {
        let cursor = value.len();
        Self {
            kind,
            target,
            value,
            cursor,
        }
    }

    /// The label shown before the input.
    pub fn label(&self) -> &'static str {
        match self.kind {
            PromptKind::NewNote => "New note: ",
            PromptKind::NewFolder => "New folder: ",
            PromptKind::Rename => "Rename: ",
        }
    }

    /// The dim hint shown while the input is empty.
    pub fn placeholder(&self) -> &'static str {
        match self.kind {
            PromptKind::NewNote => "note.md",
            PromptKind::NewFolder => "folder",
            PromptKind::Rename => "",
        }
    }

    fn insert(&mut self, c: char) {
        self.value.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = prev_boundary(&self.value, self.cursor);
        self.value.drain(prev..self.cursor);
        self.cursor = prev;
    }

    fn delete(&mut self) {
        if self.cursor >= self.value.len() {
            return;
        }
        let next = next_boundary(&self.value, self.cursor);
        self.value.drain(self.cursor..next);
    }

    fn move_left(&mut self) {
        self.cursor = prev_boundary(&self.value, self.cursor);
    }

    fn move_right(&mut self) {
        self.cursor = next_boundary(&self.value, self.cursor);
    }
}

impl Confirm {
    /// The `Trash "name"? (y/N)` question for the status row.
    pub fn question(&self) -> String {
        if self.entries.len() == 1 {
            let entry = &self.entries[0];
            return format!("Trash {:?}? (y/N)", display_name(&entry.name, entry.is_dir));
        }
        format!("Trash {} items? (y/N)", self.entries.len())
    }
}

/// The previous character boundary at or before `index`.
fn prev_boundary(value: &str, index: usize) -> usize {
    (0..index)
        .rev()
        .find(|&i| value.is_char_boundary(i))
        .unwrap_or(0)
}

/// The next character boundary after `index`.
fn next_boundary(value: &str, index: usize) -> usize {
    ((index + 1)..=value.len())
        .find(|&i| value.is_char_boundary(i))
        .unwrap_or(value.len())
}

/// Append a slash to directories in status messages.
fn display_name(name: &str, is_dir: bool) -> String {
    if is_dir {
        format!("{name}/")
    } else {
        name.to_owned()
    }
}

impl App {
    /// Open the new-note prompt in the status row.
    pub fn open_new_note(&mut self) {
        self.prompt = Some(Prompt::new(
            PromptKind::NewNote,
            PathBuf::new(),
            String::new(),
        ));
        self.status = None;
        self.error = None;
    }

    /// Open the new-folder prompt in the status row.
    pub fn open_new_folder(&mut self) {
        self.prompt = Some(Prompt::new(
            PromptKind::NewFolder,
            PathBuf::new(),
            String::new(),
        ));
        self.status = None;
        self.error = None;
    }

    /// Open the rename prompt prefilled with the cursor entry's name.
    pub fn open_rename(&mut self) {
        let Some(entry) = self.cursor_entry().cloned() else {
            self.status = Some("nothing selected".to_owned());
            return;
        };
        self.prompt = Some(Prompt::new(PromptKind::Rename, entry.path, entry.name));
        self.status = None;
        self.error = None;
    }

    /// Route one key to the active prompt. Typing clears the validation
    /// error; `esc` cancels; `enter` submits.
    pub fn handle_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if is_quit_key(&key) {
            self.should_quit = true;
            return Vec::new();
        }
        let Some(prompt) = self.prompt.as_mut() else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.error = None;
            }
            KeyCode::Enter => return self.submit_prompt(),
            KeyCode::Backspace => {
                prompt.backspace();
                self.error = None;
            }
            KeyCode::Delete => {
                prompt.delete();
                self.error = None;
            }
            KeyCode::Left => prompt.move_left(),
            KeyCode::Right => prompt.move_right(),
            KeyCode::Home => prompt.cursor = 0,
            KeyCode::End => prompt.cursor = prompt.value.len(),
            KeyCode::Char(c) if is_typing(&key) => {
                prompt.insert(c);
                self.error = None;
            }
            _ => {}
        }
        Vec::new()
    }

    /// Validate the prompt input and run the create or rename, refreshing the
    /// listing on success. Validation failures keep the prompt open.
    fn submit_prompt(&mut self) -> Vec<Effect> {
        let Some(prompt) = self.prompt.clone() else {
            return Vec::new();
        };
        if let Err(error) = files::validate_name(&prompt.value) {
            self.error = Some(error.to_string());
            return Vec::new();
        }
        let name = prompt.value.trim();
        let result = match prompt.kind {
            PromptKind::NewNote => files::create_note(&self.notes_dir, name),
            PromptKind::NewFolder => files::create_folder(&self.notes_dir, name),
            PromptKind::Rename => files::rename(&prompt.target, name),
        };
        let path = match result {
            Ok(path) => path,
            Err(error) => {
                self.prompt = None;
                self.status = Some(error.to_string());
                return Vec::new();
            }
        };
        self.prompt = None;
        let base = file_base(&path);
        match prompt.kind {
            PromptKind::NewNote => self.status = Some(format!("created {base}")),
            PromptKind::NewFolder => self.status = Some(format!("created {base}/")),
            PromptKind::Rename => {
                if path == prompt.target {
                    self.status = Some("name unchanged".to_owned());
                    return Vec::new();
                }
                self.rename_clipboard_paths(&prompt.target, &path);
                let old = prompt
                    .target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.status = Some(format!("renamed {old} to {base}"));
            }
        }
        vec![Effect::LoadDirectory {
            path: self.notes_dir.clone(),
            reselect: Some(path),
        }]
    }

    /// Ask for confirmation before moving the effective selection to trash.
    pub fn request_trash(&mut self) {
        self.commit_visual();
        let entries = self.operation_entries();
        if entries.is_empty() {
            self.status = Some("nothing selected".to_owned());
            return;
        }
        self.confirm = Some(Confirm { entries });
    }

    /// Route one key to the trash confirmation: `y` trashes, `n` and `esc`
    /// cancel, anything else is ignored so the question stays open.
    pub fn handle_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if is_quit_key(&key) {
            self.should_quit = true;
            return Vec::new();
        }
        match key.code {
            KeyCode::Esc => {
                self.confirm = None;
                self.status = Some("cancelled".to_owned());
            }
            KeyCode::Char('n') | KeyCode::Char('N') if is_plain(&key) => {
                self.confirm = None;
                self.status = Some("cancelled".to_owned());
            }
            KeyCode::Char('y') | KeyCode::Char('Y') if is_plain(&key) => {
                let entries = self.confirm.take().map(|confirm| confirm.entries);
                if let Some(entries) = entries {
                    return self.do_trash(entries);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// Move every confirmed entry to the trash, continuing after failures and
    /// reporting a partial-success summary with the first error.
    fn do_trash(&mut self, entries: Vec<Entry>) -> Vec<Effect> {
        let mut removed = HashSet::new();
        let mut succeeded = 0;
        let mut first_err = None;
        for entry in &entries {
            match files::trash(&entry.path) {
                Ok(_) => {
                    succeeded += 1;
                    removed.insert(entry.path.clone());
                    self.forget_clipboard_path(&entry.path);
                }
                Err(error) => {
                    if error.is_not_found() {
                        self.forget_clipboard_path(&entry.path);
                    }
                    if first_err.is_none() {
                        first_err = Some(error.to_string());
                    }
                }
            }
        }
        let mut effects = Vec::new();
        if succeeded > 0 {
            effects.push(Effect::LoadDirectory {
                path: self.notes_dir.clone(),
                reselect: self.selection_after_trash(&removed),
            });
        }
        if let Some(error) = first_err {
            self.status = Some(format!(
                "trashed {succeeded} of {} items: {error}",
                entries.len()
            ));
        } else if entries.len() == 1 {
            let entry = &entries[0];
            self.status = Some(format!(
                "trashed {}",
                display_name(&entry.name, entry.is_dir)
            ));
        } else {
            self.status = Some(format!("trashed {succeeded} items"));
        }
        effects
    }

    /// Copy or move every clipboard entry into the current directory. Missing
    /// sources are dropped; other failures keep their entry for another try.
    /// Copy entries stay queued for repeated pastes, while moved cut entries
    /// leave the clipboard.
    pub fn do_paste(&mut self) -> Vec<Effect> {
        if !self.clipboard.active() {
            self.status = Some("clipboard is empty".to_owned());
            return Vec::new();
        }
        self.error = None;
        let cut = self.clipboard.cut;
        if cut && clipboard_in_dir(&self.clipboard.entries, &self.notes_dir) {
            self.status = Some(already_here_status(&self.clipboard.entries));
            return Vec::new();
        }
        let verb = if cut { "moved" } else { "copied" };
        let total = self.clipboard.entries.len();
        let single = self.clipboard.entries.first().cloned();
        let mut succeeded = 0;
        let mut first_err = None;
        let mut last_dest = None;
        let mut remaining = Vec::with_capacity(total);
        for entry in std::mem::take(&mut self.clipboard.entries) {
            if cut && entry_in_dir(&entry, &self.notes_dir) {
                remaining.push(entry);
                continue;
            }
            let result = if cut {
                files::move_entry(&entry.path, &self.notes_dir)
            } else {
                files::copy_entry(&entry.path, &self.notes_dir)
            };
            match result {
                Ok(dest) => {
                    succeeded += 1;
                    last_dest = Some(dest);
                    if !cut {
                        remaining.push(entry);
                    }
                }
                Err(error) => {
                    let missing = error.is_not_found();
                    if first_err.is_none() {
                        first_err = Some(error.to_string());
                    }
                    if !missing {
                        remaining.push(entry);
                    }
                }
            }
        }
        self.clipboard = Clipboard {
            entries: remaining,
            cut,
        };
        let mut effects = Vec::new();
        if succeeded > 0 {
            effects.push(Effect::LoadDirectory {
                path: self.notes_dir.clone(),
                reselect: last_dest.clone(),
            });
        }
        if let Some(error) = first_err {
            self.status = Some(format!("{verb} {succeeded} of {total} items: {error}"));
        } else if total == 1 {
            let src = single.expect("one queued entry");
            let dest_name = last_dest.as_deref().map(file_base).unwrap_or_default();
            if dest_name != src.name {
                self.status = Some(format!(
                    "{verb} {} to {}",
                    display_name(&src.name, src.is_dir),
                    display_name(&dest_name, src.is_dir)
                ));
            } else {
                self.status = Some(format!("{verb} {}", display_name(&src.name, src.is_dir)));
            }
        } else {
            self.status = Some(format!("{verb} {succeeded} items"));
        }
        effects
    }

    /// Follow a rename in the clipboard, both for the renamed entry itself
    /// and for queued descendants of a renamed folder.
    fn rename_clipboard_paths(&mut self, old: &Path, new: &Path) {
        if !self.clipboard.active() {
            return;
        }
        for entry in &mut self.clipboard.entries {
            if entry.path == old {
                entry.path = new.to_path_buf();
                if let Some(name) = new.file_name() {
                    entry.name = name.to_string_lossy().into_owned();
                }
                continue;
            }
            if let Ok(rel) = entry.path.strip_prefix(old) {
                entry.path = new.join(rel);
            }
        }
    }

    /// Drop the trashed path and its descendants from the clipboard.
    fn forget_clipboard_path(&mut self, path: &Path) {
        if !self.clipboard.active() {
            return;
        }
        self.clipboard
            .entries
            .retain(|entry| !files::is_within(path, &entry.path));
        if self.clipboard.entries.is_empty() {
            self.clipboard = Clipboard::default();
        }
    }

    /// Pick the entry the cursor lands on after a bulk trash: the nearest
    /// remaining entry at or after the cursor, else the nearest before it.
    fn selection_after_trash(&self, removed: &HashSet<PathBuf>) -> Option<PathBuf> {
        (self.cursor..self.entries.len())
            .chain((0..self.cursor).rev())
            .map(|index| &self.entries[index])
            .find(|entry| !removed.contains(&entry.path))
            .map(|entry| entry.path.clone())
    }
}

/// Whether `key` quits the app from a prompt or confirmation (Ctrl-C only;
/// plain `q` types into the prompt).
fn is_quit_key(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// Whether `key` is a plain typing or answer key: any `Char` without Ctrl or
/// Alt held. Shift is allowed so shifted letters still type and `Y`/`N` still
/// answer on terminals that report the Shift modifier.
fn is_typing(key: &KeyEvent) -> bool {
    !key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Whether `key` is a plain `y`/`n` answer, without Ctrl or Alt held.
fn is_plain(key: &KeyEvent) -> bool {
    is_typing(key)
}

/// Whether every queued entry already lives in `dir`.
fn clipboard_in_dir(entries: &[Entry], dir: &Path) -> bool {
    !entries.is_empty() && entries.iter().all(|entry| entry_in_dir(entry, dir))
}

/// Whether `entry` lives directly in `dir`.
fn entry_in_dir(entry: &Entry, dir: &Path) -> bool {
    entry.path.parent().is_some_and(|parent| parent == dir)
}

/// Describe a cut paste into the source directory.
fn already_here_status(entries: &[Entry]) -> String {
    if entries.len() == 1 {
        let entry = &entries[0];
        return format!(
            "{} is already in this folder",
            display_name(&entry.name, entry.is_dir)
        );
    }
    format!("{} items are already in this folder", entries.len())
}

/// The lossy file name of `path`.
fn file_base(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}
