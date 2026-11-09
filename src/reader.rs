//! Markdown rendering and scroll state for the embedded reader.
//!
//! On terminals with the Kitty graphics protocol (Kitty and Ghostty) block
//! images render inline as terminal graphics, including SVG images which are
//! rasterized to PNG first; everywhere else images fall back to their alt
//! text. The image-aware line count is established by the first draw, which
//! builds the live widget cache; until then scroll offsets use the text line
//! count.
//!
//! Images load through [`ImageStore`] on a worker thread. Opening a note
//! collects its image targets and returns immediately, so the first frame
//! shows alt text for images that are still loading; the event loop calls
//! [`Reader::refresh_images`] when the cache has them.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::images::{ImageStore, collect_assets, resolve_reference};
use crate::zellij_images::{ZellijImages, marker_line};
use pulldown_cmark::{Event, Parser, Tag};
use pulldown_cmark_mdcat::ratatui::{
    ImageMode, MdcatWidget, MdcatWidgetState, RenderOptions, Renderer,
};
use pulldown_cmark_mdcat::resources::ResourceUrlHandler;
use pulldown_cmark_mdcat::{Environment, markdown_options, strip_frontmatter};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use thiserror::Error;
use url::Url;

pub use pulldown_cmark_mdcat::ratatui::{
    ImageCapability, ImagePicker, ImageProtocol, detect_image_picker,
};

/// Widest reading column, in terminal cells.
pub const MAX_WIDTH: u16 = 100;

/// Rows reserved for the reader header and help row.
pub const CHROME_ROWS: u16 = 2;

/// A note loaded into the embedded reader.
pub struct Reader {
    path: PathBuf,
    source: String,
    content: Text<'static>,
    width: u16,
    viewport: usize,
    scroll: usize,
    /// Rendered line count including image overlay rows.
    lines: usize,
    environment: Option<Environment>,
    images: ImageStore,
    picker: Option<ImagePicker>,
    widget: MdcatWidgetState,
    /// Placeholder-free image painter for Zellij. When set, the reader
    /// renders text only and the painter emits the images around each frame.
    zellij: Option<ZellijImages>,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reader")
            .field("path", &self.path)
            .field("width", &self.width)
            .field("viewport", &self.viewport)
            .field("scroll", &self.scroll)
            .field("lines", &self.lines)
            .field("has_images", &self.has_images())
            .field("has_zellij_painter", &self.zellij.is_some())
            .finish()
    }
}

/// A note read from disk and rendered to text, before image handling.
struct LoadedNote {
    path: PathBuf,
    source: String,
    content: Text<'static>,
    width: u16,
    environment: Option<Environment>,
    images: ImageStore,
}

