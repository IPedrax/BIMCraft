//! The tiles of an aerial imagery plan, fetched a few at a time, kept on disk, and laid in a mosaic. What a service sends is hostile:
//! a tile is taken only if its bytes are a PNG or a JPEG of 256 by 256 pixels (told by the bytes, not by what the service says it
//! is), a reply is read to a cap, and what is kept on disk is decoded again when it is read. A tile the service has none of (an
//! error status 404, or a picture of one flat colour) is a tile with no imagery, left clear; any other trouble fails the fetch, so the
//! picture never has a hole that is not the service's.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bimcraft_core::imagery::{Imagery, Mosaic, Plan, Source, TILE};
use bimcraft_core::texture::{decode_rgb, flat_rgb};

use crate::cache::{Cache, key};
use crate::net::Net;
use crate::probe::Format;

/// How long a tile is kept, seconds: thirty days.
pub const TTL: u64 = 30 * 24 * 3600;
/// The most a tile's file may be, bytes.
pub const MAX_TILE: u64 = 512 << 10;
/// The most tiles asked for at once from a service.
pub const WORKERS: usize = 4;

pub struct ImageryTiles {
    net: Arc<dyn Net>,
    cache: Arc<Cache>,
}

impl ImageryTiles {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>) -> ImageryTiles {
        ImageryTiles { net, cache }
    }

    /// The tiles of `plan` from `source`, as a mosaic. Every address is made first, so a plan the source cannot serve is refused before
    /// anything is asked; then at most [`WORKERS`] tiles are asked for at once, and the mosaic is laid in the plan's order.
    pub fn fetch(&self, source: &Source, plan: &Plan) -> Result<Imagery, String> {
        let mut mosaic = Mosaic::new(plan);
        mosaic.check()?;
        let wanted = (0..plan.rows)
            .flat_map(|row| (0..plan.cols).map(move |col| (col, row)))
            .map(|(col, row)| source.url(plan.zoom, plan.x0 + col, plan.y0 + row).map(|url| (col, row, url)))
            .collect::<Result<Vec<_>, _>>()?;
        let results: Mutex<Vec<Option<Result<Tile, String>>>> = Mutex::new(wanted.iter().map(|_| None).collect());
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..WORKERS.min(wanted.len()) {
                scope.spawn(|| {
                    while let Some((col, row, url)) = wanted.get(next.fetch_add(1, Ordering::SeqCst)) {
                        let got = self.tile(source, plan, (*col, *row), url);
                        if let Ok(mut all) = results.lock()
                            && let Some(slot) = all.get_mut(row_major(plan, *col, *row))
                        {
                            *slot = Some(got);
                        }
                    }
                });
            }
        });
        let results = results.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (mut any, mut cached) = (false, true);
        for ((col, row, _), result) in wanted.iter().zip(results) {
            match result {
                Some(Ok(Tile::Imagery(rgb, from_cache))) => {
                    mosaic.put(*col, *row, &rgb)?;
                    (any, cached) = (true, cached && from_cache);
                }
                Some(Ok(Tile::None)) => {}
                Some(Err(why)) => return Err(why),
                None => return Err(format!("{} tile {col}, {row} was not fetched", source.name)),
            }
        }
        if !any {
            return Err(format!("{} has no imagery of this place", source.name));
        }
        Ok(Imagery { source: source.id.into(), plan: plan.clone(), mosaic, cached })
    }

    /// One tile: from the disk if it is kept and still decodes, else from the service.
    fn tile(&self, source: &Source, plan: &Plan, (col, row): (u32, u32), url: &str) -> Result<Tile, String> {
        let label = format!("{} tile {}/{}/{}", source.name, plan.zoom, plan.x0 + col, plan.y0 + row);
        let name = key(&[b"imagery", source.id.as_bytes(), url.as_bytes()]);
        if let Some(rgb) = self.cache.read(&name, TTL).and_then(|bytes| decode_tile(&bytes).ok()) {
            return Ok(Tile::of(rgb, true));
        }
        let reply = match self.net.get(url, MAX_TILE) {
            Ok(reply) => reply,
            // The service says it has none.
            Err(e) if e.status == Some(404) => return Ok(Tile::None),
            Err(e) => return Err(format!("{label}: {e}")),
        };
        let rgb = decode_tile(&reply.body).map_err(|e| format!("{label}: {e}"))?;
        self.cache.write(&name, &reply.body);
        Ok(Tile::of(rgb, false))
    }
}

