//! A scan's index (`.bimpc`): every point of it, in an octree on disk, so a view reads only what it shows at the detail it
//! shows it (`cloud/lod.rs`), and a project holds only the top of it. BIMCraft's own format, written here and described in
//! `docs/areas/point-clouds.md`; the method, each level a grid sample of what's below it, is Potree's (Schütz, 2016).
//!
//! The file, little-endian: a header of [`HEADER`] bytes, the nodes' points, then the table of nodes. A point's place is a
//! `u32` along each axis of the root cube, so a node at level `l` is the points whose top `l` bits are its key; a node holds the
//! bits below those (the top 16 of them where that's within half a millimetre, else all 32), then each point's colour and
//! intensity, the whole deflated. A point is in one node only: a node keeps one point in each
//! cell of a [`GRID`]³ grid of its cube and hands the rest to its children, down to a leaf.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use flate2::Compression;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use rayon::prelude::*;

use super::{Format, GREY, MAX_SPAN, Raw, Scan, Source, Station, Stats, each};
use crate::progress::Progress;

/// What a `.bimpc` file starts with.
pub const MAGIC: [u8; 8] = *b"BIMPC\0\r\n";
/// The version of the format this writes and reads.
pub const VERSION: u32 = 1;
/// How long the header is, bytes.
const HEADER: u64 = 128;
/// How long a node's entry in the table is, bytes.
const ENTRY: u64 = 32;
/// How many cells a node's grid has along each side, as a power of two: 128.
const GRID_BITS: u32 = 7;
/// The deepest a node may be: below it a node's grid cells would be finer than a point's place holds.
pub const MAX_DEPTH: u8 = 32 - GRID_BITS as u8 - 1;
/// The most nodes a file may have: a billion points make some tens of thousands, and a node's entry is held for each.
pub const MAX_NODES: u64 = 4_000_000;
/// The most points a node may hold.
pub const MAX_NODE_POINTS: u32 = 1 << 20;
/// The most a node may take in the file, bytes.
const MAX_NODE_BYTES: u32 = 64 << 20;
/// The most points an index may hold: a trillion.
pub const MAX_COUNT: u64 = 1_000_000_000_000;
/// How long a point is while the index is built, bytes: its place, colour and intensity.
const REC: usize = 16;
/// The finest a node stores its points' places, mm: past it, a node stores all 32 bits.
const FINEST: f64 = 0.5;

/// What the header says.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    /// Made when the file was written: the project holds it too, so another scan's index is never taken for this one's.
    pub id: u128,
    /// The points it holds.
    pub count: u64,
    /// Where the scan's middle is in its files' coordinates, mm: its points are about it, as a [`super::Cloud`]'s.
    pub origin: [f64; 3],
    /// The root cube's lowest corner from `origin`, and how wide it is, mm.
    pub min: [f64; 3],
    pub edge: f64,
    /// Whether its points have colours, and intensities.
    pub colour: bool,
    pub intensity: bool,
    table: u64,
    nodes: u64,
}

/// A node of the octree: where it is, how many points it holds, and where they are in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub level: u8,
    pub key: [u32; 3],
    pub points: u32,
    offset: u64,
    size: u32,
}

/// A node's points, about the scan's middle as a [`super::Cloud`]'s, mm, with their colours and intensities if it has them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodePoints {
    pub points: Vec<[f32; 3]>,
    pub colours: Option<Vec<[u8; 3]>>,
    pub intensity: Option<Vec<u8>>,
}

impl NodePoints {
    /// What it takes of memory, bytes.
    pub fn bytes(&self) -> usize {
        self.points.len() * 12
            + self.colours.as_ref().map_or(0, |c| c.len() * 3)
            + self.intensity.as_ref().map_or(0, Vec::len)
            + std::mem::size_of::<NodePoints>()
    }
}

/// An index opened: its header and table, its points read a node at a time. The table is in the order of its nodes' places
/// (level, then key), which is how a node's children are found; the file is read where each node is, without a lock.
pub struct Index {
    pub header: Header,
    pub nodes: Vec<Node>,
    file: Backing,
    size: u64,
}

/// What an index is read from: its file, read at a place without moving a cursor (so readers on several threads share it),
/// or bytes in memory.
enum Backing {
    File(File),
    Bytes(Vec<u8>),
}

