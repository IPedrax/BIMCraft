//! Plan geometry in millimetres: points, rings, and the planar faces that walls enclose.

use std::ops::{Add, Mul, Sub};

use serde::{Deserialize, Serialize};

/// A plan point or vector in millimetres (x east, y north), written as `[x, y]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct P2(pub f64, pub f64);

impl Add for P2 {
    type Output = P2;
    fn add(self, o: P2) -> P2 {
        P2(self.0 + o.0, self.1 + o.1)
    }
}
impl Sub for P2 {
    type Output = P2;
    fn sub(self, o: P2) -> P2 {
        P2(self.0 - o.0, self.1 - o.1)
    }
}
impl Mul<f64> for P2 {
    type Output = P2;
    fn mul(self, k: f64) -> P2 {
        P2(self.0 * k, self.1 * k)
    }
}

impl P2 {
    pub fn dot(self, o: P2) -> f64 {
        self.0 * o.0 + self.1 * o.1
    }
    pub fn cross(self, o: P2) -> f64 {
        self.0 * o.1 - self.1 * o.0
    }
    pub fn length(self) -> f64 {
        self.0.hypot(self.1)
    }
    pub fn dist(self, o: P2) -> f64 {
        (self - o).length()
    }
    /// The unit vector along `self` (zero stays zero).
    pub fn unit(self) -> P2 {
        let l = self.length();
        if l > 0.0 { self * (1.0 / l) } else { P2(0.0, 0.0) }
    }
    /// Turned 90° counter-clockwise.
    pub fn perp(self) -> P2 {
        P2(-self.1, self.0)
    }
    pub fn is_finite(self) -> bool {
        self.0.is_finite() && self.1.is_finite()
    }
}

/// The edges of a closed ring: (p0, p1), (p1, p2) … (pn, p0).
/// Where a rectangle's edges have their grips, at their middles: west, east, south and north (a
/// crop region's in a section: its start, its end, its bottom and its top).
pub fn edge_grips([lo, hi]: [P2; 2]) -> [P2; 4] {
    let mid = (lo + hi) * 0.5;
    [P2(lo.0, mid.1), P2(hi.0, mid.1), P2(mid.0, lo.1), P2(mid.0, hi.1)]
}

/// The rectangle with edge `k` (as [`edge_grips`] orders them) moved to `p`, at least 100 mm across.
pub fn move_edge([lo, hi]: [P2; 2], k: usize, p: P2) -> [P2; 2] {
    match k {
        0 => [P2(p.0.min(hi.0 - 100.0), lo.1), hi],
        1 => [lo, P2(p.0.max(lo.0 + 100.0), hi.1)],
        2 => [P2(lo.0, p.1.min(hi.1 - 100.0)), hi],
        _ => [lo, P2(hi.0, p.1.max(lo.1 + 100.0))],
    }
}

pub fn ring_edges(ring: &[P2]) -> impl Iterator<Item = (P2, P2)> + '_ {
    ring.iter().copied().zip(ring.iter().copied().cycle().skip(1))
}

/// Signed area of a ring (counter-clockwise positive).
pub fn area(ring: &[P2]) -> f64 {
    ring_edges(ring).map(|(a, b)| a.cross(b)).sum::<f64>() / 2.0
}

pub fn perimeter(ring: &[P2]) -> f64 {
    ring_edges(ring).map(|(a, b)| a.dist(b)).sum()
}

/// Even-odd point in ring.
pub fn contains(ring: &[P2], p: P2) -> bool {
    // `(a.1 > p.1) != (b.1 > p.1)` rules out a horizontal edge, so the division is safe.
    ring_edges(ring).filter(|(a, b)| (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1)).count() % 2 == 1
}

/// Whether the line through `a` and `b` runs the way the one through `c` and `d` does, within a
/// thousandth of a radian.
pub fn parallel(a: P2, b: P2, c: P2, d: P2) -> bool {
    (b - a).unit().cross((d - c).unit()).abs() <= 1e-3
}

/// A ring's corners: its points less those on a straight run between their neighbours.
pub fn corners(ring: &[P2]) -> Vec<P2> {
    let n = ring.len();
    let pt = |i: usize| ring.get(i % n.max(1)).copied().unwrap_or_default();
    (0..n)
        .map(|i| (pt(i + n - 1), pt(i), pt(i + 1)))
        .filter(|(a, b, c)| (*b - *a).unit().cross((*c - *b).unit()).abs() > 1e-6)
        .map(|(_, b, _)| b)
        .collect()
}

