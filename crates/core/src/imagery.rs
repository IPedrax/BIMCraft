//! Aerial imagery for the terrain, pure: where it can come from (a table of sources with their terms), which zoom and tiles a box
//! needs, the mosaic of tiles an app fetched, and the warp that lays the mosaic into the project's own axes, so the 3D view and the
//! plans can draw it by the ground's x and y alone. The core never touches the network: `GeoSource::imagery` is the hook an app
//! attaches, and what it returns is hostile, as a file is, so a mosaic is checked before it is used.

use crate::geo::{Bbox, tile_xy};
use crate::geom::P2;

/// The side of a tile, px. Every service in the table gives 256 px tiles.
pub const TILE: usize = 256;
/// The finest zoom an address is made for: 2^24 columns and rows.
pub const MAX_ZOOM: u8 = 24;
/// The longest address template taken, characters.
pub const MAX_TEMPLATE: usize = 2048;
/// The most tiles one fetch may need.
pub const MAX_TILES: usize = 100;
/// The fewest and the most pixels along the long side of the picture a fetch is asked for.
pub const MIN_SIDE: u32 = 64;
pub const MAX_SIDE: u32 = 2048;
/// The pixels along the long side of the picture when none is asked for.
pub const DEFAULT_SIDE: u32 = 1024;
/// The ground a pixel of a 256-pixel tile at zoom 0 stands for at the equator, metres.
const METRES_AT_ZOOM_0: f64 = 156_543.033_928_040_97;
/// The cells along each side of the lattice a warp maps: 33 points by 33.
const LATTICE: usize = 32;
/// The points along each side of the box [`envelope`] walks.
const WALK: usize = 8;
/// The most ground a picture may be across, metres.
const MAX_GROUND: f64 = 1e6;

/// A service imagery can come from. Only services whose terms allow fetching their tiles into a project are in the table; the
/// terms are the service's own and are to be read at `terms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub id: &'static str,
    pub name: &'static str,
    /// The address of a tile, with `{z}`, `{x}` and `{y}` in it (and `{-y}` where a service counts rows from the south).
    pub template: &'static str,
    /// The zooms it has tiles at.
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// Where it has imagery, as south, west, north, east in degrees: a rectangle, so a place in it may still have none, which the
    /// tiles say (an error status, or a tile of one flat colour).
    pub bounds: [f64; 4],
    /// The words that go with the picture wherever it is shown.
    pub credit: &'static str,
    /// Where the terms of use are.
    pub terms: &'static str,
}

/// The sources, the finest and most local first, so that the first one that has a place's imagery is the best there is.
pub static SOURCES: [Source; 4] = [
    Source {
        id: "flanders",
        name: "Flanders orthophoto mosaic (Digitaal Vlaanderen)",
        template: "https://geo.api.vlaanderen.be/OMWRGBMRVL/wmts?service=WMTS&request=GetTile&version=1.0.0&layer=omwrgbmrvl&style=default&tilematrixset=GoogleMapsVL&tilematrix={z}&tilerow={y}&tilecol={x}&format=image/png",
        min_zoom: 0,
        max_zoom: 21,
        bounds: [50.67, 2.50, 51.52, 5.95],
        credit: "Orthofotomozaiek, middenschalig, winteropnamen, kleur, meest recent: Digitaal Vlaanderen",
        terms: "https://www.vlaanderen.be/digitaal-vlaanderen/onze-oplossingen/geografische-webdiensten/gebruiksrecht-en-privacyverklaring-geografische-webdiensten",
    },
    Source {
        id: "pdok",
        name: "Aerial photographs, most recent (PDOK)",
        template: "https://service.pdok.nl/hwh/luchtfotorgb/wmts/v1_0/Actueel_orthoHR/EPSG:3857/{z}/{x}/{y}.jpeg",
        min_zoom: 0,
        max_zoom: 21,
        bounds: [50.70, 3.20, 53.70, 7.30],
        credit: "Luchtfoto Actueel_orthoHR: Landelijke Voorziening Beeldmateriaal, PDOK, CC BY 4.0",
        terms: "https://www.pdok.nl/",
    },
    Source {
        id: "usgs",
        name: "USGS imagery",
        template: "https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}",
        min_zoom: 0,
        max_zoom: 16,
        bounds: [24.0, -125.0, 50.0, -66.0],
        credit: "Imagery courtesy of the U.S. Geological Survey, The National Map",
        terms: "https://www.usgs.gov/centers/eros/data-citation",
    },
    Source {
        id: "eox2016",
        name: "Sentinel-2 cloudless 2016 (EOX)",
        template: "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/GoogleMapsCompatible/{z}/{y}/{x}.jpg",
        min_zoom: 0,
        max_zoom: 14,
        bounds: [-85.0, -180.0, 85.0, 180.0],
        credit: "EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016), CC BY 4.0",
        terms: "https://cloudless.eox.at",
    },
];