impl Backing {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), String> {
        match self {
            Backing::Bytes(b) => {
                let from = usize::try_from(at).map_err(|_| broken("a node lies past the file"))?;
                let src = b.get(from..from + buf.len()).ok_or_else(|| broken("a node lies past the file"))?;
                buf.copy_from_slice(src);
                Ok(())
            }
            #[cfg(unix)]
            Backing::File(f) => std::os::unix::fs::FileExt::read_exact_at(f, buf, at).map_err(broken),
            #[cfg(windows)]
            Backing::File(f) => {
                let (mut done, mut at) = (0, at);
                while done < buf.len() {
                    let n = std::os::windows::fs::FileExt::seek_read(f, &mut buf[done..], at).map_err(broken)?;
                    if n == 0 {
                        return Err(broken("it's cut short"));
                    }
                    (done, at) = (done + n, at + n as u64);
                }
                Ok(())
            }
            #[cfg(not(any(unix, windows)))]
            Backing::File(f) => {
                let mut f = f;
                f.seek(SeekFrom::Start(at)).map_err(broken)?;
                f.read_exact(buf).map_err(broken)
            }
        }
    }
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Index({} points, {} nodes)", self.header.count, self.nodes.len())
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8).map_or(0, |s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn f64_at(b: &[u8], at: usize) -> f64 {
    f64::from_bits(u64_at(b, at))
}

fn broken(e: impl std::fmt::Display) -> String {
    format!("the index can't be read: {e}")
}

impl Header {
    fn bytes(&self) -> [u8; HEADER as usize] {
        let mut b = [0u8; HEADER as usize];
        let mut at = 0;
        let mut put = |s: &[u8]| {
            b[at..at + s.len()].copy_from_slice(s);
            at += s.len();
        };
        put(&MAGIC);
        put(&VERSION.to_le_bytes());
        put(&(u32::from(self.colour) | (u32::from(self.intensity) << 1)).to_le_bytes());
        put(&self.id.to_le_bytes());
        put(&self.count.to_le_bytes());
        for v in self.origin.iter().chain(&self.min).chain([&self.edge]) {
            put(&v.to_le_bytes());
        }
        put(&self.table.to_le_bytes());
        put(&self.nodes.to_le_bytes());
        b
    }

    fn read(b: &[u8], size: u64) -> Result<Header, String> {
        if b.get(..8) != Some(&MAGIC[..]) {
            return Err("it isn't a BIMCraft point cloud index (.bimpc)".into());
        }
        let version = u32_at(b, 8);
        if version != VERSION {
            return Err(format!("it's an index of version {version}; this BIMCraft reads version {VERSION}: build it again"));
        }
        let flags = u32_at(b, 12);
        let id = b.get(16..32).map_or(0, |s| u128::from_le_bytes(s.try_into().unwrap_or([0; 16])));
        let count = u64_at(b, 32);
        let origin = [0, 1, 2].map(|k| f64_at(b, 40 + 8 * k));
        let min = [0, 1, 2].map(|k| f64_at(b, 64 + 8 * k));
        let edge = f64_at(b, 88);
        let (table, nodes) = (u64_at(b, 96), u64_at(b, 104));
        if count > MAX_COUNT {
            return Err(broken(format!("it says it holds {count} points; up to {MAX_COUNT}")));
        }
        if !origin.iter().all(|v| v.is_finite() && v.abs() <= super::MAX_FILE_COORD) {
            return Err(broken("its origin isn't one"));
        }
        if !(edge.is_finite() && edge > 0.0 && edge <= 2.0 * MAX_SPAN) || !min.iter().all(|v| v.is_finite() && v.abs() <= 2.0 * MAX_SPAN) {
            return Err(broken("its cube isn't one"));
        }
        if nodes == 0 || nodes > MAX_NODES {
            return Err(broken(format!("it says it has {nodes} nodes; 1 to {MAX_NODES}")));
        }
        if table < HEADER || nodes.checked_mul(ENTRY).and_then(|n| n.checked_add(table)) != Some(size) {
            return Err(broken("its table of nodes isn't where it says, or it's cut short"));
        }
        Ok(Header { id, count, origin, min, edge, colour: flags & 1 != 0, intensity: flags & 2 != 0, table, nodes })
    }

    /// How many low bits of a place a node at `level` drops, and so how many it stores (16 or 32).
    fn shift(&self, level: u8) -> u32 {
        let below = 32 - u32::from(level);
        let shift = below.saturating_sub(16);
        let step = self.edge / 2f64.powi(32) * 2f64.powi(shift as i32);
        if step <= FINEST { shift } else { 0 }
    }

    /// A place, from the scan's middle, mm.
    fn place(&self, q: [u64; 3], half: f64) -> [f32; 3] {
        [0, 1, 2].map(|k| (self.min[k] + (q[k] as f64 + half) / 2f64.powi(32) * self.edge) as f32)
    }
}

impl Node {
    fn bytes(&self) -> [u8; ENTRY as usize] {
        let mut b = [0u8; ENTRY as usize];
        b[0] = self.level;
        for k in 0..3 {
            b[4 + 4 * k..8 + 4 * k].copy_from_slice(&self.key[k].to_le_bytes());
        }
        b[16..24].copy_from_slice(&self.offset.to_le_bytes());
        b[24..28].copy_from_slice(&self.size.to_le_bytes());
        b[28..32].copy_from_slice(&self.points.to_le_bytes());
        b
    }

    fn read(b: &[u8], h: &Header) -> Result<Node, String> {
        let level = b.first().copied().unwrap_or(u8::MAX);
        let key = [0, 1, 2].map(|k| u32_at(b, 4 + 4 * k));
        let (offset, size, points) = (u64_at(b, 16), u32_at(b, 24), u32_at(b, 28));
        if level > MAX_DEPTH || key.iter().any(|&v| u64::from(v) >> level != 0) {
            return Err(broken(format!("a node at level {level} lies outside the cube")));
        }
        if points > MAX_NODE_POINTS || size > MAX_NODE_BYTES || (points == 0) != (size == 0) {
            return Err(broken(format!("a node says it holds {points} points in {size} bytes")));
        }
        if offset < HEADER || offset.saturating_add(u64::from(size)) > h.table {
            return Err(broken("a node's points lie outside the file"));
        }
        Ok(Node { level, key, points, offset, size })
    }
}

impl Index {
    /// The index at `path`, its header and table read and checked; its points are read when asked for.
    pub fn open(path: &Path) -> Result<Index, String> {
        let mut file = File::open(path).map_err(|e| format!("the index {} can't be opened: {e}", path.display()))?;
        let (header, nodes, size) = Index::table(&mut file)?;
        Ok(Index { header, nodes, file: Backing::File(file), size })
    }

    /// The index `bytes` hold.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Index, String> {
        let (header, nodes, size) = Index::table(&mut std::io::Cursor::new(&bytes))?;
        Ok(Index { header, nodes, file: Backing::Bytes(bytes), size })
    }

    /// The header and the table of nodes `src` holds, checked, and its size.
    fn table(mut src: &mut dyn Source) -> Result<(Header, Vec<Node>, u64), String> {
        let size = src.seek(SeekFrom::End(0)).map_err(broken)?;
        let mut head = Vec::with_capacity(HEADER as usize);
        src.seek(SeekFrom::Start(0)).map_err(broken)?;
        (&mut src).take(HEADER).read_to_end(&mut head).map_err(broken)?;
        if head.len() < HEADER as usize && head.get(..MAGIC.len().min(head.len())) == Some(&MAGIC[..MAGIC.len().min(head.len())]) {
            return Err(broken("it's cut short"));
        }
        let header = Header::read(&head, size)?;
        src.seek(SeekFrom::Start(header.table)).map_err(broken)?;
        let mut r = BufReader::with_capacity(1 << 20, &mut src);
        let mut nodes: Vec<Node> = Vec::with_capacity(header.nodes.min(1 << 20) as usize);
        let (mut total, mut entry) = (0_u64, [0u8; ENTRY as usize]);
        for _ in 0..header.nodes {
            r.read_exact(&mut entry).map_err(|_| broken("its table is cut short"))?;
            let n = Node::read(&entry, &header)?;
            total += u64::from(n.points);
            if nodes.last().is_some_and(|last| (last.level, last.key) >= (n.level, n.key)) {
                return Err(broken("its table isn't in the order of its nodes, or two nodes are in one place"));
            }
            nodes.push(n);
        }
        drop(r);
        if total != header.count {
            return Err(broken(format!("its nodes hold {total} points, and its header says {}", header.count)));
        }
        if nodes.first().is_none_or(|n| n.level != 0) {
            return Err(broken("it has no root"));
        }
        Ok((header, nodes, size))
    }

    /// The node at `level` and `key`, if there is one.
    fn find(&self, level: u8, key: [u32; 3]) -> Option<usize> {
        self.nodes.binary_search_by(|n| (n.level, n.key).cmp(&(level, key))).ok()
    }

    /// The root node.
    pub fn root(&self) -> usize {
        0
    }

    /// Node `i`'s children that hold anything or have children of their own.
    pub fn children(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let n = self.nodes.get(i).copied();
        (0..8_u32).filter_map(move |o| {
            let n = n?;
            if n.level >= MAX_DEPTH {
                return None;
            }
            let key = [0, 1, 2].map(|k| (n.key[k] << 1) | ((o >> k) & 1));
            self.find(n.level + 1, key)
        })
    }

    /// Node `i`'s cube from the scan's middle, mm: its lowest corner and how wide it is.
    pub fn cube(&self, i: usize) -> ([f64; 3], f64) {
        let Some(n) = self.nodes.get(i) else { return (self.header.min, self.header.edge) };
        let edge = self.header.edge / 2f64.powi(i32::from(n.level));
        ([0, 1, 2].map(|k| self.header.min[k] + f64::from(n.key[k]) * edge), edge)
    }

    /// How far apart a node's points are at `level`, at least, mm: a cell of its grid.
    pub fn spacing(&self, level: u8) -> f64 {
        self.header.edge / 2f64.powi(i32::from(level) + GRID_BITS as i32)
    }

    /// The size of the file, bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Node `i`'s points, read from the file.
    pub fn load(&self, i: usize) -> Result<NodePoints, String> {
        let n = *self.nodes.get(i).ok_or_else(|| broken("no such node"))?;
        if n.points == 0 {
            return Ok(NodePoints {
                colours: self.header.colour.then(Vec::new),
                intensity: self.header.intensity.then(Vec::new),
                ..NodePoints::default()
            });
        }
        let mut packed = vec![0u8; n.size as usize];
        self.file.read_at(n.offset, &mut packed)?;
        decode(&self.header, &n, &packed)
    }
}

