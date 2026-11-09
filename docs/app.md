# `app`

Source: `src/app/mod.rs`, `src/app/message.rs`, `src/app/update.rs`, `src/app/selection.rs`, `src/app/clipboard.rs`, `src/app/prompt.rs`, `src/app/refresh.rs`

## Purpose

Owns the browser's and reader's UI state and turns input into state changes or effects. Rendering lives in `src/ui`; directory reads are requested through effects and executed by the event loop, while a note is read and rendered synchronously by `src/reader`.

## What it handles

- The notes root and current directory, the current listing, cursor entry, scroll offset, viewport size, terminal size, status, error, help state, and quit flag.
- The open reader and its rendered markdown: `enter` on a note reads and renders it, `q`/`h`/`esc` (or left/backspace) return to the browser with the cursor and selection unchanged, and `j`/`k`, `PageDown`/`PageUp`, and `g`/`G` scroll the rendered note. `open` receives the shared Kitty image picker so block images render as terminal graphics where supported; under Zellij the reader opens in painter mode instead (see `docs/zellij_images.md`). Both modes prefetch through the app's `ImageStore` and refresh the open reader when images finish loading.
- Reader live reload: `Msg::NoteChanged` re-reads the open note and re-renders it at the reading ratio; reports for other paths are ignored, and a note that can no longer be read closes the reader with the failure in the browser message row. Watch and editor failures stay visible in the reader help row; a shown `error` clears on the next non-quit reader key while `watcher_error` stays until the next successful refresh.
- Reader live reload: `Msg::NoteChanged` re-reads the open note and re-renders it at the reading ratio; reports for other paths are ignored, and a note that can no longer be read closes the reader with the failure in the browser message row.
- Reader editing: `e` in the reader emits `Effect::OpenEditor` for the open note; the event loop refreshes the listing and re-renders the reader so the app returns to the reader after the editor exits.
- Image completions: `refresh_reader_images` asks the open reader to re-render after background loads. The event loop drains a batch of completions and calls it once, then redraws; a render failure closes the reader and reports in the browser message row.
- Read or render failures stay in the browser and appear in the message row. Ctrl-C still quits while the reader is open.
- Multi-selection: `Space` toggles the cursor entry; `v` starts a Vim-style range at the cursor that `j`/`k` extend, and a second `v` commits the range. `Space` is ignored while a visual range is active.
- The internal clipboard: `y` copies and `x` cuts the effective selection, which falls back to the cursor entry when nothing is selected. The clipboard survives navigation; `y`/`x` leave the selection intact until paste or `esc`.
- Copy/cut status messages (`copied <name>`, `cut <name>`, `<n> items`) and the `nothing selected` status for an empty listing.
- The new note (`a`), new folder (`A`), and rename (`R`) prompts in the status row, with typing, arrow/Home/End movement, Backspace/Delete, prefilled rename names, `.md` handling, validation errors under the header, and `esc` cancel.
- Paste (`p`): copies or moves every clipboard entry into the current directory, drops missing sources, keeps copy entries for repeated pastes, removes moved cut entries, continues after failures with `<verb> <n> of <m> items: <error>` summaries, then refreshes and reselects the last pasted entry.
- Trash (`d`) with a `Trash "name"? (y/N)` confirmation: moves every confirmed entry to the flat XDG trash, continues after failures, refreshes onto the nearest remaining entry, and filters trashed paths out of the clipboard.
- Rename follows the renamed path (and queued descendants) in the clipboard.
- `esc` priority: an open prompt or trash confirmation cancels first, then the help overlay closes, and otherwise one press clears visual mode, all selections, and the clipboard.
- Any non-visual key commits the active visual range before running its action, so ranges never leak into unrelated behavior. Rename, edit, and open always use the cursor entry.
- The row model: `FOLDERS` and `NOTES` labels, one blank gap between groups, and entry rows. Cursor and scroll positions are tracked in rendered rows, so labels and the gap are accounted for.
- Navigation: `j`/down, `k`/up, `PageDown`/`PageUp`, `g`/`Home`, `G`/`End`, `h`/left/backspace for the parent, and `l`/right/enter to open.
- Opening a folder returns `Effect::LoadDirectory`; opening a note loads it into the reader.
- `e` opens the cursor note in the external editor via `Effect::OpenEditor` and is restricted to the cursor entry like rename. Folders and empty listings report a status instead.
- Parent navigation stops at the configured notes root. Returning to a parent reselects the child directory it came from.
- Directory listings loaded or failed by the event loop. Loading a new listing clears the selection and visual range but keeps the clipboard.
- Live refresh: a `WatcherEvent` for the current directory returns `Effect::RefreshDirectory`. `RefreshLoaded` reconciles the listing: the cursor stays on its entry (clamped when it is gone), surviving selections are kept, the active visual range commits first, and the clipboard, prompts, confirmation, help, status, and error state are left alone. Identical listings only update modification times. Reports for other directories are stale and ignored.
- Failed refreshes keep the last listing and show the failure in `watcher_error` (rendered in the message row). The next successful refresh or navigation clears it, so navigation doubles as a retry.
- Resize handling: recomputes the visible list height and keeps the cursor row visible. When a note is open, the reader is reflowed to the new column width and clamped to the new viewport height; a reflow failure closes the reader and shows the error in the browser.
- Help overlay state and message dismissal.

