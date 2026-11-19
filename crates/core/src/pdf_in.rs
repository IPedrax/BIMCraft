//! A PDF's page as a picture, to place in a plan (Insert > PDF, `image` of a `.pdf`): drawn by hayro (pure Rust, Apache-2.0
//! OR MIT), over white, at most [`MAX_PLACED_SIDE`] px a side and as fine as 150 dpi. Text in a standard font the PDF doesn't
//! embed is drawn in the fonts hayro carries, PDFium's Foxit fonts (BSD-3, in `NOTICE`), so it reads as its maker meant.
//!
//! A PDF is hostile: its file is held to [`MAX_BYTES`], a page it hasn't is refused, and what can't be read says so. An
//! encrypted PDF isn't read.

use crate::texture::{MAX_PLACED_SIDE, Picture};

/// The largest PDF read, bytes.
pub const MAX_BYTES: u64 = 64 << 20;
/// How fine a page is drawn at most, dots an inch.
const DPI: f64 = 150.0;
/// The most pixels a page is drawn with: a picture's.
const MAX_PIXELS: f64 = 16_000_000.0;
/// The longest side of the picture drawn of a page, px.
const PREVIEW_SIDE: usize = MAX_PLACED_SIDE / 2;
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
    // Dots for each point: as fine as the dpi, at most half what a picture placed may be: the page is drawn as its lines and
    // fills, and this is what an older version shows.
    let scale = (DPI / 72.0).min(PREVIEW_SIDE as f64 / w.max(h)).min((MAX_PIXELS / (w * h)).sqrt()).max(1.0 / w.max(h));
    let settings =
        hayro::PixmapSettings { x_scale: scale as f32, y_scale: scale as f32, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
    let cache = hayro::RenderCache::new();
    let pixmap =
        hayro::render(p, &cache, &hayro::hayro_interpret::InterpreterSettings::default(), &hayro::RenderSettings::default(), &settings);
    let png = pixmap.into_png().map_err(|e| format!("its page {page} couldn't be drawn: {e}"))?;
    let picture = Picture::import_at(&format!("{name} p{page}"), &png, MAX_PLACED_SIDE)?;
    Ok((picture, [w * POINT, h * POINT], count))
}

/// The most points a page's drawing holds, all its paths' together: past it, the rest of the page is left out (and said).
pub const MAX_POINTS: usize = 4_000_000;
/// The most pixels the pictures inside a page hold, together.
const MAX_IMAGE_PIXELS: usize = 16_000_000;
/// How close a curve is followed, points (a point is 0.35 mm on paper).
const TOLERANCE: f64 = 0.05;

/// What a page draws, as BIMCraft draws it: in the page's points, x right and y down from its top left corner, as hayro lays
/// a page out to draw it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// Rings filled in a colour (red, green, blue, alpha), by the even-odd rule or not.
    Fill { rings: Vec<Vec<[f32; 2]>>, rgba: [u8; 4], even_odd: bool, bounds: [f32; 4] },
    /// Lines `width` points wide, dashed (on and off, points) or not.
    Stroke { lines: Vec<(Vec<[f32; 2]>, bool)>, rgba: [u8; 4], width: f32, dash: Option<Vec<f32>>, bounds: [f32; 4] },
    /// A picture of `w` by `h` pixels (red, green, blue, alpha), its pixel (x, y) at `m` applied to (x, y) scaled to its own
    /// size `local`.
    Image { w: u32, h: u32, rgba: std::sync::Arc<Vec<u8>>, m: [f64; 6], local: [f64; 2], bounds: [f32; 4] },
}

impl Item {
    /// Its box, points: left, top, right, bottom.
    pub fn bounds(&self) -> [f32; 4] {
        match self {
            Item::Fill { bounds, .. } | Item::Stroke { bounds, .. } | Item::Image { bounds, .. } => *bounds,
        }
    }
}

/// A page's drawing: its size, points, what it draws, and whether some of it was left out (past [`MAX_POINTS`], or what
/// BIMCraft doesn't draw: patterns and shadings fill grey).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub size: [f64; 2],
    pub items: Vec<Item>,
    pub cut_short: bool,
}

