//! COPC (Cloud Optimized Point Cloud, copc.io, open): LAZ 1.4 whose points are written node by node of an octree, each node a
//! LASzip chunk of its own, with a hierarchy of the nodes in records of the file's own. A view reads it as it reads an index of
//! ours ([`NodeSource`]), so a COPC file from PDAL, QGIS or `copc_converter` is drawn at the detail a view shows without
//! building one. The points of a node are decompressed by the laz crate, as a LAZ file's chunks are (`cloud/laz.rs`).
//!
//! The file, as COPC 1.0 says: LAS 1.4's header, then the COPC info record (user `copc`, record 1, 160 bytes: the root cube's
//! centre and half its width, the root's spacing, and where the root's page of the hierarchy is), LASzip's record, the points,
//! and the hierarchy's pages, each a list of entries of 32 bytes: a node's key (level, x, y, z, as `i32`), where its chunk is
//! and its bytes, and its points, or -1 for an entry that's another page.

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, SeekFrom};
use std::path::Path;

use super::index::{MAX_DEPTH, MAX_NODE_POINTS, MAX_NODES, NodePoints};
use super::source::NodeSource;
use super::{GREY, LasHead, Source, unread};

/// COPC's info record: who writes it and its number, and how long it is.
const COPC_USER: &[u8] = b"copc";
const INFO_RECORD: u16 = 1;
const INFO_LEN: usize = 160;
/// The most pages of the hierarchy a file may have.
const MAX_PAGES: usize = 1 << 16;
/// The most a node's chunk may take in the file, bytes.
const MAX_CHUNK_BYTES: u64 = 64 << 20;

fn broken(e: impl std::fmt::Display) -> String {
    format!("the COPC file can't be read: {e}")
}

/// A node of the hierarchy.
#[derive(Clone, Copy, Debug)]
struct Node {
    level: u8,
    key: [u32; 3],
    offset: u64,
    bytes: u64,
    points: u32,
}

/// A COPC file opened: its hierarchy, its points read a node at a time.
pub struct Copc {
    head: LasHead,
    vlr: laz::LazVlr,
    nodes: Vec<Node>,
    /// The root cube's lowest corner from the scan's middle, and its width, mm; the root's spacing, mm.
    min: [f64; 3],
    edge: f64,
    spacing: f64,
    /// Millimetres to the file's unit, and the scan's middle in the file's coordinates, mm.
    scale: f64,
    origin: [f64; 3],
    id: u128,
    count: u64,
    /// The brightest colour a channel holds (sixteen bits or eight), and the range of intensity, from the root's points.
    full: f64,
    bright: (f64, f64),
    file: File,
    size: u64,
}

impl std::fmt::Debug for Copc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Copc({} points, {} nodes)", self.count, self.nodes.len())
    }
}

/// The COPC info record of a LAZ file `src` (whose header `h` is), if it has one: COPC puts it first.
fn info(src: &mut dyn Source, h: &LasHead) -> Option<[u8; INFO_LEN]> {
    if h.vlrs == 0 || h.header + 54 + INFO_LEN as u64 > h.start {
        return None;
    }
    let mut head = [0u8; 54];
    src.seek(SeekFrom::Start(h.header)).ok()?;
    src.read_exact(&mut head).ok()?;
    let user = &head[2..18];
    let user = &user[..user.iter().position(|&b| b == 0).unwrap_or(user.len())];
    let (id, len) = (u16::from_le_bytes([head[18], head[19]]), u16::from_le_bytes([head[20], head[21]]));
    if user != COPC_USER || id != INFO_RECORD || usize::from(len) != INFO_LEN {
        return None;
    }
    let mut b = [0u8; INFO_LEN];
    src.read_exact(&mut b).ok()?;
    Some(b)
}

