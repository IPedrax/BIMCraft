//! Wall geometry. A wall's location line runs from its start to its end, straight or an arc; its
//! faces and its layers' boundaries lie across it where its type and its location line put them,
//! the exterior side to its left. Where walls meet, end to end at any angle or one ending on
//! another, their faces meet in corners: each end's cap. Caps and faces make the outline plans
//! fill, 3D and sections cut, and IFC sweeps. A wall's top may follow a roof or a floor above it,
//! its base one below: the lowest of their planes, which is a pitched roof's surface.

use std::f64::consts::{PI, TAU};

use crate::geom::{self, P2};
use crate::model::{Element, Function, Guid, Id, Layer, Location, Model, TOL, Wall};

/// A wall's location line, from its start to its end: straight, or an arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub start: P2,
    pub end: P2,
    /// An arc's centre, its radius, the angle its start lies at from the centre, and how far it
    /// sweeps from there (radians, counter-clockwise positive).
    arc: Option<(P2, f64, f64, f64)>,
}

impl Axis {
    pub fn of(w: &Wall) -> Axis {
        Axis::new(w.start, w.end, w.arc)
    }

    /// From `start` to `end`, turning `arc` degrees.
    pub fn new(start: P2, end: P2, arc: f64) -> Axis {
        let (sweep, d) = (arc.to_radians(), start.dist(end));
        if !(sweep.is_finite() && sweep.abs() > 1e-9 && d > 0.0) {
            return Axis { start, end, arc: None };
        }
        let half = sweep.abs() / 2.0;
        // The centre: off the chord's middle, to its left turning left, past half a circle to its right.
        let c = (start + end) * 0.5 + (end - start).unit().perp() * (sweep.signum() * d / (2.0 * half.tan()));
        Axis { start, end, arc: Some((c, d / (2.0 * half.sin()), (start.1 - c.1).atan2(start.0 - c.0), sweep)) }
    }

    pub fn centre(&self) -> Option<P2> {
        self.arc.map(|a| a.0)
    }

    pub fn radius(&self) -> Option<f64> {
        self.arc.map(|a| a.1)
    }

    pub fn length(&self) -> f64 {
        match self.arc {
            Some((_, r, _, sweep)) => r * sweep.abs(),
            None => self.start.dist(self.end),
        }
    }

    /// The angle from the centre `s` mm along an arc.
    fn angle(r: f64, a0: f64, sweep: f64, s: f64) -> f64 {
        a0 + sweep.signum() * s / r
    }

    /// The way it runs `s` mm along it.
    pub fn tangent(&self, s: f64) -> P2 {
        match self.arc {
            Some((_, r, a0, sweep)) => {
                let a = Axis::angle(r, a0, sweep, s);
                P2(-a.sin(), a.cos()) * sweep.signum()
            }
            None => (self.end - self.start).unit(),
        }
    }

    /// The point `s` mm along it, `off` mm to its left.
    pub fn at(&self, s: f64, off: f64) -> P2 {
        match self.arc {
            // Turning left, its left is toward the centre.
            Some((c, r, a0, sweep)) => {
                let a = Axis::angle(r, a0, sweep, s);
                c + P2(a.cos(), a.sin()) * (r - sweep.signum() * off)
            }
            None => {
                let u = (self.end - self.start).unit();
                self.start + u * s + u.perp() * off
            }
        }
    }

    /// How far along it the point nearest `p` is (from 0 to its length), and how far to its left
    /// `p` lies.
    pub fn project(&self, p: P2) -> (f64, f64) {
        match self.arc {
            Some((c, r, a0, sweep)) => {
                // Turned from its start the way it turns, counted from its middle so a point past
                // either end goes to the nearer one.
                let mid = sweep.abs() / 2.0;
                let t = (sweep.signum() * ((p.1 - c.1).atan2(p.0 - c.0) - a0) - mid + PI).rem_euclid(TAU) - PI + mid;
                (t.clamp(0.0, sweep.abs()) * r, sweep.signum() * (r - p.dist(c)))
            }
            None => {
                let u = (self.end - self.start).unit();
                ((p - self.start).dot(u).clamp(0.0, self.length()), (p - self.start).dot(u.perp()))
            }
        }
    }

    /// How far `p` is from the line `off` mm to its left.
    pub fn distance(&self, p: P2, off: f64) -> f64 {
        p.dist(self.at(self.project(p).0, off))
    }