/// A counter-clockwise ring grown outward by `d`: each edge moved out, the corners where they meet.
pub fn grow(ring: &[P2], d: f64) -> Vec<P2> {
    let n = ring.len();
    let pt = |i: usize| ring.get(i % n.max(1)).copied().unwrap_or_default();
    // An edge moved out: to its right, going counter-clockwise.
    let out = |a: P2, b: P2| {
        let shift = (b - a).unit().perp() * -d;
        (a + shift, b + shift)
    };
    (0..n)
        .map(|i| {
            let ((a, b), (c, e)) = (out(pt(i + n - 1), pt(i)), out(pt(i), pt(i + 1)));
            line_intersection(a, b, c, e).unwrap_or(b)
        })
        .collect()
}

/// The convex hull round `points`, counter-clockwise (Andrew's monotone chain).
pub fn hull(mut points: Vec<P2>) -> Vec<P2> {
    points.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup_by(|a, b| a.dist(*b) < 1e-9);
    if points.len() < 3 {
        return points;
    }
    let mut out: Vec<P2> = Vec::with_capacity(points.len() + 1);
    for pass in [false, true] {
        let start = out.len();
        let chain: Box<dyn Iterator<Item = &P2>> = if pass { Box::new(points.iter().rev()) } else { Box::new(points.iter()) };
        for &p in chain {
            while out.len() >= start + 2 && out.get(out.len() - 2).zip(out.last()).is_some_and(|(a, b)| (*b - *a).cross(p - *a) <= 0.0) {
                out.pop();
            }
            out.push(p);
        }
        // Each chain's last point starts the other.
        out.pop();
    }
    out
}

/// A simple ring cut into triangles, as indices into it, each counter-clockwise: ear clipping.
/// A ring that crosses itself gets the ears there are; a point repeated in a row is one corner.
pub fn triangulate(ring: &[P2]) -> Vec<[usize; 3]> {
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let mut left: Vec<usize> = (0..ring.len()).collect();
    left.dedup_by(|b, a| pt(*a).dist(pt(*b)) < 1e-6);
    if left.len() > 1 && left.first().zip(left.last()).is_some_and(|(a, b)| pt(*a).dist(pt(*b)) < 1e-6) {
        left.pop();
    }
    if area(ring) < 0.0 {
        left.reverse();
    }
    // Whether `p` is in the counter-clockwise triangle `a`, `b`, `c`, or on its edge.
    let inside = |p: P2, a: P2, b: P2, c: P2| (b - a).cross(p - a) >= 0.0 && (c - b).cross(p - b) >= 0.0 && (a - c).cross(p - c) >= 0.0;
    let mut out = Vec::new();
    // ponytail: O(n³) ear clipping, fine for floor outlines; a sweep if outlines grow to thousands of points.
    while left.len() >= 3 {
        let n = left.len();
        let corner = |k: usize| (left[(k + n - 1) % n], left[k], left[(k + 1) % n]);
        let ear = (0..n).find(|&k| {
            let (a, b, c) = corner(k);
            (pt(b) - pt(a)).cross(pt(c) - pt(b)) > 0.0
                && left.iter().all(|&j| j == a || j == b || j == c || !inside(pt(j), pt(a), pt(b), pt(c)))
        });
        let Some(k) = ear else { break };
        let (a, b, c) = corner(k);
        out.push([a, b, c]);
        left.remove(k);
    }
    out
}

/// The part of a ring where `f` is zero or more, side by side (Sutherland-Hodgman): exact for an
/// `f` that's linear, which cuts it along a line.
pub fn clip(ring: &[P2], f: impl Fn(P2) -> f64) -> Vec<P2> {
    let mut kept = Vec::with_capacity(ring.len() + 2);
    for (a, b) in ring_edges(ring) {
        let (fa, fb) = (f(a), f(b));
        if fa >= 0.0 {
            kept.push(a);
        }
        if (fa >= 0.0) != (fb >= 0.0) {
            kept.push(a + (b - a) * (fa / (fa - fb)));
        }
    }
    kept
}

/// How far along `a`→`b` the point nearest `p` lies, from 0 at `a` to 1 at `b`.
pub fn param_on_segment(p: P2, a: P2, b: P2) -> f64 {
    let d = b - a;
    let len2 = d.dot(d);
    if len2 > 0.0 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 }
}

pub fn dist_to_segment(p: P2, a: P2, b: P2) -> f64 {
    p.dist(a + (b - a) * param_on_segment(p, a, b))
}

