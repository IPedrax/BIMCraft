//! A terrain surface: points and lines with elevations (contours, and breaklines whose elevation varies) triangulated in
//! plan, the lines kept as triangles' edges, each corner lifted to its elevation. What a toposolid is made of
//! ([the site plan](../../../../docs/site-plan.md)).

use std::collections::BTreeMap;

use crate::geom::{self, P2};
use crate::model::{MAX_COORD, TOL};
use crate::triangulation;

/// A line the surface keeps as triangles' edges, in plan, with an elevation for each of its points: a contour when they
/// are all the same, a breakline (a road's edge, a ridge) when they vary.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub points: Vec<P2>,
    pub z: Vec<f64>,
    pub closed: bool,
}

impl Line {
    /// A contour: every point at `z`.
    pub fn contour(z: f64, points: Vec<P2>, closed: bool) -> Line {
        Line { z: vec![z; points.len()], points, closed }
    }
}

/// How a surface is made from its data.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// A contour's vertex within this many mm of the line through its neighbours is left out; 0 keeps them all.
    pub simplify: f64,
    /// A closed contour enclosing less than this many mm² is left out: the loops photogrammetry draws round trees.
    pub ignore_loops_under: f64,
    /// The outline the surface is clipped to; without one, its points' convex hull.
    pub boundary: Option<Vec<P2>>,
}

impl Default for Options {
    fn default() -> Options {
        Options { simplify: 50.0, ignore_loops_under: 0.0, boundary: None }
    }
}

/// A triangulated surface: corners in plan, each with its elevation, and triangles counter-clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub points: Vec<P2>,
    pub z: Vec<f64>,
    pub triangles: Vec<[usize; 3]>,
    /// What was done to the data on the way, for the reply.
    pub notes: Vec<String>,
}

/// A position as messages give it, to the millimetre.
fn at(p: P2) -> String {
    format!("({:.0}, {:.0})", p.0, p.1)
}

/// A position as a key: to the nearest `TOL`.
fn key(p: P2) -> (i64, i64) {
    ((p.0 / TOL).round() as i64, (p.1 / TOL).round() as i64)
}

/// Elevations by position: each given point's and line vertex's. A position given at two elevations (contours that
/// touch, as a noisy survey's do) takes their mean, and is remembered so the reply can say where.
#[derive(Default)]
struct Heights {
    /// By position: the sum of its elevations, how many, the lowest and the highest.
    at: BTreeMap<(i64, i64), (f64, usize, f64, f64)>,
}

impl Heights {
    fn add(&mut self, p: P2, z: f64) {
        let e = self.at.entry(key(p)).or_insert((0.0, 0, z, z));
        *e = (e.0 + z, e.1 + 1, e.2.min(z), e.3.max(z));
    }

    fn len(&self) -> usize {
        self.at.len()
    }

    /// The positions given at more than one elevation, in order.
    fn torn(&self) -> Vec<P2> {
        self.at.iter().filter(|(_, e)| e.3 - e.2 > 1e-6).map(|((x, y), _)| P2(*x as f64 * TOL, *y as f64 * TOL)).collect()
    }

    /// The elevation at a corner the triangulation made from the positions given: at its key, or the next one round it
    /// (corners within `TOL` merge, so the one kept may round to a neighbouring key).
    fn of(&self, p: P2) -> Option<f64> {
        let (x, y) = key(p);
        [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)]
            .iter()
            .find_map(|(dx, dy)| self.at.get(&(x + dx, y + dy)).map(|e| e.0 / e.1 as f64))
    }
}

/// A few positions as a message lists them.
fn some(points: &[P2]) -> String {
    let shown: Vec<String> = points.iter().take(3).map(|&p| at(p)).collect();
    if points.len() > 3 { format!("{} and {} more", shown.join(", "), points.len() - 3) } else { shown.join(", ") }
}