    /// Where it's cut into pieces from `s0` to `s1`: the two, and along an arc, enough between that
    /// no chord strays a millimetre from it or turns more than 5°.
    pub fn stations(&self, s0: f64, s1: f64) -> Vec<f64> {
        let n = match self.arc {
            Some((_, r, _, _)) => {
                let step = (2.0 * (1.0 - 1.0 / r.max(2.0)).acos()).min(5f64.to_radians());
                ((s1 - s0).abs() / r / step).ceil().clamp(1.0, 720.0) as usize
            }
            None => 1,
        };
        (0..=n).map(|i| s0 + (s1 - s0) * i as f64 / n as f64).collect()
    }

    /// Points from `s0` to `s1` along the line `off` mm to its left: see [`Axis::stations`].
    pub fn points(&self, s0: f64, s1: f64, off: f64) -> Vec<P2> {
        self.stations(s0, s1).into_iter().map(|s| self.at(s, off)).collect()
    }
}

/// How far an arc from `a` through `p` to `b` turns, degrees counter-clockwise: 0 when `p` lies on
/// the line from `a` to `b`, more the farther it bows out to the right.
pub fn sweep_through(a: P2, p: P2, b: P2) -> f64 {
    let (u, v) = (a - p, b - p);
    let side = (b - a).cross(p - a);
    if side.abs() <= 1e-9 * (b - a).length() * (p - a).length() {
        return 0.0;
    }
    // Twice the angle the chord subtends beyond the one at `p`, the way the arc turns.
    let sweep = 2.0 * (PI - u.cross(v).atan2(u.dot(v)).abs());
    if side < 0.0 { sweep.to_degrees() } else { -sweep.to_degrees() }
}

/// How far from the exterior face of a wall of `thickness` with `layers` its location line `loc`
/// lies. The core runs from the first structure layer to the last; without one, it's all of it.
fn depth(thickness: f64, layers: &[Layer], loc: Location) -> f64 {
    let mut core: Option<(f64, f64)> = None;
    let mut at = 0.0;
    for l in layers {
        if l.function == Function::Structure {
            core = Some((core.map_or(at, |c| c.0), at + l.thickness));
        }
        at += l.thickness;
    }
    let (c0, c1) = core.unwrap_or((0.0, thickness));
    match loc {
        Location::Centre => thickness / 2.0,
        Location::CoreCentre => (c0 + c1) / 2.0,
        Location::ExteriorFinish => 0.0,
        Location::InteriorFinish => thickness,
        Location::ExteriorCore => c0,
        Location::InteriorCore => c1,
    }
}

/// Where a wall's faces and its layers' boundaries lie: mm to the left of its location line, from
/// its exterior face to its interior one.
pub fn boundaries(m: &Model, w: &Wall) -> Vec<f64> {
    let Some(t) = m.types.wall.get(&w.type_name) else { return vec![0.0, 0.0] };
    let ext = depth(t.thickness, &t.layers, w.location);
    let mut out = vec![ext];
    let mut at = ext;
    for l in &t.layers {
        at -= l.thickness;
        out.push(at);
    }
    if t.layers.is_empty() {
        out.push(ext - t.thickness);
    }
    out
}

/// A wall's exterior and interior faces: how far to the left of its location line they lie.
pub fn faces(m: &Model, w: &Wall) -> (f64, f64) {
    let t = m.thickness(w);
    let ext = m.types.wall.get(&w.type_name).map_or(t / 2.0, |ty| depth(t, &ty.layers, w.location));
    (ext, ext - t)
}

/// How far to the left of its location line a wall's centre line lies.
pub fn centre(m: &Model, w: &Wall) -> f64 {
    let (ext, int) = faces(m, w);
    (ext + int) / 2.0
}

/// Whether two walls overlap in height, so they join.
pub fn stacked(m: &Model, a: &Wall, b: &Wall) -> bool {
    m.base_z(a) < m.top_z(b) && m.base_z(b) < m.top_z(a)
}

/// A wall where it leaves a node: the way it goes, and where its faces' lines and its centre line
/// are there (each running that way): the face on the counter-clockwise side of that way, the one
/// on the clockwise side.
struct Arm {
    mine: bool,
    way: P2,
    ccw: P2,
    cw: P2,
    centre: P2,
    thick: f64,
}