/// What came of one tile.
enum Tile {
    /// Its pixels (RGB), and whether they came from the disk.
    Imagery(Vec<u8>, bool),
    /// The service has none of this place.
    None,
}

impl Tile {
    /// A tile of one flat colour is what a service sends for a place it has no imagery of.
    fn of(rgb: Vec<u8>, cached: bool) -> Tile {
        if flat_rgb(&rgb).is_some() { Tile::None } else { Tile::Imagery(rgb, cached) }
    }
}

/// The place of the tile at `col`, `row` in a list that runs along each row of the plan in turn.
fn row_major(plan: &Plan, col: u32, row: u32) -> usize {
    row as usize * plan.cols as usize + col as usize
}

/// The pixels (RGB) of a tile's bytes: a PNG or a JPEG, told by its bytes, of exactly 256 by 256 pixels, decoded under limits.
fn decode_tile(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let format = Format::sniff(bytes);
    if !matches!(format, Format::Png | Format::Jpeg) {
        return Err(format!("a tile is a PNG or a JPEG, and this is not a picture BIMCraft can read ({})", format.name()));
    }
    let (w, h, rgb) = decode_rgb(bytes).map_err(|e| format!("the picture is damaged or too big: {e}"))?;
    if (w, h) != (TILE, TILE) || rgb.len() != TILE * TILE * 3 {
        return Err(format!("a tile is {TILE} x {TILE} pixels, not {w} x {h}"));
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Fake, Scratch};
    use bimcraft_core::imagery::source;

    const JPEG: &[u8] = include_bytes!("../testdata/tile256.jpg");

    fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&pixel(x, y));
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&data).unwrap();
        w.finish().unwrap();
        out
    }

    /// A tile whose pixel (x, y) is (x, y, tag): never one colour.
    fn tile(tag: u8) -> Vec<u8> {
        png(256, 256, |x, y| [x as u8, y as u8, tag])
    }

    fn plan(cols: u32, rows: u32) -> Plan {
        Plan { zoom: 18, x0: 1000, y0: 2000, cols, rows, metres_per_pixel: 0.37, width: 100, height: 100 }
    }

    fn url(s: &Source, p: &Plan, col: u32, row: u32) -> String {
        s.url(p.zoom, p.x0 + col, p.y0 + row).unwrap()
    }

    /// A net that has each tile of `plan` as `body(col, row)` says.
    fn net_for(s: &Source, p: &Plan, body: impl Fn(u32, u32) -> Vec<u8>) -> Fake {
        let mut net = Fake::new();
        for row in 0..p.rows {
            for col in 0..p.cols {
                net = net.get(&url(s, p, col, row), &body(col, row));
            }
        }
        net
    }

    fn fetcher(net: impl Net + 'static, dir: &Scratch) -> ImageryTiles {
        ImageryTiles::new(Arc::new(net), Arc::new(Cache::new(Some(dir.0.clone()))))
    }

    #[test]
    fn the_tiles_of_a_plan_come_in_a_mosaic_each_in_its_place_and_are_kept() {
        let dir = Scratch::new("imagery");
        let (s, p) = (source("pdok").unwrap(), plan(3, 2));
        let net = Arc::new(net_for(s, &p, |col, row| tile((col * 40 + row * 7) as u8)));
        let tiles = ImageryTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let got = tiles.fetch(s, &p).unwrap();
        got.mosaic.check().unwrap();
        assert_eq!((got.source.as_str(), got.cached, got.plan.clone()), ("pdok", false, p.clone()));
        assert_eq!(got.mosaic.has, [true; 6]);
        let at = |col: usize, row: usize, x: usize, y: usize| -> [u8; 3] {
            let i = ((row * 3 + col) * TILE * TILE + y * TILE + x) * 3;
            [got.mosaic.rgb[i], got.mosaic.rgb[i + 1], got.mosaic.rgb[i + 2]]
        };
        assert_eq!(at(2, 1, 5, 9), [5, 9, 2 * 40 + 7], "the tile at column 2, row 1");
        assert_eq!(at(0, 0, 255, 0), [255, 0, 0]);
        assert_eq!(at(1, 0, 0, 255), [0, 255, 40]);
        assert_eq!(net.asked().len(), 6);
        // Again, from the disk: a net that knows nothing is not asked.
        let again = fetcher(Fake::new(), &dir).fetch(s, &p).unwrap();
        assert!(again.cached && again.mosaic == got.mosaic);
    }

    #[test]
    fn a_tile_the_service_has_none_of_is_left_clear_and_the_rest_come() {
        let dir = Scratch::new("imagery-holes");
        let (s, p) = (source("usgs").unwrap(), Plan { zoom: 16, ..plan(2, 2) });
        let flat = png(256, 256, |_, _| [255, 255, 255]);
        let net = Fake::new()
            .get(&url(s, &p, 0, 0), &tile(1))
            .status("GET", &url(s, &p, 1, 0), 404, None)
            .get(&url(s, &p, 0, 1), &flat)
            .get(&url(s, &p, 1, 1), &tile(4));
        let got = fetcher(net, &dir).fetch(s, &p).unwrap();
        got.mosaic.check().unwrap();
        assert_eq!(got.mosaic.has, [true, false, false, true], "a 404 and a tile of one colour have no imagery");
        let tile_bytes = TILE * TILE * 3;
        assert!(got.mosaic.rgb[tile_bytes..2 * tile_bytes].iter().all(|b| *b == 0), "nothing was put where there is none");
        // Where there is none anywhere, there is no imagery, and it says so.
        let none = Fake::new()
            .status("GET", &url(s, &p, 0, 0), 404, None)
            .get(&url(s, &p, 1, 0), &flat)
            .status("GET", &url(s, &p, 0, 1), 404, None)
            .get(&url(s, &p, 1, 1), &flat);
        let why = fetcher(none, &Scratch::new("imagery-none")).fetch(s, &p).unwrap_err();
        assert!(why.contains("no imagery") && why.contains(s.name), "{why}");
    }

    #[test]
    fn a_tile_that_is_not_one_or_will_not_come_fails_the_fetch_and_says_which() {
        let (s, p) = (source("pdok").unwrap(), plan(1, 1));
        let u = url(s, &p, 0, 0);
        let hugs = {
            let mut huge = tile(1);
            huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
            huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
            huge
        };
        for (what, net, says) in [
            ("a page", Fake::new().get(&u, b"<html>429 Too Many Requests</html>"), "not a picture"),
            ("nothing", Fake::new().get(&u, b""), "not a picture"),
            ("a gif", Fake::new().get(&u, b"GIF89a\x01\0\x01\0"), "not a picture"),
            ("a small picture", Fake::new().get(&u, &png(128, 128, |x, y| [x as u8, y as u8, 3])), "256"),
            ("a wide picture", Fake::new().get(&u, &png(512, 256, |x, y| [x as u8, y as u8, 3])), "256"),
            ("a cut-off picture", Fake::new().get(&u, &tile(3)[..300]), "damaged"),
            ("a picture of a country's size", Fake::new().get(&u, &hugs), "damaged"),
            ("an error", Fake::new().status("GET", &u, 500, None), "500"),
            ("a refusal to serve", Fake::new().status("GET", &u, 429, Some(30)), "429"),
            ("a forbidden", Fake::new().status("GET", &u, 403, None), "403"),
        ] {
            let why = fetcher(net, &Scratch::new("imagery-bad")).fetch(s, &p).unwrap_err();
            assert!(why.contains(says), "{what}: {why}");
        }
        // It says which tile, by its place on the map.
        let why = fetcher(Fake::new().status("GET", &u, 500, None), &Scratch::new("imagery-which")).fetch(s, &p).unwrap_err();
        assert!(why.contains("18/1000/2000"), "{why}");
        // A JPEG is a tile too.
        let ok = fetcher(Fake::new().get(&u, JPEG), &Scratch::new("imagery-jpeg")).fetch(s, &p).unwrap();
        assert_eq!(ok.mosaic.has, [true]);
        assert!(ok.mosaic.rgb[3 * 255..3 * 255 + 3][0] > 200, "red rises across it");
    }

    #[test]
    fn a_damaged_file_in_the_cache_is_fetched_again_not_used() {
        let dir = Scratch::new("imagery-damaged");
        let (s, p) = (source("pdok").unwrap(), plan(1, 1));
        let u = url(s, &p, 0, 0);
        Cache::new(Some(dir.0.clone())).write(&key(&[b"imagery", s.id.as_bytes(), u.as_bytes()]), b"not a picture");
        let net = Arc::new(Fake::new().get(&u, &tile(9)));
        let got = ImageryTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone())))).fetch(s, &p).unwrap();
        assert!(!got.cached && net.asked().len() == 1);
    }

    #[test]
    fn a_plan_that_is_not_one_the_source_can_serve_is_refused_before_anything_is_asked() {
        let dir = Scratch::new("imagery-plans");
        let net = Arc::new(Fake::new());
        let tiles = ImageryTiles::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let usgs = source("usgs").unwrap();
        for (what, s, p) in [
            ("a zoom the source lacks", usgs, Plan { zoom: 17, ..plan(1, 1) }),
            ("no tiles", usgs, plan(0, 1)),
            ("too many tiles", usgs, Plan { zoom: 16, ..plan(11, 10) }),
            ("tiles off the map", usgs, Plan { zoom: 3, x0: 7, ..plan(2, 1) }),
            ("a count that overflows", usgs, plan(u32::MAX, u32::MAX)),
        ] {
            assert!(tiles.fetch(s, &p).is_err(), "{what}");
        }
        assert!(net.asked().is_empty());
    }

    /// A net that gives the same tile to every request after a moment, and counts how many are being asked at once.
    struct Counting {
        body: Vec<u8>,
        now: AtomicUsize,
        most: AtomicUsize,
    }

    impl Net for Counting {
        fn get(&self, _: &str, _: u64) -> Result<crate::net::Reply, crate::net::NetError> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok(crate::net::Reply { body: self.body.clone(), content_type: Some("image/png".into()) })
        }
        fn get_range(&self, _: &str, _: u64, _: u64) -> Result<crate::net::Reply, crate::net::NetError> {
            Err(crate::net::NetError::new("no"))
        }
        fn post(&self, _: &str, _: &str, _: &[u8], _: u64) -> Result<crate::net::Reply, crate::net::NetError> {
            Err(crate::net::NetError::new("no"))
        }
    }

    #[test]
    fn a_service_is_asked_for_a_few_tiles_at_a_time_and_the_mosaic_is_in_order_not_arrival_order() {
        let (s, p) = (source("pdok").unwrap(), plan(5, 5));
        let counting = Arc::new(Counting { body: tile(5), now: AtomicUsize::new(0), most: AtomicUsize::new(0) });
        let tiles = ImageryTiles::new(counting.clone(), Arc::new(Cache::new(None)));
        let got = tiles.fetch(s, &p).unwrap();
        let most = counting.most.load(Ordering::SeqCst);
        assert!((2..=WORKERS).contains(&most), "{most} at once");
        assert_eq!(got.mosaic.has, vec![true; 25]);
        got.mosaic.check().unwrap();
        // Every tile is the same here, so any tile put in the wrong place would still look right: the order is tested above, where
        // each tile is its own.
    }
}