/// Which of `segments` properly cross another (each crossing's two), found through a grid of cells about as many as
/// the segments, so a survey's thousands take a moment, not their square.
fn crossing(segments: &[(P2, P2)]) -> Vec<bool> {
    let mut out = vec![false; segments.len()];
    let (mut lo, mut hi) = (P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN));
    for (a, b) in segments {
        lo = P2(lo.0.min(a.0).min(b.0), lo.1.min(a.1).min(b.1));
        hi = P2(hi.0.max(a.0).max(b.0), hi.1.max(a.1).max(b.1));
    }
    if segments.is_empty() {
        return out;
    }
    let cell = ((hi.0 - lo.0).max(hi.1 - lo.1) / (segments.len() as f64).sqrt()).max(TOL);
    let index = |v: f64, o: f64| ((v - o) / cell).floor() as i64;
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, (a, b)) in segments.iter().enumerate() {
        for gx in index(a.0.min(b.0), lo.0)..=index(a.0.max(b.0), lo.0) {
            for gy in index(a.1.min(b.1), lo.1)..=index(a.1.max(b.1), lo.1) {
                grid.entry((gx, gy)).or_default().push(i);
            }
        }
    }
    let side = |p: P2, q: P2, r: P2| (q - p).cross(r - p);
    for ids in grid.values() {
        for (n, &i) in ids.iter().enumerate() {
            for &j in &ids[n + 1..] {
                let ((a, b), (c, d)) = (segments[i], segments[j]);
                if side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0 {
                    out[i] = true;
                    out[j] = true;
                }
            }
        }
    }
    out
}

fn check_point(p: P2, z: f64) -> Result<(), String> {
    let ok = |v: f64| v.is_finite() && v.abs() <= MAX_COORD;
    if ok(p.0) && ok(p.1) && ok(z) {
        Ok(())
    } else {
        Err(format!("a point at ({}, {}, {}) is not finite or lies past {MAX_COORD} mm", p.0, p.1, z))
    }
}

/// A line's points and elevations with its repeats left out: a vertex within `TOL` of the last one kept, and a closed
/// line's last vertex where it is its first. None when fewer than two are left.
fn cleaned(line: &Line) -> Result<Option<(Vec<P2>, Vec<f64>, bool)>, String> {
    if line.z.len() != line.points.len() {
        return Err(format!("a line has {} points but {} elevations", line.points.len(), line.z.len()));
    }
    let mut pts: Vec<P2> = Vec::with_capacity(line.points.len());
    let mut zs: Vec<f64> = Vec::with_capacity(line.points.len());
    for (&p, &z) in line.points.iter().zip(&line.z) {
        check_point(p, z)?;
        if pts.last().is_some_and(|q: &P2| q.dist(p) < TOL) {
            continue;
        }
        pts.push(p);
        zs.push(z);
    }
    let closed = line.closed || (pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < TOL));
    if closed && pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < TOL) {
        pts.pop();
        zs.pop();
    }
    let closed = closed && pts.len() > 2;
    Ok((pts.len() >= 2).then_some((pts, zs, closed)))
}

/// A line made ready: its points, their elevations, and whether it closes.
type Ready = (Vec<P2>, Vec<f64>, bool);

/// Whether a line is a contour: one elevation all along.
fn is_contour(line: &Ready) -> bool {
    line.1.windows(2).all(|w| w[0] == w[1])
}

/// A contour's points without those within `tol` of the segment from the last point kept to the next: a wiggle of
/// photogrammetry's left out, a bend kept. Its ends stay; a closed one keeps at least three.
fn simplified(pts: &[P2], closed: bool, tol: f64) -> Vec<P2> {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return pts.to_vec() };
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut kept = vec![first];
    for w in pts.windows(3) {
        let prev = kept.last().copied().unwrap_or(first);
        if geom::dist_to_segment(w[1], prev, w[2]) >= tol {
            kept.push(w[1]);
        }
    }
    kept.push(last);
    if closed && kept.len() < 3 { pts.to_vec() } else { kept }
}