/// Where the line through `a` and `b` meets the line through `c` and `d`; `None` when they're
/// parallel.
pub fn line_intersection(a: P2, b: P2, c: P2, d: P2) -> Option<P2> {
    let (r, s) = (b - a, d - c);
    let den = r.cross(s);
    if den.abs() <= 1e-12 * r.length() * s.length() {
        return None;
    }
    Some(a + r * ((c - a).cross(s) / den))
}

/// Where `a`→`b` properly crosses `c`→`d`, as a parameter along `a`→`b` strictly inside (0, 1).
pub(crate) fn crossing(a: P2, b: P2, c: P2, d: P2) -> Option<f64> {
    let (r, s) = (b - a, d - c);
    let den = r.cross(s);
    if den.abs() <= 1e-12 * r.length() * s.length() {
        return None; // parallel
    }
    let t = (c - a).cross(s) / den;
    let u = (c - a).cross(r) / den;
    (t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0).then_some(t)
}

/// The segments cut wherever another one crosses them or ends on them.
fn split(segs: &[(P2, P2)], tol: f64) -> Vec<(P2, P2)> {
    let mut out = Vec::new();
    for (i, &(a, b)) in segs.iter().enumerate() {
        let mut ts = vec![0.0, 1.0];
        for (j, &(c, d)) in segs.iter().enumerate() {
            if i == j {
                continue;
            }
            // An end of the other segment on this one: a T-junction or a collinear overlap.
            for q in [c, d] {
                if dist_to_segment(q, a, b) <= tol {
                    ts.push(param_on_segment(q, a, b));
                }
            }
            ts.extend(crossing(a, b, c, d));
        }
        ts.sort_by(f64::total_cmp);
        let pts: Vec<P2> = ts.iter().map(|&t| a + (b - a) * t).collect();
        out.extend(pts.windows(2).filter_map(|w| match w {
            [p, q] if p.dist(*q) > tol => Some((*p, *q)),
            _ => None,
        }));
    }
    out
}

/// Every face of the planar graph the segments make, with ends closer than `tol` joined: each
/// bounded face counter-clockwise (positive area), and the outline of each connected piece
/// clockwise (negative area).
pub fn faces(segs: &[(P2, P2)], tol: f64) -> Vec<Vec<P2>> {
    // ponytail: O(n²) splitting and node matching; a spatial grid once plans reach thousands of walls.
    let mut nodes: Vec<P2> = Vec::new();
    let mut node = |p: P2| match nodes.iter().position(|q| q.dist(p) <= tol) {
        Some(i) => i,
        None => {
            nodes.push(p);
            nodes.len() - 1
        }
    };
    let mut edges = std::collections::BTreeSet::new();
    for (a, b) in split(segs, tol) {
        let (i, j) = (node(a), node(b));
        if i != j {
            edges.insert((i.min(j), i.max(j)));
        }
    }
    let mut around: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for &(i, j) in &edges {
        around[i].push(j);
        around[j].push(i);
    }
    let angle = |from: usize, to: usize| {
        let d = nodes[to] - nodes[from];
        d.1.atan2(d.0)
    };
    for (i, list) in around.iter_mut().enumerate() {
        list.sort_by(|&a, &b| angle(i, a).total_cmp(&angle(i, b)));
    }
    // Walk each half-edge once, keeping the face on the left: at every node take the edge just
    // clockwise of the one we arrived along.
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for &(i, j) in &edges {
        for start in [(i, j), (j, i)] {
            let (mut u, mut v) = start;
            let mut face = Vec::new();
            while seen.insert((u, v)) {
                face.push(nodes[u]);
                let list = &around[v];
                let k = list.iter().position(|&w| w == u).unwrap_or(0);
                let w = list[(k + list.len() - 1) % list.len()];
                (u, v) = (v, w);
            }
            if !face.is_empty() {
                out.push(face);
            }
        }
    }
    out
}

/// A room-shaped region: an outer ring (counter-clockwise) and the islands standing inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
}

impl Region {
    pub fn area(&self) -> f64 {
        area(&self.outer) - self.holes.iter().map(|h| area(h).abs()).sum::<f64>()
    }
    pub fn perimeter(&self) -> f64 {
        perimeter(&self.outer) + self.holes.iter().map(|h| perimeter(h)).sum::<f64>()
    }
}

/// The region around `p`: the smallest loop of segments enclosing it, minus any separate pieces
/// standing inside it (a shaft in an open office). `None` when nothing encloses `p`.
pub fn region_at(segs: &[(P2, P2)], p: P2, tol: f64) -> Option<Region> {
    region_in(&faces(segs, tol), p, tol)
}

