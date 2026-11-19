//! Copernicus DEM GLO-30: one consistent 30 m surface model for the world, in one-degree GeoTIFF tiles (cloud-optimised: tiled,
//! float32, deflate with the floating-point predictor) in a public bucket. A tile is a few tens of megabytes, so it is never fetched
//! whole: the file is read through range requests, in blocks kept on disk, and only the chunks of it a box needs are decoded.
//!
//! The data is Copernicus DEM GLO-30 (c) DLR e.V. 2010-2014 and (c) Airbus Defence and Space GmbH 2014-2018, provided under
//! COPERNICUS by the European Union and ESA; all rights reserved ([`ATTRIBUTION`]).

use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bimcraft_core::geo::{Bbox, ElevationGrid};
use tiff::decoder::{Decoder, DecodingResult, Limits};
use tiff::tags::Tag;

use crate::cache::{Cache, key};
use crate::layout::Layout;
use crate::net::Net;

/// What the data asks to be credited.
pub const ATTRIBUTION: &str = "Contains modified Copernicus DEM GLO-30 data: (c) DLR e.V. 2010-2014 and (c) Airbus Defence and Space GmbH \
    2014-2018, provided under COPERNICUS by the European Union and ESA; all rights reserved";
/// Where the tiles are.
pub const BASE: &str = "https://copernicus-dem-30m.s3.amazonaws.com";
/// How long a block is kept, seconds: the model does not change.
pub const TTL: u64 = 90 * 24 * 3600;

/// The size of a block of a remote file, bytes.
const BLOCK: u64 = 128 << 10;
/// The most blocks of one file that are read, so a damaged file cannot be read for ever.
const MAX_BLOCKS: usize = 400;
/// The most pixels a tile may have: a one-degree tile is 3600 x 3600, and the file is not trusted to say fewer than it has.
const MAX_PIXELS: u64 = 40_000_000;
/// The most one-degree tiles a box may span.
const MAX_FILES: usize = 9;
/// What a missing tile looks like in the data: below any ground.
const FLOOR: f32 = -1000.0;

pub struct Copernicus {
    net: Arc<dyn Net>,
    cache: Arc<Cache>,
}

/// A file on a server, read as a local one is: through ranges of it, a block at a time, kept in memory and on disk.
pub(crate) struct Remote<'a> {
    net: &'a dyn Net,
    cache: &'a Cache,
    url: String,
    pos: u64,
    blocks: HashMap<u64, Vec<u8>>,
    /// How long the file is, once a block that ends short has said.
    size: Option<u64>,
    /// How many blocks came from the network, shared so the caller can tell after the file is handed to the decoder.
    fetched: Arc<AtomicUsize>,
    /// The status of the last request that failed.
    status: Option<u16>,
}

impl<'a> Remote<'a> {
    pub(crate) fn new(net: &'a dyn Net, cache: &'a Cache, url: String) -> Remote<'a> {
        Remote { net, cache, url, pos: 0, blocks: HashMap::new(), size: None, fetched: Arc::new(AtomicUsize::new(0)), status: None }
    }

    /// The block `i`: held, or kept on disk, or asked for.
    fn block(&mut self, i: u64) -> io::Result<&Vec<u8>> {
        if !self.blocks.contains_key(&i) {
            if self.blocks.len() >= MAX_BLOCKS {
                return Err(io::Error::other("the file is read in more pieces than a tile needs"));
            }
            let name = key(&[b"copernicus", self.url.as_bytes(), &i.to_be_bytes()]);
            let kept = self.cache.read(&name, TTL).filter(|b| b.len() as u64 <= BLOCK);
            let body = match kept {
                Some(b) => b,
                None => {
                    let body = match self.net.get_range(&self.url, i * BLOCK, BLOCK) {
                        Ok(reply) if reply.body.len() as u64 <= BLOCK => reply.body,
                        Ok(_) => return Err(io::Error::other("the server sent more than was asked for")),
                        // Past the end of the file is the end of the file.
                        Err(e) if e.status == Some(416) => Vec::new(),
                        Err(e) => {
                            self.status = e.status;
                            return Err(io::Error::other(e.message));
                        }
                    };
                    self.fetched.fetch_add(1, Ordering::Relaxed);
                    self.cache.write(&name, &body);
                    body
                }
            };
            // A block that ends short, with something in it, ends the file there; an empty one may be from far past the end.
            if !body.is_empty() && (body.len() as u64) < BLOCK {
                self.size = Some(i * BLOCK + body.len() as u64);
            }
            self.blocks.insert(i, body);
        }
        self.blocks.get(&i).ok_or_else(|| io::Error::other("a block is missing"))
    }
}

