//! Solids IFC builds by turning a profile about an axis or sweeping one along a curve, and advanced B-reps, read as faces
//! turned out, as `Reader::faces` reads the rest; and the B-spline curves profiles and edges follow. An item builds at most
//! `MAX_FACES` faces. One that builds nothing, or less than it holds, says why through `Reader::give_up`, and the import
//! puts that in its note: a B-rep with a B-spline surface, a surface BIMCraft doesn't tessellate, a face that wouldn't
//! triangulate, a sweep along part of a curve it can't measure by parameter.
//!
//! - A revolved solid is its profile turned in `TURN` steps a whole turn, its holes cut as an extrusion's are (convex
//!   pieces); a tapered one is its first profile turned, as a tapered extrusion is read. A positive angle turns it the way
//!   IfcOpenShell does, against the right hand about the axis: about y, from x toward z.
//! - A sweep along a curve (a fixed reference sweep, or one along a curve on a plane) has its profile square to the curve,
//!   its x toward the fixed direction or the plane's normal (ISO 10303-42's swept area solids), mitred at each bend; a
//!   bend that turns back on itself ends one piece and starts the next. Its start and end parameters are read on a polyline
//!   and an indexed polycurve of lines (each segment a parameter of 1), a circle or an ellipse (its angle), and a trimmed
//!   curve where they are its own trims.
//! - An advanced B-rep's faces are each read on their surface: a plane, a cylinder, a sphere, a surface of revolution or of
//!   linear extrusion; each edge sampled as `Reader::curve` draws it (a circle in `CIRCLE` chords a turn). A flat face of one
//!   loop is that loop; any other is triangulated in a plane its surface's parameters lie flat in (a turned surface's angle
//!   and length along its meridian as polar coordinates, so a whole band is a ring), with points in it a step apart on
//!   a curved surface, and each triangle turned the way the surface's normal is, as the face's sense says.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use super::{Frame, Reader, enumeration, indices, list, newell, orthonormal, real, reference, refs};
use crate::family::MAX_TRIANGLES;
use crate::geom::P2;
use crate::model::{MAX_COORD, MAX_POINTS, TOL};
use crate::step::{Entity, V};
use crate::triangulation;
use crate::view3d::{P3, add, cross, dot, mul, sub, unit};
use crate::walls;

/// The most faces one representation item builds: as many as a family's mesh holds triangles.
pub(super) const MAX_FACES: usize = MAX_TRIANGLES;
/// Why an item past them is left out.
pub(super) const TOO_MANY: &str = "it would take more than 100000 faces";
const BSPLINE: &str = "a B-rep with B-spline surfaces, which BIMCraft doesn't tessellate";
const SURFACE: &str = "a B-rep with a face on a surface BIMCraft doesn't tessellate";
const EDGE: &str = "a B-rep with an edge along a curve BIMCraft doesn't read";
const UNTRIANGULATED: &str = "a B-rep with a face BIMCraft couldn't triangulate";
const NOT_PLANE: &str = "a profile swept along a curve on a surface other than a plane";
const PARAMETERS: &str = "a profile swept along part of a curve BIMCraft can't measure by its parameters";

/// Steps of a whole turn, as a family's revolved form takes.
const TURN: f64 = 32.0;
/// Chords of a whole circle on an edge, as `Reader::curve` draws one.
const CIRCLE: f64 = 24.0;
/// Points of a B-spline curve between each two knots.
const SPAN: usize = 12;
/// The highest degree a B-spline curve is read in.
const MAX_DEGREE: usize = 16;
/// The most points put inside one curved face to triangulate it by, and the most rows of them along each way.
const MAX_INSIDE: usize = 4_000;
const MAX_ROWS: usize = 64;
/// The most points round a face that isn't flat: each point put inside it is measured from every one.
const MAX_ROUND: usize = 4_096;

fn far(a: P3, b: P3) -> bool {
    let d = sub(a, b);
    dot(d, d) > 1e-12
}

/// `p` turned about the axis through `o` along unit `d` by `t` radians, as the right hand turns about it.
fn turn(p: P3, o: P3, d: P3, t: f64) -> P3 {
    let v = sub(p, o);
    let (s, c) = t.sin_cos();
    add(o, add(add(mul(v, c), mul(cross(d, v), s)), mul(d, dot(d, v) * (1.0 - c))))
}

/// `pts` less repeats in a row and the closing repeat of the first.
fn ring(pts: Vec<P3>) -> Vec<P3> {
    let mut out: Vec<P3> = Vec::with_capacity(pts.len());
    for p in pts {
        if out.last().is_none_or(|q| far(*q, p)) {
            out.push(p);
        }
    }
    while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| !far(*a, *b)) {
        out.pop();
    }
    out
}

/// The volume `faces` close, by the tetrahedra from the first corner: positive while they face out.
fn signed_volume(faces: &[Vec<P3>]) -> f64 {
    let Some(o) = faces.first().and_then(|f| f.first()).copied() else { return 0.0 };
    let mut v = 0.0;
    for f in faces {
        let Some(a) = f.first().map(|p| sub(*p, o)) else { continue };
        for w in f.get(1..).unwrap_or_default().windows(2) {
            if let [b, c] = w {
                v += dot(a, cross(sub(*b, o), sub(*c, o)));
            }
        }
    }
    v / 6.0
}

/// `faces` turned inside out where they face in.
fn turned_out(faces: &mut [Vec<P3>]) {
    if signed_volume(faces) < 0.0 {
        faces.iter_mut().for_each(|f| f.reverse());
    }
}

