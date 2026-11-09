# `watch`

Source: `src/watch.rs`

## Purpose

Watches the current folder and the open note with `notify` and reports relevant changes over a
channel, so the single-threaded event loop stays free of an async runtime.
The folder watcher is non-recursive: only direct children of the watched directory
matter.

## What it handles

- Spawns a non-recursive watcher for one directory and drains reports without
  blocking (`FolderWatcher::spawn`, `watch`, `drain`, `watched`).
- Watches the open note for saves (`watch_note`, `watched_note`). The directory
  watch already reports saves for notes inside it; the file is also watched
  directly when it lives elsewhere.
- Re-points the watcher on navigation; watching the same directory again is a
  no-op. Watching the same note again is also a no-op.
- Filters directory events to direct children of the watched directory (or the watched
  directory itself, for example when it is removed).
- Ignores content-only writes (`Modify(Data(Content))`) and file accesses for the
  directory listing, which never change the listing.
- Treats content-only writes as relevant for the open note (`is_note_relevant`):
  any non-access event naming the note reports `WatchMessage::NoteChanged`.
  Path-less events never reload the reader.
- Treats path-less directory events as relevant so unusual backends still trigger a
  rescan.
- Tags every directory change with the directory that was watched when the event fired,
  so the app can drop stale reports from a previous directory. Note changes carry
  the note path so the app can drop reports for a closed note.
- Forwards watcher failures as `WatchMessage::Failed` instead of panicking,
  so the browser keeps working without live refresh.

## What it does not handle

- Reading directories or reconciling listings. The event loop re-reads with
  `browser::list` and the app reconciles in `src/app/refresh.rs`.
- Rendering or error display. Failures surface in the message row through
  `App::watcher_error`.
- Retrying failed watches. The event loop retries on navigation.

## Dependencies

- `notify` — cross-platform file watching (`RecommendedWatcher`).
- `std::sync::mpsc` — the watcher thread reports to the event loop.

## Public API

```rust
pub enum WatchMessage {
    Changed { dir: PathBuf },
    NoteChanged { path: PathBuf },
    Failed { error: String },
}

pub enum WatchError {
    Spawn(#[source] notify::Error),
    Watch { path: PathBuf, #[source] source: notify::Error },
}

pub struct FolderWatcher { /* watcher, watched dir, watched note, channel */ }

impl FolderWatcher {
    pub fn spawn(dir: &Path) -> Result<Self, WatchError>;
    pub fn watched(&self) -> &Path;
    pub fn watched_note(&self) -> Option<&Path>;
    pub fn watch(&mut self, dir: &Path) -> Result<(), WatchError>;
    pub fn watch_note(&mut self, note: Option<&Path>) -> Result<(), WatchError>;
    pub fn drain(&self) -> Vec<WatchMessage>;
}

pub fn is_relevant(event: &Event, watched: &Path) -> bool;
pub fn is_note_relevant(event: &Event, note: &Path) -> bool;
```

- `Changed.dir` is the directory watched when the event fired; the app
  ignores it when it no longer matches the current directory.
- `NoteChanged.path` is the note watched when the event fired; the app ignores
  it when the reader is closed or on another note.
- `is_relevant` and `is_note_relevant` are the testable filters behind the watcher callback; tests
  build `notify::Event` values directly with no sleeping watcher.

## Example

```rust
let watcher = FolderWatcher::spawn(&app.notes_dir)?;
watcher.watch_note(app.reader.as_ref().map(|reader| reader.path()))?;
for message in watcher.drain() {
    match message {
        WatchMessage::Changed { dir } => app.update(Msg::WatcherEvent { dir }),
        WatchMessage::NoteChanged { path } => app.update(Msg::NoteChanged { path }),
        WatchMessage::Failed { error } => app.update(Msg::RefreshFailed { error }),
    };
}
```
