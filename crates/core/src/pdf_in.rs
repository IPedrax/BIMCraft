//! A PDF's page as a picture, to place in a plan (Insert > PDF, `image` of a `.pdf`): drawn by hayro (pure Rust, Apache-2.0
//! OR MIT), over white, at most [`MAX_PLACED_SIDE`] px a side and as fine as 150 dpi, and as its lines, fills and pictures.
//!
//! Text is drawn in the fonts the PDF embeds, its letters' own outlines. A font it only names is looked for on this computer
//! ([`interpreter`]): one of PDF's standard families (Helvetica or Arial, Times or Times New Roman, Courier or Courier New) in
//! the computer's font of that family or one drawn to its widths (Liberation, Arimo), a CID font by its own name; else the
//! fonts hayro carries, PDFium's Foxit fonts (BSD-3, in `NOTICE`), drawn to the standard ones' widths. A font of no standard
//! family is drawn in the standard one nearest it (hayro reads an ordinary font that way, whatever the computer has), unless
//! it is a CID font the computer has, and is named in [`Drawing::missing_fonts`]. Which font draws it is decided where the
//! project is open: the project holds the PDF, never a font.
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
    let fonts = interpreter(&Default::default());
    let pixmap = hayro::render(p, &cache, &fonts, &hayro::RenderSettings::default(), &settings);
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
    /// Rings filled in a colour (red, green, blue, alpha), by the even-odd rule or not; `letter` when they are a glyph's.
    Fill { rings: Vec<Vec<[f32; 2]>>, rgba: [u8; 4], even_odd: bool, letter: bool, bounds: [f32; 4] },
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
/// BIMCraft doesn't draw: patterns and shadings fill grey); and the fonts it names but doesn't hold that are drawn in a
/// stand-in of another family.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub size: [f64; 2],
    pub items: Vec<Item>,
    pub cut_short: bool,
    pub missing_fonts: Vec<String>,
}

/// The most rings of one even-odd fill wound again for the non-zero rule (each looked for inside every other).
const MAX_WOUND: usize = 2_000;

