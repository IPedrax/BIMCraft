//! Where to fetch from, and what comes back: a bounding box on the Earth, the tiles of the Web Mercator map that cover it, the
//! Overpass query for its buildings, a grid of elevations, and the hook (`GeoSource`) an app attaches to do the fetching. All pure:
//! the core never touches the network, so `fetch_osm` and `fetch_terrain` ask the hook, and a build without one refuses them.
//! What an app's hook returns is hostile, as a file is: the commands check it.

use std::sync::Arc;

use crate::geom::P2;

use crate::imagery::{Imagery, Plan};

/// The most area one fetch may cover, km².
pub const MAX_FETCH_AREA_KM2: f64 = 25.0;
/// The half-width of the box a fetch takes round the project's origin when it is given none, metres.
pub const DEFAULT_FETCH_RADIUS_M: f64 = 1000.0;
/// The most map tiles one fetch may need.
pub const MAX_TILES: usize = 100;
/// The finest Web Mercator zoom the elevation tiles go to.
pub const MAX_ZOOM: u8 = 15;
/// The most nodes an elevation grid may have.
pub const MAX_GRID_NODES: usize = 4_000_000;
/// Earth's mean radius, km.
const RADIUS_KM: f64 = 6371.0088;
/// Metres in a degree of latitude, near enough.
const METRES_PER_DEGREE: f64 = 111_320.0;
/// The Web Mercator map ends at this latitude.
const MAX_MERCATOR_LAT: f64 = 85.051_128_779_8;
/// The metres a pixel is across at the equator at zoom 0, on 256 px tiles.
const EQUATOR_METRES_PER_PIXEL: f64 = 156_543.033_92;

/// A box on the Earth: south and north latitude, west and east longitude, degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bbox {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl Bbox {
    /// The box, in Overpass's order (south, west, north, east), or what is wrong with it.
    pub fn new(south: f64, west: f64, north: f64, east: f64) -> Result<Bbox, String> {
        let ok = [south, west, north, east].iter().all(|x| x.is_finite())
            && -90.0 <= south
            && south < north
            && north <= 90.0
            && -180.0 <= west
            && west < east
            && east <= 180.0;
        if ok {
            Ok(Bbox { south, west, north, east })
        } else {
            Err(format!(
                "`bbox` is [south, west, north, east] in degrees, south below north and west below east, not [{south}, {west}, {north}, {east}]"
            ))
        }
    }

    pub fn as_array(&self) -> [f64; 4] {
        [self.south, self.west, self.north, self.east]
    }

    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        (self.west..=self.east).contains(&lon) && (self.south..=self.north).contains(&lat)
    }

    /// The middle, as (longitude, latitude).
    pub fn centre(&self) -> (f64, f64) {
        ((self.west + self.east) / 2.0, (self.south + self.north) / 2.0)
    }

    /// Its area on a sphere, km².
    pub fn area_km2(&self) -> f64 {
        RADIUS_KM * RADIUS_KM * (self.east - self.west).to_radians() * (self.north.to_radians().sin() - self.south.to_radians().sin())
    }

    /// The box `radius` metres each way round a place.
    pub fn around(lon: f64, lat: f64, radius: f64) -> Result<Bbox, String> {
        if !radius.is_finite() || radius <= 0.0 || radius > 1e6 {
            return Err(format!("`radius` is a distance in metres above 0 and up to 1,000,000, not {radius}"));
        }
        let (dlat, cos) = (radius / METRES_PER_DEGREE, lat.to_radians().cos());
        if !lon.is_finite() || !lat.is_finite() || cos < 1e-6 {
            return Err(format!("a box cannot be put round ({lon}, {lat})"));
        }
        let dlon = radius / (METRES_PER_DEGREE * cos);
        Bbox::new(lat - dlat, lon - dlon, lat + dlat, lon + dlon)
            .map_err(|_| format!("a box of {radius} m round ({lon}, {lat}) runs off the map: use a smaller `radius`"))
    }

    /// Whether it is small enough to fetch.
    pub fn check_area(&self) -> Result<(), String> {
        let area = self.area_km2();
        if area <= MAX_FETCH_AREA_KM2 {
            Ok(())
        } else {
            Err(format!(
                "the box is {area:.1} km², more than the {MAX_FETCH_AREA_KM2} km² one fetch may cover: use a smaller `bbox` or `radius`"
            ))
        }
    }
}

/// A place's position on the Web Mercator map at `zoom`, in tiles: whole numbers are tile corners.
pub fn tile_xy(lon: f64, lat: f64, zoom: u8) -> (f64, f64) {
    let n = 2f64.powi(i32::from(zoom));
    let lat = lat.clamp(-MAX_MERCATOR_LAT, MAX_MERCATOR_LAT).to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n)
}

