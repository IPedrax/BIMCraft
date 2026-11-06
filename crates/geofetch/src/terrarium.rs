//! Terrarium tiles: Mapzen's elevation on the Web Mercator map, 256 x 256 pixel PNGs where a pixel's red, green and blue say its
//! height in metres (R * 256 + G + B / 256 - 32768). The tiles a box needs are fetched (and kept a month), checked, laid side by
//! side and sampled onto a grid of longitude and latitude, so a seam between tiles is not a seam in the ground.
//! Hosted on AWS's open data (`elevation-tiles-prod`): each tile has its own sources, and their credits are in [`ATTRIBUTION`].

use std::collections::HashMap;
use std::sync::Arc;

use bimcraft_core::geo::{Bbox, ElevationGrid, tile_xy, tiles_for, zoom_for};

use crate::cache::{Cache, key};
use crate::layout::Layout;
use crate::net::Net;

/// How long a tile is kept, seconds: a month, for the ground does not change by the day.
pub const TTL: u64 = 30 * 24 * 3600;
/// The most a tile's file may be, bytes.
pub const MAX_TILE: u64 = 2 << 20;
/// The side of a tile, pixels.
pub const SIDE: usize = 256;
/// Where the tiles are.
pub const BASE: &str = "https://s3.amazonaws.com/elevation-tiles-prod/terrarium";
/// What the data asks to be credited.
pub const ATTRIBUTION: &str = "Elevation: Terrain Tiles (Terrarium) on AWS Open Data, made from USGS 3DEP and SRTM, GMTED2010 and ETOPO1 \
    (NOAA), and other national sources: see https://github.com/tilezen/joerd/blob/master/docs/attribution.md";

pub struct Terrarium {
    net: Arc<dyn Net>,
    cache: Arc<Cache>,
}

/// The heights, metres, of a tile's PNG, row after row from its north west corner, or why the file is not a tile. The header is
/// read and checked (256 x 256, 8-bit colour) before anything of that size is made, and the decoder is held to a few megabytes.
pub fn decode(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_limits(png::Limits { bytes: 4 << 20 });
    let mut reader = decoder.read_info().map_err(|e| format!("it is not a picture BIMCraft can read: {e}"))?;
    let info = reader.info();
    if (info.width, info.height) != (SIDE as u32, SIDE as u32) {
        return Err(format!("a tile is {SIDE} x {SIDE} pixels, not {} x {}", info.width, info.height));
    }
    let step = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgb, png::BitDepth::Eight) => 3,
        (png::ColorType::Rgba, png::BitDepth::Eight) => 4,
        (c, d) => return Err(format!("a tile is an 8-bit colour picture, not {c:?} at {d:?}")),
    };
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|e| format!("the picture is damaged: {e}"))?;
    if frame.buffer_size() != SIDE * SIDE * step {
        return Err("the picture is not the size its header says".into());
    }
    Ok(buf[..frame.buffer_size()]
        .chunks_exact(step)
        .map(|p| f32::from(p[0]) * 256.0 + f32::from(p[1]) + f32::from(p[2]) / 256.0 - 32768.0)
        .collect())
}

/// Tiles by (x, y), each its heights.
type Tiles = HashMap<(u32, u32), Vec<f32>>;

/// The height at a place on the map at `zoom`, from the pixels round it (those of tiles that are held, weighted by nearness).
fn sample(tiles: &Tiles, zoom: u8, lon: f64, lat: f64) -> Option<f32> {
    let (x, y) = tile_xy(lon, lat, zoom);
    let (u, v) = (x * SIDE as f64 - 0.5, y * SIDE as f64 - 0.5);
    let (i, j, fx, fy) = (u.floor(), v.floor(), u - u.floor(), v - v.floor());
    let (mut sum, mut weight) = (0.0, 0.0);
    for (di, dj, w) in [(0.0, 0.0, (1.0 - fx) * (1.0 - fy)), (1.0, 0.0, fx * (1.0 - fy)), (0.0, 1.0, (1.0 - fx) * fy), (1.0, 1.0, fx * fy)]
    {
        let (gx, gy) = (i + di, j + dj);
        if gx < 0.0 || gy < 0.0 || w <= 0.0 {
            continue;
        }
        let (gx, gy) = (gx as usize, gy as usize);
        if let Some(h) = tiles.get(&((gx / SIDE) as u32, (gy / SIDE) as u32)).map(|t| t[(gy % SIDE) * SIDE + gx % SIDE]) {
            sum += w * f64::from(h);
            weight += w;
        }
    }
    (weight > 0.0).then(|| (sum / weight) as f32)
}

