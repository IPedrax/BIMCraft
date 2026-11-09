//! PDF for sheets (PDF 1.4): a page a drawing, its lines and fills as paths, its pictures as
//! images. The lettering is our strokes, so a page needs no fonts. Streams are deflated.

use std::fmt::Write as _;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::geom::P2;
use crate::render::{Drawing, Prim};

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing into a Vec can't fail.
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

/// A picture as a page holds it: its width and height, its pixels' RGB, and their alpha if any isn't opaque.
type Picture = (u32, u32, Vec<u8>, Option<Vec<u8>>);

/// Where a drawing lies on its page: `k` paper mm to the drawing's pixel, its pixel (0, 0) `at` mm from the paper's top left
/// corner, and drawn only inside `clip` (left, top, width, height, mm from the top left) when there is one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub k: f64,
    pub at: P2,
    pub clip: Option<[f64; 4]>,
}

/// A page: a drawing on `paper` mm, where `place` puts it (none, at full size from the corner: a sheet on its own paper), and
/// its title in the file's bookmarks, if it has one.
pub struct Page<'a> {
    pub drawing: &'a Drawing,
    pub paper: P2,
    pub place: Option<Place>,
    pub title: Option<String>,
}

/// Points to the mm.
const PT: f64 = 72.0 / 25.4;

/// [`contents_at`] at full size from the corner, as the tests read a page.
#[cfg(test)]
pub(crate) fn contents(d: &Drawing, paper: P2) -> (String, Vec<Picture>) {
    contents_at(d, paper, None)
}

/// A page's content stream: the drawing's pixels mapped onto `paper` mm in points, y up, where `place` puts them (none, at full
/// size from the corner); and its pictures, RGB and the alpha of one not all there, in the order the stream names them (`/Im0`,
/// `/Im1`…).
fn contents_at(d: &Drawing, paper: P2, place: Option<Place>) -> (String, Vec<Picture>) {
    let Place { k, at, clip } = place.unwrap_or(Place { k: 1.0 / d.view.scale, at: P2(0.0, 0.0), clip: None });
    let mut s = String::new();
    if let Some([l, t, w, h]) = clip {
        let _ = writeln!(s, "q {:.3} {:.3} {:.3} {:.3} re W n", l * PT, (paper.1 - t - h) * PT, w * PT, h * PT);
    }
    let _ = writeln!(s, "{:.6} 0 0 {:.6} {:.3} {:.3} cm 1 J 0 j", k * PT, -k * PT, at.0 * PT, (paper.1 - at.1) * PT);
    let color =
        |c: crate::render::Rgb| format!("{:.3} {:.3} {:.3}", f32::from(c.0) / 255.0, f32::from(c.1) / 255.0, f32::from(c.2) / 255.0);
    let path = |s: &mut String, pts: &[(f32, f32)], closed: bool| {
        for (i, (x, y)) in pts.iter().enumerate() {
            let _ = write!(s, "{x:.2} {y:.2} {} ", if i == 0 { "m" } else { "l" });
        }
        if closed {
            s.push_str("h ");
        }
    };
    let mut images = Vec::new();
    for (i, p) in d.prims.iter().enumerate() {
        match p {
            Prim::Fill { rings, color: c } => {
                let _ = write!(s, "{} rg ", color(*c));
                for r in rings {
                    path(&mut s, r, true);
                }
                s.push_str("f\n");
            }
            Prim::Line { pts, width, color: c, dash, closed } => {
                let dash = dash.as_ref().map_or_else(|| "[] 0 d".to_string(), |d| format!("[{}] 0 d", crate::render::spaced(d)));
                let _ = write!(s, "{} RG {width} w {} j {dash} ", color(*c), if d.mitred(i) { 0 } else { 1 });
                path(&mut s, pts, *closed);
                s.push_str("S\n");
            }
            Prim::Image { at, width, height, rgba, .. } => {
                let (w, h) = (*width as f32, *height as f32);
                let _ = writeln!(s, "q {w} 0 0 {} {} {} cm /Im{} Do Q", -h, at.0, at.1 + h, images.len());
                let pixels = rgba.as_chunks::<4>().0;
                // What isn't all there (a scan's picture between its points) is masked by its alpha.
                let alpha = pixels.iter().any(|[.., a]| *a < 255).then(|| pixels.iter().map(|[.., a]| *a).collect::<Vec<u8>>());
                images.push((*width, *height, pixels.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect(), alpha));
            }
        }
    }
    if clip.is_some() {
        s.push_str("Q\n");
    }
    (s, images)
}

/// Text as a PDF string any reader shows, whatever its script: UTF-16, big-endian, in hex.
fn text(s: &str) -> String {
    let hex: String = s.chars().take(256).collect::<String>().encode_utf16().map(|u| format!("{u:04X}")).collect();
    format!("<FEFF{hex}>")
}

/// The pages, each a drawing of `paper` mm, as a PDF file.
pub fn pdf(pages: &[(&Drawing, P2)]) -> Vec<u8> {
    let pages: Vec<Page> = pages.iter().map(|(drawing, paper)| Page { drawing, paper: *paper, place: None, title: None }).collect();
    document(&pages)
}