/// What a node's points take before they're deflated, bytes.
fn unpacked(h: &Header, n: &Node) -> usize {
    let wide = if h.shift(n.level) > 0 { 2 } else { 4 };
    let n = n.points as usize;
    n * 3 * wide + if h.colour { n * 3 } else { 0 } + if h.intensity { n } else { 0 }
}

fn decode(h: &Header, n: &Node, packed: &[u8]) -> Result<NodePoints, String> {
    let want = unpacked(h, n);
    let mut b = Vec::with_capacity(want);
    ZlibDecoder::new(packed).take(want as u64 + 1).read_to_end(&mut b).map_err(broken)?;
    if b.len() != want {
        return Err(broken("a node's points aren't as long as it says"));
    }
    let count = n.points as usize;
    let shift = h.shift(n.level);
    let wide = if shift > 0 { 2 } else { 4 };
    let below = 32 - u32::from(n.level);
    let base = n.key.map(|k| u64::from(k) << below);
    let half = if shift > 0 { 2f64.powi(shift as i32) / 2.0 } else { 0.5 };
    let axis = |k: usize, i: usize| -> u64 {
        let at = (k * count + i) * wide;
        let v = if wide == 2 { u64::from(u16::from_le_bytes([b[at], b[at + 1]])) } else { u64::from(u32_at(&b, at)) };
        base[k] | ((v << shift) & ((1u64 << below) - 1))
    };
    let points = (0..count).map(|i| h.place([axis(0, i), axis(1, i), axis(2, i)], half)).collect();
    let mut at = count * 3 * wide;
    let colours = h.colour.then(|| {
        let c = b[at..at + count * 3].as_chunks::<3>().0.to_vec();
        at += count * 3;
        c
    });
    let intensity = h.intensity.then(|| b[at..at + count].to_vec());
    Ok(NodePoints { points, colours, intensity })
}

/// A point while the index is built: its place in the root cube, its colour and intensity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rec {
    q: [u32; 3],
    rgb: [u8; 3],
    i: u8,
}

impl Rec {
    fn bytes(&self) -> [u8; REC] {
        let mut b = [0u8; REC];
        for k in 0..3 {
            b[4 * k..4 * k + 4].copy_from_slice(&self.q[k].to_le_bytes());
        }
        b[12..15].copy_from_slice(&self.rgb);
        b[15] = self.i;
        b
    }

    fn of(b: &[u8]) -> Rec {
        Rec { q: [0, 1, 2].map(|k| u32_at(b, 4 * k)), rgb: [b[12], b[13], b[14]], i: b[15] }
    }

    /// The node at `level` it lies in.
    fn key(&self, level: u8) -> [u32; 3] {
        self.q.map(|v| if level == 0 { 0 } else { v >> (32 - u32::from(level)) })
    }

