# `svg`

Source: `src/svg.rs`

## Purpose

Rasterizes SVG images to PNG for the embedded reader. `pulldown-cmark-mdcat`'s Ratatui overlay path decodes raster formats only, so without this step SVG images fall back to their alt text.

## What it handles

- Detects SVG by `.svg` URL (case-insensitive) or by markup sniffing (`<svg` or `<?xml` containing `<svg`, after whitespace and BOM).
- Renders SVG bytes to PNG at their native size with `resvg`, using a shared system font database for SVG text.
- `warm_fonts` loads that font database once. The binary calls it on a background thread at startup so the first SVG does not pay the scan cost.
- Wraps the file plus remote dispatcher (`SvgAwareResources`) used by the image store's worker, so both local and remote SVG resolve through the same size caps and timeouts as raster images, returning PNG bytes with a PNG mime type. The converted bytes are what the store caches.
- Returns the original bytes when SVG parsing or rendering fails, so callers keep the alt-text fallback.

## What it does not handle

- No markdown parsing, scrolling, or layout. That lives in `src/reader`.
- No terminal painting or Kitty protocol. The PNG bytes flow through the existing mdcat widget and `src/zellij_images` paths.
- No network fetching or size caps of its own. The inner dispatcher owns the file and remote handlers.

## Dependencies

- `resvg` — SVG parsing and rasterization, with system and in-memory fonts.
- `pulldown-cmark-mdcat` — `DispatchingResourceHandler`, `MimeData`, and `ResourceUrlHandler` for the wrapper.
- `image` (tests only) — decode the rendered PNG to check its size.

## Public API

```rust
pub fn is_svg_data(data: &[u8]) -> bool;
pub fn is_svg_url(url: &url::Url) -> bool;
pub fn render_svg_to_png(svg: &[u8]) -> std::io::Result<Vec<u8>>;
pub fn warm_fonts();

pub struct SvgAwareResources { /* inner dispatcher */ }

impl SvgAwareResources {
    pub fn new(inner: DispatchingResourceHandler) -> Self;
}

impl ResourceUrlHandler for SvgAwareResources {
    fn read_resource(&self, url: &Url) -> std::io::Result<MimeData>;
}
```

- `is_svg_data` sniffs markup bytes; `is_svg_url` checks the `.svg` extension.
- `render_svg_to_png` renders at native size and errors when the SVG cannot be parsed or rasterized.
- `warm_fonts` blocks until the system font database is loaded; repeated calls are cheap.
- `SvgAwareResources::new` wraps the file plus remote dispatcher owned by `src/images`. `read_resource` returns PNG bytes when the source looks like SVG and conversion succeeds, otherwise the original bytes.

## Example

```rust
let inner = DispatchingResourceHandler::new(vec![
    Box::new(FileResourceHandler::new(20_000_000)),
]);
let resources = svg::SvgAwareResources::new(inner);
let png: MimeData = resources.read_resource(&url)?;
for line in &reader.content().lines {
    // PNG bytes decode with image::load_from_memory like any raster image
}
```
