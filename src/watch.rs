//! Live folder watching for the note browser.
//!
//! The watcher runs on `notify`'s thread and reports back over a standard
//! library channel, so the single-threaded event loop stays free of an async
//! runtime. Only direct children of the watched directory matter: the watcher
//! is non-recursive, content-only writes are ignored, and every report is
//! tagged with the directory that was watched when the event fired so stale
//! reports from a previous directory are dropped.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::event::{DataChange, ModifyKind};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

/// A report from the watcher thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchMessage {
    /// Something relevant changed inside `dir`.
    Changed {
        /// The directory that was watched when the event fired.
        dir: PathBuf,
    },
    /// The open note changed on disk and should be re-rendered.
    NoteChanged {
        /// The note that was watched when the event fired.
        path: PathBuf,
    },
    /// The watcher itself failed; live refresh stops until navigation retries.
    Failed {
        /// Human-readable failure for the message row.
        error: String,
    },
}

/// Errors from starting or re-pointing the folder watcher.
#[derive(Debug, Error)]
pub enum WatchError {
    #[error("could not start the file watcher: {0}")]
    Spawn(#[source] notify::Error),

    #[error("could not watch {}: {source}", .path.display())]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
}

/// A non-recursive watcher for one directory at a time.
pub struct FolderWatcher {
    watcher: RecommendedWatcher,
    watched: PathBuf,
    watched_note: Option<PathBuf>,
    sender: mpsc::Sender<WatchMessage>,
    receiver: mpsc::Receiver<WatchMessage>,
}

impl FolderWatcher {
    /// Start watching `dir` (non-recursive) and return the watcher plus its
    /// channel. Reports arrive on the channel; drain them with [`Self::drain`].
    pub fn spawn(dir: &Path) -> Result<Self, WatchError> {
        let (sender, receiver) = mpsc::channel();
        let watcher = start_watcher(dir, None, &sender).map_err(WatchError::Spawn)?;
        Ok(Self {
            watcher,
            watched: dir.to_path_buf(),
            watched_note: None,
            sender,
            receiver,
        })
    }

    /// The directory currently watched.
    pub fn watched(&self) -> &Path {
        &self.watched
    }

    /// The open note currently watched, if any.
    pub fn watched_note(&self) -> Option<&Path> {
        self.watched_note.as_deref()
    }

    /// Point the watcher at `dir`. Watching the same directory again is a
    /// no-op. The listing itself is refreshed separately through an effect.
    pub fn watch(&mut self, dir: &Path) -> Result<(), WatchError> {
        if dir == self.watched {
            return Ok(());
        }
        self.watcher =
            start_watcher(dir, self.watched_note.as_deref(), &self.sender).map_err(|source| {
                WatchError::Watch {
                    path: dir.to_path_buf(),
                    source,
                }
            })?;
        self.watched = dir.to_path_buf();
        Ok(())
    }

    /// Watch `note` for saves while it is open in the reader, or stop watching
    /// the note when passed `None`. Watching the same note again is a no-op.
    pub fn watch_note(&mut self, note: Option<&Path>) -> Result<(), WatchError> {
        if self.watched_note.as_deref() == note {
            return Ok(());
        }
        let dir = self.watched.clone();
        self.watcher =
            start_watcher(&dir, note, &self.sender).map_err(|source| WatchError::Watch {
                path: dir.clone(),
                source,
            })?;
        self.watched_note = note.map(Path::to_path_buf);
        Ok(())
    }