    /// Its cell of the grid of its node at `level`, as one number.
    fn cell(&self, level: u8) -> usize {
        let shift = 32 - u32::from(level) - GRID_BITS;
        let c = self.q.map(|v| ((v >> shift) & ((1 << GRID_BITS) - 1)) as usize);
        (c[0] << (2 * GRID_BITS)) | (c[1] << GRID_BITS) | c[2]
    }

    /// Which child of its node at `level` it lies in.
    fn octant(&self, level: u8) -> usize {
        let bit = 31 - u32::from(level);
        (0..3).map(|k| (((self.q[k] >> bit) & 1) as usize) << k).sum()
    }
}

fn encode(h: &Header, level: u8, recs: &[Rec]) -> Vec<u8> {
    let shift = h.shift(level);
    let below = 32 - u32::from(level);
    let mask = if below >= 32 { u32::MAX } else { (1u32 << below) - 1 };
    let mut b = Vec::with_capacity(recs.len() * REC);
    for k in 0..3 {
        for r in recs {
            let v = (r.q[k] & mask) >> shift;
            if shift > 0 {
                b.extend_from_slice(&(v as u16).to_le_bytes());
            } else {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    if h.colour {
        b.extend(recs.iter().flat_map(|r| r.rgb));
    }
    if h.intensity {
        b.extend(recs.iter().map(|r| r.i));
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
    let _ = z.write_all(&b);
    z.finish().unwrap_or_default()
}

/// How an index is built: what a worker holds at once and when a node is a leaf. Tests make them small.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tuning {
    /// The most points a worker sorts in memory: a bucket with more is split into its eight.
    pub(crate) bucket: u64,
    /// A node with no more points than this keeps them all.
    pub(crate) leaf: usize,
}

impl Default for Tuning {
    fn default() -> Tuning {
        Tuning { bucket: 2_000_000, leaf: 32_768 }
    }
}

/// An index built: the points it holds, the levels of it the project keeps (the sample, `scan`), the files it came from and
/// what it takes on disk.
#[derive(Debug)]
pub struct Built {
    pub id: u128,
    pub count: u64,
    /// The levels the sample holds whole: a view reads the ones below. 0 when even the root was thinned to fit.
    pub levels: u8,
    pub scan: Scan,
    pub stations: Vec<Station>,
    pub bytes: u64,
}

/// One of the files an index is built from.
pub struct Input {
    pub name: String,
    pub src: Box<dyn Source>,
    pub format: Format,
}

/// The nodes written so far, and where the next goes.
struct Out {
    file: BufWriter<File>,
    at: u64,
    nodes: Vec<Node>,
}

struct Writer {
    header: Header,
    out: Mutex<Out>,
    progress: Option<Arc<Progress>>,
}

impl Writer {
    fn write(&self, level: u8, key: [u32; 3], mut recs: Vec<Rec>) -> Result<u64, String> {
        if let Some(p) = &self.progress {
            p.check()?;
        }
        recs.truncate(MAX_NODE_POINTS as usize);
        let packed = if recs.is_empty() { Vec::new() } else { encode(&self.header, level, &recs) };
        let size = u32::try_from(packed.len()).ok().filter(|&s| s <= MAX_NODE_BYTES).ok_or_else(|| "a node is too big".to_string())?;
        let mut out = self.out.lock().map_err(|_| "the index's writer failed".to_string())?;
        let offset = out.at;
        out.file.write_all(&packed).map_err(|e| format!("the index can't be written: {e}"))?;
        out.at += u64::from(size);
        out.nodes.push(Node { level, key, points: recs.len() as u32, offset, size });
        drop(out);
        if let Some(p) = &self.progress {
            p.step(recs.len() as u64);
        }
        Ok(recs.len() as u64)
    }
}

/// A node not yet written: its parent may take some of its points.
struct Top {
    level: u8,
    key: [u32; 3],
    recs: Vec<Rec>,
}

/// A file of points sorted into one node's cube, waiting to be built.
struct Bucket {
    level: u8,
    key: [u32; 3],
    path: PathBuf,
    count: u64,
}

/// Removes what a build leaves behind, when it ends however it ends.
struct Tidy(PathBuf, bool);

impl Drop for Tidy {
    fn drop(&mut self) {
        let _ = if self.1 { std::fs::remove_dir_all(&self.0) } else { std::fs::remove_file(&self.0) };
    }
}

/// Builds the index of `files` (their unit `scale` mm) at `out`, and the sample a project holds, no more than `cap` points.
/// It reads the files twice: once to measure them, and once to sort their points into buckets beside `out`, which it removes;
/// then it builds the buckets side by side, as many at once as the memory `budget` leaves room for.
pub fn build(
    files: Vec<Input>,
    scale: f64,
    out: &Path,
    cap: usize,
    budget: Option<&crate::memory::Budget>,
    progress: Option<Arc<Progress>>,
) -> Result<Built, String> {
    #[cfg(test)]
    let (tuning, cap) = TUNED.with(|t| t.get()).map_or((Tuning::default(), cap), |(t, most)| (t, cap.min(most)));
    #[cfg(not(test))]
    let tuning = Tuning::default();
    build_tuned(files, scale, out, cap, budget, progress, tuning)
}

#[cfg(test)]
thread_local! {
    /// How the tests on this thread build an index through the commands, and the most points its sample keeps: small, so a
    /// small scan has levels below the sample.
    pub(crate) static TUNED: std::cell::Cell<Option<(Tuning, usize)>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn build_tuned(
    mut files: Vec<Input>,
    scale: f64,
    out: &Path,
    cap: usize,
    budget: Option<&crate::memory::Budget>,
    progress: Option<Arc<Progress>>,
    tuning: Tuning,
) -> Result<Built, String> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(format!("a unit of {scale} mm isn't one"));
    }
    if files.is_empty() || files.len() > super::MAX_STATIONS {
        return Err(format!("an index is built from 1 to {} files", super::MAX_STATIONS));
    }
    if cap == 0 {
        return Err("the project has no room left for a scan's points".into());
    }
    let look = |at: &mut u64, n: u64| -> Result<(), String> {
        if n >= *at + super::LOOK {
            if let Some(b) = budget {
                b.check()?;
            }
            if let Some(p) = &progress {
                p.check()?;
                p.step(n - *at);
            }
            *at = n;
        }
        Ok(())
    };
    // Measured: how far each file reaches, and how it's shaded.
    if let Some(p) = &progress {
        p.stage(bimcraft_i18n::ui_msg!("Measuring the scan"), 0);
    }
    let mut measured = Vec::with_capacity(files.len());
    for f in &mut files {
        let len = f.src.seek(SeekFrom::End(0)).map_err(super::unread)?;
        if len > super::MAX_BYTES {
            return Err(format!("{} is {len} bytes; up to {}", f.name, super::MAX_BYTES));
        }
        let (mut stats, mut at) = (Stats::default(), 0);
        each(&mut *f.src, len, f.format, 1, &mut |_, raw| {
            stats.add(&raw, scale)?;
            look(&mut at, stats.count as u64)
        })
        .map_err(|e| of(&f.name, e))?;
        if stats.count == 0 {
            return Err(format!("{} holds no points", f.name));
        }
        measured.push((len, stats));
    }
    let count: u64 = measured.iter().map(|(_, s)| s.count as u64).sum();
    if count > MAX_COUNT {
        return Err(format!("they hold {count} points; an index holds up to {MAX_COUNT}"));
    }
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for (_, s) in &measured {
        for k in 0..3 {
            lo[k] = lo[k].min(s.min[k]);
            hi[k] = hi[k].max(s.max[k]);
        }
    }
    let mut edge: f64 = 1.0;
    for k in 0..3 {
        let wide = hi[k] - lo[k];
        if wide > MAX_SPAN {
            return Err(format!("they're {wide:.0} mm wide along {}; up to {MAX_SPAN} mm", ["x", "y", "z"][k]));
        }
        edge = edge.max(wide);
    }
    // A little wider, so the farthest point is inside it.
    let edge = edge * (1.0 + 1e-9) + 1e-3;
    let origin = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let shades: Vec<_> = measured.iter().map(|(_, s)| (s.colour(), s.intensity.filter(|(lo, hi)| hi > lo))).collect();
    let colour = shades.iter().any(|(c, _)| c.is_some());
    let intensity = shades.iter().any(|(_, b)| b.is_some());
    let mut id = [0u8; 16];
    if getrandom::fill(&mut id).is_err() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        id = (nanos ^ u128::from(count) << 64).to_le_bytes();
    }
    let header = Header { id: u128::from_le_bytes(id), count, origin, min: [-edge / 2.0; 3], edge, colour, intensity, table: 0, nodes: 0 };

    // Sorted: each point into the bucket of its cube at the level where buckets hold about what a worker sorts.
    let mut level = 0_u8;
    while level < 3 && count >> (3 * u32::from(level)) > tuning.bucket {
        level += 1;
    }
    let part = out.with_extension("bimpc.part");
    let tmp = out.with_extension("bimpc.tmp");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("the index's folder {} can't be made: {e}", tmp.display()))?;
    let _tidy_tmp = Tidy(tmp.clone(), true);
    if let Some(p) = &progress {
        p.stage(bimcraft_i18n::ui_msg!("Sorting the points"), count);
    }
    let mut buckets: HashMap<[u32; 3], (BufWriter<File>, u64)> = HashMap::new();
    let bucket_path = |level: u8, key: [u32; 3]| tmp.join(format!("{level}-{}-{}-{}", key[0], key[1], key[2]));
    let mut at = 0_u64;
    let mut n = 0_u64;
    for (f, ((len, stats), (shade, bright))) in files.iter_mut().zip(measured.iter().zip(&shades)) {
        let mut put = |raw: Raw| -> Result<(), String> {
            let q = [0, 1, 2].map(|k| {
                let v = (raw.p[k] * scale - origin[k] - header.min[k]) / edge * 2f64.powi(32);
                v.floor().clamp(0.0, f64::from(u32::MAX)) as u32
            });
            let rgb = match shade {
                Some(s) => s.of(&raw),
                None => [GREY; 3],
            };
            let i = match bright {
                Some((lo, hi)) => {
                    let v = raw.intensity.filter(|v| v.is_finite()).map_or(0.0, |v| (v - lo) / (hi - lo));
                    (v * 255.0).round().clamp(0.0, 255.0) as u8
                }
                None => 128,
            };
            let r = Rec { q, rgb, i };
            let key = r.key(level);
            let slot = match buckets.entry(key) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let file = File::create(bucket_path(level, key)).map_err(|e| format!("the index's buckets can't be written: {e}"))?;
                    e.insert((BufWriter::with_capacity(1 << 16, file), 0))
                }
            };
            slot.0.write_all(&r.bytes()).map_err(|e| format!("the index's buckets can't be written: {e}"))?;
            slot.1 += 1;
            n += 1;
            look(&mut at, n)
        };
        let mut read = 0_usize;
        each(&mut *f.src, *len, f.format, 1, &mut |_, raw| {
            read += 1;
            put(raw)
        })
        .map_err(|e| of(&f.name, e))?;
        if read != stats.count {
            return Err(format!("{} held {} points the first time it was read and {read} the second", f.name, stats.count));
        }
    }
    let mut sorted = Vec::with_capacity(buckets.len());
    for (key, (mut w, count)) in buckets {
        w.flush().map_err(|e| format!("the index's buckets can't be written: {e}"))?;
        sorted.push(Bucket { level, key, path: bucket_path(level, key), count });
    }
    sorted.sort_by_key(|b| b.key);

    // Built: each bucket by a worker of its own, as many at once as there's memory for; then the levels above them.
    if let Some(p) = &progress {
        p.stage(bimcraft_i18n::ui_msg!("Building the index"), count);
    }
    let file = File::create(&part).map_err(|e| format!("the index {} can't be written: {e}", part.display()))?;
    let _tidy_part = Tidy(part.clone(), false);
    let mut file = BufWriter::with_capacity(1 << 20, file);
    file.write_all(&[0u8; HEADER as usize]).map_err(|e| format!("the index can't be written: {e}"))?;
    let w = Writer { header: header.clone(), out: Mutex::new(Out { file, at: HEADER, nodes: Vec::new() }), progress: progress.clone() };
    // What a worker takes: its points twice over while they're sorted, and their grid.
    let per = tuning.bucket * REC as u64 * 3 + (1 << 21);
    let threads = rayon::current_num_threads().max(1);
    let workers = budget.and_then(crate::memory::Budget::left).map_or(threads, |left| ((left / 2 / per) as usize).clamp(1, threads));
    let pool = rayon::ThreadPoolBuilder::new().num_threads(workers).build().map_err(|e| format!("the index can't be built: {e}"))?;
    let tops: Vec<Top> =
        pool.install(|| sorted.into_par_iter().map(|b| subtree(b, &w, &tmp, tuning)).collect::<Result<Vec<_>, String>>())?;
    let mut tops = climb(tops, 0, &w)?;
    let root = tops.pop().ok_or_else(|| "the index has no root".to_string())?;
    w.write(root.level, root.key, root.recs)?;

    // Its table, then its header, now that they're known.
    let Out { mut file, at, mut nodes } = w.out.into_inner().map_err(|_| "the index's writer failed".to_string())?;
    nodes.sort_by_key(|n| (n.level, n.key));
    let header = Header { table: at, nodes: nodes.len() as u64, ..header };
    let wrote = |e: std::io::Error| format!("the index can't be written: {e}");
    for n in &nodes {
        file.write_all(&n.bytes()).map_err(wrote)?;
    }
    file.flush().map_err(wrote)?;
    let mut file = file.into_inner().map_err(|e| wrote(e.into_error()))?;
    file.seek(SeekFrom::Start(0)).map_err(wrote)?;
    file.write_all(&header.bytes()).map_err(wrote)?;
    file.sync_all().map_err(wrote)?;
    drop(file);
    std::fs::rename(&part, out).map_err(|e| format!("the index {} can't be written: {e}", out.display()))?;

    // The sample: the levels from the root that fit in `cap`, whole.
    let index = Index::open(out)?;
    let mut per_level = vec![0_u64; usize::from(MAX_DEPTH) + 1];
    for n in &index.nodes {
        per_level[usize::from(n.level)] += u64::from(n.points);
    }
    let (mut levels, mut held) = (0_u8, 0_u64);
    while usize::from(levels) < per_level.len() && held + per_level[usize::from(levels)] <= cap as u64 {
        held += per_level[usize::from(levels)];
        levels += 1;
        if held == count {
            break;
        }
    }
    let mut scan =
        Scan { origin, points: Vec::new(), colours: colour.then(Vec::new), intensity: intensity.then(Vec::new), read: count as usize };
    let mut add = |p: NodePoints, every: usize| {
        for i in (0..p.points.len()).step_by(every.max(1)) {
            scan.points.push(p.points[i]);
            if let (Some(c), Some(from)) = (&mut scan.colours, &p.colours) {
                c.push(from.get(i).copied().unwrap_or([GREY; 3]));
            }
            if let (Some(l), Some(from)) = (&mut scan.intensity, &p.intensity) {
                l.push(from.get(i).copied().unwrap_or(128));
            }
        }
    };
    if levels == 0 {
        let root = index.root();
        let every = (index.nodes[root].points as usize).div_ceil(cap);
        add(index.load(root)?, every);
    } else {
        for (i, n) in index.nodes.iter().enumerate() {
            if n.level < levels {
                add(index.load(i)?, 1);
            }
        }
    }
    let stations = files.iter().zip(&measured).map(|(f, (_, s))| Station { source: f.name.clone(), points: s.count }).collect();
    Ok(Built { id: header.id, count, levels, scan, stations, bytes: index.size() })
}

