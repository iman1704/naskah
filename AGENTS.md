Project overview
This project is a TUI markdown note browser written in Rust. Features include directory-based browser, edit and reading mode with live file updates. Focuses on simplicity while delivering polished experienced for the available features.

Important notes
- Refer to project-plan.md for the project scope and features
- Check for outstanding tasks in TODO.md and mark it as done after you've finished it
- Prioritise simple readable codes
- Use best practices for the chosen language and libraries as well as general development workflows
- Always ask questions for clarifications and to choose between ideas before making changes
- Never make assumptions

Development workflow
- Write code -> run the relevant test module(s) (`cargo test <module>` for unit tests in a file, `cargo test --test <name>` for an integration test in `tests/<name>.rs`) -> iterate.
- Do not verify progress with elaborate terminal commands, manual TUI launches, shell pipelines, or ad-hoc scripts.
- Do not run extensive testing commands (full suite, coverage, benchmarks) unless asked to.
- Format with `cargo fmt` and lint with `cargo clippy -- -D warnings`; both stay clean at all times.

Documentation
- `docs/` holds one `.md` per top-level module: `docs/<module>.md` documents `src/<module>.rs`, or `src/<module>/mod.rs` and its submodules when the module is a folder.
- Each doc contains the module's core dependencies, external API signatures, and a description of what the module handles. Its purpose is an overview and basic reference without having to read the code; no extensive technical detail.
- When modifying files, update the corresponding doc accordingly in the same change.
- Canonical example (`docs/browser.md`) for reference:

````markdown
# `browser`

Source: `src/browser.rs`

## Purpose

Provides the directory listing behind Naskah's note browser. Given a directory path, it returns the entries the TUI is allowed to show and navigate to.

## What it handles

- Lists immediate children of one directory (no recursion).
- Includes subdirectories and `*.md` files only (extension match is case-insensitive, so `.MD` counts).
- Skips hidden entries (names starting with `.`).
- Follows symlinks to decide whether an entry is a directory; dangling symlinks are skipped.
- Sorts output with directories first, then files. Each group is ordered case-insensitively by name.
- Collects each note's modification time for the relative date labels.

## What it does not handle

- No UI state, selection, or navigation. That lives in `src/app`.
- No file creation, rename, copy, move, or trash. That lives in `src/files`.
- No reading or rendering of file contents.

## Dependencies

- `std::fs` — read the directory and resolve symlinks.
- `std::path` — build entry paths and check the `.md` extension.
- `chrono` — modification times for the date labels.

## Public API

```rust
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub modified: Option<SystemTime>,
}

pub fn list(dir: &Path) -> Result<Vec<Entry>, BrowserError>;
```

- `Entry` is one browsable item. `path` is the full joined path (`dir.join(name)`); `is_dir` is true for directories (including symlinks to directories).
- `list(dir)` returns the sorted, filtered entries for `dir`. It returns a `BrowserError` if `dir` cannot be read (for example, it does not exist).

## Example

```rust
let entries = browser::list(Path::new("/Users/me/notes"))?;
for entry in entries {
    // entry.name, entry.path, entry.is_dir, entry.modified
}
```
````

Project directory structure
Single crate with a thin binary and a library surface for tests. `app` owns UI state; `ui` renders it; `browser`, `files`, `config`, `editor`, and `watch` contain no TUI code.

- `src/main.rs`: thin entrypoint — parse the CLI, load config, start the TUI, map startup errors to stderr.
- `src/lib.rs`: exposes the modules so integration tests can exercise the app without launching a terminal.
- `src/config.rs`: loads `~/.config/naskah/naskah.toml` (honours `XDG_CONFIG_HOME`) and defaults.
- `src/browser.rs`: lists directories and `*.md` files only, sorting, note metadata.
- `src/files/`: name validation, create, rename, copy, cut/move, move-to-trash (`trash.rs`).
- `src/editor.rs`: builds the external editor command and suspends/resumes the terminal around it.
- `src/watch.rs`: folder watcher and event filtering.
- `src/app/`: `App` state and the `Msg`/`Effect` state machine — update routing, selection, clipboard, prompts, refresh reconciliation.
- `src/ui/`: ratatui rendering — layout, browser list, help overlay, prompt and confirmation, text truncation, theme tokens.
