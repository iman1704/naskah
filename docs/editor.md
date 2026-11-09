# `editor`

Source: `src/editor.rs`

## Purpose

Builds the external editor command from the config file (falling back to
`$EDITOR`) and runs it to completion. Naskah never edits text itself: `e`
hands the cursor entry to this editor and refreshes the listing when it
exits.

## What it handles

- Splits the configured command on whitespace, so values like `code --wait`
  keep working. Quoting is not supported; use a wrapper script when an
  argument contains spaces.
- Appends the note path after the configured arguments.
- Reports a missing editor, a failed start, and a non-zero exit as typed
  `EditorError` values shown in the message row.

## What it does not handle

- Terminal suspend/resume. The event loop restores the terminal before
  calling `run` and re-initializes it after, then refreshes the listing.
- Choosing or validating the editor string. `config` already merges the
  config file with `$EDITOR`.
- Built-in editing. All writing stays with the external editor.

## Dependencies

- `std::process` — build and run the editor command.
- `thiserror` — typed editor errors.

## Public API

```rust
pub enum EditorError {
    NoEditor,
    Start { program: String, #[source] source: std::io::Error },
    Failed { status: String },
}

pub fn split(command: &str) -> Option<(String, Vec<String>)>;
pub fn command(editor: Option<&str>, path: &Path) -> Result<Command, EditorError>;
pub fn run(editor: Option<&str>, path: &Path) -> Result<(), EditorError>;
```

- `split` returns `None` for a blank command; otherwise the first token is
  the program and the rest are arguments.
- `run` blocks until the editor exits and maps a non-zero status to
  `EditorError::Failed`.

## Example

```rust
editor::run(config.editor.as_deref(), &path)?;
```