/// What went wrong reading file `name`, unless it was cancelled, which says only that.
fn of(name: &str, e: String) -> String {
    if e == crate::progress::CANCELLED { e } else { format!("{name}: {e}") }
}

/// Builds the subtree of `b`: in memory when it's small enough, else split into its eight and each of those built.
fn subtree(b: Bucket, w: &Writer, tmp: &Path, tuning: Tuning) -> Result<Top, String> {
    if b.count > tuning.bucket && b.level + 1 < MAX_DEPTH {
        let children = split(&b, tmp)?;
        let tops = children.into_par_iter().map(|c| subtree(c, w, tmp, tuning)).collect::<Result<Vec<_>, String>>()?;
        let mut up = climb(tops, b.level, w)?;
        return up.pop().ok_or_else(|| "a bucket of the index has no points".to_string());
    }
    let mut bytes = Vec::new();
    File::open(&b.path).and_then(|mut f| f.read_to_end(&mut bytes)).map_err(|e| format!("the index's buckets can't be read: {e}"))?;
    let _ = std::fs::remove_file(&b.path);
    let recs: Vec<Rec> = bytes.as_chunks::<REC>().0.iter().map(|c| Rec::of(c)).collect();
    drop(bytes);
    let recs = down(b.level, b.key, recs, w, tuning.leaf)?;
    Ok(Top { level: b.level, key: b.key, recs })
}

