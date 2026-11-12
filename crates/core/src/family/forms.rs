//! F6: Revit's Blend, Sweep and Swept Blend, and their voids. A form's `sketch` is always drawn in its host plane's axes: an
//! extrusion's or a revolve's profile, a Blend's base, a Sweep's or a Swept Blend's path. A Blend adds its `top` (the same
//! plane's axes, at `to` along its normal, the base at `from`); a Sweep adds its `sweep.profile` and a Swept Blend its
//! `sweep.profile` and `sweep.end`, drawn in the profile plane's axes: x to the right of someone looking along the path, y
//! along the host's normal, origin on the path. A Sweep's profile plane is shown at the middle of the path's first line
//! (Revit's default profile plane location); a Swept Blend's two at the path's start and end.
//!
//! Built here into triangles, every one turned out (by the solid's signed volume, whatever way it was drawn), as closed
//! solids: a Blend's sides join its base's and top's vertices as [`mapped`] pairs them (Revit's default vertex connection,
//! turned by `twist`, Edit Vertices' Twist); a Sweep's profile is carried along the path, mitred at each bend, capped at
//! an open path's ends; a Swept Blend's profile turns from the first into the second along its one line or arc.

use serde::{Deserialize, Serialize};

use super::sketch::{FormLine, MAX_SKETCH, SEGMENTS};
use super::{Expr, Form, Frame};
use crate::geom::{P2, Region};
use crate::model::TOL;
use crate::sketch::SketchLine;
use crate::view3d::{P3, add, cross, dot, mul, sub};

/// Points a built ring or path may have: past it, a form is refused rather than built.
pub const MAX_RING: usize = 4096;

/// What a form is, by what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormKind {
    Extrusion,
    Revolve,
    Blend,
    Sweep,
    SweptBlend,
}

impl FormKind {
    /// Revit's name for it.
    pub fn name(self) -> &'static str {
        match self {
            FormKind::Extrusion => "Extrusion",
            FormKind::Revolve => "Revolve",
            FormKind::Blend => "Blend",
            FormKind::Sweep => "Sweep",
            FormKind::SweptBlend => "Swept Blend",
        }
    }

    /// Whether its `sketch` is a path a profile is swept along.
    pub fn swept(self) -> bool {
        matches!(self, FormKind::Sweep | FormKind::SweptBlend)
    }
}

/// A Sweep's profile, or a Swept Blend's two: drawn in the profile plane's axes (x to the right looking along the path, y
/// along the host's normal), square to the path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Swept {
    /// The profile; a Swept Blend's first, at its path's start.
    #[serde(default)]
    pub profile: Vec<FormLine>,
    /// A Swept Blend's second profile, at its path's end; none, a Sweep.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub end: Vec<FormLine>,
}

impl Form {
    /// What it is: an extrusion, a revolve (its sketch has an axis), a Blend (it has a `top`), a Sweep or a Swept Blend (it
    /// has a `sweep`, a Swept Blend a second profile).
    pub fn kind(&self) -> FormKind {
        match (&self.sweep, self.top.is_empty(), self.revolves()) {
            (Some(s), _, _) if s.end.is_empty() => FormKind::Sweep,
            (Some(_), _, _) => FormKind::SweptBlend,
            (None, false, _) => FormKind::Blend,
            (None, true, true) => FormKind::Revolve,
            (None, true, false) => FormKind::Extrusion,
        }
    }

    /// Every line it holds: its sketch, its top, its profiles, in that order.
    pub fn all_lines(&self) -> impl Iterator<Item = &FormLine> {
        let sweep = self.sweep.iter().flat_map(|s| s.profile.iter().chain(&s.end));
        self.sketch.iter().chain(&self.top).chain(sweep)
    }

    /// [`Form::all_lines`], to change.
    pub fn all_lines_mut(&mut self) -> impl Iterator<Item = &mut FormLine> {
        let sweep = self.sweep.iter_mut().flat_map(|s| s.profile.iter_mut().chain(s.end.iter_mut()));
        self.sketch.iter_mut().chain(self.top.iter_mut()).chain(sweep)
    }

    /// The lines of its sketch on its host plane a constraint may measure: its sketch (a Blend's base, a sweep's path) and a
    /// Blend's top. Its profiles are in a plane of their own, and aren't measured (F6d).
    pub fn plane_lines(&self) -> impl Iterator<Item = &FormLine> {
        self.sketch.iter().chain(&self.top)
    }

    /// Line `k` of [`Form::plane_lines`], to change.
    pub fn plane_line_mut(&mut self, k: usize) -> Option<&mut FormLine> {
        let n = self.sketch.len();
        if k < n { self.sketch.get_mut(k) } else { self.top.get_mut(k - n) }
    }