/// Which lines have a segment that crosses another line's.
fn lines_crossing(lines: &[Ready]) -> Vec<bool> {
    let mut owner = Vec::new();
    let mut segments = Vec::new();
    for (i, (pts, _, closed)) in lines.iter().enumerate() {
        for w in pts.windows(2) {
            segments.push((w[0], w[1]));
            owner.push(i);
        }
        if *closed && let (Some(&a), Some(&b)) = (pts.last(), pts.first()) {
            segments.push((a, b));
            owner.push(i);
        }
    }
    let mut out = vec![false; lines.len()];
    for (i, c) in crossing(&segments).into_iter().enumerate() {
        if c && let Some(&o) = owner.get(i) {
            out[o] = true;
        }
    }
    out
}

/// The surface through `points` (`[x, y, z]`) and `lines`, the lines kept as triangles' edges. Data a noisy survey has
/// still makes a surface, and its `notes` say what was done: a position given at two elevations takes their mean, and
/// segments that cross another are not kept as edges (their vertices stay as points). Refused, saying why: a point not
/// finite or past `MAX_COORD`, a line without an elevation for each point, fewer than three points or all on one line,
/// and what the triangulation refuses (more than `triangulation::MAX_CORNERS` corners as given, a region past
/// `triangulation::MAX_ACROSS`).
pub fn build(points: &[[f64; 3]], lines: &[Line], options: &Options) -> Result<Surface, String> {
    let ok = |v: f64| v.is_finite() && v >= 0.0;
    if !ok(options.simplify) {
        return Err(format!(
            "`simplify` is how far, mm, a contour's vertex may lie from the line through its neighbours and be left out: 0 or more, not {}",
            options.simplify
        ));
    }
    if !ok(options.ignore_loops_under) {
        return Err(format!(
            "`ignore_loops_under` is the area, mm², under which a closed contour is left out: 0 or more, not {}",
            options.ignore_loops_under
        ));
    }
    let mut notes = Vec::new();
    let mut ready: Vec<Ready> = Vec::with_capacity(lines.len());
    for line in lines {
        if let Some(r) = cleaned(line)? {
            ready.push(r);
        }
    }
    if options.ignore_loops_under > 0.0 {
        let before = ready.len();
        ready.retain(|r| !(r.2 && is_contour(r) && geom::area(&r.0).abs() < options.ignore_loops_under));
        let n = before - ready.len();
        if n > 0 {
            notes.push(format!("{n} closed contours enclosing less than {} m² were left out", options.ignore_loops_under / 1e6));
        }
    }
    if options.simplify > 0.0 {
        let raw = ready.clone();
        for r in ready.iter_mut().filter(|r| is_contour(r)) {
            let pts = simplified(&r.0, r.2, options.simplify);
            r.1 = vec![r.1.first().copied().unwrap_or(0.0); pts.len()];
            r.0 = pts;
        }
        // A contour that crosses another only once simplified keeps every vertex; again, in case that made another cross.
        let crossed_raw = lines_crossing(&raw);
        let mut reverted = 0;
        for _ in 0..3 {
            let now = lines_crossing(&ready);
            let back: Vec<usize> = (0..ready.len()).filter(|&i| now[i] && !crossed_raw[i] && ready[i].0.len() != raw[i].0.len()).collect();
            if back.is_empty() {
                break;
            }
            for i in back {
                if let (Some(r), Some(w)) = (ready.get_mut(i), raw.get(i)) {
                    *r = w.clone();
                    reverted += 1;
                }
            }
        }
        if reverted > 0 {
            notes.push(format!("simplifying would have made {reverted} contours cross, so they kept every vertex"));
        }
        let (was, now): (usize, usize) = (raw.iter().map(|r| r.0.len()).sum(), ready.iter().map(|r| r.0.len()).sum());
        if now < was {
            notes.push(format!("contours simplified within {} mm: {was} vertices became {now}", options.simplify));
        }
    }
    let mut heights = Heights::default();
    let mut isolated: Vec<P2> = Vec::with_capacity(points.len());
    for &[x, y, z] in points {
        let p = P2(x, y);
        check_point(p, z)?;
        heights.add(p, z);
        isolated.push(p);
    }
    let mut segments: Vec<(P2, P2)> = Vec::new();
    let mut positions: Vec<P2> = isolated.clone();
    for (pts, zs, closed) in ready {
        for (&p, &z) in pts.iter().zip(&zs) {
            heights.add(p, z);
        }
        segments.extend(pts.windows(2).map(|w| (w[0], w[1])));
        if closed && let (Some(&a), Some(&b)) = (pts.last(), pts.first()) {
            segments.push((a, b));
        }
        positions.extend(pts);
    }
    if heights.len() < 3 {
        return Err(format!("a surface needs at least three points; it has {}", heights.len()));
    }
    let torn = heights.torn();
    if !torn.is_empty() {
        let n = torn.len();
        notes.push(format!("{n} point{} given at two elevations took their mean: {}", if n == 1 { "" } else { "s" }, some(&torn)));
    }
    let crossed = crossing(&segments);
    if crossed.iter().any(|&c| c) {
        let (mut lo, mut hi) = (P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN));
        let mut kept = Vec::with_capacity(segments.len());
        let mut n = 0;
        for (&(a, b), &c) in segments.iter().zip(&crossed) {
            if c {
                n += 1;
                isolated.extend([a, b]);
                lo = P2(lo.0.min(a.0).min(b.0), lo.1.min(a.1).min(b.1));
                hi = P2(hi.0.max(a.0).max(b.0), hi.1.max(a.1).max(b.1));
            } else {
                kept.push((a, b));
            }
        }
        segments = kept;
        isolated.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        isolated.dedup_by(|a, b| a.dist(*b) < TOL);
        notes.push(format!(
            "{n} segments of contours or breaklines that cross another were kept as points, not edges, between {} and {}",
            at(lo),
            at(hi)
        ));
    }
    let outline = geom::hull(positions);
    if outline.len() < 3 || geom::area(&outline).abs() < TOL * TOL {
        return Err("its points are all on one line, so they make no surface".into());
    }
    let t = triangulation::constrained(&outline, &[], &isolated, &segments)
        .map_err(|e| if e.contains("cross") { format!("contours or breaklines cross: {e}") } else { e })?;
    let z = t
        .points
        .iter()
        .map(|&p| heights.of(p).ok_or_else(|| format!("the triangulation made a corner at {} that no point was given at", at(p))))
        .collect::<Result<Vec<f64>, String>>()?;
    Ok(Surface { points: t.points, z, triangles: t.triangles, notes })
}