/// The rings of an even-odd fill wound so the non-zero rule, which every output fills by, fills them alike: each one inside an
/// even number of the others one way, inside an odd number the other, so an opening cut in a shape stays open. Rings that
/// cross each other, or past [`MAX_WOUND`], are left as they come.
pub fn wind_even_odd(rings: &mut [Vec<[f32; 2]>]) {
    if rings.len() < 2 || rings.len() > MAX_WOUND {
        return;
    }
    let area = |r: &[[f32; 2]]| {
        let n = r.len();
        (0..n).map(|i| f64::from(r[i][0]) * f64::from(r[(i + 1) % n][1]) - f64::from(r[(i + 1) % n][0]) * f64::from(r[i][1])).sum::<f64>()
    };
    let boxes: Vec<[f32; 4]> = rings.iter().map(|r| bounds_of(r.iter())).collect();
    let inside = |q: [f32; 2], r: &[[f32; 2]]| {
        let (x, y) = (f64::from(q[0]), f64::from(q[1]));
        let mut c = false;
        let n = r.len();
        for i in 0..n {
            let (a, b) = (r[i], r[(i + n - 1) % n]);
            let (ax, ay, bx, by) = (f64::from(a[0]), f64::from(a[1]), f64::from(b[0]), f64::from(b[1]));
            if (ay > y) != (by > y) && x < (bx - ax) * (y - ay) / (by - ay) + ax {
                c = !c;
            }
        }
        c
    };
    let depths: Vec<usize> = (0..rings.len())
        .map(|i| {
            let Some(&q) = rings[i].first() else { return 0 };
            (0..rings.len())
                .filter(|&j| {
                    let b = boxes[j];
                    j != i && q[0] >= b[0] && q[0] <= b[2] && q[1] >= b[1] && q[1] <= b[3] && inside(q, &rings[j])
                })
                .count()
        })
        .collect();
    for (r, depth) in rings.iter_mut().zip(depths) {
        if (area(r) > 0.0) != (depth % 2 == 0) {
            r.reverse();
        }
    }
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

    fn fill(&mut self, path: &hayro::kurbo::BezPath, m: hayro::kurbo::Affine, rgba: [u8; 4], even_odd: bool, letter: bool) {
        let mut rings: Vec<Vec<[f32; 2]>> = self.flatten(path, m).into_iter().map(|(p, _)| p).filter(|p| p.len() >= 3).collect();
        if rings.is_empty() || rgba[3] == 0 {
            return;
        }
        if even_odd {
            wind_even_odd(&mut rings);
        }
        let bounds = bounds_of(rings.iter().flatten());
        self.items.push(Item::Fill { rings, rgba, even_odd, letter, bounds });
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
            DrawMode::Fill(rule) => self.fill(path, props.transform, rgba, matches!(rule, FillRule::EvenOdd), false),
            DrawMode::Stroke(s) => self.stroke(path, props.transform, rgba, s),
            DrawMode::FillAndStroke(rule, s) => {
                self.fill(path, props.transform, rgba, matches!(rule, FillRule::EvenOdd), false);
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
                        _ => self.fill(&o.outline(), m, rgba, false, true),
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

/// The most pages [`thumbnails`] draws, and the longest side it draws one, px.
pub const MAX_THUMBNAILS: usize = 200;
pub const MAX_THUMBNAIL_SIDE: u32 = 512;

/// The pages of the PDF `bytes`, from the first, each drawn over white `side` px on its longer side (at most
/// [`MAX_THUMBNAIL_SIDE`]): handed to `each` as it is drawn, with its number from 1, its width and height and its pixels (red,
/// green, blue and alpha), until `each` says to stop or [`MAX_THUMBNAILS`] are drawn. A page that can't be drawn is passed over.
pub fn thumbnails(bytes: Vec<u8>, side: u32, mut each: impl FnMut(usize, u32, u32, Vec<u8>) -> bool) -> Result<(), String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("it's {} bytes; a PDF is read up to {MAX_BYTES}", bytes.len()));
    }
    if !bytes.starts_with(b"%PDF") {
        return Err("it isn't a PDF".into());
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes).map_err(|e| format!("it isn't a PDF BIMCraft can read: {e:?}"))?;
    let side = f64::from(side.clamp(16, MAX_THUMBNAIL_SIDE));
    let fonts = interpreter(&Default::default());
    for (i, p) in pdf.pages().iter().enumerate().take(MAX_THUMBNAILS) {
        let (w, h) = p.render_dimensions();
        let (w, h) = (f64::from(w), f64::from(h));
        if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
            continue;
        }
        let k = (side / w.max(h)) as f32;
        let settings = hayro::PixmapSettings { x_scale: k, y_scale: k, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
        let pixmap = hayro::render(p, &hayro::RenderCache::new(), &fonts, &hayro::RenderSettings::default(), &settings);
        let (pw, ph) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        // Over white, so every pixel is opaque and its premultiplied colour is its colour.
        let rgba = pixmap.data_as_u8_slice().to_vec();
        if pw == 0 || ph == 0 || rgba.len() != pw as usize * ph as usize * 4 {
            continue;
        }
        if !each(i + 1, pw, ph, rgba) {
            break;
        }
    }
    Ok(())
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
    let missing = Missing::default();
    let mut ctx = hayro::hayro_interpret::Context::new(
        transform,
        hayro::kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)),
        &cache,
        p.xref(),
        interpreter(&missing),
    );
    let mut device = Collect { items: Vec::new(), points: 0, pixels: 0, cut_short: false };
    hayro::hayro_interpret::interpret_page(p, &mut ctx, &mut device);
    if let Ok(mut m) = missing.lock() {
        let mut budget = MAX_LOOKED_AT;
        stand_ins(p.resources(), &mut m, 0, &mut budget);
    }
    let missing_fonts = missing.lock().map(|m| m.iter().cloned().collect()).unwrap_or_default();
    Ok(Drawing { size: [f64::from(w), f64::from(h)], items: device.items, cut_short: device.cut_short, missing_fonts })
}