    /// Line `k` of [`Form::plane_lines`].
    pub fn plane_line(&self, k: usize) -> Option<&FormLine> {
        self.plane_lines().nth(k)
    }

    /// Which group line `k` of [`Form::plane_lines`] is in: 0 its sketch, 1 its top. Vertices join within a group alone.
    pub fn plane_group(&self, k: usize) -> usize {
        usize::from(k >= self.sketch.len())
    }
}

/// Lines as straight pieces, an arc's [`SEGMENTS`] to a whole turn (`sketch::chorded`).
fn drawn<'a>(lines: &'a [FormLine], num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Result<Vec<SketchLine>, String> {
    lines.iter().map(|l| Ok(super::sketch::chorded(&l.curve(num)?))).collect()
}

/// The loops a profile's `lines` make, read by `num`, as a sketch's are ([`Form::loops`]): each outline counter-clockwise
/// with its holes clockwise. `what` names the profile in what is said.
pub fn profile_loops<'a>(
    lines: &'a [FormLine],
    what: &str,
    num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>,
) -> Result<Vec<Region>, String> {
    if lines.is_empty() {
        return Err(format!("its {what} has no lines: draw it"));
    }
    if lines.len() > MAX_SKETCH {
        return Err(format!("its {what} has {} lines, more than {MAX_SKETCH}", lines.len()));
    }
    if lines.iter().any(|l| l.axis) {
        return Err(format!("its {what} has an axis line: only a revolve's sketch has one"));
    }
    let lines_drawn = drawn(lines, num).map_err(|e| format!("its {what}: {e}"))?;
    let solved = crate::sketch::solve(&lines_drawn).map_err(|problems| {
        let named: Vec<SketchLine> = lines.iter().map(|l| SketchLine { id: l.id, ..SketchLine::new(P2(0.0, 0.0), P2(1.0, 0.0)) }).collect();
        format!("its {what}: {}", crate::sketch::say(&problems, &crate::sketch::namer(&named)))
    })?;
    let ccw = |ring: Vec<P2>, want: bool| if (crate::geom::area(&ring) > 0.0) == want { ring } else { ring.into_iter().rev().collect() };
    let regions: Vec<Region> = solved
        .regions
        .into_iter()
        .map(|r| Region { outer: ccw(r.outer, true), holes: r.holes.into_iter().map(|h| ccw(h, false)).collect() })
        .collect();
    if regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).map(Vec::len).sum::<usize>() > MAX_RING {
        return Err(format!("its {what} has more than {MAX_RING} corners"));
    }
    Ok(regions)
}

/// The one loop a Blend's end or a Swept Blend's profile makes, counter-clockwise: refused for several or a hole, as Revit's.
pub fn one_loop<'a>(lines: &'a [FormLine], what: &str, num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Result<Vec<P2>, String> {
    let regions = profile_loops(lines, what, num)?;
    match regions.as_slice() {
        [one] if one.holes.is_empty() => Ok(one.outer.clone()),
        _ => Err(format!(
            "its {what} makes {} loops (or a hole): a blend's ends and a swept blend's profiles are one loop each",
            regions.len().max(2)
        )),
    }
}

/// A path, chained: its points in turn (a closed one's first not repeated at its end), whether it closes, and where its
/// profile plane is (Sweep: the middle of its first line drawn; Swept Blend: its start), with the way it runs there.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub points: Vec<P2>,
    pub closed: bool,
    pub at: P2,
    pub way: P2,
    /// The way it runs at its last point: a Swept Blend's end profile plane.
    pub end_way: P2,
}