/// The pages as a PDF file, with a bookmark for each that has a title.
pub fn document(pages: &[Page]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = vec![0usize; 3];
    let object = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, n: usize, dict: &str, stream: Option<&[u8]>| {
        if offsets.len() <= n {
            offsets.resize(n + 1, 0);
        }
        if let Some(o) = offsets.get_mut(n) {
            *o = out.len();
        }
        out.extend(format!("{n} 0 obj\n{dict}\n").bytes());
        if let Some(bytes) = stream {
            out.extend(b"stream\n");
            out.extend(bytes);
            out.extend(b"\nendstream\n");
        }
        out.extend(b"endobj\n");
    };
    // 1 the catalog, 2 the page tree; then a page, its contents and its images, page by page.
    let mut kids = Vec::new();
    let mut next = 3;
    let mut marks = Vec::new();
    for Page { drawing: d, paper, place, title } in pages {
        let (page, stream) = (next, next + 1);
        let (text, images) = contents_at(d, *paper, *place);
        if let Some(title) = title {
            marks.push((page, title));
        }
        // Each image's object, and its mask's after it if it has one.
        let mut numbers = Vec::with_capacity(images.len());
        next = stream + 1;
        for (.., alpha) in &images {
            numbers.push(next);
            next += if alpha.is_some() { 2 } else { 1 };
        }
        kids.push(format!("{page} 0 R"));
        let xobjects: String = numbers.iter().enumerate().map(|(i, n)| format!("/Im{i} {n} 0 R ")).collect();
        let (w, h) = (paper.0 * 72.0 / 25.4, paper.1 * 72.0 / 25.4);
        let dict = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.3} {h:.3}] /Resources << /XObject << {xobjects}>> >> /Contents {stream} 0 R >>"
        );
        object(&mut out, &mut offsets, page, &dict, None);
        let body = deflate(text.as_bytes());
        object(&mut out, &mut offsets, stream, &format!("<< /Length {} /Filter /FlateDecode >>", body.len()), Some(&body));
        for ((iw, ih, rgb, alpha), n) in images.iter().zip(&numbers) {
            let body = deflate(rgb);
            let mask = alpha.as_ref().map_or_else(String::new, |_| format!("/SMask {} 0 R ", n + 1));
            let dict = format!(
                "<< /Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /DeviceRGB /BitsPerComponent 8 {mask}/Filter /FlateDecode /Length {} >>",
                body.len()
            );
            object(&mut out, &mut offsets, *n, &dict, Some(&body));
            if let Some(alpha) = alpha {
                let body = deflate(alpha);
                let dict = format!(
                    "<< /Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                    body.len()
                );
                object(&mut out, &mut offsets, n + 1, &dict, Some(&body));
            }
        }
    }
    // The bookmarks: their root, then one a titled page, each pointing at its page fitted in the window.
    let outlines = if marks.is_empty() {
        None
    } else {
        let root = next;
        let first = root + 1;
        let last = root + marks.len();
        for (i, (page, title)) in marks.iter().enumerate() {
            let n = first + i;
            let prev = if n > first { format!("/Prev {} 0 R ", n - 1) } else { String::new() };
            let after = if n < last { format!("/Next {} 0 R ", n + 1) } else { String::new() };
            let dict = format!("<< /Title {} /Parent {root} 0 R {prev}{after}/Dest [{page} 0 R /Fit] >>", text(title));
            object(&mut out, &mut offsets, n, &dict, None);
        }
        let dict = format!("<< /Type /Outlines /First {first} 0 R /Last {last} 0 R /Count {} >>", marks.len());
        object(&mut out, &mut offsets, root, &dict, None);
        next = last + 1;
        Some(root)
    };
    let catalog = match outlines {
        Some(root) => format!("<< /Type /Catalog /Pages 2 0 R /Outlines {root} 0 R /PageMode /UseOutlines >>"),
        None => "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
    };
    object(&mut out, &mut offsets, 1, &catalog, None);
    object(&mut out, &mut offsets, 2, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len()), None);
    let start = out.len();
    let mut xref = format!("xref\n0 {next}\n0000000000 65535 f \n");
    for n in 1..next {
        // Twenty bytes an entry: the space before the line feed is the PDF's.
        let _ = writeln!(xref, "{:010} 00000 n ", offsets.get(n).copied().unwrap_or(0));
    }
    let _ = write!(xref, "trailer\n<< /Size {next} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n");
    out.extend(xref.bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    #[test]
    fn the_cross_reference_table_points_at_every_object() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let d = crate::render::plan(&s.model, 100000, 400, 300);
        let bytes = pdf(&[(&d, P2(80.0, 60.0)), (&d, P2(80.0, 60.0))]);
        let tail = String::from_utf8_lossy(&bytes[bytes.len() - 40..]).to_string();
        let start: usize = tail.split("startxref\n").nth(1).unwrap().lines().next().unwrap().parse().unwrap();
        let table = String::from_utf8_lossy(&bytes[start..]).to_string();
        let mut lines = table.lines().skip(1);
        let count: usize = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        assert_eq!(count, 1 + 2 + 2 * 2, "the free entry, catalog and pages, and a page and its contents twice");
        for (n, entry) in lines.skip(1).take(count - 1).enumerate() {
            let offset: usize = entry[..10].parse().unwrap();
            let head = format!("{} 0 obj", n + 1);
            assert!(bytes[offset..].starts_with(head.as_bytes()), "object {} isn't at {offset}", n + 1);
        }
        assert!(bytes.ends_with(b"%%EOF\n"));
    }
}