/// Wall `w` leaving the node at its start, or with `at_end`, at its end.
fn arm(m: &Model, w: &Wall, at_end: bool, mine: bool) -> Arm {
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let s = if at_end { axis.length() } else { 0.0 };
    let t = axis.tangent(s);
    // Leaving from its end, it goes back: its interior face is on the counter-clockwise side.
    let (way, ccw, cw) = if at_end { (t * -1.0, int, ext) } else { (t, ext, int) };
    Arm { mine, way, ccw: axis.at(s, ccw), cw: axis.at(s, cw), centre: axis.at(s, (ext + int) / 2.0), thick: ext - int }
}

/// The walls at wall `id`'s start (or `at_end`, its end), round the node counter-clockwise, its own
/// among them: those ending there, and a wall it ends on, both ways from where it does. Whether it
/// ends on one, too.
fn arms(m: &Model, id: Id, at_end: bool) -> (Vec<Arm>, bool) {
    let Some(w) = m.wall(id) else { return (Vec::new(), false) };
    let node = if at_end { w.end } else { w.start };
    let mut arms = vec![arm(m, w, at_end, true)];
    let mut through = false;
    // ponytail: scans every wall per end, O(n²) per drawing; index wall ends if models grow large.
    for (&other, e) in &m.elements {
        let Element::Wall(o) = e else { continue };
        if other == id || !stacked(m, w, o) {
            continue;
        }
        if o.start.dist(node) <= TOL {
            arms.push(arm(m, o, false, false));
        } else if o.end.dist(node) <= TOL {
            arms.push(arm(m, o, true, false));
        } else {
            // A wall it ends on: the node within its thickness, between its ends.
            let ax = Axis::of(o);
            let (s, across) = ax.project(node);
            let (ext, int) = faces(m, o);
            if s > TOL && s < ax.length() - TOL && (int - TOL..=ext + TOL).contains(&across) && ax.distance(node, across) <= TOL {
                let (t, (e, i), c) = (ax.tangent(s), (ax.at(s, ext), ax.at(s, int)), ax.at(s, (ext + int) / 2.0));
                arms.push(Arm { mine: false, way: t, ccw: e, cw: i, centre: c, thick: ext - int });
                arms.push(Arm { mine: false, way: t * -1.0, ccw: i, cw: e, centre: c, thick: ext - int });
                through = true;
            }
        }
    }
    arms.sort_by(|a, b| a.way.1.atan2(a.way.0).total_cmp(&b.way.1.atan2(b.way.0)));
    (arms, through)
}

/// The corner points that close wall `id`'s start, or with `at_end`, its end: from its face on the
/// clockwise side of the way it leaves the node round to the other. Walls ending where it ends meet
/// it at any angle, their faces running on until they cross (no farther behind the node than the
/// thickest is thick: past that, the end is square); a wall it ends on stops it at its face. Where
/// three or more walls meet, the node itself fills between them.
pub fn cap(m: &Model, id: Id, at_end: bool) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let (arms, through) = arms(m, id, at_end);
    let n = arms.len();
    let Some(i) = arms.iter().position(|a| a.mine) else { return Vec::new() };
    let (prev, me, next) = (&arms[(i + n - 1) % n], &arms[i], &arms[(i + 1) % n]);
    if n < 2 {
        return vec![me.cw, me.ccw];
    }
    let node = if at_end { w.end } else { w.start };
    let (len, limit) = (Axis::of(w).length(), arms.iter().map(|a| a.thick).fold(0.0, f64::max));
    // Where a face of this wall crosses a neighbour's facing face, near enough; else its own end.
    let meet = |face: P2, other: P2, other_way: P2| {
        geom::line_intersection(face, face + me.way, other, other + other_way)
            .filter(|c| (-limit..=len).contains(&(*c - node).dot(me.way)))
            .unwrap_or(face)
    };
    let (cw, ccw) = (meet(me.cw, prev.ccw, prev.way), meet(me.ccw, next.cw, next.way));
    if through { vec![cw, ccw] } else { vec![cw, node, ccw] }
}

