//! Terrain: a triangulated irregular network (TIN) from surveyed points, breaklines for ridges, valleys and kerbs, and a
//! boundary, with the height at a point, contours and areas. Revit's toposurface. Plain geometry: the element that holds the
//! points is `Element::Toposurface`, and the plans, 3D view, schedules and IFC read the TIN from here.
//!
//! The triangulation is `spade`'s constrained Delaunay: breaklines are constraint edges, so no triangle crosses them. Everything
//! is in mm, x east, y north, z up.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use spade::handles::FixedVertexHandle;
use spade::{ConstrainedDelaunayTriangulation, HasPosition, InsertionError, Point2, Triangulation};

use crate::geom::{self, P2};
use crate::view3d::P3;

/// The most contour lines one surface makes; a finer interval is refused with the interval that would do.
pub const MAX_CONTOURS: usize = 1000;

/// A contour line: the points of one level, joined.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    pub z: f64,
    pub points: Vec<P2>,
    /// It ends where it began.
    pub closed: bool,
}

/// Contours already worked out for an interval and a base (their bits).
type Memo = ((u64, u64), Arc<Vec<Contour>>);

/// What a pad takes out of the ground and puts in, mm³: where the ground is above its top the earth is cut, where below it, filled.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CutFill {
    pub cut: f64,
    pub fill: f64,
    /// The area of the pad that is over the surface, mm².
    pub covered: f64,
    /// The pad's whole area, mm².
    pub pad_area: f64,
}

/// What lies between two surfaces, where both are, taken a `cell` at a time: cut where the first is above the second, fill where below.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Between {
    pub cut: f64,
    pub fill: f64,
    /// The area compared, mm².
    pub area: f64,
    pub cells: usize,
    pub cell: f64,
}

/// The volume between two surfaces over where both are: the midpoint of each `cell` mm square, up to `most` cells.
pub fn between(first: &Tin, second: &Tin, cell: f64, most: usize) -> Result<Between, String> {
    if !cell.is_finite() || cell <= 0.0 {
        return Err(format!("`cell` is a size in mm above 0, not {cell}"));
    }
    let lo = [first.lo[0].max(second.lo[0]), first.lo[1].max(second.lo[1])];
    let hi = [first.hi[0].min(second.hi[0]), first.hi[1].min(second.hi[1])];
    if hi[0] <= lo[0] || hi[1] <= lo[1] {
        return Err("the surfaces do not overlap".into());
    }
    let (nx, ny) = (((hi[0] - lo[0]) / cell).ceil(), ((hi[1] - lo[1]) / cell).ceil());
    if nx * ny > most as f64 {
        return Err(format!("a cell of {cell} mm would make {} cells over the surfaces, more than {most}: use a larger `cell`", nx * ny));
    }
    let (nx, ny) = (nx as usize, ny as usize);
    let mut r = Between { cell, cells: nx * ny, ..Between::default() };
    let unit = cell * cell;
    for j in 0..ny {
        for i in 0..nx {
            let (x, y) = (lo[0] + (i as f64 + 0.5) * cell, lo[1] + (j as f64 + 0.5) * cell);
            let (Some(a), Some(b)) = (first.height_at(x, y), second.height_at(x, y)) else { continue };
            r.area += unit;
            if a > b {
                r.cut += (a - b) * unit;
            } else {
                r.fill += (b - a) * unit;
            }
        }
    }
    Ok(r)
}

