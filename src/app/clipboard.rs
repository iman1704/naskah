//! The internal copy/cut clipboard.
//!
//! Copy (`y`) and cut (`x`) queue the effective selection for a later paste.
//! The clipboard survives navigation; cut entries are dropped again on paste
//! (task 5) or discarded with `esc`.

use std::path::Path;

use super::App;
use crate::browser::Entry;

/// Entries queued by copy or cut.
#[derive(Debug, Default)]
pub struct Clipboard {
    /// Queued entries in listing order.
    pub entries: Vec<Entry>,
    /// Whether the entries were cut (`true`) or copied (`false`).
    pub cut: bool,
}

impl Clipboard {
    /// Whether anything is queued.
    pub fn active(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Whether `path` is queued.
    pub fn has(&self, path: &Path) -> bool {
        self.entries.iter().any(|entry| entry.path == path)
    }

    /// Drop every queued entry.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.cut = false;
    }
}

impl App {
    /// Queue the effective selection for pasting, leaving the selection
    /// intact until paste or `esc`.
    pub fn copy_selection(&mut self) {
        self.commit_visual();
        let entries = self.operation_entries();
        if entries.is_empty() {
            self.status = Some("nothing selected".to_owned());
            return;
        }
        self.error = None;
        self.clipboard = Clipboard {
            entries: entries.clone(),
            cut: false,
        };
        self.status = Some(format!("copied {}", selection_label(&entries)));
    }

    /// Queue the effective selection for moving on paste, leaving the
    /// selection intact until paste or `esc`.
    pub fn cut_selection(&mut self) {
        self.commit_visual();
        let entries = self.operation_entries();
        if entries.is_empty() {
            self.status = Some("nothing selected".to_owned());
            return;
        }
        self.error = None;
        self.clipboard = Clipboard {
            entries: entries.clone(),
            cut: true,
        };
        self.status = Some(format!("cut {}", selection_label(&entries)));
    }
}

/// Describe one entry by name or several entries by count. Directories get a
/// trailing slash, matching the Go app's status messages.
fn selection_label(entries: &[Entry]) -> String {
    if entries.len() == 1 {
        let entry = &entries[0];
        if entry.is_dir {
            return format!("{}/", entry.name);
        }
        return entry.name.clone();
    }
    format!("{} items", entries.len())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
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
    fn copy_queues_the_cursor_entry() {
        let mut app = app();

        app.copy_selection();

        assert!(!app.clipboard.cut);
        assert_eq!(app.clipboard.entries, vec![entry("a.md", false)]);
        assert_eq!(app.status.as_deref(), Some("copied a.md"));
    }

    #[test]
    fn cut_marks_the_clipboard_and_keeps_the_selection() {
        let mut app = app();
        app.toggle_selection();

        app.cut_selection();

        assert!(app.clipboard.cut);
        assert_eq!(app.clipboard.entries.len(), 1);
        assert!(!app.selection.is_empty());
        assert_eq!(app.status.as_deref(), Some("cut a.md"));
    }

    #[test]
    fn labels_count_several_entries_and_slash_directories() {
        assert_eq!(selection_label(&[entry("a.md", false)]), "a.md");
        assert_eq!(selection_label(&[entry("docs", true)]), "docs/");
        assert_eq!(
            selection_label(&[entry("a.md", false), entry("b.md", false)]),
            "2 items"
        );
    }
}
