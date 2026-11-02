//! The map a place is picked on: OpenStreetMap's standard tiles (256 x 256 pixel PNGs on the Web Mercator map), fetched one at a time
//! when the window asks for them, decoded to RGBA and kept two weeks (the tile usage policy asks for at least a week). Nothing here
//! fetches ahead or in bulk: a tile is asked for only when the picker is on screen and shows it.
//!
//! OpenStreetMap's tile servers are a free service with a usage policy (https://operations.osmfoundation.org/policies/tiles/): an
//! identifying `User-Agent` (the fetcher's `Ureq` sends BIMCraft's), caching, no bulk downloads, at most two connections (the
//! window's loader keeps to that) and the credit on screen, which is [`ATTRIBUTION`].

use std::sync::Arc;

use bimcraft_core::geo::Tile;

use crate::cache::{Cache, key};
use crate::net::Net;

/// How long a tile is kept, seconds: two weeks.
pub const TTL: u64 = 14 * 24 * 3600;
/// The most a tile's file may be, bytes.
pub const MAX_TILE: u64 = 512 << 10;
/// The side of a tile, pixels.
pub const SIDE: u32 = 256;
/// The finest zoom OpenStreetMap's tiles go to.
pub const MAX_ZOOM: u8 = 19;
/// Where the tiles are.
pub const BASE: &str = "https://tile.openstreetmap.org";
/// What the map asks to be credited, in the corner of the picture.
pub const ATTRIBUTION: &str = "© OpenStreetMap contributors";

pub struct OsmTiles {
    net: Arc<dyn Net>,
    cache: Arc<Cache>,
}

/// A tile's PNG as RGBA, or why the file is not a tile. The header is read and checked (256 x 256) before anything of that size is
/// made, and the decoder is held to a megabyte or two; a palette, grey or 16-bit picture is brought to 8-bit colour.
pub fn decode(bytes: &[u8]) -> Result<Tile, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_limits(png::Limits { bytes: 2 << 20 });
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("it is not a picture BIMCraft can read: {e}"))?;
    let info = reader.info();
    if (info.width, info.height) != (SIDE, SIDE) {
        return Err(format!("a tile is {SIDE} x {SIDE} pixels, not {} x {}", info.width, info.height));
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf).map_err(|e| format!("the picture is damaged: {e}"))?;
    let pixels = (SIDE * SIDE) as usize;
    let data = &buf[..frame.buffer_size()];
    let rgba: Vec<u8> = match (frame.color_type, frame.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) if data.len() == pixels * 4 => data.to_vec(),
        (png::ColorType::Rgb, png::BitDepth::Eight) if data.len() == pixels * 3 => {
            data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
        }
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) if data.len() == pixels * 2 => {
            data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        (png::ColorType::Grayscale, png::BitDepth::Eight) if data.len() == pixels => data.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        (c, d) => return Err(format!("a tile is an 8-bit colour picture of {SIDE} x {SIDE}, not {c:?} at {d:?} in {} bytes", data.len())),
    };
    Ok(Tile { rgba, side: SIDE, cached: false })
}

