//! PDF for sheets and views, written with krilla (MIT/Apache, the PDF writer Typst uses): a page a drawing, its lines and
//! fills as paths, its pictures as images; or each page a picture of itself (`raster`). The lettering is our strokes, so a
//! page needs no fonts. A file may be an archive (PDF/A-1b, 2b, 2u, 3b, 4), which krilla checks as it writes: a file that
//! wouldn't be one is refused, not written.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use krilla::configure::{Archival, ConfigurationBuilder, PdfVersion};
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::{BitsPerComponent, CustomImage, Image, ImageColorspace};
use krilla::metadata::{DateTime, Metadata};
use krilla::outline::{Outline, OutlineNode};
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::{Document, SerializeSettings};
use serde::{Deserialize, Serialize};

use crate::geom::P2;
use crate::render::{Drawing, Prim, Rgb};

/// Where a drawing lies on its page: `k` paper mm to the drawing's pixel, its pixel (0, 0) `at` mm from the paper's top left
/// corner, and drawn only inside `clip` (left, top, width, height, mm from the top left) when there is one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub k: f64,
    pub at: P2,
    pub clip: Option<[f64; 4]>,
}

/// A page made a picture: width and height in pixels and their RGBA, covering the whole paper.
pub type Picture = (u32, u32, Vec<u8>);

/// A page: a drawing on `paper` mm, where `place` puts it (none, at full size from the corner: a sheet on its own paper), and
/// its title in the file's bookmarks, if it has one. `marks` are hairlines on the paper (mm from its top left: a tiled page's
/// cut marks), and `picture`, when there is one, is printed in the drawing's place: the page made a picture.
pub struct Page<'a> {
    pub drawing: &'a Drawing,
    pub paper: P2,
    pub place: Option<Place>,
    pub title: Option<String>,
    pub marks: Vec<[P2; 2]>,
    pub picture: Option<Picture>,
}

impl<'a> Page<'a> {
    /// `drawing` at full size from the corner of `paper`, untitled.
    pub fn new(drawing: &'a Drawing, paper: P2) -> Page<'a> {
        Page { drawing, paper, place: None, title: None, marks: Vec::new(), picture: None }
    }
}

/// Which archive standard a file keeps to. The `a` levels (tagged, for accessibility) are not offered: a drawing has no
/// reading order to tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Archive {
    /// An ordinary PDF.
    #[default]
    None,
    /// PDF/A-1b (ISO 19005-1): PDF 1.4, nothing see-through (a picture's clear parts are made white).
    A1b,
    /// PDF/A-2b (ISO 19005-2), the usual archive: PDF 1.7.
    A2b,
    /// PDF/A-2u: 2b, every letter mapped to Unicode (a drawing's letters are strokes, so the same file).
    A2u,
    /// PDF/A-3b (ISO 19005-3): 2b, and files may be attached.
    A3b,
    /// PDF/A-4 (ISO 19005-4): PDF 2.0.
    A4,
}

impl Archive {
    pub const ALL: [Archive; 6] = [Archive::None, Archive::A1b, Archive::A2b, Archive::A2u, Archive::A3b, Archive::A4];

    pub fn label(self) -> &'static str {
        match self {
            Archive::None => "None",
            Archive::A1b => "PDF/A-1b",
            Archive::A2b => "PDF/A-2b",
            Archive::A2u => "PDF/A-2u",
            Archive::A3b => "PDF/A-3b",
            Archive::A4 => "PDF/A-4",
        }
    }

    fn krilla(self) -> Option<Archival> {
        match self {
            Archive::None => None,
            Archive::A1b => Some(Archival::A1_B),
            Archive::A2b => Some(Archival::A2_B),
            Archive::A2u => Some(Archival::A2_U),
            Archive::A3b => Some(Archival::A3_B),
            Archive::A4 => Some(Archival::A4),
        }
    }
}

