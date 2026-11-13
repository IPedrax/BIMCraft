//! PDF: each source's drawing laid on pages by the page setup, all in one file when the job combines them, with a bookmark
//! for each source.

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use super::styled;
use crate::export::page::{Layout, Tile};
use crate::geom::P2;
use crate::pdf::{Page, document};

pub struct Pdf;

/// Where to cut a page of a drawing tiled across several: on each edge of its printable area that meets another page, a
/// hairline in the margin at both ends of that edge, clear of the paper's edge and of the drawing. None without a margin to
/// put them in.
pub(crate) fn cut_marks(layout: &Layout, tile: &Tile) -> Vec<[P2; 2]> {
    let (m, P2(w, h)) = (layout.margin, layout.paper);
    if m < 2.0 {
        return Vec::new();
    }
    // From a fifth of the margin off the paper's edge to a fifth of it short of the drawing.
    let (near, far) = (m * 0.2, m * 0.8);
    let mut marks = Vec::new();
    let across = |x: f64, marks: &mut Vec<[P2; 2]>| {
        marks.push([P2(x, near), P2(x, far)]);
        marks.push([P2(x, h - far), P2(x, h - near)]);
    };
    let down = |y: f64, marks: &mut Vec<[P2; 2]>| {
        marks.push([P2(near, y), P2(far, y)]);
        marks.push([P2(w - far, y), P2(w - near, y)]);
    };
    if tile.col > 0 {
        across(m, &mut marks);
    }
    if tile.col + 1 < layout.cols {
        across(w - m, &mut marks);
    }
    if tile.row > 0 {
        down(m, &mut marks);
    }
    if tile.row + 1 < layout.rows {
        down(h - m, &mut marks);
    }
    marks
}

impl Format for Pdf {
    fn id(&self) -> &'static str {
        "pdf"
    }
    fn label(&self) -> &'static str {
        "PDF"
    }
    fn extension(&self) -> &'static str {
        "pdf"
    }
    fn family(&self) -> Family {
        Family::Document
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Drawing]
    }
    fn combines(&self) -> bool {
        true
    }
    fn paged(&self) -> bool {
        true
    }

    fn write(&self, items: &[Prepared], options: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        // Each source's drawing in the page setup's colours, and how it lies on its pages.
        let mut laid = Vec::new();
        for item in items {
            if (ctx.cancelled)() {
                return Err("stopped".to_string());
            }
            let Content::Drawing(paged) = &item.content else {
                return Err(format!("{} has no drawing to print", item.title));
            };
            let layout = paged.layout(&options.page)?;
            let restyled = styled(&paged.drawing, &options.page);
            laid.push((item, paged, layout, restyled));
        }
        // Pages made pictures, when asked: each source's at the setup's resolution, in its layout's order.
        let mut pictures = Vec::new();
        for (_, paged, ..) in &laid {
            pictures.push(if options.pdf.raster { Some(super::picture::pages(paged, &options.page, ctx)?) } else { None });
        }
        let mut pages = Vec::new();
        for ((item, paged, layout, restyled), pictures) in laid.iter().zip(pictures) {
            let drawing = restyled.as_ref().unwrap_or(&paged.drawing);
            let mut pictures = pictures.map(Vec::into_iter);
            for (i, tile) in layout.pages.iter().enumerate() {
                // A source's bookmark is on its first page; a source of many pages has one on each, numbered.
                let title = match layout.pages.len() {
                    1 => Some(item.title.clone()),
                    _ => Some(format!("{} ({})", item.title, i + 1)),
                };
                let marks = if options.pdf.cut_marks { cut_marks(layout, tile) } else { Vec::new() };
                let picture = pictures.as_mut().and_then(Iterator::next);
                pages.push(Page { drawing, paper: layout.paper, place: Some(paged.place(layout, tile)), title, marks, picture });
            }
        }
        // No bookmarks for a single page: there is nothing to go to.
        if pages.len() == 1 {
            pages.iter_mut().for_each(|p| p.title = None);
        }
        let mut settings = options.pdf.clone();
        if settings.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
            settings.title = items.first().map(|i| i.title.clone());
        }
        // Undated unless an archive standard asks for a date (`pdf::document` gives it one), so the same project prints the same bytes.
        Ok(vec![Out { suffix: None, bytes: document(&pages, &settings)? }])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::engine::Session;
    use crate::export::page::{PageSetup, Scale};
    use crate::export::{Takes, prepare};

    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               sheet {"name": "Plans"}
               place {"sheet": "A101", "view": 100000}
               sheet {"name": "More"}"#,
        )
        .unwrap();
        s
    }

    fn count(bytes: &[u8], what: &str) -> usize {
        String::from_utf8_lossy(bytes).matches(what).count()
    }

    #[test]
    fn sheets_combine_into_one_pdf_with_a_bookmark_each() {
        let s = house();
        let m: Arc<_> = s.model.clone();
        let sheets: Vec<_> = [100006, 100008].iter().map(|id| prepare(&m, &m, Some(*id), Takes::Drawing).unwrap()).collect();
        let out = Pdf.write(&sheets, &Options::default(), &Ctx::quiet()).unwrap();
        let bytes = &out[0].bytes;
        assert_eq!(count(bytes, "/Type/Page/"), 2);
        assert_eq!(count(bytes, "/Type/Outlines"), 1);
        assert!(count(bytes, "/Dest ") == 2, "a bookmark for each sheet");
        // A3, as the sheets are: 420 × 297 mm.
        assert_eq!(count(bytes, "/MediaBox[0 0 1190.5511 841.8898]"), 2);
    }

    #[test]
    fn a_sheet_fitted_to_a4_is_printed_smaller_and_clipped_to_the_margin() {
        let s = house();
        let m: Arc<_> = s.model.clone();
        let sheet = prepare(&m, &m, Some(100006), Takes::Drawing).unwrap();
        let options = Options {
            page: PageSetup { paper: Some("A4".into()), scale: Scale::Fit, margin: 5.0, ..PageSetup::default() },
            ..Options::default()
        };
        let out = Pdf.write(&[sheet], &options, &Ctx::quiet()).unwrap();
        let bytes = &out[0].bytes;
        assert_eq!(count(bytes, "/MediaBox[0 0 841.8898 595.2756]"), 1);
        assert_eq!(count(bytes, "/Type/Outlines"), 0, "one page needs no bookmarks");
    }

    #[test]
    fn a_plan_tiled_across_pages_has_a_page_for_each_tile() {
        let s = house();
        let m: Arc<_> = s.model.clone();
        let plan = prepare(&m, &m, Some(100000), Takes::Drawing).unwrap();
        // 8 m × 6 m at 1:20 is 400 × 300 mm and more with its room round it: four A4 pages at least.
        let options = Options {
            page: PageSetup { paper: Some("A4".into()), scale: Scale::Custom(20.0), margin: 10.0, ..PageSetup::default() },
            ..Options::default()
        };
        let out = Pdf.write(&[plan], &options, &Ctx::quiet()).unwrap();
        assert!(count(&out[0].bytes, "/Type/Page/") >= 4);
    }
}