/// The box of `pts`.
fn bounds_of<'p>(pts: impl Iterator<Item = &'p [f32; 2]>) -> [f32; 4] {
    pts.fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])])
}

/// Collects what a page draws, through hayro's interpreter.
struct Collect {
    items: Vec<Item>,
    points: usize,
    pixels: usize,
    cut_short: bool,
}

impl Collect {
    /// `path` under `m`, followed as polylines: each with whether it closes.
    fn flatten(&mut self, path: &hayro::kurbo::BezPath, m: hayro::kurbo::Affine) -> Vec<(Vec<[f32; 2]>, bool)> {
        let mut out: Vec<(Vec<[f32; 2]>, bool)> = Vec::new();
        let path = m * path.clone();
        let mut over = false;
        hayro::kurbo::flatten(&path, TOLERANCE, |el| {
            use hayro::kurbo::PathEl;
            if over {
                return;
            }
            match el {
                PathEl::MoveTo(p) => out.push((vec![[p.x as f32, p.y as f32]], false)),
                PathEl::LineTo(p) => match out.last_mut() {
                    Some((pts, _)) => pts.push([p.x as f32, p.y as f32]),
                    None => out.push((vec![[p.x as f32, p.y as f32]], false)),
                },
                PathEl::ClosePath => {
                    if let Some((_, closed)) = out.last_mut() {
                        *closed = true;
                    }
                }
                // Flattened: only lines come.
                _ => {}
            }
            self.points += 1;
            over = self.points > MAX_POINTS;
        });
        if over {
            self.cut_short = true;
        }
        out.retain(|(pts, _)| pts.iter().all(|p| p[0].is_finite() && p[1].is_finite()) && !pts.is_empty());
        out
    }

    /// The colour of `paint`: a pattern's or a shading's, grey (and said).
    fn colour(&mut self, paint: &hayro::hayro_interpret::Paint<'_>) -> [u8; 4] {
        match paint {
            hayro::hayro_interpret::Paint::Color(c) => c.to_rgba().to_rgba8(),
            _ => {
                self.cut_short = true;
                [128, 128, 128, 255]
            }
        }
    }

    fn fill(&mut self, path: &hayro::kurbo::BezPath, m: hayro::kurbo::Affine, rgba: [u8; 4], even_odd: bool) {
        let rings: Vec<Vec<[f32; 2]>> = self.flatten(path, m).into_iter().map(|(p, _)| p).filter(|p| p.len() >= 3).collect();
        if rings.is_empty() || rgba[3] == 0 {
            return;
        }
        let bounds = bounds_of(rings.iter().flatten());
        self.items.push(Item::Fill { rings, rgba, even_odd, bounds });
    }

    fn stroke(&mut self, path: &hayro::kurbo::BezPath, m: hayro::kurbo::Affine, rgba: [u8; 4], s: &hayro::hayro_interpret::StrokeProps) {
        let lines: Vec<(Vec<[f32; 2]>, bool)> = self.flatten(path, m).into_iter().filter(|(p, _)| p.len() >= 2).collect();
        if lines.is_empty() || rgba[3] == 0 {
            return;
        }
        // A width under the transform: as long as a unit is, on average across its two axes.
        let k = m.determinant().abs().sqrt() as f32;
        let dash: Vec<f32> = s.dash_array.iter().map(|d| d * k).collect();
        let dash = (!dash.is_empty() && dash.iter().all(|d| d.is_finite() && *d >= 0.0) && dash.iter().any(|d| *d > 0.0)).then_some(dash);
        let bounds = bounds_of(lines.iter().flat_map(|(p, _)| p));
        self.items.push(Item::Stroke { lines, rgba, width: (s.line_width * k).max(0.0), dash, bounds });
    }
}

