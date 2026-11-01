//! Constrained Delaunay triangulation in plan: the region inside an outline and outside its holes cut into triangles,
//! with corners at its own corners and at the points given, and never crossing the lines given. A shaped floor's or flat
//! roof's top (IDEAS MOD-16, [design](../../../docs/shape-editing.md)) is these triangles lifted by its points' heights.
//!
//! The same input gives the same triangles whatever order it comes in: the corners are put in order first, and every
//! choice after that reads them in that order, so where four corners lie on one circle and either diagonal would do, the
//! order picks it. Which side of a line a corner lies on, and whether it lies inside a circle, are worked out exactly: the
//! corners go on a 0.01 mm grid and the sums are in whole numbers, so a nearly flat triangle can't come out the wrong way
//! round and send the flips round in a loop.
//!
//! The steps: corners within `TOL` become one, and an edge is cut at any corner within `TOL` of it; a sweep from west to
//! east triangulates the corners' convex hull; Lawson's flips make it Delaunay; Sloan's flips bring in each edge of the
//! outline, the holes and the lines that is missing; Lawson's flips again, never across those; and a walk in from the hull,
//! crossing an outline's or a hole's edge to change from outside to inside, keeps the triangles of the region.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::geom::{self, P2};
use crate::model::{MAX_COORD, TOL};

/// How far across a region may be, mm: past about 19 km the in-circle sum no longer fits in 128 bits.
pub const MAX_ACROSS: f64 = 10_000_000.0;
/// The most corners a triangulation takes.
pub const MAX_CORNERS: usize = 20_000;
/// The most flips it makes before it gives up, so no input holds a command for long.
const MAX_FLIPS: usize = 20_000_000;
/// The grid the corners go on for the exact sums: mm.
const GRID: f64 = 0.01;

/// Triangles in plan: their corners, and each triangle as three indices into them, counter-clockwise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Triangulation {
    pub points: Vec<P2>,
    pub triangles: Vec<[usize; 3]>,
}

