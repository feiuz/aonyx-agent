//! Render still images as ratatui [`Line`]s using the half-block trick:
//! each terminal cell hosts two pixels stacked vertically via the `▀`
//! glyph with `fg = top pixel` and `bg = bottom pixel` (Phase N).
//!
//! This compromise picks universal compatibility over per-protocol
//! quality: no Kitty / iTerm / Sixel detection, just plain ANSI
//! truecolor that every modern terminal renders. Resolution is lower
//! than a real graphics protocol, but the image is immediately
//! recognisable and the same code path runs on every OS / terminal we
//! ship for.

use std::path::Path;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Maximum cell width for an inline image preview. Tuned to fit inside
/// a default 80-column terminal with comfortable padding.
pub const MAX_PREVIEW_WIDTH: u32 = 64;
/// Maximum cell *rows* for an inline image preview. Two pixels stack
/// in one cell row via the `▀` trick, so this bounds 2× pixel height.
pub const MAX_PREVIEW_ROWS: u32 = 24;

/// What can fail when trying to render an `@image.png` reference.
#[derive(Debug)]
pub enum ImageError {
    /// Could not read or decode the file (corrupt, unsupported format,
    /// permission denied, …).
    Decode(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Decode(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ImageError {}

/// Look at the file extension to decide whether a path is worth
/// attempting to decode. Bare existence test — does not open the file.
pub fn looks_like_image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Decode an image and render it as half-block ratatui lines.
///
/// The image is scaled (preserving aspect ratio) to fit inside
/// (`max_cols`, `max_rows`) cells — remember that one cell row carries
/// two pixel rows. Returns the rendered lines plus the original image
/// dimensions so callers can show a `(WxH)` hint to the user.
pub fn render(path: &Path) -> Result<RenderedImage, ImageError> {
    render_bounded(path, MAX_PREVIEW_WIDTH, MAX_PREVIEW_ROWS)
}

/// Like [`render`] but with caller-controlled bounds.
pub fn render_bounded(
    path: &Path,
    max_cols: u32,
    max_rows: u32,
) -> Result<RenderedImage, ImageError> {
    let img = image::open(path).map_err(|e| ImageError::Decode(format!("decode {path:?}: {e}")))?;
    Ok(half_block_lines(&img, max_cols, max_rows))
}

/// Pixel-dimension metadata + the rendered cell lines.
#[derive(Debug, Clone)]
pub struct RenderedImage {
    /// Original image width in pixels.
    pub width: u32,
    /// Original image height in pixels.
    pub height: u32,
    /// One ratatui line per terminal cell row.
    pub lines: Vec<Line<'static>>,
}

fn half_block_lines(img: &DynamicImage, max_cols: u32, max_rows: u32) -> RenderedImage {
    let (orig_w, orig_h) = img.dimensions();

    // Each cell row carries 2 pixel rows via the `▀` trick, so the
    // pixel target is (cols, rows*2). Resize preserving aspect ratio.
    let target_w = max_cols.max(1);
    let target_h = max_rows.max(1).saturating_mul(2);
    let scaled = if orig_w == 0 || orig_h == 0 {
        img.clone()
    } else {
        img.resize(target_w, target_h, FilterType::Triangle)
    };
    let rgba = scaled.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut lines = Vec::with_capacity((h as usize).div_ceil(2));
    let mut y = 0u32;
    while y < h {
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(w as usize);
        for x in 0..w {
            let top = rgba.get_pixel(x, y).0;
            let bottom = if y + 1 < h {
                rgba.get_pixel(x, y + 1).0
            } else {
                // Odd pixel-row height — fall back to a fully top-only
                // cell so the image doesn't sprout a phantom row.
                [0, 0, 0, 0]
            };
            spans.push(half_block_span(top, bottom));
        }
        lines.push(Line::from(spans));
        y += 2;
    }
    RenderedImage {
        width: orig_w,
        height: orig_h,
        lines,
    }
}

fn half_block_span(top: [u8; 4], bottom: [u8; 4]) -> Span<'static> {
    // Treat fully-transparent pixels as terminal-default (skip both
    // fg + bg so the surrounding theme shines through). Anything
    // partially opaque is treated as opaque — no alpha blending.
    let top_visible = top[3] >= 16;
    let bot_visible = bottom[3] >= 16;
    let style = match (top_visible, bot_visible) {
        (true, true) => Style::default()
            .fg(Color::Rgb(top[0], top[1], top[2]))
            .bg(Color::Rgb(bottom[0], bottom[1], bottom[2])),
        (true, false) => Style::default().fg(Color::Rgb(top[0], top[1], top[2])),
        (false, true) => Style::default().bg(Color::Rgb(bottom[0], bottom[1], bottom[2])),
        (false, false) => Style::default(),
    };
    Span::styled("\u{2580}", style) // ▀ upper half block
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn looks_like_image_matches_common_extensions() {
        assert!(looks_like_image("foo.png"));
        assert!(looks_like_image("FOO.PNG"));
        assert!(looks_like_image("path/to/bar.jpeg"));
        assert!(looks_like_image("baz.gif"));
        assert!(looks_like_image("a.webp"));
        assert!(!looks_like_image("foo.rs"));
        assert!(!looks_like_image("README.md"));
        assert!(!looks_like_image(""));
    }

    #[test]
    fn half_block_lines_clamps_to_max_cells() {
        // 100x100 red square should fit in our 64x24 budget.
        let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(100, 100, Rgba([200, 0, 0, 255]));
        let img = DynamicImage::ImageRgba8(buf);
        let rendered = half_block_lines(&img, 64, 24);
        assert!(rendered.lines.len() <= 24);
        assert!(rendered.lines.iter().all(|l| l.spans.len() <= 64));
        assert_eq!(rendered.width, 100);
        assert_eq!(rendered.height, 100);
    }

    #[test]
    fn half_block_lines_preserves_aspect_for_wide_images() {
        // 200x50 wide image — width should hit the 64 col bound first.
        let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(200, 50, Rgba([50, 150, 50, 255]));
        let img = DynamicImage::ImageRgba8(buf);
        let rendered = half_block_lines(&img, 64, 24);
        let cols = rendered.lines.first().map(|l| l.spans.len()).unwrap_or(0);
        let rows = rendered.lines.len();
        // Wide aspect → near 64-cell width, fewer rows.
        assert!(cols >= 32);
        assert!(rows < 24);
    }

    #[test]
    fn render_bounded_decodes_a_round_trip_png() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().with_extension("png");
        let buf: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(10, 10, Rgba([0, 100, 200, 255]));
        DynamicImage::ImageRgba8(buf).save(&path).unwrap();
        let r = render_bounded(&path, 64, 24).expect("decodes");
        assert_eq!(r.width, 10);
        assert_eq!(r.height, 10);
        assert!(!r.lines.is_empty());
    }
}
