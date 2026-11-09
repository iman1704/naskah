# `browser`

Source: `src/browser.rs`

## Purpose

Provides the directory listing behind Naskah's note browser. Given a directory path, it returns the entries the TUI is allowed to show and navigate to.

## What it handles

- Lists immediate children of one directory (no recursion).
- Includes subdirectories and `*.md` files only (extension match is case-insensitive, so `.MD` counts).
- Skips hidden entries (names starting with `.`).
- Follows symlinks to decide whether an entry is a directory; dangling symlinks are skipped. Entries whose metadata cannot be read are also skipped.
- Sorts output with directories first, then files. Each group is ordered case-insensitively by name, with the exact name as a deterministic tie-breaker.
- Collects each entry's modification time for the relative date labels.

## What it does not handle

- No UI state, selection, or navigation. That lives in `src/app`.
- No file creation, rename, copy, move, or trash. That lives in `src/files`.
- No reading or rendering of file contents.
- No date formatting; the UI turns `SystemTime` into relative labels.

## Dependencies

- `std::fs` — read the directory and resolve symlinks.
- `std::path` — build entry paths and check the `.md` extension.
- `std::time` — carry modification times.
- `thiserror` — typed errors.

## Public API

```rust
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub modified: Option<SystemTime>,
}

pub enum BrowserError {
    ReadDir { path: PathBuf, source: std::io::Error },
}

pub fn list(dir: &Path) -> Result<Vec<Entry>, BrowserError>;
```

- `Entry` is one browsable item. `path` is the full joined path (`dir.join(name)`); `is_dir` is true for directories (including symlinks to directories).
- `name` is lossy for display. `path` keeps the original bytes for filesystem operations.
- `list(dir)` returns the sorted, filtered entries for `dir`. It returns a `BrowserError` if `dir` cannot be read (for example, it does not exist).
- `modified` is `None` when the filesystem does not report a modification time.

## Example

```rust
let entries = browser::list(Path::new("/Users/me/notes"))?;
for entry in entries {
    // entry.name, entry.path, entry.is_dir, entry.modified
}
```