/// The constrained Delaunay triangulation of the region inside `outline` and outside each of `holes` (either way round),
/// with a corner at each of `points` and every one of `lines` along triangles' edges. Corners within `TOL` of each other
/// are one, the first in order of x and then y; a point or a line's end within `TOL` of an edge cuts it there. Refused,
/// saying why: more than `MAX_CORNERS` corners, one not finite or past `MAX_COORD`, a region more than `MAX_ACROSS`
/// across, an outline or a hole with no area, a hole outside the outline or over another, a point or a line outside the
/// region or in a hole, a line shorter than `TOL`, and edges that cross.
pub fn constrained(outline: &[P2], holes: &[Vec<P2>], points: &[P2], lines: &[(P2, P2)]) -> Result<Triangulation, String> {
    let given: Vec<P2> = std::iter::once(outline)
        .chain(holes.iter().map(Vec::as_slice))
        .flatten()
        .chain(points)
        .chain(lines.iter().flat_map(|(a, b)| [a, b]))
        .copied()
        .collect();
    if given.len() > MAX_CORNERS {
        return Err(format!("it has {} corners, more than the {MAX_CORNERS} a triangulation takes", given.len()));
    }
    if let Some(p) = given.iter().find(|p| !(p.is_finite() && p.0.abs() <= MAX_COORD && p.1.abs() <= MAX_COORD)) {
        return Err(format!("{} is not a finite point within {MAX_COORD} mm of the origin", at(*p)));
    }
    let mut sorted = given;
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut corners = Corners::default();
    for p in sorted {
        corners.add(p);
    }
    let pts = corners.points.clone();
    let (lo, hi) = pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| {
        (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
    });
    let across = (hi.0 - lo.0).max(hi.1 - lo.1);
    if across > MAX_ACROSS {
        return Err(format!("it is {:.1} km across, more than the {:.0} km a triangulation takes", across / 1e6, MAX_ACROSS / 1e6));
    }
    let corner = |i: usize| pts.get(i).copied().unwrap_or_default();
    let polygon = |r: &[usize]| r.iter().map(|&i| corner(i)).collect::<Vec<P2>>();

    // The rings as corners, each once.
    let ring_of = |r: &[P2]| {
        let mut v: Vec<usize> = r.iter().filter_map(|p| corners.find(*p)).collect();
        v.dedup();
        while v.len() > 1 && v.first() == v.last() {
            v.pop();
        }
        v
    };
    let outer = ring_of(outline);
    let outer_p = polygon(&outer);
    if outer.len() < 3 || geom::area(&outer_p).abs() < TOL * TOL {
        return Err("the outline has no area".into());
    }
    let hole_rings: Vec<Vec<usize>> = holes.iter().map(|h| ring_of(h)).collect();
    let holes_p: Vec<Vec<P2>> = hole_rings.iter().map(|h| polygon(h)).collect();
    if hole_rings.iter().zip(&holes_p).any(|(h, p)| h.len() < 3 || geom::area(p).abs() < TOL * TOL) {
        return Err("a hole has no area".into());
    }

    // Where a point lies: in the region, on its edges included, or not.
    let near = |r: &[P2], p: P2| geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(p, a, b) <= TOL);
    let in_outline = |p: P2| geom::contains(&outer_p, p) || near(&outer_p, p);
    let in_hole = |p: P2| holes_p.iter().any(|h| geom::contains(h, p) && !near(h, p));
    let place = |p: P2| {
        if !in_outline(p) {
            Some("outside the outline")
        } else if in_hole(p) {
            Some("in a hole")
        } else {
            None
        }
    };
    for (k, h) in holes_p.iter().enumerate() {
        if h.iter().any(|&p| !in_outline(p)) {
            return Err("a hole reaches outside the outline".into());
        }
        if holes_p.iter().enumerate().any(|(j, other)| j != k && h.iter().any(|&p| geom::contains(other, p) && !near(other, p))) {
            return Err("two holes overlap".into());
        }
    }
    for &p in points {
        if let Some(why) = corners.find(p).map(corner).and_then(place) {
            return Err(format!("the point at {} lies {why}", at(p)));
        }
    }
    let mut ends = Vec::new();
    for &(a, b) in lines {
        let (Some(i), Some(j)) = (corners.find(a), corners.find(b)) else { continue };
        if i == j {
            return Err(format!("the line from {} to {} is shorter than a millimetre", at(a), at(b)));
        }
        if let Some(why) = place(corner(i)).or_else(|| place(corner(j))) {
            return Err(format!("the line from {} to {} ends {why}", at(a), at(b)));
        }
        ends.push((i, j));
    }

    // The edges no triangle may cross, each cut at the corners on it: (ring's, line's) by its corners, low first.
    let rings = std::iter::once(&outer).chain(&hole_rings);
    let ring_edges = rings.flat_map(|r| r.iter().copied().zip(r.iter().copied().cycle().skip(1)));
    let mut pieces: BTreeMap<(usize, usize), (bool, bool)> = BTreeMap::new();
    for e in ring_edges {
        for (a, b) in cut(e, &pts) {
            pieces.entry(key(a, b)).or_default().0 = true;
        }
    }
    for &e in &ends {
        for (a, b) in cut(e, &pts) {
            pieces.entry(key(a, b)).or_default().1 = true;
        }
    }

    let g: Vec<G> = pts.iter().map(|p| (((p.0 - lo.0) / GRID).round() as i64, ((p.1 - lo.1) / GRID).round() as i64)).collect();
    let grid = |i: usize| g.get(i).copied().unwrap_or_default();
    if let Some(((a, b, line_ab), (c, d, line_cd))) = crossing(&pieces, &grid) {
        let (pa, pb, pc, pd) = (corner(a), corner(b), corner(c), corner(d));
        let x = at(geom::line_intersection(pa, pb, pc, pd).unwrap_or(pa));
        return Err(match (line_ab, line_cd) {
            (true, true) => format!("the line from {} to {} crosses another at {x}", at(pa), at(pb)),
            (true, false) => format!("the line from {} to {} crosses an edge of the outline or of a hole at {x}", at(pa), at(pb)),
            (false, true) => format!("the line from {} to {} crosses an edge of the outline or of a hole at {x}", at(pc), at(pd)),
            (false, false) => format!("the outline or a hole crosses itself or another at {x}"),
        });
    }
    // A line that runs out through a corner crosses no edge: its middle says where it went.
    for (&(a, b), _) in pieces.iter().filter(|(_, (_, line))| *line) {
        let (pa, pb) = (corner(a), corner(b));
        if let Some(why) = place((pa + pb) * 0.5) {
            return Err(format!("the line from {} to {} runs {why}", at(pa), at(pb)));
        }
    }

    let mut mesh = Mesh::sweep(g.clone())?;
    let all = mesh.interior();
    mesh.legalize(all)?;
    for &(u, v) in pieces.keys() {
        mesh.enforce(u, v)?;
    }
    let all = mesh.interior();
    mesh.legalize(all)?;
    let rims: HashSet<(usize, usize)> = pieces.iter().filter(|(_, (ring, _))| *ring).map(|(k, _)| *k).collect();
    let keep = mesh.inside(&rims);
    let mut triangles: Vec<[usize; 3]> = mesh
        .tris
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(t, _)| {
            let k = (0..3).min_by_key(|&k| t[k]).unwrap_or(0);
            [t[k], t[(k + 1) % 3], t[(k + 2) % 3]]
        })
        .collect();
    triangles.sort_unstable();
    Ok(Triangulation { points: pts, triangles })
}

/// A point as messages give it, to the millimetre.
fn at(p: P2) -> String {
    format!("({:.0}, {:.0})", p.0, p.1)
}

