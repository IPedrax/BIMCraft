//! Point clouds: what a laser scan brings in. A file's points are read from public specifications
//! (PTS and XYZ text, PLY, ASPRS LAS 1.2 to 1.4), turned into millimetres, thinned to what a project
//! may hold, and kept about their centre as `f32`, which holds a millimetre across `MAX_SPAN`. A
//! project holds each scan once, as a [`Cloud`]; a `PointCloud` element places it.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::{Arc, OnceLock};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;

use serde::{Deserialize, Serialize};

/// Points a cloud may keep: a file with more is thinned down to it.
pub const MAX_CLOUD_POINTS: usize = 5_000_000;
/// What a file may be before it's refused, bytes.
pub const MAX_BYTES: usize = 2 << 30;
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
            "laz" => Err("LAZ is compressed LAS, which BIMCraft doesn't read: decompress it to LAS first (laszip or PDAL)".into()),
            "e57" => Err("E57 isn't read yet: export the scan as LAS, PLY or PTS".into()),
            "rcp" | "rcs" => Err("ReCap's RCP and RCS are closed formats: export the scan from ReCap as E57, LAS or PTS".into()),
            _ => Err(format!("a .{ext} file isn't a point cloud BIMCraft reads: PTS, XYZ, PLY or LAS")),
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
    let mut cells = HashSet::new();
    let (mut points, mut colours, mut stations, mut read) = (Vec::new(), Vec::new(), Vec::new(), 0_usize);
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
        }
        read = read.saturating_add(scan.read);
        stations.push(Station { source, points: points.len() - before });
    }
    Ok((Scan { origin, points, colours: coloured.then_some(colours), read }, stations))
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
    lo: [f32; 3],
    hi: [f32; 3],
    /// What a project file holds of it, made the first time it's saved.
    packed: Arc<OnceLock<CloudFile>>,
    /// Where its points are from above, made the first time something snaps to it.
    grid: Arc<OnceLock<Grid>>,
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
    }
}

impl Cloud {
    /// A scan read from the file `source` (its name, for people to know it by), or why it can't be held.
    pub fn new(source: &str, scan: Scan) -> Result<Cloud, String> {
        let Scan { origin, points, colours, read } = scan;
        Cloud::checked(source.chars().take(crate::model::MAX_TEXT).collect(), Vec::new(), origin, read, points, colours)
    }

    /// A scan brought in from several files, `stations`, or why it can't be held. It's known by the first.
    pub fn of_stations(scan: Scan, stations: Vec<Station>) -> Result<Cloud, String> {
        let Scan { origin, points, colours, read } = scan;
        let source = stations.first().map(|t| t.source.clone()).unwrap_or_default();
        Cloud::checked(source, stations, origin, read, points, colours)
    }

    fn checked(
        mut source: String,
        mut stations: Vec<Station>,
        origin: [f64; 3],
        read: usize,
        points: Vec<[f32; 3]>,
        colours: Option<Vec<[u8; 3]>>,
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
        if stations.len() == 1 {
            stations.clear();
        }
        if stations.len() > MAX_STATIONS {
            return Err(format!("a cloud comes from up to {MAX_STATIONS} files, not {}", stations.len()));
        }
        if !stations.is_empty() && stations.iter().map(|t| t.points).try_fold(0_usize, usize::checked_add) != Some(points.len()) {
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
            lo,
            hi,
            packed: Arc::default(),
            grid: Arc::default(),
        })
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
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    // Writing to a Vec doesn't fail.
    let _ = z.write_all(bytes);
    z.finish().unwrap_or_default()
}