/// Wall `id`'s centre line, an arc as chords: each end it joins others at run on (or back) to the
/// nearest of their centre lines, so rooms close at the centre lines whatever line walls stand on.
pub fn centre_line(m: &Model, id: Id) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let mut line = axis.points(0.0, axis.length(), centre(m, w));
    for at_end in [false, true] {
        let (arms, _) = arms(m, id, at_end);
        let Some(me) = arms.iter().find(|a| a.mine) else { continue };
        let reach = 2.0 * arms.iter().map(|a| a.thick).fold(1.0, f64::max);
        let crossing = arms
            .iter()
            .filter(|a| !a.mine)
            .filter_map(|a| geom::line_intersection(me.centre, me.centre + me.way, a.centre, a.centre + a.way))
            .min_by(|a, b| a.dist(me.centre).total_cmp(&b.dist(me.centre)));
        let end = if at_end { line.last_mut() } else { line.first_mut() };
        if let (Some(end), Some(c)) = (end, crossing.filter(|c| c.dist(me.centre) <= reach)) {
            *end = c;
        }
    }
    line
}

/// Wall `id`'s outline in plan: its start cap, its exterior face, its end cap and its interior face.
pub fn outline(m: &Model, id: Id) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    let between = |pts: Vec<P2>| {
        let n = pts.len();
        pts.into_iter().skip(1).take(n.saturating_sub(2))
    };
    let mut ring = cap(m, id, false);
    ring.extend(between(axis.points(0.0, len, ext)));
    ring.extend(cap(m, id, true));
    ring.extend(between(axis.points(len, 0.0, int)));
    ring.dedup_by(|a, b| a.dist(*b) < 1e-6);
    ring
}

/// Where the line `off` mm to the left of wall `w`'s location line meets its cap at its start (or
/// its end): along the line's tangent there, onto the cap's corners.
fn on_cap(axis: &Axis, cap: &[P2], at_end: bool, off: f64) -> P2 {
    let s = if at_end { axis.length() } else { 0.0 };
    let (p, t) = (axis.at(s, off), axis.tangent(s));
    cap.windows(2)
        .filter_map(|seg| match seg {
            [a, b] => geom::line_intersection(p, p + t, *a, *b).filter(|q| geom::dist_to_segment(*q, *a, *b) <= 1e-6),
            _ => None,
        })
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
        .unwrap_or(p)
}

/// The lines of wall `id`'s faces and of the boundaries between its layers, exterior first, each
/// from its start cap to its end cap.
pub fn layer_lines(m: &Model, id: Id) -> Vec<Vec<P2>> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let len = axis.length();
    let (start, end) = (cap(m, id, false), cap(m, id, true));
    boundaries(m, w)
        .into_iter()
        .map(|off| {
            let mut line = axis.points(0.0, len, off);
            if let Some(first) = line.first_mut() {
                *first = on_cap(&axis, &start, false, off);
            }
            if let Some(last) = line.last_mut() {
                *last = on_cap(&axis, &end, true, off);
            }
            line
        })
        .collect()
}

/// A plane over the plan: its height at `q` is `z + g.0 · q.0 + g.1 · q.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub z: f64,
    pub g: P2,
}

impl Plane {
    pub fn flat(z: f64) -> Plane {
        Plane { z, g: P2(0.0, 0.0) }
    }

    pub fn at(&self, q: P2) -> f64 {
        self.z + self.g.dot(q)
    }

    /// `ring` lifted onto it: points `[x, y, z]`.
    pub fn lift(&self, ring: &[P2]) -> Vec<[f64; 3]> {
        ring.iter().map(|q| [q.0, q.1, self.at(*q)]).collect()
    }

    /// The plane a flat polygon in space lies in, by Newell's normal; none for an upright one.
    pub fn of(face: &[[f64; 3]]) -> Option<Plane> {
        let n = face.iter().zip(face.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| {
            [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])]
        });
        let o = face.first()?;
        (n[2].abs() > 1e-9).then(|| {
            let g = P2(-n[0] / n[2], -n[1] / n[2]);
            Plane { z: o[2] - g.0 * o[0] - g.1 * o[1], g }
        })
    }
}

/// What a wall's base stands on, or its top keeps under: a level plane at `own`, or, over the roof
/// or floor it's attached to, that one's plane there (each of a roof's slopes over its own part of
/// the plan, in convex pieces), and outside it still `own`.
#[derive(Clone)]
struct Bound {
    regions: Vec<(Vec<P2>, Plane)>,
    own: f64,
}

