# `images`

Source: `src/images.rs`

## Purpose

Loads and caches reader images in the background so opening a note never blocks on image files or remote downloads. The reader draws from the cache only; images that are still loading keep their alt text on the first frame and appear when the event loop refreshes the reader. The cache is bounded, failed URLs are retried only after a cooldown, and remote fetching can be disabled.

## What it handles

- A URL-keyed cache shared across the process through [`ImageStore::shared`]. Clones share one cache, one work queue, and one completion channel.
- A 128 MiB payload budget. Least-recently-used images are evicted when the budget is exceeded; a single image larger than the budget is still kept.
- Local `file:` images re-read when their file size or modification time changes. Remote `http`/`https` images stay cached for the lifetime of the process.
- Failed URLs are remembered for 30 seconds, so a broken image is not retried by the next open or reload; a successful retry clears the failure.
- Remote fetching can be disabled with `ImageStore::set_fetch_remote`. Local images are unaffected.
- SVG rasterization: the worker reads through `crate::svg::SvgAwareResources`, so SVG bytes are stored as PNG and every later read reuses the converted image.
- In-flight dedupe: a URL is queued at most once until its fetch finishes, whether the duplicate request comes from opening the note twice or from repeated prefetches.
- A worker thread that reads files and downloads remote images with one reused `ureq::Agent`, the existing size caps, and the timeout, then reports each successful URL over `mpsc`.
- Cache-first drawing via `ResourceUrlHandler`: cached images return their bytes immediately and everything else returns `io::ErrorKind::WouldBlock`. Drawing never reads a file or the network.
- Batched completions: `drain_completions` returns every URL that loaded since the last call so the caller can refresh and redraw once per batch. Failed loads do not request a redraw.
- Collecting image assets in document order from note markdown (skipping frontmatter and code blocks) and resolving relative targets against the note's base URL.

## What it does not handle

- Markdown rendering, scrolling, or the reader screen. That lives in `src/reader.rs` and `src/ui/reader.rs`.
- When to call `refresh_images` or how the event loop redraws. `src/app` owns the reader refresh and `src/main.rs` drives the loop.
- Choosing which images are near the viewport. `src/reader` orders the prefetch queue and this module preserves that order.
- Math images, which mdcat renders synchronously from LaTeX rather than through a resource URL.

## Dependencies

- `pulldown-cmark` — scanning note source for image tags.
- `pulldown-cmark-mdcat` — `ResourceUrlHandler`, `MimeData`, `FileResourceHandler`, frontmatter stripping, and markdown options.
- `crate::svg` — SVG detection and rasterization through the dispatching resource handler.
- `ureq` — one blocking agent for `http`/`https` downloads with a timeout and a size cap.
- `url` — cache keys and resolving targets against the note's base URL.
- `std::sync::mpsc` and `std::thread` — the worker queue and completion channel. No async runtime is used.
- `std::fs` — reading local images and their metadata for change detection.

## Public API

```rust
pub struct ImageStore { /* shared cache, in-flight set, worker queue, completions */ }

impl ImageStore {
    pub fn new() -> Self;
    pub fn shared() -> Self;
    pub fn prefetch_note(&self, source: &str, base_url: Option<&Url>);
    pub fn prefetch(&self, url: &Url);
    pub fn set_fetch_remote(&self, enabled: bool);
    pub fn has_pending(&self) -> bool;
    pub fn drain_completions(&self) -> Vec<Url>;
}

impl Default for ImageStore { /* delegates to new */ }

impl ResourceUrlHandler for ImageStore {
    fn read_resource(&self, url: &Url) -> std::io::Result<MimeData>;
}

pub fn collect_image_targets(source: &str, base_url: Option<&Url>) -> Vec<Url>;
```

- `new` creates an independent store with its own worker; `shared` returns the process-wide store that the app and every reader use. Tests use `new` to keep caches apart.
- `prefetch_note` collects, resolves, and queues the note's images in document order; `prefetch` queues one URL and skips cached, in-flight, cooling-down, or disabled-remote entries.
- `set_fetch_remote` toggles network fetching; the binary applies `fetch_remote_images` from the config at startup.
- `has_pending` is true while any queued load is unfinished. The event loop shortens its poll timeout while it is true.
- `drain_completions` returns all URLs that loaded since the last call, in completion order.
- `read_resource` is cache-only: cached bytes or `WouldBlock`. Unsupported schemes report `Unsupported`.
- `collect_image_targets` returns deduplicated resolved URLs and skips images inside code blocks.

## Example

```rust
use naskah::images::ImageStore;

let images = ImageStore::shared();
images.set_fetch_remote(config.fetch_remote_images);
images.prefetch_note(source, Some(&note_directory_url));

// mdcat calls ImageStore::read_resource while drawing. It returns cached
// bytes or WouldBlock and never performs image I/O itself.

// in the event loop:
if !images.drain_completions().is_empty() {
    app.refresh_reader_images();
    redraw();
}
let timeout = if images.has_pending() { fast_poll } else { normal_poll };
```