/// The faces between each ring and the next (the last and the first too, where `closed`) and a cap at each end of an open
/// run, turned out. Rings run counter-clockwise about the way they go, or all the other way.
fn loft(out: &mut Vec<Vec<P3>>, rings: &[Vec<P3>], closed: bool) {
    let mut faces: Vec<Vec<P3>> = Vec::new();
    let mut add_face = |f: Vec<P3>| {
        let f = ring(f);
        if f.len() >= 3 {
            faces.push(f);
        }
    };
    let n = rings.len();
    let next = if closed { n } else { n.saturating_sub(1) };
    for k in 0..next {
        let (Some(r0), Some(r1)) = (rings.get(k), rings.get((k + 1) % n)) else { continue };
        for j in 0..r0.len() {
            let jj = (j + 1) % r0.len();
            if let (Some(a), Some(b), Some(c), Some(d)) = (r0.get(j), r0.get(jj), r1.get(jj), r1.get(j)) {
                add_face(vec![*a, *b, *c, *d]);
            }
        }
    }
    if !closed && let (Some(first), Some(last)) = (rings.first(), rings.last()) {
        add_face(first.iter().rev().copied().collect());
        add_face(last.clone());
    }
    turned_out(&mut faces);
    out.extend(faces);
}

/// Points along `pts` from parameter `t0` to `t1`, each segment a parameter of 1; none for no length.
fn part(pts: &[P3], t0: f64, t1: f64) -> Option<Vec<P3>> {
    if t1 - t0 < 1e-9 || pts.len() < 2 {
        return None;
    }
    let at = |t: f64| {
        let i = (t.floor().max(0.0) as usize).min(pts.len() - 2);
        let (a, b) = (pts.get(i)?, pts.get(i + 1)?);
        Some(add(*a, mul(sub(*b, *a), t - i as f64)))
    };
    let mut out = vec![at(t0)?];
    for i in (t0.floor() as usize + 1)..(t1.ceil() as usize) {
        out.push(*pts.get(i)?);
    }
    out.push(at(t1)?);
    Some(out)
}

/// An arc of the ellipse in `f` of radii `ra` and `rb`, from angle `a0` through `sweep`, in `CIRCLE` chords a turn.
fn arc(f: &Frame, ra: f64, rb: f64, a0: f64, sweep: f64) -> Vec<P3> {
    let steps = (sweep.abs() / TAU * CIRCLE).ceil().max(1.0) as usize;
    (0..=steps).map(|k| a0 + sweep * k as f64 / steps as f64).map(|t| f.at([ra * t.cos(), rb * t.sin(), 0.0])).collect()
}

/// The runs of rings `piece` makes along `line`, its x toward `toward`, and whether a run closes on itself. Along a
/// `smooth` line (a curve drawn as chords) each ring is square to the curve at its point, the way between its chords, and
/// at an end the way the curve's first or last two chords say; along any other, square to each segment and mitred where it
/// bends, and a bend that turns back on itself ends one run and starts the next.
fn mitred(piece: &[P2], line: &[P3], toward: P3, smooth: bool) -> Vec<(Vec<Vec<P3>>, bool)> {
    let mut pts: Vec<P3> = Vec::with_capacity(line.len());
    for p in line {
        if pts.last().is_none_or(|q| far(*q, *p)) {
            pts.push(*p);
        }
    }
    let closed = pts.len() > 3 && pts.first().zip(pts.last()).is_some_and(|(a, b)| !far(*a, *b));
    if closed {
        pts.pop();
    }
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let segments = if closed { n } else { n - 1 };
    let at = |i: usize| pts.get(i % n).copied().unwrap_or_default();
    // Each segment's way and axes: x toward `toward` square to it, else the last segment's x squared to it.
    let mut axes: Vec<(P3, P3, P3)> = Vec::with_capacity(segments);
    let mut last: Option<P3> = None;
    for k in 0..segments {
        let d = unit(sub(at(k + 1), at(k)));
        let square = |v: P3| sub(v, mul(d, dot(v, d)));
        let x = [Some(toward), last, Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0])]
            .into_iter()
            .flatten()
            .map(square)
            .find(|v| dot(*v, *v) > 1e-12)
            .map_or([0.0, 0.0, 1.0], unit);
        last = Some(x);
        axes.push((d, x, cross(d, x)));
    }
    let axis = |k: usize| axes.get(k % segments).copied().unwrap_or(([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]));
    let place = |k: usize, o: P3| -> Vec<P3> {
        let (_, x, y) = axis(k);
        piece.iter().map(|p| add(o, add(mul(x, p.0), mul(y, p.1)))).collect()
    };
    // Square to `t` at `o`: x toward `toward`, else segment `k`'s x, squared to it.
    let square_to = |t: P3, k: usize, o: P3| -> Vec<P3> {
        let square = |v: P3| sub(v, mul(t, dot(v, t)));
        let x = [toward, axis(k).1].into_iter().map(square).find(|v| dot(*v, *v) > 1e-12).map_or(axis(k).1, unit);
        let y = cross(t, x);
        piece.iter().map(|p| add(o, add(mul(x, p.0), mul(y, p.1)))).collect()
    };
    // The way a curve drawn as chords `a` then `b` goes where they meet; and at the start of `a`, before `b`.
    let between_chords = |a: P3, b: P3| Some(add(a, b)).filter(|v| dot(*v, *v) > 1e-12).map(unit);
    let at_end = |a: P3, b: P3| between_chords(a, sub(mul(a, 2.0 * dot(a, b)), b)).unwrap_or(a);
    if smooth && segments > 1 {
        let ways: Option<Vec<P3>> = (0..n)
            .map(|i| match (closed, i) {
                (false, 0) => Some(at_end(axis(0).0, axis(1).0)),
                (false, i) if i == n - 1 => Some(at_end(axis(segments - 1).0, axis(segments - 2).0)),
                _ => between_chords(axis((i + segments - 1) % segments).0, axis(i % segments).0),
            })
            .collect();
        if let Some(ways) = ways {
            let rings = ways.iter().enumerate().map(|(i, t)| square_to(*t, i.min(segments - 1), at(i))).collect();
            return vec![(rings, closed)];
        }
    }
    // Segment `a`'s ring at `o`, slid along it onto the plane halfway between it and segment `b`.
    let mitre = |a: usize, b: usize, o: P3| -> Option<Vec<P3>> {
        let (da, db) = (axis(a).0, axis(b).0);
        let half = add(da, db);
        if dot(half, half) < 1e-12 {
            return None;
        }
        let m = unit(half);
        let c = dot(da, m);
        (c >= 0.1).then(|| place(a, o).into_iter().map(|q| add(q, mul(da, -dot(sub(q, o), m) / c))).collect())
    };
    if closed {
        let rings: Option<Vec<Vec<P3>>> = (0..n).map(|i| mitre((i + n - 1) % n, i, at(i))).collect();
        if let Some(rings) = rings {
            return vec![(rings, true)];
        }
        // Turned back on itself somewhere: an open line, back to its start.
        let mut open = pts.clone();
        open.push(at(0));
        return mitred(piece, &open, toward, false);
    }
    let mut runs = Vec::new();
    let mut rings = vec![place(0, at(0))];
    for i in 1..n - 1 {
        match mitre(i - 1, i, at(i)) {
            Some(r) => rings.push(r),
            None => {
                rings.push(place(i - 1, at(i)));
                runs.push((std::mem::take(&mut rings), false));
                rings.push(place(i, at(i)));
            }
        }
    }
    rings.push(place(segments - 1, at(n - 1)));
    runs.push((rings, false));
    runs
}

