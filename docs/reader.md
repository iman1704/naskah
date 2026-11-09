# `reader`

Source: `src/reader.rs`

## Purpose

Reads a note and renders its markdown into Ratatui text for the embedded reader, then tracks the scroll position of that rendered text. Rendering uses `pulldown-cmark-mdcat`'s Ratatui `Renderer`, so reader output follows mdcat's markdown support. On terminals with the Kitty graphics protocol (Kitty and Ghostty) block images render inline as terminal graphics through mdcat's `MdcatWidget`; under Zellij block images are painted by `src/zellij_images.rs` without unicode placeholders; everywhere else images fall back to their alt text. Images load asynchronously through `src/images`, so opening a note returns immediately and the first frame shows alt text for images that are still loading.

## What it handles

- Reads a note from disk as UTF-8 and renders it with mdcat's default theme, syntax highlighting, and markdown extensions.
- Wraps rendered markdown to a caller-supplied column width and resolves relative links against the note's directory.
- Stores the rendered `Text` plus the note source, column width, viewport height, and scroll offset.
- Scrolling by lines and pages, jumping to the top or bottom, and clamping at both ends.
- A scroll percentage for the reader header. Notes that fit in the viewport read as `100%`.
- Reflowing on width changes and keeping the reading position on viewport changes. In Zellij painter mode the first visible text line is used as an anchor; a width change or a missing anchor falls back to the scroll ratio.
- Re-reading the note from disk and re-rendering it at the current width (`reload`), restoring the text-line anchor in painter mode so saves preserve the reading position; other modes keep the scroll ratio.
- Block images as terminal graphics when opened with a Kitty image picker (`open` takes `Option<ImagePicker>`): image targets are resolved against the note's base URL and prefetched on the `ImageStore` worker, then decoded by mdcat into overlays drawn by `MdcatWidgetState`. Targets within the viewport and one page beyond are queued before the rest of the note, so offscreen images do not compete with visible ones. SVG images are rasterized to PNG by the store, so they render the same as raster images. Missing or unloadable images fall back to their alt text.
- Cache-first drawing: the renderer reads images through `ImageStore`, which only returns cached bytes or `WouldBlock`, so a draw never reads a file or the network. Uncached images keep their alt text on the first frame.
- `refresh_images`: after background loads finish, Kitty's render cache is cleared so the next draw rebuilds the overlays; in Zellij painter mode the note is rendered again and the painter reflowed, which reuses already transmitted variants and restores the first visible text line.
- Scroll offsets measured against the image-adjusted line count while a picker is set, so tall images scroll fully into view. The count is established by the first draw, which builds the live widget cache; until then offsets use the text line count, so opening, resizing, and reloading stay at a single render.
- Making duplicate image alt texts distinct with invisible zero-width prefixes before rendering. mdcat locates an image's fallback line by its alt text, so without this a failed image can make a later image render in the wrong place.
- Reloading also prefetches the new source's images, so a newly referenced image appears once the worker finishes.
- Read errors that include the note path, and render errors that report the renderer failure.

Links render as visible references (for example `text[1]` plus a reference list), because Ratatui text spans cannot carry hyperlink targets. Images render as alt text when no image picker is available.

## What it does not handle

- Reader screen layout, header, or help row. That lives in `src/ui/reader.rs`.
- Key handling or app state. `src/app` owns the open reader and routes keys to it.
- Watching the note file for saves. `src/watch` reports the change and `src/app` calls `reload`.
- Terminal graphics detection. The binary probes once at startup with `detect_image_picker` and shares the picker through `App`; a probe that saw the Kitty capability but fell back is upgraded back to Kitty. Inside Zellij (`ZELLIJ`/`ZELLIJ_SESSION_NAME` set) no widget picker is kept: Zellij's Kitty support does not resolve the unicode placeholders `ratatui-image` displays through, so the reader opens in painter mode instead (see `docs/zellij_images.md`). Only the Kitty protocol is kept otherwise.
- Reader editing (the `e` key emits an editor effect from `src/app`), image protocols other than Kitty, or clickable links.
- Background image loading, caching, or the decision to redraw. Those live in `src/images` and the event loop.
- Themes or syntax-theme configuration; the mdcat defaults are used for now.

## Dependencies