impl Read for Remote<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (i, at) = (self.pos / BLOCK, (self.pos % BLOCK) as usize);
        let block = self.block(i)?;
        // A position at or past the end of the block, which is shorter at the end of the file, reads nothing.
        let Some(rest) = block.get(at..).filter(|r| !r.is_empty()) else { return Ok(0) };
        let n = rest.len().min(buf.len());
        buf[..n].copy_from_slice(&rest[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Remote<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => match self.size {
                Some(size) => size.checked_add_signed(d),
                None => return Err(io::Error::other("the size of the file is not known yet")),
            },
        };
        self.pos = pos.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "a position before the start of the file"))?;
        Ok(self.pos)
    }
}

/// One tile of the model, opened: where it is, how it is cut into chunks, and the chunks decoded so far.
struct Tile<'a> {
    decoder: Decoder<Remote<'a>>,
    west: f64,
    north: f64,
    dx: f64,
    dy: f64,
    width: u32,
    height: u32,
    chunk_w: u32,
    chunk_h: u32,
    across: u32,
    /// Each decoded chunk's heights, and how many are in a row of it.
    chunks: HashMap<u32, (Vec<f32>, u32)>,
}

impl Tile<'_> {
    /// The height of pixel (`x`, `y`): none off the tile or where the model has none. Decodes the chunk it is in, once.
    fn pixel(&mut self, x: i64, y: i64) -> Result<Option<f32>, String> {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return Ok(None);
        }
        let (x, y) = (x as u32, y as u32);
        let index = (y / self.chunk_h) * self.across + x / self.chunk_w;
        if !self.chunks.contains_key(&index) {
            let (w, _) = self.decoder.chunk_data_dimensions(index);
            let data = match self.decoder.read_chunk(index).map_err(|e| format!("a chunk of it is damaged: {e}"))? {
                DecodingResult::F32(v) => v,
                _ => return Err("it does not hold 32-bit heights".into()),
            };
            self.chunks.insert(index, (data, w));
        }
        let Some((data, w)) = self.chunks.get(&index) else { return Ok(None) };
        let at = (y % self.chunk_h) as usize * *w as usize + (x % self.chunk_w) as usize;
        Ok(data.get(at).copied().filter(|h| h.is_finite() && *h > FLOOR))
    }

    /// The height at a place, from the pixels round it, weighted by nearness, those that exist.
    fn sample(&mut self, lon: f64, lat: f64) -> Result<Option<f32>, String> {
        let (u, v) = ((lon - self.west) / self.dx - 0.5, (self.north - lat) / self.dy - 0.5);
        let (i, j, fx, fy) = (u.floor(), v.floor(), u - u.floor(), v - v.floor());
        if !i.is_finite() || !j.is_finite() || i.abs() > 1e9 || j.abs() > 1e9 {
            return Ok(None);
        }
        let (mut sum, mut weight) = (0.0, 0.0);
        for (di, dj, w) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] {
            if w <= 0.0 {
                continue;
            }
            if let Some(h) = self.pixel(i as i64 + di, j as i64 + dj)? {
                sum += w * f64::from(h);
                weight += w;
            }
        }
        Ok((weight > 0.0).then(|| (sum / weight) as f32))
    }
}

impl Copernicus {
    pub fn new(net: Arc<dyn Net>, cache: Arc<Cache>) -> Copernicus {
        Copernicus { net, cache }
    }