/// Splits bucket `b` into the buckets of its eight children.
fn split(b: &Bucket, tmp: &Path) -> Result<Vec<Bucket>, String> {
    let level = b.level + 1;
    let mut out: Vec<Option<(BufWriter<File>, Bucket)>> = (0..8).map(|_| None).collect();
    let bad = |e: std::io::Error| format!("the index's buckets can't be split: {e}");
    let mut r = BufReader::with_capacity(1 << 20, File::open(&b.path).map_err(bad)?);
    let mut rec = [0u8; REC];
    loop {
        match r.read_exact(&mut rec) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(bad(e)),
        }
        let r = Rec::of(&rec);
        let o = r.octant(b.level);
        let slot = &mut out[o];
        if slot.is_none() {
            let key = r.key(level);
            let path = tmp.join(format!("{level}-{}-{}-{}", key[0], key[1], key[2]));
            let file = File::create(&path).map_err(bad)?;
            *slot = Some((BufWriter::with_capacity(1 << 16, file), Bucket { level, key, path, count: 0 }));
        }
        if let Some((w, c)) = slot {
            w.write_all(&rec).map_err(bad)?;
            c.count += 1;
        }
    }
    drop(r);
    let _ = std::fs::remove_file(&b.path);
    let mut children = Vec::new();
    for (mut w, c) in out.into_iter().flatten() {
        w.flush().map_err(bad)?;
        children.push(c);
    }
    Ok(children)
}