## What it does not handle

- Reading directories. The event loop executes effects with `browser::list`.
- Reading or rendering notes. That belongs to `src/reader`.
- Drawing either screen. That belongs to `src/ui`.
- Running the external editor or watching directories. The event loop owns the `watch` watcher and suspends the terminal around `editor::run`; the app only emits effects and reconciles results.
- Startup errors. The binary reports those to stderr.

## Dependencies

- `std::path` — root, current directory, and reselection paths.
- `crossterm` — `KeyCode`, `KeyEvent`, and modifiers.
- `crate::browser` — `Entry` and `BrowserError`.
- `crate::files` — synchronous create, rename, copy, move, and trash.
- `crate::images` — the process-wide image store shared with the reader.
- `crate::reader` — reading, rendering, and scrolling the open note.

## Public API

```rust
pub const CHROME_ROWS: u16 = 3;

pub fn reading_width(width: u16) -> u16;
pub fn reading_height(height: u16) -> usize;

pub enum Section { Folders, Notes }
pub enum ListRow { Section(Section), Gap, Entry(usize) }

pub struct App {
    pub root: PathBuf,
    pub notes_dir: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub scroll: usize,
    pub viewport: usize,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub reader: Option<Reader>,
    pub images: ImageStore,
    pub image_picker: Option<ImagePicker>,
    pub zellij_painter: bool,
    pub zellij_cell: (u16, u16),
    pub zellij_ids: Vec<u32>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub selection: HashSet<PathBuf>,
    pub visual: bool,
    pub anchor: usize,
    pub clipboard: Clipboard,
    pub prompt: Option<Prompt>,
    pub confirm: Option<Confirm>,
    pub help_visible: bool,
    pub watcher_error: Option<String>,
    pub should_quit: bool,
}

pub struct Clipboard {
    pub entries: Vec<Entry>,
    pub cut: bool,
}

impl Clipboard {
    pub fn active(&self) -> bool;
    pub fn has(&self, path: &Path) -> bool;
    pub fn clear(&mut self);
}

pub enum PromptKind { NewNote, NewFolder, Rename }

pub struct Prompt {
    pub kind: PromptKind,
    pub target: PathBuf,
    pub value: String,
    pub cursor: usize,
}

pub struct Confirm {
    pub entries: Vec<Entry>,
}

impl Confirm {
    pub fn question(&self) -> String;
}

impl App {
    pub fn new(notes_dir: PathBuf, entries: Vec<Entry>) -> Self;
    pub fn rows(&self) -> Vec<ListRow>;
    pub fn resize(&mut self, width: u16, height: u16);
    pub fn refresh_reader_images(&mut self);
    pub fn note_count(&self) -> usize;
    pub fn position(&self) -> (usize, usize);
    pub fn update(&mut self, msg: Msg) -> Vec<Effect>;
    pub fn in_visual_range(&self, index: usize) -> bool;
    pub fn entry_selected(&self, index: usize) -> bool;
    pub fn selected_entries(&self) -> Vec<Entry>;
    pub fn operation_entries(&self) -> Vec<Entry>;
    pub fn toggle_selection(&mut self);
    pub fn toggle_visual(&mut self);
    pub fn commit_visual(&mut self);
    pub fn cancel_browser(&mut self) -> bool;
    pub fn copy_selection(&mut self);
    pub fn cut_selection(&mut self);
    pub fn open_new_note(&mut self);
    pub fn open_new_folder(&mut self);
    pub fn open_rename(&mut self);
    pub fn handle_prompt_key(&mut self, key: KeyEvent) -> Vec<Effect>;
    pub fn request_trash(&mut self);
    pub fn handle_confirm_key(&mut self, key: KeyEvent) -> Vec<Effect>;
    pub fn do_paste(&mut self) -> Vec<Effect>;
    pub fn apply_refresh(&mut self, dir: &Path, entries: Vec<Entry>);
    pub fn apply_note_changed(&mut self, path: &Path);
    pub fn refresh_failed(&mut self, error: String);
}

pub enum Msg {
    Key(KeyEvent),
    Resize { width: u16, height: u16 },
    DirectoryLoaded { path: PathBuf, entries: Vec<Entry>, reselect: Option<PathBuf> },
    DirectoryLoadFailed { error: BrowserError },
    WatcherEvent { dir: PathBuf },
    NoteChanged { path: PathBuf },
    RefreshLoaded { dir: PathBuf, entries: Vec<Entry> },
    RefreshFailed { error: String },
}

pub enum Effect {
    LoadDirectory { path: PathBuf, reselect: Option<PathBuf> },
    RefreshDirectory { dir: PathBuf },
    OpenEditor { path: PathBuf },
}
```

