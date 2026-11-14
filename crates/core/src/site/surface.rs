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

/// The corners a surface is made of, each with its elevations: a position given within `TOL` of a corner already
/// there is that corner (the triangulation would merge them anyway, so they are merged here first, where their
/// elevations can be kept and a line between them dropped). A corner given at two elevations (contours that touch, as a
/// noisy survey's do) takes their mean, and is remembered so the reply can say where.
#[derive(Default)]
struct Heights {
    /// Each corner: where it is, the sum of its elevations, how many, the lowest and the highest.
    corners: Vec<(P2, f64, usize, f64, f64)>,
    /// The corners by their `TOL` cell, to find one within `TOL` of a position.
    grid: BTreeMap<(i64, i64), Vec<usize>>,
}

impl Heights {
    /// The corner within `TOL` of `p`, the nearest when there are several.
    fn near(&self, p: P2) -> Option<usize> {
        let (x, y) = key(p);
        (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)))
            .filter_map(|k| self.grid.get(&k))
            .flatten()
            .filter_map(|&i| self.corners.get(i).map(|c| (i, c.0.dist(p))))
            .filter(|(_, d)| *d <= TOL)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(i, _)| i)
    }

    /// `p` at elevation `z`: the corner it is, where that corner stands.
    fn add(&mut self, p: P2, z: f64) -> P2 {
        if let Some(c) = self.near(p).and_then(|i| self.corners.get_mut(i)) {
            *c = (c.0, c.1 + z, c.2 + 1, c.3.min(z), c.4.max(z));
            return c.0;
        }
        self.grid.entry(key(p)).or_default().push(self.corners.len());
        self.corners.push((p, z, 1, z, z));
        p
    }

    fn len(&self) -> usize {
        self.corners.len()
    }

    /// The corners given at more than one elevation, in the order they came.
    fn torn(&self) -> Vec<P2> {
        self.corners.iter().filter(|c| c.4 - c.3 > 1e-6).map(|c| c.0).collect()
    }

    /// The elevation of the corner at (or within `TOL` of) a corner the triangulation made.
    fn of(&self, p: P2) -> Option<f64> {
        self.near(p).and_then(|i| self.corners.get(i)).map(|c| c.1 / c.2 as f64)
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

/// A contour's points without those within `tol` of the line that replaces them (Douglas–Peucker): a wiggle of
/// photogrammetry's left out, a bend kept, and no point of the contour ever further than `tol` from what is left. Its ends
/// stay; a closed one keeps at least three.
fn simplified(pts: &[P2], closed: bool, tol: f64) -> Vec<P2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    // A closed contour runs back to its first point, so it is simplified as a path that ends where it starts.
    let path: Vec<P2> = if closed { pts.iter().chain(pts.first()).copied().collect() } else { pts.to_vec() };
    let last = path.len() - 1;
    let mut keep = vec![false; path.len()];
    keep[0] = true;
    keep[last] = true;
    let mut todo = vec![(0, last)];
    while let Some((i, j)) = todo.pop() {
        let (a, b) = (path[i], path[j]);
        let far = (i + 1..j).map(|k| (k, geom::dist_to_segment(path[k], a, b))).max_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((k, d)) = far
            && d > tol
        {
            keep[k] = true;
            todo.push((i, k));
            todo.push((k, j));
        }
    }
    let mut kept: Vec<P2> = path.iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
    if closed {
        kept.pop();
    }
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

/// The nearest point to `p` on the segment from `a` to `b`.
fn nearest_on(p: P2, a: P2, b: P2) -> P2 {
    a + (b - a) * geom::param_on_segment(p, a, b)
}

/// Whether segments a–b and c–d cross at a point inside both.
fn proper_cross(a: P2, b: P2, c: P2, d: P2) -> bool {
    let side = |p: P2, q: P2, r: P2| (q - p).cross(r - p);
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

/// The elevation for a point `c` of flat ground at `z0` that contours made: from the contour of `z0` nearest it (at
/// `d_own`) toward the nearest contour of another elevation (`z_q`, at `d_q`). Where the way there crosses a contour of
/// `z0`, `c` is inside a closed contour with nothing else in it (a summit or a pit), so the slope outside carries on in:
/// `z0 + (z0 − z_q)·d_own / (d_q − d_own)`. Otherwise it lies between the two (a valley's head, a ridge's nose):
/// `z0 + (z_q − z0)·d_own / (d_own + d_q)`. Either way within one interval, `|z0 − z_q|`, of `z0`. None when there is no
/// contour of another elevation.
fn lifted(c: P2, z0: f64, contours: &[(P2, P2, f64)]) -> Option<f64> {
    let same = |z: f64| (z - z0).abs() <= 1e-6;
    let mut own: Option<f64> = None;
    let mut other: Option<(f64, P2, f64)> = None;
    for &(a, b, z) in contours {
        let q = nearest_on(c, a, b);
        let d = c.dist(q);
        if same(z) {
            own = Some(own.map_or(d, |o| o.min(d)));
        } else if other.is_none_or(|(od, _, _)| d < od) {
            other = Some((d, q, z));
        }
    }
    let (d_own, (d_q, q, z_q)) = (own?, other?);
    let enclosed = contours.iter().any(|&(a, b, z)| same(z) && proper_cross(c, q, a, b));
    let z = if enclosed { z0 + (z0 - z_q) * d_own / (d_q - d_own).max(TOL) } else { z0 + (z_q - z0) * d_own / (d_own + d_q) };
    // Ground stays within one contour interval of the contour round it, or another contour would have been drawn; noisy
    // contours almost touching would otherwise send a summit's slope up without end.
    let interval = (z0 - z_q).abs();
    Some(z.clamp(z0 - interval, z0 + interval))
}

/// Whether `p` is at one of `keys`, or next to one (corners within `TOL` merge, so the one kept may round beside it).
fn near_key(keys: &std::collections::BTreeSet<(i64, i64)>, p: P2) -> bool {
    let (x, y) = key(p);
    (-1..=1).any(|dx| (-1..=1).any(|dy| keys.contains(&(x + dx, y + dy))))
}

/// The surface through `points` (`[x, y, z]`) and `lines`, the lines kept as triangles' edges. Data a noisy survey has
/// still makes a surface, and its `notes` say what was done: a position given at two elevations takes their mean, and
/// segments that cross another are not kept as edges (their vertices stay as points). Refused, saying why: a point not
/// finite or past `MAX_COORD`, a line without an elevation for each point, fewer than three points or all on one line,
/// and what the triangulation refuses (more than `triangulation::MAX_CORNERS` corners as given, a region past
/// `triangulation::MAX_ACROSS`).
pub fn build(points: &[[f64; 3]], lines: &[Line], options: &Options) -> Result<Surface, String> {
    let Some(boundary) = &options.boundary else { return build_in(points, lines, options, None, triangulation::MAX_CORNERS) };
    let whole = build_in(points, lines, &Options { boundary: None, ..options.clone() }, None, triangulation::MAX_CORNERS)?;
    let ring = checked_boundary(boundary)?;
    let mut corners = Vec::with_capacity(ring.len());
    for &p in &ring {
        let z = whole.elevation_at(p).ok_or_else(|| format!("the boundary's corner at {} is past the survey", at(p)))?;
        corners.push([p.0, p.1, z]);
    }
    let inside: Vec<[f64; 3]> = points.iter().copied().filter(|&[x, y, _]| geom::contains(&ring, P2(x, y))).collect();
    let mut pieces = Vec::new();
    for line in lines {
        if let Some((pts, zs, closed)) = cleaned(line)? {
            pieces.extend(clipped(&pts, &zs, closed, &ring));
        }
    }
    let left = (points.len() - inside.len(), lines.len());
    let mut s = build_in(&[inside, corners].concat(), &pieces, options, Some(&ring), triangulation::MAX_CORNERS)?;
    s.notes.insert(
        0,
        format!("clipped to the boundary: {} points outside the boundary were left out, and lines were cut where they cross it", left.0),
    );
    Ok(s)
}

/// A boundary checked: at least three corners, finite, with an area, and not crossing itself.
fn checked_boundary(boundary: &[P2]) -> Result<Vec<P2>, String> {
    let mut ring: Vec<P2> = Vec::with_capacity(boundary.len());
    for &p in boundary {
        check_point(p, 0.0)?;
        if ring.last().is_none_or(|q: &P2| q.dist(p) >= TOL) {
            ring.push(p);
        }
    }
    if ring.len() > 2 && ring.first().zip(ring.last()).is_some_and(|(a, b)| a.dist(*b) < TOL) {
        ring.pop();
    }
    if ring.len() < 3 {
        return Err("the boundary needs at least three corners round an area".into());
    }
    let n = ring.len();
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b, c, d) = (ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]);
            if proper_cross(a, b, c, d) {
                let x = geom::line_intersection(a, b, c, d).unwrap_or(a);
                return Err(format!("the boundary crosses itself at {}", at(x)));
            }
        }
    }
    if geom::area(&ring).abs() < TOL * TOL {
        return Err("the boundary needs at least three corners round an area".into());
    }
    Ok(ring)
}