- `pulldown-cmark-mdcat` with the `ratatui` feature — markdown parsing, terminal rendering, Ratatui `Text` conversion, and the image-aware `MdcatWidget`.
- `pulldown-cmark` — locating image tokens so each alt text can be given a unique invisible prefix.
- `crate::images` — process-wide image cache, background prefetch, and the cache-first resource handler.
- `crate::zellij_images` — placeholder-free image painting for the Zellij reader mode.
- `ratatui` — `Text` output, the stateful image widget, and drawing in `Reader::draw`.
- `url` — the note's base URL for resolving and prefetching image targets.
- `thiserror` — typed read and render errors.

## Public API

```rust
pub const MAX_WIDTH: u16 = 100;
pub const CHROME_ROWS: u16 = 2;

pub struct Reader { /* path, source, rendered text, width, viewport, scroll, line count */ }

impl Reader {
    pub fn open(path: &Path, width: u16, viewport: usize, picker: Option<ImagePicker>, images: ImageStore) -> Result<Self, ReaderError>;
    pub fn open_zellij(path: &Path, width: u16, viewport: usize, cell: (u16, u16), images: ImageStore) -> Result<Self, ReaderError>;
    pub fn path(&self) -> &Path;
    pub fn content(&self) -> &Text<'static>;
    pub fn has_images(&self) -> bool;
    pub fn zellij_images(&mut self) -> Option<&mut ZellijImages>;
    pub fn draw(&mut self, frame: &mut Frame, area: Rect);
    pub fn scroll(&self) -> usize;
    pub fn max_scroll(&self) -> usize;
    pub fn percent(&self) -> u16;
    pub fn resize(&mut self, width: u16, viewport: usize) -> Result<(), ReaderError>;
    pub fn reload(&mut self) -> Result<(), ReaderError>;
    pub fn refresh_images(&mut self) -> Result<(), ReaderError>;
    pub fn scroll_by(&mut self, delta: isize);
    pub fn page_by(&mut self, pages: isize);
    pub fn top(&mut self);
    pub fn bottom(&mut self);
}

pub use pulldown_cmark_mdcat::ratatui::{ImageCapability, ImagePicker, ImageProtocol, detect_image_picker};

pub enum ReaderError {
    Read { path: PathBuf, source: std::io::Error },
    Render(std::io::Error),
}
```

- `MAX_WIDTH` caps the centered reading column; `CHROME_ROWS` is the header plus help row count used by the app.
- `open` reads and renders in one step, so a note either opens fully or reports an error before the reader screen is shown. The optional picker enables image overlays; `None` keeps the text fallback. When a picker is set, the note's images are prefetched on `images` after rendering, ordered by their rendered position. The image-adjusted line count is not measured here; the first draw builds the live widget cache and records it.
- `open_zellij` reads and renders the same way and always prefetches in viewport order, then swaps cached raster image markers for blank spacers and attaches the painter from `src/zellij_images.rs`, which emits the images around each frame. Images that are still loading, unresolvable images, SVG, and math keep the text treatment.
- `content` is re-rendered by `resize` when the column width changes. The next draw refreshes the image-adjusted line count, so `max_scroll` counts overlay rows after the first frame. In painter mode `resize` reflows the spacers without refetching images.
- `reload` re-reads the file, prefetches any new image targets, and re-renders at the current width, then restores the text anchor; the next draw refreshes the image-adjusted line count. In painter mode `reload` rebuilds the painter from the cache.
- `refresh_images` is called by the event loop after image completions arrive. It clears the Kitty widget cache so the next draw sees the new images; in painter mode it renders the note again and reflows the painter, keeping the first visible text line. It never prefetches, so a failed image cannot loop into another completion.
- `zellij_images` exposes the painter for the event loop, which paints after each frame and deletes the last frame on reader close.
- `draw` renders the markdown area with image overlays; the caller keeps the header, help row, and centering. It also records the image-adjusted line count, which is what makes `max_scroll` image-aware after the first frame.

## Example

```rust
let width = 100;
let viewport = 30;
let images = naskah::images::ImageStore::shared();
let mut reader = reader::Reader::open(Path::new("/notes/note.md"), width, viewport, None, images)?;

reader.scroll_by(3);
reader.page_by(1);
reader.bottom();

for line in &reader.content().lines {
    // render with a ratatui Paragraph scrolled to reader.scroll(),
    // or call reader.draw(frame, area) for image overlays
}
```
