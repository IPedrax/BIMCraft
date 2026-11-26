//! Point clouds: what a laser scan brings in. A file's points are read from public specifications
//! (PTS and XYZ text, PLY, ASPRS LAS 1.2 to 1.4, LAZ and COPC as LASzip compresses them, ASTM E57),
//! turned into millimetres, thinned to what a project may hold, and kept about their centre as `f32`, which holds a
//! millimetre across `MAX_SPAN`. A project holds each scan once, as a [`Cloud`]; a `PointCloud` element places it.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::sync::{Arc, OnceLock};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use serde::{Deserialize, Serialize};

mod e57;
pub mod index;
#[cfg(feature = "laz")]
mod laz;
pub mod lod;

/// Points a cloud may keep: a file with more is thinned down to it.
pub const MAX_CLOUD_POINTS: usize = 5_000_000;
/// What a file may be before it's refused, bytes: a terabyte. A file is read a piece at a time and never held, so what it
/// takes of memory is the points kept, not its size.
pub const MAX_BYTES: u64 = 1 << 40;
/// What a point kept takes of memory while a file is read, bytes: its place and colour, its cube when thinned by spacing, and
/// for a file read once (LAZ), what it was in the file until the file's bounds are known.
pub const KEPT_BYTES: u64 = 96;
/// The longest line a text file of points may have, bytes: a point is a hundred or so.
const MAX_LINE: usize = 1 << 20;
/// How much of a file is read at a time, bytes.
const BLOCK: usize = 1 << 20;
/// How far a file's coordinates may lie from its own origin, mm: a survey's, a long way off, but never past it.
pub const MAX_FILE_COORD: f64 = 1e12;
/// The widest a cloud may be along any axis, mm: `f32` about its centre holds a millimetre that far.
pub const MAX_SPAN: f64 = 2e7;
/// Scans one project may hold.
pub const MAX_CLOUDS: usize = 16;
/// Files one scan may be brought in from: a survey's stations.
pub const MAX_STATIONS: usize = 256;
/// How wide a square of a cloud's [`Grid`] is, mm.
const CELL: f64 = 250.0;
/// The most nodes of a scan's index a snap reads.
const NEAREST_NODES: usize = 64;

/// What a build without the `laz` feature says of a LAZ file.
const NO_LAZ: &str =
    "this build of BIMCraft reads no LAZ (it was built without the `laz` feature): decompress it to LAS first (laszip or PDAL)";

/// What a file of points is written as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Text: a count line, then `x y z`, with an intensity and a colour or not.
    Pts,
    /// Text: `x y z`, with a colour or not.
    Xyz,
    /// The Polygon File Format: text or binary.
    Ply,
    /// ASPRS LAS 1.2 to 1.4, uncompressed.
    Las,
    /// LAZ: LAS whose points LASzip compressed; COPC (`.copc.laz`) is one.
    Laz,
    /// ASTM E57: every scan the file holds, placed by its pose.
    E57,
}

impl Format {
    /// The format a file's extension says, or why it can't be read.
    pub fn from_extension(ext: &str) -> Result<Format, String> {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        match ext.as_str() {
            "pts" => Ok(Format::Pts),
            "xyz" | "txt" => Ok(Format::Xyz),
            "ply" => Ok(Format::Ply),
            "las" => Ok(Format::Las),
            "laz" if cfg!(feature = "laz") => Ok(Format::Laz),
            "laz" => Err(NO_LAZ.into()),
            "e57" => Ok(Format::E57),
            "rcp" | "rcs" => Err("ReCap's RCP and RCS are closed formats: export the scan from ReCap as E57, LAS or PTS".into()),
            _ => Err(format!("a .{ext} file isn't a point cloud BIMCraft reads: PTS, XYZ, PLY, LAS, LAZ or E57")),
        }
    }
}

/// How a file is read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Millimetres to the file's unit: 1000 for metres.
    pub scale: f64,
    /// Thinned to one point in each cube this wide, mm; 0 keeps them all.
    pub spacing: f64,
    /// The most points kept.
    pub cap: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options { scale: 1000.0, spacing: 0.0, cap: MAX_CLOUD_POINTS }
    }
}

/// Millimetres to a unit a file may be in, by name: mm, cm, m, in, ft.
pub fn unit(name: &str) -> Option<f64> {
    match name.trim().to_ascii_lowercase().as_str() {
        "mm" => Some(1.0),
        "cm" => Some(10.0),
        "m" => Some(1000.0),
        "in" => Some(25.4),
        "ft" => Some(304.8),
        _ => None,
    }
}

/// The points read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Scan {
    /// Where the cloud's own zero is in the file's coordinates, mm: the middle of its bounding box.
    pub origin: [f64; 3],
    /// Each point from `origin`, mm.
    pub points: Vec<[f32; 3]>,
    /// Each point's colour, or its intensity as grey; none when the file holds neither.
    pub colours: Option<Vec<[u8; 3]>>,
    /// Each point's intensity, from the least in the file (0) to the most (255); none when the file holds none, or all alike.
    pub intensity: Option<Vec<u8>>,
    /// The points the file holds, before thinning.
    pub read: usize,
}

impl Scan {
    /// A point in the file's coordinates, mm.
    pub fn absolute(&self, i: usize) -> Option<[f64; 3]> {
        let p = self.points.get(i)?;
        Some([self.origin[0] + f64::from(p[0]), self.origin[1] + f64::from(p[1]), self.origin[2] + f64::from(p[2])])
    }
}

/// One of the files a scan was brought in from: its name, and how many of the scan's points, following the stations before it,
/// came from it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Station {
    pub source: String,
    pub points: usize,
}

/// Several files' scans as one, about the middle of them all, and a station for each. Where `spacing` is more than 0, one point
/// is kept in each cube of it, so where the stations overlap they aren't kept twice. Among files with colours, one without is
/// grey.
pub fn merge(scans: Vec<(String, Scan)>, spacing: f64) -> Result<(Scan, Vec<Station>), String> {
    if !(spacing.is_finite() && spacing >= 0.0) {
        return Err(format!("a spacing of {spacing} mm isn't one"));
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (_, scan) in &scans {
        for p in &scan.points {
            for k in 0..3 {
                let v = scan.origin[k] + f64::from(p[k]);
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
    }
    if !lo.iter().all(|v| v.is_finite()) {
        return Err("the files hold no points".into());
    }
    for k in 0..3 {
        if hi[k] - lo[k] > MAX_SPAN {
            return Err(format!("together they're {:.0} mm wide along {}; up to {MAX_SPAN} mm", hi[k] - lo[k], ["x", "y", "z"][k]));
        }
    }
    let origin = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let coloured = scans.iter().any(|(_, scan)| scan.colours.is_some());
    let bright = scans.iter().any(|(_, scan)| scan.intensity.is_some());
    let mut cells = HashSet::new();
    let (mut points, mut colours, mut levels, mut stations, mut read) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), 0_usize);
    for (source, scan) in scans {
        let before = points.len();
        for (i, p) in scan.points.iter().enumerate() {
            let a = [0, 1, 2].map(|k| scan.origin[k] + f64::from(p[k]));
            if spacing > 0.0 && !cells.insert([0, 1, 2].map(|k| ((a[k] - lo[k]) / spacing).floor() as i64)) {
                continue;
            }
            points.push([0, 1, 2].map(|k| (a[k] - origin[k]) as f32));
            if coloured {
                colours.push(scan.colours.as_ref().and_then(|c| c.get(i).copied()).unwrap_or([GREY; 3]));
            }
            // Among files with an intensity, one without is in the middle of the range.
            if bright {
                levels.push(scan.intensity.as_ref().and_then(|l| l.get(i).copied()).unwrap_or(128));
            }
        }
        read = read.saturating_add(scan.read);
        stations.push(Station { source, points: points.len() - before });
    }
    Ok((Scan { origin, points, colours: coloured.then_some(colours), intensity: bright.then_some(levels), read }, stations))
}

/// A scan as a project holds it: its points about their middle, mm, their colours if the file had any, and where it came
/// from. Cheap to clone: the points are shared.
#[derive(Clone)]
pub struct Cloud {
    source: String,
    /// The files it came from, when it came from more than one.
    stations: Vec<Station>,
    origin: [f64; 3],
    read: usize,
    points: Arc<Vec<[f32; 3]>>,
    colours: Option<Arc<Vec<[u8; 3]>>>,
    /// Each point's intensity, 0 to 255 over the file's range, if the file had one.
    intensity: Option<Arc<Vec<u8>>>,
    lo: [f32; 3],
    hi: [f32; 3],
    /// What a project file holds of it, made the first time it's saved.
    packed: Arc<OnceLock<CloudFile>>,
    /// Where its points are from above, made the first time something snaps to it.
    grid: Arc<OnceLock<Grid>>,
    /// Its index, when every point of it is kept in one beside the project and these are the top of it.
    index: Option<lod::IndexRef>,
    /// Its index opened, once it has been: not in a file.
    opened: Arc<OnceLock<Arc<lod::Indexed>>>,
    /// Each intensity's place among its points', worked out the first time a view colours by them.
    stretch: Arc<OnceLock<Arc<[u8; 256]>>>,
}

/// A cloud's points by the square [`CELL`] mm wide they lie in, seen from above.
struct Grid {
    cells: HashMap<(i64, i64), Vec<u32>>,
}

fn cell(v: f64) -> i64 {
    (v / CELL).floor() as i64
}

impl std::fmt::Debug for Cloud {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cloud({:?}, {} points)", self.source, self.points.len())
    }
}

impl PartialEq for Cloud {
    fn eq(&self, other: &Cloud) -> bool {
        self.source == other.source
            && self.stations == other.stations
            && self.origin == other.origin
            && self.read == other.read
            && (Arc::ptr_eq(&self.points, &other.points) || self.points == other.points)
            && self.colours == other.colours
            && self.intensity == other.intensity
            && self.index == other.index
    }
}

