# `ui`

Source: `src/ui/mod.rs` and its submodules

## Purpose

Renders the browser or the reader from `App` state with ratatui. Every draw is a pure function of the app state and the terminal area.

## What it handles

- Browser screen: four layout regions — header, message row, list, and status/help row.
- Header brand, home-shortened left-truncated breadcrumb with the parent directories dimmed and the final segment brighter, and a dimmed right-aligned note count and cursor position.
- Grouped `FOLDERS`/`NOTES` list with section labels, a gap between groups, cursor rail, two-cell glyph gutter plus two-space entry offset (entry titles sit one cell further right than the section text), bold lifted-slate cursor tint, bold selected-row tint, folder and note styles, and relative dates. Folders use `FOLDER` normally and the brighter `SELECTED_FOLDER` on the cursor or a selected row.
- Selection and clipboard indicators: a `✓` for selected entries, `⧉` for copied and `✂` for cut entries. Clipboard icons take precedence over `✓`. The cursor owns the first gutter cell, so the icon moves to the second cell on the cursor row.
- Header summary of visual mode (`VISUAL`), clipboard (`copy <n>`/`cut <n>`), and selection (`<n> sel`) before the note count and cursor position.
- Visual-mode status row (`VISUAL · j/k extend · v commit · esc cancel`).
- Prompt row: the `New note: `, `New folder: `, or `Rename: ` label with the typed value, a block cursor, and a dim `note.md`/`folder` placeholder while empty.
- Trash confirmation row (`Trash "name"? (y/N)`).
- Full key reference in the `?` overlay.
- Relative dates: `Today`, `Yesterday`, a weekday for the last week, then a short date. Dates are omitted on narrow terminals.
- Empty state: `no markdown notes here — a new note · A new folder`.
- Default help bar, operation status, and the error row under the header (validation, editor, and watcher errors share the row; validation takes precedence).
- Centered `?` help overlay with the current key bindings.
- Reader screen: a centered reading column capped at 100 cells, a header with the brand, home-shortened note path with the parent directories dimmed and the filename brighter, and a dimmed scroll percentage, the markdown text scrolled to the reading position (a stateful image widget with Kitty graphics when the note has a picker, otherwise a plain paragraph), and a help row with the reader keys including `e` edit and `q/h/esc` back. When `App::error` or `App::watcher_error` is set, the help row shows `! <error>` in the error style instead of the keys, truncated to the column width. `content_rect` exposes the column rectangle so the event loop can paint Zellij images at the same coordinates.
- Width-aware truncation that respects grapheme clusters and wide characters, and home-shortened path display.
- Choosing the reader screen whenever `App::reader` is set; otherwise the browser.

## What it does not handle

- App state, key handling, scroll calculation, markdown parsing, filesystem access, or image fetching.
- Reader images on protocols other than Kitty.

## Dependencies

- `ratatui` — layout, widgets, styles, and the frame.
- `crate::reader` — rendered markdown and scroll state for the reader screen.
- `chrono` — local relative date labels.
- `unicode-width` — display widths for truncation and padding.
- `unicode-segmentation` — grapheme-aware truncation.

## Public API

```rust
pub fn draw(frame: &mut Frame, app: &mut App);
```

The submodules are also public for focused testing and reuse:

```rust
pub mod browser;
pub mod header;
pub mod help;
pub mod layout;
pub mod prompt;
pub mod reader;
pub mod text;
pub mod theme;
```

- `layout::split(area)` divides the browser frame into `Areas { header, message, list, status }`.
- `reader::draw(frame, reader, error, area)` draws the reader screen; the reader text, scroll state, and image widget come from `crate::reader::Reader` (mutable for the stateful image widget), and `error` is the browser `error` or `watcher_error` shown in the help row.
- `prompt::draw_prompt(frame, prompt, area)` renders the status-row input; `prompt::draw_confirm(frame, confirm, area)` renders the trash question.
- `text::truncate_right(text, max_width)` and `text::truncate_left(text, max_width)` return strings that fit the given cell width and end or start with `…` when shortened.
- `text::display_path(path)` shortens a path under the home directory to `~` form.
- `text::split_last_segment(display)` splits a display path into its parent head (including the trailing `/`) and its final segment for two-tone breadcrumb rendering.
- `theme` contains style tokens drawn from the Tokyo Night // WOPR palette (`wargames-tokyonight-color-palette.md`): `breadcrumb` is the editor foreground, `breadcrumb_dim`/`summary`/`section`/`date` use the dim comment slate, and `summary` stays dimmed. `cursor_row` is bold terminal-white on terminal-black navy (`#1e2742`), `multi_row` is bold on editor-background night (`#0a0f1e`), `selected_folder`/`brand`/`help_key` use terminal cyan (`#5ac8ff`), `marker`/`status`/`empty_key` use terminal green (`#5cffc6`), and `warning` uses terminal yellow (`#ffcc66`).

## Example

```rust
let mut terminal = Terminal::new(TestBackend::new(80, 24))?;
terminal.draw(|frame| ui::draw(frame, &app))?;
```