/// `lines` (a sweep's sketch) chained end to end into one path, read by `num`: refused, saying why, where they make no
/// single chain (a gap, a branch, two pieces), where a line has no length, or where it crosses itself.
pub fn path<'a>(lines: &'a [FormLine], num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Result<Path, String> {
    if lines.is_empty() {
        return Err("its path has no lines: draw it (Sketch Path) or pick it (Pick Path)".into());
    }
    if lines.len() > MAX_SKETCH {
        return Err(format!("its path has {} lines, more than {MAX_SKETCH}", lines.len()));
    }
    if lines.iter().any(|l| l.axis) {
        return Err("its path has an axis line: only a revolve's sketch has one".into());
    }
    let curves: Vec<SketchLine> = lines.iter().map(|l| l.curve(num)).collect::<Result<_, _>>().map_err(|e| format!("its path: {e}"))?;
    // A lone circle: a closed path round it.
    if let [one] = curves.as_slice()
        && let crate::curves::Curve::Circle { centre, radius } = &one.curve
    {
        let points: Vec<P2> = (0..SEGMENTS)
            .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
            .map(|(s, c)| P2(centre.0 + radius * c, centre.1 + radius * s))
            .collect();
        let way = (points[1] - points[0]).unit();
        let path = Path { at: (points[0] + points[1]) * 0.5, way, end_way: way, points, closed: true };
        return checked(path);
    }
    if curves.iter().any(|c| matches!(c.curve, crate::curves::Curve::Circle { .. })) {
        return Err("its path has a circle among other lines: a path is one chain of lines and arcs, or one circle".into());
    }
    // Each line's chords, start to end.
    let pieces: Vec<Vec<P2>> = curves.iter().map(|c| super::sketch::chorded(c).curve.points()).collect();
    for (k, p) in pieces.iter().enumerate() {
        let len: f64 = p.windows(2).map(|w| w[0].dist(w[1])).sum();
        if p.len() < 2 || len <= TOL {
            return Err(format!("its path's line {} has no length", k + 1));
        }
    }
    // Ends joined within TOL into vertices; each line one edge between two.
    let mut verts: Vec<P2> = Vec::new();
    let mut vertex = |p: P2| match verts.iter().position(|v| v.dist(p) <= TOL) {
        Some(i) => i,
        None => {
            verts.push(p);
            verts.len() - 1
        }
    };
    let edges: Vec<(usize, usize)> = pieces.iter().map(|p| (vertex(p[0]), vertex(p[p.len() - 1]))).collect();
    if edges.iter().any(|(a, b)| a == b) {
        return Err("its path has a line that ends where it starts: draw a circle for a closed round path".into());
    }
    let mut degree = vec![0usize; verts.len()];
    for (a, b) in &edges {
        degree[*a] += 1;
        degree[*b] += 1;
    }
    if let Some(v) = degree.iter().position(|d| *d > 2) {
        let p = verts[v];
        return Err(format!(
            "its path branches at ({:.0}, {:.0}): a path is one chain, each line joined to at most one at each end",
            p.0, p.1
        ));
    }
    let ends: Vec<usize> = (0..verts.len()).filter(|v| degree[*v] == 1).collect();
    let closed = ends.is_empty();
    if !closed && ends.len() != 2 {
        return Err("its path is in pieces: join its lines end to end into one chain".into());
    }
    // Walk from an end (closed: line 1's start), every line once.
    let start = if closed { edges[0].0 } else { ends[0] };
    let mut used = vec![false; edges.len()];
    let mut order: Vec<(usize, bool)> = Vec::new();
    let mut at = start;
    while let Some(k) = (0..edges.len()).find(|k| !used[*k] && (edges[*k].0 == at || edges[*k].1 == at)) {
        used[k] = true;
        let forward = edges[k].0 == at;
        order.push((k, forward));
        at = if forward { edges[k].1 } else { edges[k].0 };
        if closed && order.len() > 1 && at == start {
            break;
        }
    }
    if used.iter().any(|u| !u) {
        return Err("its path is in pieces: join its lines end to end into one chain".into());
    }
    // Line 1 as drawn sets the way: walked backwards, the whole path is turned round.
    if order.iter().find(|(k, _)| *k == 0).is_some_and(|(_, f)| !*f) {
        order.reverse();
        order.iter_mut().for_each(|o| o.1 = !o.1);
    }
    let mut points: Vec<P2> = Vec::new();
    let mut first: Option<(P2, P2)> = None;
    for (k, forward) in &order {
        let mut p = pieces[*k].clone();
        if !forward {
            p.reverse();
        }
        if *k == 0 {
            let len: f64 = p.windows(2).map(|w| w[0].dist(w[1])).sum();
            first = Some(along(&p, len / 2.0));
        }
        let skip = usize::from(!points.is_empty());
        points.extend(p.into_iter().skip(skip));
        if points.len() > MAX_RING {
            return Err(format!("its path has more than {MAX_RING} points"));
        }
    }
    if closed {
        points.pop();
    }
    let (at, way) = first.unwrap_or((points[0], P2(1.0, 0.0)));
    let n = points.len();
    let end_way = if closed { (points[1] - points[0]).unit() } else { (points[n - 1] - points[n - 2]).unit() };
    checked(Path { points, closed, at, way, end_way })
}

/// The point `d` along polyline `p` and the way it runs there.
fn along(p: &[P2], d: f64) -> (P2, P2) {
    let mut left = d;
    for w in p.windows(2) {
        let len = w[0].dist(w[1]);
        if left <= len || len <= 0.0 {
            let t = if len > 0.0 { left / len } else { 0.0 };
            return (w[0] + (w[1] - w[0]) * t, (w[1] - w[0]).unit());
        }
        left -= len;
    }
    let n = p.len();
    (p[n - 1], (p[n - 1] - p[n - 2]).unit())
}