    /// The address of the file for the one-degree tile whose south west corner is (`lon`, `lat`), whole degrees.
    pub fn url(lon: i32, lat: i32) -> String {
        let name = format!(
            "Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM",
            if lat >= 0 { 'N' } else { 'S' },
            lat.unsigned_abs(),
            if lon >= 0 { 'E' } else { 'W' },
            lon.unsigned_abs()
        );
        format!("{BASE}/{name}/{name}.tif")
    }

    /// The tile over (`lon`, `lat`) opened, or none where the model has no file (the bucket says 403 or 404 for what is not there).
    fn open(&self, lon: i32, lat: i32) -> Result<Option<(Tile<'_>, Arc<AtomicUsize>)>, String> {
        let url = Self::url(lon, lat);
        let name = url.rsplit('/').next().unwrap_or("tile").to_string();
        let mut remote = Remote::new(&*self.net, &self.cache, url);
        // The first block says whether there is a file, and holds the head of it, where the layout is.
        let head = remote.block(0).map(|b| !b.is_empty());
        let first = match head {
            Ok(has) => has,
            Err(_) if matches!(remote.status, Some(403 | 404)) => return Ok(None),
            Err(e) => return Err(format!("Copernicus tile {name}: {e}")),
        };
        if !first {
            return Ok(None);
        }
        let fetched = remote.fetched.clone();
        let wrong = |what: &str| format!("Copernicus tile {name}: {what}");
        let mut limits = Limits::default();
        (limits.decoding_buffer_size, limits.ifd_value_size, limits.intermediate_buffer_size) = (64 << 20, 1 << 20, 16 << 20);
        let mut decoder =
            Decoder::new(remote).map_err(|e| wrong(&format!("it is not a GeoTIFF BIMCraft can read: {e}")))?.with_limits(limits);
        let (width, height) = decoder.dimensions().map_err(|e| wrong(&e.to_string()))?;
        if width < 2 || height < 2 || u64::from(width) * u64::from(height) > MAX_PIXELS {
            return Err(wrong(&format!("a one-degree tile is not {width} x {height} pixels")));
        }
        let scale = decoder.get_tag_f64_vec(Tag::ModelPixelScaleTag).map_err(|_| wrong("it has no place on the Earth (no pixel scale)"))?;
        let tie = decoder.get_tag_f64_vec(Tag::ModelTiepointTag).map_err(|_| wrong("it has no place on the Earth (no tie point)"))?;
        let (&[dx, dy, ..], &[i, j, _, x, y, ..]) = (scale.as_slice(), tie.as_slice()) else {
            return Err(wrong("its place on the Earth is not written as a GeoTIFF's is"));
        };
        if ![dx, dy, i, j, x, y].iter().all(|v| v.is_finite()) || dx <= 0.0 || dy <= 0.0 || dx > 1.0 || dy > 1.0 {
            return Err(wrong("its pixel size and tie point are not numbers a tile has"));
        }
        let (chunk_w, chunk_h) = decoder.chunk_dimensions();
        if chunk_w == 0 || chunk_h == 0 {
            return Err(wrong("it is cut into chunks of no size"));
        }
        let tile = Tile {
            decoder,
            west: x - i * dx,
            north: y + j * dy,
            dx,
            dy,
            width,
            height,
            chunk_w,
            chunk_h,
            across: width.div_ceil(chunk_w),
            chunks: HashMap::new(),
        };
        Ok(Some((tile, fetched)))
    }