impl Bound {
    /// The roof or floor `id` from below (with `top`, from above), or `own` without one.
    fn of(m: &Model, id: Option<Guid>, top: bool, own: f64) -> Bound {
        let regions = match id.and_then(|id| m.elements.get(id)) {
            Some(Element::Roof(r)) => m
                .roof_pieces(r)
                .unwrap_or_default()
                .into_iter()
                .flat_map(|(ring, p)| {
                    let plane = Plane { z: p.z + if top { m.roof_rise(r, &p) } else { 0.0 }, ..p };
                    convex_pieces(&ring).into_iter().map(move |c| (c, plane))
                })
                .collect(),
            Some(Element::Floor(f)) => {
                let face = m.floor_top(f);
                let plane = Plane { z: face.z - if top { 0.0 } else { m.floor_depth(f) }, ..face };
                convex_pieces(&f.boundary).into_iter().map(|c| (c, plane)).collect()
            }
            _ => Vec::new(),
        };
        Bound { regions, own }
    }

    /// The plane over `q`: of the part over it, or within a millimetre of it; of two, the lower.
    fn over(&self, q: P2) -> Option<Plane> {
        let near = |p: &Vec<P2>| geom::contains(p, q) || geom::ring_edges(p).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
        self.regions.iter().filter(|(r, _)| near(r)).map(|(_, p)| *p).min_by(|a, b| a.at(q).total_cmp(&b.at(q)))
    }

    /// Its height over `q`.
    fn at(&self, q: P2) -> f64 {
        self.over(q).map_or(self.own, |p| p.at(q))
    }

    /// `ring` cut where a different plane holds, each part with its plane.
    fn parts(&self, ring: Vec<P2>) -> Vec<(Vec<P2>, Plane)> {
        let mut out = Vec::new();
        let mut rest = vec![ring];
        for (piece, plane) in &self.regions {
            let mut outside = Vec::new();
            for r in rest {
                let (inn, out_) = split(r, piece);
                out.extend(inn.map(|i| (i, *plane)));
                outside.extend(out_);
            }
            rest = outside;
        }
        out.extend(rest.into_iter().map(|r| (r, Plane::flat(self.own))));
        out
    }
}

/// A polygon as convex pieces, each counter-clockwise: itself when it's convex, else its triangles.
pub(crate) fn convex_pieces(ring: &[P2]) -> Vec<Vec<P2>> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let ccw: Vec<P2> = if geom::area(ring) < 0.0 { ring.iter().rev().copied().collect() } else { ring.to_vec() };
    let n = ccw.len();
    let convex = (0..n).all(|i| (ccw[(i + 1) % n] - ccw[i]).cross(ccw[(i + 2) % n] - ccw[(i + 1) % n]) >= -1e-6);
    if convex {
        return vec![ccw];
    }
    geom::triangulate(&ccw).into_iter().map(|t| t.iter().filter_map(|i| ccw.get(*i).copied()).collect()).collect()
}

/// `ring` cut by the convex counter-clockwise `piece`: the part inside it (if any), and the parts
/// outside, apart. A repeated point of the piece's is no edge to cut by.
pub(crate) fn split(ring: Vec<P2>, piece: &[P2]) -> (Option<Vec<P2>>, Vec<Vec<P2>>) {
    let big = |r: &Vec<P2>| r.len() >= 3 && geom::area(r).abs() > 1.0;
    let mut inside = ring;
    let mut outside = Vec::new();
    for (a, b) in geom::ring_edges(piece).filter(|(a, b)| a.dist(*b) > 1e-6) {
        let left = |q: P2| (b - a).cross(q - a);
        let out = geom::clip(&inside, |q| -left(q));
        if big(&out) {
            outside.push(out);
        }
        inside = geom::clip(&inside, left);
    }
    (Some(inside).filter(big), outside)
}

/// What's left of `rings` outside `hole`: convex pieces, counter-clockwise.
pub(crate) fn minus(rings: &[Vec<P2>], hole: &[P2]) -> Vec<Vec<P2>> {
    let holes = convex_pieces(hole);
    rings
        .iter()
        .flat_map(|r| holes.iter().fold(convex_pieces(r), |parts, piece| parts.into_iter().flat_map(|p| split(p, piece).1).collect()))
        .collect()
}

/// The underside of roof or floor `id` over `q`, if it's over `q`.
pub fn underside(m: &Model, id: Id, q: P2) -> Option<f64> {
    Bound::of(m, Some(m.guid(id)), false, f64::NAN).over(q).map(|p| p.at(q))
}

/// A wall's base and top: what it's attached to, or a level plane.
fn bounds(m: &Model, w: &Wall) -> (Bound, Bound) {
    (Bound::of(m, w.attach_base, true, m.own_base(w)), Bound::of(m, w.attach_top, false, m.own_top(w)))
}