/// `path` refused where two of its segments not next to each other cross or touch.
fn checked(path: Path) -> Result<Path, String> {
    let p = &path.points;
    let n = p.len();
    let segs = if path.closed { n } else { n - 1 };
    if segs > MAX_RING {
        return Err(format!("its path has more than {MAX_RING} points"));
    }
    for i in 0..segs {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if a.dist(b) <= 1e-9 {
            return Err("its path has a segment of no length".into());
        }
        for j in i + 2..segs {
            if path.closed && i == 0 && j == segs - 1 {
                continue;
            }
            let (c, d) = (p[j], p[(j + 1) % n]);
            if touch(a, b, c, d) {
                return Err(format!("its path crosses itself near ({:.0}, {:.0})", a.0, a.1));
            }
        }
    }
    Ok(path)
}

/// Whether segments `ab` and `cd` cross or touch (within a micron).
fn touch(a: P2, b: P2, c: P2, d: P2) -> bool {
    let side = |p: P2, q: P2, r: P2| (q - p).cross(r - p);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    crate::geom::dist_to_segment(a, c, d) < 1e-3
        || crate::geom::dist_to_segment(b, c, d) < 1e-3
        || crate::geom::dist_to_segment(c, a, b) < 1e-3
        || crate::geom::dist_to_segment(d, a, b) < 1e-3
}

/// The plane a profile is drawn on where a path drawn on `host` runs `way` through `at`: x to the right looking along the
/// path, y the host's normal, origin on the path, its normal back along it.
pub fn profile_frame(host: &Frame, at: P2, way: P2) -> Frame {
    let x = host.dir(P2(way.1, -way.0));
    let y = host.z;
    Frame { origin: host.point(at, 0.0), x, y, z: cross(x, y) }
}

/// Each loop of `a` and `b` (counter-clockwise rings) with as many corners, paired in turn, as a Blend's sides join them:
/// each starts at its corner nearest the angle (about its middle, from +x) of `a`'s corner at the least angle, `b`'s moved
/// on by `twist`; rings of one count pair corner to corner, others are walked together by the shorter diagonal, a corner of
/// one repeated while the other's run past it (a fan), so every corner of each is kept and none is added.
pub fn mapped(a: &[P2], b: &[P2], twist: i32) -> (Vec<P2>, Vec<P2>) {
    // Each ring without a corner repeated (a loop solved may close on its first corner again).
    let single = |ring: &[P2]| -> Vec<P2> {
        let mut out: Vec<P2> = Vec::with_capacity(ring.len());
        for p in ring {
            if out.last().is_none_or(|q| q.dist(*p) > 1e-9) {
                out.push(*p);
            }
        }
        while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(f, l)| f.dist(*l) <= 1e-9) {
            out.pop();
        }
        out
    };
    let (a, b) = (&single(a)[..], &single(b)[..]);
    let angle = |ring: &[P2], p: P2| {
        let c = middle(ring);
        (p.1 - c.1).atan2(p.0 - c.0).rem_euclid(std::f64::consts::TAU)
    };
    let a0 = (0..a.len()).min_by(|i, j| angle(a, a[*i]).total_cmp(&angle(a, a[*j]))).unwrap_or(0);
    let want = a.get(a0).map_or(0.0, |p| angle(a, *p));
    let gap = |x: f64| {
        let d = (x - want).rem_euclid(std::f64::consts::TAU);
        d.min(std::f64::consts::TAU - d)
    };
    let b0 = (0..b.len()).min_by(|i, j| gap(angle(b, b[*i])).total_cmp(&gap(angle(b, b[*j])))).unwrap_or(0);
    let b0 = if b.is_empty() { 0 } else { (b0 as i64 + i64::from(twist)).rem_euclid(b.len() as i64) as usize };
    let ra: Vec<P2> = (0..a.len()).map(|k| a[(a0 + k) % a.len()]).collect();
    let rb: Vec<P2> = (0..b.len()).map(|k| b[(b0 + k) % b.len()]).collect();
    if ra.len() == rb.len() || ra.is_empty() || rb.is_empty() {
        return (ra, rb);
    }
    // Walked together, each step moving on round one of them: the one whose next corner is nearer, by its angle about its
    // own ring's middle, to the other's corner (the shortest diagonal, measured as turns rather than lengths, so a ring off
    // the other's middle, as a swept blend's round end low in its square start, is paired side to side and doesn't fold).
    // A corner of one is held while the other moves on, so a side joins a corner of one to a run of the other's (a fan),
    // and no corner is added to either.
    let tau = std::f64::consts::TAU;
    let turns = |ring: &[P2], from: f64| -> Vec<f64> {
        let mut out: Vec<f64> = ring.iter().map(|p| from + (angle(ring, *p) - from).rem_euclid(tau)).collect();
        // Its first corner is where it starts, whatever the rounding.
        if let Some(first) = out.first_mut() {
            *first = from;
        }
        out.push(from + tau);
        out
    };
    let alpha = angle(&ra, ra[0]);
    let beta = alpha + (angle(&rb, rb[0]) - alpha + std::f64::consts::PI).rem_euclid(tau) - std::f64::consts::PI;
    let (ta, tb) = (turns(&ra, alpha), turns(&rb, beta));
    let (n, m) = (ra.len(), rb.len());
    let (mut i, mut j) = (0, 0);
    let (mut oa, mut ob) = (vec![ra[0]], vec![rb[0]]);
    while i < n || j < m {
        let step_a = i < n && (j >= m || (ta[i + 1] - tb[j]).abs() <= (ta[i] - tb[j + 1]).abs());
        if step_a {
            i += 1;
        } else {
            j += 1;
        }
        if i >= n && j >= m {
            break;
        }
        let pair = (ra[i % n], rb[j % m]);
        // A pair again (both rings back at their first corners) closes the walk: no side between a pair and itself.
        if (pair.0.dist(ra[0]) <= 1e-9 && pair.1.dist(rb[0]) <= 1e-9)
            || oa.last().zip(ob.last()).is_some_and(|(x, y)| *x == pair.0 && *y == pair.1)
        {
            continue;
        }
        oa.push(pair.0);
        ob.push(pair.1);
    }
    (oa, ob)
}

