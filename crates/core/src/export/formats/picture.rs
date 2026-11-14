//! Pictures and drawings of a view or a sheet: PNG, JPEG and JPEG XL, a picture a page at the page setup's resolution; SVG, the
//! drawing as lines.
//!
//! JPEG is lossy, at the options' quality (90 if none); JPEG XL is lossless (zune-jpegxl: the only lossy JPEG XL encoder in Rust
//! is AGPL). A page in grey or black and white is written as one channel of grey.

use super::super::{Content, Ctx, Family, Format, Options, Out, Paged, Prepared, Takes, page};
use super::styled;
use crate::render;

/// The one drawing a picture format writes.
fn drawing(items: &[Prepared]) -> Result<(&Prepared, &Paged), String> {
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

/// A page's pixels as the channels a picture in `colour` needs: grey alone for grey and black and white, else red, green and
/// blue. The page is opaque (painted on white paper), so its alpha is dropped as it is.
fn channels(rgba: &[u8], colour: page::Colour) -> (Vec<u8>, bool) {
    match colour {
        page::Colour::Colour => (rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(), false),
        page::Colour::Grey | page::Colour::Black => (rgba.as_chunks::<4>().0.iter().map(|p| p[0]).collect(), true),
    }
}

pub struct Jpeg;

/// The widest and tallest picture JPEG holds, pixels.
const JPEG_MAX: u32 = 65_535;

impl Format for Jpeg {
    fn id(&self) -> &'static str {
        "jpeg"
    }
    fn label(&self) -> &'static str {
        "JPEG"
    }
    fn extension(&self) -> &'static str {
        "jpg"
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
        let dpi = u16::try_from(options.page.dpi()).unwrap_or(u16::MAX);
        let mut files = Vec::new();
        for (width, height, rgba) in pages(paged, &options.page, ctx)? {
            let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
                return Err(format!("a JPEG is at most {JPEG_MAX} px a side, not {width} × {height}; lower its resolution"));
            };
            let (pixels, grey) = channels(&rgba, options.page.colour);
            let mut out = Vec::new();
            let mut encoder = jpeg_encoder::Encoder::new(&mut out, options.quality.unwrap_or(90).clamp(1, 100));
            encoder.set_density(jpeg_encoder::PixelDensity::dpi(dpi));
            let kind = if grey { jpeg_encoder::ColorType::Luma } else { jpeg_encoder::ColorType::Rgb };
            encoder.encode(&pixels, w, h, kind).map_err(|e| format!("JPEG: {e}"))?;
            files.push(out);
        }
        Ok(numbered(files))
    }
}

pub struct Jxl;

impl Format for Jxl {
    fn id(&self) -> &'static str {
        "jxl"
    }
    fn label(&self) -> &'static str {
        "JPEG XL"
    }
    fn extension(&self) -> &'static str {
        "jxl"
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
        use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
        let (_, paged) = drawing(items)?;
        let mut files = Vec::new();
        for (width, height, rgba) in pages(paged, &options.page, ctx)? {
            let (pixels, grey) = channels(&rgba, options.page.colour);
            let space = if grey { ColorSpace::Luma } else { ColorSpace::RGB };
            let settings = EncoderOptions::new(width as usize, height as usize, space, BitDepth::Eight);
            let mut out = Vec::new();
            zune_jpegxl::JxlSimpleEncoder::new(&pixels, settings).encode(&mut out).map_err(|e| format!("JPEG XL: {e:?}"))?;
            files.push(out);
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

    /// The plan on A4 at 72 dpi, in `colour`.
    fn a4(colour: Colour) -> Options {
        Options {
            page: PageSetup { paper: Some("A4".into()), scale: Scale::Fit, dpi: Some(72), colour, ..PageSetup::default() },
            ..Options::default()
        }
    }

    #[test]
    fn a_jpeg_is_the_png_picture_at_its_dpi_and_grey_in_one_channel() {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::bytestream::ZCursor;
        let png = decode(&Png.write(&[plan().1], &a4(Colour::Colour), &Ctx::quiet()).unwrap()[0].bytes);
        let out = Jpeg.write(&[plan().1], &a4(Colour::Colour), &Ctx::quiet()).unwrap();
        let b = &out[0].bytes;
        // JFIF, its density 72 dots an inch.
        assert_eq!(&b[..3], &[0xFF, 0xD8, 0xFF]);
        assert_eq!(&b[6..11], b"JFIF\0");
        assert_eq!((b[13], u16::from_be_bytes([b[14], b[15]])), (1, 72));
        let mut d = JpegDecoder::new(ZCursor::new(b.as_slice()));
        let rgb = d.decode().unwrap();
        assert_eq!(d.dimensions(), Some((png.width() as usize, png.height() as usize)));
        // The same picture, near enough for a lossy one: on average a few levels off.
        let off: u64 = rgb
            .iter()
            .zip(png.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]))
            .map(|(a, b)| u64::from(a.abs_diff(b)))
            .sum();
        assert!(off / (rgb.len() as u64) < 6, "{} levels off on average", off / rgb.len() as u64);
        // Lower quality, smaller; grey, a channel.
        let rough = Options { quality: Some(20), ..a4(Colour::Colour) };
        assert!(Jpeg.write(&[plan().1], &rough, &Ctx::quiet()).unwrap()[0].bytes.len() < b.len());
        let grey = Jpeg.write(&[plan().1], &a4(Colour::Grey), &Ctx::quiet()).unwrap();
        let mut d = JpegDecoder::new(ZCursor::new(grey[0].bytes.as_slice()));
        d.decode_headers().unwrap();
        assert_eq!(d.input_colorspace().map(|c| c.num_components()), Some(1));
    }

    #[test]
    fn a_jpeg_xl_is_the_png_picture_to_the_pixel() {
        let png = decode(&Png.write(&[plan().1], &a4(Colour::Colour), &Ctx::quiet()).unwrap()[0].bytes);
        for colour in [Colour::Colour, Colour::Grey] {
            let png = if colour == Colour::Grey {
                decode(&Png.write(&[plan().1], &a4(colour), &Ctx::quiet()).unwrap()[0].bytes)
            } else {
                png.clone()
            };
            let out = Jxl.write(&[plan().1], &a4(colour), &Ctx::quiet()).unwrap();
            let image = jxl_oxide::JxlImage::builder().read(out[0].bytes.as_slice()).unwrap();
            assert_eq!((image.width(), image.height()), (png.width(), png.height()));
            let fb = image.render_frame(0).unwrap().image_all_channels();
            let want: Vec<u8> = match colour {
                Colour::Grey => png.data().as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
                _ => png.data().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(),
            };
            assert_eq!(fb.channels(), if colour == Colour::Grey { 1 } else { 3 });
            let got: Vec<u8> = fb.buf().iter().map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8).collect();
            assert!(got == want, "lossless: {} pixels differ", got.iter().zip(&want).filter(|(a, b)| a != b).count());
        }
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