    /// A grid of heights over the box, from the one-degree tiles it spans (a tile the model lacks leaves its part of the grid
    /// without heights), and whether every block came from the cache.
    pub fn grid(&self, bbox: &Bbox, spacing: f64) -> Result<(ElevationGrid, bool), String> {
        let layout = Layout::new(bbox, spacing)?;
        let extent = layout.extent(bbox)?;
        let (lon0, lon1) = (extent.west.floor() as i32, (extent.east.ceil() as i32 - 1).max(extent.west.floor() as i32));
        let (lat0, lat1) = (extent.south.floor() as i32, (extent.north.ceil() as i32 - 1).max(extent.south.floor() as i32));
        if ((lon1 - lon0 + 1) * (lat1 - lat0 + 1)) as usize > MAX_FILES {
            return Err(format!("the box spans more than {MAX_FILES} Copernicus tiles: use a smaller box"));
        }
        let mut tiles: HashMap<(i32, i32), Tile> = HashMap::new();
        let mut counters = Vec::new();
        for lat in lat0..=lat1 {
            for lon in lon0..=lon1 {
                if let Some((tile, fetched)) = self.open(lon, lat)? {
                    tiles.insert((lon, lat), tile);
                    counters.push(fetched);
                }
            }
        }
        if tiles.is_empty() {
            return Err("there is no Copernicus tile over this box (the model has none over open water): try the other source".into());
        }
        let mut values = Vec::with_capacity(layout.rows * layout.cols);
        for r in 0..layout.rows {
            for c in 0..layout.cols {
                let (lon, lat) = layout.at(bbox, c, r);
                let home = (lon.floor() as i32, lat.floor() as i32);
                let h = match tiles.get_mut(&home) {
                    Some(t) => t
                        .sample(lon, lat)
                        .map_err(|e| format!("Copernicus tile {}: {e}", Self::url(home.0, home.1).rsplit('/').next().unwrap_or("")))?,
                    None => None,
                };
                values.push(h.unwrap_or(f32::NAN));
            }
        }
        // The fetches counted after the sampling too: chunks are read as they are needed.
        let cached = counters.iter().all(|n| n.load(Ordering::Relaxed) == 0);
        Ok((ElevationGrid::new(bbox.west, bbox.north, layout.dlon, layout.dlat, layout.cols, layout.rows, values)?, cached))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Fake, Scratch};
    use std::io::Write;

    const HOME: (f64, f64) = (-46.6333, -23.5505);

    /// The ground the files hold: 760 m at home, 1 m more each 0.001 degrees east, 2 m each 0.001 north.
    fn ground(lon: f64, lat: f64) -> f64 {
        760.0 + 1000.0 * (lon - HOME.0) + 2000.0 * (lat - HOME.1)
    }

    /// What a test file is like, and what can be wrong with it.
    struct Spec {
        width: u32,
        height: u32,
        tile: u32,
        /// Pixel size, degrees.
        pixel: f64,
        west: f64,
        north: f64,
        /// SampleFormat: 3 float, 1 unsigned.
        format: u16,
        compression: u16,
        geo: bool,
    }

    impl Spec {
        /// The one-degree tile of São Paulo's lon -47..-46, lat -24..-23, 0.002 degrees a pixel: 500 pixels, in 128 pixel tiles
        /// (so the last tile is cut short).
        fn sao_paulo() -> Spec {
            Spec { width: 500, height: 500, tile: 128, pixel: 0.002, west: -47.0, north: -23.0, format: 3, compression: 8, geo: true }
        }
    }