/// A ring's middle: its corners' mean.
fn middle(ring: &[P2]) -> P2 {
    let n = ring.len().max(1) as f64;
    let s = ring.iter().fold(P2(0.0, 0.0), |s, p| s + *p);
    s * (1.0 / n)
}

/// A cap of closed `ring` (counter-clockwise), as triangles of its corners: cut into ears, or, where corners lie on a line
/// (a Blend's rings take each other's), by the constrained triangulation, its points taken back to the ring's.
fn cap(ring: &[P2]) -> Result<Vec<[usize; 3]>, String> {
    if let Ok(tris) = super::cap(ring) {
        return Ok(tris);
    }
    let t = crate::triangulation::constrained(ring, &[], &[], &[]).map_err(|e| format!("its profile can't be capped: {e}"))?;
    let nearest = |p: P2| (0..ring.len()).min_by(|i, j| ring[*i].dist(p).total_cmp(&ring[*j].dist(p))).unwrap_or(0);
    let index: Vec<usize> = t.points.iter().map(|p| nearest(*p)).collect();
    let tris: Vec<[usize; 3]> =
        t.triangles.iter().map(|[a, b, c]| [index[*a], index[*b], index[*c]]).filter(|[a, b, c]| a != b && b != c && a != c).collect();
    if tris.is_empty() {
        return Err("its profile crosses itself".into());
    }
    Ok(tris)
}

