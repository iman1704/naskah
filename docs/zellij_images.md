# `zellij_images`

Source: `src/zellij_images.rs`

## Purpose

Paints raster images in the embedded reader when running inside Zellij. Zellij implements the Kitty graphics protocol but not its unicode placeholders, which is the only way `ratatui-image` (used by the normal Kitty path) displays images — a Kitty picker there would leave blank gaps. So under Zellij the reader renders text only, and this module emits the images itself with explicit image ids, placement ids, and cached display variants.

## What it handles

- Scanning the note source for image assets in document order with `pulldown-cmark`, skipping code blocks. Math is ignored and stays unicode text.
- Locating each image's `alt[N]` marker line in the rendered text, mirroring mdcat's overlay lookup (including the zero-width alt prefixes the reader adds for duplicate alts).
- Resolving each target against the note's base URL and reading it through the reader's image store, which returns cached bytes or `WouldBlock`. Images still loading keep their alt text until `Reader::refresh_images` reflows the painter after the background fetch finishes. SVG is already stored as PNG by the image store, so it paints like any raster image. Targets that cannot be resolved, read, or decoded keep their alt text.
- Decoding raster bytes with the `image` crate, sizing them to the reading column and terminal cell size, downscaling to display pixels, and re-encoding to PNG for transmit.
- Swapping placed image markers for blank spacer rows and dropping their `[N]: target` reference lines, so scrolling and the scroll percentage count image rows with no widget involved.
- Reflowing spacers on resize without refetching (decoded pixels are matched by target and reused), and reflowing again when background images arrive. A reflow with the same column width carries transmitted variants across, so existing images are not retransmitted.
- Painting per frame with stable ids: each visible image gets one explicit image id per display variant and a stable placement id. An unchanged placement emits nothing, a moved placement is re-emitted with the same `(image, placement)` pair for an atomic in-place move, and a hidden image loses only its placement.
- Caching partially visible crops: each unique crop is encoded and transmitted once, then reused when scrolling returns to it. Encoded PNGs are kept even after terminal payloads are freed, so returning to an old crop does not re-encode it. Per-image caches cap both transmitted variants and encoded PNGs, freeing least-recently-used payloads.
- Deleting all owned image ids and placements on reader close, so no image data outlives the reader. Quitting needs no cleanup: restoring the terminal clears the alternate screen the images live on.

## What it does not handle

- The reader screen, scrolling, or text rendering. That lives in `src/reader.rs` and `src/ui/reader.rs`; the painter only mutates the text and paints around frames.
- Terminal probing and mode selection. `src/main.rs` detects Zellij (`ZELLIJ`/`ZELLIJ_SESSION_NAME`), keeps no widget picker there, and drives paint/clear from the event loop.
- Math images or non-raster protocols. Math stays unicode text.
- Font-size changes without reopening: cell size is captured at open and reused until the note is reopened.

## Dependencies

- `pulldown-cmark-mdcat` — frontmatter stripping, markdown options, and `ResourceUrlHandler` for image fetching.
- `crate::images` — the cache-first image store and target resolution.
- `crate::svg` — SVG rasterization fallback for tests that pass a raw dispatcher instead of the store.
- `image` — decoding, downscaling (`Triangle`), cropping, and PNG encoding.
- `base64` — chunked transmit payloads in the mdcat CLI's 4096-byte shape.
- `ratatui` — `Text`/`Line` mutation for spacers and reference removal.
- `unicode-width` — marker indent measurement.
- `url` — resolving targets against the note's base URL.

## Public API

```rust
pub const DEFAULT_CELL: (u16, u16);

pub struct ZellijImages { /* placements, stale ids, variant caches, cell, column width */ }

impl ZellijImages {
    pub fn build(source: &str, text: &mut Text<'static>, base_url: Option<&Url>, resources: &dyn ResourceUrlHandler, cell: (u16, u16), column_width: u16) -> Self;
    pub fn rebuild(&mut self, source: &str, text: &mut Text<'static>, base_url: Option<&Url>, resources: &dyn ResourceUrlHandler);
    pub fn reflow(&mut self, source: &str, text: &mut Text<'static>, base_url: Option<&Url>, resources: &dyn ResourceUrlHandler);
    pub fn reflow_at_width(&mut self, source: &str, text: &mut Text<'static>, base_url: Option<&Url>, resources: &dyn ResourceUrlHandler, column_width: u16);
    pub fn is_empty(&self) -> bool;
    pub fn paint(&mut self, writer: &mut dyn Write, col_x: u16, content_y: u16, viewport_rows: usize, scroll: usize) -> io::Result<Vec<u32>>;
    pub fn clear(&mut self, writer: &mut dyn Write) -> io::Result<()>;
}

pub fn delete_images(writer: &mut dyn Write, ids: &[u32]) -> io::Result<()>;
```

- `DEFAULT_CELL` is the assumed cell size in pixels when no probe result exists.
- `build` places images and mutates `text` in one step; `rebuild` fetches everything fresh (changed note source); `reflow` reuses decoded pixels by target at the current width, and `reflow_at_width` also changes the column width. Both reuse transmitted variants when the width is unchanged.
- `paint` emits only the placement moves, deletions, and new transmissions needed for this frame, then returns every image id the painter currently owns. The caller stores them for `delete_images` on close.
- `clear` frees every owned image and placement without painting anything.

## Example

```rust
let mut painter = ZellijImages::build(source, &mut text, base_url, &resources, (10, 20), 100);
// after each frame:
let ids = painter.paint(&mut stdout, col_x, content_y, viewport_rows, scroll)?;
stdout.flush()?;
// when the reader closes:
zellij_images::delete_images(&mut stdout, &ids)?;
```

## Protocol notes

Images are transmitted once with explicit ids: chunked `a=t,t=d,i=<id>,f=100,s=<w>,v=<h>,m=<more>,q=2`. Placements use `a=p,i=<id>,p=<placement>,q=2`; re-emitting the same pair moves the placement atomically. Hidden images use `a=d,d=i,i=<id>,p=<placement>,q=2`, which removes the placement but keeps the image data. Closing frees data with `a=d,d=I,i=<id>,q=2`. Cursor addressing uses absolute `CSI <row>;<col> H`. The `q=2` flag suppresses responses on every command so nothing pollutes stdin.