/// De Boor's evaluation of the curve of `degree` with `knots` and homogeneous control points `h` at `u`, in knot span
/// `span`.
fn de_boor(knots: &[f64], h: &[[f64; 4]], degree: usize, span: usize, u: f64) -> Option<P3> {
    let mut d: Vec<[f64; 4]> = (0..=degree).map(|j| h.get(j + span - degree).copied()).collect::<Option<_>>()?;
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let (lo, hi) = (*knots.get(j + span - degree)?, *knots.get(j + 1 + span - r)?);
            let alpha = if hi > lo { (u - lo) / (hi - lo) } else { 0.0 };
            let (prev, now) = (*d.get(j - 1)?, *d.get(j)?);
            *d.get_mut(j)? = [0, 1, 2, 3].map(|k| (1.0 - alpha) * prev[k] + alpha * now[k]);
        }
    }
    let p = d.get(degree)?;
    (p[3].abs() > 1e-12).then(|| [p[0] / p[3], p[1] / p[3], p[2] / p[3]])
}

/// What a face lies on.
enum Surface {
    Plane(Frame),
    /// About the frame's z through its origin: its meridian, each point's length along it and where it is, how far along
    /// the axis and how far out from it (and 0).
    Turned(Frame, Vec<(f64, P3)>),
    /// A curve swept straight along a direction: each of its points and its length along it there.
    Extruded(Vec<(f64, P3)>, P3),
}

/// Points of a line and their lengths along it, none repeated; none for fewer than two.
fn lengths<T: Copy>(pts: &[T], dist: impl Fn(T, T) -> f64) -> Vec<(f64, T)> {
    let mut out: Vec<(f64, T)> = Vec::with_capacity(pts.len());
    for p in pts {
        match out.last() {
            None => out.push((0.0, *p)),
            Some((s, q)) => {
                let d = dist(*q, *p);
                if d > 1e-6 {
                    out.push((s + d, *p));
                }
            }
        }
    }
    if out.len() < 2 { Vec::new() } else { out }
}

/// The length along polyline `line` of the point on it nearest `p`, by `nearest` of a segment's ends and `p`: the point's
/// share of the way from the first to the second, and how far it is.
fn nearest_on<T: Copy>(line: &[(f64, T)], p: T, nearest: impl Fn(T, T, T) -> (f64, f64)) -> f64 {
    let mut best = (f64::INFINITY, line.first().map_or(0.0, |(s, _)| *s));
    for w in line.windows(2) {
        if let [(s0, a), (s1, b)] = w {
            let (share, d) = nearest(*a, *b, p);
            if d < best.0 {
                best = (d, s0 + (s1 - s0) * share);
            }
        }
    }
    best.1
}

/// Where along `line` length `s` is: the two points it lies between and its share of the way.
fn between<T: Copy>(line: &[(f64, T)], s: f64) -> Option<(T, T, f64)> {
    let i = line.partition_point(|(t, _)| *t <= s).clamp(1, line.len().saturating_sub(1).max(1));
    let ((s0, a), (s1, b)) = (*line.get(i - 1)?, *line.get(i)?);
    Some((a, b, if s1 > s0 { ((s - s0) / (s1 - s0)).clamp(0.0, 1.0) } else { 0.0 }))
}

fn on_segment(a: P3, b: P3, p: P3) -> (f64, f64) {
    let ab = sub(b, a);
    let share = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-18)).clamp(0.0, 1.0);
    let q = sub(p, add(a, mul(ab, share)));
    (share, dot(q, q))
}