/// Triangles of a closed solid turned out by its signed volume; refused where it has none, or is too big.
fn closed(mut tris: Vec<[P3; 3]>) -> Result<Vec<[P3; 3]>, String> {
    if tris.len() > super::MAX_TRIANGLES {
        return Err(format!("it would be more than {} triangles", super::MAX_TRIANGLES));
    }
    if tris.iter().flatten().flatten().any(|x| !x.is_finite() || x.abs() > crate::model::MAX_COORD) {
        return Err("it reaches past where the model may go".into());
    }
    let volume: f64 = tris.iter().map(|[a, b, c]| dot(*a, cross(*b, *c))).sum::<f64>() / 6.0;
    if !volume.is_finite() || volume.abs() < 1.0 {
        return Err("it has no volume".into());
    }
    if volume < 0.0 {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok(tris)
}

/// Rings `a` (at `from`) and `b` (at `to`) of a Blend on `frame`, mapped ([`mapped`]): its triangles, turned out.
pub fn blend(a: &[P2], b: &[P2], (from, to): (f64, f64), twist: i32, frame: &Frame) -> Result<Vec<[P3; 3]>, String> {
    let (ra, rb) = mapped(a, b, twist);
    if ra.len() > MAX_RING {
        return Err(format!("its ends together have more than {MAX_RING} corners"));
    }
    let base: Vec<P3> = ra.iter().map(|p| frame.point(*p, from)).collect();
    let top: Vec<P3> = rb.iter().map(|p| frame.point(*p, to)).collect();
    let mut tris = Vec::new();
    for [x, y, z] in cap(&ra)? {
        tris.push([base[x], base[z], base[y]]);
    }
    for [x, y, z] in cap(&rb)? {
        tris.push([top[x], top[y], top[z]]);
    }
    bands(&mut tris, &[base, top], false);
    closed(tris.into_iter().filter(|t| area3(t) > 1e-6).collect())
}

/// Each quad between ring `k` and ring `k + 1` of `rings` (all of one count) as two triangles; `wrap`, the last back to the
/// first.
fn bands(tris: &mut Vec<[P3; 3]>, rings: &[Vec<P3>], wrap: bool) {
    let n = rings.len();
    let steps = if wrap { n } else { n.saturating_sub(1) };
    for k in 0..steps {
        let (r0, r1) = (&rings[k], &rings[(k + 1) % n]);
        let m = r0.len().min(r1.len());
        for i in 0..m {
            let j = (i + 1) % m;
            tris.push([r0[i], r0[j], r1[j]]);
            tris.push([r0[i], r1[j], r1[i]]);
        }
    }
}

fn area3([a, b, c]: &[P3; 3]) -> f64 {
    let n = cross(sub(*b, *a), sub(*c, *a));
    dot(n, n).sqrt() / 2.0
}

/// Where each point of a path's profile goes: its point, the way its x runs (mitred, so as long as it must be) and its y.
struct Station {
    at: P3,
    x: P3,
    y: P3,
}

/// The stations along `path` on `host`, every profile point's x reaching at most `reach` (to the left, to the right) of it:
/// at a bend, the profile mitred along the bisector of the ways either side; refused where it would fold over itself (the
/// mitre's set-backs either end of a segment longer than it).
fn stations(path: &Path, host: &Frame, reach: (f64, f64)) -> Result<Vec<Station>, String> {
    let p = &path.points;
    let n = p.len();
    let segs = if path.closed { n } else { n - 1 };
    let way = |i: usize| (p[(i + 1) % n] - p[i]).unit();
    let right = |d: P2| P2(d.1, -d.0);
    let mut out = Vec::with_capacity(n);
    let mut setback = vec![0.0_f64; n];
    for k in 0..n {
        let (d_in, d_out) = match (path.closed, k) {
            (true, _) => (way((k + n - 1) % n), way(k)),
            (false, 0) => (way(0), way(0)),
            (false, k) if k == n - 1 => (way(k - 1), way(k - 1)),
            (false, k) => (way(k - 1), way(k)),
        };
        let (r_in, r_out) = (right(d_in), right(d_out));
        let m = (r_in + r_out).unit();
        let c = m.dot(r_in);
        if c < 0.05 {
            return Err(format!("its path turns back on itself at ({:.0}, {:.0}): the profile can't follow it", p[k].0, p[k].1));
        }
        // Turning left, the inside is the profile's left (its -x); right, its +x.
        let turn = d_in.cross(d_out);
        let inner = if turn > 0.0 { reach.0 } else { reach.1 };
        let half = c.acos();
        setback[k] = inner * half.tan();
        out.push(Station { at: host.point(p[k], 0.0), x: host.dir(m * (1.0 / c)), y: host.z });
    }
    for i in 0..segs {
        let (a, b) = (i, (i + 1) % n);
        let len = p[a].dist(p[b]);
        if setback[a] + setback[b] > len + 1e-6 {
            return Err(format!(
                "its profile is too big for the path's bend near ({:.0}, {:.0}): it would fold over itself",
                p[b].0, p[b].1
            ));
        }
    }
    Ok(out)
}

/// How far the points of `rings` reach to the left (-x) and the right (+x) of the path.
fn reach<'a>(rings: impl Iterator<Item = &'a P2>) -> (f64, f64) {
    rings.fold((0.0_f64, 0.0_f64), |(l, r), p| (l.max(-p.0), r.max(p.0)))
}

fn placed(s: &Station, q: P2) -> P3 {
    add(add(s.at, mul(s.x, q.0)), mul(s.y, q.1))
}

/// What a sweep on a level plane covers seen from above: its outline (counter-clockwise) and, round a closed path, its hole
/// (clockwise): the band between each station's least and greatest x (`span`, per station) of the profile.
pub type Plan = (Vec<P2>, Vec<Vec<P2>>);

/// A form's triangles and, where it has one, what it covers seen from above.
pub type Built = (Vec<[P3; 3]>, Option<Plan>);