/// The pieces of a line inside `ring`: each segment cut where it crosses the ring, the pieces whose middle is inside
/// kept and joined, their elevations interpolated along the segment.
fn clipped(pts: &[P2], zs: &[f64], closed: bool, ring: &[P2]) -> Vec<Line> {
    let mut segs: Vec<(P2, f64, P2, f64)> = pts.windows(2).zip(zs.windows(2)).map(|(p, z)| (p[0], z[0], p[1], z[1])).collect();
    if closed && let (Some(&a), Some(&b), Some(&za), Some(&zb)) = (pts.last(), pts.first(), zs.last(), zs.first()) {
        segs.push((a, za, b, zb));
    }
    let mut out: Vec<Line> = Vec::new();
    let mut run: Option<Line> = None;
    for (a, za, b, zb) in segs {
        let d = b - a;
        let mut cuts = vec![0.0, 1.0];
        for (c, e) in geom::ring_edges(ring) {
            let den = d.cross(e - c);
            if den.abs() <= 1e-12 * d.length() * (e - c).length() {
                continue;
            }
            let (t, u) = ((c - a).cross(e - c) / den, (c - a).cross(d) / den);
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                cuts.push(t);
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|x, y| (*x - *y).abs() * d.length() < TOL);
        for w in cuts.windows(2) {
            let (p, q) = (a + d * w[0], a + d * w[1]);
            let (zp, zq) = (za + (zb - za) * w[0], za + (zb - za) * w[1]);
            if geom::contains(ring, (p + q) * 0.5) {
                match run.as_mut().filter(|r| r.points.last().is_some_and(|l| l.dist(p) < TOL)) {
                    Some(r) => {
                        r.points.push(q);
                        r.z.push(zq);
                    }
                    None => {
                        out.extend(run.take());
                        run = Some(Line { points: vec![p, q], z: vec![zp, zq], closed: false });
                    }
                }
            } else {
                out.extend(run.take());
            }
        }
    }
    out.extend(run);
    out
}