/// The part of `poly` inside the triangle `tri`, whichever way round it is (Sutherland-Hodgman: a triangle is convex, so a polygon of any
/// shape is clipped by its three sides in turn).
fn clip_to(mut poly: Vec<P2>, tri: [P2; 3]) -> Vec<P2> {
    let tri = if (tri[1] - tri[0]).cross(tri[2] - tri[0]) < 0.0 { [tri[0], tri[2], tri[1]] } else { tri };
    for k in 0..3 {
        let (a, b) = (tri[k], tri[(k + 1) % 3]);
        let side = |p: P2| (b - a).cross(p - a);
        let mut out = Vec::with_capacity(poly.len() + 2);
        for i in 0..poly.len() {
            let (prev, cur) = (poly[(i + poly.len() - 1) % poly.len()], poly[i]);
            let (sp, sc) = (side(prev), side(cur));
            if (sc >= 0.0) != (sp >= 0.0) {
                let t = sp / (sp - sc);
                out.push(prev + (cur - prev) * t);
            }
            if sc >= 0.0 {
                out.push(cur);
            }
        }
        poly = out;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    poly
}

/// The height of the plane through the triangle `t`, over (x, y), even where that is off the triangle.
fn plane_at(t: &[P3; 3], x: f64, y: f64) -> Option<f64> {
    let [a, b, c] = t;
    let d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if d.abs() < 1e-12 {
        return None;
    }
    let gx = ((b[2] - a[2]) * (c[1] - a[1]) - (c[2] - a[2]) * (b[1] - a[1])) / d;
    let gy = ((c[2] - a[2]) * (b[0] - a[0]) - (b[2] - a[2]) * (c[0] - a[0])) / d;
    Some(a[2] + gx * (x - a[0]) + gy * (y - a[1]))
}

/// The mean of the positive part of the linear function that has `v` at the corners of a triangle: exact, by cutting the triangle where
/// the function is zero.
fn positive(v: [f64; 3]) -> f64 {
    let mut s = v;
    s.sort_by(|a, b| b.total_cmp(a));
    let (a, b, c) = (s[0], s[1], s[2]);
    if c >= 0.0 {
        (a + b + c) / 3.0
    } else if a <= 0.0 {
        0.0
    } else if b >= 0.0 {
        // Only the lowest corner is below zero: take from the whole the corner that is.
        let (t1, t2) = (-c / (a - c), -c / (b - c));
        (a + b + c) / 3.0 - t1 * t2 * c / 3.0
    } else {
        // Only the highest is above zero: the corner that is, a triangle of it.
        let (s2, s3) = (a / (a - b), a / (a - c));
        s2 * s3 * a / 3.0
    }
}

impl Tin {
    /// The triangles whose box meets the box `lo`..`hi`.
    fn near(&self, lo: [f64; 2], hi: [f64; 2]) -> Vec<u32> {
        let cell = |v: f64, i: usize| (((v - self.lo[i]) / self.size[i]).floor().max(0.0) as usize).min(self.n - 1);
        if hi[0] < self.lo[0] || hi[1] < self.lo[1] || lo[0] > self.hi[0] || lo[1] > self.hi[1] {
            return Vec::new();
        }
        let mut found: Vec<u32> = self.big.clone();
        for cy in cell(lo[1], 1)..=cell(hi[1], 1) {
            for cx in cell(lo[0], 0)..=cell(hi[0], 0) {
                found.extend(&self.cells[cy * self.n + cx]);
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }

    /// What a flat pad of outline `ring` with its top at `z` cuts and fills in this ground: exact, since the ground is flat in each
    /// triangle and the pad is cut into triangles that meet them.
    pub fn cut_fill(&self, ring: &[P2], z: f64) -> Result<CutFill, String> {
        if ring.len() < 3 || !z.is_finite() || ring.iter().any(|p| !p.0.is_finite() || !p.1.is_finite()) {
            return Err("a pad needs an outline of three points or more, and a height, that are numbers".into());
        }
        if geom::area(ring).abs() < 1e-6 {
            return Err("the pad's outline encloses no area".into());
        }
        let pieces = geom::triangulate(ring);
        if pieces.is_empty() {
            return Err("the pad's outline cannot be cut into triangles: does it cross itself?".into());
        }
        let mut r = CutFill::default();
        for [i, j, k] in pieces {
            let pad = [ring[i], ring[j], ring[k]];
            r.pad_area += geom::area(&pad).abs();
            let lo = [pad.iter().map(|p| p.0).fold(f64::MAX, f64::min), pad.iter().map(|p| p.1).fold(f64::MAX, f64::min)];
            let hi = [pad.iter().map(|p| p.0).fold(f64::MIN, f64::max), pad.iter().map(|p| p.1).fold(f64::MIN, f64::max)];
            for n in self.near(lo, hi) {
                let t = &self.tris[n as usize];
                let piece = clip_to(pad.to_vec(), [P2(t[0][0], t[0][1]), P2(t[1][0], t[1][1]), P2(t[2][0], t[2][1])]);
                for m in 1..piece.len().saturating_sub(1) {
                    let corner = [piece[0], piece[m], piece[m + 1]];
                    let area = geom::area(&corner).abs();
                    let Some(f) = corner.iter().map(|p| plane_at(t, p.0, p.1).map(|h| h - z)).collect::<Option<Vec<f64>>>() else {
                        continue;
                    };
                    r.covered += area;
                    r.cut += area * positive([f[0], f[1], f[2]]);
                    r.fill += area * positive([-f[0], -f[1], -f[2]]);
                }
            }
        }
        Ok(r)
    }
}

/// A terrain's triangles, indexed to find the one over a point.
#[derive(Clone, Debug, Default)]
pub struct Tin {
    tris: Vec<[P3; 3]>,
    /// The index: a grid of `n` x `n` cells `size` wide from `lo`, each with the triangles over it; `big` are over too many.
    lo: [f64; 2],
    hi: [f64; 2],
    size: [f64; 2],
    n: usize,
    cells: Vec<Vec<u32>>,
    big: Vec<u32>,
    /// Contours already worked out, by interval and base, shared with every clone.
    memo: Arc<Mutex<Vec<Memo>>>,
}

/// A vertex of the triangulation: where it is in plan, and how high.
struct Vtx {
    at: Point2<f64>,
    z: f64,
}

impl HasPosition for Vtx {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.at
    }
}

/// The TIN of `points` (x, y, z), with `breaklines` (polylines in 3D that triangle edges follow) and an optional `boundary`
/// (a ring in plan outside which there are no triangles, whole triangles at a time; fewer than three points, none). Of two points
/// over one place the breaklines' and then the first one's height stands; a breakline that crosses an earlier one is not followed.
pub fn build(points: &[P3], breaklines: &[Vec<P3>], boundary: &[P2]) -> Result<Tin, String> {
    if points.len() < 3 {
        return Err("a surface needs at least three points".into());
    }
    let finite = |p: &P3| p.iter().all(|c| c.is_finite());
    if !points.iter().chain(breaklines.iter().flatten()).all(finite) || !boundary.iter().all(|p| p.0.is_finite() && p.1.is_finite()) {
        return Err("every coordinate must be a finite number".into());
    }
    let line = || "its points all lie on one line (or over one place), so there is no surface between them".to_string();
    let wrong = |e: InsertionError| format!("the points cannot be triangulated: {e:?}");
    let mut cdt: ConstrainedDelaunayTriangulation<Vtx> = ConstrainedDelaunayTriangulation::new();
    // Breaklines' vertices first, so their heights stand where a point is over the same place.
    let mut chains: Vec<Vec<FixedVertexHandle>> = Vec::new();
    for l in breaklines {
        let mut chain = Vec::with_capacity(l.len());
        for p in l {
            chain.push(cdt.insert(Vtx { at: Point2::new(p[0], p[1]), z: p[2] }).map_err(wrong)?);
        }
        chains.push(chain);
    }
    for p in points {
        cdt.insert(Vtx { at: Point2::new(p[0], p[1]), z: p[2] }).map_err(wrong)?;
    }
    for chain in &chains {
        for pair in chain.windows(2) {
            if pair[0] != pair[1] && cdt.can_add_constraint(pair[0], pair[1]) {
                cdt.add_constraint(pair[0], pair[1]);
            }
        }
    }
    let ring = if boundary.len() >= 3 { boundary } else { &[] };
    let mut tris = Vec::with_capacity(cdt.num_inner_faces());
    for f in cdt.inner_faces() {
        let [a, b, c] = f.vertices();
        let t = [a, b, c].map(|v| [v.position().x, v.position().y, v.data().z]);
        let twice = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]);
        if twice.abs() < 1e-9 {
            continue;
        }
        if !ring.is_empty() {
            let middle = P2((t[0][0] + t[1][0] + t[2][0]) / 3.0, (t[0][1] + t[1][1] + t[2][1]) / 3.0);
            if !geom::contains(ring, middle) {
                continue;
            }
        }
        tris.push(t);
    }
    if tris.is_empty() {
        return Err(if ring.is_empty() { line() } else { "the boundary leaves no triangle of the surface inside it".into() });
    }
    Ok(Tin::new(tris))
}

