//! PDF: each source's drawing laid on pages by the page setup, all in one file when the job combines them, with a bookmark
//! for each source.

use super::super::{Content, Ctx, Family, Format, Options, Out, Prepared, Takes};
use super::styled;
use crate::pdf::{Page, document};

pub struct Pdf;

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
        let mut pages = Vec::new();
        for (item, paged, layout, restyled) in &laid {
            let drawing = restyled.as_ref().unwrap_or(&paged.drawing);
            for (i, tile) in layout.pages.iter().enumerate() {
                // A source's bookmark is on its first page; a source of many pages has one on each, numbered.
                let title = match layout.pages.len() {
                    1 => Some(item.title.clone()),
                    _ => Some(format!("{} ({})", item.title, i + 1)),
                };
                pages.push(Page { drawing, paper: layout.paper, place: Some(paged.place(layout, tile)), title });
            }
        }
        // No bookmarks for a single page: there is nothing to go to.
        if pages.len() == 1 {
            pages.iter_mut().for_each(|p| p.title = None);
        }
        Ok(vec![Out { suffix: None, bytes: document(&pages) }])
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
        assert_eq!(count(bytes, "/Type /Page "), 2);
        assert_eq!(count(bytes, "/Type /Outlines"), 1);
        assert!(count(bytes, "/Dest [") == 2, "a bookmark for each sheet");
        // A3, as the sheets are: 420 × 297 mm.
        assert_eq!(count(bytes, "/MediaBox [0 0 1190.551 841.890]"), 2);
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
        assert_eq!(count(bytes, "/MediaBox [0 0 841.890 595.276]"), 1);
        assert_eq!(count(bytes, "/Type /Outlines"), 0, "one page needs no bookmarks");
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
        assert!(count(&out[0].bytes, "/Type /Page ") >= 4);
    }
}
