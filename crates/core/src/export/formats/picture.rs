//! Pictures and drawings of a view or a sheet: PNG, a picture a page at the page setup's resolution; SVG, the drawing as
//! lines.

use super::super::{Content, Ctx, Family, Format, Options, Out, Paged, Prepared, Takes, page};
use super::styled;
use crate::render;

/// The one drawing a picture format writes.
fn drawing<'a>(items: &'a [Prepared]) -> Result<(&'a Prepared, &'a Paged), String> {
    match items {
        [item] => match &item.content {
            Content::Drawing(paged) => Ok((item, paged)),
            _ => Err(format!("{} has no drawing to make a picture of", item.title)),
        },
        _ => Err("a picture is of one view or sheet".to_string()),
    }
}

/// Each page of `paged` laid by `setup`, as RGBA pixels at its resolution: width, height and pixels.
pub(crate) fn pages(paged: &Paged, setup: &page::PageSetup, ctx: &Ctx) -> Result<Vec<(u32, u32, Vec<u8>)>, String> {
    let layout = paged.layout(setup)?;
    let restyled = styled(&paged.drawing, setup);
    let drawing = restyled.as_ref().unwrap_or(&paged.drawing);
    let px = f64::from(setup.dpi()) / 25.4;
    let (width, height) = ((layout.paper.0 * px).round().max(1.0) as u32, (layout.paper.1 * px).round().max(1.0) as u32);
    let mut out = Vec::new();
    for tile in &layout.pages {
        if (ctx.cancelled)() {
            return Err("stopped".to_string());
        }
        let place = paged.place(&layout, tile);
        let clip = place.clip.map(|[l, t, w, h]| [(l * px) as f32, (t * px) as f32, (w * px) as f32, (h * px) as f32]);
        let rgba =
            render::raster(drawing, width, height, (place.k * px) as f32, ((place.at.0 * px) as f32, (place.at.1 * px) as f32), clip)?;
        out.push((width, height, rgba));
    }
    Ok(out)
}

/// Pages' pictures as files, numbered when there are several.
fn numbered(pictures: Vec<Vec<u8>>) -> Vec<Out> {
    let many = pictures.len() > 1;
    pictures.into_iter().enumerate().map(|(i, bytes)| Out { suffix: many.then(|| (i + 1).to_string()), bytes }).collect()
}

pub struct Png;

impl Format for Png {
    fn id(&self) -> &'static str {
        "png"
    }
    fn label(&self) -> &'static str {
        "PNG"
    }
    fn extension(&self) -> &'static str {
        "png"
    }
    fn family(&self) -> Family {
        Family::Raster
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Drawing]
    }
    fn paged(&self) -> bool {
        true
    }

    fn write(&self, items: &[Prepared], options: &Options, ctx: &Ctx) -> Result<Vec<Out>, String> {
        let (_, paged) = drawing(items)?;
        let mut files = Vec::new();
        for (width, height, rgba) in pages(paged, &options.page, ctx)? {
            let size = tiny_skia::IntSize::from_wh(width, height).ok_or("the picture's size is out of range")?;
            let pixmap = tiny_skia::Pixmap::from_vec(rgba, size).ok_or("the picture's pixels don't fit its size")?;
            files.push(pixmap.encode_png().map_err(|e| e.to_string())?);
        }
        Ok(numbered(files))
    }
}

pub struct Svg;

impl Format for Svg {
    fn id(&self) -> &'static str {
        "svg"
    }
    fn label(&self) -> &'static str {
        "SVG"
    }
    fn extension(&self) -> &'static str {
        "svg"
    }
    fn family(&self) -> Family {
        Family::Vector
    }
    fn takes(&self) -> &'static [Takes] {
        &[Takes::Drawing]
    }

    fn write(&self, items: &[Prepared], options: &Options, _: &Ctx) -> Result<Vec<Out>, String> {
        let (_, paged) = drawing(items)?;
        let restyled = styled(&paged.drawing, &options.page);
        Ok(vec![Out { suffix: None, bytes: render::svg(restyled.as_ref().unwrap_or(&paged.drawing)).into_bytes() }])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::engine::Session;
    use crate::export::page::{Colour, PageSetup, Scale};
    use crate::export::prepare;

    fn plan() -> (Session, Prepared) {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let m: Arc<_> = s.model.clone();
        let p = prepare(&m, &m, Some(100000), Takes::Drawing).unwrap();
        (s, p)
    }

    fn decode(bytes: &[u8]) -> tiny_skia::Pixmap {
        tiny_skia::Pixmap::decode_png(bytes).unwrap()
    }

    #[test]
    fn a_plan_on_a4_at_100_dpi_is_a_picture_of_that_size_with_its_walls_on_it() {
        let (_s, p) = plan();
        let options = Options {
            page: PageSetup { paper: Some("A4".into()), scale: Scale::Fit, dpi: Some(100), ..PageSetup::default() },
            ..Options::default()
        };
        let out = Png.write(&[p], &options, &Ctx::quiet()).unwrap();
        assert_eq!(out.len(), 1);
        let pm = decode(&out[0].bytes);
        // A4 landscape at 100 dpi: 1169 × 827.
        assert_eq!((pm.width(), pm.height()), (1169, 827));
        let dark = pm.pixels().iter().filter(|c| c.red() < 100).count();
        assert!(dark > 1000, "the walls are drawn: {dark} dark pixels");
    }

    #[test]
    fn a_plan_tiled_on_two_pages_is_two_numbered_pictures() {
        let (_s, p) = plan();
        // 8 m at 1:20 is 400 mm and more: two A3 pages across at least.
        let options = Options {
            page: PageSetup { paper: Some("A3".into()), scale: Scale::Custom(20.0), dpi: Some(36), ..PageSetup::default() },
            ..Options::default()
        };
        let out = Png.write(&[p], &options, &Ctx::quiet()).unwrap();
        assert!(out.len() >= 2);
        assert_eq!(out[0].suffix.as_deref(), Some("1"));
    }

    #[test]
    fn grey_prints_no_colour() {
        let (_s, p) = plan();
        let options = Options {
            page: PageSetup { scale: Scale::Fit, paper: Some("A5".into()), colour: Colour::Grey, dpi: Some(50), ..PageSetup::default() },
            ..Options::default()
        };
        // A5 isn't a size this knows: say so, not print.
        assert!(Png.write(&[p], &options, &Ctx::quiet()).unwrap_err().contains("A5"));
        let (_s, p) = plan();
        let options = Options { page: PageSetup { paper: Some("A4".into()), ..options.page }, ..Options::default() };
        let pm = decode(&Png.write(&[p], &options, &Ctx::quiet()).unwrap()[0].bytes);
        assert!(pm.pixels().iter().all(|c| c.red() == c.green() && c.green() == c.blue()));
    }

    #[test]
    fn an_svg_is_the_drawing_as_lines() {
        let (_s, p) = plan();
        let out = Svg.write(&[p], &Options::default(), &Ctx::quiet()).unwrap();
        let text = String::from_utf8(out[0].bytes.clone()).unwrap();
        assert!(text.starts_with("<svg") || text.starts_with("<?xml"));
    }

    #[test]
    fn a_cancelled_export_stops() {
        let (_s, p) = plan();
        let ctx = Ctx { cancelled: &|| true, progress: &|_, _, _| {} };
        assert!(Png.write(&[p], &Options::default(), &ctx).is_err());
    }
}
