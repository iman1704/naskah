//! Placeholder-free Kitty image display for Zellij.
//!
//! Zellij implements the Kitty graphics protocol but not its unicode
//! placeholders, which is the only way `ratatui-image` shows images. So under
//! Zellij the reader renders text only and this module paints raster and SVG
//! images itself: it locates each image's marker line in the rendered text,
//! swaps those lines for blank spacers, and emits transmit/display/delete
//! sequences around each frame. Unresolvable or undecodable images keep
//! their alt text, and math stays unicode text.
//!
//! Visible images keep stable ids and placement ids: an unchanged placement
//! costs nothing, a moved one is re-emitted in place, and only a new crop is
//! transmitted. Encoded crops stay cached so scrolling back to one is cheap.
//! The caller must free every returned image id when the reader closes,
//! because the ids die with this painter.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Cursor, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageFormat, imageops::FilterType};
use pulldown_cmark_mdcat::resources::ResourceUrlHandler;
use ratatui::text::{Line, Text};
use unicode_width::UnicodeWidthStr;
use url::Url;

use crate::images::{ImageAsset, collect_assets, resolve_reference};

/// Base64 payload bytes per Kitty transmit chunk, matching the mdcat CLI.
const CHUNK_BYTES: usize = 4096;

/// Fallback terminal cell size in pixels when no probe result is available.
pub const DEFAULT_CELL: (u16, u16) = (10, 20);

/// Largest number of transmitted variants kept per image.
const VARIANT_CACHE: usize = 8;

/// Largest number of encoded PNG variants kept per image. Payloads outlive
/// their terminal transmission snapshot, so returning to an old crop does
/// not re-encode it.
const PNG_CACHE: usize = 16;

/// Placement id used for every painter image. Placement ids are scoped to the
/// image id, so one value is enough.
const PLACEMENT_ID: u32 = 1;

/// One raster image with its note coordinates and cached display variants.
#[derive(Debug)]
struct PlacedImage {
    /// Note-text line of the image marker.
    line: usize,
    /// Cell indent within the reading column.
    indent: u16,
    /// Spacer size in rows.
    height: u16,
    /// Resized pixels, for cropping partially visible frames.
    decoded: DynamicImage,
    /// Resolved URL string, for matching images across reflows.
    target: String,
    /// Display variants already transmitted to the terminal.
    variants: HashMap<VariantKey, Variant>,
    /// Variant recency, oldest first.
    variant_order: VecDeque<VariantKey>,
    /// Placement currently on screen.
    live: Option<LivePlacement>,
}

/// A full or cropped rendering of one image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum VariantKey {
    Full,
    Crop { offset: u16, rows: u16 },
}

/// One transmitted display variant with stable image id.
#[derive(Debug)]
struct Variant {
    /// `None` after the terminal data was freed; `png` still caches it.
    id: Option<u32>,
    pixels: (u32, u32),
    png: Vec<u8>,
}

/// Where an image is currently placed on screen.
#[derive(Debug, Clone, Copy)]
struct LivePlacement {
    key: VariantKey,
    image_id: u32,
    x: u16,
    y: u16,
}

impl PlacedImage {
    fn variants(&self) -> impl Iterator<Item = u32> + '_ {
        self.variants.values().filter_map(|variant| variant.id)
    }
}

/// Image state for one open note under Zellij.
#[derive(Debug, Default)]
pub struct ZellijImages {
    images: Vec<PlacedImage>,
    /// Image ids left behind by a rebuild or reflow, freed on the next paint.
    stale_ids: Vec<u32>,
    next_id: u32,
    cell: (u16, u16),
    column_width: u16,
}

impl ZellijImages {
    /// Scan `source` for raster images, resolve and decode them, and swap
    /// their marker lines in `text` for blank spacers. Images that cannot be
    /// resolved or decoded keep their alt text.
    pub fn build(
        source: &str,
        text: &mut Text<'static>,
        base_url: Option<&Url>,
        resources: &dyn ResourceUrlHandler,
        cell: (u16, u16),
        column_width: u16,
    ) -> Self {
        let mut painter = Self {
            images: Vec::new(),
            stale_ids: Vec::new(),
            next_id: 1,
            cell,
            column_width,
        };
        painter.rebuild(source, text, base_url, resources);
        painter
    }