/// The terrains built lately, by a digest of what they are made of: drawing a plan again, or asking a height again, is not
/// triangulating again. Failures are kept too.
static BUILT: Mutex<Vec<Built>> = Mutex::new(Vec::new());

/// A terrain built (or not) from points with this digest.
type Built = (u64, Result<Arc<Tin>, String>);
const KEPT: usize = 32;

/// [`build`], remembering the last few: the same points, breaklines and boundary give the same [`Tin`].
pub fn cached(points: &[P3], breaklines: &[Vec<P3>], boundary: &[P2]) -> Result<Arc<Tin>, String> {
    let mut h = DefaultHasher::new();
    points.len().hash(&mut h);
    points.iter().flatten().for_each(|c| c.to_bits().hash(&mut h));
    breaklines.len().hash(&mut h);
    for l in breaklines {
        l.len().hash(&mut h);
        l.iter().flatten().for_each(|c| c.to_bits().hash(&mut h));
    }
    boundary.len().hash(&mut h);
    boundary.iter().for_each(|p| (p.0.to_bits(), p.1.to_bits()).hash(&mut h));
    let key = h.finish();
    if let Ok(mut kept) = BUILT.lock()
        && let Some(i) = kept.iter().position(|(k, _)| *k == key)
    {
        let entry = kept.remove(i);
        let found = entry.1.clone();
        kept.push(entry);
        return found;
    }
    let built = build(points, breaklines, boundary).map(Arc::new);
    if let Ok(mut kept) = BUILT.lock() {
        kept.push((key, built.clone()));
        if kept.len() > KEPT {
            drop(kept.remove(0));
        }
    }
    built
}