- `cursor` indexes `entries`; `scroll` is the first rendered list row; `viewport` is the number of list rows available.
- `CHROME_ROWS` is the header, message, and status row count used by browser resize handling. `reading_width` and `reading_height` derive the centered reader column and content height.
- `reader` is `None` on the browser screen and holds the rendered note plus scroll state on the reader screen. `image_picker` is detected once at startup by the binary and shared by every opened note; `None` keeps the text fallback.
- `images` is the process-wide image store (`ImageStore::shared()`), cloned into every opened reader. The event loop drains its completions, calls `refresh_reader_images` once per batch, and redraws.
- File operations run synchronously inside `update`. `Effect::LoadDirectory` asks the event loop to call `browser::list` and feed the result back as a `Msg`, which clears the selection but keeps the clipboard.
- `Effect::RefreshDirectory` reconciles instead of resetting: `apply_refresh` preserves the cursor and surviving selections, so external changes never steal the user's place. Reader state is independent and survives listing refreshes.
- `Msg::NoteChanged` calls `apply_note_changed`, which reloads the reader through `Reader::reload` and keeps the scroll ratio. `Effect::OpenEditor` from the reader returns to the reader: the event loop refreshes the listing and then reloads the open note.

## Example

```rust
let entries = browser::list(&config.notes_dir)?;
let mut app = App::new(config.notes_dir, entries);

for effect in app.update(Msg::Key(key)) {
    if let Effect::LoadDirectory { path, reselect } = effect {
        let entries = browser::list(&path)?;
        app.update(Msg::DirectoryLoaded { path, entries, reselect });
    }
}
```