    /// Re-scan `source` after the note changed, reusing nothing. Reference
    /// lines and spacers from the previous build belong to the old `text`.
    pub fn rebuild(
        &mut self,
        source: &str,
        text: &mut Text<'static>,
        base_url: Option<&Url>,
        resources: &dyn ResourceUrlHandler,
    ) {
        self.stale_ids
            .extend(self.images.iter().flat_map(PlacedImage::variants));
        self.images.clear();
        for asset in collect_assets(source) {
            let Some(image) = place_asset(
                &asset,
                text,
                base_url,
                resources,
                self.cell,
                self.column_width,
            ) else {
                continue;
            };
            self.images.push(image);
        }
    }

    /// Re-scan `source` after a reflow at the painter's current column width.
    pub fn reflow(
        &mut self,
        source: &str,
        text: &mut Text<'static>,
        base_url: Option<&Url>,
        resources: &dyn ResourceUrlHandler,
    ) {
        let column_width = self.column_width;
        self.reflow_at_width(source, text, base_url, resources, column_width);
    }

    /// Re-scan `source` after a reflow (same bytes, new wrapping), reusing
    /// decoded pixels and already transmitted variants for images whose
    /// resolved URL did not change.
    pub fn reflow_at_width(
        &mut self,
        source: &str,
        text: &mut Text<'static>,
        base_url: Option<&Url>,
        resources: &dyn ResourceUrlHandler,
        column_width: u16,
    ) {
        let reuse_variants = column_width == self.column_width;
        self.column_width = column_width;
        let mut old = std::mem::take(&mut self.images);
        for asset in collect_assets(source) {
            let reused = old
                .iter()
                .position(|image| image.target == asset.target)
                .map(|index| old.remove(index));
            if let Some(mut reused) = reused {
                if let Some(mut image) = place_decoded(
                    &asset,
                    text,
                    &reused.decoded,
                    reused.target.clone(),
                    self.cell,
                    column_width,
                ) {
                    if reuse_variants {
                        image.variants = std::mem::take(&mut reused.variants);
                        image.variant_order = std::mem::take(&mut reused.variant_order);
                        image.live = reused.live.take();
                    } else {
                        self.stale_ids.extend(reused.variants());
                    }
                    self.images.push(image);
                    continue;
                }
                self.stale_ids.extend(reused.variants());
            }
            let Some(image) =
                place_asset(&asset, text, base_url, resources, self.cell, column_width)
            else {
                continue;
            };
            self.images.push(image);
        }
        self.stale_ids
            .extend(old.iter().flat_map(PlacedImage::variants));
    }

    /// Whether any image is ready to paint.
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Paint visible images at their screen positions and return every image
    /// id the painter currently owns. Image data is transmitted once per
    /// variant; unchanged placements are left alone, moved placements are
    /// re-emitted in place, and hidden ones lose only their placement.
    pub fn paint(
        &mut self,
        writer: &mut dyn Write,
        col_x: u16,
        content_y: u16,
        viewport_rows: usize,
        scroll: usize,
    ) -> io::Result<Vec<u32>> {
        for id in self.stale_ids.drain(..) {
            free_image(writer, id)?;
        }

        let mut next_id = self.next_id;
        for image in &mut self.images {
            let bottom = image.line + image.height as usize;
            if bottom <= scroll || image.line >= scroll + viewport_rows {
                if let Some(live) = image.live.take() {
                    delete_placement(writer, live.image_id)?;
                }
                continue;
            }
            let vis_top = image.line.max(scroll);
            let vis_rows = bottom.min(scroll + viewport_rows) - vis_top;
            if vis_rows == 0 {
                continue;
            }
            let key = if vis_top == image.line && vis_rows == image.height as usize {
                VariantKey::Full
            } else {
                VariantKey::Crop {
                    offset: (vis_top - image.line) as u16,
                    rows: vis_rows as u16,
                }
            };
            let x = col_x.saturating_add(image.indent);
            let y = content_y + (vis_top - scroll) as u16;

            if let Some(live) = image.live
                && live.key == key
            {
                // Kitty moves a placement atomically when the (image,
                // placement) pair is already present, so no delete or
                // retransmit is needed.
                if live.x != x || live.y != y {
                    move_cursor(writer, x, y)?;
                    display_image(writer, live.image_id)?;
                    image.live = Some(LivePlacement { x, y, ..live });
                }
                continue;
            }
            if let Some(live) = image.live.take() {
                delete_placement(writer, live.image_id)?;
            }
            let (image_id, _) = ensure_variant(image, key, self.cell, &mut next_id, writer)?;
            move_cursor(writer, x, y)?;
            display_image(writer, image_id)?;
            image.live = Some(LivePlacement {
                key,
                image_id,
                x,
                y,
            });
        }
        self.next_id = next_id;
        Ok(self.image_ids())
    }

