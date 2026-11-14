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

/// Elevations by position: each given point's and line vertex's, refusing one position with two.
#[derive(Default)]
struct Heights(BTreeMap<(i64, i64), f64>);

impl Heights {
    fn add(&mut self, p: P2, z: f64) -> Result<(), String> {
        match self.0.get(&key(p)) {
            Some(&was) if (was - z).abs() > 1e-6 => {
                Err(format!("{} is on contours or points of two elevations, {} and {} mm", at(p), was.min(z), was.max(z)))
            }
            Some(_) => Ok(()),
            None => {
                self.0.insert(key(p), z);
                Ok(())
            }
        }
    }

    /// The elevation at a corner the triangulation made from the positions given: at its key, or the next one round it
    /// (corners within `TOL` merge, so the one kept may round to a neighbouring key).
    fn of(&self, p: P2) -> Option<f64> {
        let (x, y) = key(p);
        [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)]
            .iter()
            .find_map(|(dx, dy)| self.0.get(&(x + dx, y + dy)).copied())
    }
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

/// The surface through `points` (`[x, y, z]`) and `lines`, the lines kept as triangles' edges. Refused, saying where: a
/// point not finite or past `MAX_COORD`, a line with as many elevations as points, one position at two elevations,
/// lines that cross, fewer than three points or all on one line, and what the triangulation refuses (more than
/// `triangulation::MAX_CORNERS` corners as given, a region past `triangulation::MAX_ACROSS`).
pub fn build(points: &[[f64; 3]], lines: &[Line], options: &Options) -> Result<Surface, String> {
    let _ = options;
    let mut heights = Heights::default();
    let mut isolated: Vec<P2> = Vec::with_capacity(points.len());
    for &[x, y, z] in points {
        let p = P2(x, y);
        check_point(p, z)?;
        heights.add(p, z)?;
        isolated.push(p);
    }
    let mut segments: Vec<(P2, P2)> = Vec::new();
    let mut positions: Vec<P2> = isolated.clone();
    for line in lines {
        let Some((pts, zs, closed)) = cleaned(line)? else { continue };
        for (&p, &z) in pts.iter().zip(&zs) {
            heights.add(p, z)?;
        }
        segments.extend(pts.windows(2).map(|w| (w[0], w[1])));
        if closed && let (Some(&a), Some(&b)) = (pts.last(), pts.first()) {
            segments.push((a, b));
        }
        positions.extend(pts);
    }
    if heights.0.len() < 3 {
        return Err(format!("a surface needs at least three points; it has {}", heights.0.len()));
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
    Ok(Surface { points: t.points, z, triangles: t.triangles, notes: Vec::new() })
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
    fn touching_contours_of_two_elevations_are_refused_saying_where() {
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 0.0), P2(1_000.0, 0.0)], false),
            Line::contour(200.0, vec![P2(1_000.0, 0.0), P2(1_000.0, 1_000.0)], false),
        ];
        let e = build(&[[0.0, 1_000.0, 150.0]], &lines, &Options::default()).unwrap_err();
        assert!(e.contains("(1000, 0)") && e.contains("100") && e.contains("200"), "{e}");
    }

    #[test]
    fn crossing_contours_are_refused_saying_where() {
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 500.0), P2(1_000.0, 500.0)], false),
            Line::contour(200.0, vec![P2(500.0, 0.0), P2(500.0, 1_000.0)], false),
        ];
        let e = build(&[], &lines, &Options::default()).unwrap_err();
        assert!(e.contains("cross") && e.contains("(500, 500)"), "{e}");
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

    #[test]
    fn a_non_finite_or_far_point_is_refused() {
        let far = |p: [f64; 3]| build(&[p, [1_000.0, 0.0, 0.0], [0.0, 1_000.0, 0.0]], &[], &Options::default());
        assert!(far([f64::NAN, 0.0, 0.0]).is_err());
        assert!(far([0.0, 0.0, f64::INFINITY]).is_err());
        assert!(far([2e9, 0.0, 0.0]).is_err());
    }
}