/// The longitude and latitude of a position on the map at `zoom`, in tiles.
pub fn tile_lonlat(x: f64, y: f64, zoom: u8) -> (f64, f64) {
    let n = 2f64.powi(i32::from(zoom));
    (x / n * 360.0 - 180.0, (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees())
}

/// The tiles (x, y) at `zoom` that cover the box, row after row from the north, or why there are too many.
pub fn tiles_for(b: &Bbox, zoom: u8) -> Result<Vec<(u32, u32)>, String> {
    let last = 2f64.powi(i32::from(zoom)) - 1.0;
    let cell = |v: f64| v.floor().clamp(0.0, last) as u32;
    let ((x0, y0), (x1, y1)) = (tile_xy(b.west, b.north, zoom), tile_xy(b.east, b.south, zoom));
    let (x0, y0, x1, y1) = (cell(x0), cell(y0), cell(x1), cell(y1));
    let count = u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1);
    if count > MAX_TILES as u64 {
        return Err(format!(
            "the box needs {count} map tiles at zoom {zoom}; at most {MAX_TILES}: use a smaller box or a larger `spacing`"
        ));
    }
    Ok((y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).collect())
}

/// The coarsest zoom whose ground resolution at the box is `metres_per_pixel` or finer, up to [`MAX_ZOOM`].
pub fn zoom_for(b: &Bbox, metres_per_pixel: f64) -> u8 {
    if !metres_per_pixel.is_finite() || metres_per_pixel <= 0.0 {
        return MAX_ZOOM;
    }
    let cos = b.centre().1.to_radians().cos().max(1e-3);
    let z = (EQUATOR_METRES_PER_PIXEL * cos / metres_per_pixel).log2().ceil();
    z.clamp(0.0, f64::from(MAX_ZOOM)) as u8
}

/// What to ask Overpass for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers {
    /// Parts of buildings too (and the columns that hold them up): a building is its parts, so by default.
    pub parts: bool,
    /// The entrances, doors and exits mapped as nodes.
    pub entrances: bool,
}

impl Default for Layers {
    fn default() -> Layers {
        Layers { parts: true, entrances: true }
    }
}