    /// Free every placement and stored image owned by this painter, for
    /// example when the reader closes.
    pub fn clear(&mut self, writer: &mut dyn Write) -> io::Result<()> {
        for id in self.stale_ids.drain(..) {
            free_image(writer, id)?;
        }
        for image in &mut self.images {
            for id in image.variants.values().filter_map(|variant| variant.id) {
                free_image(writer, id)?;
            }
            image.variants.clear();
            image.variant_order.clear();
            image.live = None;
        }
        Ok(())
    }

    fn image_ids(&self) -> Vec<u32> {
        self.images.iter().flat_map(PlacedImage::variants).collect()
    }
}

/// Return the transmitted variant for `key`, transmitting and caching it when
/// needed. Old non-live variants are freed once the per-image cache is full.
fn ensure_variant(
    image: &mut PlacedImage,
    key: VariantKey,
    cell: (u16, u16),
    next_id: &mut u32,
    writer: &mut dyn Write,
) -> io::Result<(u32, (u32, u32))> {
    if image.variants.contains_key(&key) {
        let (pixels, id) = {
            let variant = image
                .variants
                .get_mut(&key)
                .expect("variant was just found");
            let pixels = variant.pixels;
            let id = match variant.id {
                Some(id) => id,
                None => {
                    let id = *next_id;
                    *next_id = next_id.wrapping_add(1).max(1);
                    transmit_image(writer, id, &variant.png, pixels)?;
                    variant.id = Some(id);
                    id
                }
            };
            (pixels, id)
        };
        touch_variant(image, key);
        evict_variants(image, key, writer)?;
        return Ok((id, pixels));
    }

    let cropped;
    let source = match key {
        VariantKey::Full => &image.decoded,
        VariantKey::Crop { offset, rows } => {
            cropped = crop_rows(image, offset as usize, rows as usize, cell)?;
            &cropped
        }
    };
    let (pixels, png) = encode_png(source)?;
    let id = *next_id;
    *next_id = next_id.wrapping_add(1).max(1);
    transmit_image(writer, id, &png, pixels)?;
    image.variants.insert(
        key,
        Variant {
            id: Some(id),
            pixels,
            png,
        },
    );
    image.variant_order.push_back(key);
    evict_variants(image, key, writer)?;
    Ok((id, pixels))
}

/// Advance a variant to the newest position in the cache order.
fn touch_variant(image: &mut PlacedImage, key: VariantKey) {
    image.variant_order.retain(|candidate| *candidate != key);
    image.variant_order.push_back(key);
}

/// Free the least-recently-used non-live terminal payloads over the transmit
/// limit, then drop encoded PNGs over the larger PNG limit.
fn evict_variants(
    image: &mut PlacedImage,
    keep: VariantKey,
    writer: &mut dyn Write,
) -> io::Result<()> {
    let live = image.live.map(|live| live.key);
    while image
        .variants
        .values()
        .filter(|variant| variant.id.is_some())
        .count()
        > VARIANT_CACHE
    {
        let Some(key) = image.variant_order.iter().copied().find(|key| {
            *key != keep
                && *key != VariantKey::Full
                && Some(*key) != live
                && image
                    .variants
                    .get(key)
                    .is_some_and(|variant| variant.id.is_some())
        }) else {
            break;
        };
        if let Some(variant) = image.variants.get_mut(&key)
            && let Some(id) = variant.id.take()
        {
            free_image(writer, id)?;
        }
    }

    while image.variants.len() > PNG_CACHE {
        let Some(position) = image
            .variant_order
            .iter()
            .position(|key| *key != keep && *key != VariantKey::Full && Some(*key) != live)
        else {
            break;
        };
        let key = image
            .variant_order
            .remove(position)
            .expect("variant order position exists");
        if let Some(variant) = image.variants.remove(&key)
            && let Some(id) = variant.id
        {
            free_image(writer, id)?;
        }
    }
    Ok(())
}