/// The fonts a page names, doesn't hold and draws in a stand-in of another family.
type Missing = std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>;
/// The most of them named.
const MAX_MISSING: usize = 32;
/// The most fonts and forms of a page's resources looked at for them, and how deep in forms.
const MAX_LOOKED_AT: usize = 4_000;
const MAX_FORM_DEPTH: u32 = 4;

/// The families a font the PDF doesn't hold is drawn true in: PDF's standard ones, which the computer's own or hayro's draw to
/// the same widths.
fn standard_family(name: &str) -> bool {
    let family = family_of(name).to_lowercase();
    ["helvetica", "arial", "times", "times new roman", "courier", "courier new", "symbol", "zapf dingbats"].contains(&family.as_str())
}

/// Puts in `out` the ordinary fonts `res` (a page's resources, or a form's in it, `depth` deep) names but doesn't hold and
/// that are of no standard family: hayro draws them in a standard family's. A CID font is asked for by name, and named by
/// [`interpreter`] if the computer hasn't it.
fn stand_ins(res: &hayro::hayro_syntax::page::Resources<'_>, out: &mut std::collections::BTreeSet<String>, depth: u32, budget: &mut usize) {
    use hayro::hayro_syntax::object::{Dict, Name, Stream};
    for key in res.fonts.keys() {
        if *budget == 0 || out.len() >= MAX_MISSING {
            return;
        }
        *budget -= 1;
        let Some(font) = res.fonts.get::<Dict<'_>>(key.as_str()) else { continue };
        let kind = font.get::<Name<'_>>("Subtype");
        if kind.as_ref().is_some_and(|k| matches!(k.as_str(), "Type0" | "Type3")) {
            continue;
        }
        let held = font
            .get::<Dict<'_>>("FontDescriptor")
            .is_some_and(|d| ["FontFile", "FontFile2", "FontFile3"].iter().any(|k| d.contains_key(k)));
        let Some(name) = font.get::<Name<'_>>("BaseFont") else { continue };
        let name = name.as_str();
        let name = name.split_once('+').filter(|(tag, _)| tag.len() == 6).map_or(name, |(_, n)| n);
        if !held && !standard_family(name) {
            out.insert(name.chars().take(64).collect());
        }
    }
    if depth >= MAX_FORM_DEPTH {
        return;
    }
    for key in res.x_objects.keys() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let Some(form) = res.x_objects.get::<Stream<'_>>(key.as_str()) else { continue };
        if form.dict().get::<Name<'_>>("Subtype").is_none_or(|k| k.as_str() != "Form") {
            continue;
        }
        let Some(inner) = form.dict().get::<Dict<'_>>("Resources") else { continue };
        stand_ins(&hayro::hayro_syntax::page::Resources::new(inner), out, depth + 1, budget);
    }
}

/// How hayro reads a page here: a font the PDF doesn't hold is this computer's ([`system_font`]), else the Foxit font hayro
/// carries for its family; one of no standard family that the computer hasn't either is named in `missing`.
fn interpreter(missing: &Missing) -> hayro::hayro_interpret::InterpreterSettings {
    use hayro::hayro_interpret::font::FontQuery;
    let mut s = hayro::hayro_interpret::InterpreterSettings::default();
    let carried = s.font_resolver.clone();
    let missing = missing.clone();
    s.font_resolver = std::sync::Arc::new(move |q: &FontQuery| {
        if let Some(found) = system_font(crate::text::system::index(), q) {
            return Some(found);
        }
        if let FontQuery::Fallback(f) = q
            && let Some(name) = f.post_script_name.as_ref().or(f.font_name.as_ref()).or(f.font_family.as_ref())
            && let Ok(mut m) = missing.lock()
            && m.len() < MAX_MISSING
        {
            m.insert(name.chars().take(64).collect());
        }
        carried(q)
    });
    s
}