impl<'a> hayro::hayro_interpret::Device<'a> for Collect {
    fn draw_path(
        &mut self,
        path: &hayro::kurbo::BezPath,
        props: hayro::hayro_interpret::DrawProps<'a>,
        mode: &hayro::hayro_interpret::DrawMode,
    ) {
        use hayro::hayro_interpret::{DrawMode, FillRule};
        let rgba = self.colour(&props.paint);
        match mode {
            DrawMode::Fill(rule) => self.fill(path, props.transform, rgba, matches!(rule, FillRule::EvenOdd)),
            DrawMode::Stroke(s) => self.stroke(path, props.transform, rgba, s),
            DrawMode::FillAndStroke(rule, s) => {
                self.fill(path, props.transform, rgba, matches!(rule, FillRule::EvenOdd));
                self.stroke(path, props.transform, rgba, s);
            }
            DrawMode::Invisible => {}
        }
    }

    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}

    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}

    fn draw_glyph_run(
        &mut self,
        run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>,
        props: hayro::hayro_interpret::DrawProps<'a>,
        mode: &hayro::hayro_interpret::DrawMode,
    ) {
        use hayro::hayro_interpret::font::Glyph;
        let rgba = self.colour(&props.paint);
        for g in run.glyphs() {
            match &**g {
                Glyph::Outline(o) => {
                    let m = props.transform * g.transform();
                    match mode {
                        hayro::hayro_interpret::DrawMode::Invisible => {}
                        hayro::hayro_interpret::DrawMode::Stroke(s) => self.stroke(&o.outline(), m, rgba, s),
                        _ => self.fill(&o.outline(), m, rgba, false),
                    }
                }
                Glyph::Type3(t) => t.interpret(self, props.transform, g.transform(), &props.paint),
            }
        }
    }

    fn draw_image(&mut self, image: hayro::hayro_interpret::Image<'a, '_>, props: hayro::hayro_interpret::ImageDrawProps<'a>) {
        let hayro::hayro_interpret::Image::Raster(r) = image else {
            self.cut_short = true;
            return;
        };
        let local = [f64::from(r.width()), f64::from(r.height())];
        let mut made = None;
        r.with_rgba(
            |data, alpha| {
                let (w, h) = (data.width(), data.height());
                let n = w as usize * h as usize;
                if n == 0 || self.pixels + n > MAX_IMAGE_PIXELS {
                    return;
                }
                let (rgb, channels): (&[u8], usize) = match &data {
                    hayro::hayro_interpret::ImageData::Rgb(d) => (&d.data, 3),
                    hayro::hayro_interpret::ImageData::Luma(d) => (&d.data, 1),
                };
                if rgb.len() < n * channels {
                    return;
                }
                let a = alpha.filter(|a| a.width == w && a.height == h && a.data.len() >= n);
                let mut rgba = Vec::with_capacity(n * 4);
                for i in 0..n {
                    let (r, g, b) = if channels == 3 { (rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]) } else { (rgb[i], rgb[i], rgb[i]) };
                    rgba.extend([r, g, b, a.as_ref().map_or(255, |a| a.data[i])]);
                }
                self.pixels += n;
                made = Some((w, h, rgba));
            },
            None,
        );
        let Some((w, h, rgba)) = made else { return };
        let m = props.transform.as_coeffs();
        let corners = [[0.0, 0.0], [local[0], 0.0], [local[0], local[1]], [0.0, local[1]]]
            .map(|[x, y]| [(m[0] * x + m[2] * y + m[4]) as f32, (m[1] * x + m[3] * y + m[5]) as f32]);
        if corners.iter().flatten().any(|v| !v.is_finite()) {
            return;
        }
        let bounds = bounds_of(corners.iter());
        self.items.push(Item::Image { w, h, rgba: std::sync::Arc::new(rgba), m, local, bounds });
    }

    fn pop_clip(&mut self) {}

    fn pop_transparency_group(&mut self) {}
}