/// A wall's top over the point `s` mm along its centre line.
pub fn top_at(m: &Model, w: &Wall, s: f64) -> f64 {
    bounds(m, w).1.at(Axis::of(w).at(s, centre(m, w)))
}

/// A wall's lowest base and highest top along its faces and its centre line, where what it's
/// attached to bends (sampled: a ridge along it may fall between samples).
pub fn heights(m: &Model, w: &Wall) -> (f64, f64) {
    if w.attach_base.is_none() && w.attach_top.is_none() {
        return (m.own_base(w), m.own_top(w));
    }
    let (base, top) = bounds(m, w);
    let (axis, (ext, int)) = (Axis::of(w), faces(m, w));
    let len = axis.length();
    let pts: Vec<P2> =
        [ext, (ext + int) / 2.0, int].iter().flat_map(|off| (0..=32).map(move |i| axis.at(len * f64::from(i) / 32.0, *off))).collect();
    let lo = pts.iter().map(|q| base.at(*q)).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|q| top.at(*q)).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

/// A part of a wall's solid: a ring in plan, from a bottom plane up to a top one; of a curved wall,
/// with its centre and its centre line's radius.
pub struct Prism {
    pub ring: Vec<P2>,
    pub bottom: Plane,
    pub top: Plane,
    pub curve: Option<(P2, f64)>,
}