/// The source called `id`.
pub fn source(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id)
}

/// The sources that have the middle of `b`, in the table's order: the finest first, and the worldwide one last.
pub fn candidates(b: &Bbox) -> Vec<&'static Source> {
    let (lon, lat) = b.centre();
    SOURCES.iter().filter(|s| (s.bounds[1]..=s.bounds[3]).contains(&lon) && (s.bounds[0]..=s.bounds[2]).contains(&lat)).collect()
}

impl Source {
    /// The address of the tile `x`, `y` at `zoom`, if the source has tiles at that zoom and the map has such a tile.
    pub fn url(&self, zoom: u8, x: u32, y: u32) -> Result<String, String> {
        if !(self.min_zoom..=self.max_zoom).contains(&zoom) {
            return Err(format!("{} has tiles at zooms {} to {}, not {zoom}", self.name, self.min_zoom, self.max_zoom));
        }
        expand(self.template, zoom, x, y)
    }
}

/// The address of a tile: `template` with `{z}`, `{x}` and `{y}` (or `{-y}`, rows from the south) put in.
///
/// WMTS's own names for them (`{TileMatrix}`, `{TileCol}`, `{TileRow}`) and `{zoom}` are taken too, so an address copied from a
/// capabilities document works. Anything else in braces (`{Style}`, `{TileMatrixSet}`) is refused by name, to be written in by hand.
pub fn expand(template: &str, zoom: u8, x: u32, y: u32) -> Result<String, String> {
    if template.len() > MAX_TEMPLATE {
        return Err(format!("the address is {} characters; up to {MAX_TEMPLATE} are taken", template.len()));
    }
    if zoom > MAX_ZOOM {
        return Err(format!("zoom {zoom} is finer than the {MAX_ZOOM} an address is made for"));
    }
    let n = 1u64 << zoom;
    if u64::from(x) >= n || u64::from(y) >= n {
        return Err(format!("there is no tile {x}/{y} at zoom {zoom}: the map has {n} columns and rows there"));
    }
    let (mut out, mut rest) = (String::new(), template);
    let (mut has_zoom, mut has_column, mut has_row) = (false, false, false);
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at..).unwrap_or_default();
        if tail.starts_with('}') {
            return Err("the address has a } with no { before it".into());
        }
        let Some(close) = tail.find('}') else { return Err("the address has a { with no } after it".into()) };
        match tail.get(1..close).unwrap_or_default() {
            "z" | "zoom" | "TileMatrix" => {
                out.push_str(&zoom.to_string());
                has_zoom = true;
            }
            "x" | "TileCol" => {
                out.push_str(&x.to_string());
                has_column = true;
            }
            "y" | "TileRow" => {
                out.push_str(&y.to_string());
                has_row = true;
            }
            "-y" => {
                out.push_str(&(n - 1 - u64::from(y)).to_string());
                has_row = true;
            }
            other => {
                return Err(format!(
                    "the address has {{{other}}}, which is none of {{z}}, {{x}}, {{y}} and {{-y}}: write that part in by hand"
                ));
            }
        }
        rest = tail.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    if !(has_zoom && has_column && has_row) {
        return Err("an address needs {z}, {x} and {y} (or {-y}) in it".into());
    }
    Ok(out)
}

/// What a fetch is asked for: the zoom, the tiles that cover the box, and the size of the picture they are warped to.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub zoom: u8,
    /// The column and row of the top left tile, and how many columns and rows there are.
    pub x0: u32,
    pub y0: u32,
    pub cols: u32,
    pub rows: u32,
    /// The ground a pixel of the tiles stands for at the middle of the box, metres.
    pub metres_per_pixel: f64,
    /// The pixels of the picture the mosaic is warped to.
    pub width: u32,
    pub height: u32,
}