/// Read `path` and render it for a column `width` cells wide.
fn load(path: &Path, width: u16, images: ImageStore) -> Result<LoadedNote, ReaderError> {
    let source = fs::read_to_string(path).map_err(|source| ReaderError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let source = unique_image_alts(&source);
    let environment = render_environment(path);
    let width = width.max(1);
    let content = render(environment.as_ref(), &source, width)?;
    Ok(LoadedNote {
        path: path.to_path_buf(),
        source,
        content,
        width,
        environment,
        images,
    })
}

impl Reader {
    /// Read `path` and render it for a column `width` cells wide with a
    /// `viewport`-row reading area.
    ///
    /// Pass the shared Kitty image picker to render block images as terminal
    /// graphics; pass `None` to keep the text fallback. Images are prefetched
    /// through `images` when a picker is present.
    pub fn open(
        path: &Path,
        width: u16,
        viewport: usize,
        picker: Option<ImagePicker>,
        images: ImageStore,
    ) -> Result<Self, ReaderError> {
        let loaded = load(path, width, images)?;
        let lines = loaded.content.lines.len();
        let reader = Self {
            path: loaded.path,
            source: loaded.source,
            content: loaded.content,
            width: loaded.width,
            viewport,
            scroll: 0,
            lines,
            environment: loaded.environment,
            images: loaded.images,
            picker,
            widget: MdcatWidgetState::new(),
            zellij: None,
        };
        if reader.picker.is_some() {
            reader.prefetch_images();
        }
        Ok(reader)
    }

    /// Read `path` and render it for Zellij, where unicode placeholders do
    /// not display. The note renders as text with blank spacers where raster
    /// images belong, and the painter emits those images around each frame.
    /// `cell` is the terminal cell size in pixels, used to size the images.
    pub fn open_zellij(
        path: &Path,
        width: u16,
        viewport: usize,
        cell: (u16, u16),
        images: ImageStore,
    ) -> Result<Self, ReaderError> {
        let loaded = load(path, width, images)?;
        let mut reader = Self {
            path: loaded.path,
            source: loaded.source,
            content: loaded.content,
            width: loaded.width,
            viewport,
            scroll: 0,
            lines: 0,
            environment: loaded.environment,
            images: loaded.images,
            picker: None,
            widget: MdcatWidgetState::new(),
            zellij: None,
        };
        reader.prefetch_images();
        let base_url = reader.base_url();
        let painter = ZellijImages::build(
            &reader.source,
            &mut reader.content,
            base_url.as_ref(),
            &reader.images,
            cell,
            reader.width,
        );
        reader.lines = reader.content.lines.len();
        reader.zellij = Some(painter);
        Ok(reader)
    }

    /// Path of the note being read.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Rendered markdown lines (the text fallback, also used for scrolling
    /// when no image picker is set).
    pub fn content(&self) -> &Text<'static> {
        &self.content
    }

    /// Whether block images render as terminal graphics.
    pub fn has_images(&self) -> bool {
        self.picker.is_some()
    }

    /// Placeholder-free image painter for Zellij, if this reader uses one.
    pub fn zellij_images(&mut self) -> Option<&mut ZellijImages> {
        self.zellij.as_mut()
    }

    /// First rendered line shown in the viewport.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Largest valid scroll offset, or zero when the note fits on screen.
    pub fn max_scroll(&self) -> usize {
        if self.has_images() {
            self.lines.saturating_sub(self.viewport)
        } else {
            self.content.lines.len().saturating_sub(self.viewport)
        }
    }

    /// Scroll position as a whole percentage. A note that fits reads as 100%.
    pub fn percent(&self) -> u16 {
        let max = self.max_scroll();
        if max == 0 {
            100
        } else {
            ((self.scroll as u32 * 100 + max as u32 / 2) / max as u32) as u16
        }
    }

    /// Resize the reading area, re-rendering the note when the column width
    /// changes and keeping the approximate reading position.
    pub fn resize(&mut self, width: u16, viewport: usize) -> Result<(), ReaderError> {
        let ratio = self.ratio();
        let anchor = (self.zellij.is_some() && width.max(1) == self.width)
            .then(|| self.capture_anchor())
            .flatten();
        let width = width.max(1);
        self.viewport = viewport;

        if width != self.width {
            self.content = render(self.environment.as_ref(), &self.source, width)?;
            self.width = width;
            if self.zellij.is_some() {
                let base_url = self.base_url();
                if let Some(painter) = self.zellij.as_mut() {
                    painter.reflow_at_width(
                        &self.source,
                        &mut self.content,
                        base_url.as_ref(),
                        &self.images,
                        width,
                    );
                }
            }
            self.lines = self.content.lines.len();
        }

        self.restore_position(anchor.as_ref(), ratio);
        Ok(())
    }

    /// Re-read the note from disk and re-render it at the current width,
    /// keeping the approximate reading position.
    pub fn reload(&mut self) -> Result<(), ReaderError> {
        let ratio = self.ratio();
        let anchor = self
            .zellij
            .is_some()
            .then(|| self.capture_anchor())
            .flatten();
        let source = fs::read_to_string(&self.path).map_err(|source| ReaderError::Read {
            path: self.path.clone(),
            source,
        })?;
        let source = unique_image_alts(&source);
        self.content = render(self.environment.as_ref(), &source, self.width)?;
        self.source = source;
        self.prefetch_images();
        let base_url = self.base_url();
        if self.zellij.is_some()
            && let Some(painter) = self.zellij.as_mut()
        {
            painter.rebuild(
                &self.source,
                &mut self.content,
                base_url.as_ref(),
                &self.images,
            );
        }
        self.lines = self.content.lines.len();

        self.restore_position(anchor.as_ref(), ratio);
        Ok(())
    }

    /// Show images that finished loading after the note opened.
    ///
    /// Kitty rendering caches the whole document including which images could
    /// be read, so the cache is cleared and the next draw rebuilds it from the
    /// now-populated image store. In Zellij painter mode the text is rendered
    /// again from the source and the painter rebuilds its spacers. The reading
    /// position is kept.
    pub fn refresh_images(&mut self) -> Result<(), ReaderError> {
        if self.picker.is_some() {
            self.widget.clear_cache();
        }
        if self.zellij.is_some() {
            let ratio = self.ratio();
            let anchor = self.capture_anchor();
            self.content = render(self.environment.as_ref(), &self.source, self.width)?;
            let base_url = self.base_url();
            if let Some(painter) = self.zellij.as_mut() {
                painter.reflow_at_width(
                    &self.source,
                    &mut self.content,
                    base_url.as_ref(),
                    &self.images,
                    self.width,
                );
            }
            self.lines = self.content.lines.len();
            self.restore_position(anchor.as_ref(), ratio);
        }
        Ok(())
    }

    /// Draw the note body, with image overlays when a picker is set.
    ///
    /// The surrounding centered column, header, and help row stay in
    /// `src/ui/reader.rs`; this only renders the markdown area.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let renderer = image_renderer(
            self.environment.as_ref(),
            &self.images,
            self.width,
            self.picker.as_ref(),
        );
        self.widget.set_scroll(self.clamped_scroll());
        let widget = MdcatWidget::with_renderer(self.source.as_str(), renderer);
        frame.render_stateful_widget(widget, area, &mut self.widget);
        self.lines = self.widget.total_lines();
        self.clamp();
    }

    /// Scroll by a number of rendered lines.
    pub fn scroll_by(&mut self, delta: isize) {
        let next = if delta < 0 {
            self.scroll.saturating_sub(delta.unsigned_abs())
        } else {
            self.scroll.saturating_add(delta as usize)
        };
        self.scroll = next.min(self.max_scroll());
    }

    /// Scroll by a number of viewport pages.
    pub fn page_by(&mut self, pages: isize) {
        self.scroll_by(pages.saturating_mul(self.viewport.max(1) as isize));
    }

    /// Jump to the first rendered line.
    pub fn top(&mut self) {
        self.scroll = 0;
    }

    /// Jump to the last page of rendered lines.
    pub fn bottom(&mut self) {
        self.scroll = self.max_scroll();
    }

    fn ratio(&self) -> f32 {
        let max = self.max_scroll();
        if max == 0 {
            0.0
        } else {
            self.scroll as f32 / max as f32
        }
    }

    fn clamped_scroll(&self) -> u16 {
        u16::try_from(self.scroll.min(self.max_scroll())).unwrap_or(u16::MAX)
    }

    fn clamp(&mut self) {
        self.scroll = self.scroll.min(self.max_scroll());
    }

    fn base_url(&self) -> Option<Url> {
        base_url(self.environment.as_ref())
    }

    /// Queue the note's images, favouring those within the viewport and one
    /// page beyond it. Remaining targets still load, just later.
    fn prefetch_images(&self) {
        let base_url = self.base_url();
        let horizon = self.scroll.saturating_add(self.viewport.saturating_mul(2));
        let mut near = Vec::new();
        let mut far = Vec::new();
        for asset in collect_assets(&self.source) {
            let Some(url) = resolve_reference(base_url.as_ref(), &asset.target) else {
                continue;
            };
            let line = marker_line(&self.content, &asset.alt).unwrap_or(usize::MAX);
            if line < horizon {
                near.push(url);
            } else {
                far.push(url);
            }
        }
        for url in near.into_iter().chain(far) {
            self.images.prefetch(&url);
        }
    }

    /// Identity of the first visible non-empty line, used to keep that line in
    /// place when images above it insert rows.
    fn capture_anchor(&self) -> Option<ScrollAnchor> {
        for (index, line) in self.content.lines.iter().enumerate().skip(self.scroll) {
            let text = line_text(line);
            if text.trim().is_empty() {
                continue;
            }
            let occurrence = self.content.lines[..index]
                .iter()
                .filter(|line| line_text(line) == text)
                .count();
            return Some(ScrollAnchor {
                text,
                occurrence,
                offset: index - self.scroll,
            });
        }
        None
    }

    fn restore_anchor(&mut self, anchor: &ScrollAnchor) -> bool {
        let mut occurrence = 0;
        for (index, line) in self.content.lines.iter().enumerate() {
            if line_text(line) != anchor.text {
                continue;
            }
            if occurrence == anchor.occurrence {
                self.scroll = index.saturating_sub(anchor.offset);
                return true;
            }
            occurrence += 1;
        }
        false
    }

    /// Restore the captured text anchor, or fall back to the scroll ratio
    /// when reflowing or reloading made the anchor disappear.
    fn restore_position(&mut self, anchor: Option<&ScrollAnchor>, ratio: f32) {
        if ratio <= 0.0 {
            self.scroll = 0;
        } else if ratio >= 1.0 {
            self.scroll = self.max_scroll();
        } else if anchor.is_none_or(|anchor| !self.restore_anchor(anchor)) {
            self.scroll = (ratio * self.max_scroll() as f32).round() as usize;
        }
        self.clamp();
    }
}