    /// Collect every pending watcher report without blocking.
    pub fn drain(&self) -> Vec<WatchMessage> {
        self.receiver.try_iter().collect()
    }
}

/// Build a non-recursive watcher for `dir` that forwards relevant events.
/// When `note` is set, the open note is also watched so saves re-render the
/// reader; content-only writes still count for the note even though they never
/// change the directory listing.
fn start_watcher(
    dir: &Path,
    note: Option<&Path>,
    sender: &mpsc::Sender<WatchMessage>,
) -> Result<RecommendedWatcher, notify::Error> {
    let watched = dir.to_path_buf();
    let watched_note = note.map(Path::to_path_buf);
    let sender = sender.clone();
    let watched_in_callback = watched.clone();
    let note_in_callback = watched_note.clone();
    let mut watcher = RecommendedWatcher::new(
        move |result: Result<Event, notify::Error>| match result {
            Ok(event) => {
                if is_relevant(&event, &watched_in_callback) {
                    let _ = sender.send(WatchMessage::Changed {
                        dir: watched_in_callback.clone(),
                    });
                }
                if note_in_callback
                    .as_ref()
                    .is_some_and(|note| is_note_relevant(&event, note))
                    && let Some(path) = note_in_callback.clone()
                {
                    let _ = sender.send(WatchMessage::NoteChanged { path });
                }
            }
            Err(error) => {
                let _ = sender.send(WatchMessage::Failed {
                    error: error.to_string(),
                });
            }
        },
        notify::Config::default(),
    )?;
    watcher.watch(dir, RecursiveMode::NonRecursive)?;
    // The directory watch already reports saves for notes inside it. Only watch
    // the file directly when it lives elsewhere, and ignore the extra watch
    // when it fails (for example the note was just deleted): the directory
    // report still drives the reload attempt.
    if let Some(note) = watched_note
        && note.parent() != Some(watched.as_path())
    {
        let _ = watcher.watch(&note, RecursiveMode::NonRecursive);
    }
    Ok(watcher)
}

/// Whether a `notify` event should reload the open note.
///
/// Unlike the directory listing, content-only writes matter here: any
/// non-access event that names `note` directly counts. Accesses and path-less
/// events never reload the reader.
pub fn is_note_relevant(event: &Event, note: &Path) -> bool {
    if let EventKind::Access(_) = &event.kind {
        return false;
    }
    event.paths.iter().any(|path| path == note)
}

/// Whether a `notify` event should refresh the listing for `watched`.
///
/// Content-only writes (`Modify(Data(Content))`) and file accesses never
/// change the listing, so they are ignored. Everything else counts when at
/// least one reported path is a direct child of `watched` (or `watched`
/// itself, for example when the directory is removed). Path-less events are
/// treated as relevant so unusual backends still trigger a rescan.
pub fn is_relevant(event: &Event, watched: &Path) -> bool {
    match &event.kind {
        EventKind::Access(_) => return false,
        EventKind::Modify(ModifyKind::Data(DataChange::Content)) => return false,
        _ => {}
    }
    if event.paths.is_empty() {
        return true;
    }
    event
        .paths
        .iter()
        .any(|path| path == watched || path.parent().is_some_and(|parent| parent == watched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::EventKind;
    use notify::event::{AccessKind, AccessMode, CreateKind, DataChange, ModifyKind, RemoveKind};

    fn event(kind: EventKind, paths: &[&str]) -> Event {
        let mut event = Event::new(kind);
        event.paths = paths.iter().map(PathBuf::from).collect();
        event
    }

    #[test]
    fn create_inside_the_watched_dir_is_relevant() {
        let event = event(EventKind::Create(CreateKind::File), &["/notes/new.md"]);

        assert!(is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn nested_paths_are_ignored() {
        let event = event(
            EventKind::Create(CreateKind::File),
            &["/notes/sub/nested.md"],
        );

        assert!(!is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn events_for_another_directory_are_ignored() {
        let event = event(EventKind::Remove(RemoveKind::File), &["/other/old.md"]);

        assert!(!is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn content_only_writes_are_ignored() {
        let event = event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            &["/notes/note.md"],
        );

        assert!(!is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn renames_are_relevant() {
        let event = event(
            EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::To)),
            &["/notes/renamed.md"],
        );

        assert!(is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn accesses_are_ignored() {
        let event = event(
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            &["/notes/note.md"],
        );

        assert!(!is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn the_watched_directory_itself_is_relevant() {
        let event = event(EventKind::Remove(RemoveKind::Folder), &["/notes"]);

        assert!(is_relevant(&event, Path::new("/notes")));
    }

    #[test]
    fn content_writes_to_the_open_note_are_relevant() {
        let event = event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            &["/notes/note.md"],
        );

        assert!(is_note_relevant(&event, Path::new("/notes/note.md")));
    }

    #[test]
    fn other_files_do_not_reload_the_open_note() {
        let event = event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            &["/notes/other.md"],
        );

        assert!(!is_note_relevant(&event, Path::new("/notes/note.md")));
    }

    #[test]
    fn accesses_and_path_less_events_do_not_reload_the_note() {
        let access = event(
            EventKind::Access(AccessKind::Open(AccessMode::Read)),
            &["/notes/note.md"],
        );
        assert!(!is_note_relevant(&access, Path::new("/notes/note.md")));

        let mut path_less = Event::new(EventKind::Modify(ModifyKind::Other));
        path_less.paths = Vec::new();
        assert!(!is_note_relevant(&path_less, Path::new("/notes/note.md")));
    }
}