/// The column, row, columns and rows of the tiles at `zoom` that cover the box.
fn tile_range(b: &Bbox, zoom: u8) -> (u32, u32, u32, u32) {
    let last = 2f64.powi(i32::from(zoom)) - 1.0;
    let cell = |v: f64| v.floor().clamp(0.0, last) as u32;
    let ((x0, y0), (x1, y1)) = (tile_xy(b.west, b.north, zoom), tile_xy(b.east, b.south, zoom));
    let (x0, y0, x1, y1) = (cell(x0), cell(y0), cell(x1), cell(y1));
    (x0, y0, x1.saturating_sub(x0) + 1, y1.saturating_sub(y0) + 1)
}

/// The plan to cover `envelope` (the longitudes and latitudes of the ground to be pictured) from `source`, for a picture of
/// `width_m` by `height_m` metres with `max_side` pixels along its longer side: the finest zoom whose pixels are no smaller than the
/// picture's, so nothing is made blurrier than it is, within the source's zooms and the tiles one fetch may take.
pub fn plan(source: &Source, envelope: &Bbox, width_m: f64, height_m: f64, max_side: u32) -> Result<Plan, String> {
    if !(MIN_SIDE..=MAX_SIDE).contains(&max_side) {
        return Err(format!("`max_side` is the pixels along the long side of the picture, {MIN_SIDE} to {MAX_SIDE}, not {max_side}"));
    }
    let size_ok = |m: f64| m.is_finite() && m > 0.0 && m <= MAX_GROUND;
    if !size_ok(width_m) || !size_ok(height_m) {
        return Err(format!("the ground to picture is above 0 and up to {MAX_GROUND} metres each way, not {width_m} by {height_m}"));
    }
    let cos = envelope.centre().1.to_radians().cos().max(1e-3);
    // The ground a pixel of the picture stands for, if the long side has all its pixels; and the finest zoom whose pixels are no smaller.
    let wanted = width_m.max(height_m) / f64::from(max_side);
    let finest = (METRES_AT_ZOOM_0 * cos / wanted).log2().floor();
    let least = source.min_zoom.min(MAX_ZOOM);
    let mut zoom = finest.clamp(f64::from(least), f64::from(source.max_zoom.min(MAX_ZOOM))) as u8;
    loop {
        let (x0, y0, cols, rows) = tile_range(envelope, zoom);
        if u64::from(cols) * u64::from(rows) <= MAX_TILES as u64 {
            let metres_per_pixel = METRES_AT_ZOOM_0 * cos / f64::from(1u32 << zoom);
            let each = wanted.max(metres_per_pixel);
            let pixels = |metres: f64| ((metres / each).ceil() as u32).clamp(1, max_side);
            return Ok(Plan { zoom, x0, y0, cols, rows, metres_per_pixel, width: pixels(width_m), height: pixels(height_m) });
        }
        if zoom <= least {
            return Err(format!(
                "the ground needs {cols} by {rows} tiles even at {}'s coarsest zoom, {least}; at most {MAX_TILES}: use a smaller area",
                source.name
            ));
        }
        zoom -= 1;
    }
}

/// What an imagery fetch brings: the source it came from (`SOURCES`), the plan it was asked for, the mosaic, and whether every tile
/// was kept from an earlier fetch.
#[derive(Clone, Debug, PartialEq)]
pub struct Imagery {
    pub source: String,
    pub plan: Plan,
    pub mosaic: Mosaic,
    pub cached: bool,
}

/// The tiles of a fetch, laid side by side: each is `TILE` by `TILE` pixels of RGB, row after row, and the tiles follow one another
/// row after row from the top left.
#[derive(Clone, Debug, PartialEq)]
pub struct Mosaic {
    pub zoom: u8,
    pub x0: u32,
    pub y0: u32,
    pub cols: u32,
    pub rows: u32,
    pub rgb: Vec<u8>,
    /// One for each tile, row after row: whether it has imagery (an error status, or a tile of one flat colour, has none).
    pub has: Vec<bool>,
}

impl Mosaic {
    /// A mosaic for `plan` with nothing in it. A plan with more tiles than a fetch may take gives one that fails its check.
    pub fn new(plan: &Plan) -> Mosaic {
        let tiles = (u64::from(plan.cols) * u64::from(plan.rows)).min(MAX_TILES as u64) as usize;
        Mosaic {
            zoom: plan.zoom,
            x0: plan.x0,
            y0: plan.y0,
            cols: plan.cols,
            rows: plan.rows,
            rgb: vec![0; tiles * TILE * TILE * 3],
            has: vec![false; tiles],
        }
    }