/// Node (`level`, `key`) of `recs`: its own points returned, the rest handed to its children, which are written.
fn down(level: u8, key: [u32; 3], recs: Vec<Rec>, w: &Writer, leaf: usize) -> Result<Vec<Rec>, String> {
    if recs.len() <= leaf || level >= MAX_DEPTH {
        return Ok(recs);
    }
    let mut taken = vec![0u64; 1 << (3 * GRID_BITS - 6)];
    let (mut own, mut rest) = (Vec::new(), Vec::with_capacity(recs.len()));
    for r in recs {
        let c = r.cell(level);
        let (word, bit) = (c >> 6, 1u64 << (c & 63));
        if taken[word] & bit == 0 {
            taken[word] |= bit;
            own.push(r);
        } else {
            rest.push(r);
        }
    }
    drop(taken);
    let mut children: Vec<Vec<Rec>> = (0..8).map(|_| Vec::new()).collect();
    for r in rest {
        children[r.octant(level)].push(r);
    }
    for (o, recs) in children.into_iter().enumerate() {
        if recs.is_empty() {
            continue;
        }
        let child = [0, 1, 2].map(|k| (key[k] << 1) | ((o as u32 >> k) & 1));
        let kept = down(level + 1, child, recs, w, leaf)?;
        w.write(level + 1, child, kept)?;
    }
    Ok(own)
}