/// Resolve, decode, size, and place one asset, mutating `text`.
fn place_asset(
    asset: &ImageAsset,
    text: &mut Text<'static>,
    base_url: Option<&Url>,
    resources: &dyn ResourceUrlHandler,
    cell: (u16, u16),
    column_width: u16,
) -> Option<PlacedImage> {
    let url = resolve_reference(base_url, &asset.target)?;
    let data = resources.read_resource(&url).ok()?;
    // Decoding guesses the format; unresolvable bytes keep their alt text.
    // The image store normally hands back PNG for SVG, but rasterize here
    // too for tests that pass a raw dispatcher.
    let decoded = match image::load_from_memory(&data.data) {
        Ok(decoded) => decoded,
        Err(_) if crate::svg::is_svg_url(&url) || crate::svg::is_svg_data(&data.data) => {
            let png = crate::svg::render_svg_to_png(&data.data).ok()?;
            image::load_from_memory(&png).ok()?
        }
        Err(_) => return None,
    };
    place_decoded(
        asset,
        text,
        &decoded,
        asset.target.clone(),
        cell,
        column_width,
    )
}

/// Size and place an already decoded image, mutating `text`.
fn place_decoded(
    asset: &ImageAsset,
    text: &mut Text<'static>,
    decoded: &DynamicImage,
    target: String,
    cell: (u16, u16),
    column_width: u16,
) -> Option<PlacedImage> {
    let (line, indent, index) = find_marker(text, &asset.alt)?;
    let (width, height) = size_in_cells(decoded.width(), decoded.height(), cell, column_width);
    let pixels = (
        width as u32 * cell.0.max(1) as u32,
        height as u32 * cell.1.max(1) as u32,
    );
    let resized = DynamicImage::ImageRgba8(
        decoded
            .resize_exact(pixels.0.max(1), pixels.1.max(1), FilterType::Triangle)
            .to_rgba8(),
    );
    replace_with_spacers(text, line, height);
    remove_reference(text, index);
    Some(PlacedImage {
        line,
        indent,
        height,
        decoded: resized,
        target,
        variants: HashMap::new(),
        variant_order: VecDeque::new(),
        live: None,
    })
}

/// Spacer size in cells for `pixels`, capped to the reading column.
fn size_in_cells(px_w: u32, px_h: u32, cell: (u16, u16), column_width: u16) -> (u16, u16) {
    let width = px_w
        .div_ceil(cell.0.max(1) as u32)
        .max(1)
        .min(column_width.max(1) as u32) as u16;
    let height = px_h
        .div_ceil(cell.1.max(1) as u32)
        .max(1)
        .min(u16::MAX as u32) as u16;
    (width, height)
}

/// Plain text of one rendered line.
fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Rendered line of an image's `alt[N]` marker.
pub(crate) fn marker_line(text: &Text<'_>, alt: &str) -> Option<usize> {
    find_marker(text, alt).map(|(line, _, _)| line)
}

/// Locate the `alt[N]` marker line, mirroring mdcat's image overlay lookup.
fn find_marker(text: &Text<'_>, alt: &str) -> Option<(usize, u16, usize)> {
    text.lines.iter().enumerate().find_map(|(index, line)| {
        let full = line_text(line);
        let trimmed = full.trim_start();
        let prefix_len = full.len() - trimmed.len();
        let indent = u16::try_from(UnicodeWidthStr::width(&full[..prefix_len])).unwrap_or(u16::MAX);
        let marker = trimmed.strip_prefix(alt)?;
        let marker = marker.strip_prefix('[')?;
        let (number, _) = marker.split_once(']')?;
        number
            .parse()
            .ok()
            .map(|reference| (index, indent, reference))
    })
}

/// Swap the marker line for `height` blank spacer rows.
fn replace_with_spacers(text: &mut Text<'static>, line: usize, height: u16) {
    let blanks = std::iter::repeat_with(|| Line::from(String::new())).take(height.max(1).into());
    text.lines.splice(line..=line, blanks);
}

/// Drop the `[N]: target` reference line for a placed image.
fn remove_reference(text: &mut Text<'static>, index: usize) {
    let prefix = format!("[{index}]: ");
    text.lines
        .retain(|line| !line_text(line).starts_with(&prefix));
}