/// The band [`Plan`] is, from the stations and each one's span of x; none where the host isn't level.
fn band(st: &[Station], span: &[(f64, f64)], closed: bool, host: &Frame) -> Option<Plan> {
    if host.z[2].abs() < 1.0 - 1e-9 {
        return None;
    }
    let flat = |p: P3| P2(p[0], p[1]);
    let left: Vec<P2> = st.iter().zip(span).map(|(s, (lo, _))| flat(placed(s, P2(*lo, 0.0)))).collect();
    let right: Vec<P2> = st.iter().zip(span).map(|(s, (_, hi))| flat(placed(s, P2(*hi, 0.0)))).collect();
    let ccw = |ring: Vec<P2>, want: bool| if (crate::geom::area(&ring) > 0.0) == want { ring } else { ring.into_iter().rev().collect() };
    if closed {
        let (a, b) = (crate::geom::area(&left).abs(), crate::geom::area(&right).abs());
        let (outer, inner) = if a > b { (left, right) } else { (right, left) };
        return Some((ccw(outer, true), vec![ccw(inner, false)]));
    }
    let mut ring = right;
    ring.extend(left.into_iter().rev());
    Some((ccw(ring, true), Vec::new()))
}

/// The least and greatest x of `pts`.
fn span<'a>(pts: impl Iterator<Item = &'a P2>) -> (f64, f64) {
    pts.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)))
}

/// A Sweep: `regions` (its profile, in the profile plane's axes) carried along `path` on `host`, its triangles turned out,
/// and on a level plane what it covers seen from above.
pub fn sweep(regions: &[Region], path: &Path, host: &Frame) -> Result<(Vec<[P3; 3]>, Option<Plan>), String> {
    let rings: Vec<&Vec<P2>> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).collect();
    let st = stations(path, host, reach(rings.iter().flat_map(|r| r.iter())))?;
    let count = st.len() * rings.iter().map(|r| r.len()).sum::<usize>();
    if count * 2 > super::MAX_TRIANGLES {
        return Err(format!("it would be more than {} triangles", super::MAX_TRIANGLES));
    }
    let mut tris = Vec::new();
    for ring in &rings {
        let along: Vec<Vec<P3>> = st.iter().map(|s| ring.iter().map(|q| placed(s, *q)).collect()).collect();
        bands(&mut tris, &along, path.closed);
    }
    if !path.closed {
        let (first, last) = (&st[0], &st[st.len() - 1]);
        for (points, caps) in super::caps(regions)? {
            for [a, b, c] in caps {
                let (pa, pb, pc) = (points[a], points[b], points[c]);
                tris.push([placed(first, pa), placed(first, pc), placed(first, pb)]);
                tris.push([placed(last, pa), placed(last, pb), placed(last, pc)]);
            }
        }
    }
    let width = span(regions.iter().flat_map(|r| r.outer.iter()));
    let plan = band(&st, &vec![width; st.len()], path.closed, host);
    Ok((closed(tris.into_iter().filter(|t| area3(t) > 1e-6).collect())?, plan))
}

/// A Swept Blend: profile `a` at `path`'s start turning into `b` at its end (each one loop, in its profile plane's axes,
/// mapped as a Blend's), its triangles turned out.
pub fn swept_blend(a: &[P2], b: &[P2], twist: i32, path: &Path, host: &Frame) -> Result<(Vec<[P3; 3]>, Option<Plan>), String> {
    let (ra, rb) = mapped(a, b, twist);
    if path.closed {
        return Err("its path closes: a swept blend's path is one line or one arc".into());
    }
    let st = stations(path, host, reach(ra.iter().chain(&rb)))?;
    if st.len() * ra.len() * 2 > super::MAX_TRIANGLES {
        return Err(format!("it would be more than {} triangles", super::MAX_TRIANGLES));
    }
    let p = &path.points;
    let total: f64 = p.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut run = 0.0;
    let mut rings = Vec::with_capacity(st.len());
    let mut spans = Vec::with_capacity(st.len());
    for (k, s) in st.iter().enumerate() {
        if k > 0 {
            run += p[k].dist(p[k - 1]);
        }
        // The ends exactly the profiles (a sum of chords rounds), so a corner held by one ring stays one point there.
        let t = match k {
            0 => 0.0,
            k if k + 1 == st.len() => 1.0,
            _ if total > 0.0 => run / total,
            _ => 0.0,
        };
        let local: Vec<P2> = ra.iter().zip(&rb).map(|(x, y)| *x * (1.0 - t) + *y * t).collect();
        spans.push(span(local.iter()));
        rings.push(local.iter().map(|q| placed(s, *q)).collect::<Vec<P3>>());
    }
    let mut tris = Vec::new();
    bands(&mut tris, &rings, false);
    let (first, last) = (&st[0], &st[st.len() - 1]);
    for [x, y, z] in cap(&ra)? {
        tris.push([placed(first, ra[x]), placed(first, ra[z]), placed(first, ra[y])]);
    }
    for [x, y, z] in cap(&rb)? {
        tris.push([placed(last, rb[x]), placed(last, rb[y]), placed(last, rb[z])]);
    }
    let plan = band(&st, &spans, false, host);
    Ok((closed(tris.into_iter().filter(|t| area3(t) > 1e-6).collect())?, plan))
}