impl Surface {
    /// A triangle's corners in plan and their elevations.
    fn corners(&self, t: &[usize; 3]) -> Option<([P2; 3], [f64; 3])> {
        let p = |i: usize| self.points.get(t[i]).copied();
        let z = |i: usize| self.z.get(t[i]).copied();
        Some(([p(0)?, p(1)?, p(2)?], [z(0)?, z(1)?, z(2)?]))
    }

    /// The surface's elevation at a plan point; none outside it.
    pub fn elevation_at(&self, at: P2) -> Option<f64> {
        self.triangles.iter().find_map(|t| {
            let ([a, b, c], [za, zb, zc]) = self.corners(t)?;
            let area = (b - a).cross(c - a);
            if area.abs() <= 0.0 {
                return None;
            }
            let (l1, l2) = ((c - b).cross(at - b) / area, (a - c).cross(at - c) / area);
            let l3 = 1.0 - l1 - l2;
            let eps = -1e-9;
            (l1 >= eps && l2 >= eps && l3 >= eps).then(|| l1 * za + l2 * zb + l3 * zc)
        })
    }

    /// Its area in plan, mm².
    pub fn area(&self) -> f64 {
        self.triangles.iter().filter_map(|t| self.corners(t)).map(|([a, b, c], _)| (b - a).cross(c - a).abs() / 2.0).sum()
    }