/// A surface made inside `outline` (the convex hull of its points when none is given).
fn build_in(points: &[[f64; 3]], lines: &[Line], options: &Options, outline: Option<&[P2]>, cap: usize) -> Result<Surface, String> {
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
    // Contours as segments, with their elevation, and where their vertices are: what flat triangles are found by.
    let mut contour_segs: Vec<(P2, P2, f64)> = Vec::new();
    let mut contour_keys = std::collections::BTreeSet::new();
    let mut rings: Vec<(usize, f64)> = Vec::new();
    for (i, r) in ready.iter().enumerate().filter(|(_, r)| is_contour(r)) {
        let z = r.1.first().copied().unwrap_or(0.0);
        contour_segs.extend(r.0.windows(2).map(|w| (w[0], w[1], z)));
        if r.2
            && let (Some(&a), Some(&b)) = (r.0.last(), r.0.first())
        {
            contour_segs.push((a, b, z));
            rings.push((i, z));
        }
    }
    // A summit or a pit: a closed contour with nothing else inside it gets a point at its middle.
    let mut poles: Vec<(P2, f64)> = Vec::new();
    for &(i, z0) in &rings {
        let Some(ring) = ready.get(i).map(|r| &r.0) else { continue };
        // Another line's vertex or a spot height inside it: the data says what is there, so no pole is made up.
        let inner = ready.iter().enumerate().any(|(j, r)| j != i && r.0.iter().any(|&p| geom::contains(ring, p)))
            || points.iter().any(|&[x, y, _]| geom::contains(ring, P2(x, y)));
        let mid = geom::centroid(ring);
        if inner || !geom::contains(ring, mid) {
            continue;
        }
        if let Some(z) = lifted(mid, z0, &contour_segs) {
            poles.push((mid, z));
        }
    }
    let mut heights = Heights::default();
    let mut isolated: Vec<P2> = Vec::with_capacity(points.len());
    for &[x, y, z] in points {
        let p = P2(x, y);
        check_point(p, z)?;
        isolated.push(heights.add(p, z));
    }
    let mut segments: Vec<(P2, P2)> = Vec::new();
    let mut positions: Vec<P2> = isolated.clone();
    for (pts, zs, closed) in ready {
        let contour = zs.windows(2).all(|w| w[0] == w[1]);
        // Each vertex as the corner it is; two in a row that are one corner make no segment.
        let mut at_corners: Vec<P2> = Vec::with_capacity(pts.len());
        for (&p, &z) in pts.iter().zip(&zs) {
            let c = heights.add(p, z);
            if at_corners.last() != Some(&c) {
                at_corners.push(c);
            }
            if contour {
                contour_keys.insert(key(c));
            }
        }
        segments.extend(at_corners.windows(2).map(|w| (w[0], w[1])));
        if closed && let (Some(&a), Some(&b)) = (at_corners.last(), at_corners.first())
            && a != b
            && at_corners.len() > 2
        {
            segments.push((a, b));
        }
        positions.extend(at_corners);
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
    let outline = match outline {
        Some(o) => o.to_vec(),
        None => geom::hull(positions),
    };
    if outline.len() < 3 || geom::area(&outline).abs() < TOL * TOL {
        return Err("its points are all on one line, so they make no surface".into());
    }
    let mut added = 0;
    // Points added to flat ground stop where the triangulation's limit would be passed: a surface with some flat
    // triangles left is better than none.
    let mut skipped = 0;
    for (p, lift) in poles {
        if heights.len() >= cap {
            skipped += 1;
            continue;
        }
        if heights.near(p).is_none() {
            isolated.push(heights.add(p, lift));
            added += 1;
        }
    }
    let triangulate = |isolated: &[P2], heights: &Heights| -> Result<(triangulation::Triangulation, Vec<f64>), String> {
        let t = triangulation::constrained(&outline, &[], isolated, &segments)
            .map_err(|e| if e.contains("cross") { format!("contours or breaklines cross: {e}") } else { e })?;
        let z = t
            .points
            .iter()
            .map(|&p| heights.of(p).ok_or_else(|| format!("the triangulation made a corner at {} that no point was given at", at(p))))
            .collect::<Result<Vec<f64>, String>>()?;
        Ok((t, z))
    };
    let (mut t, mut z) = triangulate(&isolated, &heights)?;
    // Flat triangles: three corners on contours of one elevation. Each gets a point at its middle, lifted toward the
    // next contour, and the surface is triangulated again, up to three times.
    for _ in 0..3 {
        let mut more = Vec::new();
        for tri in &t.triangles {
            let (Some(&a), Some(&b), Some(&c)) = (t.points.get(tri[0]), t.points.get(tri[1]), t.points.get(tri[2])) else {
                continue;
            };
            let (Some(&za), Some(&zb), Some(&zc)) = (z.get(tri[0]), z.get(tri[1]), z.get(tri[2])) else { continue };
            if za != zb || zb != zc || ![a, b, c].iter().all(|&p| near_key(&contour_keys, p)) {
                continue;
            }
            let mid = P2((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
            if let Some(lift) = lifted(mid, za, &contour_segs) {
                more.push((mid, lift));
            }
        }
        let before = added;
        for (p, lift) in more {
            if heights.len() >= cap {
                skipped += 1;
                continue;
            }
            if heights.near(p).is_none() {
                isolated.push(heights.add(p, lift));
                added += 1;
            }
        }
        if added == before {
            break;
        }
        (t, z) = triangulate(&isolated, &heights)?;
    }
    if added > 0 {
        notes.push(format!("{added} points added where contours made flat ground at summits, pits and valleys"));
    }
    if skipped > 0 {
        notes.push(format!(
            "{skipped} more points would have been added to flat ground, but the surface reached the limit of {cap} corners"
        ));
    }
    Ok(Surface { points: t.points, z, triangles: t.triangles, notes })
}

/// Contour segments joined into polylines: each segment is keyed by the two triangle edges it ends on, and segments
/// sharing an edge meet there. Open chains first, from an edge only one segment ends on; then the closed ones.
type Ends = BTreeMap<(usize, usize), Vec<((usize, usize), P2, P2)>>;

fn joined(mut ends: Ends) -> Vec<Vec<P2>> {
    let mut lines = Vec::new();
    let take = |ends: &mut Ends, from: (usize, usize)| -> Option<((usize, usize), P2, P2)> {
        let next = ends.get_mut(&from)?.pop()?;
        // The same segment, seen from its other end, goes too.
        if let Some(back) = ends.get_mut(&next.0)
            && let Some(i) = back.iter().position(|(k, _, _)| *k == from)
        {
            back.swap_remove(i);
        }
        Some(next)
    };
    let walk = |ends: &mut Ends, start: (usize, usize)| -> Vec<P2> {
        let mut line: Vec<P2> = Vec::new();
        let mut at = start;
        while let Some((next, p, q)) = take(ends, at) {
            if line.last().is_none_or(|l: &P2| l.dist(p) > 1e-9) {
                line.push(p);
            }
            if line.last().is_none_or(|l: &P2| l.dist(q) > 1e-9) {
                line.push(q);
            }
            at = next;
        }
        line
    };
    let starts: Vec<(usize, usize)> = ends.iter().filter(|(_, v)| v.len() == 1).map(|(k, _)| *k).collect();
    for start in starts {
        if ends.get(&start).is_some_and(|v| v.len() == 1) {
            let line = walk(&mut ends, start);
            if line.len() > 1 {
                lines.push(line);
            }
        }
    }
    while let Some(start) = ends.iter().find(|(_, v)| !v.is_empty()).map(|(k, _)| *k) {
        let line = walk(&mut ends, start);
        if line.len() > 1 {
            lines.push(line);
        }
    }
    lines
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

    /// Its contours: for each multiple of `interval` (mm) within its range, the polylines where it has that elevation, in
    /// order of elevation, each joined across triangles (a closed one ends where it starts). A corner exactly at a level
    /// counts as above it, so a level along triangles' edges (a given contour's) is drawn once. Refused: an interval not
    /// finite or not above 0, and one making more than 10 000 levels.
    pub fn contours(&self, interval: f64) -> Result<Vec<(f64, Vec<Vec<P2>>)>, String> {
        if !(interval.is_finite() && interval > 0.0) {
            return Err(format!("a contour interval is a length above 0, mm, not {interval}"));
        }
        let Some((lo, hi)) = self.range() else { return Ok(Vec::new()) };
        let (first, last) = ((lo / interval).ceil(), (hi / interval).floor());
        if last - first + 1.0 > 10_000.0 {
            return Err(format!("an interval of {interval} mm makes more than 10 000 contours over {} mm", hi - lo));
        }
        let mut out = Vec::new();
        let mut k = first;
        while k <= last {
            let level = k * interval;
            k += 1.0;
            // Each triangle crossing the level gives a segment between two of its edges, keyed by their corners.
            let mut ends: BTreeMap<(usize, usize), Vec<((usize, usize), P2, P2)>> = BTreeMap::new();
            for t in &self.triangles {
                let Some((_, zs)) = self.corners(t) else { continue };
                let above = zs.map(|z| z >= level);
                if above.iter().all(|&a| a) || above.iter().all(|&a| !a) {
                    continue;
                }
                let mut cuts = Vec::with_capacity(2);
                for i in 0..3 {
                    let (a, b) = (t[i], t[(i + 1) % 3]);
                    if above[i] == above[(i + 1) % 3] {
                        continue;
                    }
                    let (Some(&pa), Some(&pb), Some(&za), Some(&zb)) =
                        (self.points.get(a), self.points.get(b), self.z.get(a), self.z.get(b))
                    else {
                        continue;
                    };
                    let p = pa + (pb - pa) * ((level - za) / (zb - za));
                    cuts.push(((a.min(b), a.max(b)), p));
                }
                if let [(ka, pa), (kb, pb)] = cuts[..] {
                    ends.entry(ka).or_default().push((kb, pa, pb));
                    ends.entry(kb).or_default().push((ka, pb, pa));
                }
            }
            let lines = joined(ends);
            if !lines.is_empty() {
                out.push((level, lines));
            }
        }
        Ok(out)
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
    fn ends_closer_than_a_millimetre_merge_without_failing() {
        // B's segment is 1.6 mm long, but both its ends lie within a millimetre of A's end, which comes first: as one
        // corner, B is nothing, and the surface is made without it rather than refused.
        let lines = vec![
            Line::contour(100.0, vec![P2(0.0, 0.0), P2(1_000.0, 0.0)], false),
            Line::contour(100.0, vec![P2(1_000.5, 0.8), P2(1_000.5, -0.8)], false),
        ];
        let s = build(&[[0.0, 1_000.0, 200.0], [1_000.0, 1_000.0, 200.0]], &lines, &exact()).unwrap();
        assert!((s.elevation_at(P2(500.0, 0.0)).unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn positions_within_a_millimetre_meet_at_their_mean_and_say_so() {
        // 0.2 mm apart, so one corner, but rounding would key them apart: their elevations still meet, and are noted.
        let pts = [[500.4, 500.0, 100.0], [500.6, 500.0, 200.0], [0.0, 0.0, 0.0], [1_000.0, 0.0, 0.0], [500.0, 1_000.0, 0.0]];
        let s = build(&pts, &[], &exact()).unwrap();
        assert!(s.notes.iter().any(|n| n.contains("two elevations")), "{:?}", s.notes);
        let z = s.points.iter().zip(&s.z).find(|(p, _)| p.dist(P2(500.5, 500.0)) < 1.0).map(|(_, z)| *z).unwrap();
        assert!((z - 150.0).abs() < 1e-6, "{z}");
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
        // A dips 45 mm at x = 500, which simplifying at 50 mm would straighten; B, 30 mm above, dips 60 mm (kept) into
        // where A's straightened line would run. Apart they never cross; A keeps its dip.
        let a = Line::contour(100.0, vec![P2(0.0, 0.0), P2(400.0, 0.0), P2(500.0, -45.0), P2(600.0, 0.0), P2(1_000.0, 0.0)], false);
        let b = Line::contour(200.0, vec![P2(0.0, 30.0), P2(400.0, 30.0), P2(500.0, -30.0), P2(600.0, 30.0), P2(1_000.0, 30.0)], false);
        let corners = vec![[0.0, -1_000.0, 0.0], [1_000.0, -1_000.0, 0.0], [1_000.0, 1_000.0, 300.0], [0.0, 1_000.0, 300.0]];
        let s = build(&corners, &[a.clone(), b.clone()], &Options { simplify: 50.0, ..Options::default() }).unwrap();
        assert!(s.notes.iter().any(|n| n.contains("kept every vertex")), "{:?}", s.notes);
        assert!(s.notes.iter().all(|n| !n.contains("kept as points")), "{:?}", s.notes);
        assert!(crosses_none(&s, &[a, b]));
        assert!((s.elevation_at(P2(500.0, -45.0)).unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn simplifying_a_dense_arc_stays_within_its_tolerance() {
        // A quarter of a 100 m circle, a vertex every 100 mm: every original vertex must stay within `simplify` of the
        // line that replaces it, however many are left out in a row.
        let r = 100_000.0;
        let n = (r * std::f64::consts::FRAC_PI_2 / 100.0) as usize;
        let arc: Vec<P2> = (0..=n).map(|i| {
            let a = std::f64::consts::FRAC_PI_2 * i as f64 / n as f64;
            P2(r * a.cos(), r * a.sin())
        }).collect();
        let corners = vec![[0.0, 0.0, 0.0], [120_000.0, 0.0, 0.0], [120_000.0, 120_000.0, 0.0], [0.0, 120_000.0, 0.0]];
        let s = build(&corners, &[Line::contour(100.0, arc.clone(), false)], &Options { simplify: 50.0, ..Options::default() }).unwrap();
        // The kept vertices: the surface's corners at elevation 100.
        let mut kept: Vec<P2> = s.points.iter().zip(&s.z).filter(|(_, z)| **z == 100.0).map(|(p, _)| *p).collect();
        kept.sort_by(|a, b| a.1.atan2(a.0).total_cmp(&b.1.atan2(b.0)));
        assert!(kept.len() < arc.len() / 4, "{} of {}", kept.len(), arc.len());
        let worst = arc.iter().map(|&p| kept.windows(2).map(|w| geom::dist_to_segment(p, w[0], w[1])).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
        assert!(worst <= 50.0 + 1e-6, "{worst}");
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
        // Kept, each loop is a summit: its middle rises past 5000.
        let kept = build(&plane_corners(), &lines, &Options::default()).unwrap();
        assert!(kept.range().is_some_and(|(lo, hi)| lo == 100.0 && hi > 5000.0), "{:?}", kept.range());
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

    /// A cone of apex 5000 at the origin, drawn by 64-gon contours of radius 40, 32, 24, 16 and 8 m at 0 to 4000 (`sign` −1
    /// for a pit).
    fn cone(sign: f64) -> Vec<Line> {
        (0..5)
            .map(|k| {
                let r = 40_000.0 - 8_000.0 * k as f64;
                let pts = (0..64).map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 64.0;
                    P2(r * a.cos(), r * a.sin())
                });
                Line::contour(sign * 1000.0 * k as f64, pts.collect(), true)
            })
            .collect()
    }

    fn exact() -> Options {
        Options { simplify: 0.0, ..Options::default() }
    }

    fn flat(s: &Surface) -> usize {
        s.triangles.iter().filter(|t| s.z[t[0]] == s.z[t[1]] && s.z[t[1]] == s.z[t[2]]).count()
    }

    #[test]
    fn a_cones_summit_rises_and_no_triangle_is_flat() {
        let s = build(&[], &cone(1.0), &exact()).unwrap();
        assert_eq!(flat(&s), 0);
        let top = s.elevation_at(P2(0.0, 0.0)).unwrap();
        assert!((top - 5000.0).abs() < 100.0, "{top}");
        // The 64-gon's pyramid: a third of its base's area times its height.
        let base = 0.5 * 64.0 * 40_000.0f64.powi(2) * (std::f64::consts::TAU / 64.0).sin();
        let want = base * 5000.0 / 3.0;
        let got = s.volume_above(0.0);
        assert!((got - want).abs() / want < 0.005, "{got} {want}");
        assert!(s.notes.iter().any(|n| n.contains("points added")), "{:?}", s.notes);
    }

    #[test]
    fn a_spot_height_inside_a_summit_is_the_summit() {
        // A surveyed spot height inside the top ring is the summit: no second peak is made beside it.
        let s = build(&[[2_000.0, 1_000.0, 4_800.0]], &cone(1.0), &exact()).unwrap();
        assert!(s.points.iter().all(|p| p.dist(P2(0.0, 0.0)) > 1.0), "a pole was added at the ring's middle");
        assert!((s.elevation_at(P2(2_000.0, 1_000.0)).unwrap() - 4_800.0).abs() < 1e-6);
    }

    #[test]
    fn added_points_stop_at_the_triangulations_limit() {
        // The cone's 320 contour vertices and no room for its summit's point: the surface is still made, with what fits,
        // and says so.
        let s = build_in(&[], &cone(1.0), &exact(), None, 320).unwrap();
        assert_eq!(s.points.len(), 320);
        assert!(s.notes.iter().any(|n| n.contains("limit")), "{:?}", s.notes);
    }

    #[test]
    fn a_pits_bottom_sinks() {
        let s = build(&[], &cone(-1.0), &exact()).unwrap();
        assert_eq!(flat(&s), 0);
        let bottom = s.elevation_at(P2(0.0, 0.0)).unwrap();
        assert!((bottom + 5000.0).abs() < 100.0, "{bottom}");
    }

    #[test]
    fn a_valleys_tongue_slopes_down_to_the_next_contour() {
        // A valley's head: a U at 1000 open toward +x, 4 m wide and 20 m long, a vertex every metre; beyond its mouth the
        // contour at 0; behind it, ground at 1000.
        let mut u: Vec<P2> = (0..=20).rev().map(|i| P2(i as f64 * 1000.0, -2_000.0)).collect();
        u.extend((0..=20).map(|i| P2(i as f64 * 1000.0, 2_000.0)));
        let lines = vec![Line::contour(1000.0, u, false), Line::contour(0.0, vec![P2(30_000.0, -10_000.0), P2(30_000.0, 10_000.0)], false)];
        let corners = [[-10_000.0, -10_000.0, 1000.0], [-10_000.0, 10_000.0, 1000.0]];
        let s = build(&corners, &lines, &exact()).unwrap();
        let inside: Vec<(f64, f64)> =
            s.points.iter().zip(&s.z).filter(|(p, _)| p.0 > 1.0 && p.0 < 19_999.0 && p.1.abs() < 1_999.0).map(|(p, z)| (p.0, *z)).collect();
        assert!(!inside.is_empty());
        assert!(inside.iter().all(|&(_, z)| z > 0.0 && z < 1000.0), "{inside:?}");
        let mean = |f: &dyn Fn(f64) -> bool| {
            let v: Vec<f64> = inside.iter().filter(|(x, _)| f(*x)).map(|(_, z)| *z).collect();
            v.iter().sum::<f64>() / v.len().max(1) as f64
        };
        assert!(mean(&|x| x > 10_000.0) < mean(&|x| x < 10_000.0), "{inside:?}");
    }

    #[test]
    fn a_summit_never_rises_past_the_next_contour_interval() {
        // A ring at 100 with one at 0 just 1.5 mm outside it, as noise draws them: extrapolating the slope between them
        // would put the middle tens of metres up. Ground inside a contour stays within one interval of it, or another
        // contour would have been drawn.
        let ring = |r: f64, z: f64| {
            Line::contour(
                z,
                (0..32)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / 32.0;
                        P2(r * a.cos(), r * a.sin())
                    })
                    .collect(),
                true,
            )
        };
        let s = build(&[], &[ring(1_000.0, 100.0), ring(1_001.5, 0.0)], &exact()).unwrap();
        let (lo, hi) = s.range().unwrap();
        assert!(lo >= 0.0 && hi <= 200.0, "{lo} {hi}");
    }

    #[test]
    fn a_surface_with_no_flat_triangle_is_left_as_it_is() {
        let s = plane_surface();
        assert!(s.notes.iter().all(|n| !n.contains("added")), "{:?}", s.notes);
    }

    #[test]
    fn flat_ground_made_of_points_is_left_flat() {
        // Points all at one elevation are flat ground, not a contour's artefact.
        let s = build(&[[0.0, 0.0, 7.0], [1_000.0, 0.0, 7.0], [1_000.0, 1_000.0, 7.0], [0.0, 1_000.0, 7.0]], &[], &exact()).unwrap();
        assert_eq!(s.points.len(), 4);
    }

    fn square(lo: f64, hi: f64) -> Vec<P2> {
        vec![P2(lo, lo), P2(hi, lo), P2(hi, hi), P2(lo, hi)]
    }

    #[test]
    fn a_boundary_clips_the_surface() {
        let options = Options { boundary: Some(square(5_000.0, 15_000.0)), ..Options::default() };
        let s = build(&plane_corners(), &plane_contours(), &options).unwrap();
        assert!((s.area() - 1e8).abs() < 1e-3, "{}", s.area());
        assert_eq!(s.elevation_at(P2(4_000.0, 10_000.0)), None);
        for at in [P2(10_000.0, 10_000.0), P2(5_000.0, 5_000.0), P2(15_000.0, 15_000.0), P2(5_001.0, 14_999.0)] {
            let z = s.elevation_at(at).unwrap();
            assert!((z - plane(at)).abs() < 1e-6, "{at:?}: {z}");
        }
        assert!(s.notes.iter().any(|n| n.contains("outside the boundary")), "{:?}", s.notes);
    }

    #[test]
    fn a_boundary_must_be_a_simple_ring_inside_the_data() {
        let bow_tie = vec![P2(5_000.0, 5_000.0), P2(15_000.0, 15_000.0), P2(15_000.0, 5_000.0), P2(5_000.0, 15_000.0)];
        let e = build(&plane_corners(), &plane_contours(), &Options { boundary: Some(bow_tie), ..Options::default() }).unwrap_err();
        assert!(e.contains("crosses itself"), "{e}");
        let past = build(&plane_corners(), &plane_contours(), &Options { boundary: Some(square(5_000.0, 25_000.0)), ..Options::default() })
            .unwrap_err();
        assert!(past.contains("past the survey") && past.contains("(25000, 5000)"), "{past}");
        let line = vec![P2(0.0, 0.0), P2(1_000.0, 0.0), P2(2_000.0, 0.0)];
        assert!(build(&plane_corners(), &plane_contours(), &Options { boundary: Some(line), ..Options::default() }).is_err());
    }

    #[test]
    fn a_planes_contours_are_its_straight_lines() {
        let s = plane_surface();
        let c = s.contours(500.0).unwrap();
        let levels: Vec<f64> = c.iter().map(|(z, _)| *z).collect();
        assert_eq!(levels, [500.0, 1000.0, 1500.0, 2000.0, 2500.0, 3000.0]);
        let on_side = |p: P2| [p.0, p.1].iter().any(|&v| v.abs() < 1e-6 || (v - SIDE).abs() < 1e-6);
        for (z, lines) in &c {
            assert_eq!(lines.len(), 1, "{z}: {lines:?}");
            let line = &lines[0];
            assert!(line.iter().all(|&p| (plane(p) - z).abs() < 1e-6), "{z}: {line:?}");
            assert!(line.first().is_some_and(|&p| on_side(p)) && line.last().is_some_and(|&p| on_side(p)), "{z}: {line:?}");
        }
        // A level along the given contours' own edges is drawn once, not twice.
        let fine = s.contours(100.0).unwrap();
        let at_600 = fine.iter().find(|(z, _)| *z == 600.0).map(|(_, l)| l.clone()).unwrap();
        assert_eq!(at_600.len(), 1, "{at_600:?}");
    }

    #[test]
    fn a_cones_contours_are_closed_rings_at_their_radii() {
        let s = build(&[], &cone(1.0), &exact()).unwrap();
        let c = s.contours(1000.0).unwrap();
        for (k, r) in [(1, 32_000.0), (2, 24_000.0), (3, 16_000.0), (4, 8_000.0)] {
            let level = 1000.0 * k as f64;
            let lines = &c.iter().find(|(z, _)| *z == level).unwrap_or_else(|| panic!("no {level}")).1;
            assert_eq!(lines.len(), 1, "{level}: {} lines", lines.len());
            let ring = &lines[0];
            assert!(ring.first().zip(ring.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-6), "{level} isn't closed");
            let chord = r * (1.0 - (std::f64::consts::PI / 64.0).cos());
            assert!(ring.iter().all(|p| (p.length() - r).abs() <= chord + 1e-6), "{level}");
        }
    }

    #[test]
    fn bad_intervals_are_refused() {
        let s = plane_surface();
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.001] {
            assert!(s.contours(bad).is_err(), "{bad}");
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