/// What [`region_at`] answers, among faces already made by [`faces`]: the arrangement of a plan's walls is the costly part,
/// and a caller that asks about many points of one plan (the Revit import tries a grid of them per room) makes it once.
pub fn region_in(all: &[Vec<P2>], p: P2, tol: f64) -> Option<Region> {
    let outer = all.iter().filter(|f| area(f) > tol * tol && contains(f, p)).min_by(|a, b| area(a).total_cmp(&area(b)))?;
    // Outlines of other pieces lying inside it; ours is the one around `p`.
    let islands: Vec<&Vec<P2>> = all.iter().filter(|f| area(f) < 0.0 && !contains(f, p) && f.iter().all(|q| contains(outer, *q))).collect();
    let holes = islands
        .iter()
        .filter(|h| !islands.iter().any(|o| !std::ptr::eq(*o, **h) && h.iter().all(|q| contains(o, *q))))
        .map(|h| h.iter().rev().copied().collect())
        .collect();
    Some(Region { outer: outer.clone(), holes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_grow_outward_at_every_edge() {
        let l = [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 2000.0), P2(2000.0, 2000.0), P2(2000.0, 5000.0), P2(0.0, 5000.0)];
        let g = grow(&l, 100.0);
        assert_eq!(g[0], P2(-100.0, -100.0));
        assert_eq!(g[3], P2(2100.0, 2100.0), "an inner corner moves out too");
        // d times the perimeter, and d² for each outer corner less each inner one: five less one.
        assert!((area(&g) - area(&l) - 100.0 * perimeter(&l) - 4.0 * 100.0 * 100.0).abs() < 1e-6);
    }

    #[test]
    fn rings_clip_to_where_a_linear_function_is_not_negative() {
        let sq = [P2(0.0, 0.0), P2(4.0, 0.0), P2(4.0, 4.0), P2(0.0, 4.0)];
        let left = clip(&sq, |p| 1.0 - p.0);
        assert_eq!(area(&left), 4.0);
        assert!(clip(&sq, |p| -1.0 - p.0).is_empty());
        assert_eq!(clip(&sq, |p| 9.0 - p.0), sq);
    }

    #[test]
    fn concave_outlines_triangulate_into_their_area() {
        // An L, counter-clockwise; its inner corner can't be an ear.
        let l = [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 2000.0), P2(2000.0, 2000.0), P2(2000.0, 5000.0), P2(0.0, 5000.0)];
        let tris = triangulate(&l);
        assert_eq!(tris.len(), 4);
        let areas: Vec<f64> = tris.iter().map(|t| area(&t.map(|i| l[i]))).collect();
        assert!(areas.iter().all(|a| *a > 0.0), "{areas:?}");
        assert!((areas.iter().sum::<f64>() - area(&l)).abs() < 1e-6);
        assert!(triangulate(&l[..2]).is_empty(), "two points enclose nothing");
        // A corner given twice, as a clipped outline may, is one corner.
        let twice = [l[0], l[1], l[2], l[2], l[3], l[4], l[5], l[0]];
        let sum: f64 = triangulate(&twice).iter().map(|t| area(&t.map(|i| twice[i]))).sum();
        assert!((sum - area(&l)).abs() < 1e-6, "{sum}");
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(P2, P2)> {
        let (a, b, c, d) = (P2(x0, y0), P2(x1, y0), P2(x1, y1), P2(x0, y1));
        vec![(a, b), (b, c), (c, d), (d, a)]
    }

    fn areas(segs: &[(P2, P2)]) -> Vec<f64> {
        let mut a: Vec<f64> = faces(segs, 1.0).iter().map(|f| area(f)).collect();
        a.sort_by(f64::total_cmp);
        a
    }

    #[test]
    fn ring_area_is_positive_counter_clockwise() {
        let ccw = [P2(0.0, 0.0), P2(4.0, 0.0), P2(4.0, 3.0), P2(0.0, 3.0)];
        assert_eq!(area(&ccw), 12.0);
        let cw: Vec<P2> = ccw.iter().rev().copied().collect();
        assert_eq!(area(&cw), -12.0);
        assert_eq!(perimeter(&ccw), 14.0);
    }

    #[test]
    fn contains_is_even_odd() {
        let sq = [P2(0.0, 0.0), P2(4.0, 0.0), P2(4.0, 4.0), P2(0.0, 4.0)];
        assert!(contains(&sq, P2(2.0, 2.0)));
        assert!(!contains(&sq, P2(5.0, 2.0)));
        assert!(!contains(&sq, P2(-1.0, 2.0)));
    }

    #[test]
    fn distance_to_a_segment_clamps_to_its_ends() {
        let (a, b) = (P2(0.0, 0.0), P2(10.0, 0.0));
        assert_eq!(dist_to_segment(P2(5.0, 3.0), a, b), 3.0);
        assert_eq!(dist_to_segment(P2(13.0, 4.0), a, b), 5.0);
    }

    #[test]
    fn lines_meet_where_they_cross_and_parallel_ones_never() {
        let hit = line_intersection(P2(0.0, 3000.0), P2(8000.0, 3000.0), P2(9000.0, 0.0), P2(9000.0, 6000.0));
        assert_eq!(hit, Some(P2(9000.0, 3000.0)), "past the ends of both");
        assert_eq!(line_intersection(P2(0.0, 0.0), P2(1.0, 0.0), P2(0.0, 1.0), P2(5.0, 1.0)), None);
    }

    #[test]
    fn a_closed_loop_has_an_inside_and_an_outside_face() {
        assert_eq!(areas(&rect(0.0, 0.0, 4000.0, 3000.0)), vec![-12e6, 12e6]);
    }

    #[test]
    fn partitions_split_rooms_at_t_junctions_and_crossings() {
        // A partition meeting the top and bottom walls mid-span (T-junctions)...
        let mut segs = rect(0.0, 0.0, 6000.0, 4000.0);
        segs.push((P2(2000.0, 0.0), P2(2000.0, 4000.0)));
        assert_eq!(areas(&segs), vec![-24e6, 8e6, 16e6]);
        // ...and one crossing it (an X-junction) that overshoots the outer walls.
        segs.push((P2(-500.0, 2000.0), P2(6500.0, 2000.0)));
        let rooms: Vec<f64> = areas(&segs).into_iter().filter(|a| *a > 0.0).collect();
        assert_eq!(rooms, vec![4e6, 4e6, 8e6, 8e6]);
    }

    #[test]
    fn a_wall_stub_inside_a_room_leaves_its_area_alone() {
        let mut segs = rect(0.0, 0.0, 4000.0, 3000.0);
        segs.push((P2(4000.0, 1500.0), P2(3000.0, 1500.0)));
        let r = region_at(&segs, P2(1000.0, 1000.0), 1.0).unwrap();
        assert_eq!(r.area(), 12e6);
    }

    #[test]
    fn rooms_take_the_smallest_loop_around_them_minus_free_standing_islands() {
        let mut segs = rect(0.0, 0.0, 10000.0, 10000.0);
        segs.extend(rect(4000.0, 4000.0, 6000.0, 6000.0)); // a shaft touching nothing
        segs.extend(rect(4500.0, 4500.0, 5500.0, 5500.0)); // a duct inside the shaft
        let hall = region_at(&segs, P2(1000.0, 1000.0), 1.0).unwrap();
        assert_eq!(hall.holes.len(), 1, "only the outermost island is a hole");
        assert_eq!(hall.area(), 96e6);
        assert_eq!(hall.perimeter(), 48000.0);
        let shaft = region_at(&segs, P2(4200.0, 4200.0), 1.0).unwrap();
        assert_eq!(shaft.area(), 3e6);
        assert!(region_at(&segs, P2(-10.0, 5.0), 1.0).is_none());
    }

    #[test]
    fn one_arrangement_answers_as_many_points_as_a_fresh_one_would() {
        let mut segs = rect(0.0, 0.0, 10000.0, 10000.0);
        segs.extend(rect(4000.0, 4000.0, 6000.0, 6000.0));
        segs.push((P2(0.0, 5000.0), P2(3000.0, 5000.0)));
        let all = faces(&segs, 1.0);
        for p in [P2(1000.0, 1000.0), P2(7000.0, 2000.0), P2(4200.0, 4200.0), P2(-10.0, 5.0)] {
            assert_eq!(region_in(&all, p, 1.0), region_at(&segs, p, 1.0), "at {p:?}");
        }
    }

    #[test]
    fn wall_ends_within_tolerance_still_close_a_room() {
        let segs = vec![
            (P2(0.0, 0.0), P2(4000.0, 0.0)),
            (P2(4000.4, 0.3), P2(4000.0, 3000.0)),
            (P2(4000.0, 3000.0), P2(0.0, 3000.0)),
            (P2(0.0, 3000.0), P2(0.2, -0.2)),
        ];
        let r = region_at(&segs, P2(2000.0, 1500.0), 1.0).unwrap();
        assert!((r.area() - 12e6).abs() < 3000.0, "{}", r.area());
    }
}