/// The nodes above `tops` up to `level`, each a grid sample of its children's points, which are then written: what's left
/// is the nodes at `level`, not yet written.
fn climb(mut tops: Vec<Top>, level: u8, w: &Writer) -> Result<Vec<Top>, String> {
    while tops.first().is_some_and(|t| t.level > level) {
        let parent = tops[0].level - 1;
        let mut by: HashMap<[u32; 3], Vec<Top>> = HashMap::new();
        for t in tops {
            by.entry(t.key.map(|k| k >> 1)).or_default().push(t);
        }
        let mut up = Vec::with_capacity(by.len());
        let mut groups: Vec<_> = by.into_iter().collect();
        groups.sort_by_key(|(k, _)| *k);
        for (key, mut children) in groups {
            children.sort_by_key(|c| c.key);
            let mut taken = vec![0u64; 1 << (3 * GRID_BITS - 6)];
            let mut own = Vec::new();
            for c in &mut children {
                c.recs.retain(|r| {
                    let cell = r.cell(parent);
                    let (word, bit) = (cell >> 6, 1u64 << (cell & 63));
                    if taken[word] & bit == 0 {
                        taken[word] |= bit;
                        own.push(*r);
                        false
                    } else {
                        true
                    }
                });
            }
            for c in children {
                w.write(c.level, c.key, c.recs)?;
            }
            up.push(Top { level: parent, key, recs: own });
        }
        tops = up;
    }
    Ok(tops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bimcraft-index-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("scan.bimpc")
    }

    /// A text file of `n` points on a surface and in a block, with colours and intensities: x y z i r g b, metres.
    fn pts(n: usize, seed: u64) -> String {
        let mut s = format!("{n}\n");
        let mut x = seed;
        for i in 0..n {
            x = crate::model::mix(x.wrapping_add(i as u64));
            let (a, b, c) = ((x & 0xFFFF) as f64 / 65535.0, ((x >> 16) & 0xFFFF) as f64 / 65535.0, ((x >> 32) & 0xFFFF) as f64 / 65535.0);
            let z = if i % 3 == 0 { c * 4.0 } else { 0.0 };
            s += &format!(
                "{:.4} {:.4} {:.4} {} {} {} {}\n",
                a * 20.0,
                b * 12.0,
                z,
                i % 2000,
                (x >> 40) & 255,
                (x >> 48) & 255,
                (x >> 56) & 255
            );
        }
        s
    }

    fn input(text: &str) -> Input {
        Input { name: "scan.pts".into(), src: Box::new(std::io::Cursor::new(text.as_bytes().to_vec())), format: Format::Pts }
    }

    fn all(index: &Index) -> Vec<[f32; 3]> {
        let mut v: Vec<[f32; 3]> = (0..index.nodes.len()).flat_map(|i| index.load(i).unwrap().points).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    }

    /// Whether `a` and `b` are the same points, each within a millimetre of one of the other's.
    fn within_a_millimetre(a: &[[f32; 3]], b: &[[f32; 3]]) {
        assert_eq!(a.len(), b.len());
        let cell = |p: &[f32; 3]| p.map(|v| (v / 2.0).floor() as i64);
        let mut theirs: HashMap<[i64; 3], Vec<[f32; 3]>> = HashMap::new();
        for p in b {
            theirs.entry(cell(p)).or_default().push(*p);
        }
        for a in a {
            let c = cell(a);
            let near = (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [c[0] + x, c[1] + y, c[2] + z])));
            let found = near.flat_map(|c| theirs.get(&c).into_iter().flatten()).any(|b| (0..3).all(|k| (a[k] - b[k]).abs() <= 1.0));
            assert!(found, "{a:?} isn't one of the other points");
        }
    }

    #[test]
    fn every_point_is_in_one_node_where_it_was() {
        let text = pts(60_000, 1);
        let out = temp("every");
        let built = build_tuned(vec![input(&text)], 1000.0, &out, 5_000, None, None, Tuning { bucket: 50_000, leaf: 2_000 }).unwrap();
        assert_eq!(built.count, 60_000);
        let index = Index::open(&out).unwrap();
        assert_eq!(index.header.id, built.id);
        assert!(index.nodes.len() > 8, "{} nodes", index.nodes.len());
        let ours = all(&index);
        // The same points read the ordinary way, to a millimetre.
        let read = super::super::read(text.as_bytes(), Format::Pts, &super::super::Options { cap: 100_000, ..Default::default() }).unwrap();
        assert_eq!(read.origin, index.header.origin);
        within_a_millimetre(&ours, &read.points);
        // Nothing left behind but the index.
        let left: Vec<_> = std::fs::read_dir(out.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("scan.bimpc")]);
    }

    #[test]
    fn a_node_keeps_one_point_in_each_cell_and_the_sample_is_whole_levels() {
        let text = pts(40_000, 2);
        let out = temp("cells");
        let built = build_tuned(vec![input(&text)], 1000.0, &out, 30_000, None, None, Tuning { bucket: 1_000_000, leaf: 1_000 }).unwrap();
        let index = Index::open(&out).unwrap();
        for (i, n) in index.nodes.iter().enumerate() {
            if index.children(i).next().is_none() {
                continue;
            }
            let (min, _) = index.cube(i);
            let s = index.spacing(n.level);
            let mut cells = std::collections::HashSet::new();
            for p in index.load(i).unwrap().points {
                let c = [0, 1, 2].map(|k| ((f64::from(p[k]) - min[k]) / s).floor() as i64);
                assert!(cells.insert(c), "node {i} at level {} has two points in cell {c:?}", n.level);
            }
        }
        let whole: u64 = index.nodes.iter().filter(|n| n.level < built.levels).map(|n| u64::from(n.points)).sum();
        assert!(built.levels >= 1);
        assert_eq!(built.scan.points.len() as u64, whole);
        assert!(whole <= 30_000);
        assert_eq!(built.scan.colours.as_ref().unwrap().len(), built.scan.points.len());
        assert_eq!(built.scan.intensity.as_ref().unwrap().len(), built.scan.points.len());
        assert_eq!(built.scan.read, 40_000);
    }

    #[test]
    fn buckets_split_or_not_hold_the_same_points() {
        let text = pts(30_000, 3);
        let (a, b) = (temp("split-a"), temp("split-b"));
        build_tuned(vec![input(&text)], 1000.0, &a, 1_000, None, None, Tuning { bucket: 1_000_000, leaf: 500 }).unwrap();
        build_tuned(vec![input(&text)], 1000.0, &b, 1_000, None, None, Tuning { bucket: 700, leaf: 500 }).unwrap();
        let (a, b) = (Index::open(&a).unwrap(), Index::open(&b).unwrap());
        assert!(b.nodes.len() > 8);
        within_a_millimetre(&all(&a), &all(&b));
    }

    #[test]
    fn several_files_are_one_index_with_a_station_each() {
        let (one, two) = (pts(5_000, 4), pts(7_000, 5));
        let out = temp("stations");
        let built = build_tuned(
            vec![Input { name: "a.pts".into(), ..input(&one) }, Input { name: "b.pts".into(), ..input(&two) }],
            1000.0,
            &out,
            100_000,
            None,
            None,
            Tuning::default(),
        )
        .unwrap();
        assert_eq!(built.count, 12_000);
        assert_eq!(
            built.stations,
            vec![Station { source: "a.pts".into(), points: 5_000 }, Station { source: "b.pts".into(), points: 7_000 }]
        );
        assert_eq!(built.scan.points.len(), 12_000);
    }

    #[test]
    fn a_cancelled_build_leaves_nothing() {
        let text = pts(200_000, 6);
        let out = temp("cancel");
        let progress = Arc::new(Progress::new());
        progress.cancel();
        let e = build_tuned(vec![input(&text)], 1000.0, &out, 1_000, None, Some(progress), Tuning::default()).unwrap_err();
        assert_eq!(e, crate::progress::CANCELLED);
        assert_eq!(std::fs::read_dir(out.parent().unwrap()).unwrap().count(), 0);
    }

    #[test]
    fn a_hostile_index_is_refused_never_panics() {
        let out = temp("hostile");
        build_tuned(vec![input(&pts(20_000, 7))], 1000.0, &out, 1_000, None, None, Tuning { bucket: 1_000_000, leaf: 1_000 }).unwrap();
        let good = std::fs::read(&out).unwrap();
        let open = Index::from_bytes;
        assert!(open(good.clone()).is_ok());
        // Cut short anywhere, or nothing.
        for n in [0, 7, 100, 128, good.len() / 2, good.len() - 1] {
            assert!(open(good[..n].to_vec()).is_err(), "cut at {n}");
        }
        // Not ours.
        let mut b = good.clone();
        b[0] = b'X';
        assert!(open(b).unwrap_err().contains("isn't a BIMCraft"));
        // A count that lies.
        let mut b = good.clone();
        b[32..40].copy_from_slice(&123u64.to_le_bytes());
        assert!(open(b).is_err());
        // Bytes flipped all over: opening and reading every node never panics.
        let mut x = 99_u64;
        for round in 0..400 {
            let mut b = good.clone();
            for _ in 0..1 + round % 8 {
                x = crate::model::mix(x);
                let at = (x as usize) % b.len();
                b[at] ^= (x >> 32) as u8 | 1;
            }
            if let Ok(index) = open(b) {
                for i in 0..index.nodes.len() {
                    let _ = index.load(i);
                }
                let _ = index.children(index.root()).count();
            }
        }
        // A table whose node's points lie past the table.
        let index = open(good.clone()).unwrap();
        let mut b = good.clone();
        let entry = index.header.table as usize;
        b[entry + 16..entry + 24].copy_from_slice(&(good.len() as u64).to_le_bytes());
        assert!(open(b).is_err());
        assert!(Index::open(&out.with_extension("missing")).unwrap_err().contains("can't be opened"));
    }
}
