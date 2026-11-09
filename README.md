# Naskah

A TUI markdown note browser.

## Install

```sh
brew tap iman1704/tap
brew install iman1704/tap/naskah
```

The binary is not notarized by Apple. Homebrew clears the quarantine attribute
after installation; if you built or downloaded a binary manually and macOS
still blocks it, clear the attribute:

```sh
xattr -dr com.apple.quarantine "$(brew --prefix)/Cellar/naskah"
```

### Build from source

```sh
cargo install --locked --path .
```

Requires Rust 1.95 or newer.

## Getting started

Run Naskah from your notes directory, or point it at one via config:

```sh
naskah
```

On first run Naskah creates `~/.config/naskah/naskah.toml` (honours
`XDG_CONFIG_HOME`) with defaults:

```toml
# Directory Naskah opens on startup. Empty means the current working directory.
notes_dir = ""
# Editor used to open notes. Empty falls back to $EDITOR.
editor = ""
# Set to false to skip fetching remote images in the reader.
fetch_remote_images = true
```

Set `notes_dir` to your notes folder (e.g. `~/notes`) and `editor` to your
preferred editor (e.g. `nvim`). If `editor` is empty, `$EDITOR` is used.

Core keys (`?` shows the full reference in-app):

- `j` / `k` or arrow keys — move
- `enter` / `l` — open directory or read note
- `h` — back to parent directory, `q` — quit (or back from the reader)
- `e` — edit note in external editor
- `space` / `v` — select one / select range, `esc` — clear selection
- `a` / `A` — new note / new folder, `R` — rename, `y` / `x` / `p` — copy / cut / paste, `d` — move to trash

## Features

- Directory-based browser for folders and `*.md` files, with vim-style navigation.
- Embedded markdown reader powered by [pulldown-cmark-mdcat](https://crates.io/crates/pulldown-cmark-mdcat), with a centered reading column and images on Kitty/Ghostty with Zellij compatibility.
- Live refresh when notes or folders change on disk, keeping the cursor and selections in place.
- Edit notes in your own editor.
- File management: create, rename, copy, cut/paste, and trash (soft-delete, recoverable).
- Multi-select with `space` and `v` for bulk operations.

## Development

Run from source:

```sh
cargo run
```

Run the tests:

```sh
cargo test
```

Build the binary:

```sh
cargo build --locked --release
```

The binary is written to `target/release/naskah`.

## Roadmap

- Custom colour themes
- Remap hotkeys
- Filter
- Search
- And more