    /// A TIFF: tiled, deflate, predictor 3 for floats, with GeoTIFF's scale and tie point; each pixel `value(lon, lat)` at its centre.
    fn tiff(s: &Spec, value: impl Fn(f64, f64) -> f32) -> Vec<u8> {
        let (across, down) = (s.width.div_ceil(s.tile), s.height.div_ceil(s.tile));
        let mut tiles: Vec<Vec<u8>> = Vec::new();
        for ty in 0..down {
            for tx in 0..across {
                let mut raw = Vec::new();
                for r in 0..s.tile {
                    let row: Vec<f32> = (0..s.tile)
                        .map(|c| {
                            let (x, y) = (tx * s.tile + c, ty * s.tile + r);
                            if x < s.width && y < s.height {
                                value(s.west + (f64::from(x) + 0.5) * s.pixel, s.north - (f64::from(y) + 0.5) * s.pixel)
                            } else {
                                0.0
                            }
                        })
                        .collect();
                    if s.format == 3 {
                        // The floating-point predictor: the row's bytes grouped by significance (most first), then each byte less the one before.
                        let n = row.len();
                        let mut shuffled = vec![0u8; n * 4];
                        for (i, v) in row.iter().enumerate() {
                            for (j, b) in v.to_be_bytes().iter().enumerate() {
                                shuffled[j * n + i] = *b;
                            }
                        }
                        for k in (1..shuffled.len()).rev() {
                            shuffled[k] = shuffled[k].wrapping_sub(shuffled[k - 1]);
                        }
                        raw.extend(shuffled);
                    } else {
                        raw.extend(row.iter().flat_map(|v| (*v as u32).to_le_bytes()));
                    }
                }
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(&raw).unwrap();
                tiles.push(z.finish().unwrap());
            }
        }
        // The layout: header, the IFD, what its entries point to, then the tiles.
        let mut entries: Vec<(u16, u16, u32, Vec<u8>)> = Vec::new();
        let short = |v: u16| v.to_le_bytes().to_vec();
        let long = |v: u32| v.to_le_bytes().to_vec();
        entries.push((256, 4, 1, long(s.width)));
        entries.push((257, 4, 1, long(s.height)));
        entries.push((258, 3, 1, short(32)));
        entries.push((259, 3, 1, short(s.compression)));
        entries.push((262, 3, 1, short(1)));
        entries.push((277, 3, 1, short(1)));
        entries.push((284, 3, 1, short(1)));
        if s.format == 3 {
            entries.push((317, 3, 1, short(3)));
        }
        entries.push((322, 4, 1, long(s.tile)));
        entries.push((323, 4, 1, long(s.tile)));
        let n = tiles.len() as u32;
        entries.push((324, 4, n, Vec::new()));
        entries.push((325, 4, n, tiles.iter().flat_map(|t| (t.len() as u32).to_le_bytes()).collect()));
        entries.push((339, 3, 1, short(s.format)));
        if s.geo {
            entries.push((33550, 12, 3, [s.pixel, s.pixel, 0.0].iter().flat_map(|d| d.to_le_bytes()).collect()));
            entries.push((33922, 12, 6, [0.0, 0.0, 0.0, s.west, s.north, 0.0].iter().flat_map(|d| d.to_le_bytes()).collect()));
        }
        let ifd_len = 2 + entries.len() * 12 + 4;
        let mut extra_at = 8 + ifd_len;
        // Which entries keep their values after the IFD (more than four bytes), and where.
        let mut spots = Vec::new();
        for (_, _, _, v) in &entries {
            if v.len() > 4 || (v.is_empty()) {
                spots.push(Some(extra_at));
                extra_at += v.len().max(4 * n as usize * usize::from(v.is_empty()));
            } else {
                spots.push(None);
            }
        }
        let tiles_at = extra_at;
        let mut offsets = Vec::new();
        let mut at = tiles_at as u32;
        for t in &tiles {
            offsets.extend(at.to_le_bytes());
            at += t.len() as u32;
        }
        let mut out = vec![b'I', b'I', 42, 0, 8, 0, 0, 0];
        out.extend((entries.len() as u16).to_le_bytes());
        for ((tag, kind, count, v), spot) in entries.iter().zip(&spots) {
            out.extend(tag.to_le_bytes());
            out.extend(kind.to_le_bytes());
            out.extend(count.to_le_bytes());
            match spot {
                Some(at) => out.extend((*at as u32).to_le_bytes()),
                None => {
                    let mut four = v.clone();
                    four.resize(4, 0);
                    out.extend(four);
                }
            }
        }
        out.extend([0, 0, 0, 0]);
        for ((_, _, _, v), spot) in entries.iter().zip(&spots) {
            if spot.is_some() {
                out.extend(if v.is_empty() { offsets.clone() } else { v.clone() });
            }
        }
        for t in &tiles {
            out.extend(t);
        }
        out
    }

    fn block() -> Bbox {
        Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap()
    }

    fn net_with(file: &[u8]) -> Arc<Fake> {
        Arc::new(Fake::new().get(&Copernicus::url(-47, -24), file))
    }