/// A degree as the query writes it: seven places at most, no trailing zeros, no exponent.
fn degrees(x: f64) -> String {
    let text = format!("{x:.7}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() { "0".into() } else { text.into() }
}

/// The Overpass QL that asks for the buildings in the box, with their geometry.
pub fn overpass_query(b: &Bbox, layers: Layers) -> String {
    let at = format!("({},{},{},{})", degrees(b.south), degrees(b.west), degrees(b.north), degrees(b.east));
    let mut q =
        format!("[out:json][timeout:60][maxsize:67108864];(way[\"building\"]{at};relation[\"building\"][\"type\"=\"multipolygon\"]{at};");
    if layers.parts {
        q.push_str(&format!(
            "way[\"building:part\"]{at};relation[\"building:part\"][\"type\"=\"multipolygon\"]{at};relation[\"type\"=\"building\"]{at};way[\"man_made\"=\"column\"]{at};"
        ));
    }
    if layers.entrances {
        q.push_str(&format!("node[\"entrance\"]{at};node[\"door\"]{at};"));
    }
    q.push_str(");out geom;");
    q
}

/// A regular grid of heights (metres) from `west` and `north`, a node every `dlon` east and `dlat` south, row after row from the north.
#[derive(Clone, Debug, PartialEq)]
pub struct ElevationGrid {
    pub west: f64,
    pub north: f64,
    pub dlon: f64,
    pub dlat: f64,
    pub cols: usize,
    pub rows: usize,
    /// NaN where there is none.
    pub values: Vec<f32>,
}

impl ElevationGrid {
    pub fn new(west: f64, north: f64, dlon: f64, dlat: f64, cols: usize, rows: usize, values: Vec<f32>) -> Result<ElevationGrid, String> {
        if ![west, north, dlon, dlat].iter().all(|x| x.is_finite()) || dlon <= 0.0 || dlat <= 0.0 {
            return Err("an elevation grid needs finite corners and steps above 0".into());
        }
        if cols < 2 || rows < 2 || cols.checked_mul(rows).is_none_or(|n| n > MAX_GRID_NODES) {
            return Err(format!("an elevation grid is 2 or more nodes each way and at most {MAX_GRID_NODES} in all, not {cols} x {rows}"));
        }
        if values.len() != cols * rows {
            return Err(format!("an elevation grid of {cols} x {rows} has {} heights, not {}", cols * rows, values.len()));
        }
        Ok(ElevationGrid { west, north, dlon, dlat, cols, rows, values })
    }

    /// Whether it is what [`ElevationGrid::new`] would have made: its fields are public, and what a fetcher returns is not trusted.
    pub fn check(&self) -> Result<(), String> {
        if ![self.west, self.north, self.dlon, self.dlat].iter().all(|x| x.is_finite()) || self.dlon <= 0.0 || self.dlat <= 0.0 {
            return Err("an elevation grid needs finite corners and steps above 0".into());
        }
        if self.cols < 2 || self.rows < 2 || self.cols.checked_mul(self.rows).is_none_or(|n| n > MAX_GRID_NODES) {
            return Err(format!(
                "an elevation grid is 2 or more nodes each way and at most {MAX_GRID_NODES} in all, not {} x {}",
                self.cols, self.rows
            ));
        }
        if self.values.len() != self.cols * self.rows {
            return Err(format!(
                "an elevation grid of {} x {} has {} heights, not {}",
                self.cols,
                self.rows,
                self.cols * self.rows,
                self.values.len()
            ));
        }
        Ok(())
    }

    /// The height at a place, from the four nodes round it: none off the grid or where one of them has none.
    pub fn sample(&self, lon: f64, lat: f64) -> Option<f64> {
        if !lon.is_finite() || !lat.is_finite() {
            return None;
        }
        let (fx, fy) = ((lon - self.west) / self.dlon, (self.north - lat) / self.dlat);
        // A place on the edge of the grid is on it, whatever the last bit of the division says.
        const EDGE: f64 = 1e-9;
        let (last_x, last_y) = ((self.cols - 1) as f64, (self.rows - 1) as f64);
        if !(-EDGE..=last_x + EDGE).contains(&fx) || !(-EDGE..=last_y + EDGE).contains(&fy) {
            return None;
        }
        let (fx, fy) = (fx.clamp(0.0, last_x), fy.clamp(0.0, last_y));
        let (i, j) = ((fx.floor() as usize).min(self.cols - 2), (fy.floor() as usize).min(self.rows - 2));
        let (tx, ty) = (fx - i as f64, fy - j as f64);
        let at = |c: usize, r: usize| f64::from(self.values[r * self.cols + c]);
        let (v00, v10, v01, v11) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
        let z = (v00 * (1.0 - tx) + v10 * tx) * (1.0 - ty) + (v01 * (1.0 - tx) + v11 * tx) * ty;
        [v00, v10, v01, v11].iter().all(|v| v.is_finite()).then_some(z)
    }
}

/// The nodes of a grid: where each is (lon, lat), row after row from the north, and how many columns and rows there are.
#[derive(Clone, Debug, PartialEq)]
pub struct Nodes {
    pub at: Vec<(f64, f64)>,
    pub cols: usize,
    pub rows: usize,
}

/// The nodes of a grid over the box, a node every `spacing` metres (lon, lat) from its north west corner, row after row, and how many
/// columns and rows; or why it would have more than `most` (or [`MAX_GRID_NODES`]).
pub fn nodes(b: &Bbox, spacing: f64, most: usize) -> Result<Nodes, String> {
    if !spacing.is_finite() || spacing <= 0.0 {
        return Err(format!("`spacing` is a distance in metres above 0, not {spacing}"));
    }
    let dlat = spacing / METRES_PER_DEGREE;
    let dlon = spacing / (METRES_PER_DEGREE * b.centre().1.to_radians().cos().max(1e-3));
    let (cols, rows) = (((b.east - b.west) / dlon).floor() + 1.0, ((b.north - b.south) / dlat).floor() + 1.0);
    let limit = most.min(MAX_GRID_NODES);
    if cols * rows > limit as f64 {
        return Err(format!(
            "a `spacing` of {spacing} m over this box would make {} nodes, more than the {limit} allowed: use a larger `spacing` or a smaller box",
            cols * rows
        ));
    }
    let (cols, rows) = (cols as usize, rows as usize);
    let at = (0..rows).flat_map(|j| (0..cols).map(move |i| (b.west + i as f64 * dlon, b.north - j as f64 * dlat))).collect();
    Ok(Nodes { at, cols, rows })
}

/// What an Overpass fetch brings.
#[derive(Clone, Debug)]
pub struct Reply {
    pub text: String,
    /// It was kept from an earlier fetch.
    pub cached: bool,
}

/// What an elevation fetch brings.
#[derive(Clone, Debug)]
pub struct Elevation {
    pub grid: ElevationGrid,
    /// Which source it came from: `terrarium` or `copernicus`.
    pub source: String,
    pub cached: bool,
}

/// A tile of the map as a picture: `side` x `side` pixels of RGBA, row after row from the top left.
#[derive(Clone, Debug)]
pub struct Tile {
    pub rgba: Vec<u8>,
    pub side: u32,
    /// It was kept from an earlier fetch.
    pub cached: bool,
}

/// The most a tile may be across, pixels: the map's own are 256.
pub const MAX_TILE_SIDE: u32 = 512;

impl Tile {
    /// Whether it is a picture of the size it says (what an app's hook returns is hostile, as a file is).
    pub fn check(&self) -> Result<(), String> {
        let side = self.side as usize;
        if self.side == 0 || self.side > MAX_TILE_SIDE || self.rgba.len() != side * side * 4 {
            return Err(format!(
                "a map tile is up to {MAX_TILE_SIDE} pixels across, in RGBA, not {} bytes for {} pixels across",
                self.rgba.len(),
                self.side
            ));
        }
        Ok(())
    }
}

/// What fetches for the core: an app attaches one (the desktop's, which talks to the network); a headless build has none.
pub trait GeoSource: Send + Sync {
    /// The Overpass reply to `query` for `bbox`.
    fn overpass(&self, bbox: &Bbox, query: &str) -> Result<Reply, String>;
    /// A grid of heights over `bbox` at about `spacing` metres, from `source` (`terrarium`, `copernicus`; none, the best there is).
    fn elevation(&self, bbox: &Bbox, spacing: f64, source: Option<&str>) -> Result<Elevation, String>;
    /// The credit that goes with the data: `osm`, or an elevation source's name; `tiles` is the map's.
    fn attribution(&self, what: &str) -> String;
    /// The picture of the map tile at `zoom`, column `x`, row `y`: what the map a place is picked on is drawn with. A fetcher with
    /// none says so.
    fn tile(&self, _zoom: u8, _x: u32, _y: u32) -> Result<Tile, String> {
        Err("this BIMCraft has no map tiles: give the place as latitude and longitude".into())
    }
    /// The aerial imagery of `plan` from the source called `source` (`imagery::SOURCES`), as a mosaic of tiles. A fetcher with none
    /// says so. What it returns is checked by the commands, as a file is.
    fn imagery(&self, _source: &str, _plan: &Plan) -> Result<Imagery, String> {
        Err("this BIMCraft has no aerial imagery to fetch".into())
    }
    /// How far the imagery fetch that is running has got, from another thread: the tiles done, and the tiles in all. None for a
    /// fetcher that does not say, or when none has begun.
    fn imagery_progress(&self) -> Option<(usize, usize)> {
        None
    }
}

/// A fetcher an app attached.
pub type Fetcher = Arc<dyn GeoSource>;

/// The spacing, metres, `fetch_terrain` uses when none is given: as fine as keeps the nodes to about 36,000, and no finer than 10 m.
pub fn default_spacing(b: &Bbox) -> f64 {
    (b.area_km2() * 1e6 / 36_000.0).sqrt().ceil().max(10.0)
}

/// What an app fetched on a thread of its own, held so the commands `fetch_osm` and `fetch_terrain` can be run on the main thread over it:
/// they ask this for the same box, query, spacing and source, and are answered at once, from memory. One validation path, one undo
/// step and one report, and the window never waits on the network. Anything else is refused, never fetched; the credits are the real
/// source's.
pub struct Prefetched {
    inner: Fetcher,
    bbox: Bbox,
    osm: Option<(String, Reply)>,
    heights: Option<(f64, Option<String>, Elevation)>,
    imagery: Option<Imagery>,
}

impl Prefetched {
    /// Nothing held yet, for `bbox`; `inner` names the credits.
    pub fn new(inner: Fetcher, bbox: Bbox) -> Prefetched {
        Prefetched { inner, bbox, osm: None, heights: None, imagery: None }
    }

    /// Holds the imagery fetched: it is handed back for the source and plan it was fetched for.
    pub fn with_imagery(mut self, got: Imagery) -> Prefetched {
        self.imagery = Some(got);
        self
    }

    /// Holds the Overpass `reply` to `query`.
    pub fn with_osm(mut self, query: String, reply: Reply) -> Prefetched {
        self.osm = Some((query, reply));
        self
    }

    /// Holds the heights fetched at `spacing` from `source` (as it was asked for).
    pub fn with_elevation(mut self, spacing: f64, source: Option<String>, got: Elevation) -> Prefetched {
        self.heights = Some((spacing, source, got));
        self
    }
}

impl GeoSource for Prefetched {
    fn overpass(&self, bbox: &Bbox, query: &str) -> Result<Reply, String> {
        match &self.osm {
            Some((q, reply)) if *bbox == self.bbox && q == query => Ok(reply.clone()),
            _ => Err("this is not what was fetched: ask again from the Site tab".into()),
        }
    }

    fn elevation(&self, bbox: &Bbox, spacing: f64, source: Option<&str>) -> Result<Elevation, String> {
        match &self.heights {
            Some((sp, src, got)) if *bbox == self.bbox && *sp == spacing && src.as_deref() == source => Ok(got.clone()),
            _ => Err("these are not the heights that were fetched: ask again from the Site tab".into()),
        }
    }

    fn attribution(&self, what: &str) -> String {
        self.inner.attribution(what)
    }

    fn tile(&self, zoom: u8, x: u32, y: u32) -> Result<Tile, String> {
        self.inner.tile(zoom, x, y)
    }

    fn imagery_progress(&self) -> Option<(usize, usize)> {
        self.inner.imagery_progress()
    }

    fn imagery(&self, source: &str, plan: &Plan) -> Result<Imagery, String> {
        match &self.imagery {
            Some(got) if got.source == source && got.plan == *plan => Ok(got.clone()),
            _ => Err("this is not the imagery that was fetched: ask again from the Site tab".into()),
        }
    }
}

/// The standard UTM zone of a place, with Norway's and Svalbard's exceptions.
pub fn zone_of(longitude: f64, latitude: f64) -> u8 {
    if !longitude.is_finite() || !latitude.is_finite() {
        return 31;
    }
    let mut zone = (((longitude + 180.0) / 6.0).floor() as i64 + 1).clamp(1, 60) as u8;
    if (56.0..64.0).contains(&latitude) && (3.0..12.0).contains(&longitude) {
        zone = 32;
    }
    if (72.0..84.0).contains(&latitude) {
        zone = match longitude {
            l if (0.0..9.0).contains(&l) => 31,
            l if (9.0..21.0).contains(&l) => 33,
            l if (21.0..33.0).contains(&l) => 35,
            l if (33.0..42.0).contains(&l) => 37,
            _ => zone,
        };
    }
    zone
}

/// The EPSG code of WGS 84 / UTM for a place: 326zz north of the equator, 327zz south.
pub fn utm_epsg(longitude: f64, latitude: f64) -> u32 {
    (if latitude >= 0.0 { 32600 } else { 32700 }) + u32::from(zone_of(longitude, latitude))
}

/// The longest text [`parse_coordinates`] reads: a pair is under fifty characters, and a clipboard holds anything.
const MAX_COORDINATES: usize = 200;

/// One number of a pair, with the hemisphere it names (`N`, `S`, `E` or `W`, before or after it), as the sign it makes: `23.5505 S` is
/// -23.5505. A decimal comma is a point; a degree sign is dropped.
fn coordinate(part: &str) -> Result<(f64, Option<char>), String> {
    let text = part.trim().replace('°', "");
    let text = text.trim();
    let number = |t: &str| -> Option<f64> {
        let t = t.trim();
        let t = if t.contains('.') { t.to_string() } else { t.replace(',', ".") };
        t.parse::<f64>().ok().filter(|x| x.is_finite())
    };
    if let Some(x) = number(text) {
        return Ok((x, None));
    }
    let hemisphere = |c: char| matches!(c.to_ascii_uppercase(), 'N' | 'S' | 'E' | 'W');
    let (first, last) = (text.chars().next(), text.chars().next_back());
    let tries = [
        first.filter(|c| hemisphere(*c)).map(|c| (c, &text[c.len_utf8()..])),
        last.filter(|c| hemisphere(*c)).map(|c| (c, &text[..text.len() - c.len_utf8()])),
    ];
    for (letter, rest) in tries.into_iter().flatten() {
        if let Some(x) = number(rest) {
            let letter = letter.to_ascii_uppercase();
            if x < 0.0 || rest.trim_start().starts_with('+') {
                return Err(format!("`{text}` has a sign and a hemisphere ({letter}): give one or the other"));
            }
            return Ok((if matches!(letter, 'S' | 'W') { -x } else { x }, Some(letter)));
        }
    }
    Err(format!("`{text}` is not a number of degrees"))
}

/// A latitude and a longitude, degrees, read from the text of a pair as a map gives it: `51.2254809, 6.7700762`, with a `;` or a space
/// for the comma, a decimal comma (`51,2254809; 6,7700762`), degree signs, brackets, and `N`, `S`, `E`, `W` before or after a number
/// (which may then come in either order). The pair is latitude first unless a hemisphere says otherwise; a latitude beyond 90 or a
/// longitude beyond 180 is refused, never taken for a swapped pair.
pub fn parse_coordinates(text: &str) -> Result<(f64, f64), String> {
    let wanted = "give a latitude and longitude in degrees, such as 51.2254809, 6.7700762";
    let t = text.trim();
    if t.is_empty() {
        return Err(wanted.to_string());
    }
    if t.chars().count() > MAX_COORDINATES {
        return Err(format!("that is too long to be a pair of coordinates: {wanted}"));
    }
    let t = t.trim_matches(|c| "()[]{}".contains(c)).trim();
    let parts: Vec<&str> = if t.contains(';') {
        t.split(';').collect()
    } else if t.matches(',').count() == 1 {
        t.split(',').collect()
    } else {
        t.split_whitespace().collect()
    };
    let [first, second] = parts.as_slice() else {
        return Err(format!("two numbers (latitude, longitude) are wanted, not {}: {wanted}", parts.len()));
    };
    let ((a, ha), (b, hb)) = (coordinate(first)?, coordinate(second)?);
    let is_latitude = |h: char| matches!(h, 'N' | 'S');
    // Which is the latitude: what a hemisphere says, else the first.
    let first_is_latitude = match (ha, hb) {
        (Some(x), Some(y)) if is_latitude(x) == is_latitude(y) => {
            return Err(format!(
                "`{first}` and `{second}` are both a {}: one latitude (N, S) and one longitude (E, W) are wanted",
                if is_latitude(x) { "latitude" } else { "longitude" }
            ));
        }
        (Some(x), _) => is_latitude(x),
        (None, Some(y)) => !is_latitude(y),
        (None, None) => true,
    };
    let (latitude, longitude) = if first_is_latitude { (a, b) } else { (b, a) };
    if latitude.abs() > 90.0 {
        return Err(format!("the latitude {latitude} is beyond 90°: a pair is latitude first, then longitude"));
    }
    if longitude.abs() > 180.0 {
        return Err(format!("the longitude {longitude} is beyond 180°"));
    }
    Ok((latitude, longitude))
}

/// The active site's way between WGS84 and the project's own coordinates, built once for the many points of an import or a
/// fetch: its georeference (`Model::site`) and its map system's projector.
pub struct Projector {
    site: crate::model::Georeference,
    map: crate::crs::Projector,
}

impl Projector {
    /// The projector of `m`'s active site; refused, saying how to give it one, when the site has no map system.
    pub fn of(m: &crate::model::Model) -> Result<Projector, String> {
        let site = m.site().clone();
        let code = crate::crs::code(&site.crs).ok_or_else(|| {
            let name = serde_json::Value::from(m.coordinates.active.as_str());
            format!(
                "the project has no place on Earth: give its site a map system and where it is, as `location {{\"name\": {name}, \
                 \"crs\": \"EPSG:32723\", \"latitude\": -23.5505, \"longitude\": -46.6333}}` (Manage > Location); WGS 84's UTM \
                 zone of the place (EPSG:326zz north of the equator, 327zz south) suits most places"
            )
        })?;
        Ok(Projector { map: crate::crs::Projector::new(code)?, site })
    }

    /// Longitude `lon`, latitude `lat` as a point of the project's plan, mm.
    pub fn point(&self, lon: f64, lat: f64) -> Result<P2, String> {
        let (e, n) = self.map.from_wgs84(lon, lat)?;
        let [x, y, _] = self.site.to_internal([e, n, self.site.height]);
        let p = P2(x, y);
        if !(x.is_finite() && y.is_finite()) || x.abs() > crate::model::MAX_COORD || y.abs() > crate::model::MAX_COORD {
            return Err(format!("{lon}, {lat} is too far from the project's origin to be drawn in it"));
        }
        Ok(p)
    }

    /// Point `q` of the project's plan (mm) as longitude and latitude.
    pub fn lonlat(&self, q: P2) -> Result<(f64, f64), String> {
        let [e, n, _] = self.site.to_shared([q.0, q.1, 0.0]);
        self.map.to_wgs84(e, n)
    }

    /// Where the project's internal origin is on Earth.
    pub fn origin(&self) -> Result<(f64, f64), String> {
        self.lonlat(P2(0.0, 0.0))
    }

    /// The height on the map of the project's zero, mm (what a height above sea level is less to be one of the project's).
    pub fn height(&self) -> f64 {
        self.site.height
    }

    /// The site it projects through.
    pub fn site(&self) -> &crate::model::Georeference {
        &self.site
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAO_PAULO: (f64, f64) = (-46.6333, -23.5505);

    fn near(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() <= eps
    }

    fn block() -> Bbox {
        Bbox::new(-23.56, -46.64, -23.54, -46.62).unwrap()
    }

    #[test]
    fn a_box_is_checked_and_has_its_area() {
        let b = block();
        assert_eq!(b.as_array(), [-23.56, -46.64, -23.54, -46.62]);
        // Independent: R^2 * dlon * (sin north - sin south) on a sphere of 6371.0088 km.
        assert!(near(b.area_km2(), 4.533_816, 1e-4), "{}", b.area_km2());
        assert!(near(Bbox::new(0.0, 0.0, 1.0, 1.0).unwrap().area_km2(), 12_363.718, 1e-2));
        assert!(b.contains(SAO_PAULO.0, SAO_PAULO.1) && !b.contains(-46.0, -23.55) && !b.contains(-46.63, -22.0));
        let (lon, lat) = b.centre();
        assert!(near(lon, -46.63, 1e-9) && near(lat, -23.55, 1e-9));
        for (s, w, n, e) in [
            (-23.5, -46.6, -23.6, -46.5),
            (-23.6, -46.5, -23.5, -46.6),
            (-23.6, -46.6, -23.6, -46.5),
            (-91.0, 0.0, 0.0, 1.0),
            (0.0, 0.0, 91.0, 1.0),
            (0.0, -181.0, 1.0, 0.0),
            (0.0, 0.0, 1.0, 181.0),
            (f64::NAN, 0.0, 1.0, 1.0),
            (0.0, 0.0, f64::INFINITY, 1.0),
        ] {
            assert!(Bbox::new(s, w, n, e).unwrap_err().contains("bbox"), "{s} {w} {n} {e}");
        }
    }

    #[test]
    fn a_box_round_a_place_is_the_size_asked_for_and_too_big_a_box_is_refused() {
        let b = Bbox::around(SAO_PAULO.0, SAO_PAULO.1, 1000.0).unwrap();
        assert!(near(b.area_km2(), 4.0, 0.05), "2 km by 2 km: {}", b.area_km2());
        assert!(b.contains(SAO_PAULO.0, SAO_PAULO.1));
        assert!(b.check_area().is_ok());
        let big = Bbox::around(SAO_PAULO.0, SAO_PAULO.1, 4000.0).unwrap();
        let why = big.check_area().unwrap_err();
        assert!(why.contains("km²") && why.contains("25"), "{why}");
        assert!(Bbox::around(0.0, 89.99999, 1000.0).is_err(), "over the pole");
        assert!(Bbox::around(179.9999, 0.0, 1000.0).is_err(), "over the antimeridian");
        assert!(Bbox::around(0.0, 0.0, -5.0).is_err() && Bbox::around(0.0, 0.0, f64::NAN).is_err() && Bbox::around(0.0, 0.0, 0.0).is_err());
    }

    #[test]
    fn places_have_tile_positions_as_the_web_mercator_map_has_them() {
        // Independent values (Python, the slippy map formulae) for São Paulo.
        for (z, x, y) in [
            (0, 0.370_463_055_555_555_6, 0.567_341_942_171_303_8),
            (1, 0.740_926_111_111_111_2, 1.134_683_884_342_607_7),
            (10, 379.354_168_888_888_9, 580.958_148_783_415_1),
            (14, 6_069.666_702_222_223, 9_295.330_380_534_642),
            (15, 12_139.333_404_444_445, 18_590.660_761_069_284),
        ] {
            let (tx, ty) = tile_xy(SAO_PAULO.0, SAO_PAULO.1, z);
            assert!(near(tx, x, 1e-9) && near(ty, y, 1e-9), "z{z}: {tx} {ty}");
            let (lon, lat) = tile_lonlat(tx, ty, z);
            assert!(near(lon, SAO_PAULO.0, 1e-9) && near(lat, SAO_PAULO.1, 1e-9), "z{z} back: {lon} {lat}");
        }
        assert_eq!(tile_xy(0.0, 0.0, 3), (4.0, 4.0));
        assert!(near(tile_xy(-180.0, 90.0, 2).1, 0.0, 1e-6), "past the map's top is its top");
        assert!(near(tile_xy(180.0, -90.0, 2).1, 4.0, 1e-6));
    }

    #[test]
    fn the_tiles_that_cover_a_box_are_listed_and_too_many_are_refused() {
        let b = block();
        assert_eq!(tiles_for(&b, 12).unwrap(), [(1517, 2323)]);
        assert_eq!(tiles_for(&b, 14).unwrap(), [(6069, 9294), (6070, 9294), (6069, 9295), (6070, 9295)]);
        assert_eq!(tiles_for(&b, 15).unwrap().len(), 9);
        let wide = Bbox::new(-24.0, -47.0, -23.0, -46.0).unwrap();
        assert!(tiles_for(&wide, 15).unwrap_err().contains("tiles"));
    }

    #[test]
    fn the_zoom_is_the_coarsest_that_is_fine_enough() {
        let b = block();
        // 156543.03392 * cos(lat) / 2^z metres a pixel: 17.5 at 13, 8.76 at 14, 4.38 at 15.
        assert_eq!(zoom_for(&b, 10.0), 14);
        assert_eq!(zoom_for(&b, 5.0), 15);
        assert_eq!(zoom_for(&b, 1.0), 15, "no finer than the tiles go");
        assert_eq!(zoom_for(&b, 1000.0), 8);
        assert_eq!(zoom_for(&b, 1e9), 0);
        assert_eq!(zoom_for(&b, f64::NAN), 15, "nonsense asks for the finest");
    }

    #[test]
    fn the_overpass_query_asks_for_buildings_in_the_box() {
        // Without parts, the outlines alone (an agent may ask for that).
        let q = overpass_query(&block(), Layers { parts: false, entrances: false });
        assert_eq!(
            q,
            "[out:json][timeout:60][maxsize:67108864];(way[\"building\"](-23.56,-46.64,-23.54,-46.62);\
             relation[\"building\"][\"type\"=\"multipolygon\"](-23.56,-46.64,-23.54,-46.62););out geom;"
        );
        // By default a building is its parts: they, and the columns that hold a roof up, are in the query.
        assert_eq!(overpass_query(&block(), Layers::default()), overpass_query(&block(), Layers::default()));
        let with = overpass_query(&block(), Layers::default());
        assert!(with.contains("way[\"man_made\"=\"column\"](-23.56,-46.64,-23.54,-46.62)"), "{with}");
        assert!(with.contains("way[\"building:part\"](-23.56,-46.64,-23.54,-46.62)"), "{with}");
        assert!(with.contains("relation[\"building:part\"][\"type\"=\"multipolygon\"]"), "{with}");
        // A relation that groups an outline with its parts, too.
        assert!(with.contains("relation[\"type\"=\"building\"](-23.56,-46.64,-23.54,-46.62)"), "{with}");
        assert!(!overpass_query(&block(), Layers { parts: false, entrances: false }).contains("type\"=\"building"));
        // The entrances, doors and exits mapped as nodes, in the box; and not unless asked for.
        assert!(
            with.contains("node[\"entrance\"](-23.56,-46.64,-23.54,-46.62);node[\"door\"](-23.56,-46.64,-23.54,-46.62););out geom;"),
            "{with}"
        );
        assert!(!overpass_query(&block(), Layers { parts: true, entrances: false }).contains("node["));
        // Numbers are written plainly: no exponent, no trailing zeros.
        let tiny = Bbox::new(0.000_001, 0.5, 0.000_002, 1.0).unwrap();
        assert!(
            overpass_query(&tiny, Layers::default()).contains("(0.000001,0.5,0.000002,1)"),
            "{}",
            overpass_query(&tiny, Layers::default())
        );
    }

    #[test]
    fn a_grid_of_heights_is_sampled_between_its_nodes() {
        // z = 10 * (lon - west) + 100 * (north - lat): a plane, 5 x 4 nodes a tenth of a degree apart.
        let (west, north) = (-47.0, -23.0);
        let values: Vec<f32> =
            (0..4).flat_map(|r| (0..5).map(move |c| (10.0 * (c as f64 * 0.1) + 100.0 * (r as f64 * 0.1)) as f32)).collect();
        let g = ElevationGrid::new(west, north, 0.1, 0.1, 5, 4, values.clone()).unwrap();
        for (lon, lat) in [(-47.0, -23.0), (-46.63, -23.17), (-46.6, -23.3), (-46.77, -23.04)] {
            let z = g.sample(lon, lat).unwrap_or_else(|| panic!("no height at {lon} {lat}"));
            assert!(near(z, 10.0 * (lon - west) + 100.0 * (north - lat), 1e-4), "{lon} {lat}: {z}");
        }
        assert_eq!(
            (g.sample(-47.01, -23.1), g.sample(-46.5, -23.1), g.sample(-46.8, -22.9), g.sample(-46.8, -23.31)),
            (None, None, None, None)
        );
        assert_eq!(g.sample(f64::NAN, -23.1), None);
        // A node with no height leaves the cells round it without one.
        let mut holey = values;
        holey[6] = f32::NAN;
        let g = ElevationGrid::new(west, north, 0.1, 0.1, 5, 4, holey).unwrap();
        assert_eq!(g.sample(-46.85, -23.15), None);
        assert!(g.sample(-46.65, -23.25).is_some());
        for bad in [
            ElevationGrid::new(west, north, 0.0, 0.1, 5, 4, vec![0.0; 20]),
            ElevationGrid::new(west, north, 0.1, -0.1, 5, 4, vec![0.0; 20]),
            ElevationGrid::new(west, north, 0.1, 0.1, 5, 4, vec![0.0; 19]),
            ElevationGrid::new(west, north, 0.1, 0.1, 1, 4, vec![0.0; 4]),
            ElevationGrid::new(f64::NAN, north, 0.1, 0.1, 5, 4, vec![0.0; 20]),
            ElevationGrid::new(west, north, 0.1, 0.1, 5000, 5000, Vec::new()),
        ] {
            assert!(bad.is_err());
        }
    }

    #[test]
    fn nodes_are_laid_out_a_spacing_apart_and_never_too_many() {
        let b = block();
        let Nodes { at: n, cols, rows } = nodes(&b, 100.0, 50_000).unwrap();
        assert_eq!(n.len(), cols * rows);
        // 2.2 km by 2.2 km at 100 m: 23 or so on each side.
        assert!((20..=26).contains(&cols) && (20..=26).contains(&rows), "{cols} x {rows}");
        assert!(n[0].0 >= b.west - 1e-9 && n[0].1 <= b.north + 1e-9, "from the north west corner");
        assert!(n.iter().all(|(lon, lat)| b.contains(*lon, *lat)));
        assert!(nodes(&b, 1.0, 50_000).unwrap_err().contains("spacing"));
        assert!(nodes(&b, 0.0, 50_000).is_err() && nodes(&b, f64::NAN, 50_000).is_err() && nodes(&b, -1.0, 50_000).is_err());
        assert!(nodes(&b, 100.0, 10).is_err());
    }
}