/// PNG-encode `image` for transmit, returning its pixel size and bytes.
fn encode_png(image: &DynamicImage) -> io::Result<((u32, u32), Vec<u8>)> {
    let pixels = (image.width().max(1), image.height().max(1));
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(io::Error::other)?;
    Ok((pixels, png))
}

/// Crop the visible cell rows of a partially visible image.
fn crop_rows(
    image: &PlacedImage,
    row_offset: usize,
    vis_rows: usize,
    cell: (u16, u16),
) -> io::Result<DynamicImage> {
    let cell_h = cell.1.max(1) as u32;
    let y = row_offset as u32 * cell_h;
    let h = (vis_rows as u32 * cell_h).min(image.decoded.height().saturating_sub(y));
    if h == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "image is not visible",
        ));
    }
    Ok(DynamicImage::ImageRgba8(
        image::imageops::crop_imm(&image.decoded, 0, y, image.decoded.width(), h).to_image(),
    ))
}

/// Free image data and its placements by id, for example after the reader
/// closed.
pub fn delete_images(writer: &mut dyn Write, ids: &[u32]) -> io::Result<()> {
    for &id in ids {
        free_image(writer, id)?;
    }
    Ok(())
}

/// Move the terminal cursor to a 0-based cell.
fn move_cursor(writer: &mut dyn Write, x: u16, y: u16) -> io::Result<()> {
    write!(writer, "\x1b[{};{}H", y + 1, x + 1)
}

