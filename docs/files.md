# `files`

Source: `src/files/mod.rs`, `src/files/trash.rs`

## Purpose

Validates names and performs Naskah's synchronous note and folder operations: create, rename, copy, move, and move-to-trash. Everything runs on the calling thread; a large copy can pause the UI in this milestone.

## What it handles

- Name validation: trimmed names must not be empty, `.`, `..`, hidden, contain path separators or control characters, or exceed 255 bytes.
- Creating notes (`.md` appended case-insensitively when missing) and folders, with numeric collision suffixes (`name.1.md`, `name.1` for folders).
- Renaming within the same directory, preserving `.md` for notes. Renaming to the current name is a no-op; case-only renames work on case-insensitive filesystems.
- Recursively copying files and folders, recreating symlinks as symlinks. Copying a folder into itself or a descendant is refused, and failed copies clean up their destination.
- Moving entries with a copy-then-remove fallback across filesystems. Moving into the same directory or into itself/a descendant is refused.
- Move-to-trash deletion into the flat `$XDG_DATA_HOME/naskah/trash` directory (or `~/.local/share/naskah/trash`), with collision suffixes.
- Trash directory resolution (honouring an absolute `XDG_DATA_HOME`, else `HOME`), without creating the directory.

## What it does not handle

- No UI state, prompts, confirmations, or clipboard. Those live in `src/app`.
- No rendering. That belongs to `src/ui`.
- No directory listing. That belongs to `src/browser`.
- No trash browsing or restore in this milestone.

## Dependencies

- `std::fs` — create, rename, copy, remove, symlink metadata, and permissions.
- `std::os::unix::fs` — symlink creation, directory modes, and device/inode comparison for case-only renames.
- `std::path` — path joins, file names, and case-insensitive extension checks.
- `thiserror` — typed errors with Go-matching messages.

## Public API

```rust
pub enum Error {
    InvalidName(String),
    Refused(String),
    Io { context: &'static str, source: std::io::Error },
    NoTrashDir,
}

impl Error {
    pub fn is_not_found(&self) -> bool;
}

pub fn validate_name(name: &str) -> Result<(), Error>;
pub fn clean_name(name: &str) -> Result<String, Error>;
pub fn is_within(parent: &Path, child: &Path) -> bool;
pub fn create_note(dir: &Path, name: &str) -> Result<PathBuf, Error>;
pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, Error>;
pub fn rename(source: &Path, name: &str) -> Result<PathBuf, Error>;
pub fn copy_entry(src: &Path, dst_dir: &Path) -> Result<PathBuf, Error>;
pub fn move_entry(src: &Path, dst_dir: &Path) -> Result<PathBuf, Error>;

pub mod trash {
    pub fn trash_dir() -> Result<PathBuf, Error>;
    pub fn trash(path: &Path) -> Result<PathBuf, Error>;
}
```

- `validate_name` checks the naming rules; `clean_name` also returns the trimmed name.
- `is_not_found` reports a missing source, which paste and trash drop quietly.
- Every operation returns the path it created. `is_within` is also reused by the app to follow renames and forget trashed clipboard entries.

## Example

```rust
let note = files::create_note(&config.notes_dir, "reading list")?;
assert_eq!(note.extension().and_then(|ext| ext.to_str()), Some("md"));

let trashed = files::trash::trash(&note)?;
```

## Platform notes

- macOS and Linux only. `std::os::unix` is used directly for symlinks, directory modes, and same-file detection.