    /// Puts the tile at `col`, `row` (counted from the top left one) in: `TILE` by `TILE` pixels of RGB.
    pub fn put(&mut self, col: u32, row: u32, rgb: &[u8]) -> Result<(), String> {
        let bytes = TILE * TILE * 3;
        if col >= self.cols || row >= self.rows {
            return Err(format!("there is no tile {col}, {row} in a mosaic of {} by {}", self.cols, self.rows));
        }
        if rgb.len() != bytes {
            return Err(format!("a tile is {TILE} by {TILE} pixels of RGB, {bytes} bytes, not {}", rgb.len()));
        }
        self.check()?;
        let index = row as usize * self.cols as usize + col as usize;
        let to = self.rgb.get_mut(index * bytes..(index + 1) * bytes).ok_or("the tile is outside the mosaic")?;
        to.copy_from_slice(rgb);
        if let Some(has) = self.has.get_mut(index) {
            *has = true;
        }
        Ok(())
    }

    /// Whether it is sound: tiles of the map, as many as the pixels and the flags say, and not more than a fetch may take.
    pub fn check(&self) -> Result<(), String> {
        if self.zoom > MAX_ZOOM {
            return Err(format!("zoom {} is finer than the {MAX_ZOOM} the map is made for", self.zoom));
        }
        let tiles = u64::from(self.cols) * u64::from(self.rows);
        if self.cols == 0 || self.rows == 0 || tiles > MAX_TILES as u64 {
            return Err(format!("a mosaic is 1 to {MAX_TILES} tiles, not {} by {}", self.cols, self.rows));
        }
        let n = 1u64 << self.zoom;
        if u64::from(self.x0) + u64::from(self.cols) > n || u64::from(self.y0) + u64::from(self.rows) > n {
            return Err(format!(
                "tiles {} by {} from {}, {} are off the map, which has {n} columns and rows at zoom {}",
                self.cols, self.rows, self.x0, self.y0, self.zoom
            ));
        }
        if self.has.len() as u64 != tiles {
            return Err(format!("{} tiles are flagged for a mosaic of {tiles}", self.has.len()));
        }
        let bytes = tiles * (TILE * TILE * 3) as u64;
        if self.rgb.len() as u64 != bytes {
            return Err(format!("it has {} bytes of pixels, not the {bytes} of {tiles} tiles", self.rgb.len()));
        }
        Ok(())
    }

    /// The colour at `px`, `py` (pixels from the top left of the mosaic, a pixel's middle at half a pixel), blended between the four
    /// pixels round it. None outside the mosaic or where any of the four is in a tile with no imagery. The mosaic is checked.
    fn sample(&self, px: f64, py: f64) -> Option<[u8; 3]> {
        let (w, h) = (self.cols as usize * TILE, self.rows as usize * TILE);
        if !(0.0..=w as f64).contains(&px) || !(0.0..=h as f64).contains(&py) {
            return None;
        }
        let (fx, fy) = (px - 0.5, py - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |x: f64, y: f64| -> Option<[f64; 3]> {
            let (x, y) = ((x.max(0.0) as usize).min(w - 1), (y.max(0.0) as usize).min(h - 1));
            let tile = (y / TILE) * self.cols as usize + x / TILE;
            if !*self.has.get(tile)? {
                return None;
            }
            let from = tile * TILE * TILE * 3 + ((y % TILE) * TILE + x % TILE) * 3;
            let p = self.rgb.get(from..from + 3)?;
            Some([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])])
        };
        let (a, b, c, d) = (at(x0, y0)?, at(x0 + 1.0, y0)?, at(x0, y0 + 1.0)?, at(x0 + 1.0, y0 + 1.0)?);
        Some(std::array::from_fn(|k| ((a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty).round() as u8))
    }
}

/// Whether `extent` (west, south, east, north) is a box with numbers in it.
fn extent_ok(extent: [f64; 4]) -> Result<(), String> {
    let [west, south, east, north] = extent;
    if extent.iter().all(|v| v.is_finite()) && west < east && south < north {
        Ok(())
    } else {
        Err(format!("the extent is [west, south, east, north] in mm, west below east and south below north, not {extent:?}"))
    }
}