/// A visible text line and its distance from the top of the viewport.
#[derive(Debug)]
struct ScrollAnchor {
    text: String,
    occurrence: usize,
    offset: usize,
}

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn base_url(environment: Option<&Environment>) -> Option<Url> {
    environment.map(|environment| environment.base_url.clone())
}

/// Errors from reading or rendering a note.
#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("could not read note {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not render markdown: {0}")]
    Render(#[source] io::Error),
}

fn render_environment(path: &Path) -> Option<Environment> {
    let parent = path.parent().filter(|parent| parent.is_absolute())?;
    Environment::for_local_directory(&parent).ok()
}

/// Give every image a unique alt prefix before handing the note to mdcat.
///
/// mdcat locates the rendered fallback line of an image by matching its alt
/// text. When two images share an alt and an earlier one cannot be loaded, a
/// later image can match the earlier image's fallback and render in the wrong
/// place. Prefixing each alt with a unique run of zero-width spaces keeps the
/// fallbacks distinct; the prefix has no display width, so the visible alt
/// text is unchanged.
fn unique_image_alts(source: &str) -> String {
    let body = strip_frontmatter(source);
    let base = source.len() - body.len();
    let mut positions: Vec<usize> = Parser::new_ext(body, markdown_options(false))
        .into_offset_iter()
        .filter_map(|(event, range)| {
            if !matches!(event, Event::Start(Tag::Image { .. })) {
                return None;
            }
            let start = base + range.start;
            let end = base + range.end;
            let offset = source.get(start..end)?.find("![")?;
            Some(start + offset + "![".len())
        })
        .collect();
    if positions.is_empty() {
        return source.to_owned();
    }
    positions.sort_unstable();
    positions.dedup();

    let mut output = String::with_capacity(source.len() + positions.len());
    let mut cursor = 0;
    for (index, position) in positions.into_iter().enumerate() {
        if position < cursor || !source.is_char_boundary(position) {
            continue;
        }
        output.push_str(&source[cursor..position]);
        output.extend(std::iter::repeat_n('\u{200B}', index + 1));
        cursor = position;
    }
    output.push_str(&source[cursor..]);
    output
}