/// Whether the LAZ file `src` is COPC.
pub fn is_copc(src: &mut dyn Source) -> bool {
    let Ok(size) = src.seek(SeekFrom::End(0)) else { return false };
    if src.seek(SeekFrom::Start(0)).is_err() {
        return false;
    }
    LasHead::read(src, size).ok().filter(|h| h.compressed).is_some_and(|h| info(src, &h).is_some())
}

fn f64_at(b: &[u8], at: usize) -> f64 {
    b.get(at..at + 8).map_or(f64::NAN, |s| f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8).map_or(0, |s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    b.get(at..at + 4).map_or(0, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

impl Copc {
    /// The COPC file at `path`, its coordinates `scale` mm to their unit: its header, its records and its hierarchy read and
    /// checked, and its root's points to see how its colours and intensities run. Its points are read when asked for.
    pub fn open(path: &Path, scale: f64) -> Result<Copc, String> {
        let mut file = File::open(path).map_err(|e| format!("{} can't be opened: {e}", path.display()))?;
        Copc::read(file.try_clone().map_err(unread)?, &mut file, scale)
    }

    fn read(file: File, src: &mut dyn Source, scale: f64) -> Result<Copc, String> {
        if !(scale.is_finite() && scale > 0.0) {
            return Err(format!("a unit of {scale} mm isn't one"));
        }
        let size = src.seek(SeekFrom::End(0)).map_err(unread)?;
        src.seek(SeekFrom::Start(0)).map_err(unread)?;
        let h = LasHead::read(src, size)?;
        if !h.compressed {
            return Err(broken("its points aren't compressed: it's LAS, not COPC"));
        }
        let info = info(src, &h).ok_or_else(|| broken("it has no COPC info record first among its records: it's LAZ, not COPC"))?;
        let data = super::laz::laszip_record(src, &h)?;
        let vlr = laz::LazVlr::from_buffer(&data).map_err(broken)?;
        if vlr.items_size() != h.len as u64 {
            return Err(broken("its records are not the length LASzip's record says"));
        }
        // The header's bounds, for the scan's middle, and the cube.
        let mut head = Vec::with_capacity(375);
        src.seek(SeekFrom::Start(0)).map_err(unread)?;
        (&mut *src).take(375).read_to_end(&mut head).map_err(unread)?;
        let (hi, lo): ([f64; 3], [f64; 3]) =
            (std::array::from_fn(|k| f64_at(&head, 179 + 16 * k)), std::array::from_fn(|k| f64_at(&head, 187 + 16 * k)));
        let bounds_ok = lo.iter().chain(&hi).all(|v| v.is_finite() && (v * scale).abs() <= super::MAX_FILE_COORD);
        if !bounds_ok || (0..3).any(|k| hi[k] < lo[k] || (hi[k] - lo[k]) * scale > super::MAX_SPAN) {
            return Err(broken("its bounds aren't a scan's"));
        }
        let origin = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0 * scale);
        let centre: [f64; 3] = std::array::from_fn(|k| f64_at(&info, 8 * k));
        let (half, spacing) = (f64_at(&info, 24), f64_at(&info, 32));
        let (root_at, root_bytes) = (u64_at(&info, 40), u64_at(&info, 48));
        if !centre.iter().all(|v| v.is_finite() && (v * scale).abs() <= super::MAX_FILE_COORD)
            || !(half.is_finite() && half > 0.0 && half * 2.0 * scale <= 2.0 * super::MAX_SPAN)
            || !(spacing.is_finite() && spacing > 0.0)
        {
            return Err(broken("its info record's cube isn't one"));
        }
        let nodes = hierarchy(src, size, root_at, root_bytes, &h)?;
        let count: u64 = nodes.iter().map(|n| u64::from(n.points)).sum();
        // A file's own: what it is, so another isn't taken for it.
        let id = {
            let mut x = crate::model::mix(size ^ count.rotate_left(17));
            for b in head.iter().chain(&info) {
                x = crate::model::mix(x ^ u64::from(*b));
            }
            (u128::from(x) << 64) | u128::from(crate::model::mix(x ^ 0x9E37_79B9_7F4A_7C15))
        };
        let mut c = Copc {
            head: h,
            vlr,
            nodes,
            min: std::array::from_fn(|k| (centre[k] - half) * scale - origin[k]),
            edge: half * 2.0 * scale,
            spacing: spacing * scale,
            scale,
            origin,
            id,
            count,
            full: 65535.0,
            bright: (0.0, 65535.0),
            file,
            size,
        };
        // How its colours and intensities run, from its root's points.
        let root = c.raw(0)?;
        let most = root.iter().filter_map(|r| r.rgb).flatten().fold(0.0_f64, f64::max);
        c.full = if most > 255.0 { 65535.0 } else { 255.0 };
        let (lo, hi) = root.iter().filter_map(|r| r.intensity).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        c.bright = if hi > lo { (lo, hi) } else { (0.0, 65535.0) };
        Ok(c)
    }

    /// The levels from the root whole that hold no more than `cap` points, and how many they hold.
    pub fn levels_within(&self, cap: usize) -> (u8, u64) {
        let mut per = vec![0_u64; usize::from(MAX_DEPTH) + 1];
        for n in &self.nodes {
            per[usize::from(n.level)] += u64::from(n.points);
        }
        let (mut levels, mut held) = (0_u8, 0_u64);
        while usize::from(levels) < per.len() && held + per[usize::from(levels)] <= cap as u64 {
            held += per[usize::from(levels)];
            levels += 1;
            if held == self.count {
                break;
            }
        }
        (levels, held)
    }

    /// The scan's middle in its file's coordinates, mm: a [`super::Cloud`]'s origin.
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// Node `i`'s points as the file holds them.
    fn raw(&self, i: usize) -> Result<Vec<super::Raw>, String> {
        let n = *self.nodes.get(i).ok_or_else(|| broken("no such node"))?;
        if n.points == 0 {
            return Ok(Vec::new());
        }
        let mut packed = vec![0u8; usize::try_from(n.bytes).map_err(|_| broken("a node too big"))?];
        read_at(&self.file, n.offset, &mut packed)?;
        let mut out = vec![0u8; n.points as usize * self.head.len];
        super::laz::decompressor(&self.vlr, &packed)?.decompress_many(&mut out).map_err(broken)?;
        out.chunks_exact(self.head.len).map(|r| self.head.point(r)).collect()
    }
}

/// `buf` read from `file` at `at`, with no cursor moved.
fn read_at(file: &File, at: u64, buf: &mut [u8]) -> Result<(), String> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::read_exact_at(file, buf, at).map_err(broken);
    #[cfg(windows)]
    {
        let (mut done, mut at) = (0, at);
        while done < buf.len() {
            let n = std::os::windows::fs::FileExt::seek_read(file, &mut buf[done..], at).map_err(broken)?;
            if n == 0 {
                return Err(broken("it's cut short"));
            }
            (done, at) = (done + n, at + n as u64);
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let mut f = file;
        f.seek(SeekFrom::Start(at)).map_err(broken)?;
        f.read_exact(buf).map_err(broken)
    }
}

/// The nodes the hierarchy's pages list, from the root's page at `at` of `bytes`, checked: no more than [`MAX_PAGES`] pages
/// and [`MAX_NODES`] nodes, each page read once, chunks within the file before the hierarchy and not too big, and their points
/// within a node's and the header's. In the order of their places (level, then key), the root first.
fn hierarchy(src: &mut dyn Source, size: u64, at: u64, bytes: u64, h: &LasHead) -> Result<Vec<Node>, String> {
    let mut nodes = Vec::new();
    let mut pages = VecDeque::from([(at, bytes)]);
    let (mut seen, mut read) = (HashSet::new(), 0_usize);
    let mut places = HashSet::new();
    while let Some((at, bytes)) = pages.pop_front() {
        if !seen.insert(at) {
            continue;
        }
        read += 1;
        if read > MAX_PAGES {
            return Err(broken(format!("its hierarchy has more than {MAX_PAGES} pages")));
        }
        if bytes % 32 != 0 || at.checked_add(bytes).is_none_or(|end| end > size) || bytes > 32 * MAX_NODES {
            return Err(broken("a page of its hierarchy lies outside the file or is cut short"));
        }
        let mut b = vec![0u8; bytes as usize];
        src.seek(SeekFrom::Start(at)).map_err(unread)?;
        src.read_exact(&mut b).map_err(unread)?;
        for e in b.as_chunks::<32>().0 {
            let (level, key) = (i32_at(e, 0), [i32_at(e, 4), i32_at(e, 8), i32_at(e, 12)]);
            let (offset, size_of, points) = (u64_at(e, 16), i32_at(e, 24), i32_at(e, 28));
            let Ok(level) = u8::try_from(level) else { return Err(broken("a node's level isn't one")) };
            if level > MAX_DEPTH || key.iter().any(|&k| k < 0 || (k as u64) >> level != 0) {
                return Err(broken(format!("a node at level {level} lies outside the cube")));
            }
            let Ok(len) = u64::try_from(size_of) else { return Err(broken("an entry's size isn't one")) };
            if points == -1 {
                pages.push_back((offset, len));
                continue;
            }
            let Ok(points) = u32::try_from(points) else { return Err(broken("an entry's points aren't a count")) };
            if points > MAX_NODE_POINTS || len > MAX_CHUNK_BYTES || offset.checked_add(len).is_none_or(|end| end > size) || offset < h.start
            {
                return Err(broken(format!("a node says it holds {points} points in {len} bytes at {offset}")));
            }
            let key = key.map(|k| k as u32);
            if !places.insert((level, key)) {
                return Err(broken("two nodes are in one place"));
            }
            nodes.push(Node { level, key, offset, bytes: len, points });
            if nodes.len() as u64 > MAX_NODES {
                return Err(broken(format!("its hierarchy has more than {MAX_NODES} nodes")));
            }
        }
    }
    nodes.sort_by_key(|n| (n.level, n.key));
    if nodes.first().is_none_or(|n| n.level != 0) {
        return Err(broken("its hierarchy has no root"));
    }
    let total: u64 = nodes.iter().map(|n| u64::from(n.points)).sum();
    if total > h.count {
        return Err(broken(format!("its nodes hold {total} points, and its header says {}", h.count)));
    }
    Ok(nodes)
}

impl NodeSource for Copc {
    fn id(&self) -> u128 {
        self.id
    }

    fn count(&self) -> u64 {
        self.count
    }

    fn len(&self) -> usize {
        self.nodes.len()
    }

    fn node(&self, i: usize) -> Option<(u8, u32)> {
        self.nodes.get(i).map(|n| (n.level, n.points))
    }

    fn children(&self, i: usize) -> Vec<usize> {
        let Some(n) = self.nodes.get(i) else { return Vec::new() };
        if n.level >= MAX_DEPTH {
            return Vec::new();
        }
        (0..8_u32)
            .filter_map(|o| {
                let key = [0, 1, 2].map(|k| (n.key[k] << 1) | ((o >> k) & 1));
                self.nodes.binary_search_by(|m| (m.level, m.key).cmp(&(n.level + 1, key))).ok()
            })
            .collect()
    }

    fn cube(&self, i: usize) -> ([f64; 3], f64) {
        let Some(n) = self.nodes.get(i) else { return (self.min, self.edge) };
        let edge = self.edge / 2f64.powi(i32::from(n.level));
        ([0, 1, 2].map(|k| self.min[k] + f64::from(n.key[k]) * edge), edge)
    }

    fn spacing(&self, level: u8) -> f64 {
        self.spacing / 2f64.powi(i32::from(level))
    }

    fn load(&self, i: usize) -> Result<NodePoints, String> {
        let raw = self.raw(i)?;
        let byte = |v: f64| if v.is_finite() { (v * 255.0).round().clamp(0.0, 255.0) as u8 } else { GREY };
        let mut out = NodePoints { points: Vec::with_capacity(raw.len()), ..NodePoints::default() };
        let coloured = self.head.colour.is_some();
        let mut colours = coloured.then(|| Vec::with_capacity(raw.len()));
        let mut levels = Vec::with_capacity(raw.len());
        let (lo, hi) = self.bright;
        for r in &raw {
            out.points.push([0, 1, 2].map(|k| (r.p[k] * self.scale - self.origin[k]) as f32));
            if let Some(c) = &mut colours {
                c.push(r.rgb.map_or([GREY; 3], |rgb| rgb.map(|v| byte(v / self.full))));
            }
            levels.push(byte(r.intensity.map_or(0.5, |v| (v - lo) / (hi - lo))));
        }
        out.colours = colours;
        out.intensity = Some(levels);
        Ok(out)
    }

    fn size(&self) -> u64 {
        self.size
    }
}

/// What the top of a COPC file, the levels from its root that fit in `cap` points, comes to as a scan: the points, colours and
/// intensities of those levels, and how many levels.
pub fn top(copc: &Copc, cap: usize) -> Result<(super::Scan, u8), String> {
    let (levels, _) = copc.levels_within(cap.max(1));
    let mut scan = super::Scan {
        origin: copc.origin,
        points: Vec::new(),
        colours: copc.head.colour.is_some().then(Vec::new),
        intensity: Some(Vec::new()),
        read: usize::try_from(copc.count).unwrap_or(usize::MAX),
    };
    let take: Vec<usize> = if levels == 0 { vec![0] } else { (0..copc.nodes.len()).filter(|&i| copc.nodes[i].level < levels).collect() };
    for i in take {
        let p = copc.load(i)?;
        let every = if levels == 0 { p.points.len().div_ceil(cap.max(1)).max(1) } else { 1 };
        for k in (0..p.points.len()).step_by(every) {
            scan.points.push(p.points[k]);
            if let (Some(c), Some(from)) = (&mut scan.colours, &p.colours) {
                c.push(from.get(k).copied().unwrap_or([GREY; 3]));
            }
            if let (Some(l), Some(from)) = (&mut scan.intensity, &p.intensity) {
                l.push(from.get(k).copied().unwrap_or(128));
            }
        }
    }
    if scan.points.is_empty() {
        return Err(broken("its top holds no points"));
    }
    Ok((scan, levels))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::{Cursor, Seek, SeekFrom, Write};

    use super::super::tests::las;
    use super::*;

    /// A COPC file of `n` points over a cube 20 m wide about (10, 10, 10) m, as a writer lays one: its root's points first, then
    /// its two children's at level 1 (x below the middle, and above), each a LASzip chunk of its own, then the hierarchy's page.
    /// Point format 7 (colours), a millimetre to a unit; each point red and its intensity its place among them.
    pub(crate) fn copc(n: i32) -> Vec<u8> {
        let mut pts: Vec<([i32; 3], u16, [u16; 3])> =
            (0..n).map(|i| ([(i * 37) % 20_000, (i * 53) % 20_000, (i * 11) % 20_000], (i % 1000) as u16, [65535, 0, 0])).collect();
        // The root takes every tenth, the children the rest by their side of the middle.
        let (mut root, mut low, mut high) = (Vec::new(), Vec::new(), Vec::new());
        for (i, p) in pts.drain(..).enumerate() {
            if i % 10 == 0 {
                root.push(p);
            } else if p.0[0] < 10_000 {
                low.push(p);
            } else {
                high.push(p);
            }
        }
        let nodes: Vec<(i32, [i32; 3], usize)> = vec![(0, [0, 0, 0], root.len()), (1, [0, 0, 0], low.len()), (1, [1, 0, 0], high.len())];
        let ordered: Vec<_> = root.into_iter().chain(low).chain(high).collect();
        let plain = las(4, 7, [0.001; 3], [0.0; 3], 0, &ordered);
        let (header, len) = (375_usize, 36_usize);
        let vlr = laz::LazVlrBuilder::default().with_point_format(7, 0).unwrap().with_variable_chunk_size().build();
        let mut data = Vec::new();
        vlr.write_to(&mut data).unwrap();
        let record = |user: &[u8], id: u16, body: &[u8]| {
            let mut b = vec![0u8; 54];
            b[2..2 + user.len()].copy_from_slice(user);
            b[18..20].copy_from_slice(&id.to_le_bytes());
            b[20..22].copy_from_slice(&(body.len() as u16).to_le_bytes());
            b.extend_from_slice(body);
            b
        };
        let mut info = vec![0u8; INFO_LEN];
        let mut records = record(COPC_USER, INFO_RECORD, &info);
        records.extend(record(b"laszip encoded", 22204, &data));
        let start = header + records.len();
        let mut head = plain[..header].to_vec();
        // The header's bounds, as a writer of COPC sets them.
        for k in 0..3 {
            let (lo, hi) = ordered.iter().fold((i32::MAX, i32::MIN), |(a, b), p| (a.min(p.0[k]), b.max(p.0[k])));
            head[179 + 16 * k..187 + 16 * k].copy_from_slice(&(f64::from(hi) * 0.001).to_le_bytes());
            head[187 + 16 * k..195 + 16 * k].copy_from_slice(&(f64::from(lo) * 0.001).to_le_bytes());
        }
        head[96..100].copy_from_slice(&(start as u32).to_le_bytes());
        head[100..104].copy_from_slice(&2u32.to_le_bytes());
        head[104] |= 0x80;
        let mut out = Cursor::new(Vec::new());
        out.write_all(&head).unwrap();
        out.write_all(&records).unwrap();
        let points = &plain[header..];
        let mut c = laz::LasZipCompressor::new(out, vlr).unwrap();
        let mut at = 0;
        let mut chunks = Vec::new();
        for (_, _, k) in &nodes {
            chunks.push(&points[at * len..(at + k) * len]);
            at += k;
        }
        c.compress_chunks(chunks).unwrap();
        c.done().unwrap();
        let mut out = c.into_inner();
        // Where each node's chunk is, from the table the compressor wrote.
        out.seek(SeekFrom::Start(start as u64)).unwrap();
        let mut b = [0u8; 8];
        std::io::Read::read_exact(&mut out, &mut b).unwrap();
        out.seek(SeekFrom::Start(u64::from_le_bytes(b))).unwrap();
        let table = laz::laszip::ChunkTable::read(&mut out, true).unwrap();
        let mut file = out.into_inner();
        let mut page = Vec::new();
        let mut offset = start as u64 + 8;
        for ((level, key, k), e) in nodes.iter().zip(table.as_ref()) {
            for v in [*level, key[0], key[1], key[2]] {
                page.extend(v.to_le_bytes());
            }
            page.extend(offset.to_le_bytes());
            page.extend((e.byte_count as i32).to_le_bytes());
            page.extend((*k as i32).to_le_bytes());
            offset += e.byte_count;
        }
        let page_at = file.len() as u64;
        file.extend(&page);
        for (k, v) in [10.0f64, 10.0, 10.0, 10.0, 0.2].iter().enumerate() {
            info[8 * k..8 * k + 8].copy_from_slice(&v.to_le_bytes());
        }
        info[40..48].copy_from_slice(&page_at.to_le_bytes());
        info[48..56].copy_from_slice(&(page.len() as u64).to_le_bytes());
        file[header + 54..header + 54 + INFO_LEN].copy_from_slice(&info);
        file
    }

    fn opened(bytes: &[u8], name: &str) -> Result<Copc, String> {
        let dir = std::env::temp_dir().join(format!("bimcraft-copc-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scan.copc.laz");
        std::fs::write(&path, bytes).unwrap();
        Copc::open(&path, 1000.0)
    }

    #[test]
    fn a_copc_file_is_read_node_by_node_as_the_laz_it_is() {
        let file = copc(3_000);
        assert!(is_copc(&mut Cursor::new(&file)));
        let c = opened(&file, "read").unwrap();
        assert_eq!((c.count(), c.len()), (3_000, 3));
        assert_eq!(c.children(0), vec![1, 2]);
        assert!(c.children(1).is_empty());
        let (lo, edge) = c.cube(2);
        assert!((edge - 10_000.0).abs() < 1e-6 && (lo[0] - c.cube(0).0[0] - 10_000.0).abs() < 1e-6, "{lo:?} {edge}");
        assert!((c.spacing(1) - 100.0).abs() < 1e-9);
        // Every node's points, together, are the file's, read as LAZ, to a millimetre.
        let mut ours: Vec<[f32; 3]> = (0..3).flat_map(|i| c.load(i).unwrap().points).collect();
        let laz =
            super::super::read(&file, super::super::Format::Laz, &super::super::Options { cap: 10_000, ..Default::default() }).unwrap();
        let mut theirs = laz.points.clone();
        assert_eq!(laz.origin, c.origin());
        ours.sort_by(|a, b| a.partial_cmp(b).unwrap());
        theirs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(ours.len(), theirs.len());
        assert!(ours.iter().zip(&theirs).all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() < 0.01)));
        let p = c.load(1).unwrap();
        assert!(p.colours.unwrap().iter().all(|c| *c == [255, 0, 0]));
        // The top within 400 points is the root alone, whole.
        let (scan, levels) = top(&c, 400).unwrap();
        assert_eq!((levels, scan.points.len()), (1, 300));
        // A plain LAZ file isn't COPC.
        let plain = las(4, 7, [0.001; 3], [0.0; 3], 0, &[([1, 2, 3], 4, [5, 6, 7])]);
        assert!(!is_copc(&mut Cursor::new(&plain)));
    }

    #[test]
    fn a_hostile_copc_file_is_refused_and_never_crashes_it() {
        let good = copc(2_000);
        let info_at = 375 + 54;
        // The root's page past the file's end.
        let mut b = good.clone();
        b[info_at + 40..info_at + 48].copy_from_slice(&(good.len() as u64).to_le_bytes());
        assert!(opened(&b, "past").unwrap_err().contains("outside the file"));
        // A page that names itself as its child: read once, then the nodes it lists twice are two in one place.
        let page_at = u64::from_le_bytes(good[info_at + 40..info_at + 48].try_into().unwrap()) as usize;
        let mut b = good.clone();
        b[page_at + 32 + 28..page_at + 32 + 32].copy_from_slice(&(-1i32).to_le_bytes());
        b[page_at + 32 + 16..page_at + 32 + 24].copy_from_slice(&(page_at as u64).to_le_bytes());
        b[page_at + 32 + 24..page_at + 32 + 28].copy_from_slice(&96i32.to_le_bytes());
        let _ = opened(&b, "cycle");
        // Bytes flipped all over: opening and reading every node never panics.
        let mut x = 7_u64;
        for round in 0..200 {
            let mut b = good.clone();
            for _ in 0..1 + round % 6 {
                x = crate::model::mix(x);
                let at = (x as usize) % b.len();
                b[at] ^= (x >> 32) as u8 | 1;
            }
            if let Ok(c) = opened(&b, "flip") {
                for i in 0..c.len() {
                    let _ = c.load(i);
                    let _ = c.children(i);
                }
            }
        }
    }
}