/// The PDF version of a file that isn't an archive (an archive's is its standard's).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Version {
    #[serde(rename = "1.4")]
    V14,
    #[serde(rename = "1.5")]
    V15,
    #[serde(rename = "1.6")]
    V16,
    #[default]
    #[serde(rename = "1.7")]
    V17,
    #[serde(rename = "2.0")]
    V20,
}

impl Version {
    pub const ALL: [Version; 5] = [Version::V14, Version::V15, Version::V16, Version::V17, Version::V20];

    pub fn label(self) -> &'static str {
        match self {
            Version::V14 => "1.4",
            Version::V15 => "1.5",
            Version::V16 => "1.6",
            Version::V17 => "1.7",
            Version::V20 => "2.0",
        }
    }

    fn krilla(self) -> PdfVersion {
        match self {
            Version::V14 => PdfVersion::Pdf14,
            Version::V15 => PdfVersion::Pdf15,
            Version::V16 => PdfVersion::Pdf16,
            Version::V17 => PdfVersion::Pdf17,
            Version::V20 => PdfVersion::Pdf20,
        }
    }
}

/// How a PDF file is written, besides its pages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub archive: Archive,
    /// Ignored for an archive, which has its standard's.
    pub version: Version,
    /// Each page a picture at the page setup's resolution instead of lines: for a printer or a reader that struggles
    /// with a big drawing, or a file nobody should take the lines out of.
    pub raster: bool,
    /// On a drawing tiled across pages, a hairline in the margin at each edge that meets another page: where to cut it.
    pub cut_marks: bool,
    /// The document's title (none, the first page's bookmark or the file's name), author, subject and keywords.
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Vec<String>,
    /// When it was made, seconds since 1970 (UTC); none, no date.
    pub created: Option<u64>,
}

/// Points to the mm.
const PT: f64 = 72.0 / 25.4;

/// A picture of RGB pixels and, when any isn't all there, their alpha: an image krilla writes as it is.
#[derive(Clone)]
struct Pixels {
    size: (u32, u32),
    rgb: Arc<Vec<u8>>,
    alpha: Option<Arc<Vec<u8>>>,
    hash: u64,
}

impl Hash for Pixels {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl CustomImage for Pixels {
    fn color_channel(&self) -> &[u8] {
        &self.rgb
    }
    fn alpha_channel(&self) -> Option<&[u8]> {
        self.alpha.as_deref().map(Vec::as_slice)
    }
    fn bits_per_component(&self) -> BitsPerComponent {
        BitsPerComponent::Eight
    }
    fn size(&self) -> (u32, u32) {
        self.size
    }
    fn icc_profile(&self) -> Option<&[u8]> {
        None
    }
    fn color_space(&self) -> ImageColorspace {
        ImageColorspace::Rgb
    }
}

/// RGBA pixels as an image: their alpha kept where any isn't opaque, unless `opaque` asks it laid on white (PDF/A-1 has
/// nothing see-through). None when the pixels aren't as many as the size says.
fn image(width: u32, height: u32, rgba: &[u8], opaque: bool) -> Option<Image> {
    let n = (width as usize).checked_mul(height as usize)?;
    if width == 0 || height == 0 || rgba.len() != n.checked_mul(4)? {
        return None;
    }
    let pixels = rgba.as_chunks::<4>().0;
    let clear = pixels.iter().any(|[.., a]| *a < 255);
    let over_white = |c: u8, a: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a))) / 255) as u8;
    let rgb: Vec<u8> = if clear && opaque {
        pixels.iter().flat_map(|[r, g, b, a]| [over_white(*r, *a), over_white(*g, *a), over_white(*b, *a)]).collect()
    } else {
        pixels.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()
    };
    let alpha = (clear && !opaque).then(|| pixels.iter().map(|[.., a]| *a).collect::<Vec<u8>>());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (width, height, &rgb, &alpha).hash(&mut h);
    let pixels = Pixels { size: (width, height), rgb: Arc::new(rgb), alpha: alpha.map(Arc::new), hash: h.finish() };
    Image::from_custom(pixels, false).ok()
}