/// Transmit PNG bytes under an explicit image id, without displaying them.
fn transmit_image(
    writer: &mut dyn Write,
    id: u32,
    png: &[u8],
    pixels: (u32, u32),
) -> io::Result<()> {
    let encoded = STANDARD.encode(png);
    let chunks: Vec<_> = encoded.as_bytes().chunks(CHUNK_BYTES).collect();
    for (position, chunk) in chunks.iter().enumerate() {
        let more = u8::from(position + 1 < chunks.len());
        if position == 0 {
            write!(
                writer,
                "\x1b_Ga=t,t=d,i={id},f=100,s={},v={},m={more},q=2;",
                pixels.0, pixels.1
            )?;
        } else {
            write!(writer, "\x1b_Gm={more},q=2;")?;
        }
        writer.write_all(chunk)?;
        writer.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Display a transmitted image at the cursor.
fn display_image(writer: &mut dyn Write, id: u32) -> io::Result<()> {
    write!(writer, "\x1b_Ga=p,i={id},p={PLACEMENT_ID},q=2\x1b\\")
}

/// Delete only the current placement, keeping the image data.
fn delete_placement(writer: &mut dyn Write, id: u32) -> io::Result<()> {
    write!(writer, "\x1b_Ga=d,d=i,i={id},p={PLACEMENT_ID},q=2\x1b\\")
}

/// Delete an image and its placements, freeing its data.
fn free_image(writer: &mut dyn Write, id: u32) -> io::Result<()> {
    write!(writer, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use pulldown_cmark_mdcat::resources::{DispatchingResourceHandler, FileResourceHandler};

    const CELL: (u16, u16) = (10, 20);

    fn resources() -> DispatchingResourceHandler {
        DispatchingResourceHandler::new(vec![Box::new(FileResourceHandler::new(20_000_000))])
    }

    fn base_url(dir: &std::path::Path) -> Url {
        Url::from_directory_path(dir).expect("directory URL")
    }

    fn write_png(path: &std::path::Path, width: u32, height: u32) {
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        picture.save(path).expect("write png");
    }

    fn note(source: &str) -> Text<'static> {
        // Render like the reader does, then hand the text to the painter.
        let options = pulldown_cmark_mdcat::ratatui::RenderOptions::default().width(60);
        let renderer = pulldown_cmark_mdcat::ratatui::Renderer::new(options);
        renderer.text_from_str(source).expect("render")
    }

    fn marker_text() -> Text<'static> {
        Text::from(vec![
            Line::from(" 体 Logo[2]"),
            Line::from("body"),
            Line::from("[2]: pic.png"),
        ])
    }

    #[test]
    fn collects_images_in_document_order() {
        let assets = collect_assets("![One](a.png)\n\n```\n![Fake](b.png)\n```\n\n![Two](c.png)\n");

        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].target, "a.png");
        assert_eq!(assets[1].target, "c.png");
    }

    #[test]
    fn finds_the_marker_line_and_its_reference() {
        let found = find_marker(&marker_text(), "体 Logo").expect("marker");

        assert_eq!(found.0, 0);
        assert_eq!(found.1, 1);
        assert_eq!(found.2, 2);
    }

    #[test]
    fn mutation_replaces_the_marker_with_spacers_and_drops_the_reference() {
        let mut text = marker_text();

        replace_with_spacers(&mut text, 0, 3);
        remove_reference(&mut text, 2);

        assert_eq!(text.lines.len(), 4);
        assert!(
            text.lines[..3]
                .iter()
                .all(|line| line_text(line).is_empty())
        );
        assert_eq!(line_text(&text.lines[3]), "body");
    }

    #[test]
    fn sizes_images_in_cells_and_caps_to_the_column() {
        assert_eq!(size_in_cells(20, 120, CELL, 60), (2, 6));
        assert_eq!(size_in_cells(2000, 10, CELL, 60), (60, 1));
    }

    #[test]
    fn build_places_a_local_image_and_hides_its_reference() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 120);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());

        let painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        assert_eq!(painter.images.len(), 1);
        let image = &painter.images[0];
        assert_eq!(image.height, 6);
        assert!(
            !text
                .lines
                .iter()
                .any(|line| line_text(line).contains("pic.png"))
        );
    }

    #[test]
    fn build_skips_missing_images_and_keeps_their_alt_text() {
        let temp = tempfile::tempdir().expect("scratch directory");
        let mut text = note("![Gone](nope.png)\n");
        let base = base_url(temp.path());

        let painter = ZellijImages::build(
            "![Gone](nope.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        assert!(painter.is_empty());
        assert!(
            text.lines
                .iter()
                .any(|line| line_text(line).contains("Gone"))
        );
    }

    #[test]
    fn build_places_a_local_svg_and_hides_its_alt_text() {
        let temp = tempfile::tempdir().expect("scratch directory");
        fs::write(
            temp.path().join("pic.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"40\">\
             <rect width=\"20\" height=\"40\" fill=\"red\"/></svg>",
        )
        .expect("write svg");
        let mut text = note("![Vector](pic.svg)\n");
        let base = base_url(temp.path());

        let painter = ZellijImages::build(
            "![Vector](pic.svg)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        assert!(!painter.is_empty());
        assert!(
            !text
                .lines
                .iter()
                .any(|line| line_text(line).contains("Vector"))
        );
    }

    #[test]
    fn build_keeps_the_alt_text_of_an_invalid_svg() {
        let temp = tempfile::tempdir().expect("scratch directory");
        fs::write(temp.path().join("pic.svg"), "<svg").expect("write svg");
        let mut text = note("![Vector](pic.svg)\n");
        let base = base_url(temp.path());

        let painter = ZellijImages::build(
            "![Vector](pic.svg)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        assert!(painter.is_empty());
        assert!(
            text.lines
                .iter()
                .any(|line| line_text(line).contains("Vector"))
        );
    }

    #[test]
    fn paint_transmits_once_and_moves_without_retransmitting() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 40);
        let mut text = note("Before\n\n![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "Before\n\n![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        let mut first = Vec::new();
        let ids = painter.paint(&mut first, 4, 1, 30, 0).expect("first paint");
        let first = String::from_utf8(first).expect("escape sequences");
        assert_eq!(ids, [1]);
        assert!(first.contains("\x1b["));
        assert!(first.contains("a=t") && first.contains("i=1") && first.contains("f=100"));
        assert!(first.contains("a=p") && first.contains("p=1"));

        // An unchanged repaint emits nothing.
        let mut second = Vec::new();
        let ids = painter
            .paint(&mut second, 4, 1, 30, 0)
            .expect("second paint");
        assert!(second.is_empty());
        assert_eq!(ids, [1]);

        // Scrolling moves the existing placement in place.
        let mut third = Vec::new();
        painter.paint(&mut third, 4, 1, 30, 1).expect("moved paint");
        let third = String::from_utf8(third).expect("escape sequences");
        assert!(!third.contains("a=t"));
        assert!(!third.contains("a=d"));
        assert!(third.contains("a=p") && third.contains("i=1"));
    }

    #[test]
    fn hidden_images_keep_their_data_for_later_frames() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 40);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        let mut visible = Vec::new();
        painter.paint(&mut visible, 4, 1, 30, 0).expect("paint");

        let mut hidden = Vec::new();
        painter.paint(&mut hidden, 4, 1, 1, 10).expect("hide");
        let hidden = String::from_utf8(hidden).expect("escape sequences");
        assert!(hidden.contains("a=d,d=i") && hidden.contains("i=1"));
        assert!(!hidden.contains("a=t"));
        assert!(!hidden.contains("d=I"));

        let mut again = Vec::new();
        painter.paint(&mut again, 4, 1, 30, 0).expect("show");
        let again = String::from_utf8(again).expect("escape sequences");
        assert!(!again.contains("a=t"));
        assert!(again.contains("a=p"));
    }

    #[test]
    fn cropped_variants_are_cached_per_image() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 120);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        let mut crop = Vec::new();
        painter.paint(&mut crop, 0, 1, 2, 4).expect("crop");
        let crop = String::from_utf8(crop).expect("escape sequences");
        assert!(crop.contains("a=t") && crop.contains("i=1"));

        let mut full = Vec::new();
        painter.paint(&mut full, 0, 1, 10, 0).expect("full");
        let full = String::from_utf8(full).expect("escape sequences");
        assert!(full.contains("a=t") && full.contains("i=2"));

        // Returning to the cached crop needs no new transmit.
        let mut crop_again = Vec::new();
        painter
            .paint(&mut crop_again, 0, 1, 2, 4)
            .expect("crop again");
        let crop_again = String::from_utf8(crop_again).expect("escape sequences");
        assert!(!crop_again.contains("a=t"));
        assert!(crop_again.contains("a=p") && crop_again.contains("i=1"));
    }

    #[test]
    fn scrolling_through_a_large_image_reuses_cached_variants() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 400);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        for scroll in 0..5 {
            let mut output = Vec::new();
            painter
                .paint(&mut output, 0, 1, 8, scroll)
                .expect("first pass");
        }
        let mut second_pass = Vec::new();
        for scroll in 0..5 {
            painter
                .paint(&mut second_pass, 0, 1, 8, scroll)
                .expect("second pass");
        }
        let second_pass = String::from_utf8(second_pass).expect("escape sequences");

        assert!(!second_pass.contains("a=t"));
        assert!(second_pass.contains("a=p"));
    }

    #[test]
    fn clear_frees_placements_and_image_data() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 40);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );
        let mut output = Vec::new();
        painter.paint(&mut output, 4, 1, 30, 0).expect("paint");

        let mut cleared = Vec::new();
        painter.clear(&mut cleared).expect("clear");
        let cleared = String::from_utf8(cleared).expect("escape sequences");

        assert!(cleared.contains("a=d,d=I") && cleared.contains("i=1"));
        assert!(painter.image_ids().is_empty());
    }

    #[test]
    fn paint_skips_fully_hidden_images() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 40);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        let mut output = Vec::new();
        let ids = painter.paint(&mut output, 4, 1, 1, 10).expect("paint");

        assert!(ids.is_empty());
        assert!(output.is_empty());
    }

    #[test]
    fn paint_crops_partially_visible_images() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 120);
        let mut text = note("![Logo](pic.png)\n");
        let base = base_url(temp.path());
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );
        // The 6-row image starts at line 0; showing viewport rows 4..6 crops it.
        let mut output = Vec::new();
        let ids = painter.paint(&mut output, 0, 1, 2, 4).expect("paint");
        let output = String::from_utf8(output).expect("escape sequences");

        assert_eq!(ids.len(), 1);
        assert!(output.contains("a=t") && output.contains("a=p"));
    }

    #[test]
    fn reflow_reuses_decoded_pixels_without_refetching() {
        let temp = tempfile::tempdir().expect("scratch directory");
        write_png(&temp.path().join("pic.png"), 20, 40);
        let base = base_url(temp.path());
        let mut text = note("![Logo](pic.png)\n");
        let mut painter = ZellijImages::build(
            "![Logo](pic.png)\n",
            &mut text,
            Some(&base),
            &resources(),
            CELL,
            60,
        );

        fs::remove_file(temp.path().join("pic.png")).expect("remove image");
        let mut rewrapped = note("![Logo](pic.png)\n");
        painter.reflow(
            "![Logo](pic.png)\n",
            &mut rewrapped,
            Some(&base),
            &resources(),
        );

        assert_eq!(painter.images.len(), 1);
    }
}