/// How many cells a triangle may cover in the index before it is kept apart from them.
const MAX_CELLS: usize = 64;

/// The key of a point in a contour: its coordinates exactly, so lines from two triangles that meet at an edge join.
type Key = (u64, u64);

fn key(p: P2) -> Key {
    // Negative zero is zero.
    ((p.0 + 0.0).to_bits(), (p.1 + 0.0).to_bits())
}

impl Tin {
    fn new(tris: Vec<[P3; 3]>) -> Tin {
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        for p in tris.iter().flatten() {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        // About a triangle or two to a cell.
        let n = ((tris.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
        let size = [((hi[0] - lo[0]) / n as f64).max(1e-9), ((hi[1] - lo[1]) / n as f64).max(1e-9)];
        let mut tin = Tin { tris, lo, hi, size, n, cells: vec![Vec::new(); n * n], big: Vec::new(), memo: Arc::default() };
        for (i, t) in tin.tris.iter().enumerate() {
            let (c0, c1) = tin.cells_of(t);
            if (c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) > MAX_CELLS {
                tin.big.push(i as u32);
            } else {
                for cx in c0.0..=c1.0 {
                    for cy in c0.1..=c1.1 {
                        tin.cells[cy * n + cx].push(i as u32);
                    }
                }
            }
        }
        tin
    }

    /// The cells a triangle's box covers, as (first x, first y) and (last x, last y).
    fn cells_of(&self, t: &[P3; 3]) -> ((usize, usize), (usize, usize)) {
        let cell = |v: f64, i: usize| (((v - self.lo[i]) / self.size[i]).floor().max(0.0) as usize).min(self.n - 1);
        let (mut a, mut b) = ((usize::MAX, usize::MAX), (0, 0));
        for p in t {
            let (cx, cy) = (cell(p[0], 0), cell(p[1], 1));
            a = (a.0.min(cx), a.1.min(cy));
            b = (b.0.max(cx), b.1.max(cy));
        }
        (a, b)
    }

    /// The lower and the upper corner of the box round the surface, in plan.
    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        (self.lo, self.hi)
    }

    pub fn triangles(&self) -> &[[P3; 3]] {
        &self.tris
    }

    /// The surface's height over (x, y), if the point is over a triangle.
    pub fn height_at(&self, x: f64, y: f64) -> Option<f64> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        // The top and right edges are in the last cell.
        let cell = |v: f64, i: usize| (((v - self.lo[i]) / self.size[i]).floor() as usize).min(self.n - 1);
        let inside = x >= self.lo[0] && x <= self.hi[0] && y >= self.lo[1] && y <= self.hi[1];
        let here = if inside { self.cells[cell(y, 1) * self.n + cell(x, 0)].as_slice() } else { &[] };
        here.iter().chain(&self.big).find_map(|i| over(&self.tris[*i as usize], x, y))
    }

    /// The contours every `interval` mm of height, counted from `base`, lowest first. A contour is where the surface is at
    /// a level, with the part of it at or above that level on its one side; a flat part right at a level is not a line.
    pub fn contours(&self, interval: f64, base: f64) -> Result<Vec<Contour>, String> {
        if !interval.is_finite() || interval <= 0.0 || !base.is_finite() {
            return Err("`interval` must be a positive number of mm, and `base` a number".into());
        }
        let Some((zmin, zmax)) = self.z_range() else { return Ok(Vec::new()) };
        let (first, last) = (((zmin - base) / interval).ceil(), ((zmax - base) / interval).floor());
        if last < first {
            return Ok(Vec::new());
        }
        if last - first + 1.0 > MAX_CONTOURS as f64 {
            let least = ((zmax - zmin) / (MAX_CONTOURS as f64 - 1.0)).ceil();
            return Err(format!("an interval of {interval} mm would make more than {MAX_CONTOURS} contours: use at least {least} mm"));
        }
        let level = |k: f64| base + k * interval;
        let mut by_level: BTreeMap<i64, Vec<(P2, P2)>> = BTreeMap::new();
        for t in &self.tris {
            let (lo, hi) = (t.iter().map(|p| p[2]).fold(f64::MAX, f64::min), t.iter().map(|p| p[2]).fold(f64::MIN, f64::max));
            let (k0, k1) = (((lo - base) / interval).ceil().max(first), ((hi - base) / interval).floor().min(last));
            let mut k = k0;
            while k <= k1 {
                if let Some(s) = crossing(t, level(k)) {
                    by_level.entry(k as i64).or_default().push(s);
                }
                k += 1.0;
            }
        }
        let mut out = Vec::new();
        for (k, segs) in by_level {
            join(level(k as f64), segs, &mut out);
        }
        Ok(out)
    }

    /// [`Tin::contours`], remembering the last few for this surface.
    pub fn contours_shared(&self, interval: f64, base: f64) -> Result<Arc<Vec<Contour>>, String> {
        let key = (interval.to_bits(), base.to_bits());
        if let Ok(memo) = self.memo.lock()
            && let Some((_, found)) = memo.iter().find(|(k, _)| *k == key)
        {
            return Ok(found.clone());
        }
        let made = Arc::new(self.contours(interval, base)?);
        if let Ok(mut memo) = self.memo.lock() {
            memo.push((key, made.clone()));
            if memo.len() > 8 {
                memo.remove(0);
            }
        }
        Ok(made)
    }

    /// The area in plan, mm².
    pub fn plan_area(&self) -> f64 {
        self.tris.iter().map(|t| ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0])).abs() / 2.0).sum()
    }

    /// The area of the surface itself, mm².
    pub fn surface_area(&self) -> f64 {
        self.tris
            .iter()
            .map(|t| {
                let (u, v) = (sub(t[1], t[0]), sub(t[2], t[0]));
                let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() / 2.0
            })
            .sum()
    }

    /// The lowest and highest heights.
    pub fn z_range(&self) -> Option<(f64, f64)> {
        let mut zs = self.tris.iter().flatten().map(|p| p[2]);
        let first = zs.next()?;
        Some(zs.fold((first, first), |(lo, hi), z| (lo.min(z), hi.max(z))))
    }
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The height of triangle `t` over (x, y), if it is over it (its edges too, to a billionth).
fn over(t: &[P3; 3], x: f64, y: f64) -> Option<f64> {
    let [a, b, c] = t;
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if d.abs() < 1e-12 {
        return None;
    }
    let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
    let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
    let l3 = 1.0 - l1 - l2;
    const EDGE: f64 = -1e-9;
    (l1 >= EDGE && l2 >= EDGE && l3 >= EDGE).then(|| l1 * a[2] + l2 * b[2] + l3 * c[2])
}

