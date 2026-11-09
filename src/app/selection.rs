//! Multi-selection and the visual range.
//!
//! `Space` toggles the cursor entry. `v` starts a Vim-style range at the
//! cursor that `j`/`k` extend; pressing `v` again commits the range to the
//! selection. Copy and cut use the effective selection and fall back to the
//! cursor entry when nothing is selected.

use super::App;
use crate::browser::Entry;

impl App {
    /// Whether `index` lies between the visual anchor and the cursor.
    pub fn in_visual_range(&self, index: usize) -> bool {
        if !self.visual || self.entries.is_empty() {
            return false;
        }
        let (lo, hi) = if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        index >= lo && index <= hi
    }

    /// Whether the entry at `index` is selected, either with `Space` or
    /// through the active visual range.
    pub fn entry_selected(&self, index: usize) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        self.in_visual_range(index) || self.selection.contains(&entry.path)
    }

    /// The effective selection in listing order.
    pub fn selected_entries(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(index, _)| self.entry_selected(*index))
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    /// The selection, falling back to the cursor entry when nothing is
    /// selected. Copy, cut, and trash act on these entries, while rename,
    /// edit, and open always use the cursor entry.
    pub fn operation_entries(&self) -> Vec<Entry> {
        let selected = self.selected_entries();
        if selected.is_empty() {
            self.cursor_entry().cloned().into_iter().collect()
        } else {
            selected
        }
    }

    /// Add or remove the cursor entry from the selection. Ignored while a
    /// visual range is active.
    pub fn toggle_selection(&mut self) {
        if self.visual {
            return;
        }
        let Some(entry) = self.cursor_entry().cloned() else {
            self.status = Some("nothing selected".to_owned());
            return;
        };
        if !self.selection.remove(&entry.path) {
            self.selection.insert(entry.path);
        }
    }

    /// Start a visual range at the cursor, or commit the active range.
    pub fn toggle_visual(&mut self) {
        if self.visual {
            self.commit_visual();
            return;
        }
        if self.cursor_entry().is_none() {
            self.status = Some("nothing selected".to_owned());
            return;
        }
        self.visual = true;
        self.anchor = self.cursor;
    }

    /// Add every entry in the active visual range to the selection and leave
    /// visual mode. Other keys call this before running their action so the
    /// range never leaks into unrelated behavior.
    pub fn commit_visual(&mut self) {
        if !self.visual {
            return;
        }
        self.visual = false;
        if self.entries.is_empty() {
            self.anchor = 0;
            return;
        }
        let (lo, hi) = if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        };
        let last = self.entries.len() - 1;
        let (lo, hi) = (lo.min(last), hi.min(last));
        for entry in &self.entries[lo..=hi] {
            self.selection.insert(entry.path.clone());
        }
        self.anchor = 0;
    }

    /// Clear the visual range, multi-selection, and clipboard in one press.
    /// Returns whether anything was cleared, so `esc` can report `cancelled`
    /// only when it actually discards state.
    pub fn cancel_browser(&mut self) -> bool {
        if !self.visual && self.selection.is_empty() && !self.clipboard.active() {
            return false;
        }
        self.selection.clear();
        self.visual = false;
        self.anchor = 0;
        self.clipboard.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            is_dir: false,
            modified: None,
        }
    }

    fn app() -> App {
        App::new(
            PathBuf::from("/notes"),
            vec![entry("a.md"), entry("b.md"), entry("c.md")],
        )
    }

    #[test]
    fn space_toggles_the_cursor_entry() {
        let mut app = app();

        app.toggle_selection();
        assert!(app.entry_selected(0));

        app.toggle_selection();
        assert!(!app.entry_selected(0));
    }

    #[test]
    fn space_is_ignored_while_visual() {
        let mut app = app();

        app.toggle_visual();
        app.toggle_selection();

        assert!(app.selection.is_empty());
        assert!(app.entry_selected(0));
        assert!(!app.entry_selected(1));
    }

    #[test]
    fn visual_range_commits_on_second_v() {
        let mut app = app();

        app.toggle_visual();
        app.move_cursor_by(2);
        app.toggle_visual();

        assert!(!app.visual);
        assert!(app.entry_selected(0));
        assert!(app.entry_selected(1));
        assert!(app.entry_selected(2));
    }

    #[test]
    fn other_keys_commit_the_range_first() {
        let mut app = app();

        app.toggle_visual();
        app.move_cursor_by(1);
        app.commit_visual();
        app.move_cursor_by(1);

        assert!(!app.visual);
        assert!(app.entry_selected(0));
        assert!(app.entry_selected(1));
        assert!(!app.entry_selected(2));
    }

    #[test]
    fn operation_entries_fall_back_to_the_cursor() {
        let mut app = app();
        app.move_cursor_by(1);

        assert_eq!(
            app.operation_entries(),
            vec![entry("b.md")],
            "with no selection the cursor entry is used",
        );

        app.toggle_selection();
        app.move_cursor_by(1);

        assert_eq!(
            app.operation_entries(),
            vec![entry("b.md")],
            "with a selection the cursor entry alone is not used",
        );
    }
}