/// The ground to be pictured, as the box of longitudes and latitudes that holds it: `extent` is west, south, east, north in the
/// project's plane (mm), and `lonlat` takes a point of the plane to degrees. The box is walked, so a turned project is held.
pub fn envelope(extent: [f64; 4], lonlat: &dyn Fn(P2) -> Result<(f64, f64), String>) -> Result<Bbox, String> {
    extent_ok(extent)?;
    let [west, south, east, north] = extent;
    let (mut low, mut high) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for j in 0..=WALK {
        for i in 0..=WALK {
            let (fx, fy) = (i as f64 / WALK as f64, j as f64 / WALK as f64);
            let (lon, lat) = lonlat(P2(west + (east - west) * fx, south + (north - south) * fy))?;
            if !lon.is_finite() || !lat.is_finite() {
                return Err("a point of the ground cannot be mapped to degrees".into());
            }
            low = (low.0.min(lon), low.1.min(lat));
            high = (high.0.max(lon), high.1.max(lat));
        }
    }
    Bbox::new(low.1, low.0, high.1, high.0)
}

/// A picture in the project's axes: RGBA, the first row the north one, clear where there is no imagery.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The mosaic resampled to `width` by `height` pixels covering `extent` (west, south, east, north in the project's plane, mm).
/// `lonlat` takes a point of the plane to degrees. The map is Mercator and the plane is turned, so a lattice of points is mapped and
/// the pixels between them follow it by bilinear steps, which is exact for the turn and within millimetres for the curve.
pub fn warp(
    mosaic: &Mosaic,
    extent: [f64; 4],
    width: u32,
    height: u32,
    lonlat: &dyn Fn(P2) -> Result<(f64, f64), String>,
) -> Result<Raster, String> {
    mosaic.check()?;
    extent_ok(extent)?;
    let sides = 1..=MAX_SIDE;
    if !sides.contains(&width) || !sides.contains(&height) {
        return Err(format!("the picture is 1 to {MAX_SIDE} pixels each way, not {width} by {height}"));
    }
    let [west, south, east, north] = extent;
    // Where each point of the lattice over the picture is in the mosaic, px.
    let mut nodes = Vec::with_capacity((LATTICE + 1) * (LATTICE + 1));
    for j in 0..=LATTICE {
        for i in 0..=LATTICE {
            let (fx, fy) = (i as f64 / LATTICE as f64, j as f64 / LATTICE as f64);
            let (lon, lat) = lonlat(P2(west + (east - west) * fx, north - (north - south) * fy))?;
            let (tx, ty) = tile_xy(lon, lat, mosaic.zoom);
            let node = ((tx - f64::from(mosaic.x0)) * TILE as f64, (ty - f64::from(mosaic.y0)) * TILE as f64);
            if !(lon.is_finite() && lat.is_finite() && node.0.is_finite() && node.1.is_finite()) {
                return Err("a point of the ground cannot be put on the map".into());
            }
            nodes.push(node);
        }
    }
    let node = |i: usize, j: usize| nodes.get(j * (LATTICE + 1) + i).copied().unwrap_or((f64::NAN, f64::NAN));
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for v in 0..height as usize {
        let fj = (v as f64 + 0.5) / f64::from(height) * LATTICE as f64;
        let j = (fj.floor() as usize).min(LATTICE - 1);
        let tj = fj - j as f64;
        for u in 0..width as usize {
            let fi = (u as f64 + 0.5) / f64::from(width) * LATTICE as f64;
            let i = (fi.floor() as usize).min(LATTICE - 1);
            let ti = fi - i as f64;
            let (a, b, c, d) = (node(i, j), node(i + 1, j), node(i, j + 1), node(i + 1, j + 1));
            let lerp = |p: f64, q: f64, r: f64, s: f64| (p * (1.0 - ti) + q * ti) * (1.0 - tj) + (r * (1.0 - ti) + s * ti) * tj;
            let (px, py) = (lerp(a.0, b.0, c.0, d.0), lerp(a.1, b.1, c.1, d.1));
            if let Some([r, g, bl]) = mosaic.sample(px, py)
                && let Some(to) = rgba.get_mut((v * width as usize + u) * 4..(v * width as usize + u) * 4 + 4)
            {
                to.copy_from_slice(&[r, g, bl, 255]);
            }
        }
    }
    Ok(Raster { width, height, rgba })
}