/// Where the surface of `t` crosses `level`: a segment, if the triangle has vertices on both sides (at the level counts as above).
fn crossing(t: &[P3; 3], level: f64) -> Option<(P2, P2)> {
    let above = t.map(|p| p[2] >= level);
    if above[0] == above[1] && above[1] == above[2] {
        return None;
    }
    let mut at = Vec::with_capacity(2);
    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
        if above[i] != above[j] {
            // Ordered the same from either triangle that shares the edge, so both give the same point.
            let (p, q) = if (t[i][0], t[i][1]) <= (t[j][0], t[j][1]) { (t[i], t[j]) } else { (t[j], t[i]) };
            let s = (level - p[2]) / (q[2] - p[2]);
            at.push(P2(p[0] * (1.0 - s) + q[0] * s, p[1] * (1.0 - s) + q[1] * s));
        }
    }
    match at.as_slice() {
        [a, b] if key(*a) != key(*b) => Some((*a, *b)),
        _ => None,
    }
}

/// Joins the segments of one level into lines, which end where there are no more to join or where they began.
fn join(z: f64, segs: Vec<(P2, P2)>, out: &mut Vec<Contour>) {
    let mut seen = HashSet::new();
    let segs: Vec<(P2, P2)> = segs
        .into_iter()
        .filter(|(a, b)| {
            let (ka, kb) = (key(*a), key(*b));
            seen.insert(if ka <= kb { (ka, kb) } else { (kb, ka) })
        })
        .collect();
    let mut at: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        at.entry(key(*a)).or_default().push(i);
        at.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    // The unused segment at `end`, and the far end of it.
    let next = |end: P2, used: &mut Vec<bool>| -> Option<P2> {
        let i = *at.get(&key(end))?.iter().find(|i| !used[**i])?;
        used[i] = true;
        let (a, b) = segs[i];
        Some(if key(a) == key(end) { b } else { a })
    };
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut line = VecDeque::from([segs[start].0, segs[start].1]);
        while let Some(p) = line.back().copied().and_then(|e| next(e, &mut used)) {
            line.push_back(p);
        }
        while let Some(p) = line.front().copied().and_then(|e| next(e, &mut used)) {
            line.push_front(p);
        }
        let closed = line.len() > 3 && line.front().map(|p| key(*p)) == line.back().map(|p| key(*p));
        let mut points: Vec<P2> = line.into();
        if closed {
            points.pop();
        }
        out.push(Contour { z, points, closed });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square grid of points `step` apart from (0, 0), with the height `f` of each.
    fn grid(n: usize, step: f64, f: impl Fn(f64, f64) -> f64) -> Vec<P3> {
        (0..n).flat_map(|i| (0..n).map(move |j| (i as f64 * step, j as f64 * step))).map(|(x, y)| [x, y, f(x, y)]).collect()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_height_over_a_point_is_the_planes_when_the_points_lie_in_one() {
        let tin = build(&grid(6, 1000.0, |x, y| 0.1 * x + 0.05 * y), &[], &[]).unwrap();
        for (x, y) in [(0.0, 0.0), (2500.0, 1300.0), (5000.0, 5000.0), (1234.5, 4321.0), (1000.0, 1000.0)] {
            let z = tin.height_at(x, y).unwrap_or_else(|| panic!("no height at {x} {y}"));
            assert!(near(z, 0.1 * x + 0.05 * y), "{x} {y}: {z}");
        }
        assert_eq!(tin.height_at(-1.0, 0.0), None);
        assert_eq!(tin.height_at(2000.0, 5000.1), None);
        assert_eq!(tin.triangles().len(), 50, "a 6 x 6 grid is 5 x 5 squares of two triangles");
    }

    #[test]
    fn areas_are_the_plans_and_the_surfaces() {
        let flat = build(&grid(3, 5000.0, |_, _| 5.0), &[], &[]).unwrap();
        assert!(near(flat.plan_area(), 1e8) && near(flat.surface_area(), 1e8));
        assert_eq!(flat.z_range(), Some((5.0, 5.0)));
        // A 45 degree slope: the surface is the plan's area times the square root of two.
        let slope = build(&grid(3, 5000.0, |x, _| x), &[], &[]).unwrap();
        assert!(near(slope.plan_area(), 1e8) && (slope.surface_area() - 1e8 * 2f64.sqrt()).abs() < 1e-3);
        assert_eq!(slope.z_range(), Some((0.0, 10000.0)));
    }

    #[test]
    fn contours_across_a_slope_are_one_line_each_at_the_levels() {
        let tin = build(&grid(11, 1000.0, |x, _| x / 10.0), &[], &[]).unwrap();
        let lines = tin.contours(250.0, 0.0).unwrap();
        let levels: Vec<f64> = lines.iter().map(|c| c.z).collect();
        assert_eq!(
            levels,
            [250.0, 500.0, 750.0, 1000.0],
            "a line at the top edge, where the surface touches 1000; none at 0, which it never goes below"
        );
        for c in &lines[0..3] {
            assert!(!c.closed);
            assert!(c.points.iter().all(|p| near(p.0, c.z * 10.0)), "{c:?}");
            let (lo, hi) = c.points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
            assert!(near(lo, 0.0) && near(hi, 10000.0), "it runs the whole way across: {lo} {hi}");
        }
        // Counted from another base.
        let shifted = tin.contours(500.0, 100.0).unwrap();
        assert_eq!(shifted.iter().map(|c| c.z).collect::<Vec<_>>(), [100.0, 600.0]);
    }

    #[test]
    fn a_hill_has_closed_contours() {
        // A pyramid 1000 high on a 10 m square, its peak in the middle.
        let tin = build(&grid(11, 1000.0, |x, y| 1000.0 - (x - 5000.0).abs().max((y - 5000.0).abs()) * 0.2), &[], &[]).unwrap();
        let lines = tin.contours(200.0, 0.0).unwrap();
        let at = |z: f64| lines.iter().filter(|c| near(c.z, z)).collect::<Vec<_>>();
        let ring = at(600.0);
        assert_eq!(ring.len(), 1);
        assert!(ring[0].closed, "{:?}", ring[0]);
        // The 600 line is where 0.2 times the distance is 400: 2000 from the middle, a 4000 square.
        assert!(near(crate::geom::area(&ring[0].points).abs(), 16_000_000.0), "{}", crate::geom::area(&ring[0].points));
    }

    #[test]
    fn a_breakline_makes_the_triangles_follow_it() {
        let square = vec![[0.0, 0.0, 0.0], [10000.0, 0.0, 10.0], [10000.0, 10000.0, 0.0], [0.0, 10000.0, 10.0]];
        // The four corners are on one circle: either diagonal is Delaunay. A breakline on each in turn decides.
        let along_low = build(&square, &[vec![[0.0, 0.0, 0.0], [10000.0, 10000.0, 0.0]]], &[]).unwrap();
        assert!(near(along_low.height_at(5000.0, 5000.0).unwrap(), 0.0), "a valley along the diagonal from the low corner");
        let along_high = build(&square, &[vec![[10000.0, 0.0, 10.0], [0.0, 10000.0, 10.0]]], &[]).unwrap();
        assert!(near(along_high.height_at(5000.0, 5000.0).unwrap(), 10.0), "a ridge along the other");
        // Two that cross can't both be followed; the surface is still made.
        let both = build(&square, &[vec![[0.0, 0.0, 0.0], [10000.0, 10000.0, 0.0]], vec![[10000.0, 0.0, 10.0], [0.0, 10000.0, 10.0]]], &[])
            .unwrap();
        assert!(both.height_at(5000.0, 5000.0).is_some());
    }

    #[test]
    fn a_boundary_cuts_away_the_triangles_outside_it() {
        let points = grid(5, 1000.0, |_, _| 1.0);
        // An L: the square less its top right quarter.
        let l = [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 2000.0), P2(2000.0, 2000.0), P2(2000.0, 4000.0), P2(0.0, 4000.0)];
        let tin = build(&points, &[], &l).unwrap();
        assert!(tin.height_at(1000.0, 3000.0).is_some() && tin.height_at(3000.0, 1000.0).is_some());
        assert_eq!(tin.height_at(3000.0, 3000.0), None, "the notch");
        assert!(near(tin.plan_area(), 12_000_000.0), "{}", tin.plan_area());
    }

    #[test]
    fn what_cannot_make_a_surface_is_refused_with_why() {
        let why = |points: &[P3]| build(points, &[], &[]).unwrap_err();
        assert!(why(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]).contains("three"));
        assert!(why(&[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [2000.0, 0.0, 0.0]]).contains("line"));
        assert!(why(&[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, f64::NAN, 0.0]]).contains("finite"));
        assert!(why(&[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 0.0, 5.0], [0.0, 0.0, 6.0]]).contains("line"), "stacked points are one");
        let tin = build(&grid(3, 1000.0, |_, _| 0.0), &[], &[]).unwrap();
        assert!(tin.contours(0.5, 0.0).is_ok(), "no relief, one line at most");
        let hill = build(&grid(3, 1000.0, |x, _| x * 100.0), &[], &[]).unwrap();
        assert!(hill.contours(1.0, 0.0).unwrap_err().contains("interval"));
        assert!(hill.contours(0.0, 0.0).is_err() && hill.contours(f64::NAN, 0.0).is_err() && hill.contours(-5.0, 0.0).is_err());
    }

    #[test]
    fn the_same_terrain_is_built_once_and_its_contours_with_it() {
        let points = grid(7, 937.0, |x, y| x / 7.0 + y / 11.0);
        let a = cached(&points, &[], &[]).unwrap();
        let b = cached(&points, &[], &[]).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "the same points, the same triangles");
        let mut raised = points.clone();
        raised[3][2] += 1.0;
        assert!(!Arc::ptr_eq(&a, &cached(&raised, &[], &[]).unwrap()), "a point moved, a new surface");
        let x = a.contours_shared(100.0, 0.0).unwrap();
        let y = a.contours_shared(100.0, 0.0).unwrap();
        assert!(Arc::ptr_eq(&x, &y) && *x == a.contours(100.0, 0.0).unwrap());
        assert!(!Arc::ptr_eq(&x, &a.contours_shared(200.0, 0.0).unwrap()));
        assert!(cached(&points[..2], &[], &[]).is_err(), "a failure is told again, not built again");
        assert!(cached(&points[..2], &[], &[]).is_err());
    }

    /// A ring of the corners of a rectangle, counter-clockwise.
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P2> {
        vec![P2(x0, y0), P2(x1, y0), P2(x1, y1), P2(x0, y1)]
    }

    #[test]
    fn a_flat_pad_over_flat_ground_is_all_cut_or_all_fill() {
        let tin = build(&grid(3, 5000.0, |_, _| 1000.0), &[], &[]).unwrap();
        let pad = rect(3000.0, 3000.0, 7000.0, 7000.0);
        let low = tin.cut_fill(&pad, 500.0).unwrap();
        assert!(near(low.cut, 16e6 * 500.0) && low.fill.abs() < 1.0, "{low:?}");
        assert!(near(low.covered, 16e6) && near(low.pad_area, 16e6));
        let high = tin.cut_fill(&pad, 1500.0).unwrap();
        assert!(near(high.fill, 16e6 * 500.0) && high.cut.abs() < 1.0, "{high:?}");
        let level = tin.cut_fill(&pad, 1000.0).unwrap();
        assert!(level.cut.abs() < 1.0 && level.fill.abs() < 1.0, "{level:?}");
        // A clockwise ring is the same pad.
        let mut cw = pad.clone();
        cw.reverse();
        assert_eq!(tin.cut_fill(&cw, 500.0).unwrap(), low);
    }

    #[test]
    fn on_a_slope_the_cut_and_the_fill_are_what_the_arithmetic_says() {
        // Ground z = x / 10 from 0 to 1000 over 10 m; a 4 m square pad from x = 2 m to 6 m at z = 500. The ground is above the pad
        // for x past 5 m, by up to 100 mm: a cut of 50 mm on average over 1 m by 4 m, 0.2 m3. Below it from 2 m to 5 m, by up to 300:
        // 150 on average over 3 m by 4 m, 1.8 m3.
        let tin = build(&grid(11, 1000.0, |x, _| x / 10.0), &[], &[]).unwrap();
        let r = tin.cut_fill(&rect(2000.0, 2000.0, 6000.0, 6000.0), 500.0).unwrap();
        assert!((r.cut - 2e8).abs() < 1e3 && (r.fill - 1.8e9).abs() < 1e3, "{r:?}");
        assert!(near(r.covered, 16e6));
    }

    #[test]
    fn a_pad_of_any_shape_counts_its_own_area_only() {
        let tin = build(&grid(3, 5000.0, |_, _| 1000.0), &[], &[]).unwrap();
        // An L: the 4 m square less a 2 m square from its corner: 12 m2.
        let l =
            vec![P2(2000.0, 2000.0), P2(6000.0, 2000.0), P2(6000.0, 4000.0), P2(4000.0, 4000.0), P2(4000.0, 6000.0), P2(2000.0, 6000.0)];
        let r = tin.cut_fill(&l, 0.0).unwrap();
        assert!(near(r.cut, 12e6 * 1000.0) && near(r.covered, 12e6), "{r:?}");
        // Partly off the ground: only what is over it is dug.
        let off = tin.cut_fill(&rect(8000.0, 2000.0, 12000.0, 4000.0), 0.0).unwrap();
        assert!(near(off.covered, 4e6) && near(off.pad_area, 8e6) && near(off.cut, 4e6 * 1000.0), "{off:?}");
        // Wholly off it: nothing.
        let none = tin.cut_fill(&rect(20000.0, 20000.0, 22000.0, 22000.0), 0.0).unwrap();
        assert_eq!((none.cut, none.fill, none.covered), (0.0, 0.0, 0.0));
        assert!(tin.cut_fill(&[P2(0.0, 0.0), P2(1.0, 1.0)], 0.0).is_err());
        assert!(tin.cut_fill(&rect(0.0, 0.0, 1000.0, 1000.0), f64::NAN).is_err());
        assert!(tin.cut_fill(&[P2(0.0, 0.0), P2(1.0, 0.0), P2(2.0, 0.0)], 0.0).is_err(), "no area");
        assert!(tin.cut_fill(&[P2(0.0, 0.0), P2(f64::NAN, 0.0), P2(0.0, 1.0)], 0.0).is_err());
    }

    #[test]
    fn two_surfaces_are_compared_where_both_are() {
        // Ground rising 0 to 1000 mm over 10 m, against flat at 500: half is cut, half fill, 12.5 m3 each (1.25e10 mm3).
        let rising = build(&grid(11, 1000.0, |x, _| x / 10.0), &[], &[]).unwrap();
        let flat = build(&grid(3, 5000.0, |_, _| 500.0), &[], &[]).unwrap();
        let b = between(&rising, &flat, 100.0, 1_000_000).unwrap();
        assert!((b.cut - 1.25e10).abs() < 1.25e10 * 0.01 && (b.fill - 1.25e10).abs() < 1.25e10 * 0.01, "{b:?}");
        assert!(near(b.area, 1e8) && b.cells == 10_000, "{b:?}");
        // Only the common ground: the second covers the east half.
        let east =
            build(&[[5000.0, 0.0, 500.0], [10000.0, 0.0, 500.0], [10000.0, 10000.0, 500.0], [5000.0, 10000.0, 500.0]], &[], &[]).unwrap();
        let b = between(&rising, &east, 100.0, 1_000_000).unwrap();
        assert!((b.area - 5e7).abs() < 5e5 && b.fill < 1.0 && (b.cut - 1.25e10).abs() < 1.25e10 * 0.01, "{b:?}");
        // Cut by one is fill the other way round.
        let back = between(&east, &rising, 100.0, 1_000_000).unwrap();
        assert!((back.fill - b.cut).abs() < 1e7 && back.cut < 1.0, "{back:?}");
        // Too fine a cell, nonsense, and surfaces that do not touch.
        assert!(between(&rising, &flat, 1.0, 1_000_000).unwrap_err().contains("cell"));
        for bad in [0.0, -1.0, f64::NAN] {
            assert!(between(&rising, &flat, bad, 1000).is_err());
        }
        let far = build(&[[50000.0, 0.0, 0.0], [60000.0, 0.0, 0.0], [60000.0, 10000.0, 0.0]], &[], &[]).unwrap();
        assert!(between(&rising, &far, 100.0, 1_000_000).unwrap_err().contains("do not overlap"));
    }

    #[test]
    fn repeated_points_count_once() {
        let mut points = grid(4, 1000.0, |x, y| x + y);
        points.extend(grid(4, 1000.0, |x, y| x + y));
        let tin = build(&points, &[], &[]).unwrap();
        assert_eq!(tin.triangles().len(), 18);
    }
}
