//! Terrain: a triangulated irregular network (TIN) from surveyed points, breaklines for ridges, valleys and kerbs, and a
//! boundary, with the height at a point, contours and areas. Revit's toposurface. Plain geometry: the element that holds the
//! points is `Element::Toposurface`, and the plans, 3D view, schedules and IFC read the TIN from here.
//!
//! The triangulation is `spade`'s constrained Delaunay: breaklines are constraint edges, so no triangle crosses them. Everything
//! is in mm, x east, y north, z up.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

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
        let mut tin = Tin { tris, lo, hi, size, n, cells: vec![Vec::new(); n * n], big: Vec::new() };
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
    fn repeated_points_count_once() {
        let mut points = grid(4, 1000.0, |x, y| x + y);
        points.extend(grid(4, 1000.0, |x, y| x + y));
        let tin = build(&points, &[], &[]).unwrap();
        assert_eq!(tin.triangles().len(), 18);
    }
}