/// The family a PostScript font name names, spaced as a family is written: `ArialMT` Arial, `TimesNewRomanPS-BoldMT` Times New
/// Roman, `Calibri,Bold` Calibri. Its style is left to the font's other fields.
pub fn family_of(name: &str) -> String {
    let name =
        name.split_once('+').filter(|(tag, _)| tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase())).map_or(name, |(_, n)| n);
    let mut base = name.split(['-', ',']).next().unwrap_or(name).trim();
    loop {
        let cut = ["PSMT", "MT", "PS"].iter().find_map(|s| base.strip_suffix(s).filter(|b| !b.is_empty()));
        match cut {
            Some(b) => base = b,
            None => break,
        }
    }
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for ch in base.chars() {
        if ch.is_uppercase() && prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
            out.push(' ');
        }
        out.push(ch);
        prev = Some(ch);
    }
    out
}

/// A family of `index` by `name`, its case, spaces and punctuation aside (`DejaVuSans` is DejaVu Sans).
fn family<'i>(index: &'i crate::text::system::Index, name: &str) -> Option<&'i [crate::text::system::SystemFace]> {
    let bare = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let want = bare(name);
    if want.is_empty() {
        return None;
    }
    index.family(name).or_else(|| index.families.iter().find(|(k, f)| bare(k) == want && !f.is_empty()).map(|(_, f)| f.as_slice()))
}

/// The font of this computer that draws what query `q` asks for: the family it names, else one drawn to its widths or like it
/// (`text::system::substitutes`), the face nearest its weight and slant. Symbol and Zapf Dingbats are left to hayro's own,
/// whose letters are where a PDF expects them.
pub fn system_font(
    index: &crate::text::system::Index,
    q: &hayro::hayro_interpret::font::FontQuery,
) -> Option<(hayro::hayro_interpret::font::FontData, u32)> {
    use hayro::hayro_interpret::font::FontQuery;
    let (names, bold, italic): (Vec<String>, bool, bool) = match q {
        FontQuery::Standard(f) => {
            let ps = f.postscript_name();
            let family = match ps.split('-').next() {
                Some("Helvetica") => "Helvetica",
                Some("Times") => "Times",
                Some("Courier") => "Courier",
                _ => return None,
            };
            (vec![family.into()], ps.contains("Bold"), ps.contains("Oblique") || ps.contains("Italic"))
        }
        FontQuery::Fallback(f) => {
            let mut names: Vec<String> = f.font_family.iter().cloned().collect();
            names.extend([&f.post_script_name, &f.font_name].into_iter().flatten().map(|n| family_of(n)));
            (names, f.is_bold || f.font_weight >= 600, f.is_italic)
        }
    };
    let faces = names.iter().find_map(|n| {
        let alike = crate::text::system::substitutes(n).iter().copied();
        std::iter::once(n.as_str()).chain(alike).find_map(|n| family(index, n))
    })?;
    let want: i32 = if bold { 700 } else { 400 };
    let face = faces.iter().min_by_key(|f| {
        u32::from(f.italic != italic) * 2000
            + (i32::from(f.weight) - want).unsigned_abs()
            + if f.width == 5 { 0 } else { 300 }
            + if f.variable && bold { 150 } else { 0 }
    })?;
    let bytes = crate::text::system::load(&face.path)?;
    Some((bytes, face.index))
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

    /// The fonts its page names that are neither in the PDF nor on this computer, drawn in a stand-in.
    pub fn missing_fonts(&self) -> &[String] {
        &self.drawing.missing_fonts
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