/// Wall `id` as prisms: its outline cut across where its doors and windows start and end (with
/// `openings`) and along an arc into short straight pieces, each piece from its base up to its
/// top less the openings over it, cut where what it's attached to changes plane.
pub fn prisms(m: &Model, id: Id, openings: bool) -> Vec<Prism> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let guid = m.guid(id);
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    let z0 = m.own_base(w);
    // The openings: along the wall, and up it.
    let holes: Vec<((f64, f64), (f64, f64))> = m
        .elements
        .values()
        .filter(|_| openings)
        .filter_map(|e| match e {
            Element::Door(o) | Element::Window(o) if o.host == guid => {
                let t = m.opening_type(e)?;
                Some(((o.offset - t.width / 2.0, o.offset + t.width / 2.0), (z0 + o.sill, z0 + o.sill + t.height)))
            }
            _ => None,
        })
        .collect();
    let mut cuts = axis.stations(0.0, len);
    cuts.extend(holes.iter().flat_map(|((s0, s1), _)| [*s0, *s1]).filter(|s| *s > 0.0 && *s < len));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let (start, end) = (cap(m, id, false), cap(m, id, true));
    let (base, top) = bounds(m, w);
    let curve = axis.centre().map(|c| (c, axis.at(0.0, (ext + int) / 2.0).dist(c)));
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        let mut ring = if a <= 0.0 { start.clone() } else { vec![axis.at(a, int), axis.at(a, ext)] };
        ring.extend(if b >= len { end.clone() } else { vec![axis.at(b, ext), axis.at(b, int)] });
        // Up from its base to its top, past the openings over its middle.
        let mid = (a + b) / 2.0;
        let mut gaps: Vec<(f64, f64)> = holes.iter().filter(|((s0, s1), _)| *s0 <= mid && mid <= *s1).map(|(_, z)| *z).collect();
        gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
        let flat = |z: f64| Bound { regions: Vec::new(), own: z };
        let mut floors: Vec<Bound> = vec![base.clone()];
        let mut ceilings: Vec<Bound> = Vec::new();
        for (g0, g1) in gaps {
            ceilings.push(flat(g0));
            floors.push(flat(g1));
        }
        ceilings.push(top.clone());
        for (under, over) in floors.iter().zip(&ceilings) {
            for (part, bottom) in under.parts(ring.clone()) {
                for (part, top) in over.parts(part) {
                    // Only where it has some height.
                    let part = geom::clip(&part, |q| top.at(q) - bottom.at(q));
                    if part.len() >= 3 && geom::area(&part).abs() > 1.0 {
                        out.push(Prism { ring: part, bottom, top, curve });
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;

    fn session(script: &str) -> Session {
        let mut s = Session::new();
        s.script(script).unwrap();
        s
    }

    fn near(a: P2, b: P2) -> bool {
        a.dist(b) < 1e-6
    }

    #[test]
    fn an_arc_through_three_points_turns_toward_the_middle_one() {
        assert!((sweep_through(P2(0.0, 0.0), P2(2000.0, -2000.0), P2(4000.0, 0.0)) - 180.0).abs() < 1e-9);
        assert!((sweep_through(P2(0.0, 0.0), P2(2000.0, 2000.0), P2(4000.0, 0.0)) + 180.0).abs() < 1e-9);
        let a = Axis::new(P2(100.0, 50.0), P2(2100.0, -900.0), -75.0);
        assert!((sweep_through(a.start, a.at(700.0, 0.0), a.end) + 75.0).abs() < 1e-9);
        assert_eq!(sweep_through(P2(0.0, 0.0), P2(1000.0, 0.0), P2(4000.0, 0.0)), 0.0);
    }

    #[test]
    fn arcs_turn_left_for_a_positive_sweep_and_measure_along_themselves() {
        // From west to east turning left a half circle: it bulges south.
        let a = Axis::new(P2(0.0, 0.0), P2(4000.0, 0.0), 180.0);
        assert!(near(a.centre().unwrap(), P2(2000.0, 0.0)) && (a.radius().unwrap() - 2000.0).abs() < 1e-9);
        assert!(near(a.at(a.length() / 2.0, 0.0), P2(2000.0, -2000.0)));
        assert!(near(a.tangent(0.0), P2(0.0, -1.0)), "it sets off south");
        // Its left is toward the centre; a point projects to how far along it lies and how far left.
        assert!(near(a.at(a.length() / 2.0, 100.0), P2(2000.0, -1900.0)));
        let (s, left) = a.project(P2(2000.0, -2100.0));
        assert!((s - a.length() / 2.0).abs() < 1e-6 && (left + 100.0).abs() < 1e-9);
        assert_eq!(a.project(P2(-500.0, 300.0)).0, 0.0, "past its start, its start");
        // Past half a circle its centre is to the right of the chord.
        let big = Axis::new(P2(0.0, 0.0), P2(2000.0, 0.0), 270.0);
        assert!(big.centre().unwrap().1 < 0.0);
        assert!((big.length() - 1.5 * PI * big.radius().unwrap()).abs() < 1e-6);
        let points = a.points(0.0, a.length(), 0.0);
        assert!(points.len() > 30 && points.iter().all(|p| (p.dist(P2(2000.0, 0.0)) - 2000.0).abs() < 1e-6));
    }

    /// Two 200 mm walls leaving the origin, east and `degrees` round from it.
    fn corner(degrees: f64) -> Session {
        let (x, y) = (4000.0 * degrees.to_radians().cos(), 4000.0 * degrees.to_radians().sin());
        session(&format!(r#"wall {{"points": [[4000, 0], [0, 0], [{x}, {y}]]}}"#))
    }

    #[test]
    fn walls_meet_at_any_angle_in_a_mitre() {
        for degrees in [90.0, 60.0, 120.0, 150.0] {
            let s = corner(degrees);
            let (a, b) = (outline(&s.model, 100002), outline(&s.model, 100003));
            // The faces cross on the bisector, 100 / sin(half the angle) from the corner, both ways.
            let half = (degrees / 2.0f64).to_radians();
            let inner = P2(half.cos(), half.sin()) * (100.0 / half.sin());
            for o in [&a, &b] {
                assert!(o.iter().any(|p| near(*p, inner)) && o.iter().any(|p| near(*p, inner * -1.0)), "{degrees}°: {o:?}");
                // Mitred, each is as big as its length times its thickness: nothing overlaps, nothing's missing.
                assert!((geom::area(o).abs() - 800_000.0).abs() < 1e-3, "{degrees}°: {}", geom::area(o));
            }
        }
    }

    #[test]
    fn sharp_corners_square_off_rather_than_spike() {
        let s = corner(20.0);
        let o = outline(&s.model, 100003);
        // Outside the corner the faces would cross 5.7 thicknesses back: the end goes square there.
        assert!(o.iter().all(|p| p.length() < 700.0 || p.length() > 3000.0), "{o:?}");
        assert!(o.iter().any(|p| near(*p, P2(-20f64.to_radians().sin(), 20f64.to_radians().cos()) * 100.0)), "{o:?}");
    }

    #[test]
    fn a_wall_ending_on_another_stops_at_its_face() {
        let s = session(
            r#"wall {"points": [[-2000, 0], [2000, 0]], "type": "Exterior - 300mm"}
               wall {"points": [[0, 0], [1000, 3000]], "type": "Interior - 100mm"}"#,
        );
        let t = outline(&s.model, 100003);
        assert!(t.iter().filter(|p| p.1 < 1000.0).all(|p| (p.1 - 150.0).abs() < 1e-6), "{t:?}");
        assert_eq!(cap(&s.model, 100003, false).len(), 2, "the through wall fills the node");
        // The through wall goes on, square at its ends.
        assert_eq!(outline(&s.model, 100002).len(), 4);
    }

    #[test]
    fn three_walls_meeting_share_the_node() {
        let s = session(
            r#"wall {"points": [[0, 0], [3000, 0]]}
               wall {"points": [[0, 0], [-1500, 2598.0762]]}
               wall {"points": [[0, 0], [-1500, -2598.0762]]}"#,
        );
        let total: f64 = [100_002, 100_003, 100_004].iter().map(|id| geom::area(&outline(&s.model, *id)).abs()).sum();
        // Three 3 m arms 200 mm thick, less the triangle they'd all three cover where they meet.
        let hub = 3f64.sqrt() / 4.0 * 200.0 * 200.0;
        assert!((total - (3.0 * 600_000.0 - hub)).abs() < 1.0, "{total}");
        for id in [100_002, 100_003, 100_004] {
            assert!(cap(&s.model, id, false).iter().any(|p| near(*p, P2(0.0, 0.0))), "{id}");
        }
    }

    #[test]
    fn curved_walls_outline_their_arc_and_join_at_their_ends() {
        let s = session(
            r#"wall {"points": [[0, 0], [4000, 0]], "arc": 180}
               wall {"points": [[4000, 0], [4000, 3000]]}"#,
        );
        // Half a ring 2 m out, 200 mm thick: its length on the centre line times its thickness.
        let o = outline(&s.model, 100002);
        let ring = PI * 2000.0 * 200.0;
        assert!((geom::area(&o).abs() - ring).abs() < 0.005 * ring, "{}", geom::area(&o));
        // At its east end it runs north, on into the straight wall: their ends meet square.
        let end = cap(&s.model, 100002, true);
        assert!(end.iter().any(|p| near(*p, P2(3900.0, 0.0))) && end.iter().any(|p| near(*p, P2(4100.0, 0.0))), "{end:?}");
    }

    #[test]
    fn faces_and_layers_lie_where_the_location_line_puts_them() {
        let s = session(
            r#"type {"category": "wall", "name": "C", "layers": [{"function": "finish", "thickness": 100}, {"function": "thermal", "thickness": 50}, {"function": "structure", "thickness": 140}, {"function": "finish", "thickness": 10}]}
               wall {"points": [[0, 0], [5000, 0]], "type": "C"}
               wall {"points": [[0, 2000], [5000, 2000]], "type": "C", "location": "core-centre"}"#,
        );
        let (a, b) = (s.model.wall(100002).unwrap(), s.model.wall(100003).unwrap());
        assert_eq!(boundaries(&s.model, a), [150.0, 50.0, 0.0, -140.0, -150.0]);
        assert_eq!(boundaries(&s.model, b), [220.0, 120.0, 70.0, -70.0, -80.0]);
        assert_eq!((faces(&s.model, b), centre(&s.model, b)), ((220.0, -80.0), 70.0));
        let lines = layer_lines(&s.model, 100003);
        assert_eq!(lines.len(), 5);
        assert!(near(lines[2][0], P2(0.0, 2070.0)) && near(lines[2][1], P2(5000.0, 2070.0)));
    }

    #[test]
    fn walls_split_into_prisms_around_their_openings_and_under_what_they_follow() {
        let s = session(
            r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
               window {"at": [8000, 3000]}
               roof {"gable": true}"#,
        );
        // Unattached: below and above the window, and either side of it.
        assert_eq!(prisms(&s.model, 100003, true).len(), 4);
        assert_eq!(prisms(&s.model, 100003, false).len(), 1);
        let mut m = (*s.model).clone();
        let roof = m.guid(100007);
        if let Some(Element::Wall(w)) = m.elements.get_mut(100003) {
            w.attach_top = Some(roof);
        }
        // Attached under a gable: the parts either side of the ridge, under the two slopes.
        let whole = prisms(&m, 100003, false);
        assert_eq!(whole.len(), 2);
        let tan = 30f64.to_radians().tan();
        assert!(whole.iter().all(|p| (p.top.at(P2(8000.0, 3000.0)) - (3000.0 + 3100.0 * tan)).abs() < 1e-6));
    }
}