#[cfg(test)]
thread_local! {
    /// The points [`Cloud::nearest`] has looked at on this thread: the tests assert on it, not on time, which varies with the
    /// load of the machine.
    pub(crate) static LOOKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl Cloud {
    /// A scan read from the file `source` (its name, for people to know it by), or why it can't be held.
    pub fn new(source: &str, scan: Scan) -> Result<Cloud, String> {
        let Scan { origin, points, colours, intensity, read } = scan;
        Cloud::checked(source.chars().take(crate::model::MAX_TEXT).collect(), Vec::new(), origin, read, points, colours, intensity)
    }

    /// A scan brought in from several files, `stations`, or why it can't be held. It's known by the first.
    pub fn of_stations(scan: Scan, stations: Vec<Station>) -> Result<Cloud, String> {
        let Scan { origin, points, colours, intensity, read } = scan;
        let source = stations.first().map(|t| t.source.clone()).unwrap_or_default();
        Cloud::checked(source, stations, origin, read, points, colours, intensity)
    }

    fn checked(
        mut source: String,
        mut stations: Vec<Station>,
        origin: [f64; 3],
        read: usize,
        points: Vec<[f32; 3]>,
        colours: Option<Vec<[u8; 3]>>,
        intensity: Option<Vec<u8>>,
    ) -> Result<Cloud, String> {
        if points.is_empty() || points.len() > MAX_CLOUD_POINTS {
            return Err(format!("a cloud holds 1 to {MAX_CLOUD_POINTS} points, not {}", points.len()));
        }
        if read < points.len() {
            return Err(format!("a cloud read from {read} points can't keep {}", points.len()));
        }
        if !origin.iter().all(|v| v.is_finite() && v.abs() <= MAX_FILE_COORD) {
            return Err(format!("a cloud's origin {origin:?} isn't one"));
        }
        if colours.as_ref().is_some_and(|c| c.len() != points.len()) {
            return Err("a cloud has a colour for each point or none".into());
        }
        if intensity.as_ref().is_some_and(|l| l.len() != points.len()) {
            return Err("a cloud has an intensity for each point or none".into());
        }
        if stations.len() == 1 {
            stations.clear();
        }
        if stations.len() > MAX_STATIONS {
            return Err(format!("a cloud comes from up to {MAX_STATIONS} files, not {}", stations.len()));
        }
        // An indexed scan's stations hold the points each file gave the index.
        if !stations.is_empty()
            && !stations.iter().map(|t| t.points).try_fold(0_usize, usize::checked_add).is_some_and(|n| n == points.len() || n == read)
        {
            return Err("a cloud's stations hold all its points between them".into());
        }
        for t in &mut stations {
            t.source = t.source.chars().take(crate::model::MAX_TEXT).collect();
        }
        if let Some(first) = stations.first() {
            source.clone_from(&first.source);
        }
        let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &points {
            for k in 0..3 {
                let v = p[k];
                if !(v.is_finite() && f64::from(v.abs()) <= MAX_SPAN) {
                    return Err(format!("a cloud's point lies {v} mm from its middle, past {MAX_SPAN}"));
                }
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        Ok(Cloud {
            source,
            stations,
            origin,
            read,
            points: Arc::new(points),
            colours: colours.map(Arc::new),
            intensity: intensity.map(Arc::new),
            lo,
            hi,
            packed: Arc::default(),
            grid: Arc::default(),
            index: None,
            opened: Arc::default(),
            stretch: Arc::default(),
        })
    }

    /// This scan with every point of it in the index `index` names, these being the top of it.
    pub fn with_index(self, index: lod::IndexRef) -> Result<Cloud, String> {
        index.check()?;
        Ok(Cloud { index: Some(index), packed: Arc::default(), opened: Arc::default(), ..self })
    }

    /// What the project holds of its index, if it has one.
    pub fn index(&self) -> Option<&lod::IndexRef> {
        self.index.as_ref()
    }

    /// Its index, open, if it has been opened ([`Cloud::attach`]).
    pub fn indexed(&self) -> Option<&Arc<lod::Indexed>> {
        self.opened.get()
    }

    /// Opens its index at the path the project holds (from the folder of the project at `project`), or the full one it was
    /// built at, unless it's open; why it can't be when it can't. Every copy of the scan shares it.
    pub fn attach(&self, project: Option<&std::path::Path>) -> Result<(), String> {
        let Some(r) = &self.index else { return Ok(()) };
        if self.opened.get().is_some() {
            return Ok(());
        }
        let near = lod::resolve(&r.path, project);
        let opened = match lod::Indexed::open(&near, r) {
            Ok(i) => i,
            Err(first) => match r.full.as_deref().map(std::path::Path::new).filter(|f| *f != near) {
                Some(full) => lod::Indexed::open(full, r).map_err(|_| first)?,
                None => return Err(first),
            },
        };
        let _ = self.opened.set(Arc::new(opened));
        Ok(())
    }

    /// The file it was read from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The files it came from, each with its share of the points, when it came from more than one.
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    /// Where its middle was in the file's coordinates, mm.
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The points the file held, before thinning.
    pub fn read(&self) -> usize {
        self.read
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// Each point from its middle, mm.
    pub fn points(&self) -> &[[f32; 3]] {
        &self.points
    }

    pub fn colours(&self) -> Option<&[[u8; 3]]> {
        self.colours.as_deref().map(Vec::as_slice)
    }

    /// Its points, shared: what a view keeps of it without copying them.
    pub(crate) fn shared_points(&self) -> Arc<Vec<[f32; 3]>> {
        Arc::clone(&self.points)
    }

    pub(crate) fn shared_colours(&self) -> Option<Arc<Vec<[u8; 3]>>> {
        self.colours.clone()
    }

    /// Each point's intensity, 0 to 255 over the file's range, if the file had one: what a view colours by intensity.
    pub fn intensity(&self) -> Option<&[u8]> {
        self.intensity.as_deref().map(Vec::as_slice)
    }

    pub(crate) fn shared_intensity(&self) -> Option<Arc<Vec<u8>>> {
        self.intensity.clone()
    }

    /// Each intensity's place among its points', 0 to 255: how many are weaker, and half those as strong, of them all. A view
    /// colours by intensity through it (histogram equalization), so its colours spread over the whole ramp however the
    /// returns crowd, as a surveyor's viewer does.
    pub fn intensity_levels(&self) -> Arc<[u8; 256]> {
        Arc::clone(self.stretch.get_or_init(|| {
            let Some(l) = self.intensity.as_deref().filter(|l| !l.is_empty()) else {
                return Arc::new(std::array::from_fn(|v| v as u8));
            };
            let mut count = [0_u64; 256];
            for &v in l.iter() {
                count[usize::from(v)] += 1;
            }
            let n = l.len() as f64;
            let mut below = 0_u64;
            Arc::new(std::array::from_fn(|v| {
                let at = (below as f64 + count[v] as f64 / 2.0) / n;
                below += count[v];
                (at * 255.0).round().clamp(0.0, 255.0) as u8
            }))
        }))
    }

    /// Of its points whose height from its middle lies within `z`, the nearest to `at` (from its middle, mm) seen from above and
    /// no farther than `reach`: what a plan snaps to. It looks only in the squares of its [`Grid`] that `reach` touches.
    pub fn nearest(&self, at: [f64; 2], reach: f64, z: (f64, f64)) -> Option<[f32; 3]> {
        if !(reach.is_finite() && reach >= 0.0 && at.iter().all(|v| v.is_finite() && v.abs() <= MAX_FILE_COORD)) {
            return None;
        }
        // Only the squares it has: none when `at` is far from it.
        let (lo, hi) = self.bounds();
        let (x0, x1) = (cell((at[0] - reach).max(lo[0])), cell((at[0] + reach).min(hi[0])));
        let (y0, y1) = (cell((at[1] - reach).max(lo[1])), cell((at[1] + reach).min(hi[1])));
        if x0 > x1 || y0 > y1 {
            return None;
        }
        let grid = self.grid.get_or_init(|| {
            let mut cells: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
            for (i, p) in self.points.iter().enumerate() {
                cells.entry((cell(f64::from(p[0])), cell(f64::from(p[1])))).or_default().push(i as u32);
            }
            Grid { cells }
        });
        let mut best: Option<(f64, [f32; 3])> = None;
        for x in x0..=x1 {
            for y in y0..=y1 {
                for &i in grid.cells.get(&(x, y)).into_iter().flatten() {
                    #[cfg(test)]
                    LOOKED.with(|n| n.set(n.get() + 1));
                    let Some(&p) = self.points.get(i as usize) else { continue };
                    let d = (f64::from(p[0]) - at[0]).hypot(f64::from(p[1]) - at[1]);
                    if d <= reach && (z.0..=z.1).contains(&f64::from(p[2])) && best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, p));
                    }
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// [`Cloud::nearest`] among every point of it: those of its index too, when it's open, read as they're needed (a few
    /// nodes, those round `at`, which stay read for the next).
    pub fn nearest_of_all(&self, at: [f64; 2], reach: f64, z: (f64, f64)) -> Option<[f32; 3]> {
        let mut best = self.nearest(at, reach, z).map(|p| ((f64::from(p[0]) - at[0]).hypot(f64::from(p[1]) - at[1]), p));
        let Some(ix) = self.indexed() else { return best.map(|(_, p)| p) };
        if !(reach.is_finite() && reach >= 0.0 && at.iter().all(|v| v.is_finite())) {
            return best.map(|(_, p)| p);
        }
        let near = |lo: [f64; 3], hi: [f64; 3]| {
            let touches = lo[0] <= at[0] + reach && hi[0] >= at[0] - reach && lo[1] <= at[1] + reach && hi[1] >= at[1] - reach;
            (touches && lo[2] <= z.1 && hi[2] >= z.0).then_some(f64::MAX / 4.0)
        };
        for i in ix.select(&near, 2_000_000).into_iter().take(NEAREST_NODES) {
            let Some(node) = ix.fetch(i) else { continue };
            for p in node.points.iter() {
                let d = (f64::from(p[0]) - at[0]).hypot(f64::from(p[1]) - at[1]);
                if d <= reach && (z.0..=z.1).contains(&f64::from(p[2])) && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, *p));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    /// The box round its points, from its middle, mm.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        (self.lo.map(f64::from), self.hi.map(f64::from))
    }

    fn file(&self) -> &CloudFile {
        self.packed.get_or_init(|| {
            let points: Vec<u8> = self.points.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            let colours = self.colours.as_ref().map(|c| c.iter().flatten().copied().collect::<Vec<u8>>());
            CloudFile {
                source: self.source.clone(),
                stations: self.stations.clone(),
                origin: self.origin,
                read: self.read,
                points: crate::commands::base64(&deflate(&points)),
                colours: colours.map(|c| crate::commands::base64(&deflate(&c))),
                intensity: self.intensity.as_ref().map(|l| crate::commands::base64(&deflate(l))),
                index: self.index.clone(),
            }
        })
    }
}

/// How a cloud goes in a project file: its points as little-endian `f32` and its colours as bytes, each deflated (zlib)
/// and in base64.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CloudFile {
    source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stations: Vec<Station>,
    origin: [f64; 3],
    read: usize,
    points: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colours: Option<String>,
    /// Each point's intensity as a byte, deflated and in base64; an older file has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    intensity: Option<String>,
    /// Where every point of it is, when the project holds only the top of them; an older file has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    index: Option<lod::IndexRef>,
}

pub(crate) fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    // Writing to a Vec doesn't fail.
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

/// The bytes base64 and zlib hide, if they're no more than `most`.
fn inflate(text: &str, most: usize) -> Result<Vec<u8>, String> {
    inflate_of("a cloud", text, most)
}

/// [`inflate`] of `what`'s data, saying whose it is when it isn't what it should be.
pub(crate) fn inflate_of(what: &str, text: &str, most: usize) -> Result<Vec<u8>, String> {
    let packed = crate::texture::unbase64(text).ok_or_else(|| format!("{what}'s data isn't base64"))?;
    let mut out = Vec::new();
    ZlibDecoder::new(packed.as_slice())
        .take(most as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("{what}'s data isn't zlib: {e}"))?;
    if out.len() > most {
        return Err(format!("{what}'s data holds more than {most} bytes"));
    }
    Ok(out)
}

impl TryFrom<CloudFile> for Cloud {
    type Error = String;

    fn try_from(f: CloudFile) -> Result<Cloud, String> {
        let bytes = inflate(&f.points, MAX_CLOUD_POINTS * 12)?;
        if bytes.len() % 12 != 0 {
            return Err("a cloud's points are cut short".into());
        }
        let points: Vec<[f32; 3]> =
            bytes.as_chunks::<12>().0.iter().map(|c| [0, 4, 8].map(|i| f32::from_le_bytes([c[i], c[i + 1], c[i + 2], c[i + 3]]))).collect();
        let colours = match &f.colours {
            Some(text) => Some(inflate(text, MAX_CLOUD_POINTS * 3)?.as_chunks::<3>().0.to_vec()),
            None => None,
        };
        let intensity = match &f.intensity {
            Some(text) => Some(inflate(text, MAX_CLOUD_POINTS)?),
            None => None,
        };
        let source = f.source.chars().take(crate::model::MAX_TEXT).collect();
        let mut cloud = Cloud::checked(source, f.stations.clone(), f.origin, f.read, points, colours, intensity)?;
        if let Some(r) = &f.index {
            r.check()?;
            cloud.index = Some(r.clone());
        }
        let _ = cloud.packed.set(f);
        Ok(cloud)
    }
}

impl Serialize for Cloud {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.file().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Cloud {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Cloud, D::Error> {
        Cloud::try_from(CloudFile::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// A project file's scans (`Model::clouds`), read no further than a project may hold: the entry past [`MAX_CLOUDS`] is
/// refused before its data is unpacked, and so is the one that would take the points past [`MAX_CLOUD_POINTS`], since
/// unpacking is where the memory goes (a few kilobytes of zeros inflate to tens of megabytes) and `Model::check` only runs after.
pub(crate) fn deserialize_scans<'de, D: serde::Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, Cloud>, D::Error> {
    scans_within(d, MAX_CLOUDS, MAX_CLOUD_POINTS)
}

fn scans_within<'de, D: serde::Deserializer<'de>>(
    d: D,
    most: usize,
    points: usize,
) -> Result<std::collections::BTreeMap<String, Cloud>, D::Error> {
    use serde::de::{Error, MapAccess, Visitor};
    struct Scans {
        most: usize,
        points: usize,
    }
    impl<'de> Visitor<'de> for Scans {
        type Value = std::collections::BTreeMap<String, Cloud>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of scans")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let (mut scans, mut held) = (Self::Value::new(), 0_usize);
            while let Some(name) = a.next_key::<String>()? {
                if scans.len() >= self.most {
                    return Err(A::Error::custom(format!("the file holds more than {} scans, all a project may hold", self.most)));
                }
                let cloud: Cloud = a.next_value()?;
                held = held.saturating_add(cloud.len());
                if held > self.points {
                    return Err(A::Error::custom(format!(
                        "the file's scans hold more than {} points, all a project may hold",
                        self.points
                    )));
                }
                scans.insert(name, cloud);
            }
            Ok(scans)
        }
    }
    d.deserialize_map(Scans { most, points })
}

/// A scan read on a thread of its own ([`crate::engine::Session::read_scan_ahead`]), for the `point_cloud` that places it to
/// take ([`crate::engine::Session::give_scan`]) instead of reading its files again: so a front end reads a big file without
/// stopping, and the command that changes the project is still one undo step on its own session. It's taken only for the files
/// and options it was read for, the project leaving it the same room.
#[derive(Clone)]
pub struct ScanRead {
    pub(crate) key: ScanKey,
    pub(crate) cloud: Cloud,
}

/// What a scan was read for: its files, their unit, the spacing, and the most points the project left it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScanKey {
    pub(crate) paths: Vec<std::path::PathBuf>,
    /// The index it was built into, if it was.
    pub(crate) index: Option<std::path::PathBuf>,
    pub(crate) scale: f64,
    pub(crate) spacing: f64,
    pub(crate) cap: usize,
}

/// A file of points as it's read: one on disk, a piece at a time, or bytes in memory.
pub trait Source: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync + ?Sized> Source for T {}

/// The points of a file of `format` held in memory, or what's wrong with it.
pub fn read(bytes: &[u8], format: Format, o: &Options) -> Result<Scan, String> {
    read_from(&mut std::io::Cursor::new(bytes), format, o, None)
}

/// How many points are read between looks at the memory and at Cancel.
const LOOK: u64 = 1 << 16;

/// The points of a file of `format`, read from `src` a piece at a time and never held, or what's wrong with it: what it takes
/// of memory is the points kept. `budget`, when given, is looked at as it reads (the memory the machine has free, and Cancel),
/// and its progress hears how far it is.
pub fn read_from(src: &mut dyn Source, format: Format, o: &Options, budget: Option<&crate::memory::Budget>) -> Result<Scan, String> {
    if !(o.scale.is_finite() && o.scale > 0.0) {
        return Err(format!("a unit of {} mm isn't one", o.scale));
    }
    if !(o.spacing.is_finite() && o.spacing >= 0.0) {
        return Err(format!("a spacing of {} mm isn't one", o.spacing));
    }
    if o.cap == 0 {
        return Err("a cloud keeps one point at least".into());
    }
    let len = src.seek(SeekFrom::End(0)).map_err(|e| format!("it can't be read: {e}"))?;
    if len > MAX_BYTES {
        return Err(format!("it's {len} bytes; up to {MAX_BYTES}"));
    }
    let progress = budget.and_then(crate::memory::Budget::progress);
    // Every `LOOK` points on: the memory and Cancel looked at, and the progress moved on to the point at `now`.
    let look = |last: &mut u64, now: u64| -> Result<(), String> {
        if now >= last.saturating_add(LOOK) {
            if let Some(b) = budget {
                b.check()?;
            }
            if let Some(p) = progress {
                p.step(now - *last);
            }
            *last = now;
        }
        Ok(())
    };
    // ponytail: the file is parsed twice, once to measure it and once to keep its points; text files pay for it. A LAZ file
    // is read once: its points can't be stepped over, so a second pass would decompress every one of them again, and its
    // header says how many it holds, so which to keep is known as it's read.
    let known = once(src, len, format);
    let every = known.map(|count| count.div_ceil(o.cap as u64).max(1));
    if let Some(p) = progress {
        match known {
            Some(count) => p.stage(bimcraft_i18n::ui_msg!("Reading the points"), count),
            None => p.stage(bimcraft_i18n::ui_msg!("Measuring the scan"), 0),
        }
    }
    let (mut stats, mut last, mut held) = (Stats::default(), 0, Vec::new());
    each(src, len, format, 1, &mut |at, raw| {
        stats.add(&raw, o.scale)?;
        if every.is_some_and(|every| at.is_multiple_of(every)) {
            held.push(Held::of(&raw));
        }
        look(&mut last, stats.count as u64)
    })?;
    if stats.count == 0 {
        return Err("it holds no points".into());
    }
    let mut origin = [0.0; 3];
    for k in 0..3 {
        let wide = stats.max[k] - stats.min[k];
        if wide > MAX_SPAN {
            return Err(format!("it's {wide:.0} mm wide along {}; up to {MAX_SPAN} mm", ["x", "y", "z"][k]));
        }
        origin[k] = (stats.min[k] + stats.max[k]) / 2.0;
    }
    // Every so many, so the cap is never passed, and then one in each cube of the spacing.
    let stride = stats.count.div_ceil(o.cap).max(1);
    let bright = stats.intensity.filter(|(lo, hi)| hi > lo);
    let mut keep = Keep { scale: o.scale, spacing: o.spacing, min: stats.min, origin, colour: stats.colour(), bright, ..Keep::default() };
    if let Some(count) = known {
        if stats.count as u64 != count {
            return Err(format!("it holds {} points, and its header says {count}", stats.count));
        }
        for h in &held {
            keep.push(&h.raw());
        }
    } else {
        let mut last = 0;
        if let Some(p) = progress {
            p.stage(bimcraft_i18n::ui_msg!("Reading the points"), stats.count as u64);
        }
        each(src, len, format, stride as u64, &mut |at, raw| {
            look(&mut last, at)?;
            if at.is_multiple_of(stride as u64) {
                keep.push(&raw);
            }
            Ok(())
        })?;
    }
    let colours = keep.colour.as_ref().map(|_| keep.colours);
    let intensity = keep.bright.map(|_| keep.levels);
    Ok(Scan { origin, points: keep.points, colours, intensity, read: stats.count })
}

/// How many points a file holds, when that's known before it's read and reading it once is what's wanted: a LAZ file's, from
/// its header.
fn once(src: &mut dyn Source, size: u64, format: Format) -> Option<u64> {
    if !matches!(format, Format::Las | Format::Laz) {
        return None;
    }
    src.seek(SeekFrom::Start(0)).ok()?;
    LasHead::read(src, size).ok().filter(|h| h.compressed).map(|h| h.count)
}

/// A point held while a file is read once, until its bounds and shades are known: its place, and its intensity and colour,
/// which a LAZ file holds in sixteen bits and `f32` holds exactly (NaN where it has none).
struct Held {
    p: [f64; 3],
    intensity: f32,
    rgb: [f32; 3],
}

impl Held {
    fn of(raw: &Raw) -> Held {
        Held {
            p: raw.p,
            intensity: raw.intensity.map_or(f32::NAN, |i| i as f32),
            rgb: raw.rgb.map_or([f32::NAN; 3], |c| c.map(|v| v as f32)),
        }
    }

    fn raw(&self) -> Raw {
        Raw {
            p: self.p,
            intensity: (!self.intensity.is_nan()).then_some(f64::from(self.intensity)),
            rgb: (!self.rgb[0].is_nan()).then(|| self.rgb.map(f64::from)),
        }
    }
}

/// The points a read keeps, once its bounds and shades are known: one in each cube of `spacing` (from `min`), about `origin`.
#[derive(Default)]
struct Keep {
    scale: f64,
    spacing: f64,
    min: [f64; 3],
    origin: [f64; 3],
    colour: Option<Shade>,
    /// The file's range of intensity, when its points have one that varies.
    bright: Option<(f64, f64)>,
    cells: HashSet<[i64; 3]>,
    points: Vec<[f32; 3]>,
    colours: Vec<[u8; 3]>,
    levels: Vec<u8>,
}

impl Keep {
    fn push(&mut self, raw: &Raw) {
        let p = raw.p.map(|v| v * self.scale);
        if self.spacing > 0.0 {
            let cell = [0, 1, 2].map(|k| ((p[k] - self.min[k]) / self.spacing).floor() as i64);
            if !self.cells.insert(cell) {
                return;
            }
        }
        self.points.push([0, 1, 2].map(|k| (p[k] - self.origin[k]) as f32));
        if let Some(c) = &self.colour {
            self.colours.push(c.of(raw));
        }
        if let Some((lo, hi)) = self.bright {
            let v = raw.intensity.filter(|v| v.is_finite()).map_or(0.0, |v| (v - lo) / (hi - lo));
            self.levels.push((v * 255.0).round().clamp(0.0, 255.0) as u8);
        }
    }
}

/// A point as a file holds it: in the file's unit, and its intensity and colour in the file's own range.
pub(super) struct Raw {
    p: [f64; 3],
    intensity: Option<f64>,
    rgb: Option<[f64; 3]>,
}

/// What the first pass learns: how many points, how far they reach, and the ranges of their shades.
struct Stats {
    count: usize,
    min: [f64; 3],
    max: [f64; 3],
    intensity: Option<(f64, f64)>,
    rgb: Option<f64>,
    fraction: bool,
}

impl Default for Stats {
    fn default() -> Stats {
        Stats { count: 0, min: [f64::INFINITY; 3], max: [f64::NEG_INFINITY; 3], intensity: None, rgb: None, fraction: false }
    }
}

impl Stats {
    fn add(&mut self, raw: &Raw, scale: f64) -> Result<(), String> {
        for k in 0..3 {
            let v = raw.p[k] * scale;
            if !v.is_finite() {
                return Err(format!("point {} isn't a number along {}", self.count + 1, ["x", "y", "z"][k]));
            }
            if v.abs() > MAX_FILE_COORD {
                return Err(format!("point {} lies {v:.0} mm from the file's origin, farther than {MAX_FILE_COORD} mm", self.count + 1));
            }
            self.min[k] = self.min[k].min(v);
            self.max[k] = self.max[k].max(v);
        }
        if let Some(i) = raw.intensity.filter(|i| i.is_finite()) {
            let (lo, hi) = self.intensity.unwrap_or((i, i));
            self.intensity = Some((lo.min(i), hi.max(i)));
        }
        if let Some(rgb) = raw.rgb {
            for v in rgb.into_iter().filter(|v| v.is_finite()) {
                self.rgb = Some(self.rgb.unwrap_or(0.0).max(v));
                self.fraction |= v.fract() != 0.0;
            }
        }
        self.count += 1;
        Ok(())
    }

    /// How the points are shaded, or none when the file holds neither colour nor intensity.
    fn colour(&self) -> Option<Shade> {
        if self.rgb.is_none() && self.intensity.is_none() {
            return None;
        }
        // Sixteen bits, nought to one, or eight bits: what the brightest colour says.
        let full = match self.rgb {
            Some(m) if m > 255.0 => 65535.0,
            Some(m) if m <= 1.0 && self.fraction => 1.0,
            _ => 255.0,
        };
        Some(Shade { full, intensity: self.intensity.unwrap_or((0.0, 0.0)) })
    }
}

/// Turns a point's colour, or its intensity, into eight-bit RGB.
struct Shade {
    full: f64,
    intensity: (f64, f64),
}

/// The grey of a point with neither colour nor intensity: in a file whose others have one, and in a view, of a scan with none.
pub const GREY: u8 = 160;

impl Shade {
    fn of(&self, raw: &Raw) -> [u8; 3] {
        let byte = |v: f64| if v.is_finite() { (v * 255.0).round().clamp(0.0, 255.0) as u8 } else { GREY };
        if let Some(rgb) = raw.rgb {
            return rgb.map(|v| byte(v / self.full));
        }
        match raw.intensity {
            Some(i) => {
                let (lo, hi) = self.intensity;
                let g = if hi > lo { byte((i - lo) / (hi - lo)) } else { GREY };
                [g; 3]
            }
            None => [GREY; 3],
        }
    }
}

/// Hands each point of a file to `f` with its place among the file's points, in the file's order. A reader that can step
/// over records (LAS, binary PLY) hands only every `every`th; the others hand every point and leave the choosing to `f`.
fn each(
    src: &mut dyn Source,
    size: u64,
    format: Format,
    every: u64,
    f: &mut impl FnMut(u64, Raw) -> Result<(), String>,
) -> Result<(), String> {
    src.seek(SeekFrom::Start(0)).map_err(unread)?;
    let mut n = 0_u64;
    let mut counted = |raw| {
        let at = n;
        n += 1;
        f(at, raw)
    };
    match format {
        Format::Pts | Format::Xyz => text(&mut BufReader::with_capacity(BLOCK, src), format == Format::Pts, &mut counted),
        Format::Ply => ply(src, size, every, f),
        Format::Las | Format::Laz => las(src, size, every, f),
        Format::E57 => e57::read(src, size, &mut counted),
    }
}

/// What a read that failed says.
fn unread(e: std::io::Error) -> String {
    format!("it can't be read: {e}")
}

/// The next line of `r` in `buf`, without its end; false at the end of the file. A line longer than [`MAX_LINE`] is refused
/// rather than held.
fn line(r: &mut impl BufRead, buf: &mut Vec<u8>, n: usize) -> Result<bool, String> {
    buf.clear();
    if r.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', buf).map_err(unread)? == 0 {
        return Ok(false);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    } else if buf.len() > MAX_LINE {
        return Err(format!("line {n} is longer than {MAX_LINE} bytes, far more than a point"));
    }
    Ok(true)
}

/// Hands `count` records of `size` bytes from byte `start` (which the caller has found within the file) to `f`, with their
/// place: every `every`th of them, stepped to one by one when they lie far apart, else read a block at a time.
fn records(
    src: &mut dyn Source,
    start: u64,
    count: u64,
    size: usize,
    every: u64,
    f: &mut impl FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if size == 0 || count == 0 {
        return Ok(());
    }
    let every = every.max(1);
    let cut = |e: std::io::Error| format!("its points are cut short: {e}");
    let wide = size as u64;
    if every > 1 && every.saturating_mul(wide) >= (BLOCK as u64) / 16 {
        let mut r = vec![0; size];
        let mut i = 0;
        while i < count {
            src.seek(SeekFrom::Start(start.saturating_add(i.saturating_mul(wide)))).map_err(cut)?;
            src.read_exact(&mut r).map_err(cut)?;
            f(i, &r)?;
            i = i.saturating_add(every);
        }
        return Ok(());
    }
    src.seek(SeekFrom::Start(start)).map_err(cut)?;
    let per = (BLOCK / size).max(1) as u64;
    let mut block = Vec::new();
    let mut i = 0;
    while i < count {
        let n = per.min(count - i);
        block.resize(n as usize * size, 0);
        src.read_exact(&mut block).map_err(cut)?;
        for (k, r) in block.chunks_exact(size).enumerate() {
            let at = i + k as u64;
            if at.is_multiple_of(every) {
                f(at, r)?;
            }
        }
        i += n;
    }
    Ok(())
}

/// Rows of `x y z`, then an intensity, a colour, or both, parted by spaces, tabs or commas. In PTS a row of one
/// number is the count of the scan that follows.
fn text(r: &mut impl BufRead, pts: bool, f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
    let mut buf = Vec::new();
    let mut n = 0;
    while line(r, &mut buf, n + 1)? {
        n += 1;
        let line = std::str::from_utf8(&buf).map_err(|_| format!("line {n} isn't text"))?.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let words: Vec<&str> = line.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        if pts && words.len() == 1 && words.first().is_some_and(|w| w.parse::<u64>().is_ok()) {
            continue;
        }
        let mut v = [0.0; 7];
        for (k, w) in words.iter().take(7).enumerate() {
            let x: f64 = w.parse().map_err(|_| format!("line {n}: {w} isn't a number"))?;
            if !x.is_finite() {
                return Err(format!("line {n}: {w} isn't a number BIMCraft can place"));
            }
            v[k] = x;
        }
        let [x, y, z, a, b, c, d] = v;
        let (intensity, rgb) = match words.len() {
            3 => (None, None),
            4 => (Some(a), None),
            6 => (None, Some([a, b, c])),
            7 => (Some(a), Some([b, c, d])),
            k => return Err(format!("line {n}: {k} columns; a point is x y z, then an intensity, a colour (r g b), or both")),
        };
        f(Raw { p: [x, y, z], intensity, rgb })?;
    }
    Ok(())
}

/// A number type a PLY property may be.
#[derive(Clone, Copy)]
enum Num {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Num {
    fn of(name: &str) -> Option<Num> {
        Some(match name {
            "char" | "int8" => Num::I8,
            "uchar" | "uint8" => Num::U8,
            "short" | "int16" => Num::I16,
            "ushort" | "uint16" => Num::U16,
            "int" | "int32" => Num::I32,
            "uint" | "uint32" => Num::U32,
            "float" | "float32" => Num::F32,
            "double" | "float64" => Num::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Num::I8 | Num::U8 => 1,
            Num::I16 | Num::U16 => 2,
            Num::I32 | Num::U32 | Num::F32 => 4,
            Num::F64 => 8,
        }
    }

    fn float(self) -> bool {
        matches!(self, Num::F32 | Num::F64)
    }

    /// The value at the start of `b`, in the byte order `big` says.
    fn read(self, b: &[u8], big: bool) -> Option<f64> {
        fn arr<const N: usize>(b: &[u8], big: bool) -> Option<[u8; N]> {
            let mut a: [u8; N] = b.get(..N)?.try_into().ok()?;
            if big {
                a.reverse();
            }
            Some(a)
        }
        Some(match self {
            Num::I8 => f64::from(i8::from_le_bytes(arr(b, big)?)),
            Num::U8 => f64::from(u8::from_le_bytes(arr(b, big)?)),
            Num::I16 => f64::from(i16::from_le_bytes(arr(b, big)?)),
            Num::U16 => f64::from(u16::from_le_bytes(arr(b, big)?)),
            Num::I32 => f64::from(i32::from_le_bytes(arr(b, big)?)),
            Num::U32 => f64::from(u32::from_le_bytes(arr(b, big)?)),
            Num::F32 => f64::from(f32::from_le_bytes(arr(b, big)?)),
            Num::F64 => f64::from_le_bytes(arr(b, big)?),
        })
    }
}

/// An element of a PLY header: its name, how many, and its properties (a list has none of a fixed size).
struct Element {
    name: String,
    count: usize,
    props: Vec<(String, Option<Num>)>,
}

impl Element {
    /// Bytes an instance takes in a binary file, or none when a list makes it vary.
    fn stride(&self) -> Option<usize> {
        self.props.iter().map(|(_, t)| t.map(Num::size)).sum()
    }

    fn find(&self, names: &[&str]) -> Option<usize> {
        self.props.iter().position(|(n, _)| names.iter().any(|w| n.eq_ignore_ascii_case(w)))
    }
}

/// The longest a PLY header may be, bytes.
const PLY_HEADER: usize = 1 << 20;

/// The Polygon File Format: a text header, then its elements as text or binary. Only the vertices are read.
fn ply(src: &mut dyn Source, size: u64, every: u64, f: &mut impl FnMut(u64, Raw) -> Result<(), String>) -> Result<(), String> {
    let mut head = Vec::new();
    (&mut *src).take(PLY_HEADER as u64).read_to_end(&mut head).map_err(unread)?;
    if !head.starts_with(b"ply") {
        return Err("it isn't a PLY file: it doesn't start with ply".into());
    }
    let end = head.windows(10).position(|w| w == b"end_header").ok_or("its header has no end_header")?;
    let after = head.get(end..).unwrap_or(&[]);
    let body = (end + after.iter().position(|b| *b == b'\n').map_or(after.len(), |i| i + 1)) as u64;
    let header = std::str::from_utf8(head.get(..end).unwrap_or(&[])).map_err(|_| "its header isn't text")?;
    let mut form = None;
    let mut elements: Vec<Element> = Vec::new();
    for line in header.lines().skip(1).map(str::trim).filter(|l| !l.is_empty()) {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", f, _] => form = Some(*f),
            ["comment", ..] | ["obj_info", ..] => {}
            ["element", name, count] => {
                let count = count.parse().map_err(|_| format!("element {name} {count}: not a count"))?;
                elements.push(Element { name: (*name).into(), count, props: Vec::new() });
            }
            ["property", "list", _, _, name] => {
                let e = elements.last_mut().ok_or("a property before any element")?;
                e.props.push(((*name).into(), None));
            }
            ["property", t, name] => {
                let e = elements.last_mut().ok_or("a property before any element")?;
                let t = Num::of(t).ok_or_else(|| format!("property {name} is of type {t}, which PLY doesn't have"))?;
                e.props.push(((*name).into(), Some(t)));
            }
            _ => return Err(format!("its header says {line:?}, which isn't PLY")),
        }
    }
    let big = match form {
        Some("ascii") => None,
        Some("binary_little_endian") => Some(false),
        Some("binary_big_endian") => Some(true),
        Some(other) => return Err(format!("its format is {other}: PLY is ascii, binary_little_endian or binary_big_endian")),
        None => return Err("its header has no format".into()),
    };
    let at = elements.iter().position(|e| e.name == "vertex").ok_or("it has no vertex element")?;
    let (before, vertex) = elements.split_at(at);
    let vertex = vertex.first().ok_or("it has no vertex element")?;
    if vertex.props.iter().any(|(_, t)| t.is_none()) {
        return Err("its vertices hold a list, which a point can't".into());
    }
    let axes = [["x"], ["y"], ["z"]].map(|n| vertex.find(&n));
    let [Some(x), Some(y), Some(z)] = axes else {
        return Err(format!(
            "its vertices have no {}",
            ["x", "y", "z"].iter().zip(axes).find(|(_, a)| a.is_none()).map_or("x", |(n, _)| *n)
        ));
    };
    let rgb = [["red", "diffuse_red", "r"], ["green", "diffuse_green", "g"], ["blue", "diffuse_blue", "b"]].map(|n| vertex.find(&n));
    let rgb = match rgb {
        [Some(r), Some(g), Some(b)] => Some([r, g, b]),
        _ => None,
    };
    let intensity = vertex.find(&["intensity", "scalar_intensity"]);
    // A colour in floats runs from nought to one; it's handed on in eight bits' range.
    let float_rgb = rgb.is_some_and(|c| c.iter().any(|i| vertex.props.get(*i).and_then(|p| p.1).is_some_and(Num::float)));
    let point = |v: &dyn Fn(usize) -> Option<f64>| -> Option<Raw> {
        let up = if float_rgb { 255.0 } else { 1.0 };
        Some(Raw {
            p: [v(x)?, v(y)?, v(z)?],
            intensity: match intensity {
                Some(i) => Some(v(i)?),
                None => None,
            },
            rgb: match rgb {
                Some([r, g, b]) => Some([v(r)? * up, v(g)? * up, v(b)? * up]),
                None => None,
            },
        })
    };
    match big {
        None => {
            src.seek(SeekFrom::Start(body)).map_err(unread)?;
            let mut r = BufReader::with_capacity(BLOCK, &mut *src);
            let (mut buf, mut n) = (Vec::new(), 0);
            // The next line that isn't empty, or none at the end of the file.
            let mut next = |buf: &mut Vec<u8>| -> Result<bool, String> {
                loop {
                    n += 1;
                    if !line(&mut r, buf, n)? {
                        return Ok(false);
                    }
                    let trimmed = buf.trim_ascii();
                    if !trimmed.is_empty() {
                        *buf = trimmed.to_vec();
                        return Ok(true);
                    }
                }
            };
            for e in before {
                for _ in 0..e.count {
                    if !next(&mut buf)? {
                        return Err(format!("it's cut short in its {} elements", e.name));
                    }
                }
            }
            for n in 0..vertex.count {
                if !next(&mut buf)? {
                    return Err(format!("it's cut short: {} vertices of {}", n, vertex.count));
                }
                let line = std::str::from_utf8(&buf).map_err(|_| format!("vertex {} isn't text", n + 1))?;
                let words: Vec<f64> = line
                    .split_whitespace()
                    .map(|w| w.parse::<f64>().map_err(|_| format!("vertex {}: {w} isn't a number", n + 1)))
                    .collect::<Result<_, _>>()?;
                let raw = point(&|i| words.get(i).copied()).ok_or_else(|| format!("vertex {} has too few values", n + 1))?;
                f(n as u64, raw)?;
            }
        }
        Some(big) => {
            let mut skip = 0_usize;
            for e in before {
                let stride = e
                    .stride()
                    .ok_or_else(|| format!("its {} elements, before its vertices, hold a list, which BIMCraft can't step over", e.name))?;
                skip = e.count.checked_mul(stride).and_then(|b| b.checked_add(skip)).ok_or("it's cut short")?;
            }
            let stride = vertex.stride().unwrap_or(0);
            let offsets: Vec<usize> = vertex
                .props
                .iter()
                .scan(0, |o, (_, t)| {
                    let at = *o;
                    *o += t.map_or(0, Num::size);
                    Some(at)
                })
                .collect();
            let start = body.checked_add(skip as u64);
            let fits = vertex.count.checked_mul(stride).and_then(|need| start?.checked_add(need as u64)).is_some_and(|end| end <= size);
            let (Some(start), true) = (start, fits) else {
                return Err(format!("it's cut short: {} vertices don't fit in it", vertex.count));
            };
            records(src, start, vertex.count as u64, stride, every, &mut |at, row| {
                let v = |i: usize| -> Option<f64> {
                    let t = vertex.props.get(i)?.1?;
                    t.read(row.get(*offsets.get(i)?..)?, big)
                };
                f(at, point(&v).ok_or("a vertex can't be read")?)
            })?;
        }
    }
    Ok(())
}

/// The smallest record each LAS point format takes, bytes, from format 0 to 10.
const LAS_RECORD: [usize; 11] = [20, 28, 26, 34, 57, 63, 30, 36, 38, 59, 67];

/// What a LAS file's header says of its points, as far as they are read; a LAZ file's too.
pub(super) struct LasHead {
    /// Where its records of its own start: right after the header, bytes.
    #[cfg_attr(not(feature = "laz"), allow(dead_code))]
    pub(super) header: u64,
    /// Where its points start, bytes.
    pub(super) start: u64,
    /// How many records of its own it has.
    #[cfg_attr(not(feature = "laz"), allow(dead_code))]
    pub(super) vlrs: u32,
    /// Whether LASzip compressed its points (LAZ).
    pub(super) compressed: bool,
    /// How long each record is, bytes.
    pub(super) len: usize,
    /// How many points it holds.
    pub(super) count: u64,
    scale: [f64; 3],
    offset: [f64; 3],
    /// Where in a record its colour is, if the format has one.
    colour: Option<usize>,
}

impl LasHead {
    /// The 375 bytes at most of a LAS file's header, read from the start of `src`, checked against the file's `size`.
    fn read(src: &mut dyn Source, size: u64) -> Result<LasHead, String> {
        let mut head = Vec::new();
        (&mut *src).take(375).read_to_end(&mut head).map_err(unread)?;
        let bytes = head.as_slice();
        if !bytes.starts_with(b"LASF") {
            return Err("it isn't a LAS file: it doesn't start with LASF".into());
        }
        let short = || "its header is cut short".to_string();
        let u8_at = |i: usize| bytes.get(i).copied().ok_or_else(short);
        let u16_at = |i: usize| bytes.get(i..i + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).ok_or_else(short);
        let u32_at = |i: usize| bytes.get(i..i + 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes).ok_or_else(short);
        let u64_at = |i: usize| bytes.get(i..i + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).ok_or_else(short);
        let f64_at = |i: usize| bytes.get(i..i + 8).and_then(|b| b.try_into().ok()).map(f64::from_le_bytes).ok_or_else(short);
        if bytes.len() < 227 {
            return Err(short());
        }
        let (major, minor) = (u8_at(24)?, u8_at(25)?);
        if major != 1 || minor > 4 {
            return Err(format!("it's LAS {major}.{minor}; BIMCraft reads 1.0 to 1.4"));
        }
        let header = u64::from(u16_at(94)?);
        if header < 227 || header > size {
            return Err(short());
        }
        let start = u64::from(u32_at(96)?);
        let vlrs = u32_at(100)?;
        // LASzip marks a compressed file's point format with its top bit (and the one below it, in its compatibility mode).
        let byte = u8_at(104)?;
        let (format, compressed) = (byte & 0x3F, byte & 0xC0 != 0);
        let need = *LAS_RECORD.get(usize::from(format)).ok_or_else(|| format!("its point format {format} isn't one LAS has (0 to 10)"))?;
        let len = usize::from(u16_at(105)?);
        if len < need {
            return Err(format!("its point records are {len} bytes; point format {format} needs {need}"));
        }
        let legacy = u64::from(u32_at(107)?);
        let count = if minor >= 4 && header >= 255 && (legacy == 0 || format >= 6) { u64_at(247)? } else { legacy };
        let mut scale = [0.0; 3];
        let mut offset = [0.0; 3];
        for k in 0..3 {
            scale[k] = f64_at(131 + 8 * k)?;
            offset[k] = f64_at(155 + 8 * k)?;
            if !(scale[k].is_finite() && scale[k] != 0.0) {
                return Err(format!("its scale along {} is {}", ["x", "y", "z"][k], scale[k]));
            }
            if !offset[k].is_finite() {
                return Err(format!("its offset along {} is {}", ["x", "y", "z"][k], offset[k]));
            }
        }
        if start < header || start > size {
            return Err(format!("its points are cut short: they start at byte {start} of {size}"));
        }
        let colour = match format {
            2 => Some(20),
            3 | 5 => Some(28),
            7 | 8 | 10 => Some(30),
            _ => None,
        };
        Ok(LasHead { header, start, vlrs, compressed, len, count, scale, offset, colour })
    }

    /// The point a record of this file holds.
    pub(super) fn point(&self, r: &[u8]) -> Result<Raw, String> {
        let i32_at = |i: usize| r.get(i..i + 4).and_then(|b| b.try_into().ok()).map(i32::from_le_bytes).map(f64::from);
        let u16_in = |i: usize| r.get(i..i + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).map(f64::from);
        let xyz = [i32_at(0), i32_at(4), i32_at(8)];
        let [Some(x), Some(y), Some(z)] = xyz else {
            return Err("a point record is cut short".into());
        };
        let (scale, offset) = (self.scale, self.offset);
        let p = [x * scale[0] + offset[0], y * scale[1] + offset[1], z * scale[2] + offset[2]];
        let rgb = match self.colour.map(|at| [u16_in(at), u16_in(at + 2), u16_in(at + 4)]) {
            Some([Some(r), Some(g), Some(b)]) => Some([r, g, b]),
            _ => None,
        };
        Ok(Raw { p, intensity: u16_in(12), rgb })
    }
}

/// ASPRS LAS 1.0 to 1.4: a header, records of its own, then the points, each scaled and moved by the header. A file whose
/// points LASzip compressed (LAZ, COPC) is read by `laz` (without the `laz` feature, refused), whatever its name says.
fn las(src: &mut dyn Source, size: u64, every: u64, f: &mut impl FnMut(u64, Raw) -> Result<(), String>) -> Result<(), String> {
    let h = LasHead::read(src, size)?;
    if h.compressed {
        #[cfg(feature = "laz")]
        return laz::read(src, size, &h, f);
        #[cfg(not(feature = "laz"))]
        return Err(NO_LAZ.into());
    }
    let (start, count, len) = (h.start, h.count, h.len);
    let end = count.checked_mul(len as u64).and_then(|b| b.checked_add(start));
    if !end.is_some_and(|end| end <= size) {
        return Err(format!("its points are cut short: {count} of {len} bytes from byte {start} don't fit in {size} bytes"));
    }
    records(src, start, count, len, every, &mut |at, r| f(at, h.point(r)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mm() -> Options {
        Options { scale: 1.0, ..Options::default() }
    }

    fn near(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() <= tol)
    }

    fn all(s: &Scan) -> Vec<[f64; 3]> {
        (0..s.points.len()).filter_map(|i| s.absolute(i)).collect()
    }

    /// A LAS file: `minor` version, point format, scale and offset, and each point's record numbers, its
    /// intensity and its colour; `gap` bytes of variable length records between the header and the points.
    pub(super) fn las(minor: u8, format: u8, scale: [f64; 3], offset: [f64; 3], gap: usize, pts: &[([i32; 3], u16, [u16; 3])]) -> Vec<u8> {
        let header = if minor >= 4 { 375 } else { 227 };
        let len: usize = match format {
            0 => 20,
            1 => 28,
            2 => 26,
            3 => 34,
            6 => 30,
            7 => 36,
            _ => panic!("no such format in these tests"),
        };
        let mut b = vec![0u8; header + gap];
        b[0..4].copy_from_slice(b"LASF");
        b[24] = 1;
        b[25] = minor;
        b[94..96].copy_from_slice(&(header as u16).to_le_bytes());
        b[96..100].copy_from_slice(&((header + gap) as u32).to_le_bytes());
        b[104] = format;
        b[105..107].copy_from_slice(&(len as u16).to_le_bytes());
        if format >= 6 {
            b[247..255].copy_from_slice(&(pts.len() as u64).to_le_bytes());
        } else {
            b[107..111].copy_from_slice(&(pts.len() as u32).to_le_bytes());
        }
        for k in 0..3 {
            b[131 + 8 * k..139 + 8 * k].copy_from_slice(&scale[k].to_le_bytes());
            b[155 + 8 * k..163 + 8 * k].copy_from_slice(&offset[k].to_le_bytes());
        }
        let colour = match format {
            2 => Some(20),
            3 => Some(28),
            7 => Some(30),
            _ => None,
        };
        for (xyz, intensity, rgb) in pts {
            let mut r = vec![0u8; len];
            for k in 0..3 {
                r[4 * k..4 * k + 4].copy_from_slice(&xyz[k].to_le_bytes());
            }
            r[12..14].copy_from_slice(&intensity.to_le_bytes());
            if let Some(at) = colour {
                for k in 0..3 {
                    r[at + 2 * k..at + 2 * k + 2].copy_from_slice(&rgb[k].to_le_bytes());
                }
            }
            b.extend(r);
        }
        b
    }

    /// A PLY file of points with a normal's x between them and their colour, and an empty face element after.
    fn ply(form: &str, pts: &[([f32; 3], [u8; 3])]) -> Vec<u8> {
        let mut b = format!(
            "ply\r\nformat {form} 1.0\ncomment made by hand\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\n\
             property float nx\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 0\n\
             property list uchar int vertex_indices\nend_header\n",
            pts.len()
        )
        .into_bytes();
        for (p, c) in pts {
            match form {
                "ascii" => b.extend(format!("{} {} {} 0.5 {} {} {}\n", p[0], p[1], p[2], c[0], c[1], c[2]).bytes()),
                _ => {
                    for v in [p[0], p[1], p[2], 0.5] {
                        b.extend(if form == "binary_little_endian" { v.to_le_bytes() } else { v.to_be_bytes() });
                    }
                    b.extend(c);
                }
            }
        }
        b
    }

    #[test]
    fn formats_come_from_extensions_and_the_ones_it_cant_read_say_why() {
        let known = [
            ("pts", Format::Pts),
            ("XYZ", Format::Xyz),
            ("txt", Format::Xyz),
            ("ply", Format::Ply),
            (".las", Format::Las),
            ("E57", Format::E57),
            #[cfg(feature = "laz")]
            ("laz", Format::Laz),
            #[cfg(feature = "laz")]
            ("LAZ", Format::Laz),
        ];
        for (ext, f) in known {
            assert_eq!(Format::from_extension(ext), Ok(f), "{ext}");
        }
        #[cfg(not(feature = "laz"))]
        assert!(Format::from_extension("laz").unwrap_err().contains("`laz` feature"));
        for (ext, says) in [("rcp", "ReCap"), ("rcs", "ReCap"), ("dwg", "dwg")] {
            let e = Format::from_extension(ext).unwrap_err();
            assert!(e.contains(says), "{ext}: {e}");
        }
    }

    #[test]
    fn units_scale_to_millimetres() {
        for (name, mm) in [("mm", 1.0), ("cm", 10.0), ("m", 1000.0), ("M", 1000.0), ("in", 25.4), ("ft", 304.8)] {
            assert_eq!(unit(name), Some(mm), "{name}");
        }
        assert_eq!(unit("furlong"), None);
        let s = read(b"1 2 3\n", Format::Xyz, &Options { scale: 304.8, ..Options::default() }).unwrap();
        assert!(near(s.absolute(0).unwrap(), [304.8, 609.6, 914.4], 1e-3), "{:?}", s.absolute(0));
    }

    #[test]
    fn pts_reads_points_their_intensity_and_their_colour() {
        // Two scans, each after its count; comments and blank lines between.
        let text = "2\n0 0 0 -2048 255 0 0\n1 0 0 2047 0 255 0\n\n# the second scan\n1\n0 1 0.5 0 0 0 255\n";
        let s = read(text.as_bytes(), Format::Pts, &mm()).unwrap();
        assert_eq!((s.read, s.points.len()), (3, 3));
        assert_eq!(all(&s), vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.5]]);
        assert_eq!(s.colours, Some(vec![[255, 0, 0], [0, 255, 0], [0, 0, 255]]));
        // Intensity alone is grey, from the darkest the file holds to the lightest.
        let s = read(b"3\n0 0 0 -2048\n1 0 0 0\n2 0 0 2047\n", Format::Pts, &mm()).unwrap();
        let grey: Vec<u8> = s.colours.unwrap().iter().map(|c| c[0]).collect();
        assert_eq!((grey[0], grey[2]), (0, 255));
        assert!((120..136).contains(&grey[1]), "{grey:?}");
    }

    #[test]
    fn xyz_reads_three_columns_or_six_parted_by_spaces_tabs_or_commas() {
        let s = read(b"0 0 0\n1\t2\t3\r\n4,5,6\n", Format::Xyz, &mm()).unwrap();
        assert_eq!(all(&s), vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(s.colours, None);
        // A colour in sixteen bits is brought down to eight.
        let s = read(b"0 0 0 65535 0 32896\n1 1 1 0 65535 0\n", Format::Xyz, &mm()).unwrap();
        assert_eq!(s.colours, Some(vec![[255, 0, 128], [0, 255, 0]]));
        // And one from nought to one, up.
        let s = read(b"0 0 0 1 0 0.5\n", Format::Xyz, &mm()).unwrap();
        assert_eq!(s.colours, Some(vec![[255, 0, 128]]));
    }

    #[test]
    fn ply_as_text_and_in_either_byte_order_reads_the_same() {
        let pts = [([0.0, 0.0, 0.0], [255, 0, 0]), ([1.5, -2.0, 3.25], [10, 20, 30]), ([100.0, 0.0, 7.0], [0, 0, 255])];
        let text = read(&ply("ascii", &pts), Format::Ply, &mm()).unwrap();
        assert_eq!(all(&text), vec![[0.0, 0.0, 0.0], [1.5, -2.0, 3.25], [100.0, 0.0, 7.0]]);
        assert_eq!(text.colours, Some(vec![[255, 0, 0], [10, 20, 30], [0, 0, 255]]));
        for form in ["binary_little_endian", "binary_big_endian"] {
            assert_eq!(read(&ply(form, &pts), Format::Ply, &mm()).unwrap(), text, "{form}");
        }
    }

    #[test]
    fn las_applies_its_scale_and_offset_past_its_records() {
        // Survey coordinates in metres, a millimetre a step, far from the origin: they come in about their middle.
        let pts = [([0, 0, 0], 100, [65535, 0, 0]), ([10_000, 20_000, 3_000], 200, [0, 65535, 0])];
        let b = las(2, 2, [0.001; 3], [500_000.0, 7_000_000.0, 10.0], 54, &pts);
        let s = read(&b, Format::Las, &Options::default()).unwrap();
        assert_eq!(s.read, 2);
        assert!(near(s.absolute(0).unwrap(), [5e8, 7e9, 1e4], 1.0), "{:?}", s.absolute(0));
        assert!(near(s.absolute(1).unwrap(), [5e8 + 1e4, 7e9 + 2e4, 1.3e4], 1.0), "{:?}", s.absolute(1));
        assert!(near(s.origin, [5e8 + 5e3, 7e9 + 1e4, 1.15e4], 1e-6), "{:?}", s.origin);
        assert_eq!(s.colours, Some(vec![[255, 0, 0], [0, 255, 0]]));
        // A format with no colour shades by intensity; 1.3's header is 1.2's with a word more.
        let b = las(2, 1, [0.001; 3], [0.0; 3], 0, &[([0, 0, 0], 0, [0; 3]), ([1, 0, 0], 1000, [0; 3])]);
        assert_eq!(read(&b, Format::Las, &Options::default()).unwrap().colours, Some(vec![[0; 3], [255; 3]]));
    }

    #[test]
    fn las_14_counts_in_64_bits_and_its_new_formats_carry_colour() {
        let pts = [([0, 0, 0], 0, [255, 128, 0]), ([1000, 0, 0], 0, [0, 0, 255])];
        let s = read(&las(4, 7, [0.001; 3], [0.0; 3], 0, &pts), Format::Las, &Options::default()).unwrap();
        assert_eq!(all(&s), vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0]]);
        assert_eq!(s.colours, Some(vec![[255, 128, 0], [0, 0, 255]]));
        let s = read(&las(4, 6, [0.01; 3], [0.0; 3], 0, &pts), Format::Las, &Options::default()).unwrap();
        assert_eq!(all(&s), vec![[0.0, 0.0, 0.0], [10_000.0, 0.0, 0.0]]);
    }

    #[test]
    fn a_survey_far_off_keeps_its_millimetres_about_its_middle() {
        // 4 km by 3 km of UTM coordinates in metres, 10^10 mm from the origin.
        let text = "500000.001 7000000.002 12.003\n504000.004 7003000.005 52.006\n";
        let s = read(text.as_bytes(), Format::Xyz, &Options::default()).unwrap();
        assert!(near(s.absolute(0).unwrap(), [500_000_001.0, 7_000_000_002.0, 12_003.0], 1.0));
        assert!(near(s.absolute(1).unwrap(), [504_000_004.0, 7_003_000_005.0, 52_006.0], 1.0));
        assert!(s.points.iter().flatten().all(|v| v.abs() <= 2_000_002.0));
    }

    #[test]
    fn thinning_is_the_same_every_time_and_never_keeps_more_than_the_cap() {
        // 1000 points a millimetre apart, in a cube of ten.
        let text: String = (0..1000).map(|i| format!("{} {} {}\n", i % 10, i / 10 % 10, i / 100)).collect();
        let keep = |spacing: f64, cap: usize| read(text.as_bytes(), Format::Xyz, &Options { scale: 1.0, spacing, cap }).unwrap();
        assert_eq!(keep(0.0, MAX_CLOUD_POINTS).points.len(), 1000);
        // One in each cube of 2 mm: five to a side.
        let thin = keep(2.0, MAX_CLOUD_POINTS);
        assert_eq!((thin.read, thin.points.len()), (1000, 125));
        assert_eq!(thin, keep(2.0, MAX_CLOUD_POINTS));
        // Past the cap, every so many, whether or not it's thinned too.
        for (spacing, cap) in [(0.0, 100), (0.0, 999), (2.0, 50), (0.0, 1)] {
            let s = keep(spacing, cap);
            assert!(!s.points.is_empty() && s.points.len() <= cap, "{spacing} {cap}: {}", s.points.len());
            assert_eq!(s, keep(spacing, cap));
        }
    }

    #[test]
    fn hostile_files_are_refused_and_never_crash_it() {
        let o = Options::default();
        for (bytes, format, says) in [
            (&b""[..], Format::Xyz, "no points"),
            (b"# nothing but this\n", Format::Pts, "no points"),
            (b"0 0 zero\n", Format::Xyz, "line 1"),
            (b"0 0 0\n0 0 nan\n", Format::Xyz, "line 2"),
            (b"0 0 1e400\n", Format::Xyz, "line 1"),
            (b"0 0 1e20\n", Format::Xyz, "far"),
            (b"0 0 0 1 2\n", Format::Xyz, "columns"),
            (b"0 0 0\n30000 0 0\n", Format::Xyz, "wide"),
            (b"", Format::Ply, "PLY"),
            (b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\n", Format::Ply, "end_header"),
            (b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float y\nproperty float z\nend_header\n0 0\n", Format::Ply, "x"),
            (b"ply\nformat ascii 1.0\nelement vertex -1\nproperty float x\nproperty float y\nproperty float z\nend_header\n", Format::Ply, "count"),
            (b"ply\nformat ascii 1.0\nelement vertex 5\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n", Format::Ply, "short"),
            (b"ply\nformat binary_little_endian 1.0\nelement vertex 99999999999\nproperty float x\nproperty float y\nproperty float z\nend_header\n", Format::Ply, "short"),
            (b"ply\nformat ascii 1.0\nelement vertex 1\nproperty quaternion x\nend_header\n0\n", Format::Ply, "quaternion"),
            (b"ply\nformat ascii 1.0\nelement vertex 1\nproperty list uchar float x\nend_header\n", Format::Ply, "list"),
            (b"ply\nformat binary_little_endian 1.0\nelement face 1\nproperty list uchar int i\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\n", Format::Ply, "list"),
            (b"ply\nformat ebcdic 1.0\nend_header\n", Format::Ply, "ebcdic"),
            (b"LASF", Format::Las, "short"),
            (b"PK\x03\x04", Format::Las, "LAS"),
        ] {
            let e = read(bytes, format, &o).unwrap_err();
            assert!(e.contains(says), "{says}: {e}");
        }
        let one = [([0, 0, 0], 0, [0; 3])];
        let good = las(2, 2, [0.001; 3], [0.0; 3], 0, &one);
        let mut laz = good.clone();
        laz[104] |= 0x80;
        let mut short = good.clone();
        short[105..107].copy_from_slice(&10u16.to_le_bytes());
        let mut many = good.clone();
        many[107..111].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut past = good.clone();
        past[96..100].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut unknown = good.clone();
        unknown[104] = 11;
        let mut flat = good.clone();
        flat[131..139].copy_from_slice(&0.0f64.to_le_bytes());
        let mut nan = good.clone();
        nan[163..171].copy_from_slice(&f64::NAN.to_le_bytes());
        for (bytes, says) in [
            (laz, if cfg!(feature = "laz") { "LASzip" } else { "`laz` feature" }),
            (short, "record"),
            (many, "short"),
            (past, "short"),
            (unknown, "format 11"),
            (flat, "scale"),
            (nan, "offset"),
            (good[..200].to_vec(), "short"),
            (good[..good.len() - 1].to_vec(), "short"),
        ] {
            let e = read(&bytes, Format::Las, &o).unwrap_err();
            assert!(e.contains(says), "{says}: {e}");
        }
        // Options that make no sense are said, not acted on.
        for bad in [Options { scale: 0.0, ..o }, Options { scale: f64::NAN, ..o }, Options { spacing: -1.0, ..o }, Options { cap: 0, ..o }]
        {
            assert!(read(b"0 0 0\n", Format::Xyz, &bad).is_err(), "{bad:?}");
        }
        // Noise, in every format and of every length, is an error or points, never a panic.
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut noise = |n: usize| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    seed as u8
                })
                .collect()
        };
        for n in [0, 1, 4, 100, 227, 375, 1000, 5000] {
            for f in [Format::Pts, Format::Xyz, Format::Ply, Format::Las] {
                let mut bytes = noise(n);
                let _ = read(&bytes, f, &o);
                // Behind a real header too, where the parser gets further.
                let head: &[u8] = match f {
                    Format::Ply => b"ply\nformat binary_big_endian 1.0\nelement vertex 40\nproperty double x\nproperty double y\nproperty double z\nproperty ushort red\nproperty ushort green\nproperty ushort blue\nend_header\n",
                    Format::Las => &good[..227],
                    _ => b"",
                };
                bytes.splice(0..0, head.iter().copied());
                let _ = read(&bytes, f, &o);
            }
        }
    }

    #[test]
    fn a_scan_in_a_project_file_comes_back_as_it_went_and_a_broken_one_is_refused() {
        let scan = read(b"0 0 0 255 0 0\n1 2 3 0 255 0\n", Format::Xyz, &Options::default()).unwrap();
        let cloud = Cloud::new("two.xyz", scan).unwrap();
        let text = serde_json::to_string(&cloud).unwrap();
        let back: Cloud = serde_json::from_str(&text).unwrap();
        assert_eq!(back, cloud);
        assert_eq!((back.len(), back.read(), back.source(), back.colours().map(<[_]>::len)), (2, 2, "two.xyz", Some(2)));
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        let with = |key: &str, v: serde_json::Value| {
            let mut f = file.clone();
            f[key] = v;
            serde_json::from_value::<Cloud>(f)
        };
        let packed = |bytes: &[u8]| serde_json::Value::from(crate::commands::base64(&deflate(bytes)));
        for (key, v, says) in [
            ("points", "!!!".into(), "base64"),
            ("points", crate::commands::base64(b"not zlib").into(), "zlib"),
            ("points", packed(&[0; 13]), "cut short"),
            ("points", packed(&[]), "points"),
            ("points", packed(&[f32::NAN.to_le_bytes(), [0; 4], [0; 4], [0; 4], [0; 4], [0; 4]].concat()), "past"),
            ("points", packed(&[1e9f32.to_le_bytes(), [0; 4], [0; 4], [0; 4], [0; 4], [0; 4]].concat()), "past"),
            ("colours", packed(&[1, 2, 3]), "colour"),
            ("read", 1.into(), "read"),
            ("origin", serde_json::json!([1e13, 0, 0]), "origin"),
            ("extra", 1.into(), "unknown"),
            ("stations", serde_json::json!([{"source": "a", "points": 1}, {"source": "b", "points": 2}]), "stations"),
            ("stations", serde_json::json!([{"source": "a", "points": usize::MAX}, {"source": "b", "points": 3}]), "stations"),
        ] {
            let e = with(key, v).unwrap_err().to_string();
            assert!(e.contains(says), "{key}: {e}");
        }
        // Data that unpacks past what a cloud may hold stops at it.
        let e = inflate(&crate::commands::base64(&deflate(&vec![0; 1000])), 999).unwrap_err();
        assert!(e.contains("more than 999"), "{e}");
    }

    /// A project file's `clouds` object: `n` scans of two points, then `tail` (more entries, as JSON members).
    fn scans_text(n: usize, tail: &str) -> String {
        let scan = read(b"0 0 0\n1 0 0\n", Format::Xyz, &Options::default()).unwrap();
        let one = serde_json::to_string(&Cloud::new("a.xyz", scan).unwrap()).unwrap();
        let members: Vec<String> = (0..n).map(|i| format!("\"s{i}\":{one}")).collect();
        format!("{{{}{}{tail}}}", members.join(","), if n > 0 && !tail.is_empty() { "," } else { "" })
    }

    fn scans_in(text: &str, most: usize, points: usize) -> Result<usize, String> {
        scans_within(&mut serde_json::Deserializer::from_str(text), most, points).map(|m| m.len()).map_err(|e| e.to_string())
    }

    #[test]
    fn a_project_file_holds_no_more_scans_or_points_than_a_project_may_and_says_so_before_unpacking_them() {
        assert_eq!(scans_in(&scans_text(2, ""), 2, 4), Ok(2));
        assert_eq!(scans_in("{}", 2, 4), Ok(0));
        // The entry past the count is never looked at: its data would be an error of another kind.
        let e = scans_in(&scans_text(2, "\"s2\":{\"points\":\"!!!\"}"), 2, 100).unwrap_err();
        assert!(e.contains("more than 2 scans"), "{e}");
        // The scan that takes the points past the budget is the one refused; the ones before it were within it.
        let e = scans_in(&scans_text(3, ""), 16, 5).unwrap_err();
        assert!(e.contains("more than 5 points"), "{e}");
        assert_eq!(scans_in(&scans_text(2, ""), 16, 4), Ok(2));
        // The real caps are the project's.
        assert_eq!(scans_in(&scans_text(MAX_CLOUDS, ""), MAX_CLOUDS, MAX_CLOUD_POINTS), Ok(MAX_CLOUDS));
        // Something that isn't a map of scans is no map of scans.
        assert!(scans_in("[1]", 2, 4).is_err() && scans_in("{\"s\":1}", 2, 4).is_err());
    }

    /// A reader that counts the bytes read through it, as a file on disk would hand them.
    struct Counted<R> {
        inner: R,
        read: u64,
    }

    impl<R: Read> Read for Counted<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counted<R> {
        fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(to)
        }
    }

    fn sample_files() -> Vec<(Format, Vec<u8>)> {
        let pts: Vec<([f32; 3], [u8; 3])> = (0..500).map(|i| ([i as f32, (i * 7 % 13) as f32, 0.5 * i as f32], [i as u8, 3, 9])).collect();
        let records: Vec<([i32; 3], u16, [u16; 3])> =
            (0..500).map(|i| ([i, -i, 2 * i], i as u16, [256 * (i as u16 % 200), 0, 9])).collect();
        let text: String = (0..500).map(|i| format!("{i} {} 1.5 {} 10 20 30\n", i % 7, i % 100)).collect();
        vec![
            (Format::Pts, format!("500\n{text}").into_bytes()),
            (Format::Xyz, text.into_bytes()),
            (Format::Ply, ply("ascii", &pts)),
            (Format::Ply, ply("binary_little_endian", &pts)),
            (Format::Ply, ply("binary_big_endian", &pts)),
            (Format::Las, las(2, 3, [0.01; 3], [5.0, 6.0, 7.0], 33, &records)),
            (Format::Las, las(4, 7, [0.001; 3], [0.0; 3], 0, &records)),
        ]
    }

    #[test]
    fn a_file_read_from_disk_a_piece_at_a_time_reads_as_its_bytes_do() {
        let dir = tempfile::tempdir().unwrap();
        for (k, (format, bytes)) in sample_files().into_iter().enumerate() {
            for o in [mm(), Options { cap: 37, ..mm() }, Options { spacing: 3.0, ..mm() }] {
                let whole = read(&bytes, format, &o).unwrap();
                let path = dir.path().join(format!("{k}"));
                std::fs::write(&path, &bytes).unwrap();
                let mut file = std::fs::File::open(&path).unwrap();
                let piecewise = read_from(&mut file, format, &o, None).unwrap();
                assert_eq!(whole, piecewise, "{format:?} {k} {o:?}");
            }
        }
    }

    #[test]
    fn a_las_file_thinned_hard_reads_only_the_records_it_keeps_the_second_time() {
        // 400 000 records of 20 bytes, kept to 100: every 4000th, 80 000 bytes apart.
        let records: Vec<([i32; 3], u16, [u16; 3])> = (0..400_000).map(|i| ([i, i % 1000, 0], 0, [0; 3])).collect();
        let bytes = las(2, 0, [0.001; 3], [0.0; 3], 0, &records);
        let mut r = Counted { inner: std::io::Cursor::new(&bytes), read: 0 };
        let s = read_from(&mut r, Format::Las, &Options { cap: 100, ..mm() }, None).unwrap();
        assert_eq!((s.points.len(), s.read), (100, 400_000));
        assert_eq!(s, read(&bytes, Format::Las, &Options { cap: 100, ..mm() }).unwrap());
        // One pass to measure it, and of the second only the records kept.
        assert!(r.read < bytes.len() as u64 * 11 / 10, "{} bytes read of a file of {}", r.read, bytes.len());
    }

    #[test]
    fn a_line_longer_than_any_point_is_refused_not_held() {
        let long = format!("1 2 3\n{}\n", "1".repeat(MAX_LINE + 10));
        let e = read(long.as_bytes(), Format::Xyz, &mm()).unwrap_err();
        assert!(e.contains("line 2") && e.contains("longer"), "{e}");
    }

    #[test]
    fn a_file_larger_than_memory_need_be_is_no_longer_refused_for_its_size() {
        const { assert!(MAX_BYTES >= 1 << 40, "a scan's size is capped below a terabyte") };
    }
}