    fn near(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn each_tile_is_named_for_its_south_west_corner() {
        let name = |lon, lat| Copernicus::url(lon, lat);
        let one = |n: &str| format!("{BASE}/Copernicus_DSM_COG_10_{n}_00_DEM/Copernicus_DSM_COG_10_{n}_00_DEM.tif");
        assert_eq!(name(-47, -24), one("S24_00_W047"));
        assert_eq!(name(0, 0), one("N00_00_E000"));
        assert_eq!(name(6, -1), one("S01_00_E006"));
        assert_eq!(name(-122, 37), one("N37_00_W122"));
        assert_eq!(name(179, 85), one("N85_00_E179"));
    }

    #[test]
    fn a_box_is_read_from_a_tiled_float_geotiff_and_is_the_ground() {
        let file = tiff(&Spec::sao_paulo(), |lon, lat| ground(lon, lat) as f32);
        let net = net_with(&file);
        let dir = Scratch::new("copernicus");
        let c = Copernicus::new(net.clone(), Arc::new(Cache::new(Some(dir.0.clone()))));
        let b = block();
        let (g, cached) = c.grid(&b, 30.0).unwrap();
        assert!(!cached);
        for (lon, lat) in
            [HOME, (b.west + 0.0004, b.south + 0.0004), (b.east - 0.0004, b.north - 0.0004), (-46.64, -23.55), (-46.62, -23.55)]
        {
            let z = g.sample(lon, lat).unwrap_or_else(|| panic!("no height at {lon} {lat}"));
            assert!(near(z, ground(lon, lat), 0.02), "{lon} {lat}: {z} vs {}", ground(lon, lat));
        }
        // Only ranges of the file were asked for, never the whole of it at once, and again it is the cache's.
        let asked = net.asked();
        assert!(
            !asked.is_empty() && asked.iter().all(|(m, u, body)| m == "GET" && u.contains("S24_00_W047") && body.starts_with(b"bytes=")),
            "{asked:?}"
        );
        let n = asked.len();
        let (again, cached) = c.grid(&b, 30.0).unwrap();
        assert!(cached && again == g);
        assert_eq!(net.asked().len(), n, "no more requests");
    }

    #[test]
    fn a_box_over_two_tiles_takes_the_ground_from_both_and_one_that_is_missing_leaves_a_hole() {
        // Two files side by side: lon -47..-46 and -46..-45 (so the box spans -46.01..-45.99), the ground one function.
        let west = Spec::sao_paulo();
        let east = Spec { west: -46.0, ..Spec::sao_paulo() };
        let f = |lon: f64, lat: f64| (500.0 + 300.0 * (lon + 46.0) + 800.0 * (lat + 23.5)) as f32;
        let seam = Bbox::new(-23.51, -46.01, -23.49, -45.99).unwrap();
        let both = Arc::new(Fake::new().get(&Copernicus::url(-47, -24), &tiff(&west, f)).get(&Copernicus::url(-46, -24), &tiff(&east, f)));
        let c = Copernicus::new(both, Arc::new(Cache::new(None)));
        let (g, _) = c.grid(&seam, 30.0).unwrap();
        for lon in [-46.009, -46.0, -45.991] {
            let z = g.sample(lon, -23.5).unwrap_or_else(|| panic!("no height at {lon}"));
            // Exactly on the seam the nearest pixel stands for the missing one: half a pixel of slope (0.3 m on these big pixels).
            let tolerance = if lon == -46.0 { 0.35 } else { 0.05 };
            assert!(near(z, f64::from(f(lon, -23.5)), tolerance), "{lon}: {z}");
        }
        let only_west = Arc::new(Fake::new().get(&Copernicus::url(-47, -24), &tiff(&west, f)));
        let c = Copernicus::new(only_west, Arc::new(Cache::new(None)));
        let (g, _) = c.grid(&seam, 30.0).unwrap();
        assert!(g.sample(-46.005, -23.5).is_some() && g.sample(-45.995, -23.5).is_none(), "no data east of the seam");
        // None at all: the words say so.
        let c = Copernicus::new(Arc::new(Fake::new()), Arc::new(Cache::new(None)));
        assert!(c.grid(&block(), 30.0).unwrap_err().contains("no Copernicus"));
    }

    #[test]
    fn a_file_that_is_not_what_it_should_be_is_refused_whole() {
        let b = block();
        let good = tiff(&Spec::sao_paulo(), |lon, lat| ground(lon, lat) as f32);
        let huge = Spec { width: 200_000, height: 200_000, ..Spec::sao_paulo() };
        for (what, file) in [
            ("html", b"<html>Access Denied</html>".to_vec()),
            ("empty", Vec::new()),
            ("cut short", good[..good.len() / 2].to_vec()),
            ("the header only", good[..8].to_vec()),
            ("integers", tiff(&Spec { format: 1, ..Spec::sao_paulo() }, |lon, lat| ground(lon, lat) as f32)),
            ("another compression", tiff(&Spec { compression: 5, ..Spec::sao_paulo() }, |lon, lat| ground(lon, lat) as f32)),
            ("no place on the Earth", tiff(&Spec { geo: false, ..Spec::sao_paulo() }, |lon, lat| ground(lon, lat) as f32)),
        ] {
            let c = Copernicus::new(net_with(&file), Arc::new(Cache::new(None)));
            let e = c.grid(&b, 30.0).unwrap_err();
            assert!(!e.is_empty() && e.contains("S24_00_W047") || e.contains("Copernicus"), "{what}: {e}");
        }
        // A header that claims a picture of 40 billion pixels is refused from the header: the tiles of it are not even in the file.
        let mut claim = tiff(&Spec { width: 500, height: 500, ..Spec::sao_paulo() }, |_, _| 0.0);
        claim[18..22].copy_from_slice(&200_000u32.to_le_bytes());
        claim[30..34].copy_from_slice(&200_000u32.to_le_bytes());
        let c = Copernicus::new(net_with(&claim), Arc::new(Cache::new(None)));
        assert!(c.grid(&b, 30.0).is_err());
        let _ = huge;
        // A spacing that cannot be.
        let c = Copernicus::new(net_with(&good), Arc::new(Cache::new(None)));
        for bad in [0.0, -1.0, f64::NAN] {
            assert!(c.grid(&b, bad).is_err());
        }
        assert!(c.grid(&b, 0.05).unwrap_err().contains("spacing"));
    }

    #[test]
    fn a_remote_file_reads_like_a_local_one_in_blocks() {
        let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
        let net = Arc::new(Fake::new().get("https://copernicus-dem-30m.s3.amazonaws.com/f.tif", &data));
        let cache = Cache::new(None);
        let mut r = Remote::new(&*net, &cache, "https://copernicus-dem-30m.s3.amazonaws.com/f.tif".into());
        let mut four = [0u8; 4];
        r.read_exact(&mut four).unwrap();
        assert_eq!(four, data[..4]);
        r.seek(SeekFrom::Start(300_000)).unwrap();
        let mut some = vec![0u8; 5000];
        r.read_exact(&mut some).unwrap();
        assert_eq!(some, data[300_000..305_000]);
        r.seek(SeekFrom::Current(-2500)).unwrap();
        r.read_exact(&mut four).unwrap();
        assert_eq!(four, data[302_500..302_504]);
        // Reading on to the end gets what there is and then says so.
        r.seek(SeekFrom::Start(699_990)).unwrap();
        let mut tail = Vec::new();
        r.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, data[699_990..]);
        assert_eq!(r.read(&mut four).unwrap(), 0);
        r.seek(SeekFrom::Start(5_000_000)).unwrap();
        assert_eq!(r.read(&mut four).unwrap(), 0, "past the end is the end");
        assert_eq!(r.seek(SeekFrom::End(-1)).unwrap(), 699_999, "the size is known once a block has ended short");
        assert!(r.seek(SeekFrom::Current(-10_000_000)).is_err());
        let mut fresh = Remote::new(&*net, &cache, "https://copernicus-dem-30m.s3.amazonaws.com/f.tif".into());
        assert!(fresh.seek(SeekFrom::End(-1)).is_err(), "the size is not known beforehand");
        // 3 blocks only: the start, where 300,000 is, and the end.
        let blocks = net.asked().len();
        assert!(blocks <= 6, "{blocks}");
    }
}