/// The bytes base64 and zlib hide, if they're no more than `most`.
fn inflate(text: &str, most: usize) -> Result<Vec<u8>, String> {
    let packed = crate::texture::unbase64(text).ok_or("a cloud's data isn't base64")?;
    let mut out = Vec::new();
    ZlibDecoder::new(packed.as_slice())
        .take(most as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("a cloud's data isn't zlib: {e}"))?;
    if out.len() > most {
        return Err(format!("a cloud's data holds more than {most} bytes"));
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
        let cloud =
            Cloud::checked(f.source.chars().take(crate::model::MAX_TEXT).collect(), f.stations.clone(), f.origin, f.read, points, colours)?;
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

/// The points of a file of `format`, or what's wrong with it.
pub fn read(bytes: &[u8], format: Format, o: &Options) -> Result<Scan, String> {
    if !(o.scale.is_finite() && o.scale > 0.0) {
        return Err(format!("a unit of {} mm isn't one", o.scale));
    }
    if !(o.spacing.is_finite() && o.spacing >= 0.0) {
        return Err(format!("a spacing of {} mm isn't one", o.spacing));
    }
    if o.cap == 0 {
        return Err("a cloud keeps one point at least".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err(format!("it's {} bytes; up to {MAX_BYTES}", bytes.len()));
    }
    // ponytail: the file is parsed twice, once to measure it and once to keep its points; text files pay for it.
    let mut stats = Stats::default();
    each(bytes, format, &mut |raw| stats.add(&raw, o.scale))?;
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
    let colour = stats.colour();
    let mut cells = HashSet::new();
    let (mut points, mut colours, mut n) = (Vec::new(), Vec::new(), 0_usize);
    each(bytes, format, &mut |raw| {
        let at = n;
        n += 1;
        if at % stride != 0 {
            return Ok(());
        }
        let p = raw.p.map(|v| v * o.scale);
        if o.spacing > 0.0 {
            let cell = [0, 1, 2].map(|k| ((p[k] - stats.min[k]) / o.spacing).floor() as i64);
            if !cells.insert(cell) {
                return Ok(());
            }
        }
        points.push([0, 1, 2].map(|k| (p[k] - origin[k]) as f32));
        if let Some(c) = &colour {
            colours.push(c.of(&raw));
        }
        Ok(())
    })?;
    Ok(Scan { origin, points, colours: colour.map(|_| colours), read: stats.count })
}

/// A point as a file holds it: in the file's unit, and its intensity and colour in the file's own range.
struct Raw {
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

/// Hands each point of a file to `f`, in the file's order.
fn each(bytes: &[u8], format: Format, f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
    match format {
        Format::Pts | Format::Xyz => text(bytes, format == Format::Pts, f),
        Format::Ply => ply(bytes, f),
        Format::Las => las(bytes, f),
    }
}

/// Rows of `x y z`, then an intensity, a colour, or both, parted by spaces, tabs or commas. In PTS a row of one
/// number is the count of the scan that follows.
fn text(bytes: &[u8], pts: bool, f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
    for (n, line) in bytes.split(|b| *b == b'\n').enumerate() {
        let n = n + 1;
        let line = std::str::from_utf8(line).map_err(|_| format!("line {n} isn't text"))?.trim();
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
fn ply(bytes: &[u8], f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
    if !bytes.starts_with(b"ply") {
        return Err("it isn't a PLY file: it doesn't start with ply".into());
    }
    let head = bytes.get(..PLY_HEADER.min(bytes.len())).unwrap_or(bytes);
    let end = head.windows(10).position(|w| w == b"end_header").ok_or("its header has no end_header")?;
    let body = bytes.get(end..).unwrap_or(&[]);
    let body = body.get(body.iter().position(|b| *b == b'\n').map_or(body.len(), |i| i + 1)..).unwrap_or(&[]);
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
            let mut lines = body.split(|b| *b == b'\n').map(|l| l.trim_ascii()).filter(|l| !l.is_empty());
            for e in before {
                for _ in 0..e.count {
                    lines.next().ok_or_else(|| format!("it's cut short in its {} elements", e.name))?;
                }
            }
            for n in 0..vertex.count {
                let line = lines.next().ok_or_else(|| format!("it's cut short: {} vertices of {}", n, vertex.count))?;
                let line = std::str::from_utf8(line).map_err(|_| format!("vertex {} isn't text", n + 1))?;
                let words: Vec<f64> = line
                    .split_whitespace()
                    .map(|w| w.parse::<f64>().map_err(|_| format!("vertex {}: {w} isn't a number", n + 1)))
                    .collect::<Result<_, _>>()?;
                let raw = point(&|i| words.get(i).copied()).ok_or_else(|| format!("vertex {} has too few values", n + 1))?;
                f(raw)?;
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
            let need = vertex.count.checked_mul(stride).and_then(|b| b.checked_add(skip));
            let rows = need
                .and_then(|need| body.get(skip..need))
                .ok_or_else(|| format!("it's cut short: {} vertices don't fit in it", vertex.count))?;
            for row in rows.chunks_exact(stride.max(1)) {
                let v = |i: usize| -> Option<f64> {
                    let t = vertex.props.get(i)?.1?;
                    t.read(row.get(*offsets.get(i)?..)?, big)
                };
                f(point(&v).ok_or("a vertex can't be read")?)?;
            }
        }
    }
    Ok(())
}

/// The smallest record each LAS point format takes, bytes, from format 0 to 10.
const LAS_RECORD: [usize; 11] = [20, 28, 26, 34, 57, 63, 30, 36, 38, 59, 67];

/// ASPRS LAS 1.0 to 1.4: a header, records of its own, then the points, each scaled and moved by the header.
fn las(bytes: &[u8], f: &mut impl FnMut(Raw) -> Result<(), String>) -> Result<(), String> {
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
    let header = usize::from(u16_at(94)?);
    if header < 227 || header > bytes.len() {
        return Err(short());
    }
    let start = u32_at(96)? as usize;
    let format = u8_at(104)?;
    if format & 0xC0 != 0 {
        return Err("its points are compressed (LAZ): decompress it to LAS first (laszip or PDAL)".into());
    }
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
    let end = usize::try_from(count).ok().and_then(|c| c.checked_mul(len)).and_then(|b| b.checked_add(start));
    let points = end
        .filter(|_| start >= header)
        .and_then(|end| bytes.get(start..end))
        .ok_or_else(|| format!("its points are cut short: {count} of {len} bytes from byte {start} don't fit in {} bytes", bytes.len()))?;
    let colour = match format {
        2 => Some(20),
        3 | 5 => Some(28),
        7 | 8 | 10 => Some(30),
        _ => None,
    };
    for r in points.chunks_exact(len) {
        let i32_at = |i: usize| r.get(i..i + 4).and_then(|b| b.try_into().ok()).map(i32::from_le_bytes).map(f64::from);
        let u16_in = |i: usize| r.get(i..i + 2).and_then(|b| b.try_into().ok()).map(u16::from_le_bytes).map(f64::from);
        let xyz = [i32_at(0), i32_at(4), i32_at(8)];
        let [Some(x), Some(y), Some(z)] = xyz else {
            return Err("a point record is cut short".into());
        };
        let p = [x * scale[0] + offset[0], y * scale[1] + offset[1], z * scale[2] + offset[2]];
        let rgb = match colour.map(|at| [u16_in(at), u16_in(at + 2), u16_in(at + 4)]) {
            Some([Some(r), Some(g), Some(b)]) => Some([r, g, b]),
            _ => None,
        };
        f(Raw { p, intensity: u16_in(12), rgb })?;
    }
    Ok(())
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
    fn las(minor: u8, format: u8, scale: [f64; 3], offset: [f64; 3], gap: usize, pts: &[([i32; 3], u16, [u16; 3])]) -> Vec<u8> {
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
        for (ext, f) in [("pts", Format::Pts), ("XYZ", Format::Xyz), ("txt", Format::Xyz), ("ply", Format::Ply), (".las", Format::Las)] {
            assert_eq!(Format::from_extension(ext), Ok(f), "{ext}");
        }
        for (ext, says) in [("laz", "LAZ"), ("e57", "E57"), ("rcp", "ReCap"), ("rcs", "ReCap"), ("dwg", "dwg")] {
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
            (laz, "LAZ"),
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
}