/// Form `form`'s triangles when it is a Blend, a Sweep or a Swept Blend (none for another kind), on `host`, its numbers
/// read by `num`.
pub(crate) fn build<'a>(
    form: &'a Form,
    host: &Frame,
    num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>,
) -> Result<Option<Built>, String> {
    let tris = match form.kind() {
        FormKind::Extrusion | FormKind::Revolve => return Ok(None),
        FormKind::Blend => {
            let base = one_loop(&form.sketch, "base", num)?;
            let top = one_loop(&form.top, "top", num)?;
            let (from, to) = (num(&form.from)?, num(&form.to)?);
            if (to - from).abs() <= TOL {
                return Err(format!("its ends are at one place: first end {from}, second end {to}"));
            }
            (blend(&base, &top, (from, to), form.twist, host)?, None)
        }
        FormKind::Sweep => {
            let p = path(&form.sketch, num)?;
            let profile = form.sweep.as_ref().map(|s| s.profile.as_slice()).unwrap_or_default();
            let regions = profile_loops(profile, "profile", num)?;
            sweep(&regions, &p, host)?
        }
        FormKind::SweptBlend => {
            if form.sketch.len() != 1 {
                return Err(format!("its path has {} lines: a swept blend's path is one line or one arc", form.sketch.len()));
            }
            let p = path(&form.sketch, num)?;
            let (profile, end) = form.sweep.as_ref().map(|s| (s.profile.as_slice(), s.end.as_slice())).unwrap_or_default();
            let a = one_loop(profile, "first profile", num)?;
            let b = one_loop(end, "second profile", num)?;
            swept_blend(&a, &b, form.twist, &p, host)?
        }
    };
    Ok(Some(tris))
}

/// The profile planes of form `form` on `host` as numbers `num` read it: a Sweep's one, a Swept Blend's two (start, end);
/// none for another kind or a path that doesn't chain.
pub fn profile_frames<'a>(form: &'a Form, host: &Frame, num: &mut dyn FnMut(&'a Expr) -> Result<f64, String>) -> Vec<Frame> {
    let Ok(p) = path(&form.sketch, num) else { return Vec::new() };
    match form.kind() {
        FormKind::Sweep => vec![profile_frame(host, p.at, p.way)],
        FormKind::SweptBlend => {
            let n = p.points.len();
            vec![profile_frame(host, p.points[0], (p.points[1] - p.points[0]).unit()), profile_frame(host, p.points[n - 1], p.end_way)]
        }
        _ => Vec::new(),
    }
}

/// `v` cut to the twist a form keeps: Edit Vertices' Twist Right and Left.
pub const MAX_TWIST: i32 = 10_000;

/// Why a form's fields can't make one form, if they can't: a `top` with a `sweep`, either with an axis, a swept blend's path
/// of more than one line, a twist too big.
pub fn check_kind(form: &Form) -> Result<(), String> {
    if !form.top.is_empty() && form.sweep.is_some() {
        return Err("it has a `top` (a blend's) and a `sweep` (a sweep's profile): a form is one of them".into());
    }
    if (!form.top.is_empty() || form.sweep.is_some()) && form.revolves() {
        return Err("a blend or a sweep has no axis line: only a revolve's sketch has one".into());
    }
    if (!form.top.is_empty() || form.sweep.is_some()) && !form.mesh.is_empty() {
        return Err("a `mesh` is as it is: it takes no `top` or `sweep`".into());
    }
    if form.twist.abs() > MAX_TWIST {
        return Err(format!("its twist is {}; it may be up to {MAX_TWIST} either way", form.twist));
    }
    if form.twist != 0 && !matches!(form.kind(), FormKind::Blend | FormKind::SweptBlend) {
        return Err("only a blend or a swept blend twists".into());
    }
    let lines = form.top.len() + form.sweep.as_ref().map_or(0, |s| s.profile.len() + s.end.len());
    if lines > 2 * MAX_SKETCH {
        return Err(format!("its top and profiles have {lines} lines, more than {}", 2 * MAX_SKETCH));
    }
    if form.kind() == FormKind::SweptBlend && form.sketch.len() > 1 {
        return Err(format!("its path has {} lines: a swept blend's path is one line or one arc", form.sketch.len()));
    }
    Ok(())
}