fn colour(c: Rgb) -> krilla::color::rgb::Color {
    krilla::color::rgb::Color::new(c.0, c.1, c.2)
}

/// A path through `pts`, closed or not; none when it has no length to draw.
fn path(rings: &[&[(f32, f32)]], closed: bool) -> Option<krilla::geom::Path> {
    let mut pb = PathBuilder::new();
    for pts in rings {
        let mut points = pts.iter().filter(|(x, y)| x.is_finite() && y.is_finite());
        let Some((x, y)) = points.next() else { continue };
        pb.move_to(*x, *y);
        for (x, y) in points {
            pb.line_to(*x, *y);
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}

/// Draws the drawing of a page on `surface`, in points from the paper's top left.
fn draw(surface: &mut Surface, d: &Drawing, place: Option<Place>, opaque: bool) {
    let Place { k, at, clip } = place.unwrap_or(Place { k: 1.0 / d.view.scale, at: P2(0.0, 0.0), clip: None });
    let mut pushed = 0;
    if let Some([l, t, w, h]) = clip {
        let rect = Rect::from_xywh((l * PT) as f32, (t * PT) as f32, (w * PT) as f32, (h * PT) as f32);
        let clip = rect.and_then(|r| {
            let mut pb = PathBuilder::new();
            pb.push_rect(r);
            pb.finish()
        });
        if let Some(clip) = clip {
            surface.push_clip_path(&clip, &FillRule::NonZero);
            pushed += 1;
        }
    }
    let s = (k * PT) as f32;
    surface.push_transform(&Transform::from_row(s, 0.0, 0.0, s, (at.0 * PT) as f32, (at.1 * PT) as f32));
    pushed += 1;
    for (i, p) in d.prims.iter().enumerate() {
        match p {
            Prim::Fill { rings, color } => {
                let rings: Vec<&[(f32, f32)]> = rings.iter().map(Vec::as_slice).collect();
                let Some(path) = path(&rings, true) else { continue };
                surface.set_stroke(None);
                surface.set_fill(Some(Fill { paint: colour(*color).into(), rule: FillRule::NonZero, ..Fill::default() }));
                surface.draw_path(&path);
            }
            Prim::Line { pts, width, color, dash, closed } => {
                let Some(path) = path(&[pts], *closed) else { continue };
                let dash = dash
                    .as_ref()
                    .filter(|d| !d.is_empty() && d.iter().all(|x| x.is_finite() && *x >= 0.0) && d.iter().sum::<f32>() > 0.0)
                    .map(|d| StrokeDash { array: d.clone(), offset: 0.0 });
                let join = if d.mitred(i) { LineJoin::Miter } else { LineJoin::Round };
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: colour(*color).into(),
                    width: if width.is_finite() { width.max(0.0) } else { 0.0 },
                    miter_limit: 10.0,
                    line_cap: LineCap::Round,
                    line_join: join,
                    dash,
                    ..Stroke::default()
                }));
                surface.draw_path(&path);
            }
            Prim::Image { at, width, height, rgba, .. } => {
                let (Some(img), Some(size)) = (image(*width, *height, rgba, opaque), Size::from_wh(*width as f32, *height as f32)) else {
                    continue;
                };
                surface.push_transform(&Transform::from_translate(at.0, at.1));
                surface.draw_image(img, size);
                surface.pop();
            }
        }
    }
    for _ in 0..pushed {
        surface.pop();
    }
}

/// Hairlines on the paper, mm from its top left.
fn marks(surface: &mut Surface, marks: &[[P2; 2]]) {
    let mut pb = PathBuilder::new();
    for [a, b] in marks {
        pb.move_to((a.0 * PT) as f32, (a.1 * PT) as f32);
        pb.line_to((b.0 * PT) as f32, (b.1 * PT) as f32);
    }
    let Some(path) = pb.finish() else { return };
    surface.set_fill(None);
    surface.set_stroke(Some(Stroke { paint: colour(Rgb(0, 0, 0)).into(), width: 0.0, ..Stroke::default() }));
    surface.draw_path(&path);
}