impl Surface {
    /// Its two parameters at `p`, as ISO 10303-42 orders them: a plane's x and y; a turned surface's angle and length along
    /// its meridian; an extruded one's length along its curve and distance along its direction.
    fn param(&self, p: P3) -> (f64, f64) {
        match self {
            Surface::Plane(f) => {
                let d = sub(p, f.o);
                (dot(d, f.x), dot(d, f.y))
            }
            Surface::Turned(f, m) => {
                let d = sub(p, f.o);
                let along = dot(d, f.z);
                let w = sub(d, mul(f.z, along));
                (dot(w, f.y).atan2(dot(w, f.x)), nearest_on(m, [along, dot(w, w).sqrt(), 0.0], on_segment))
            }
            Surface::Extruded(c, along) => {
                let flat = |q: P3| sub(q, mul(*along, dot(q, *along)));
                let s = nearest_on(c, p, |a, b, q| on_segment(flat(a), flat(b), flat(q)));
                (s, dot(sub(p, self.at((s, 0.0))), *along))
            }
        }
    }

    fn at(&self, (u, v): (f64, f64)) -> P3 {
        match self {
            Surface::Plane(f) => f.at([u, v, 0.0]),
            Surface::Turned(f, m) => {
                let Some((a, b, k)) = between(m, v) else { return f.o };
                let (along, out) = (a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k);
                f.at([out * u.cos(), out * u.sin(), along])
            }
            Surface::Extruded(c, along) => {
                let Some((a, b, k)) = between(c, u) else { return [0.0; 3] };
                add(add(a, mul(sub(b, a), k)), mul(*along, v))
            }
        }
    }

    /// Its normal at parameters `q`, as ISO 10303-42 has it: the first parameter's way across the second's.
    fn normal(&self, q: (f64, f64)) -> P3 {
        if let Surface::Plane(f) = self {
            return f.z;
        }
        let h = match self {
            Surface::Turned(..) => (1e-4, 1e-2),
            _ => (1e-2, 1e-2),
        };
        let du = sub(self.at((q.0 + h.0, q.1)), self.at((q.0 - h.0, q.1)));
        let dv = sub(self.at((q.0, q.1 + h.1)), self.at((q.0, q.1 - h.1)));
        cross(du, dv)
    }
}

/// How a curved face's parameters lie flat for triangulating: a turned surface's as polar coordinates, angle and `base`
/// plus length along the meridian from `least`; any other's as they are, less `least` and the other's least.
struct Flat {
    polar: bool,
    least: (f64, f64),
    base: f64,
}

impl Flat {
    fn to(&self, (u, v): (f64, f64)) -> P2 {
        if self.polar {
            let r = self.base + v - self.least.1;
            P2(r * u.cos(), r * u.sin())
        } else {
            P2(u - self.least.0, v - self.least.1)
        }
    }

    fn from(&self, p: P2) -> (f64, f64) {
        if self.polar { (p.1.atan2(p.0), p.0.hypot(p.1) - self.base + self.least.1) } else { (p.0 + self.least.0, p.1 + self.least.1) }
    }
}

/// Whether `p` is inside `ring`, by the crossings of a ray east.
fn inside(ring: &[P2], p: P2) -> bool {
    let mut inside = false;
    for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
}

/// How far `p` is from the nearest edge of `ring`.
fn off(ring: &[P2], p: P2) -> f64 {
    let flat = |q: P2| [q.0, q.1, 0.0];
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| on_segment(flat(*a), flat(*b), flat(p)).1.sqrt())
        .fold(f64::INFINITY, f64::min)
}

fn area(ring: &[P2]) -> f64 {
    ring.iter().zip(ring.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>() / 2.0
}

/// A face on `surface` within `loops` (the largest its outline, the rest holes) as faces turned the way the surface's
/// normal is, times `sense`: a flat face of one loop as that loop, any other as triangles.
fn tessellate(surface: &Surface, loops: &[Vec<P3>], sense: f64) -> Result<Vec<Vec<P3>>, &'static str> {
    if let (Surface::Plane(f), [only]) = (surface, loops) {
        let mut face = only.clone();
        if dot(newell(&face), f.z) * sense < 0.0 {
            face.reverse();
        }
        return Ok(vec![face]);
    }
    if loops.iter().map(Vec::len).sum::<usize>() > MAX_ROUND {
        return Err(UNTRIANGULATED);
    }
    let params: Vec<Vec<(f64, f64)>> = loops.iter().map(|l| l.iter().map(|p| surface.param(*p)).collect()).collect();
    let all = || params.iter().flatten();
    let least = (all().map(|q| q.0).fold(f64::INFINITY, f64::min), all().map(|q| q.1).fold(f64::INFINITY, f64::min));
    let most = all().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max);
    if !(least.0.is_finite() && least.1.is_finite() && most.is_finite()) {
        return Err(UNTRIANGULATED);
    }
    let polar = matches!(surface, Surface::Turned(..));
    if polar {
        // A loop round the axis with no other round it bounds a face to the end of its axis, which has no ring to
        // triangulate.
        let windings = params
            .iter()
            .filter(|l| {
                let turned: f64 = l.iter().zip(l.iter().cycle().skip(1)).map(|(a, b)| (b.0 - a.0 + PI).rem_euclid(TAU) - PI).sum();
                turned.abs() > PI
            })
            .count();
        if windings % 2 == 1 {
            return Err(UNTRIANGULATED);
        }
    }
    let base = match surface {
        Surface::Turned(f, _) => {
            let mean = loops.iter().flatten().map(|p| {
                let d = sub(*p, f.o);
                let w = sub(d, mul(f.z, dot(d, f.z)));
                dot(w, w).sqrt()
            });
            let n = loops.iter().map(Vec::len).sum::<usize>().max(1) as f64;
            (mean.sum::<f64>() / n).max(most - least.1).max(1000.0)
        }
        _ => 0.0,
    };
    let flat = Flat { polar, least, base };
    let mut rings: Vec<Vec<P2>> = params.iter().map(|l| l.iter().map(|q| flat.to(*q)).collect()).collect();
    rings.sort_by(|a, b| area(b).abs().total_cmp(&area(a).abs()));
    let Some((outline, holes)) = rings.split_first() else { return Ok(Vec::new()) };
    // Points inside a curved face a step apart: a fifteenth of a turn, and as far along a turned surface's meridian or an
    // extruded one's curve and direction.
    let mut points = Vec::new();
    let mut grid = |u: f64, v: f64| {
        let p = flat.to((u, v));
        if points.len() < MAX_INSIDE
            && inside(outline, p)
            && !holes.iter().any(|h| inside(h, p))
            && std::iter::once(outline).chain(holes).all(|r| off(r, p) > 4.0 * TOL)
        {
            points.push(p);
        }
    };
    match surface {
        Surface::Plane(_) => {}
        Surface::Turned(_, m) => {
            let step = (base * PI / 12.0).max(1.0);
            let n = (((most - least.1) / step).ceil() as usize).clamp(1, MAX_ROWS);
            let mut vs: Vec<f64> = (1..n).map(|j| least.1 + (most - least.1) * j as f64 / n as f64).collect();
            vs.extend(m.iter().map(|(s, _)| *s).filter(|s| *s > least.1 && *s < most).take(MAX_ROWS));
            for v in vs {
                for k in 0..24 {
                    grid(f64::from(k) * PI / 12.0, v);
                }
            }
        }
        Surface::Extruded(c, _) => {
            let (u_most, v_most) =
                (all().map(|q| q.0).fold(f64::NEG_INFINITY, f64::max), all().map(|q| q.1).fold(f64::NEG_INFINITY, f64::max));
            for (s, _) in c.iter().filter(|(s, _)| *s > least.0 && *s < u_most).take(MAX_ROWS) {
                for j in 1..4 {
                    grid(*s, least.1 + (v_most - least.1) * f64::from(j) / 4.0);
                }
            }
        }
    }
    let t = triangulation::constrained(outline, holes, &points, &[]).map_err(|_| UNTRIANGULATED)?;
    let mut out = Vec::with_capacity(t.triangles.len());
    for [i, j, k] in t.triangles {
        let (Some(a), Some(b), Some(c)) = (t.points.get(i), t.points.get(j), t.points.get(k)) else { continue };
        let tri = [*a, *b, *c].map(|p| surface.at(flat.from(p)));
        let mid = flat.from(P2((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0));
        let facing = dot(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0])), surface.normal(mid)) * sense;
        out.push(if facing < 0.0 { vec![tri[0], tri[2], tri[1]] } else { tri.to_vec() });
    }
    Ok(out)
}