    /// Its area along the ground, mm².
    pub fn surface_area(&self) -> f64 {
        self.triangles
            .iter()
            .filter_map(|t| self.corners(t))
            .map(|([a, b, c], [za, zb, zc])| {
                let (u, v) = ([b.0 - a.0, b.1 - a.1, zb - za], [c.0 - a.0, c.1 - a.1, zc - za]);
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() / 2.0
            })
            .sum()
    }

    /// The volume between the elevation `base` and the surface, mm³: each triangle's area in plan times its mean height
    /// over `base` (negative where it lies under it).
    pub fn volume_above(&self, base: f64) -> f64 {
        self.triangles
            .iter()
            .filter_map(|t| self.corners(t))
            .map(|([a, b, c], [za, zb, zc])| (b - a).cross(c - a).abs() / 2.0 * ((za + zb + zc) / 3.0 - base))
            .sum()
    }

    /// Its lowest and highest elevation.
    pub fn range(&self) -> Option<(f64, f64)> {
        self.z.iter().fold(None, |r, &z| match r {
            None => Some((z, z)),
            Some((lo, hi)) => Some((lo.min(z), hi.max(z))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::P2;

    /// The plane z = 100 + 0.1·x + 0.05·y.
    fn plane(p: P2) -> f64 {
        100.0 + 0.1 * p.0 + 0.05 * p.1
    }

    const SIDE: f64 = 20_000.0;

    /// The square's corners, on the plane.
    fn plane_corners() -> Vec<[f64; 3]> {
        [P2(0.0, 0.0), P2(SIDE, 0.0), P2(SIDE, SIDE), P2(0.0, SIDE)].iter().map(|&p| [p.0, p.1, plane(p)]).collect()
    }

    /// The plane's contours over the square, one every 500 mm of rise from 600 to 2600: each the straight line where the
    /// plane has that elevation, from one side of the square to another.
    fn plane_contours() -> Vec<Line> {
        (0..5)
            .map(|i| {
                let z = 600.0 + 500.0 * i as f64;
                // 0.1·x + 0.05·y = z − 100, met by the square's sides.
                let k = z - 100.0;
                let mut ends: Vec<P2> =
                    [P2(0.0, k / 0.05), P2(SIDE, (k - 0.1 * SIDE) / 0.05), P2(k / 0.1, 0.0), P2((k - 0.05 * SIDE) / 0.1, SIDE)]
                        .into_iter()
                        .filter(|p| (-1e-9..=SIDE + 1e-9).contains(&p.0) && (-1e-9..=SIDE + 1e-9).contains(&p.1))
                        .collect();
                ends.dedup_by(|a, b| a.dist(*b) < 1e-6);
                Line::contour(z, ends, false)
            })
            .collect()
    }

    fn plane_surface() -> Surface {
        build(&plane_corners(), &plane_contours(), &Options::default()).unwrap()
    }

    /// Whether segments a–b and c–d cross at a point inside both.
    fn proper_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
        let side = |p: P2, q: P2, r: P2| (q - p).cross(r - p);
        let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
        d1 * d2 < -1e-6 && d3 * d4 < -1e-6
    }

    fn crosses_none(s: &Surface, lines: &[Line]) -> bool {
        s.triangles.iter().all(|t| {
            (0..3).all(|i| {
                let (a, b) = (s.points[t[i]], s.points[t[(i + 1) % 3]]);
                lines.iter().all(|l| l.points.windows(2).all(|w| !proper_cross(a, b, w[0], w[1])))
            })
        })
    }

    #[test]
    fn a_plane_from_contours_is_the_plane_everywhere_inside() {
        let s = plane_surface();
        for at in [P2(1.0, 1.0), P2(10_000.0, 5_000.0), P2(19_999.0, 19_999.0), P2(7_321.5, 12_345.6)] {
            let z = s.elevation_at(at).unwrap();
            assert!((z - plane(at)).abs() < 1e-6, "{at:?}: {z}");
        }
        assert!((s.area() - SIDE * SIDE).abs() < 1e-3, "{}", s.area());
        let tilted = SIDE * SIDE * (1.0f64 + 0.01 + 0.0025).sqrt();
        assert!((s.surface_area() - tilted).abs() < 1.0, "{} {tilted}", s.surface_area());
        // The plane's mean over the square is 100 + 0.1 × 10 000 + 0.05 × 10 000 = 1 600 mm.
        assert!((s.volume_above(0.0) - SIDE * SIDE * 1600.0).abs() < 1e3, "{}", s.volume_above(0.0));
        assert!((s.volume_above(1600.0)).abs() < 1e3, "{}", s.volume_above(1600.0));
        assert_eq!(s.range(), Some((100.0, 3100.0)));
    }

    #[test]
    fn contours_are_breaklines() {
        let corners: Vec<[f64; 3]> = [[0.0, 0.0, 0.0], [10_000.0, 0.0, 0.0], [10_000.0, 10_000.0, 0.0], [0.0, 10_000.0, 0.0]].to_vec();
        let lines = vec![
            Line::contour(100.0, vec![P2(1_000.0, 9_000.0), P2(5_000.0, 1_000.0), P2(9_000.0, 9_000.0)], false),
            Line::contour(200.0, vec![P2(3_000.0, 9_000.0), P2(5_000.0, 5_000.0), P2(7_000.0, 9_000.0)], false),
        ];
        let s = build(&corners, &lines, &Options::default()).unwrap();
        assert!(crosses_none(&s, &lines));
        // Along a contour the surface is at the contour's elevation.
        assert!((s.elevation_at(P2(3_000.0, 5_000.0)).unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_breakline_of_varying_elevation_is_followed() {
        let corners: Vec<[f64; 3]> = [[0.0, 0.0, 0.0], [10_000.0, 0.0, 0.0], [10_000.0, 10_000.0, 0.0], [0.0, 10_000.0, 0.0]].to_vec();
        let ridge = Line { points: vec![P2(0.0, 5_000.0), P2(10_000.0, 5_000.0)], z: vec![0.0, 1_000.0], closed: false };
        let s = build(&corners, std::slice::from_ref(&ridge), &Options::default()).unwrap();
        assert!(crosses_none(&s, std::slice::from_ref(&ridge)));
        assert!((s.elevation_at(P2(5_000.0, 5_000.0)).unwrap() - 500.0).abs() < 1e-6);
        assert!((s.elevation_at(P2(2_500.0, 5_000.0)).unwrap() - 250.0).abs() < 1e-6);
    }

    #[test]
    fn outside_the_surface_has_no_elevation() {
        let s = plane_surface();
        assert_eq!(s.elevation_at(P2(-1.0, 5_000.0)), None);
        assert_eq!(s.elevation_at(P2(25_000.0, 5_000.0)), None);
    }

    #[test]
    fn touching_contours_of_two_elevations_meet_halfway_and_say_so() {
        // Noisy surveys do this; a surface is still made, at the mean, and the reply says where.
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 0.0), P2(1_000.0, 0.0)], false),
            Line::contour(200.0, vec![P2(1_000.0, 0.0), P2(1_000.0, 1_000.0)], false),
        ];
        let s = build(&[[0.0, 1_000.0, 150.0]], &lines, &Options::default()).unwrap();
        assert!((s.elevation_at(P2(1_000.0, 0.0)).unwrap() - 150.0).abs() < 1e-6);
        assert!(s.notes.iter().any(|n| n.contains("1 point") && n.contains("two elevations") && n.contains("(1000, 0)")), "{:?}", s.notes);
    }

    #[test]
    fn crossing_contours_lose_their_edges_there_not_the_surface() {
        // Contours of one ground never cross; photogrammetry's often do, round trees. Where they cross they stop being
        // edges the triangles must keep, their vertices stay as points, and the reply says how many and where.
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 500.0), P2(1_000.0, 500.0)], false),
            Line::contour(200.0, vec![P2(500.0, 0.0), P2(500.0, 1_000.0)], false),
        ];
        let s = build(&[], &lines, &Options::default()).unwrap();
        for (p, z) in [(P2(0.0, 500.0), 100.0), (P2(1_000.0, 500.0), 100.0), (P2(500.0, 0.0), 200.0), (P2(500.0, 1_000.0), 200.0)] {
            assert!((s.elevation_at(p).unwrap() - z).abs() < 1e-6, "{p:?}");
        }
        assert!(
            s.notes.iter().any(|n| n.contains("2 segments") && n.contains("cross") && n.contains("(0, 0)") && n.contains("(1000, 1000)")),
            "{:?}",
            s.notes
        );
        // Lines that don't cross stay edges.
        let clean = plane_contours();
        assert!(plane_surface().notes.iter().all(|n| !n.contains("cross")));
        assert!(crosses_none(&plane_surface(), &clean));
    }

    #[test]
    fn a_repeated_vertex_is_read_once() {
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 0.0), P2(500.0, 0.0), P2(500.0, 0.0), P2(1_000.0, 0.0)], false),
            Line::contour(200.0, vec![P2(0.0, 1_000.0), P2(1_000.0, 1_000.0), P2(1_000.0, 2_000.0), P2(0.0, 1_000.0)], true),
        ];
        let s = build(&[], &lines, &Options::default()).unwrap();
        assert!((s.elevation_at(P2(500.0, 0.0)).unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn too_few_points_or_all_on_a_line_are_refused() {
        let e = build(&[[0.0, 0.0, 0.0], [1_000.0, 0.0, 0.0]], &[], &Options::default()).unwrap_err();
        assert!(e.contains("at least three"), "{e}");
        let e = build(&[[0.0, 0.0, 0.0], [1_000.0, 0.0, 0.0], [2_000.0, 0.0, 5.0]], &[], &Options::default()).unwrap_err();
        assert!(e.contains("on one line"), "{e}");
    }

    #[test]
    fn a_line_whose_elevations_dont_match_its_points_is_refused() {
        let bad = Line { points: vec![P2(0.0, 0.0), P2(1_000.0, 0.0), P2(1_000.0, 1_000.0)], z: vec![0.0, 1.0], closed: false };
        let e = build(&[], &[bad], &Options::default()).unwrap_err();
        assert!(e.contains("elevation"), "{e}");
    }

    /// A contour along y = 0 from x = 0 to 99 900, a vertex every 100 mm wiggling ±10 mm, inside a 100 m by 10 m box.
    fn wiggly() -> (Vec<[f64; 3]>, Vec<Line>) {
        let corners = vec![[0.0, -5_000.0, 0.0], [100_000.0, -5_000.0, 0.0], [100_000.0, 5_000.0, 0.0], [0.0, 5_000.0, 0.0]];
        let pts: Vec<P2> = (0..1000).map(|i| P2(i as f64 * 100.0, if i % 2 == 0 { 10.0 } else { -10.0 })).collect();
        (corners, vec![Line::contour(100.0, pts, false)])
    }

    #[test]
    fn simplifying_drops_vertices_near_the_line_through_their_neighbours() {
        let (corners, lines) = wiggly();
        let s = build(&corners, &lines, &Options { simplify: 50.0, ..Options::default() }).unwrap();
        assert!(s.points.len() <= 4 + 10, "{}", s.points.len());
        assert!(s.notes.iter().any(|n| n.contains("simplified")), "{:?}", s.notes);
        let all = build(&corners, &lines, &Options { simplify: 0.0, ..Options::default() }).unwrap();
        assert!(all.points.len() >= 1000, "{}", all.points.len());
        assert!(all.notes.iter().all(|n| !n.contains("simplified")), "{:?}", all.notes);
    }

    #[test]
    fn simplifying_never_makes_lines_cross() {
        // A dips 45 mm at x = 500, which simplifying at 50 mm would straighten; B, 30 mm above, dips 50 mm into where A's
        // straightened line would run. Apart they never cross; A keeps its dip.
        let a = Line::contour(100.0, vec![P2(0.0, 0.0), P2(400.0, 0.0), P2(500.0, -45.0), P2(600.0, 0.0), P2(1_000.0, 0.0)], false);
        let b = Line::contour(200.0, vec![P2(0.0, 30.0), P2(400.0, 30.0), P2(500.0, -20.0), P2(600.0, 30.0), P2(1_000.0, 30.0)], false);
        let corners = vec![[0.0, -1_000.0, 0.0], [1_000.0, -1_000.0, 0.0], [1_000.0, 1_000.0, 300.0], [0.0, 1_000.0, 300.0]];
        let s = build(&corners, &[a.clone(), b.clone()], &Options { simplify: 50.0, ..Options::default() }).unwrap();
        assert!(s.notes.iter().any(|n| n.contains("kept every vertex")), "{:?}", s.notes);
        assert!(s.notes.iter().all(|n| !n.contains("kept as points")), "{:?}", s.notes);
        assert!(crosses_none(&s, &[a, b]));
        assert!((s.elevation_at(P2(500.0, -45.0)).unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn a_breaklines_vertices_are_never_simplified() {
        // Leaving out a vertex of a line whose elevation varies would change the ground along it.
        let corners = vec![[0.0, -5_000.0, 0.0], [10_000.0, -5_000.0, 0.0], [10_000.0, 5_000.0, 0.0], [0.0, 5_000.0, 0.0]];
        let road = Line { points: vec![P2(0.0, 0.0), P2(5_000.0, 10.0), P2(10_000.0, 0.0)], z: vec![0.0, 900.0, 0.0], closed: false };
        let s = build(&corners, &[road], &Options { simplify: 50.0, ..Options::default() }).unwrap();
        assert!((s.elevation_at(P2(5_000.0, 10.0)).unwrap() - 900.0).abs() < 1e-6);
    }

    /// A closed square contour of half a square metre at `at`, at elevation 5000.
    fn tree(at: P2) -> Line {
        let d = 0.5e6f64.sqrt();
        Line::contour(5_000.0, vec![at, at + P2(d, 0.0), at + P2(d, d), at + P2(0.0, d)], true)
    }

    #[test]
    fn small_closed_loops_are_left_out_when_asked() {
        let mut lines = plane_contours();
        lines.extend([P2(1_000.0, 1_000.0), P2(18_000.0, 18_000.0), P2(5_000.0, 5_000.0)].map(tree));
        let s = build(&plane_corners(), &lines, &Options { ignore_loops_under: 1e6, ..Options::default() }).unwrap();
        assert_eq!(s.range(), Some((100.0, 3100.0)));
        assert!(s.notes.iter().any(|n| n.contains("3 closed contours")), "{:?}", s.notes);
        let kept = build(&plane_corners(), &lines, &Options::default()).unwrap();
        assert_eq!(kept.range(), Some((100.0, 5000.0)));
    }

    #[test]
    fn bad_options_are_refused_naming_them() {
        let (corners, lines) = wiggly();
        for (options, name) in [
            (Options { simplify: -1.0, ..Options::default() }, "simplify"),
            (Options { simplify: f64::NAN, ..Options::default() }, "simplify"),
            (Options { ignore_loops_under: -1.0, ..Options::default() }, "ignore_loops_under"),
            (Options { ignore_loops_under: f64::INFINITY, ..Options::default() }, "ignore_loops_under"),
        ] {
            let e = build(&corners, &lines, &options).unwrap_err();
            assert!(e.contains(name), "{e}");
        }
    }

    #[test]
    fn a_non_finite_or_far_point_is_refused() {
        let far = |p: [f64; 3]| build(&[p, [1_000.0, 0.0, 0.0], [0.0, 1_000.0, 0.0]], &[], &Options::default());
        assert!(far([f64::NAN, 0.0, 0.0]).is_err());
        assert!(far([0.0, 0.0, f64::INFINITY]).is_err());
        assert!(far([2e9, 0.0, 0.0]).is_err());
    }
}