/// The pages, each a drawing of `paper` mm, as a PDF file.
pub fn pdf(pages: &[(&Drawing, P2)]) -> Vec<u8> {
    let pages: Vec<Page> = pages.iter().map(|(drawing, paper)| Page::new(drawing, *paper)).collect();
    // An ordinary PDF of pages that can be drawn is always written.
    document(&pages, &Settings::default()).unwrap_or_default()
}

/// A date krilla writes, of seconds since 1970.
fn date(secs: u64) -> Option<DateTime> {
    let s = crate::ifc::utc(secs);
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<u8>().ok());
    let year = s.get(0..4).and_then(|x| x.parse::<u16>().ok())?;
    Some(DateTime::new(year).month(n(5, 7)?).day(n(8, 10)?).hour(n(11, 13)?).minute(n(14, 16)?).second(n(17, 19)?))
}

/// The pages as a PDF file written as `settings` asks, with a bookmark for each that has a title. Fails when the file
/// can't be what is asked (an archive it wouldn't pass as), saying why.
pub fn document(pages: &[Page], settings: &Settings) -> Result<Vec<u8>, String> {
    document_with(pages, settings, true)
}

fn document_with(pages: &[Page], settings: &Settings, compress: bool) -> Result<Vec<u8>, String> {
    let mut configuration = ConfigurationBuilder::new();
    match settings.archive.krilla() {
        Some(a) => configuration = configuration.with_archival_validator(a),
        None => configuration = configuration.with_version(settings.version.krilla()),
    }
    let configuration = configuration.finish().map_err(|e| format!("the PDF can't be written so: {e:?}"))?;
    let mut doc = Document::new_with(SerializeSettings {
        compress_content_streams: compress,
        enable_tagging: false,
        configuration,
        ..SerializeSettings::default()
    });
    let opaque = settings.archive == Archive::A1b;
    let mut outline = Outline::new();
    for (index, page) in pages.iter().enumerate() {
        let (w, h) = ((page.paper.0 * PT) as f32, (page.paper.1 * PT) as f32);
        let Some(size) = PageSettings::from_wh(w, h) else {
            return Err(format!("a page of {:.1} × {:.1} mm can't be printed", page.paper.0, page.paper.1));
        };
        let mut p = doc.start_page_with(size);
        let mut surface = p.surface();
        match &page.picture {
            Some((pw, ph, rgba)) => {
                if let (Some(img), Some(size)) = (image(*pw, *ph, rgba, true), Size::from_wh(w, h)) {
                    surface.draw_image(img, size);
                }
            }
            None => draw(&mut surface, page.drawing, page.place, opaque),
        }
        marks(&mut surface, &page.marks);
        surface.finish();
        p.finish();
        if let Some(title) = &page.title {
            let to = krilla::destination::XyzDestination::new(index, Point::from_xy(0.0, 0.0));
            outline.push_child(OutlineNode::new(title.chars().take(256).collect(), to));
        }
    }
    doc.set_outline(outline);
    let title = settings.title.clone().or_else(|| pages.iter().find_map(|p| p.title.clone()));
    let mut metadata = Metadata::new().creator("BIMCraft".to_string()).producer(format!("BIMCraft {}", env!("CARGO_PKG_VERSION")));
    let given = |s: &Option<String>| s.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if let Some(t) = given(&title) {
        metadata = metadata.title(t);
    }
    if let Some(a) = given(&settings.author) {
        metadata = metadata.authors(vec![a]);
    }
    if let Some(s) = given(&settings.subject) {
        metadata = metadata.description(s);
    }
    let keywords: Vec<String> = settings.keywords.iter().map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
    if !keywords.is_empty() {
        metadata = metadata.keywords(keywords);
    }
    // An archive is dated (PDF/A-1 needs it): now, unless the settings say when.
    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
    let created = settings.created.or_else(|| (settings.archive != Archive::None).then(now).flatten());
    if let Some(d) = created.and_then(date) {
        metadata = metadata.creation_date(d);
    }
    doc.set_metadata(metadata);
    doc.finish().map_err(|e| match e {
        krilla::error::KrillaError::Validation(errors) => {
            let why: Vec<String> = errors.iter().take(5).map(|(e, _)| format!("{e:?}")).collect();
            format!("the file wouldn't be {}: {}", settings.archive.label(), why.join("; "))
        }
        e => format!("the PDF couldn't be written: {e:?}"),
    })
}

