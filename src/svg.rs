//! SVG to PNG rasterization for the reader.
//!
//! `pulldown-cmark-mdcat`'s Ratatui overlay path decodes raster formats only,
//! so SVG images fall back to alt text without help. This module renders SVG
//! bytes to PNG with `resvg` so the existing Kitty overlay and Zellij painter
//! paths can display them like any raster image. Anything that fails to parse
//! or render keeps its alt text.

use std::io;
use std::sync::{Arc, OnceLock};

use pulldown_cmark_mdcat::resources::{DispatchingResourceHandler, MimeData, ResourceUrlHandler};
use resvg::tiny_skia::{IntSize, Pixmap, Transform};
use resvg::usvg::{self, Tree};
use usvg::fontdb;

/// Shared system font database for SVG text. Loaded once like mdcat does.
static FONTS: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

fn fonts() -> Arc<fontdb::Database> {
    FONTS
        .get_or_init(|| {
            let mut fontdb = fontdb::Database::new();
            fontdb.load_system_fonts();
            Arc::new(fontdb)
        })
        .clone()
}

/// Load the SVG font database ahead of the first SVG image.
///
/// Scanning system fonts can take a noticeable moment. The binary calls this
/// on a background thread at startup so the first SVG does not stall the
/// reader. Repeated calls are cheap.
pub fn warm_fonts() {
    let _ = fonts();
}

/// Whether `data` looks like SVG markup. Trims leading whitespace and an
/// optional UTF-8 BOM, then looks for `<svg` or `<?xml`.
pub fn is_svg_data(data: &[u8]) -> bool {
    let mut bytes = data;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes = &bytes[3..];
    }
    let text = bytes
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .copied()
        .collect::<Vec<_>>();
    if text.starts_with(b"<svg") {
        return true;
    }
    if text.starts_with(b"<?xml") {
        return text.windows(4).any(|window| window == b"<svg");
    }
    false
}

/// Whether `url` points at an `.svg` file (case-insensitive).
pub fn is_svg_url(url: &url::Url) -> bool {
    url.path().to_ascii_lowercase().ends_with(".svg")
}

/// Render SVG bytes to PNG bytes at their native size.
///
/// Returns an error when the SVG cannot be parsed or rasterized; callers fall
/// back to the image's alt text in that case.
pub fn render_svg_to_png(svg: &[u8]) -> io::Result<Vec<u8>> {
    let tree = parse_svg(svg).map_err(io::Error::other)?;
    let native = tree.size().to_int_size();
    let width = native.width().max(1);
    let height = native.height().max(1);
    let size = IntSize::from_wh(width, height).unwrap_or(native);
    let mut pixmap = Pixmap::new(size.width(), size.height())
        .ok_or_else(|| io::Error::other(format!("failed to allocate {size:?} pixmap for SVG")))?;
    resvg::render(&tree, Transform::from_scale(1.0, 1.0), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(io::Error::other)
}

fn parse_svg(svg: &[u8]) -> Result<Tree, usvg::Error> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    Tree::from_data(svg, &options)
}

/// A resource handler that rasterizes SVG to PNG before returning.
///
/// Wraps the normal file plus remote dispatcher. When the resolved bytes look
/// like SVG (by `.svg` URL or markup sniffing) they are rendered to PNG with
/// `resvg`, so mdcat's raster-only Ratatui overlay path and the Zellij painter
/// can display them. Unresolvable or invalid SVG falls through to the original
/// bytes so callers keep the alt-text fallback.
pub struct SvgAwareResources {
    inner: DispatchingResourceHandler,
}

impl SvgAwareResources {
    /// Wrap a file plus remote dispatcher with SVG rasterization.
    pub fn new(inner: DispatchingResourceHandler) -> Self {
        Self { inner }
    }
}

impl ResourceUrlHandler for SvgAwareResources {
    fn read_resource(&self, url: &url::Url) -> io::Result<MimeData> {
        let data = self.inner.read_resource(url)?;
        if (is_svg_url(url) || is_svg_data(&data.data))
            && let Ok(png) = render_svg_to_png(&data.data)
        {
            return Ok(MimeData {
                mime_type: Some("image/png".parse().expect("valid png mime")),
                data: png,
            });
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="red"/></svg>"#;

    #[test]
    fn detects_svg_markup() {
        assert!(is_svg_data(SIMPLE_SVG));
        assert!(is_svg_data(b"  \n<svg></svg>"));
        assert!(is_svg_data(b"<?xml version=\"1.0\"?><svg></svg>"));
        assert!(!is_svg_data(b"\x89PNG\r\n\x1a\n"));
        assert!(!is_svg_data(b"just text"));
    }

    #[test]
    fn detects_svg_urls_case_insensitively() {
        let lower = url::Url::parse("file:///notes/pic.svg").expect("parse url");
        let upper = url::Url::parse("file:///notes/PIC.SVG").expect("parse url");
        let png = url::Url::parse("file:///notes/pic.png").expect("parse url");

        assert!(is_svg_url(&lower));
        assert!(is_svg_url(&upper));
        assert!(!is_svg_url(&png));
    }

    #[test]
    fn renders_a_simple_svg_to_png() {
        let png = render_svg_to_png(SIMPLE_SVG).expect("render svg");

        let decoded = image::load_from_memory(&png).expect("decode png");
        assert_eq!((decoded.width(), decoded.height()), (24, 24));
    }

    #[test]
    fn invalid_svg_is_an_error() {
        assert!(render_svg_to_png(b"<svg").is_err());
        assert!(render_svg_to_png(b"not svg at all <><>").is_err());
    }
}
