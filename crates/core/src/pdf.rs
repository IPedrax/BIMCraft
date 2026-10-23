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

/// A page's content stream: the drawing's pixels mapped onto `paper` mm in points, y up; and its
/// pictures, RGB, in the order the stream names them (`/Im0`, `/Im1`…).
pub(crate) fn contents(d: &Drawing, paper: P2) -> (String, Vec<(u32, u32, Vec<u8>)>) {
    let k = 72.0 / 25.4 / d.view.scale;
    let mut s = format!("{k:.6} 0 0 {:.6} 0 {:.3} cm 1 J 1 j\n", -k, paper.1 * 72.0 / 25.4);
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
    for p in &d.prims {
        match p {
            Prim::Fill { rings, color: c } => {
                let _ = write!(s, "{} rg ", color(*c));
                for r in rings {
                    path(&mut s, r, true);
                }
                s.push_str("f\n");
            }
            Prim::Line { pts, width, color: c, dash, closed } => {
                let dash = dash.map_or_else(|| "[] 0 d".to_string(), |[a, b]| format!("[{a} {b}] 0 d"));
                let _ = write!(s, "{} RG {width} w {dash} ", color(*c));
                path(&mut s, pts, *closed);
                s.push_str("S\n");
            }
            Prim::Image { at, width, height, rgba } => {
                let (w, h) = (*width as f32, *height as f32);
                let _ = writeln!(s, "q {w} 0 0 {} {} {} cm /Im{} Do Q", -h, at.0, at.1 + h, images.len());
                images.push((*width, *height, rgba.as_chunks::<4>().0.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect()));
            }
        }
    }
    (s, images)
}

/// The pages, each a drawing of `paper` mm, as a PDF file.
pub fn pdf(pages: &[(&Drawing, P2)]) -> Vec<u8> {
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
    for (d, paper) in pages {
        let (page, stream) = (next, next + 1);
        let (text, images) = contents(d, *paper);
        let firsts = stream + 1;
        next = firsts + images.len();
        kids.push(format!("{page} 0 R"));
        let xobjects: String = (0..images.len()).map(|i| format!("/Im{i} {} 0 R ", firsts + i)).collect();
        let (w, h) = (paper.0 * 72.0 / 25.4, paper.1 * 72.0 / 25.4);
        let dict = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.3} {h:.3}] /Resources << /XObject << {xobjects}>> >> /Contents {stream} 0 R >>"
        );
        object(&mut out, &mut offsets, page, &dict, None);
        let body = deflate(text.as_bytes());
        object(&mut out, &mut offsets, stream, &format!("<< /Length {} /Filter /FlateDecode >>", body.len()), Some(&body));
        for (i, (iw, ih, rgb)) in images.iter().enumerate() {
            let body = deflate(rgb);
            let dict = format!(
                "<< /Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                body.len()
            );
            object(&mut out, &mut offsets, firsts + i, &dict, Some(&body));
        }
    }
    object(&mut out, &mut offsets, 1, "<< /Type /Catalog /Pages 2 0 R >>", None);
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