/// How many pages the PDF `bytes` has, and each one's size on paper, mm.
pub fn pages(bytes: Vec<u8>) -> Result<Vec<[f64; 2]>, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("it's {} bytes; a PDF is read up to {MAX_BYTES}", bytes.len()));
    }
    if !bytes.starts_with(b"%PDF") {
        return Err("it isn't a PDF".into());
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| format!("it isn't a PDF BIMCraft can read: {e:?}"))?;
    Ok(pdf
        .pages()
        .iter()
        .map(|p| {
            let (w, h) = p.render_dimensions();
            [f64::from(w) * POINT, f64::from(h) * POINT]
        })
        .collect())
}

/// What page `page` (from 1) of the PDF `bytes` draws, as lines, fills and pictures.
pub fn drawing(bytes: Vec<u8>, page: usize) -> Result<Drawing, String> {
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
    use hayro::hayro_interpret::util::TransformExt;
    let transform = p.initial_transform(true).to_kurbo();
    let cache = hayro::hayro_interpret::InterpreterCache::new();
    let mut ctx = hayro::hayro_interpret::Context::new(
        transform,
        hayro::kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
        &cache,
        p.xref(),
        hayro::hayro_interpret::InterpreterSettings::default(),
    );
    let mut device = Collect { items: Vec::new(), points: 0, pixels: 0, cut_short: false };
    hayro::hayro_interpret::interpret_page(p, &mut ctx, &mut device);
    Ok(Drawing { size: [f64::from(w), f64::from(h)], items: device.items, cut_short: device.cut_short })
}

/// A PDF's page as a project holds it, beside the picture drawn of it (the one of `Model::images` it is keyed by, which an
/// older version shows): the file, which page, and what it draws, read when the project opens. Cheap to clone.
#[derive(Clone)]
pub struct PdfDoc {
    source: String,
    page: u32,
    bytes: std::sync::Arc<Vec<u8>>,
    drawing: std::sync::Arc<Drawing>,
    packed: std::sync::Arc<std::sync::OnceLock<PdfFile>>,
}

impl std::fmt::Debug for PdfDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PdfDoc({:?} p{}, {} items)", self.source, self.page, self.drawing.items.len())
    }
}

impl PartialEq for PdfDoc {
    fn eq(&self, other: &PdfDoc) -> bool {
        self.source == other.source && self.page == other.page && self.bytes == other.bytes
    }
}

/// How a project file holds one: its name, the page, and the PDF deflated, as base64.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PdfFile {
    source: String,
    page: u32,
    pdf: String,
}

impl PdfDoc {
    /// Page `page` (from 1) of the PDF `bytes`, known by `source`, read; or why it can't be.
    pub fn new(source: &str, bytes: Vec<u8>, page: u32) -> Result<PdfDoc, String> {
        let bytes = std::sync::Arc::new(bytes);
        let drawing = drawing(bytes.to_vec(), usize::try_from(page).unwrap_or(usize::MAX))?;
        Ok(PdfDoc {
            source: source.chars().take(crate::model::MAX_TEXT).collect(),
            page,
            bytes,
            drawing: std::sync::Arc::new(drawing),
            packed: std::sync::Arc::default(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn page(&self) -> u32 {
        self.page
    }

    /// What its page draws.
    pub fn drawing(&self) -> &Drawing {
        &self.drawing
    }

    fn file(&self) -> &PdfFile {
        self.packed.get_or_init(|| PdfFile {
            source: self.source.clone(),
            page: self.page,
            pdf: crate::commands::base64(&crate::cloud::deflate(&self.bytes)),
        })
    }
}

impl serde::Serialize for PdfDoc {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.file().serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for PdfDoc {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<PdfDoc, D::Error> {
        let f = PdfFile::deserialize(d)?;
        let most = usize::try_from(MAX_BYTES).unwrap_or(usize::MAX);
        let bytes = crate::cloud::inflate_of("a PDF", &f.pdf, most).map_err(serde::de::Error::custom)?;
        let doc = PdfDoc::new(&f.source, bytes, f.page).map_err(|e| serde::de::Error::custom(format!("PDF `{}`: {e}", f.source)))?;
        let _ = doc.packed.set(f);
        Ok(doc)
    }
}
