//! Listing reconciliation for live refreshes.
//!
//! Watcher events and editor exits re-read the current directory without the
//! navigation reset: the cursor stays on its entry, surviving selections are
//! kept, and the clipboard, prompts, and help overlay are left untouched. When
//! the entry set is unchanged only modification times are updated, so note
//! dates stay fresh without moving anything.

use std::path::Path;

use super::App;
use crate::browser::Entry;

impl App {
    /// Reconcile a refreshed listing for the current directory.
    ///
    /// Reports for another directory are stale (the user navigated away while
    /// the read was in flight) and ignored. The active visual range commits
    /// first so it survives as a selection. Prompts, confirmations, help,
    /// clipboard, status, and error state are left alone.
    pub fn apply_refresh(&mut self, dir: &Path, entries: Vec<Entry>) {
        if dir != self.notes_dir.as_path() {
            return;
        }
        self.commit_visual();
        if same_listing(&self.entries, &entries) {
            for (current, fresh) in self.entries.iter_mut().zip(entries) {
                current.modified = fresh.modified;
            }
            self.watcher_error = None;
            return;
        }
        let cursor_path = self
            .entries
            .get(self.cursor)
            .map(|entry| entry.path.clone());
        self.entries = entries;
        self.cursor = cursor_path
            .and_then(|path| self.entries.iter().position(|entry| entry.path == path))
            .unwrap_or(self.cursor)
            .min(self.entries.len().saturating_sub(1));
        self.selection
            .retain(|path| self.entries.iter().any(|entry| &entry.path == path));
        self.anchor = 0;
        self.watcher_error = None;
        self.ensure_cursor_visible();
    }

    /// Keep the last listing and surface `error` in the message row.
    /// Navigation retries the read and clears the message on success.
    pub fn refresh_failed(&mut self, error: String) {
        self.watcher_error = Some(error);
    }

    /// Re-render the open note after it changed on disk, keeping the
    /// approximate reading position.
    ///
    /// Reports for another path are stale (the reader moved on while the event
    /// was in flight) and ignored. A successful reload leaves status and error
    /// state alone so an editor message survives. When the note can no longer
    /// be read or rendered, the reader closes and the failure appears in the
    /// browser message row.
    pub fn apply_note_changed(&mut self, path: &Path) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        if reader.path() != path {
            return;
        }
        if let Err(error) = reader.reload() {
            self.reader = None;
            self.status = None;
            self.error = Some(error.to_string());
        }
    }
}

/// Whether two listings hold the same entries in the same order. Modification
/// times are compared separately so date-only changes take the fast path.
fn same_listing(current: &[Entry], fresh: &[Entry]) -> bool {
    current.len() == fresh.len()
        && current
            .iter()
            .zip(fresh)
            .all(|(a, b)| a.path == b.path && a.is_dir == b.is_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(format!("/notes/{name}")),
            is_dir,
            modified: None,
        }
    }

    fn app() -> App {
        App::new(
            PathBuf::from("/notes"),
            vec![entry("a.md", false), entry("b.md", false)],
        )
    }

    #[test]
    fn refresh_keeps_the_cursor_on_its_entry() {
        let mut app = app();
        app.move_cursor_by(1);

        app.apply_refresh(
            Path::new("/notes"),
            vec![
                entry("a.md", false),
                entry("b.md", false),
                entry("c.md", false),
            ],
        );

        assert_eq!(app.cursor, 1);
        assert_eq!(app.entries[app.cursor].name, "b.md");
    }

    #[test]
    fn refresh_clamps_the_cursor_when_its_entry_is_gone() {
        let mut app = app();
        app.move_cursor_by(1);

        app.apply_refresh(Path::new("/notes"), vec![entry("a.md", false)]);

        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn identical_listings_only_update_modification_times() {
        let mut app = app();
        app.toggle_selection();
        app.move_cursor_by(1);
        let now = SystemTime::now();
        let mut fresh = vec![entry("a.md", false), entry("b.md", false)];
        fresh[0].modified = Some(now);
        fresh[1].modified = Some(now);

        app.apply_refresh(Path::new("/notes"), fresh);

        assert_eq!(app.cursor, 1);
        assert_eq!(app.entries[0].modified, Some(now));
        assert!(app.entry_selected(0));
        assert!(!app.entry_selected(1));
    }

    #[test]
    fn refresh_drops_removed_selections_and_keeps_the_clipboard() {
        let mut app = app();
        app.toggle_selection();
        app.copy_selection();

        app.apply_refresh(Path::new("/notes"), vec![entry("b.md", false)]);

        assert!(app.selection.is_empty());
        assert!(app.clipboard.active());
    }

    #[test]
    fn refresh_commits_the_visual_range_first() {
        let mut app = app();
        app.toggle_visual();
        app.move_cursor_by(1);

        app.apply_refresh(
            Path::new("/notes"),
            vec![
                entry("a.md", false),
                entry("b.md", false),
                entry("c.md", false),
            ],
        );

        assert!(!app.visual);
        assert!(app.entry_selected(0));
        assert!(app.entry_selected(1));
    }

    #[test]
    fn refresh_keeps_prompts_and_help_open() {
        let mut app = app();
        app.open_new_note();
        app.help_visible = true;

        app.apply_refresh(
            Path::new("/notes"),
            vec![
                entry("a.md", false),
                entry("b.md", false),
                entry("c.md", false),
            ],
        );

        assert!(app.prompt.is_some());
        assert!(app.help_visible);
    }

    #[test]
    fn stale_directories_are_ignored() {
        let mut app = app();

        app.apply_refresh(Path::new("/other"), vec![entry("x.md", false)]);

        assert_eq!(app.entries.len(), 2);
        assert_eq!(app.notes_dir, PathBuf::from("/notes"));
    }

    #[test]
    fn failed_refresh_keeps_the_listing_and_shows_the_error() {
        let mut app = app();

        app.refresh_failed("watcher error".to_owned());

        assert_eq!(app.entries.len(), 2);
        assert_eq!(app.watcher_error.as_deref(), Some("watcher error"));
    }

    #[test]
    fn note_changed_reloads_the_open_note_and_ignores_other_paths() {
        use crate::app::{reading_height, reading_width};
        use crate::reader::Reader;

        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        std::fs::write(&path, "Hello\n").expect("write note");
        let mut app = App::new(temp.path().to_path_buf(), Vec::new());
        app.resize(80, 24);
        app.reader = Some(
            Reader::open(
                &path,
                reading_width(80),
                reading_height(24),
                None,
                app.images.clone(),
            )
            .expect("open reader"),
        );

        std::fs::write(&path, "Hello updated\n").expect("rewrite note");
        app.apply_note_changed(temp.path().join("other.md").as_path());
        assert!(app.reader.is_some());

        app.apply_note_changed(&path);
        assert!(app.reader.is_some());
        assert!(app.error.is_none());
    }

    #[test]
    fn note_changed_closes_the_reader_when_the_note_is_gone() {
        use crate::app::{reading_height, reading_width};
        use crate::reader::Reader;

        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        std::fs::write(&path, "Hello\n").expect("write note");
        let mut app = App::new(temp.path().to_path_buf(), Vec::new());
        app.resize(80, 24);
        app.reader = Some(
            Reader::open(
                &path,
                reading_width(80),
                reading_height(24),
                None,
                app.images.clone(),
            )
            .expect("open reader"),
        );

        std::fs::remove_file(&path).expect("remove note");
        app.apply_note_changed(&path);

        assert!(app.reader.is_none());
        assert!(app.error.is_some());
    }
}