/// Renderer for image-aware drawing at `width` columns.
///
/// The single lifetime keeps every borrow tied to the reader's fields, which
/// stay disjoint from the widget state borrowed for drawing.
fn image_renderer<'r>(
    environment: Option<&'r Environment>,
    resources: &'r dyn ResourceUrlHandler,
    width: u16,
    picker: Option<&'r ImagePicker>,
) -> Renderer<'r> {
    let mut options = RenderOptions::default().width(width);
    if let Some(environment) = environment {
        options = options.environment(environment.clone());
    }
    options = options.resource_handler(resources);
    options = match picker {
        Some(picker) => options.images(ImageMode::Picker(picker.clone())),
        None => options.images(ImageMode::TextOnly),
    };
    Renderer::new(options)
}

fn render(
    environment: Option<&Environment>,
    source: &str,
    width: u16,
) -> Result<Text<'static>, ReaderError> {
    let mut options = RenderOptions::default().width(width);
    if let Some(environment) = environment {
        options = options.environment(environment.clone());
    }

    Renderer::new(options)
        .text_from_str(source)
        .map_err(ReaderError::Render)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn note(source: &str) -> (tempfile::TempDir, Reader) {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, source).expect("write note");
        let reader = Reader::open(&path, 40, 5, None, ImageStore::new()).expect("open reader");
        (temp, reader)
    }

    /// Wait until every image prefetched by the reader has finished.
    fn wait_for_images(store: &ImageStore) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while store.has_pending() {
            assert!(
                Instant::now() < deadline,
                "reader images did not finish loading"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let _ = store.drain_completions();
    }

    fn kitty_picker() -> ImagePicker {
        let mut picker = ImagePicker::halfblocks();
        picker.set_protocol_type(ImageProtocol::Kitty);
        picker
    }

    fn write_png(path: &Path, width: u32, height: u32) {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        picture.save(path).expect("write png");
    }

    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut png = Vec::new();
        picture
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode png");
        png
    }

    /// Serve `png` once over HTTP on loopback for the remote image test.
    fn serve_png_once(png: Vec<u8>) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&png);
        });
        addr
    }

    /// Serve `png` once, but wait for `release` before answering so a test can
    /// observe the reader before the image finishes loading.
    fn serve_png_when_released(png: Vec<u8>, release: mpsc::Receiver<()>) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = release.recv();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                png.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&png);
        });
        addr
    }

    fn long_note() -> String {
        (0..60)
            .map(|index| format!("Paragraph {index:02}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn content_line(reader: &Reader, needle: &str) -> usize {
        reader
            .content()
            .lines
            .iter()
            .position(|line| line_text(line).contains(needle))
            .unwrap_or_else(|| panic!("missing line containing {needle}"))
    }

    fn rendered_text(reader: &Reader) -> String {
        reader
            .content()
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn buffer_lines(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).expect("cell").symbol())
                    .collect()
            })
            .collect()
    }

    fn kitty_transmitted(terminal: &Terminal<TestBackend>) -> bool {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.symbol().contains("\u{1b}_G"))
    }

    #[test]
    fn opens_and_renders_markdown() {
        let (_temp, reader) = note("# Title\n\nHello **world**\n");

        assert!(reader.path().ends_with("note.md"));
        assert!(!reader.has_images());
        assert!(rendered_text(&reader).contains("Title"));
        assert!(rendered_text(&reader).contains("Hello world"));
    }

    #[test]
    fn scrolling_clamps_at_both_ends() {
        let (_temp, mut reader) = note(&long_note());

        assert!(reader.max_scroll() > 0);
        assert_eq!(reader.scroll(), 0);

        reader.scroll_by(-1);
        assert_eq!(reader.scroll(), 0);

        reader.scroll_by(3);
        assert_eq!(reader.scroll(), 3);

        reader.bottom();
        assert_eq!(reader.scroll(), reader.max_scroll());
        assert_eq!(reader.percent(), 100);

        reader.top();
        assert_eq!(reader.scroll(), 0);
        assert_eq!(reader.percent(), 0);
    }

    #[test]
    fn pages_move_by_the_viewport_height() {
        let (_temp, mut reader) = note(&long_note());

        reader.page_by(1);
        assert_eq!(reader.scroll(), 5);

        reader.page_by(-1);
        assert_eq!(reader.scroll(), 0);
    }

    #[test]
    fn a_note_that_fits_reads_as_complete() {
        let (_temp, reader) = note("Short note\n");

        assert_eq!(reader.max_scroll(), 0);
        assert_eq!(reader.percent(), 100);
    }

    #[test]
    fn resize_keeps_the_reading_position() {
        let (_temp, mut reader) = note(&long_note());

        reader.bottom();
        reader.resize(20, 8).expect("reflow");
        assert_eq!(reader.scroll(), reader.max_scroll());
        assert_eq!(reader.percent(), 100);

        reader.resize(80, 8).expect("reflow");
        assert!(reader.scroll() <= reader.max_scroll());
    }

    #[test]
    fn missing_note_reports_a_read_error() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("missing.md");

        let error = Reader::open(&path, 40, 5, None, ImageStore::new()).expect_err("missing note");

        assert!(matches!(error, ReaderError::Read { .. }));
    }

    #[test]
    fn reload_picks_up_saved_changes_and_keeps_the_position() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, long_note()).expect("write note");
        let mut reader = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open reader");

        reader.bottom();
        assert_eq!(reader.percent(), 100);

        fs::write(&path, format!("{}\n\nAppended line\n", long_note())).expect("rewrite note");
        reader.reload().expect("reload");

        assert!(rendered_text(&reader).contains("Appended line"));
        assert_eq!(reader.percent(), 100);
    }

    #[test]
    fn reload_of_a_removed_note_reports_a_read_error() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, "Hello\n").expect("write note");
        let mut reader = Reader::open(&path, 40, 5, None, ImageStore::new()).expect("open reader");

        fs::remove_file(&path).expect("remove note");

        assert!(matches!(
            reader.reload().expect_err("reload must fail"),
            ReaderError::Read { .. }
        ));
    }

    #[test]
    fn local_image_renders_as_an_overlay_with_a_kitty_picker() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        write_png(&temp.path().join("tall.png"), 20, 120);
        fs::write(&path, "![Tall image](tall.png)\n").expect("write note");

        let plain = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open plain");
        let store = ImageStore::new();
        let mut imaged =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open imaged");

        assert!(imaged.has_images());
        assert!(rendered_text(&plain).contains("Tall image"));
        wait_for_images(&store);
        imaged.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| imaged.draw(frame, frame.area()))
            .expect("draw");
        assert!(imaged.lines > plain.content().lines.len());
    }

    #[test]
    fn missing_image_falls_back_to_alt_text() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, "![Alt text](nope.png)\n").expect("write note");
        let store = ImageStore::new();

        let reader =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open reader");
        wait_for_images(&store);

        assert!(reader.has_images());
        assert_eq!(reader.lines, reader.content().lines.len());
        assert!(rendered_text(&reader).contains("Alt text"));
    }

    #[test]
    fn remote_image_renders_as_an_overlay() {
        let png = png_bytes(20, 120);
        let addr = serve_png_once(png);
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, format!("![Remote](http://{addr}/pic.png)\n")).expect("write note");

        let plain = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open plain");
        let store = ImageStore::new();
        let mut imaged =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open imaged");

        wait_for_images(&store);
        imaged.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| imaged.draw(frame, frame.area()))
            .expect("draw");
        assert!(imaged.lines > plain.content().lines.len());
    }

    fn svg_bytes(width: u32, height: u32) -> Vec<u8> {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\">\
             <rect width=\"{width}\" height=\"{height}\" fill=\"red\"/></svg>"
        )
        .into_bytes()
    }

    #[test]
    fn local_svg_renders_as_an_overlay_with_a_kitty_picker() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(temp.path().join("tall.svg"), svg_bytes(20, 120)).expect("write svg");
        fs::write(&path, "![Tall vector](tall.svg)\n").expect("write note");

        let plain = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open plain");
        let store = ImageStore::new();
        let mut imaged =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open imaged");

        assert!(imaged.has_images());
        assert!(rendered_text(&plain).contains("Tall vector"));
        wait_for_images(&store);
        imaged.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| imaged.draw(frame, frame.area()))
            .expect("draw");
        assert!(imaged.lines > plain.content().lines.len());
    }

    #[test]
    fn remote_svg_renders_as_an_overlay() {
        let addr = serve_png_once(svg_bytes(20, 120));
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, format!("![Remote vector](http://{addr}/pic.svg)\n")).expect("write note");

        let plain = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open plain");
        let store = ImageStore::new();
        let mut imaged =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open imaged");

        wait_for_images(&store);
        imaged.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| imaged.draw(frame, frame.area()))
            .expect("draw");
        assert!(imaged.lines > plain.content().lines.len());
    }

    #[test]
    fn images_appear_after_prefetch_and_refresh() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        write_png(&temp.path().join("tall.png"), 20, 120);
        fs::write(&path, "![Tall image](tall.png)\n").expect("write note");
        let store = ImageStore::new();
        let mut reader =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open reader");

        wait_for_images(&store);
        reader.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| reader.draw(frame, frame.area()))
            .expect("draw");

        assert!(kitty_transmitted(&terminal));
    }

    #[test]
    fn first_frame_falls_back_to_alt_text_while_images_load() {
        let (release, wait) = mpsc::channel();
        let addr = serve_png_when_released(png_bytes(20, 120), wait);
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, format!("![Remote](http://{addr}/pic.png)\n")).expect("write note");
        let store = ImageStore::new();
        let mut reader =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open reader");

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| reader.draw(frame, frame.area()))
            .expect("draw");
        assert!(!kitty_transmitted(&terminal));
        assert!(
            buffer_lines(&terminal)
                .iter()
                .any(|line| line.contains("Remote"))
        );

        release.send(()).expect("release image response");
        wait_for_images(&store);
        reader.refresh_images().expect("refresh images");
        terminal
            .draw(|frame| reader.draw(frame, frame.area()))
            .expect("draw");
        assert!(kitty_transmitted(&terminal));
    }

    #[test]
    fn first_frame_transmits_math_images() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, "Inline $E = mc^2$ math.\n").expect("write note");
        let mut reader = Reader::open(&path, 40, 8, Some(kitty_picker()), ImageStore::new())
            .expect("open reader");

        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| reader.draw(frame, frame.area()))
            .expect("draw");

        assert!(kitty_transmitted(&terminal));
    }

    #[test]
    fn shared_image_alts_get_distinct_markers() {
        let source = "![Logo](a.png)\n\n![Logo](b.png)\n\n```\n![Logo](c.png)\n```\n";

        let rewritten = unique_image_alts(source);

        assert!(rewritten.contains("![\u{200B}Logo](a.png)"));
        assert!(rewritten.contains("![\u{200B}\u{200B}Logo](b.png)"));
        assert!(rewritten.contains("```\n![Logo](c.png)\n```"));
    }

    #[test]
    fn a_missing_image_does_not_steal_a_later_shared_alt() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        write_png(&temp.path().join("real.png"), 20, 120);
        fs::write(
            &path,
            "Before\n\n![The Rust logo](missing.png)\n\nAfter first\n\n\
             ![The Rust logo](real.png)\n\nAfter second\n",
        )
        .expect("write note");
        let store = ImageStore::new();
        let mut reader =
            Reader::open(&path, 40, 30, Some(kitty_picker()), store.clone()).expect("open reader");

        wait_for_images(&store);
        reader.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 30)).expect("terminal");
        terminal
            .draw(|frame| reader.draw(frame, frame.area()))
            .expect("draw");
        let lines = buffer_lines(&terminal);
        let fallback = lines
            .iter()
            .position(|line| line.contains("The Rust logo"))
            .expect("missing image fallback");
        let after_first = lines
            .iter()
            .position(|line| line.contains("After first"))
            .expect("first paragraph");
        let after_second = lines
            .iter()
            .position(|line| line.contains("After second"))
            .expect("second paragraph");

        assert!(fallback < after_first);
        assert!(after_first < after_second);
    }

    #[test]
    fn reopening_a_note_reuses_cached_remote_images() {
        let png = png_bytes(20, 120);
        let addr = serve_png_once(png);
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(&path, format!("![Remote](http://{addr}/pic.png)\n")).expect("write note");
        let store = ImageStore::new();

        let mut first =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open first");
        wait_for_images(&store);
        first.refresh_images().expect("refresh images");
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| first.draw(frame, frame.area()))
            .expect("first draw");
        assert!(kitty_transmitted(&terminal));

        // The cache answers the second open without another request.
        let mut second =
            Reader::open(&path, 40, 8, Some(kitty_picker()), store.clone()).expect("open second");
        assert!(!store.has_pending());
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).expect("terminal");
        terminal
            .draw(|frame| second.draw(frame, frame.area()))
            .expect("second draw");
        assert!(kitty_transmitted(&terminal));
    }

    #[test]
    fn refresh_images_rebuilds_zellij_spacers_and_keeps_the_position() {
        let (release, wait) = mpsc::channel();
        let addr = serve_png_when_released(png_bytes(20, 120), wait);
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(
            &path,
            format!("![Tall image](http://{addr}/pic.png)\n\n{}", long_note()),
        )
        .expect("write note");
        let plain = Reader::open(&path, 40, 8, None, ImageStore::new()).expect("open plain");
        let store = ImageStore::new();
        let mut reader =
            Reader::open_zellij(&path, 40, 8, (10, 20), store.clone()).expect("open zellij reader");

        assert!(
            reader
                .zellij_images()
                .as_ref()
                .is_some_and(|painter| painter.is_empty())
        );
        reader.bottom();
        assert_eq!(reader.percent(), 100);

        release.send(()).expect("release image response");
        wait_for_images(&store);
        reader.refresh_images().expect("refresh images");

        assert!(!reader.zellij_images().as_ref().expect("painter").is_empty());
        assert!(reader.content().lines.len() > plain.content().lines.len());
        assert_eq!(reader.percent(), 100);

        reader.resize(20, 8).expect("reflow");
        assert!(!reader.zellij_images().as_ref().expect("painter").is_empty());
    }

    #[test]
    fn refresh_images_keeps_the_same_text_line_at_the_top() {
        let (release, wait) = mpsc::channel();
        let addr = serve_png_when_released(png_bytes(20, 120), wait);
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        fs::write(
            &path,
            format!("![Tall image](http://{addr}/pic.png)\n\n{}", long_note()),
        )
        .expect("write note");
        let store = ImageStore::new();
        let mut reader =
            Reader::open_zellij(&path, 40, 8, (10, 20), store.clone()).expect("open zellij reader");

        let old_line = content_line(&reader, "Paragraph 10");
        reader.scroll = old_line;

        release.send(()).expect("release image response");
        wait_for_images(&store);
        reader.refresh_images().expect("refresh images");

        let new_line = content_line(&reader, "Paragraph 10");
        assert!(new_line > old_line, "image rows should be inserted above");
        assert_eq!(reader.scroll(), new_line);
    }

    #[test]
    fn repeated_refreshes_do_not_duplicate_zellij_spacers() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let path = temp.path().join("note.md");
        write_png(&temp.path().join("tall.png"), 20, 120);
        fs::write(&path, "![Tall image](tall.png)\n\nAfter\n").expect("write note");
        let store = ImageStore::new();
        let mut reader =
            Reader::open_zellij(&path, 40, 8, (10, 20), store.clone()).expect("open zellij reader");
        wait_for_images(&store);

        reader.refresh_images().expect("first refresh");
        let lines = reader.content().lines.len();
        reader.refresh_images().expect("second refresh");
        reader.refresh_images().expect("third refresh");

        assert_eq!(reader.content().lines.len(), lines);
        assert!(!reader.zellij_images().as_ref().expect("painter").is_empty());
    }
}