impl Terrarium {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>) -> Terrarium {
        Terrarium { net, cache }
    }

    pub fn url(zoom: u8, x: u32, y: u32) -> String {
        format!("{BASE}/{zoom}/{x}/{y}.png")
    }

    /// A tile's heights, from the cache or the network, and whether from the cache. What is kept is only what decodes.
    fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<(Vec<f32>, bool), String> {
        let url = Self::url(zoom, x, y);
        let name = key(&[b"terrarium", url.as_bytes()]);
        if let Some(heights) = self.cache.read(&name, TTL).and_then(|b| decode(&b).ok()) {
            return Ok((heights, true));
        }
        let reply = self.net.get(&url, MAX_TILE).map_err(|e| format!("tile {zoom}/{x}/{y}: {e}"))?;
        let heights = decode(&reply.body).map_err(|e| format!("tile {zoom}/{x}/{y}: {e}"))?;
        self.cache.write(&name, &reply.body);
        Ok((heights, false))
    }

    /// A grid of heights over the box, a node every `spacing` metres, one more each way than the box has room for, so it covers it;
    /// and whether every tile came from the cache. Fewer, coarser tiles are taken when the box would need more than [`MAX_TILES`].
    pub fn grid(&self, bbox: &Bbox, spacing: f64) -> Result<(ElevationGrid, bool), String> {
        let layout = Layout::new(bbox, spacing)?;
        let extent = layout.extent(bbox)?;
        let mut zoom = zoom_for(bbox, spacing);
        let tiles = loop {
            match tiles_for(&extent, zoom) {
                Ok(t) => break t,
                Err(_) if zoom > 0 => zoom -= 1,
                Err(e) => return Err(e),
            }
        };
        let (mut held, mut all_cached) = (Tiles::new(), true);
        for (x, y) in tiles {
            let (heights, cached) = self.tile(zoom, x, y)?;
            all_cached &= cached;
            held.insert((x, y), heights);
        }
        let values: Vec<f32> = (0..layout.rows)
            .flat_map(|r| (0..layout.cols).map(move |c| (r, c)))
            .map(|(r, c)| {
                let (lon, lat) = layout.at(bbox, c, r);
                sample(&held, zoom, lon, lat).unwrap_or(f32::NAN)
            })
            .collect();
        Ok((ElevationGrid::new(bbox.west, bbox.north, layout.dlon, layout.dlat, layout.cols, layout.rows, values)?, all_cached))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Fake, Scratch};
    use bimcraft_core::geo::{MAX_TILES, tile_lonlat};

    const HOME: (f64, f64) = (-46.6333, -23.5505);

    /// The ground in the tiles: 760 m at home, 1 m more each 0.001 degrees east, 2 m each 0.001 north.
    fn ground(lon: f64, lat: f64) -> f64 {
        760.0 + 1000.0 * (lon - HOME.0) + 2000.0 * (lat - HOME.1)
    }

    fn png(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color);
        enc.set_depth(depth);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(data).unwrap();
        w.finish().unwrap();
        out
    }

    /// A tile of the ground, as Terrarium encodes it.
    fn tile(z: u8, x: u32, y: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity(SIDE * SIDE * 3);
        for j in 0..SIDE {
            for i in 0..SIDE {
                let (lon, lat) = tile_lonlat(f64::from(x) + (i as f64 + 0.5) / 256.0, f64::from(y) + (j as f64 + 0.5) / 256.0, z);
                let v = ground(lon, lat) + 32768.0;
                let (whole, frac) = (v.floor(), v - v.floor());
                data.extend_from_slice(&[(whole / 256.0).floor() as u8, (whole % 256.0) as u8, (frac * 256.0).floor() as u8]);
            }
        }
        png(256, 256, png::ColorType::Rgb, png::BitDepth::Eight, &data)
    }

    fn block() -> Bbox {
        Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap()
    }

    /// A network holding every tile of `bbox` at `z`.
    fn net_for(bbox: &Bbox, z: u8) -> Arc<Fake> {
        let mut fake = Fake::new();
        for (x, y) in tiles_for(bbox, z).unwrap() {
            fake = fake.get(&Terrarium::url(z, x, y), &tile(z, x, y));
        }
        Arc::new(fake)
    }

    fn near(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn a_pixel_is_its_height_from_red_green_and_blue() {
        // 100.5 m is 32868.5: R = 128, G = 100, B = 128.
        let mut data = vec![0u8; SIDE * SIDE * 3];
        data[..3].copy_from_slice(&[128, 100, 128]);
        data[3..6].copy_from_slice(&[127, 255, 0]);
        data[(SIDE * SIDE - 1) * 3..].copy_from_slice(&[0, 0, 0]);
        let h = decode(&png(256, 256, png::ColorType::Rgb, png::BitDepth::Eight, &data)).unwrap();
        assert_eq!(h.len(), SIDE * SIDE);
        assert_eq!((h[0], h[1], h[SIDE * SIDE - 1]), (100.5, -1.0, -32768.0));
        // With an alpha channel too.
        let rgba: Vec<u8> = data.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
        assert_eq!(decode(&png(256, 256, png::ColorType::Rgba, png::BitDepth::Eight, &rgba)).unwrap()[0], 100.5);
    }

    #[test]
    fn what_is_not_a_tile_is_refused_before_it_is_trusted() {
        let rgb = |w: u32, h: u32| png(w, h, png::ColorType::Rgb, png::BitDepth::Eight, &vec![0u8; (w * h * 3) as usize]);
        let good = rgb(256, 256);
        assert!(decode(&good).is_ok());
        for (what, bytes) in [
            ("not a picture", b"<html>Access Denied</html>".to_vec()),
            ("empty", Vec::new()),
            ("cut short", good[..good.len() / 2].to_vec()),
            ("only the signature", good[..8].to_vec()),
            ("512 square", rgb(512, 512)),
            ("1 pixel", rgb(1, 1)),
            ("wide", rgb(256, 128)),
            ("grey", png(256, 256, png::ColorType::Grayscale, png::BitDepth::Eight, &vec![0u8; 256 * 256])),
            ("16 bit", png(256, 256, png::ColorType::Rgb, png::BitDepth::Sixteen, &vec![0u8; 256 * 256 * 6])),
        ] {
            assert!(decode(&bytes).is_err(), "{what}");
        }
        // A header that claims a picture of billions of pixels: refused from the header, with nothing made of that size.
        let mut huge = good.clone();
        huge[16..20].copy_from_slice(&100_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(decode(&huge).is_err());
    }

    #[test]
    fn the_tiles_of_a_box_are_laid_together_and_sampled_onto_a_grid_without_a_seam() {
        let b = block();
        // 10 m between nodes asks for zoom 14 (8.8 m a pixel; zoom 13 is 17.5), which cuts this box into four tiles: its seams run through it.
        let net = net_for(&b, 14);
        let dir = Scratch::new("terrarium");
        let t = Terrarium::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let (g, cached) = t.grid(&b, 10.0).unwrap();
        assert!(!cached);
        assert_eq!(net.asked().len(), 4);
        assert!(net.asked().iter().all(|(m, u, _)| m == "GET" && u.contains("/14/")), "{:?}", net.asked());
        // The grid covers the box, and its heights are the ground's, in the tiles and across their seams.
        assert!(g.west <= b.west + 1e-9 && g.north >= b.north - 1e-9);
        assert!(g.west + (g.cols - 1) as f64 * g.dlon >= b.east && g.north - (g.rows - 1) as f64 * g.dlat <= b.south);
        let (x, y) = tile_xy(b.west, b.north, 14);
        let (seam_lon, seam_lat) = tile_lonlat(x.floor() + 1.0, y.floor() + 1.0, 14);
        for (lon, lat) in [
            (-46.6333, -23.5505),
            (b.west + 0.0003, b.south + 0.0003),
            (b.east - 0.0003, b.north - 0.0003),
            (seam_lon, seam_lat),
            (seam_lon + 1e-5, seam_lat - 1e-5),
            (seam_lon - 1e-4, seam_lat + 1e-4),
        ] {
            let z = g.sample(lon, lat).unwrap_or_else(|| panic!("no height at {lon} {lat}"));
            assert!(near(z, ground(lon, lat), 0.05), "{lon} {lat}: {z} vs {}", ground(lon, lat));
        }
        // Asked again, it is the cache's.
        let (again, cached) = t.grid(&b, 10.0).unwrap();
        assert!(cached && again == g);
        assert_eq!(net.asked().len(), 4, "the server was not asked again");
    }

    #[test]
    fn a_tile_that_will_not_come_or_is_not_a_tile_is_an_error_naming_it() {
        let b = block();
        let t = Terrarium::new(Arc::new(Fake::new()), Arc::new(Cache::new(None)));
        let e = t.grid(&b, 10.0).unwrap_err();
        assert!(e.contains("14/6069/9294") && e.contains("404"), "{e}");
        let mut junk = Fake::new();
        for (x, y) in tiles_for(&b, 14).unwrap() {
            junk = junk.get(&Terrarium::url(14, x, y), b"<html>");
        }
        let t = Terrarium::new(Arc::new(junk), Arc::new(Cache::new(None)));
        assert!(t.grid(&b, 10.0).unwrap_err().contains("tile 14/"));
        // And a bad spacing.
        let t = Terrarium::new(net_for(&b, 14), Arc::new(Cache::new(None)));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(t.grid(&b, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_box_that_needs_too_many_tiles_takes_a_coarser_zoom_and_one_nobody_can_hold_is_refused() {
        // 25 km2 at 1 m would be hundreds of tiles at zoom 15: it goes up a level or two instead.
        let wide = Bbox::around(HOME.0, HOME.1, 2500.0).unwrap();
        let mut fake = Fake::new();
        for z in 8..=15 {
            if let Ok(tiles) = tiles_for(&wide, z) {
                for (x, y) in tiles {
                    fake = fake.get(&Terrarium::url(z, x, y), &tile(z, x, y));
                }
            }
        }
        let net = Arc::new(fake);
        let t = Terrarium::new(net.clone(), Arc::new(Cache::new(None)));
        let (g, _) = t.grid(&wide, 20.0).unwrap();
        assert!(net.asked().len() <= MAX_TILES);
        assert!(near(g.sample(HOME.0, HOME.1).unwrap(), ground(HOME.0, HOME.1), 0.1));
        // Too many nodes.
        assert!(t.grid(&wide, 0.05).unwrap_err().contains("spacing"));
    }
}
