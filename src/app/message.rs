//! Messages accepted by the app and effects the event loop executes.

use std::path::PathBuf;

use crossterm::event::KeyEvent;

use crate::browser::{BrowserError, Entry};

/// Input or filesystem result delivered to [`crate::app::App::update`].
#[derive(Debug)]
pub enum Msg {
    /// A key press from the terminal.
    Key(KeyEvent),
    /// The terminal size changed.
    Resize { width: u16, height: u16 },
    /// A directory listing requested by an effect has loaded.
    DirectoryLoaded {
        path: PathBuf,
        entries: Vec<Entry>,
        reselect: Option<PathBuf>,
    },
    /// A requested directory listing failed.
    DirectoryLoadFailed { error: BrowserError },
    /// The watcher noticed a change inside `dir`.
    WatcherEvent { dir: PathBuf },
    /// The open note changed on disk and should be re-rendered.
    NoteChanged { path: PathBuf },
    /// A refreshed listing for the current directory arrived.
    RefreshLoaded { dir: PathBuf, entries: Vec<Entry> },
    /// A refresh (or the watcher itself) failed; the listing is unchanged.
    RefreshFailed { error: String },
}

/// Work the event loop must perform outside the app state.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Read a directory and report back with [`Msg::DirectoryLoaded`] or
    /// [`Msg::DirectoryLoadFailed`].
    LoadDirectory {
        path: PathBuf,
        reselect: Option<PathBuf>,
    },
    /// Re-read the current directory after a watcher event or an editor exit.
    /// Success reports back with [`Msg::RefreshLoaded`], failure with
    /// [`Msg::RefreshFailed`]; the listing is reconciled, not reset.
    RefreshDirectory { dir: PathBuf },
    /// Open `path` in the external editor. The event loop suspends the
    /// terminal around the editor and refreshes afterwards.
    OpenEditor { path: PathBuf },
}