impl OsmTiles {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>) -> OsmTiles {
        OsmTiles { net, cache }
    }

    /// The address of a tile, or why there is no such tile: the map at `zoom` has `2^zoom` columns and rows.
    pub fn url(zoom: u8, x: u32, y: u32) -> Result<String, String> {
        if zoom > MAX_ZOOM {
            return Err(format!("the map has no tiles finer than zoom {MAX_ZOOM}, not {zoom}"));
        }
        let n = 1u64 << zoom;
        if u64::from(x) >= n || u64::from(y) >= n {
            return Err(format!("there is no tile {x}/{y} at zoom {zoom}: the map has {n} columns and rows there"));
        }
        Ok(format!("{BASE}/{zoom}/{x}/{y}.png"))
    }

    /// A tile, from the cache or the network. What is kept is only what decodes.
    pub fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<Tile, String> {
        let url = Self::url(zoom, x, y)?;
        let name = key(&[b"osm-tile", url.as_bytes()]);
        if let Some(mut tile) = self.cache.read(&name, TTL).and_then(|b| decode(&b).ok()) {
            tile.cached = true;
            return Ok(tile);
        }
        let reply = self.net.get(&url, MAX_TILE).map_err(|e| format!("map tile {zoom}/{x}/{y}: {e}"))?;
        let tile = decode(&reply.body).map_err(|e| format!("map tile {zoom}/{x}/{y}: {e}"))?;
        self.cache.write(&name, &reply.body);
        Ok(tile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Fake, Scratch};

    fn png(width: u32, height: u32, color: png::ColorType, depth: png::BitDepth, palette: Option<&[u8]>, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color);
        enc.set_depth(depth);
        if let Some(p) = palette {
            enc.set_palette(p.to_vec());
        }
        let mut w = enc.write_header().unwrap();
        w.write_image_data(data).unwrap();
        w.finish().unwrap();
        out
    }

    /// A tile of `n` bytes of colour where the pixel in column `i` and row `j` has red `i`, green `j`.
    fn rgb_tile() -> Vec<u8> {
        let data: Vec<u8> = (0..SIDE).flat_map(|j| (0..SIDE).flat_map(move |i| [i as u8, j as u8, 7])).collect();
        png(SIDE, SIDE, png::ColorType::Rgb, png::BitDepth::Eight, None, &data)
    }

    #[test]
    fn each_kind_of_png_a_tile_may_be_comes_out_as_rgba() {
        let t = decode(&rgb_tile()).unwrap();
        assert_eq!((t.side, t.rgba.len(), t.cached), (256, 256 * 256 * 4, false));
        t.check().unwrap();
        // Pixel (5, 9): red 5, green 9, blue 7, opaque.
        let at = (9 * 256 + 5) * 4;
        assert_eq!(&t.rgba[at..at + 4], &[5, 9, 7, 255]);
        // The map's own tiles are paletted: index 1 is the second colour.
        let palette = [10, 20, 30, 200, 210, 220];
        let indices: Vec<u8> = (0..256 * 256).map(|i| (i % 2) as u8).collect();
        let t = decode(&png(SIDE, SIDE, png::ColorType::Indexed, png::BitDepth::Eight, Some(&palette), &indices)).unwrap();
        assert_eq!((&t.rgba[0..4], &t.rgba[4..8]), (&[10, 20, 30, 255][..], &[200, 210, 220, 255][..]));
        // A 4-bit palette, as small files have.
        let packed: Vec<u8> = vec![0x10; 256 * 256 / 2];
        let t = decode(&png(SIDE, SIDE, png::ColorType::Indexed, png::BitDepth::Four, Some(&palette), &packed)).unwrap();
        // 0x10 is index 1 then index 0, high nibble first.
        assert_eq!((&t.rgba[0..4], &t.rgba[4..8]), (&[200, 210, 220, 255][..], &[10, 20, 30, 255][..]));
        // Grey, grey with alpha, RGBA and 16-bit.
        let grey = decode(&png(SIDE, SIDE, png::ColorType::Grayscale, png::BitDepth::Eight, None, &vec![77u8; 256 * 256])).unwrap();
        assert_eq!(&grey.rgba[0..4], &[77, 77, 77, 255]);
        let ga =
            decode(&png(SIDE, SIDE, png::ColorType::GrayscaleAlpha, png::BitDepth::Eight, None, &[9u8, 128].repeat(256 * 256))).unwrap();
        assert_eq!(&ga.rgba[0..4], &[9, 9, 9, 128]);
        let rgba = decode(&png(SIDE, SIDE, png::ColorType::Rgba, png::BitDepth::Eight, None, &[1u8, 2, 3, 4].repeat(256 * 256))).unwrap();
        assert_eq!(&rgba.rgba[0..4], &[1, 2, 3, 4]);
        let deep = decode(&png(SIDE, SIDE, png::ColorType::Rgb, png::BitDepth::Sixteen, None, &vec![0x80u8; 256 * 256 * 6])).unwrap();
        assert_eq!(&deep.rgba[0..4], &[0x80, 0x80, 0x80, 255]);
    }

    #[test]
    fn what_is_not_a_tile_is_refused_before_it_is_trusted() {
        let wrong_size = |w: u32, h: u32| png(w, h, png::ColorType::Rgb, png::BitDepth::Eight, None, &vec![0u8; (w * h * 3) as usize]);
        for (what, bytes) in [
            ("empty", Vec::new()),
            ("text", b"<html>429 Too Many Requests</html>".to_vec()),
            ("small", wrong_size(128, 128)),
            ("wide", wrong_size(512, 256)),
            ("truncated", rgb_tile()[..200].to_vec()),
        ] {
            assert!(decode(&bytes).is_err(), "{what}");
        }
        // A header that claims a picture the size of a country is refused by the limits, not by running out of memory.
        let mut huge = rgb_tile();
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        assert!(decode(&huge).is_err());
    }

    #[test]
    fn a_tile_is_asked_for_by_its_place_on_the_map_and_no_other() {
        assert_eq!(OsmTiles::url(12, 1517, 2332).unwrap(), "https://tile.openstreetmap.org/12/1517/2332.png");
        assert_eq!(OsmTiles::url(0, 0, 0).unwrap(), "https://tile.openstreetmap.org/0/0/0.png");
        assert!(OsmTiles::url(19, (1 << 19) - 1, (1 << 19) - 1).is_ok());
        for (z, x, y) in [(20, 0, 0), (0, 1, 0), (0, 0, 1), (3, 8, 0), (3, 0, 8), (255, 0, 0), (19, 1 << 19, 0), (3, u32::MAX, 0)] {
            assert!(OsmTiles::url(z, x, y).is_err(), "{z}/{x}/{y}");
        }
    }

    #[test]
    fn a_tile_is_fetched_once_and_then_kept() {
        let dir = Scratch::new("tiles");
        let url = OsmTiles::url(12, 1517, 2332).unwrap();
        let net = Arc::new(Fake::new().get(&url, &rgb_tile()));
        let tiles = OsmTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let first = tiles.tile(12, 1517, 2332).unwrap();
        assert!(!first.cached);
        let again = tiles.tile(12, 1517, 2332).unwrap();
        assert!(again.cached && again.rgba == first.rgba);
        assert_eq!(net.asked().len(), 1, "the second was the cache's");
        // A damaged file in the cache is fetched again, not shown.
        let name = key(&[b"osm-tile", url.as_bytes()]);
        Cache::new(Some(dir.0.clone())).write(&name, b"not a png");
        let net = Arc::new(Fake::new().get(&url, &rgb_tile()));
        let tiles = OsmTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        assert!(!tiles.tile(12, 1517, 2332).unwrap().cached);
        assert_eq!(net.asked().len(), 1);
    }

    #[test]
    fn a_tile_that_will_not_come_or_is_not_one_says_which_and_keeps_nothing() {
        let dir = Scratch::new("tiles-bad");
        let url = OsmTiles::url(5, 10, 12).unwrap();
        let net = Arc::new(Fake::new().status("GET", &url, 429, Some(30)).get(&url, b"<html>blocked</html>"));
        let tiles = OsmTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let e = tiles.tile(5, 10, 12).unwrap_err();
        assert!(e.contains("map tile 5/10/12") && e.contains("429"), "{e}");
        let e = tiles.tile(5, 10, 12).unwrap_err();
        assert!(e.contains("map tile 5/10/12"), "{e}");
        assert!(tiles.cache.read(&key(&[b"osm-tile", url.as_bytes()]), TTL).is_none(), "a reply that is not a tile is not kept");
        // A tile off the map is not even asked for.
        assert!(tiles.tile(5, 32, 0).is_err());
        assert_eq!(net.asked().len(), 2);
    }
}
