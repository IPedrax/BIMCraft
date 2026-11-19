//! A PDF's page as a picture, to place in a plan (Insert > PDF, `image` of a `.pdf`): drawn by hayro (pure Rust, Apache-2.0
//! OR MIT), over white, at most [`MAX_PLACED_SIDE`] px a side and as fine as 150 dpi. Text in a standard font the PDF doesn't
//! embed is drawn in the fonts hayro carries, PDFium's Foxit fonts (BSD-3, in `NOTICE`), so it reads as its maker meant.
//!
//! A PDF is hostile: its file is held to [`MAX_BYTES`], a page it hasn't is refused, and what can't be read says so. An
//! encrypted PDF isn't read.

use crate::texture::{MAX_PLACED_SIDE, Picture};

/// The largest PDF read, bytes.
pub const MAX_BYTES: u64 = 128 << 20;
/// How fine a page is drawn at most, dots an inch.
const DPI: f64 = 150.0;
/// The most pixels a page is drawn with: a picture's.
const MAX_PIXELS: f64 = 16_000_000.0;
/// A PDF's unit, a point, in mm.
const POINT: f64 = 25.4 / 72.0;

/// Page `page` (from 1) of the PDF `bytes`, named `name`: its picture, its size on paper in mm, and how many pages the PDF
/// has.
pub fn page(name: &str, bytes: Vec<u8>, page: usize) -> Result<(Picture, [f64; 2], usize), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("it's {} bytes; a PDF is read up to {MAX_BYTES}", bytes.len()));
    }
    if !bytes.starts_with(b"%PDF") {
        return Err("it isn't a PDF".into());
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| format!("it isn't a PDF BIMCraft can read: {e:?}"))?;
    let pages = pdf.pages();
    let count = pages.len();
    let Some(p) = page.checked_sub(1).and_then(|i| pages.get(i)) else {
        return Err(format!("it has {count} pages, not a page {page}"));
    };
    let (w, h) = p.render_dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
        return Err(format!("its page {page} has no size"));
    }
    // Dots for each point: as fine as the dpi, no larger than a picture placed may be.
    let scale = (DPI / 72.0).min(MAX_PLACED_SIDE as f64 / w.max(h)).min((MAX_PIXELS / (w * h)).sqrt()).max(1.0 / w.max(h));
    let settings =
        hayro::PixmapSettings { x_scale: scale as f32, y_scale: scale as f32, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
    let cache = hayro::RenderCache::new();
    let pixmap =
        hayro::render(p, &cache, &hayro::hayro_interpret::InterpreterSettings::default(), &hayro::RenderSettings::default(), &settings);
    let png = pixmap.into_png().map_err(|e| format!("its page {page} couldn't be drawn: {e}"))?;
    let picture = Picture::import_at(&format!("{name} p{page}"), &png, MAX_PLACED_SIDE)?;
    Ok((picture, [w * POINT, h * POINT], count))
}