/// An edge as a key: its corners, the lower first.
fn key(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// The corners so far, each more than `TOL` from the rest, found by the millimetre square they're in.
#[derive(Default)]
struct Corners {
    points: Vec<P2>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl Corners {
    fn cell(p: P2) -> (i64, i64) {
        ((p.0 / TOL).floor() as i64, (p.1 / TOL).floor() as i64)
    }

    /// The first corner within `TOL` of `p`.
    fn find(&self, p: P2) -> Option<usize> {
        let (x, y) = Corners::cell(p);
        (x - 1..=x + 1)
            .flat_map(|i| (y - 1..=y + 1).map(move |j| (i, j)))
            .filter_map(|c| self.cells.get(&c))
            .flatten()
            .copied()
            .filter(|&i| self.points.get(i).is_some_and(|q| q.dist(p) <= TOL))
            .min()
    }

    fn add(&mut self, p: P2) {
        if self.find(p).is_none() {
            self.cells.entry(Corners::cell(p)).or_default().push(self.points.len());
            self.points.push(p);
        }
    }
}

/// The edge from corner `a` to corner `b` cut at every other corner within `TOL` of it, in order along it. `pts` are in
/// order of x.
fn cut((a, b): (usize, usize), pts: &[P2]) -> Vec<(usize, usize)> {
    let (Some(&pa), Some(&pb)) = (pts.get(a), pts.get(b)) else { return Vec::new() };
    let (x0, x1) = (pa.0.min(pb.0) - TOL, pa.0.max(pb.0) + TOL);
    let from = pts.partition_point(|p| p.0 < x0);
    let mut on: Vec<(f64, usize)> = (from..pts.len())
        .zip(&pts[from..])
        .take_while(|(_, p)| p.0 <= x1)
        .filter(|&(i, p)| i != a && i != b && geom::dist_to_segment(*p, pa, pb) <= TOL)
        .map(|(i, p)| (geom::param_on_segment(*p, pa, pb), i))
        .collect();
    on.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    let chain: Vec<usize> = std::iter::once(a).chain(on.into_iter().map(|(_, i)| i)).chain(std::iter::once(b)).collect();
    chain.windows(2).map(|w| (w[0], w[1])).collect()
}

/// An edge no triangle may cross: its corners, and whether it's a line's.
type Fixed = (usize, usize, bool);

/// Two of the edges that cross; none when no two do. Edges that meet at a corner don't cross, and a corner on an edge has
/// cut it already, so only a proper crossing is left to find. Swept west to east: an edge is checked against those that
/// start before it ends.
fn crossing(pieces: &BTreeMap<(usize, usize), (bool, bool)>, grid: &impl Fn(usize) -> G) -> Option<(Fixed, Fixed)> {
    // Each edge with its box: west, east, south, north.
    let mut boxed: Vec<((i64, i64, i64, i64), Fixed)> = pieces
        .iter()
        .map(|(&(a, b), &(_, line))| {
            let (p, q) = (grid(a), grid(b));
            ((p.0.min(q.0), p.0.max(q.0), p.1.min(q.1), p.1.max(q.1)), (a, b, line))
        })
        .collect();
    boxed.sort_unstable_by_key(|&((w, _, s, _), (a, b, _))| (w, s, a, b));
    for (n, &((_, east, south, north), (a, b, line_ab))) in boxed.iter().enumerate() {
        for &((w, _, s, nn), (c, d, line_cd)) in &boxed[n + 1..] {
            if w > east {
                break;
            }
            if s <= north && nn >= south && crosses(grid(a), grid(b), grid(c), grid(d)) {
                return Some(((a, b, line_ab), (c, d, line_cd)));
            }
        }
    }
    None
}

/// A corner on the grid: whole hundredths of a millimetre east and north of the region's south-west.
type G = (i64, i64);

/// Twice the area of triangle `a`, `b`, `c`: positive counter-clockwise, zero in a line. Exact: the corners lie within
/// `MAX_ACROSS` of each other, 1e9 grid steps, so each product is under 1e18.
fn orient(a: G, b: G, c: G) -> i128 {
    let w = |p: G| (i128::from(p.0) - i128::from(a.0), i128::from(p.1) - i128::from(a.1));
    let ((bx, by), (cx, cy)) = (w(b), w(c));
    bx * cy - by * cx
}

/// Positive when `d` lies inside the circle through counter-clockwise `a`, `b`, `c`; zero on it. Exact: each of the three
/// terms is under 4e36, well within an `i128`.
fn incircle(a: G, b: G, c: G, d: G) -> i128 {
    let w = |p: G| {
        let (x, y) = (i128::from(p.0) - i128::from(d.0), i128::from(p.1) - i128::from(d.1));
        (x, y, x * x + y * y)
    };
    let ((ax, ay, al), (bx, by, bl), (cx, cy, cl)) = (w(a), w(b), w(c));
    al * (bx * cy - cx * by) + bl * (cx * ay - ax * cy) + cl * (ax * by - bx * ay)
}

/// Whether `p`–`q` and `r`–`s` cross, each strictly between its ends.
fn crosses(p: G, q: G, r: G, s: G) -> bool {
    orient(p, q, r).signum() * orient(p, q, s).signum() < 0 && orient(r, s, p).signum() * orient(r, s, q).signum() < 0
}

/// A triangle's edges, each the way round it runs.
fn sides(t: [usize; 3]) -> [(usize, usize); 3] {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
}

/// What a triangulation says when it can't go on: it never should.
fn broke() -> String {
    "the triangulation broke: please report it, with the outline, the points and the lines".into()
}

/// Triangles over corners on the grid, and for each edge, the way a triangle runs round it, that triangle.
struct Mesh {
    at: Vec<G>,
    tris: Vec<[usize; 3]>,
    edges: HashMap<(usize, usize), usize>,
    /// The edges no flip may take away: the outline's, the holes' and the lines', by `key`.
    fixed: HashSet<(usize, usize)>,
    flips: usize,
}

impl Mesh {
    fn pt(&self, i: usize) -> G {
        self.at.get(i).copied().unwrap_or_default()
    }

    fn add(&mut self, t: [usize; 3]) {
        let k = self.tris.len();
        for e in sides(t) {
            self.edges.insert(e, k);
        }
        self.tris.push(t);
    }

    /// The convex hull of the corners triangulated, by a sweep west to east: each corner in turn, past all before it,
    /// joined to every edge of the hull it sees.
    fn sweep(at: Vec<G>) -> Result<Mesh, String> {
        let mut order: Vec<usize> = (0..at.len()).collect();
        order.sort_by_key(|&i| at.get(i).copied());
        let mut m = Mesh { at, tris: Vec::new(), edges: HashMap::new(), fixed: HashSet::new(), flips: 0 };
        let (Some(&o0), Some(&o1)) = (order.first(), order.get(1)) else { return Err("the outline has no area".into()) };
        let Some(k) = (2..order.len()).find(|&k| orient(m.pt(o0), m.pt(o1), m.pt(order[k])) != 0) else {
            return Err("the outline has no area: its corners lie on one line".into());
        };
        // Corners in a line, then one off it: a fan of triangles from it to the line.
        let (run, apex) = (order[..k].to_vec(), order[k]);
        for w in run.windows(2) {
            let (a, b) = (w[0], w[1]);
            if orient(m.pt(a), m.pt(b), m.pt(apex)) > 0 { m.add([a, b, apex]) } else { m.add([b, a, apex]) }
        }
        let last = run.last().copied().unwrap_or(o0);
        let mut hull: Vec<usize> = if orient(m.pt(o0), m.pt(last), m.pt(apex)) > 0 { run } else { run.into_iter().rev().collect() };
        hull.push(apex);
        for &q in &order[k + 1..] {
            let (n, pq) = (hull.len(), m.pt(q));
            let seen: Vec<bool> = (0..n).map(|i| orient(m.pt(hull[i]), m.pt(hull[(i + 1) % n]), pq) < 0).collect();
            let Some(s) = (0..n).find(|&i| seen[i] && !seen[(i + n - 1) % n]) else { return Err(broke()) };
            let mut r = 0;
            while r < n && seen[(s + r) % n] {
                m.add([hull[(s + r + 1) % n], hull[(s + r) % n], q]);
                r += 1;
            }
            if r == n {
                return Err(broke());
            }
            // The hull from the last edge it sees round to the first, then the new corner.
            let mut next: Vec<usize> = (0..=n - r).map(|i| hull[(s + r + i) % n]).collect();
            next.push(q);
            hull = next;
        }
        Ok(m)
    }

    /// The triangle with edge `a`→`b`, and its third corner.
    fn apex(&self, a: usize, b: usize) -> Option<(usize, usize)> {
        let &t = self.edges.get(&(a, b))?;
        let c = self.tris.get(t)?.iter().copied().find(|&x| x != a && x != b)?;
        Some((t, c))
    }

    /// Every edge between two triangles, by `key`, in order.
    fn interior(&self) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = self.edges.keys().filter(|&&(a, b)| a < b && self.edges.contains_key(&(b, a))).copied().collect();
        v.sort_unstable();
        v
    }

    /// The edge between `a` and `b` turned to join the two corners across it, when the four make a convex quadrilateral.
    fn flip(&mut self, a: usize, b: usize) -> Result<bool, String> {
        let (Some((t1, c)), Some((t2, d))) = (self.apex(a, b), self.apex(b, a)) else { return Ok(false) };
        let [pa, pb, pc, pd] = [a, b, c, d].map(|v| self.pt(v));
        if !(orient(pc, pd, pa) < 0 && orient(pc, pd, pb) > 0) {
            return Ok(false);
        }
        self.flips += 1;
        if self.flips > MAX_FLIPS {
            return Err("it takes too many steps to triangulate: fewer points or lines would do it".into());
        }
        for e in [(a, b), (b, c), (c, a), (b, a), (a, d), (d, b)] {
            self.edges.remove(&e);
        }
        for (t, new) in [(t1, [a, d, c]), (t2, [d, b, c])] {
            if let Some(slot) = self.tris.get_mut(t) {
                *slot = new;
            }
            for e in sides(new) {
                self.edges.insert(e, t);
            }
        }
        Ok(true)
    }

    /// Lawson's flips: each edge in `stack` whose far corner lies inside the circle of the triangle on this side is
    /// flipped, and the four edges round it looked at again, until none is; `fixed` edges stay.
    fn legalize(&mut self, mut stack: Vec<(usize, usize)>) -> Result<(), String> {
        while let Some((a, b)) = stack.pop() {
            if self.fixed.contains(&key(a, b)) {
                continue;
            }
            let (Some((_, c)), Some((_, d))) = (self.apex(a, b), self.apex(b, a)) else { continue };
            if incircle(self.pt(a), self.pt(b), self.pt(c), self.pt(d)) > 0 && self.flip(a, b)? {
                stack.extend([(a, d), (d, b), (b, c), (c, a)]);
            }
        }
        Ok(())
    }

    /// The edge from `u` to `v` made one of the triangulation's, and fixed. Sloan's way: the edges it crosses, found by
    /// walking from `u` to `v`, are flipped one at a time; one that can't be yet (its quadrilateral isn't convex) waits
    /// its turn again, and a flipped one that still crosses goes back in line.
    fn enforce(&mut self, u: usize, v: usize) -> Result<(), String> {
        self.fixed.insert(key(u, v));
        if self.edges.contains_key(&(u, v)) || self.edges.contains_key(&(v, u)) {
            return Ok(());
        }
        let (pu, pv) = (self.pt(u), self.pt(v));
        // The first edge it crosses: across from `u` in the triangle at `u` whose corner holds the way to `v`. Its `a`
        // lies to the right of the way, its `b` to the left.
        let start = self.tris.iter().find_map(|t| {
            let k = t.iter().position(|&x| x == u)?;
            let (a, b) = (t[(k + 1) % 3], t[(k + 2) % 3]);
            (orient(pu, self.pt(a), pv) > 0 && orient(pu, pv, self.pt(b)) > 0).then_some((a, b))
        });
        let Some((mut a, mut b)) = start else { return Err(broke()) };
        let mut line = VecDeque::new();
        loop {
            line.push_back((a, b));
            let Some((_, c)) = self.apex(b, a) else { return Err(broke()) };
            if c == v {
                break;
            }
            match orient(pu, pv, self.pt(c)).signum() {
                -1 => a = c,
                1 => b = c,
                _ => return Err(broke()),
            }
            if line.len() > self.tris.len() {
                return Err(broke());
            }
        }
        let mut turns = 0usize;
        while let Some((a, b)) = line.pop_front() {
            turns += 1;
            if turns > MAX_FLIPS {
                return Err("it takes too many steps to triangulate: fewer points or lines would do it".into());
            }
            let (Some((_, c)), Some((_, d))) = (self.apex(a, b), self.apex(b, a)) else { return Err(broke()) };
            if !self.flip(a, b)? {
                line.push_back((a, b));
            } else if crosses(pu, pv, self.pt(c), self.pt(d)) {
                line.push_back((c, d));
            }
        }
        if self.edges.contains_key(&(u, v)) || self.edges.contains_key(&(v, u)) { Ok(()) } else { Err(broke()) }
    }

    /// Which triangles lie in the region: walking in from the hull, outside, each step across an outline's or a hole's
    /// edge (`rims`) changes outside to inside or back.
    fn inside(&self, rims: &HashSet<(usize, usize)>) -> Vec<bool> {
        let mut side: Vec<Option<bool>> = vec![None; self.tris.len()];
        let mut queue = VecDeque::new();
        for (t, &tri) in self.tris.iter().enumerate() {
            for (a, b) in sides(tri) {
                if !self.edges.contains_key(&(b, a)) && side[t].is_none() {
                    side[t] = Some(rims.contains(&key(a, b)));
                    queue.push_back(t);
                }
            }
        }
        while let Some(t) = queue.pop_front() {
            let (Some(Some(here)), Some(&tri)) = (side.get(t).copied(), self.tris.get(t)) else { continue };
            for (a, b) in sides(tri) {
                if let Some(&u) = self.edges.get(&(b, a))
                    && side.get(u).is_some_and(Option::is_none)
                {
                    side[u] = Some(here != rims.contains(&key(a, b)));
                    queue.push_back(u);
                }
            }
        }
        side.into_iter().map(|s| s == Some(true)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P2> {
        vec![P2(x0, y0), P2(x1, y0), P2(x1, y1), P2(x0, y1)]
    }

    fn corners(t: &Triangulation, k: &[usize; 3]) -> [P2; 3] {
        k.map(|i| t.points[i])
    }

    fn area(t: &Triangulation) -> f64 {
        t.triangles.iter().map(|k| geom::area(&corners(t, k))).sum()
    }

    fn edges(t: &Triangulation) -> Vec<(P2, P2)> {
        t.triangles.iter().flat_map(|k| geom::ring_edges(&corners(t, k)).collect::<Vec<_>>()).collect()
    }

    fn has_corner(t: &Triangulation, p: P2) -> bool {
        t.points.iter().any(|q| q.dist(p) < 1e-9)
    }

    /// Whether any triangle's edge properly crosses `a`–`b`.
    fn crossed(t: &Triangulation, a: P2, b: P2) -> bool {
        edges(t).into_iter().any(|(p, q)| geom::crossing(a, b, p, q).is_some_and(|s| s > 1e-9 && s < 1.0 - 1e-9))
    }

    /// The region's area: the outline less its holes.
    fn region(outline: &[P2], holes: &[Vec<P2>]) -> f64 {
        geom::area(outline).abs() - holes.iter().map(|h| geom::area(h).abs()).sum::<f64>()
    }

    /// Every triangle turns counter-clockwise with some area, and together they cover the region exactly once.
    fn covers(t: &Triangulation, outline: &[P2], holes: &[Vec<P2>]) {
        for k in &t.triangles {
            assert!(geom::area(&corners(t, k)) > 0.0, "{k:?} is clockwise or flat: {:?}", corners(t, k));
        }
        let (got, want) = (area(t), region(outline, holes));
        assert!((got - want).abs() <= want * 1e-9, "the triangles cover {got} mm², the region is {want} mm²");
    }

    /// Whether `d` lies inside the circle through counter-clockwise `a`, `b`, `c`, by more than rounding.
    fn in_circle(a: P2, b: P2, c: P2, d: P2) -> bool {
        let w = |p: P2| (p.0 - d.0, p.1 - d.1, (p.0 - d.0).powi(2) + (p.1 - d.1).powi(2));
        let ((ax, ay, al), (bx, by, bl), (cx, cy, cl)) = (w(a), w(b), w(c));
        let det = al * (bx * cy - cx * by) + bl * (cx * ay - ax * cy) + cl * (ax * by - bx * ay);
        let scale = [al, bl, cl].into_iter().fold(0.0, f64::max);
        det > scale * scale * 1e-9
    }

    /// Every edge two triangles share is Delaunay, unless it lies along one of `lines` or the region's edges.
    fn delaunay(t: &Triangulation, fixed: &[(P2, P2)]) {
        let along =
            |p: P2, q: P2| fixed.iter().any(|&(a, b)| geom::dist_to_segment(p, a, b) < 1e-6 && geom::dist_to_segment(q, a, b) < 1e-6);
        for k in &t.triangles {
            for i in 0..3 {
                let (a, b, c) = (k[i], k[(i + 1) % 3], k[(i + 2) % 3]);
                let Some(other) = t.triangles.iter().find(|o| (0..3).any(|j| o[j] == b && o[(j + 1) % 3] == a)) else { continue };
                let Some(&d) = other.iter().find(|&&v| v != a && v != b) else { continue };
                let [pa, pb, pc, pd] = [a, b, c, d].map(|v| t.points[v]);
                assert!(along(pa, pb) || !in_circle(pa, pb, pc, pd), "{pa:?}–{pb:?} isn't Delaunay: {pd:?} is inside the circle of {pc:?}");
            }
        }
    }

    /// A small random number generator, so the sweeps repeat.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn real(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (hi - lo) * (self.next() % 1_000_000) as f64 / 1_000_000.0
        }
        fn shuffle<T>(&mut self, v: &mut [T]) {
            for i in (1..v.len()).rev() {
                v.swap(i, (self.next() % (i as u64 + 1)) as usize);
            }
        }
    }

    /// An L 20 m by 16 m with a 4 m square hole, and points scattered inside it.
    fn ell(rng: &mut Rng, n: usize) -> (Vec<P2>, Vec<Vec<P2>>, Vec<P2>) {
        let outline = vec![P2(0.0, 0.0), P2(20000.0, 0.0), P2(20000.0, 8000.0), P2(9000.0, 8000.0), P2(9000.0, 16000.0), P2(0.0, 16000.0)];
        let holes = vec![rect(2000.0, 2000.0, 6000.0, 6000.0)];
        let mut points = Vec::new();
        while points.len() < n {
            let p = P2(rng.real(100.0, 19900.0), rng.real(100.0, 15900.0));
            let clear = geom::contains(&outline, p)
                && !holes.iter().any(|h| geom::contains(&geom::grow(h, 100.0), p))
                && geom::ring_edges(&outline).all(|(a, b)| geom::dist_to_segment(p, a, b) > 100.0);
            if clear {
                points.push(p);
            }
        }
        (outline, holes, points)
    }

    #[test]
    fn a_square_with_a_centre_point_makes_four_triangles() {
        let outline = rect(0.0, 0.0, 10000.0, 10000.0);
        let t = constrained(&outline, &[], &[P2(5000.0, 5000.0)], &[]).unwrap();
        assert_eq!((t.points.len(), t.triangles.len()), (5, 4));
        assert!(t.triangles.iter().all(|k| corners(&t, k).contains(&P2(5000.0, 5000.0))), "a drain in the middle: {t:?}");
        covers(&t, &outline, &[]);
        // Without it, the two triangles of either diagonal.
        let bare = constrained(&outline, &[], &[], &[]).unwrap();
        assert_eq!((bare.points.len(), bare.triangles.len()), (4, 2));
        covers(&bare, &outline, &[]);
    }

    #[test]
    fn a_line_is_always_an_edge() {
        let outline = rect(0.0, 0.0, 10000.0, 10000.0);
        for (a, b) in [(P2(0.0, 0.0), P2(10000.0, 10000.0)), (P2(0.0, 10000.0), P2(10000.0, 0.0))] {
            let t = constrained(&outline, &[], &[], &[(a, b)]).unwrap();
            assert_eq!(t.triangles.len(), 2);
            assert!(!crossed(&t, a, b), "{a:?}–{b:?} is crossed: {t:?}");
            covers(&t, &outline, &[]);
        }
        // A valley from a corner to a drain off the middle, and a ridge across: no edge crosses either.
        let (drain, ridge) = (P2(6000.0, 4000.0), (P2(0.0, 7000.0), P2(10000.0, 7000.0)));
        let lines = [(P2(0.0, 0.0), drain), ridge];
        let t = constrained(&outline, &[], &[drain], &lines).unwrap();
        for (a, b) in lines {
            assert!(!crossed(&t, a, b), "{a:?}–{b:?} is crossed");
        }
        // The ridge's ends cut the sides they land on.
        assert!(has_corner(&t, ridge.0) && has_corner(&t, ridge.1));
        covers(&t, &outline, &[]);
        delaunay(&t, &lines);
    }

    #[test]
    fn a_hole_stays_empty() {
        let outline = rect(0.0, 0.0, 10000.0, 10000.0);
        let holes = vec![rect(4000.0, 4000.0, 6000.0, 6000.0)];
        let t = constrained(&outline, &holes, &[], &[]).unwrap();
        for k in &t.triangles {
            let c = corners(&t, k);
            let middle = P2((c[0].0 + c[1].0 + c[2].0) / 3.0, (c[0].1 + c[1].1 + c[2].1) / 3.0);
            assert!(!geom::contains(&holes[0], middle), "a triangle in the hole: {c:?}");
        }
        covers(&t, &outline, &holes);
        // Either way round, outline and hole alike.
        let mut back = outline.clone();
        back.reverse();
        let mut hole_back = holes[0].clone();
        hole_back.reverse();
        assert_eq!(constrained(&back, &[hole_back], &[], &[]).unwrap(), t);
    }

    #[test]
    fn a_point_on_an_edge_or_near_a_corner_joins_it() {
        let outline = rect(0.0, 0.0, 10000.0, 10000.0);
        // On the south side, half a millimetre in: it becomes a corner of the side, which bends through it.
        let on = P2(5000.0, 0.5);
        let t = constrained(&outline, &[], &[on], &[]).unwrap();
        assert!(has_corner(&t, on));
        assert_eq!((t.points.len(), t.triangles.len()), (5, 3));
        assert!((area(&t) - (1e8 - 0.5 * 10000.0 * 0.5)).abs() < 1e-3);
        // Within a millimetre of a corner: that corner.
        let t = constrained(&outline, &[], &[P2(10000.4, 9999.7)], &[]).unwrap();
        assert_eq!((t.points.len(), t.triangles.len()), (4, 2));
        // A line from a corner to a point on the far side ends there, at a new corner of that side.
        let t = constrained(&outline, &[], &[], &[(P2(0.0, 0.0), P2(10000.0, 4000.0))]).unwrap();
        assert!(has_corner(&t, P2(10000.0, 4000.0)));
        covers(&t, &outline, &[]);
    }

    #[test]
    fn any_order_gives_the_same_triangles() {
        let mut rng = Rng(0x5eed_1234_abcd_0001);
        let (outline, holes, points) = ell(&mut rng, 60);
        let lines = vec![
            (P2(0.0, 16000.0), P2(9000.0, 8000.0)),
            (P2(20000.0, 0.0), P2(14000.0, 4000.0)),
            (P2(14000.0, 4000.0), P2(14000.0, 8000.0)),
        ];
        let t = constrained(&outline, &holes, &points, &lines).unwrap();
        covers(&t, &outline, &holes);
        delaunay(&t, &[lines.clone(), geom::ring_edges(&outline).collect(), geom::ring_edges(&holes[0]).collect()].concat());
        for (a, b) in &lines {
            assert!(!crossed(&t, *a, *b));
        }
        for _ in 0..5 {
            let (mut o, mut h, mut p, mut l) = (outline.clone(), holes[0].clone(), points.clone(), lines.clone());
            o.rotate_left((rng.next() % 6) as usize);
            if rng.next().is_multiple_of(2) {
                o.reverse();
                h.reverse();
            }
            rng.shuffle(&mut p);
            rng.shuffle(&mut l);
            for x in l.iter_mut().filter(|_| rng.next().is_multiple_of(2)) {
                *x = (x.1, x.0);
            }
            assert_eq!(constrained(&o, &[h], &p, &l).unwrap(), t);
        }
    }

    #[test]
    fn points_on_one_circle_still_give_one_answer() {
        // A 1 m grid over a 4 m square, its sides' points too: every four neighbours lie on one circle.
        let outline = rect(0.0, 0.0, 4000.0, 4000.0);
        let mut grid: Vec<P2> = (0..5).flat_map(|i| (0..5).map(move |j| P2(1000.0 * f64::from(i), 1000.0 * f64::from(j)))).collect();
        let t = constrained(&outline, &[], &grid, &[]).unwrap();
        assert_eq!((t.points.len(), t.triangles.len()), (25, 32));
        covers(&t, &outline, &[]);
        let mut rng = Rng(77);
        for _ in 0..5 {
            rng.shuffle(&mut grid);
            assert_eq!(constrained(&outline, &[], &grid, &[]).unwrap(), t);
        }
    }

    #[test]
    fn what_it_refuses_it_says() {
        let outline = rect(0.0, 0.0, 10000.0, 10000.0);
        let holes = vec![rect(4000.0, 4000.0, 6000.0, 6000.0)];
        let refused = |o: &[P2], h: &[Vec<P2>], p: &[P2], l: &[(P2, P2)], says: &str| {
            let why = constrained(o, h, p, l).unwrap_err();
            assert!(why.contains(says), "`{why}` doesn't say `{says}`");
        };
        refused(&outline, &[], &[P2(12000.0, 5000.0)], &[], "outside the outline");
        refused(&outline, &holes, &[P2(5000.0, 5000.0)], &[], "in a hole");
        refused(&outline, &[], &[], &[(P2(1000.0, 1000.0), P2(9000.0, 9000.0)), (P2(1000.0, 9000.0), P2(9000.0, 1000.0))], "crosses");
        refused(&outline, &[], &[], &[(P2(5000.0, 5000.0), P2(15000.0, 5000.0))], "outside the outline");
        refused(&outline, &holes, &[], &[(P2(1000.0, 5000.0), P2(9000.0, 5000.0))], "crosses");
        refused(&outline, &[], &[], &[(P2(1000.0, 1000.0), P2(1000.5, 1000.0))], "shorter than a millimetre");
        refused(&outline, &[], &[P2(f64::NAN, 0.0)], &[], "not a finite point");
        refused(&[P2(0.0, 0.0), P2(1e10, 0.0), P2(0.0, 1.0)], &[], &[], &[], "not a finite point");
        refused(&[P2(0.0, 0.0), P2(5000.0, 0.0), P2(10000.0, 0.0)], &[], &[], &[], "no area");
        refused(&rect(0.0, 0.0, 20_000_000.0, 1000.0), &[], &[], &[], "across");
        let many: Vec<P2> = (0..MAX_CORNERS).map(|i| P2(1.0 + (i % 100) as f64 * 90.0, 1.0 + (i / 100) as f64 * 40.0)).collect();
        refused(&outline, &[], &many, &[], "corners");
    }

    #[test]
    fn hostile_input_never_panics() {
        let mut rng = Rng(0xdead_beef_0000_0042);
        let mut made = 0;
        for case in 0..400 {
            // A rectangle, a star round the origin (simple either way), or now and then any corners at all.
            let outline: Vec<P2> = if case % 9 == 0 {
                (0..3 + rng.next() % 9).map(|_| P2(rng.real(-5000.0, 5000.0), rng.real(-5000.0, 5000.0))).collect()
            } else if case % 2 == 0 {
                rect(-4000.0, -3000.0, 4000.0, 3000.0)
            } else {
                let n = 3 + rng.next() % 12;
                (0..n)
                    .map(|k| {
                        let turn = std::f64::consts::TAU * (k as f64 + rng.real(0.0, 0.8)) / n as f64;
                        let r = rng.real(1500.0, 5000.0);
                        P2(r * turn.cos(), r * turn.sin())
                    })
                    .collect()
            };
            let holes = if case % 4 == 0 && case % 9 != 0 { vec![rect(-500.0, -500.0, 500.0, 500.0)] } else { Vec::new() };
            let mut points = Vec::new();
            let want = (rng.next() % 40) as usize;
            for _ in 0..200 {
                let p = P2(rng.real(-5000.0, 5000.0), rng.real(-5000.0, 5000.0));
                if points.len() < want && geom::contains(&outline, p) && !holes.iter().any(|h| geom::contains(&geom::grow(h, 2.0), p)) {
                    points.push(p);
                }
            }
            // Repeats of the corners, clusters closer than a millimetre, a run along a line, points on the edges and
            // just off them, and a number that isn't.
            if case % 5 == 0 {
                points.extend(outline.iter().copied());
            }
            if case % 3 == 0 {
                let near: Vec<P2> = points.iter().take(5).flat_map(|&p| [p + P2(0.3, -0.2), p + P2(0.9, 0.0), p + P2(1.6, 0.1)]).collect();
                points.extend(near);
            }
            if case % 7 == 0 {
                points.extend((0..8).map(|i| P2(-2000.0 + 400.0 * f64::from(i), 1500.0)));
            }
            if case % 6 == 0 {
                let rim: Vec<P2> =
                    geom::ring_edges(&outline).map(|(a, b)| (a + b) * 0.5 + (b - a).unit().perp() * rng.real(-0.4, 0.4)).collect();
                points.extend(rim);
            }
            if case % 11 == 0 {
                points.push(P2(f64::INFINITY, 0.0));
            }
            let ends: Vec<P2> = outline.iter().chain(&points).copied().collect();
            let lines: Vec<(P2, P2)> = (0..rng.next() % 4)
                .filter_map(|_| {
                    let pick = |k: u64| ends.get((k % ends.len().max(1) as u64) as usize).copied();
                    Some((pick(rng.next())?, pick(rng.next())?))
                })
                .collect();
            if let Ok(t) = constrained(&outline, &holes, &points, &lines) {
                made += 1;
                assert!(t.triangles.iter().all(|k| k.iter().all(|&i| i < t.points.len())), "case {case}");
                assert!(t.triangles.iter().all(|k| geom::area(&corners(&t, k)) > 0.0), "case {case}: a clockwise or flat triangle");
                // Corners merged or edges bent within a millimetre move the region's edge by no more than that.
                let slack = (geom::perimeter(&outline) + holes.iter().map(|h| geom::perimeter(h)).sum::<f64>()) * TOL_TEST + 1.0;
                let (got, region) = (area(&t), region(&outline, &holes));
                assert!((got - region).abs() <= slack, "case {case}: the triangles cover {got} mm², the region is {region} mm²");
                for &(a, b) in &lines {
                    let run = path(&t, a, b);
                    assert!(run.windows(2).all(|w| !crossed(&t, w[0], w[1])), "case {case}: {a:?}–{b:?} is crossed, along {run:?}");
                }
            }
        }
        assert!(made >= 100, "only {made} of 400 came out: the sweep tests little but refusals");
    }

    /// How far a corner may move as corners within `TOL` become one: mm.
    const TOL_TEST: f64 = crate::model::TOL;

    /// The corners a line runs through once triangulated, in order: the corner each end became, and every corner within
    /// `TOL` of the line between them, which bends it.
    fn path(t: &Triangulation, a: P2, b: P2) -> Vec<P2> {
        let corner = |p: P2| t.points.iter().copied().find(|q| q.dist(p) <= TOL_TEST).unwrap_or(p);
        let (a, b) = (corner(a), corner(b));
        let mut on: Vec<(f64, P2)> = t
            .points
            .iter()
            .copied()
            .filter(|&p| p != a && p != b && geom::dist_to_segment(p, a, b) <= TOL_TEST)
            .map(|p| (geom::param_on_segment(p, a, b), p))
            .collect();
        on.sort_by(|x, y| x.0.total_cmp(&y.0));
        std::iter::once(a).chain(on.into_iter().map(|(_, p)| p)).chain(std::iter::once(b)).collect()
    }

    #[test]
    fn two_thousand_points_and_lines_triangulate() {
        let mut rng = Rng(0x0dd5_eed5_0000_2000);
        let (outline, holes, points) = ell(&mut rng, 2000);
        let lines = [(P2(0.0, 16000.0), P2(9000.0, 8000.0)), (P2(20000.0, 0.0), P2(9000.0, 8000.0))];
        let t = constrained(&outline, &holes, &points, &lines).unwrap();
        covers(&t, &outline, &holes);
        delaunay(&t, &[lines.to_vec(), geom::ring_edges(&outline).collect(), geom::ring_edges(&holes[0]).collect()].concat());
        for (a, b) in lines {
            assert!(!crossed(&t, a, b));
        }
    }
}