/// A one-page file of `d` on `paper`, its content streams left as text: what the tests read.
#[cfg(test)]
pub(crate) fn contents(d: &Drawing, paper: P2) -> (String, Vec<u8>) {
    let bytes = document_with(&[Page::new(d, paper)], &Settings::default(), false).unwrap();
    (String::from_utf8_lossy(&bytes).into_owned(), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn house() -> Drawing {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        crate::render::plan(&s.model, 100000, 400, 300)
    }

    #[test]
    fn a_file_of_two_pages_has_both_and_a_bookmark_each() {
        let d = house();
        let mut pages = vec![Page::new(&d, P2(80.0, 60.0)), Page::new(&d, P2(80.0, 60.0))];
        pages[0].title = Some("One".into());
        pages[1].title = Some("Two".into());
        let bytes = document(&pages, &Settings::default()).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert_eq!(text.matches("/Type/Page/").count(), 2);
        assert!(text.contains("/Outlines"));
    }

    #[test]
    fn an_archive_is_written_as_one_with_its_metadata_and_colour_profile() {
        let d = house();
        for (archive, version, part) in [
            (Archive::A1b, "%PDF-1.4", "<pdfaid:part>1</pdfaid:part>"),
            (Archive::A2b, "%PDF-1.7", "<pdfaid:part>2</pdfaid:part>"),
            (Archive::A3b, "%PDF-1.7", "<pdfaid:part>3</pdfaid:part>"),
            (Archive::A4, "%PDF-2.0", "<pdfaid:part>4</pdfaid:part>"),
        ] {
            let settings = Settings { archive, title: Some("Plan".into()), created: Some(1_700_000_000), ..Settings::default() };
            let bytes = document(&[Page::new(&d, P2(80.0, 60.0))], &settings).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(bytes.starts_with(version.as_bytes()), "{archive:?}");
            assert!(text.contains(part), "{archive:?} says it is one in its XMP");
            assert!(text.contains("/OutputIntents"), "{archive:?} has an output intent");
        }
    }

    #[test]
    fn a_picture_with_clear_parts_keeps_its_mask_but_not_in_pdf_a_1() {
        let mut d = Drawing::new(crate::render::View { angle: 0.0, scale: 1.0, cx: 0.0, cy: 0.0, width: 10, height: 10 });
        d.push(Prim::Image { at: (0.0, 0.0), width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 0, 0, 0], ink: false });
        let mask = |archive| {
            let bytes = document(&[Page::new(&d, P2(10.0, 10.0))], &Settings { archive, ..Settings::default() }).unwrap();
            bytes.windows(6).any(|w| w == b"/SMask")
        };
        assert!(mask(Archive::None));
        assert!(!mask(Archive::A1b));
    }

    #[test]
    fn cut_marks_and_a_page_made_a_picture() {
        let d = house();
        let mut page = Page::new(&d, P2(80.0, 60.0));
        page.marks = vec![[P2(10.0, 0.0), P2(10.0, 8.0)]];
        page.picture = Some((2, 2, vec![255; 16]));
        let bytes = document_with(&[page], &Settings::default(), false).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Subtype/Image"));
        assert!(text.contains("0 w"), "the marks are hairlines");
        assert_eq!(text.lines().filter(|l| *l == "S").count(), 1, "the drawing isn't drawn as lines too");
    }
}