impl Reader<'_> {
    /// Notes why the item being read builds nothing, or less than it holds: the first reason given.
    pub(super) fn give_up(&self, why: &'static str) {
        if self.why.get().is_none() {
            self.why.set(Some(why));
        }
    }

    /// The profile of swept area solid `e`, in convex pieces where it has holes.
    fn pieces(&self, e: &Entity, depth: usize) -> Vec<Vec<P2>> {
        let Some(outer) = self.profile(reference(e.args.first()), depth + 1).filter(|r| r.len() >= 3) else { return Vec::new() };
        self.voids(reference(e.args.first()), depth + 1).iter().fold(vec![outer], |parts, v| walls::minus(&parts, v))
    }

    /// A profile turned about an axis in its plane through its angle (`IfcRevolvedAreaSolid`): a ring a step, a whole turn
    /// closed on itself. A positive angle turns it against the right hand, as IfcOpenShell reads it (about y, x toward z).
    pub(super) fn revolved(&self, e: &Entity, depth: usize, out: &mut Vec<Vec<P3>>) {
        let a = |i: usize| e.args.get(i);
        let Some(axis) = self.at(a(2)) else { return };
        let o = self.point(reference(axis.args.first())).unwrap_or_default();
        let d = self.direction(reference(axis.args.get(1))).unwrap_or([0.0, 0.0, 1.0]);
        let Some(angle) = real(a(3)).map(|t| (t * self.rad).clamp(-TAU, TAU)).filter(|t| t.abs() > 1e-9) else { return };
        let whole = angle.abs() > TAU - 1e-6;
        let steps = (angle.abs() / TAU * TURN).ceil().max(1.0) as usize;
        let pos = self.axes(reference(a(1)));
        let pieces = self.pieces(e, depth);
        if out.len() + pieces.iter().map(|p| (p.len() + 2) * steps).sum::<usize>() > MAX_FACES {
            return self.give_up(TOO_MANY);
        }
        for piece in pieces {
            let rings: Vec<Vec<P3>> = (0..if whole { steps } else { steps + 1 })
                .map(|k| {
                    let t = -angle * k as f64 / steps as f64;
                    piece.iter().map(|p| pos.at(turn([p.0, p.1, 0.0], o, d, t))).collect()
                })
                .collect();
            loft(out, &rings, whole);
        }
    }

    /// A profile swept along a curve (`IfcFixedReferenceSweptAreaSolid`, `IfcSurfaceCurveSweptAreaSolid`): square to it, its
    /// x toward the fixed direction or the normal of the plane the curve lies on, mitred where it bends.
    pub(super) fn along(&self, e: &Entity, depth: usize, out: &mut Vec<Vec<P3>>) {
        let a = |i: usize| e.args.get(i);
        let toward = if e.name == "IFCSURFACECURVESWEPTAREASOLID" {
            match self.at(a(5)) {
                Some(s) if s.name == "IFCPLANE" => self.axes(reference(s.args.first())).z,
                _ => return self.give_up(NOT_PLANE),
            }
        } else {
            self.direction(reference(a(5))).unwrap_or([1.0, 0.0, 0.0])
        };
        let Some((line, smooth)) = self.directrix(reference(a(2)), real(a(3)), real(a(4)), depth + 1) else { return };
        let pos = self.axes(reference(a(1)));
        let pieces = self.pieces(e, depth);
        if out.len() + pieces.iter().map(|p| (p.len() + 2) * line.len()).sum::<usize>() > MAX_FACES {
            return self.give_up(TOO_MANY);
        }
        for piece in pieces {
            for (rings, closed) in mitred(&piece, &line, toward, smooth) {
                let placed: Vec<Vec<P3>> = rings.into_iter().map(|r| r.into_iter().map(|p| pos.at(p)).collect()).collect();
                loft(out, &placed, closed);
            }
        }
    }

    /// The points of curve `id` a sweep follows, from parameter `start` to `end` where given, and whether they are a smooth
    /// curve's drawn as chords (a circle's, an ellipse's, an arc of one, a B-spline's); a closed one ends where it starts.
    fn directrix(&self, id: Option<u64>, start: Option<f64>, end: Option<f64>, depth: usize) -> Option<(Vec<P3>, bool)> {
        let e = self.get(id?)?;
        let conic = matches!(e.name.as_str(), "IFCCIRCLE" | "IFCELLIPSE");
        let smooth = conic
            || e.name.contains("BSPLINECURVE")
            || (e.name == "IFCTRIMMEDCURVE"
                && self.at(e.args.first()).is_some_and(|b| matches!(b.name.as_str(), "IFCCIRCLE" | "IFCELLIPSE")));
        let pts = self.points_between(e, id, start, end, depth)?;
        Some((pts, smooth))
    }

    fn points_between(&self, e: &Entity, id: Option<u64>, start: Option<f64>, end: Option<f64>, depth: usize) -> Option<Vec<P3>> {
        let conic = matches!(e.name.as_str(), "IFCCIRCLE" | "IFCELLIPSE");
        if start.is_none() && end.is_none() {
            let mut pts = self.curve(id, depth)?;
            if conic && let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            return Some(pts);
        }
        let lines_only = list(e.args.get(1)).iter().all(|s| matches!(s, V::Typed(n, _) if n == "IFCLINEINDEX"));
        match e.name.as_str() {
            "IFCPOLYLINE" | "IFCINDEXEDPOLYCURVE" if e.name == "IFCPOLYLINE" || lines_only => {
                let pts = self.curve(id, depth)?;
                let last = pts.len().checked_sub(1)? as f64;
                part(&pts, start.unwrap_or(0.0).clamp(0.0, last), end.unwrap_or(last).clamp(0.0, last))
            }
            _ if conic => {
                let (f, ra, rb) = self.conic(e)?;
                let (t0, t1) = (start.map_or(0.0, |t| t * self.rad), end.map_or(TAU, |t| t * self.rad));
                (t1 > t0).then(|| arc(&f, ra, rb, t0, (t1 - t0).min(TAU)))
            }
            "IFCTRIMMEDCURVE" => {
                // Its trims as parameters: a sweep from the one to the other is all of it.
                let trim = |v: Option<&V>| {
                    list(v).iter().find_map(|t| match t {
                        V::Typed(_, p) => real(Some(p)),
                        _ => None,
                    })
                };
                let (a, b) = (trim(e.args.get(1)), trim(e.args.get(2)));
                let same =
                    |given: Option<f64>, t: Option<f64>| given.is_none_or(|g| t.is_some_and(|t| (g - t).abs() <= 1e-9 * (1.0 + t.abs())));
                if (same(start, a) && same(end, b)) || (same(start, b) && same(end, a)) {
                    return self.curve(id, depth);
                }
                self.give_up(PARAMETERS);
                None
            }
            _ => {
                self.give_up(PARAMETERS);
                None
            }
        }
    }

    /// A circle's or an ellipse's axes and radii.
    pub(super) fn conic(&self, e: &Entity) -> Option<(Frame, f64, f64)> {
        let f = self.axes(reference(e.args.first()));
        let ra = self.len(e.args.get(1)).filter(|r| *r > 0.0)?;
        let rb = if e.name == "IFCELLIPSE" { self.len(e.args.get(2)).filter(|r| *r > 0.0)? } else { ra };
        Some((f, ra, rb))
    }

    /// A B-spline curve's points (`IfcBSplineCurveWithKnots`, rational or not): `SPAN` a knot span, a curve of degree 1 at
    /// its control points; none where its knots don't fit its control points.
    pub(super) fn bspline(&self, e: &Entity) -> Option<Vec<P3>> {
        let a = |i: usize| e.args.get(i);
        let degree = match a(0) {
            Some(V::Int(d)) => usize::try_from(*d).ok().filter(|d| (1..=MAX_DEGREE).contains(d))?,
            _ => return None,
        };
        let ids = refs(a(1));
        if ids.len() <= degree || ids.len() > MAX_POINTS {
            return None;
        }
        let ctrl: Vec<P3> = ids.into_iter().map(|p| self.point(Some(p))).collect::<Option<_>>()?;
        let weights: Vec<f64> = if e.name == "IFCRATIONALBSPLINECURVEWITHKNOTS" {
            list(a(8)).iter().map(|w| real(Some(w))).collect::<Option<_>>()?
        } else {
            vec![1.0; ctrl.len()]
        };
        if weights.len() != ctrl.len() || weights.iter().any(|w| *w <= 0.0) {
            return None;
        }
        let multiplicities = indices(a(5));
        let values: Vec<f64> = list(a(6)).iter().map(|k| real(Some(k))).collect::<Option<_>>()?;
        if multiplicities.len() != values.len() {
            return None;
        }
        let wanted = ctrl.len() + degree + 1;
        let mut knots = Vec::with_capacity(wanted);
        for (m, k) in multiplicities.iter().zip(&values) {
            if knots.len() + m > wanted {
                return None;
            }
            knots.extend(std::iter::repeat_n(*k, *m));
        }
        if knots.len() != wanted || knots.windows(2).any(|w| w[1] < w[0]) {
            return None;
        }
        let h: Vec<[f64; 4]> = ctrl.iter().zip(&weights).map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w]).collect();
        let each = if degree == 1 { 1 } else { SPAN };
        let mut out = Vec::new();
        let mut last = None;
        for span in degree..ctrl.len() {
            let (u0, u1) = (*knots.get(span)?, *knots.get(span + 1)?);
            if u1 <= u0 {
                continue;
            }
            for k in 0..each {
                out.push(de_boor(&knots, &h, degree, span, u0 + (u1 - u0) * k as f64 / each as f64)?);
            }
            if out.len() > MAX_POINTS {
                return None;
            }
            last = Some((span, u1));
        }
        let (span, end) = last?;
        out.push(de_boor(&knots, &h, degree, span, end)?);
        Some(out)
    }

    /// An advanced B-rep (`IfcAdvancedBrep`, with voids or not): each face of its shells tessellated on its surface, all
    /// turned out.
    pub(super) fn advanced(&self, e: &Entity, out: &mut Vec<Vec<P3>>) {
        let mut shells = vec![reference(e.args.first())];
        if e.name == "IFCADVANCEDBREPWITHVOIDS" {
            shells.extend(refs(e.args.get(1)).into_iter().map(Some));
        }
        let mut faces = Vec::new();
        for f in shells.into_iter().flatten().filter_map(|s| self.get(s)).flat_map(|s| refs(s.args.first())) {
            let Some(face) = self.get(f) else { continue };
            match self.advanced_face(face) {
                Ok(more) => faces.extend(more),
                Err(why) => return self.give_up(why),
            }
            if out.len() + faces.len() > MAX_FACES {
                return self.give_up(TOO_MANY);
            }
        }
        turned_out(&mut faces);
        out.extend(faces);
    }

    /// An advanced face (`IfcAdvancedFace`) tessellated on its surface, turned the way its sense says.
    pub(super) fn advanced_face(&self, face: &Entity) -> Result<Vec<Vec<P3>>, &'static str> {
        let surface = self.surface(self.at(face.args.get(1)))?;
        let sense = if enumeration(face.args.get(2)) == Some("F") { -1.0 } else { 1.0 };
        let bounds: Vec<&Entity> = refs(face.args.first()).into_iter().filter_map(|b| self.get(b)).collect();
        let mut loops = Vec::new();
        for bound in &bounds {
            if let Some(lp) = self.at(bound.args.first()) {
                loops.extend(self.face_loops(lp)?);
            }
        }
        loops.retain(|l| l.len() >= 3);
        // Bounded by seams alone, it is all of a closed surface.
        if loops.is_empty() && !bounds.is_empty() {
            return Err(UNTRIANGULATED);
        }
        tessellate(&surface, &loops, sense)
    }

    /// The loops round a face of loop `lp`: its points in order, an edge it goes along both ways (a seam, where a closed
    /// surface meets itself) parting it into the loops between.
    fn face_loops(&self, lp: &Entity) -> Result<Vec<Vec<P3>>, &'static str> {
        match lp.name.as_str() {
            "IFCPOLYLOOP" => Ok(vec![ring(refs(lp.args.first()).into_iter().filter_map(|p| self.point(Some(p))).collect())]),
            "IFCEDGELOOP" => {
                let edges: Vec<&Entity> = refs(lp.args.first()).into_iter().filter_map(|x| self.get(x)).collect();
                let element = |oe: &Entity| reference(oe.args.get(2));
                let mut count: HashMap<u64, usize> = HashMap::new();
                for x in edges.iter().filter_map(|oe| element(oe)) {
                    *count.entry(x).or_default() += 1;
                }
                let mut runs: Vec<Vec<P3>> = vec![Vec::new()];
                for oe in &edges {
                    if element(oe).is_some_and(|x| count.get(&x).is_some_and(|n| *n > 1)) {
                        runs.push(Vec::new());
                        continue;
                    }
                    let pts = self.edge_points(oe)?;
                    if let Some(run) = runs.last_mut() {
                        run.extend(pts);
                    }
                }
                // The run before the first seam goes on from where the last one ends.
                if runs.len() > 1 {
                    let first = runs.remove(0);
                    if let Some(last) = runs.last_mut() {
                        last.extend(first);
                    }
                }
                Ok(runs.into_iter().map(ring).filter(|r| r.len() >= 3).collect())
            }
            // A vertex loop: a point, no area.
            _ => Ok(Vec::new()),
        }
    }

    /// The points along an oriented edge, from its start to its end.
    fn edge_points(&self, oe: &Entity) -> Result<Vec<P3>, &'static str> {
        let (edge, forward) = match oe.name.as_str() {
            "IFCORIENTEDEDGE" => (self.at(oe.args.get(2)), enumeration(oe.args.get(3)) != Some("F")),
            _ => (Some(oe), true),
        };
        let Some(edge) = edge else { return Err(EDGE) };
        let vertex = |v: Option<&V>| self.at(v).and_then(|v| self.point(reference(v.args.first())));
        let (Some(s), Some(t)) = (vertex(edge.args.first()), vertex(edge.args.get(1))) else { return Err(EDGE) };
        let mut pts = match edge.name.as_str() {
            "IFCEDGE" => vec![s, t],
            "IFCEDGECURVE" => {
                let same = enumeration(edge.args.get(3)) != Some("F");
                let geometry = self.at(edge.args.get(2)).ok_or(EDGE)?;
                match geometry.name.as_str() {
                    "IFCLINE" => vec![s, t],
                    "IFCCIRCLE" | "IFCELLIPSE" => {
                        let (f, ra, rb) = self.conic(geometry).ok_or(EDGE)?;
                        let angle = |p: P3| {
                            let d = sub(p, f.o);
                            (dot(d, f.y) / rb).atan2(dot(d, f.x) / ra)
                        };
                        let (a0, a1) = (angle(s), angle(t));
                        let whole = |x: f64| if x < 1e-9 { TAU } else { x };
                        let sweep = if same { whole((a1 - a0).rem_euclid(TAU)) } else { -whole((a0 - a1).rem_euclid(TAU)) };
                        arc(&f, ra, rb, a0, sweep)
                    }
                    _ => {
                        let mut pts = self.curve(reference(edge.args.get(2)), 0).ok_or(EDGE)?;
                        if !same {
                            pts.reverse();
                        }
                        pts
                    }
                }
            }
            _ => return Err(EDGE),
        };
        if !forward {
            pts.reverse();
        }
        Ok(pts)
    }

    /// The points of a swept surface's curve (a profile's), placed in the world by `pos`.
    fn swept_curve(&self, v: Option<&V>, pos: &Frame) -> Option<Vec<P3>> {
        let profile = self.at(v)?;
        let pts = self.curve(reference(profile.args.get(2)), 1)?;
        (pts.len() <= MAX_POINTS).then(|| pts.into_iter().map(|p| pos.at(p)).collect())
    }

    /// The surface a face lies on, where BIMCraft tessellates it.
    fn surface(&self, e: Option<&Entity>) -> Result<Surface, &'static str> {
        let e = e.ok_or(SURFACE)?;
        let a = |i: usize| e.args.get(i);
        let turned = |f: Frame, m: &[(f64, f64)]| {
            let pts: Vec<P3> = m.iter().map(|(a, r)| [*a, *r, 0.0]).collect();
            let m = lengths(&pts, |p: P3, q: P3| dot(sub(p, q), sub(p, q)).sqrt());
            if m.is_empty() { Err(SURFACE) } else { Ok(Surface::Turned(f, m)) }
        };
        match e.name.as_str() {
            "IFCPLANE" => Ok(Surface::Plane(self.axes(reference(a(0))))),
            "IFCCYLINDRICALSURFACE" => {
                let r = self.len(a(1)).filter(|r| *r > 0.0).ok_or(SURFACE)?;
                turned(self.axes(reference(a(0))), &[(-MAX_COORD, r), (MAX_COORD, r)])
            }
            "IFCSPHERICALSURFACE" => {
                let r = self.len(a(1)).filter(|r| *r > 0.0).ok_or(SURFACE)?;
                let m: Vec<(f64, f64)> =
                    (0..=24).map(|k| -PI / 2.0 + PI * f64::from(k) / 24.0).map(|t| (r * t.sin(), r * t.cos())).collect();
                turned(self.axes(reference(a(0))), &m)
            }
            "IFCSURFACEOFREVOLUTION" => {
                let pos = self.axes(reference(a(1)));
                let axis = self.at(a(2)).ok_or(SURFACE)?;
                let o = pos.at(self.point(reference(axis.args.first())).unwrap_or_default());
                let f = orthonormal(o, pos.x, pos.dir(self.direction(reference(axis.args.get(1))).unwrap_or([0.0, 0.0, 1.0])));
                let curve = self.swept_curve(a(0), &pos).ok_or(SURFACE)?;
                let m: Vec<(f64, f64)> = curve
                    .iter()
                    .map(|p| {
                        let d = sub(*p, f.o);
                        let along = dot(d, f.z);
                        let w = sub(d, mul(f.z, along));
                        (along, dot(w, w).sqrt())
                    })
                    .collect();
                turned(f, &m)
            }
            "IFCSURFACEOFLINEAREXTRUSION" => {
                let pos = self.axes(reference(a(1)));
                let curve = self.swept_curve(a(0), &pos).ok_or(SURFACE)?;
                let along = self.direction(reference(a(2))).map(|d| unit(pos.dir(d))).ok_or(SURFACE)?;
                let c = lengths(&curve, |p: P3, q: P3| dot(sub(p, q), sub(p, q)).sqrt());
                if c.is_empty() { Err(SURFACE) } else { Ok(Surface::Extruded(c, along)) }
            }
            n if n.contains("BSPLINESURFACE") || n == "IFCBEZIERSURFACE" => Err(BSPLINE),
            _ => Err(SURFACE),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_of_too_many_faces_names_the_cap() {
        assert!(TOO_MANY.contains(&MAX_FACES.to_string()));
    }

    #[test]
    fn a_knot_vector_that_does_not_fit_its_control_points_reads_no_curve() {
        let text = "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCCARTESIANPOINT((1.,0.,0.));
#3=IFCCARTESIANPOINT((1.,1.,0.));
#4=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,2),(0.,1.),.UNSPECIFIED.);
#5=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);
#6=IFCBSPLINECURVEWITHKNOTS(99999999999,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);
#7=IFCBSPLINECURVEWITHKNOTS(2,(#1,#2,#3),.UNSPECIFIED.,.F.,.F.,(3,3),(1.,0.),.UNSPECIFIED.);
ENDSEC;END-ISO-10303-21;";
        let file = crate::step::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        let r = Reader::new(&file, 1.0, 1.0, P2(0.0, 0.0));
        let read = |id: u64| r.get(id).and_then(|e| r.bspline(e));
        assert!(read(4).is_none() && read(6).is_none() && read(7).is_none());
        let pts = read(5).unwrap_or_default();
        assert_eq!(pts.len(), SPAN + 1);
        assert_eq!((pts.first().copied(), pts.last().copied()), (Some([0.0, 0.0, 0.0]), Some([1.0, 1.0, 0.0])));
    }
}
