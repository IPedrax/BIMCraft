//! IFC import: an IFC2x3 or IFC4 file, as Revit, other BIM tools and BIMCraft itself write them,
//! read into a model. Storeys become levels. Walls, straight or curved, and the doors and windows
//! in them, floors (level or sloped), ceilings, rooms, columns, grids, stairs and ramps of straight
//! flights with landings between them, railings that have an axis, and roofs (flat, or pitched
//! over a rectangle) come in as BIMCraft elements, each measured from its
//! geometry: an extruded profile, a Brep or a triangle mesh alike, so how a file describes an
//! element doesn't matter. A wall cut to a roof over it comes in attached to it; the openings
//! through floors, ceilings and roofs, as shafts.
//! What BIMCraft has no element for, or what breaks one of its rules, is left out and listed.
//! Phases and design options come back from the property sets BIMCraft writes them in, `ifc::PHASING`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::PI;
use std::sync::Arc;

use crate::commands::{micron, next_number, tidy};
use crate::family::{Category, Expr, Family, Form, MAX_TRIANGLES, Parameter};
use crate::geom::{self, P2};
use crate::library::Kind;
use crate::model::{
    Ceiling, CeilingType, Column, ColumnType, Component, Element, Extrusion, FIRST_ID, Floor, FloorType, Function, Grid, Guid, Id, Justify,
    Landing, Layer, Level, MAX_COORD, MAX_LAYERS, MAX_POINTS, MAX_PROJECT_PARAMETERS, MAX_TEXT, Mass, Model, Opening, OpeningType,
    PARAMETER_CATEGORIES, ParameterKind, ProjectParameter, Railing, RailingType, Ramp, Roof, RoofType, Room, Run, RunType, Shaft, Side,
    Stair, TOL, Wall, WallType, culprit, heading,
};
use crate::phasing;
use crate::stairs;
use crate::step::{self, Entity, File, V};
use crate::view3d::{P3, add, cross, dot, mul, sub, unit};
use crate::walls::{self, Axis, Plane};
use serde_json::json;

/// The most project parameter values an import reads: a set related to many elements hands each
/// all its values.
const MAX_VALUES: usize = 1_000_000;

/// A model read from an IFC file: its schema, what was left out and why, and how far it was moved
/// to bring it near the origin.
pub struct Imported {
    pub model: Model,
    pub schema: String,
    pub skipped: Vec<String>,
    pub moved: Option<P2>,
}

fn real(v: Option<&V>) -> Option<f64> {
    let x = match v? {
        V::Real(x) => *x,
        V::Int(i) => *i as f64,
        V::Typed(_, inner) => return real(Some(inner)),
        _ => return None,
    };
    x.is_finite().then_some(x)
}

fn text(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Str(s) => Some(s.trim()).filter(|s| !s.is_empty()),
        V::Typed(_, inner) => text(Some(inner)),
        _ => None,
    }
}

fn reference(v: Option<&V>) -> Option<u64> {
    match v? {
        V::Ref(r) => Some(*r),
        _ => None,
    }
}

fn list(v: Option<&V>) -> &[V] {
    match v {
        Some(V::List(l)) => l,
        _ => &[],
    }
}

fn refs(v: Option<&V>) -> Vec<u64> {
    list(v).iter().filter_map(|x| reference(Some(x))).collect()
}

fn enumeration(v: Option<&V>) -> Option<&str> {
    match v? {
        V::Enum(e) => Some(e),
        _ => None,
    }
}

/// A list of whole numbers: indices, 1 first.
fn indices(v: Option<&V>) -> Vec<usize> {
    match v {
        Some(V::Typed(_, inner)) => indices(Some(inner)),
        Some(V::List(l)) => l
            .iter()
            .filter_map(|x| match x {
                V::Int(i) => usize::try_from(*i).ok(),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `text` cut to what a name may hold.
fn clip(text: &str) -> String {
    text.trim().chars().take(MAX_TEXT).collect()
}

/// A number as a name shows it: to the hundredth, without trailing zeros.
fn num(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0 + 0.0)
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    o: P3,
    x: P3,
    y: P3,
    z: P3,
}

const WORLD: Frame = Frame { o: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], z: [0.0, 0.0, 1.0] };

impl Frame {
    fn at(&self, p: P3) -> P3 {
        add(self.o, self.dir(p))
    }

    fn dir(&self, d: P3) -> P3 {
        add(add(mul(self.x, d[0]), mul(self.y, d[1])), mul(self.z, d[2]))
    }

    /// `local`, given in this frame's terms, in its parent's.
    fn then(&self, local: Frame) -> Frame {
        Frame { o: self.at(local.o), x: self.dir(local.x), y: self.dir(local.y), z: self.dir(local.z) }
    }
}

/// Axes at `o`: along `z`, and `x` turned square to it.
fn orthonormal(o: P3, x: P3, z: P3) -> Frame {
    let z = if dot(z, z) > 1e-18 { unit(z) } else { [0.0, 0.0, 1.0] };
    let square = |v: P3| sub(v, mul(z, dot(v, z)));
    let x = if dot(square(x), square(x)) > 1e-12 {
        unit(square(x))
    } else {
        unit(square(if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }))
    };
    Frame { o, x, y: cross(z, x), z }
}

/// A polygon's normal, by Newell's method: as long as the polygon's area, twice over.
fn newell(f: &[P3]) -> P3 {
    f.iter().zip(f.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)))
}

fn normal(f: &[P3]) -> P3 {
    let n = newell(f);
    if dot(n, n) > 1e-12 { unit(n) } else { [0.0; 3] }
}

fn area3(f: &[P3]) -> f64 {
    dot(newell(f), newell(f)).sqrt() / 2.0
}

/// A ring and its copy `up` from it, as faces turned out.
fn prism(out: &mut Vec<Vec<P3>>, mut bottom: Vec<P3>, up: P3) {
    // Counter-clockwise round `up`, so its faces turn out whichever way its profile ran.
    if dot(newell(&bottom), up) < 0.0 {
        bottom.reverse();
    }
    let top: Vec<P3> = bottom.iter().map(|p| add(*p, up)).collect();
    for ((a, b), (c, d)) in bottom.iter().zip(&top).zip(bottom.iter().zip(&top).cycle().skip(1)) {
        out.push(vec![*a, *c, *d, *b]);
    }
    out.push(bottom.into_iter().rev().collect());
    out.push(top);
}

/// Points less repeats in a row and the closing repeat of the first.
fn open(mut pts: Vec<P2>) -> Vec<P2> {
    pts.dedup_by(|a, b| a.dist(*b) < 1e-6);
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) < 1e-6) {
        pts.pop();
    }
    pts
}

/// The loops the edges of `faces` no two share make, each face turned counter-clockwise in plan: a
/// region's outline counter-clockwise, its holes clockwise. An edge with another face's corner on it
/// is two edges, there. Each loop starts at its south-west corner, and they come in that order: as
/// the same file reads the same.
fn loops<'a>(faces: impl Iterator<Item = &'a Vec<P3>>) -> Vec<Vec<P2>> {
    // Points by y, then x: in order, south-west first.
    type K = (i64, i64);
    let key = |p: P2| -> K { ((p.1 * 10.0).round() as i64, (p.0 * 10.0).round() as i64) };
    let rings: Vec<Vec<P2>> = faces
        .map(|f| {
            let ring: Vec<P2> = f.iter().map(|p| P2(p[0], p[1])).collect();
            if geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring }
        })
        .collect();
    // ponytail: every corner against every edge, fine for a roof's or a slab's faces.
    let corners: Vec<P2> = rings.iter().flatten().copied().collect();
    let mut edges: BTreeMap<(K, K), P2> = BTreeMap::new();
    for ring in &rings {
        for (a, b) in geom::ring_edges(ring) {
            let mut on: Vec<(f64, P2)> = corners
                .iter()
                .filter(|p| geom::dist_to_segment(**p, a, b) < 0.05 && p.dist(a) > 0.05 && p.dist(b) > 0.05)
                .map(|p| (geom::param_on_segment(*p, a, b), *p))
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            let pts: Vec<P2> = std::iter::once(a).chain(on.into_iter().map(|(_, p)| p)).chain(std::iter::once(b)).collect();
            for w in pts.windows(2) {
                let &[a, b] = w else { continue };
                let (ka, kb) = (key(a), key(b));
                // An edge two faces share runs both ways, and lies inside the outline.
                if ka != kb && edges.remove(&(kb, ka)).is_none() {
                    edges.insert((ka, kb), a);
                }
            }
        }
    }
    let mut next: BTreeMap<K, (K, P2)> = edges.into_iter().map(|((ka, kb), a)| (ka, (kb, a))).collect();
    let mut out = Vec::new();
    while let Some(&start) = next.keys().next() {
        let (mut ring, mut k) = (Vec::new(), start);
        while let Some((kb, a)) = next.remove(&k) {
            ring.push(a);
            k = kb;
        }
        if ring.len() >= 3 {
            out.push(ring);
        }
    }
    out
}

/// The outline of the faces facing up, or down, whose points all lie within half a millimetre of
/// `on`, a level or sloped plane: the largest of their loops, its corners counter-clockwise; and
/// the holes in it, clockwise.
fn outline(faces: &[Vec<P3>], on: Plane, up: bool) -> Option<geom::Region> {
    let picked = faces.iter().filter(|f| {
        let n = normal(f);
        f.len() >= 3 && f.iter().all(|p| (p[2] - on.at(P2(p[0], p[1]))).abs() <= 0.5) && n[2].abs() >= 0.2 && (n[2] > 0.0) == up
    });
    let mut rings: Vec<Vec<P2>> = loops(picked).into_iter().map(|l| geom::corners(&l)).filter(|l| l.len() >= 3).collect();
    rings.sort_by(|a, b| geom::area(b).abs().total_cmp(&geom::area(a).abs()));
    let mut rings = rings.into_iter();
    let mut ring = rings.next()?;
    if geom::area(&ring) < 0.0 {
        ring.reverse();
    }
    let holes = rings.filter(|h| geom::area(h) < 0.0 && h.iter().all(|p| geom::contains(&ring, *p))).collect();
    Some(geom::Region { outer: ring, holes })
}

/// A slab's top, sloped or level: the plane of its largest face turned up, and its outline in
/// plan and the holes in it; and how far its underside, the plane of its largest face turned down,
/// lies below it, plumb.
fn top_of(faces: &[Vec<P3>]) -> Option<(Plane, geom::Region, f64)> {
    let largest = |up: bool| {
        faces
            .iter()
            .filter(|f| f.len() >= 3 && normal(f)[2].abs() > 0.2 && (normal(f)[2] > 0.0) == up)
            .max_by(|a, b| area3(a).total_cmp(&area3(b)))
    };
    let (top, under) = (Plane::of(largest(true)?)?, Plane::of(largest(false)?)?);
    let shape = outline(faces, top, true)?;
    let q = *shape.outer.first()?;
    Some((top, shape, top.at(q) - under.at(q)))
}

/// A point well inside a ring: the middle of the largest of the triangles that make it up.
fn inside(ring: &[P2]) -> Option<P2> {
    let pt = |i: usize| ring.get(i).copied();
    let tris = geom::triangulate(ring).into_iter().filter_map(|[a, b, c]| Some((pt(a)?, pt(b)?, pt(c)?)));
    let size = |(a, b, c): &(P2, P2, P2)| (*b - *a).cross(*c - *a).abs();
    tris.max_by(|x, y| size(x).total_cmp(&size(y))).map(|(a, b, c)| (a + b + c) * (1.0 / 3.0))
}

/// The least and the most of `xs`.
fn span(xs: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    xs.fold(None, |s: Option<(f64, f64)>, x| Some(s.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x)))))
}

/// Whether two rings have the same corners, to the millimetre.
fn same(a: &[P2], b: &[P2]) -> bool {
    a.len() == b.len() && a.iter().all(|p| b.iter().any(|q| q.dist(*p) < 1.0))
}

/// A run found in a body's points.
struct Found {
    start: P3,
    end: P3,
    width: f64,
    depth: f64,
    round: bool,
}

/// The run `pts` make: its centre line along the direction they spread furthest in (the covariance's dominant axis, by
/// power iteration), its section the box round them across it, square to it, with `width` level and `depth` square to that;
/// round where the section's corners are many and all as far out. An axis that stands has its width along east. None for too
/// few points.
fn run_in(pts: &[P3]) -> Option<Found> {
    use crate::view3d::{add, cross, dot, mul, sub, unit};
    if pts.len() < 4 {
        return None;
    }
    let c = mul(pts.iter().fold([0.0; 3], |a, p| add(a, *p)), 1.0 / pts.len() as f64);
    let mut cov = [[0.0; 3]; 3];
    for p in pts {
        let q = sub(*p, c);
        for (i, row) in cov.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                *x += q[i] * q[j];
            }
        }
    }
    let mut d = unit([1.0, 0.7, 0.3]);
    for _ in 0..64 {
        let next = [0, 1, 2].map(|i| (0..3).map(|j| cov[i][j] * d[j]).sum::<f64>());
        if dot(next, next) < 1e-18 {
            return None;
        }
        d = unit(next);
    }
    // Toward the east, else the north, so a run reads the same whichever end the file starts at; an upright one points up, as the
    // iteration's first direction (z positive) leaves it.
    let upright = d[0].hypot(d[1]) < 1e-6;
    if d[0] < -1e-9 || (d[0].abs() <= 1e-9 && d[1] < -1e-9) {
        d = mul(d, -1.0);
    }
    // Upright, its width is along east, as a run stood up draws it.
    let across = if upright { [1.0, 0.0, 0.0] } else { unit([-d[1], d[0], 0.0]) };
    let up = cross(d, across);
    let ext = |axis: P3| span(pts.iter().map(|p| dot(sub(*p, c), axis)));
    let ((t0, t1), (u0, u1), (v0, v1)) = (ext(d)?, ext(across)?, ext(up)?);
    let mid = add(c, add(mul(d, (t0 + t1) / 2.0), add(mul(across, (u0 + u1) / 2.0), mul(up, (v0 + v1) / 2.0))));
    let half = (t1 - t0) / 2.0;
    let q = |p: &P3| sub(*p, mid);
    let radii = span(pts.iter().map(|p| {
        let r = q(p);
        let x = sub(r, mul(d, dot(r, d)));
        dot(x, x).sqrt()
    }));
    let corners: HashSet<(i64, i64)> =
        pts.iter().map(|p| ((dot(q(p), across) * 2.0).round() as i64, (dot(q(p), up) * 2.0).round() as i64)).collect();
    let round = corners.len() >= 12 && radii.is_some_and(|(lo, hi)| hi > 0.0 && (hi - lo) / hi < 0.03);
    let size = |x: f64| (x * 100.0).round() / 100.0;
    let (width, depth) = match radii {
        Some((_, hi)) if round => (size(2.0 * hi), size(2.0 * hi)),
        _ => (size(u1 - u0), size(v1 - v0)),
    };
    (half > 0.0 && width > 0.0 && depth > 0.0).then(|| Found {
        start: sub(mid, mul(d, half)),
        end: add(mid, mul(d, half)),
        width,
        depth,
        round,
    })
}

struct Reader<'a> {
    f: &'a File,
    /// The file's length unit, in mm; its plane angle unit, in radians.
    mm: f64,
    rad: f64,
    /// Taken off every plan point: brings a model placed far out (a site at its map
    /// coordinates) near the origin.
    shift: P2,
}

impl<'a> Reader<'a> {
    fn get(&self, id: u64) -> Option<&'a Entity> {
        self.f.entities.get(&id)
    }

    fn at(&self, v: Option<&V>) -> Option<&'a Entity> {
        self.get(reference(v)?)
    }

    fn len(&self, v: Option<&V>) -> Option<f64> {
        real(v).map(|x| x * self.mm)
    }

    /// A model point in the plan.
    fn flat(&self, p: P3) -> P2 {
        P2(p[0] - self.shift.0, p[1] - self.shift.1)
    }

    fn coords(&self, v: Option<&V>) -> Vec<f64> {
        list(v).iter().filter_map(|x| real(Some(x))).collect()
    }

    fn point(&self, id: Option<u64>) -> Option<P3> {
        match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => Some([x * self.mm, y * self.mm, 0.0]),
            [x, y, z, ..] => Some([x * self.mm, y * self.mm, z * self.mm]),
            _ => None,
        }
    }

    fn direction(&self, id: Option<u64>) -> Option<P3> {
        let d = match self.coords(self.get(id?)?.args.first()).as_slice() {
            [x, y] => [*x, *y, 0.0],
            [x, y, z, ..] => [*x, *y, *z],
            _ => return None,
        };
        (dot(d, d) > 1e-18).then(|| unit(d))
    }

    /// An axis placement, 3D or 2D.
    fn axes(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| reference(e.args.get(i));
        let o = self.point(a(0)).unwrap_or_default();
        match e.name.as_str() {
            "IFCAXIS2PLACEMENT3D" => {
                orthonormal(o, self.direction(a(2)).unwrap_or([1.0, 0.0, 0.0]), self.direction(a(1)).unwrap_or([0.0, 0.0, 1.0]))
            }
            "IFCAXIS2PLACEMENT2D" => orthonormal(o, self.direction(a(1)).unwrap_or([1.0, 0.0, 0.0]), [0.0, 0.0, 1.0]),
            _ => WORLD,
        }
    }

    /// Where a placement puts what it places: its chain of local placements.
    fn placement(&self, id: Option<u64>, depth: usize) -> Frame {
        match id.and_then(|i| self.get(i)) {
            // ponytail: a chain is walked anew for each element; memoize if files run to millions of elements.
            Some(e) if e.name == "IFCLOCALPLACEMENT" && depth < 64 => {
                self.placement(reference(e.args.first()), depth + 1).then(self.axes(reference(e.args.get(1))))
            }
            _ => WORLD,
        }
    }

    /// A Cartesian transformation operator: axes, scaled.
    fn operator(&self, id: Option<u64>) -> Frame {
        let Some(e) = id.and_then(|i| self.get(i)) else { return WORLD };
        let a = |i: usize| e.args.get(i);
        let s = real(a(3)).unwrap_or(1.0);
        let (z, k) = match e.name.as_str() {
            "IFCCARTESIANTRANSFORMATIONOPERATOR3D" => (self.direction(reference(a(4))), [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR3DNONUNIFORM" => {
                (self.direction(reference(a(4))), [s, real(a(5)).unwrap_or(s), real(a(6)).unwrap_or(s)])
            }
            "IFCCARTESIANTRANSFORMATIONOPERATOR2D" => (None, [s, s, s]),
            "IFCCARTESIANTRANSFORMATIONOPERATOR2DNONUNIFORM" => (None, [s, real(a(4)).unwrap_or(s), s]),
            _ => return WORLD,
        };
        let o = self.point(reference(a(2))).unwrap_or_default();
        let f = orthonormal(o, self.direction(reference(a(0))).unwrap_or([1.0, 0.0, 0.0]), z.unwrap_or([0.0, 0.0, 1.0]));
        Frame { o: f.o, x: mul(f.x, k[0]), y: mul(f.y, k[1]), z: mul(f.z, k[2]) }
    }

    fn point_list(&self, id: Option<u64>) -> Vec<P3> {
        let Some(e) = id.and_then(|i| self.get(i)) else { return Vec::new() };
        list(e.args.first())
            .iter()
            .map(|p| {
                let c = self.coords(Some(p));
                let at = |i: usize| c.get(i).copied().unwrap_or(0.0) * self.mm;
                [at(0), at(1), at(2)]
            })
            .collect()
    }

    /// A curve's points in order, in its own coordinates; an arc as short chords.
    fn curve(&self, id: Option<u64>, depth: usize) -> Option<Vec<P3>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        match e.name.as_str() {
            "IFCPOLYLINE" => Some(refs(a(0)).into_iter().filter_map(|p| self.point(Some(p))).collect()),
            "IFCINDEXEDPOLYCURVE" => {
                let pts = self.point_list(reference(a(0)));
                let segments = list(a(1));
                if segments.is_empty() {
                    return Some(pts);
                }
                let mut out: Vec<P3> = Vec::new();
                for i in segments.iter().flat_map(|s| indices(Some(s))) {
                    if let Some(p) = i.checked_sub(1).and_then(|k| pts.get(k))
                        && out.last() != Some(p)
                    {
                        out.push(*p);
                    }
                }
                Some(out)
            }
            "IFCCOMPOSITECURVE" | "IFCCOMPOSITECURVEONSURFACE" => {
                let mut out: Vec<P3> = Vec::new();
                for seg in refs(a(0)).into_iter().filter_map(|s| self.get(s)) {
                    let Some(mut pts) = self.curve(reference(seg.args.get(2)), depth + 1) else { continue };
                    if enumeration(seg.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    for p in pts {
                        if out.last().is_none_or(|q| dot(sub(*q, p), sub(*q, p)) > 1e-12) {
                            out.push(p);
                        }
                    }
                }
                Some(out)
            }
            "IFCTRIMMEDCURVE" => self.trimmed(e),
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(a(0))), self.len(a(1))?);
                Some((0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => None,
        }
    }

    /// A trimmed line, or a trimmed circle as an arc.
    fn trimmed(&self, e: &Entity) -> Option<Vec<P3>> {
        let basis = self.at(e.args.first())?;
        // A trim: a point, or a parameter along the curve.
        let trim = |v: Option<&V>| {
            let mut out: (Option<P3>, Option<f64>) = (None, None);
            for t in list(v) {
                match t {
                    V::Ref(r) => out.0 = out.0.or(self.point(Some(*r))),
                    V::Typed(_, p) => out.1 = out.1.or(real(Some(p))),
                    _ => {}
                }
            }
            out
        };
        let (t1, t2) = (trim(e.args.get(1)), trim(e.args.get(2)));
        match basis.name.as_str() {
            "IFCLINE" => {
                let p0 = self.point(reference(basis.args.first()))?;
                let v = self.at(basis.args.get(1));
                let d = v.and_then(|v| self.direction(reference(v.args.first()))).unwrap_or([1.0, 0.0, 0.0]);
                let k = v.and_then(|v| self.len(v.args.get(1))).unwrap_or(1.0);
                let at = |(p, t): (Option<P3>, Option<f64>)| p.or_else(|| t.map(|t| add(p0, mul(d, k * t))));
                Some(vec![at(t1)?, at(t2)?])
            }
            "IFCCIRCLE" => {
                let (f, r) = (self.axes(reference(basis.args.first())), self.len(basis.args.get(1))?);
                let angle = |(p, t): (Option<P3>, Option<f64>)| {
                    p.map(|p| dot(sub(p, f.o), f.y).atan2(dot(sub(p, f.o), f.x))).or_else(|| t.map(|t| t * self.rad))
                };
                let (a1, a2) = (angle(t1)?, angle(t2)?);
                let ccw = (a2 - a1).rem_euclid(2.0 * PI);
                let sweep = if enumeration(e.args.get(3)) == Some("F") { ccw - 2.0 * PI } else { ccw };
                Some((0..=12).map(|i| a1 + sweep * f64::from(i) / 12.0).map(|t| f.at([r * t.cos(), r * t.sin(), 0.0])).collect())
            }
            _ => Some(vec![t1.0?, t2.0?]),
        }
    }

    /// A profile's outer edge, in its own plane.
    fn profile(&self, id: Option<u64>, depth: usize) -> Option<Vec<P2>> {
        let e = self.get(id?)?;
        if depth > 16 {
            return None;
        }
        let a = |i: usize| e.args.get(i);
        let placed = |pts: Vec<P2>| {
            let pos = self.axes(reference(a(2)));
            pts.into_iter().map(|p| pos.at([p.0, p.1, 0.0])).map(|q| P2(q[0], q[1])).collect::<Vec<_>>()
        };
        let rect = |w: f64, h: f64| vec![P2(-w / 2.0, -h / 2.0), P2(w / 2.0, -h / 2.0), P2(w / 2.0, h / 2.0), P2(-w / 2.0, h / 2.0)];
        let ellipse =
            |rx: f64, ry: f64| (0..24).map(|i| f64::from(i) * PI / 12.0).map(|t| P2(rx * t.cos(), ry * t.sin())).collect::<Vec<P2>>();
        match e.name.as_str() {
            "IFCRECTANGLEPROFILEDEF"
            | "IFCROUNDEDRECTANGLEPROFILEDEF"
            | "IFCRECTANGLEHOLLOWPROFILEDEF"
            | "IFCISHAPEPROFILEDEF"
            | "IFCASYMMETRICISHAPEPROFILEDEF" => Some(placed(rect(self.len(a(3))?, self.len(a(4))?))),
            // Steel sections: their bounding rectangle, depth along y.
            "IFCLSHAPEPROFILEDEF" | "IFCUSHAPEPROFILEDEF" | "IFCCSHAPEPROFILEDEF" | "IFCTSHAPEPROFILEDEF" | "IFCZSHAPEPROFILEDEF" => {
                Some(placed(rect(self.len(a(4))?, self.len(a(3))?)))
            }
            "IFCCIRCLEPROFILEDEF" | "IFCCIRCLEHOLLOWPROFILEDEF" => {
                let r = self.len(a(3))?;
                Some(placed(ellipse(r, r)))
            }
            "IFCELLIPSEPROFILEDEF" => Some(placed(ellipse(self.len(a(3))?, self.len(a(4))?))),
            "IFCARBITRARYCLOSEDPROFILEDEF" | "IFCARBITRARYPROFILEDEFWITHVOIDS" => {
                Some(open(self.curve(reference(a(2)), depth + 1)?.into_iter().map(|p| P2(p[0], p[1])).collect()))
            }
            "IFCDERIVEDPROFILEDEF" => {
                let op = self.operator(reference(a(3)));
                Some(
                    self.profile(reference(a(2)), depth + 1)?.into_iter().map(|p| op.at([p.0, p.1, 0.0])).map(|q| P2(q[0], q[1])).collect(),
                )
            }
            _ => None,
        }
    }

    /// The holes in a profile with voids, in its own plane: as many as have at most [`MAX_POINTS`]
    /// points between them.
    fn voids(&self, id: Option<u64>, depth: usize) -> Vec<Vec<P2>> {
        let Some(e) = id.and_then(|i| self.get(i)).filter(|e| e.name == "IFCARBITRARYPROFILEDEFWITHVOIDS") else { return Vec::new() };
        let mut total = 0;
        refs(e.args.get(3))
            .into_iter()
            .filter_map(|c| self.curve(Some(c), depth + 1))
            .map(|pts| open(pts.into_iter().map(|p| P2(p[0], p[1])).collect()))
            .filter(|h| h.len() >= 3)
            .take_while(|h| {
                total += h.len();
                total <= MAX_POINTS
            })
            .collect()
    }

    /// The faces of representation item `id`, as polygons in its own coordinates.
    fn faces(&self, id: u64, depth: usize, out: &mut Vec<Vec<P3>>) {
        let Some(e) = self.get(id) else { return };
        if depth > 16 {
            return;
        }
        let a = |i: usize| e.args.get(i);
        let polygon = |out: &mut Vec<Vec<P3>>, pts: Vec<P3>| {
            if pts.len() >= 3 {
                out.push(pts);
            }
        };
        match e.name.as_str() {
            "IFCEXTRUDEDAREASOLID" | "IFCEXTRUDEDAREASOLIDTAPERED" => {
                let (Some(ring), Some(length)) = (self.profile(reference(a(0)), depth + 1), self.len(a(3))) else { return };
                let pos = self.axes(reference(a(1)));
                let up = mul(pos.dir(self.direction(reference(a(2))).unwrap_or([0.0, 0.0, 1.0])), length);
                if ring.len() >= 3 {
                    // Round holes in its profile, in convex pieces.
                    let voids = self.voids(reference(a(0)), depth + 1);
                    for piece in voids.iter().fold(vec![ring], |parts, v| walls::minus(&parts, v)) {
                        prism(out, piece.iter().map(|p| pos.at([p.0, p.1, 0.0])).collect(), up);
                    }
                }
            }
            // What a clipping cuts away doesn't change what the element is.
            "IFCBOOLEANCLIPPINGRESULT" | "IFCBOOLEANRESULT" => {
                if let Some(first) = reference(a(1)) {
                    self.faces(first, depth + 1, out);
                }
                if enumeration(a(0)) == Some("UNION")
                    && let Some(second) = reference(a(2))
                {
                    self.faces(second, depth + 1, out);
                }
            }
            "IFCFACETEDBREP" | "IFCFACETEDBREPWITHVOIDS" | "IFCCSGSOLID" => {
                if let Some(inner) = reference(a(0)) {
                    self.faces(inner, depth + 1, out);
                }
            }
            "IFCCLOSEDSHELL" | "IFCOPENSHELL" | "IFCCONNECTEDFACESET" | "IFCSHELLBASEDSURFACEMODEL" | "IFCFACEBASEDSURFACEMODEL" => {
                for f in refs(a(0)) {
                    self.faces(f, depth + 1, out);
                }
            }
            "IFCFACE" | "IFCFACESURFACE" => {
                let bounds: Vec<&Entity> = refs(a(0)).into_iter().filter_map(|b| self.get(b)).collect();
                let outer = bounds.iter().find(|b| b.name == "IFCFACEOUTERBOUND").or(bounds.first());
                if let Some(bound) = outer {
                    let lp = self.at(bound.args.first());
                    let mut pts: Vec<P3> =
                        lp.map_or_else(Vec::new, |l| refs(l.args.first()).into_iter().filter_map(|p| self.point(Some(p))).collect());
                    if enumeration(bound.args.get(1)) == Some("F") {
                        pts.reverse();
                    }
                    polygon(out, pts);
                }
            }
            "IFCTRIANGULATEDFACESET" | "IFCTRIANGULATEDIRREGULARNETWORK" | "IFCPOLYGONALFACESET" => {
                let pts = self.point_list(reference(a(0)));
                let triangles = e.name != "IFCPOLYGONALFACESET";
                let pn = indices(a(if triangles { 4 } else { 3 }));
                let at = |i: usize| {
                    let i = if pn.is_empty() { i } else { i.checked_sub(1).and_then(|k| pn.get(k)).copied()? };
                    i.checked_sub(1).and_then(|k| pts.get(k)).copied()
                };
                let polys: Vec<Vec<usize>> = if triangles {
                    list(a(3)).iter().map(|t| indices(Some(t))).collect()
                } else {
                    refs(a(2)).into_iter().filter_map(|f| self.get(f)).map(|f| indices(f.args.first())).collect()
                };
                for poly in polys {
                    polygon(out, poly.into_iter().filter_map(at).collect());
                }
            }
            "IFCMAPPEDITEM" => {
                let Some(map) = self.at(a(0)) else { return };
                let mut inner = Vec::new();
                if let Some(rep) = self.at(map.args.get(1)) {
                    for item in refs(rep.args.get(3)) {
                        self.faces(item, depth + 1, &mut inner);
                    }
                }
                let f = self.operator(reference(a(1))).then(self.axes(reference(map.args.first())));
                out.extend(inner.into_iter().map(|face| face.into_iter().map(|p| f.at(p)).collect()));
            }
            // A box: its corner or its placement, and its three sizes.
            "IFCBOUNDINGBOX" | "IFCBLOCK" => {
                let pos = if e.name == "IFCBLOCK" {
                    self.axes(reference(a(0)))
                } else {
                    Frame { o: self.point(reference(a(0))).unwrap_or_default(), ..WORLD }
                };
                if let (Some(x), Some(y), Some(z)) = (self.len(a(1)), self.len(a(2)), self.len(a(3))) {
                    prism(
                        out,
                        [[0.0, 0.0, 0.0], [x, 0.0, 0.0], [x, y, 0.0], [0.0, y, 0.0]].map(|p| pos.at(p)).to_vec(),
                        pos.dir([0.0, 0.0, z]),
                    );
                }
            }
            _ => {}
        }
    }

    /// A product's shape representations: their identifiers and items.
    fn reps(&self, product: &Entity) -> Vec<(&'a str, Vec<u64>)> {
        let Some(shape) = self.at(product.args.get(6)) else { return Vec::new() };
        refs(shape.args.get(2))
            .into_iter()
            .filter_map(|r| self.get(r))
            .map(|rep| (text(rep.args.get(1)).unwrap_or(""), refs(rep.args.get(3))))
            .collect()
    }

    fn frame(&self, product: &Entity) -> Frame {
        self.placement(reference(product.args.get(5)), 0)
    }

    /// A product's body as faces, in model coordinates: its Body representation, else any but a
    /// line or a footprint.
    fn body(&self, product: &Entity) -> Vec<Vec<P3>> {
        let f = self.frame(product);
        self.body_items(product).into_iter().flat_map(|(_, faces)| faces).map(|face| face.into_iter().map(|p| f.at(p)).collect()).collect()
    }

    /// A product's body item by item, each its faces in the product's own coordinates: its Body
    /// representation's, else any but a line or a footprint's.
    fn body_items(&self, product: &Entity) -> Vec<(u64, Vec<Vec<P3>>)> {
        let reps = self.reps(product);
        let items = |keep: &dyn Fn(&str) -> bool| {
            reps.iter().filter(|(i, _)| keep(&i.to_ascii_lowercase())).flat_map(|(_, it)| it.clone()).collect::<Vec<_>>()
        };
        let mut chosen = items(&|i| i == "body" || i == "facetation");
        if chosen.is_empty() {
            chosen = items(&|i| !matches!(i, "axis" | "footprint" | "annotation" | "plan" | "profile" | "reference" | "surface"));
        }
        chosen
            .into_iter()
            .map(|item| {
                let mut out = Vec::new();
                self.faces(item, 0, &mut out);
                (item, out)
            })
            .collect()
    }

    /// A product's Axis representation, in model coordinates.
    fn axis(&self, product: &Entity) -> Option<Vec<P3>> {
        let (_, items) = self.reps(product).into_iter().find(|(i, _)| i.eq_ignore_ascii_case("Axis"))?;
        let f = self.frame(product);
        Some(self.curve(items.first().copied(), 0)?.into_iter().map(|p| f.at(p)).collect())
    }
}

/// The project's length unit in mm, and its plane angle unit in radians.
fn units(f: &File) -> Result<(f64, f64), String> {
    fn si(prefix: Option<&str>) -> f64 {
        match prefix {
            Some("EXA") => 1e18,
            Some("PETA") => 1e15,
            Some("TERA") => 1e12,
            Some("GIGA") => 1e9,
            Some("MEGA") => 1e6,
            Some("KILO") => 1e3,
            Some("HECTO") => 1e2,
            Some("DECA") => 1e1,
            Some("DECI") => 1e-1,
            Some("CENTI") => 1e-2,
            Some("MILLI") => 1e-3,
            Some("MICRO") => 1e-6,
            Some("NANO") => 1e-9,
            _ => 1.0,
        }
    }
    // A unit's kind, and its size in SI base units (metres, radians).
    fn unit(f: &File, id: Option<u64>, depth: usize) -> Option<(String, f64)> {
        let e = f.entities.get(&id?)?;
        let kind = enumeration(e.args.get(1)).unwrap_or("").to_string();
        match e.name.as_str() {
            "IFCSIUNIT" => Some((kind, si(enumeration(e.args.get(2))))),
            "IFCCONVERSIONBASEDUNIT" | "IFCCONVERSIONBASEDUNITWITHOFFSET" if depth < 8 => {
                let measure = f.entities.get(&reference(e.args.get(3))?)?;
                let (_, base) = unit(f, reference(measure.args.get(1)), depth + 1)?;
                Some((kind, real(measure.args.first())? * base))
            }
            _ => None,
        }
    }
    // The first project, the one the name comes from.
    let projects = f.entities.iter().filter(|(_, e)| e.name == "IFCPROJECT");
    let (_, project) = projects.min_by_key(|(id, _)| **id).ok_or("it has no IfcProject")?;
    let assignment = reference(project.args.get(8)).and_then(|a| f.entities.get(&a));
    let (mut mm, mut rad) = (1.0, 1.0);
    for u in refs(assignment.and_then(|a| a.args.first())) {
        match unit(f, Some(u), 0) {
            Some((kind, size)) if kind == "LENGTHUNIT" => mm = size * 1000.0,
            Some((kind, size)) if kind == "PLANEANGLEUNIT" => rad = size,
            _ => {}
        }
    }
    if !(1e-6..=1e9).contains(&mm) || !(rad.is_finite() && rad > 0.0) {
        return Err("its length or angle unit isn't one BIMCraft can use".into());
    }
    Ok((mm, rad))
}

/// How elements relate: where each is, what it's part of, what voids and fills what, and its
/// type's name.
#[derive(Default)]
struct Links {
    within: HashMap<u64, u64>,
    whole: HashMap<u64, u64>,
    parts: HashMap<u64, Vec<u64>>,
    /// An opening's host; the opening a door or window fills.
    host: HashMap<u64, u64>,
    fills: HashMap<u64, u64>,
    type_name: HashMap<u64, String>,
    /// An element's type, and what an element or a type is made of.
    typed: HashMap<u64, u64>,
    material: HashMap<u64, u64>,
    /// The group (a system) a product is assigned to.
    group: HashMap<u64, u64>,
}

impl Links {
    fn read(f: &File, ids: &[u64]) -> Links {
        let mut l = Links::default();
        for e in ids.iter().filter_map(|id| f.entities.get(id)) {
            let a = |i: usize| e.args.get(i);
            match e.name.as_str() {
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => {
                    if let Some(s) = reference(a(5)) {
                        l.within.extend(refs(a(4)).into_iter().map(|x| (x, s)));
                    }
                }
                "IFCRELAGGREGATES" => {
                    if let Some(w) = reference(a(4)) {
                        for x in refs(a(5)) {
                            l.whole.insert(x, w);
                            l.parts.entry(w).or_default().push(x);
                        }
                    }
                }
                "IFCRELVOIDSELEMENT" => {
                    if let (Some(h), Some(o)) = (reference(a(4)), reference(a(5))) {
                        l.host.insert(o, h);
                    }
                }
                "IFCRELFILLSELEMENT" => {
                    if let (Some(o), Some(d)) = (reference(a(4)), reference(a(5))) {
                        l.fills.insert(d, o);
                    }
                }
                "IFCRELDEFINESBYTYPE" => {
                    if let Some(name) = reference(a(5)).and_then(|t| f.entities.get(&t)).and_then(|t| text(t.args.get(2))) {
                        l.type_name.extend(refs(a(4)).into_iter().map(|x| (x, name.to_string())));
                    }
                    if let Some(t) = reference(a(5)) {
                        l.typed.extend(refs(a(4)).into_iter().map(|x| (x, t)));
                    }
                }
                "IFCRELASSOCIATESMATERIAL" => {
                    if let Some(mat) = reference(a(5)) {
                        l.material.extend(refs(a(4)).into_iter().map(|x| (x, mat)));
                    }
                }
                "IFCRELASSIGNSTOGROUP" => {
                    if let Some(g) = reference(a(6)) {
                        l.group.extend(refs(a(4)).into_iter().map(|x| (x, g)));
                    }
                }
                _ => {}
            }
        }
        l
    }
}

/// A curved wall's faces from how far its body's points are from its centre: the middle one of the
/// points inside its middle, and of those outside it, so a corner where it joins another (cut a
/// little past the arc) doesn't count.
fn arc_faces(radii: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let mut all: Vec<f64> = radii.filter(|r| r.is_finite()).collect();
    all.sort_by(f64::total_cmp);
    let (lo, hi) = (*all.first()?, *all.last()?);
    let (inner, outer): (Vec<f64>, Vec<f64>) = all.iter().partition(|r| **r < (lo + hi) / 2.0);
    Some((*inner.get(inner.len() / 2)?, *outer.get(outer.len() / 2)?))
}

/// A body's highest point.
fn highest(pts: &[P3]) -> P3 {
    pts.iter().copied().max_by(|a, b| a[2].total_cmp(&b[2])).unwrap_or_default()
}

/// A wall as the file has it, before walls that nearly meet are made to meet.
struct RawWall {
    ifc: u64,
    start: P2,
    end: P2,
    arc: f64,
    t: f64,
    z: (f64, f64),
    /// Where its body is highest, in plan.
    peak: P2,
    level: Guid,
    type_name: String,
}

/// Walls that nearly meet, made to meet: an end within a wall's thickness of another wall's centre
/// line moves onto it, so corners and T-junctions join as BIMCraft joins walls.
fn meet(walls: &mut [RawWall]) {
    for i in 0..walls.len() {
        for at_end in [false, true] {
            // Curved walls stay as the file has them.
            let Some(w) = walls.get(i).filter(|w| w.arc == 0.0) else { continue };
            let (a, b, p, t, z) = (w.start, w.end, if at_end { w.end } else { w.start }, w.t, w.z);
            if walls.iter().enumerate().any(|(j, o)| j != i && (o.start.dist(p) <= TOL || o.end.dist(p) <= TOL)) {
                continue;
            }
            let mut best: Option<(f64, P2)> = None;
            for o in walls.iter().enumerate().filter(|(j, o)| *j != i && o.arc == 0.0 && o.z.0 < z.1 && z.0 < o.z.1).map(|(_, o)| o) {
                if geom::parallel(a, b, o.start, o.end) {
                    continue;
                }
                let Some(x) = geom::line_intersection(a, b, o.start, o.end) else { continue };
                let reach = t.max(o.t);
                let along = (x - o.start).dot((o.end - o.start).unit());
                let d = x.dist(p);
                if d <= reach && (-reach..=o.start.dist(o.end) + reach).contains(&along) && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, x));
                }
            }
            // Never so far that the wall turns round or vanishes.
            if let Some((_, x)) = best
                && let Some(w) = walls.get_mut(i)
            {
                let (s, e) = if at_end { (w.start, x) } else { (x, w.end) };
                if (e - s).dot(b - a) > TOL * (b - a).length() {
                    (w.start, w.end) = (micron(s), micron(e));
                }
            }
        }
    }
}

/// Whether element `id`'s layer set usage stacks its layers up its own axis (`.POSITIVE.`), so its
/// first is at its back: a wall's right, a slab's bottom; none without a usage.
fn stacked_up(r: &Reader, l: &Links, id: u64) -> Option<bool> {
    let usage = l.material.get(&id).and_then(|x| r.get(*x)).filter(|u| u.name == "IFCMATERIALLAYERSETUSAGE")?;
    Some(enumeration(usage.args.get(2)) == Some("POSITIVE"))
}

/// The layers the file gives element `id`, or its type: its layer set's, each its material's name,
/// its thickness, and what it does by its category; none unless they add up to `t` within a
/// millimetre, nor where they wouldn't make a type.
fn layers_of(r: &Reader, l: &Links, id: u64, t: f64) -> Vec<Layer> {
    let Some(mut set) = l.material.get(&id).or_else(|| l.typed.get(&id).and_then(|ty| l.material.get(ty))).and_then(|x| r.get(*x)) else {
        return Vec::new();
    };
    if set.name == "IFCMATERIALLAYERSETUSAGE" {
        match r.at(set.args.first()) {
            Some(x) => set = x,
            None => return Vec::new(),
        }
    }
    if set.name != "IFCMATERIALLAYERSET" || list(set.args.first()).len() > MAX_LAYERS {
        return Vec::new();
    }
    let mut layers: Vec<Layer> = refs(set.args.first())
        .into_iter()
        .filter_map(|id| r.get(id))
        .filter(|x| x.name == "IFCMATERIALLAYER")
        .map(|x| {
            let thickness = r.len(x.args.get(1)).map_or(f64::NAN, |t| (t * 100.0).round() / 100.0);
            let function = match text(x.args.get(5)) {
                _ if thickness == 0.0 => Function::Membrane,
                Some("LoadBearing") => Function::Structure,
                Some("Insulation") => Function::Thermal,
                Some("Finish") => Function::Finish,
                Some("Substrate") => Function::Substrate,
                Some("Membrane") => Function::Membrane,
                _ => Function::Substrate,
            };
            Layer {
                function,
                material: r.at(x.args.first()).and_then(|m| text(m.args.first())).map(clip).unwrap_or_default(),
                thickness,
                ..Layer::default()
            }
        })
        .collect();
    let sum: f64 = layers.iter().map(|l| l.thickness).sum();
    if layers.is_empty()
        || layers.len() > MAX_LAYERS
        || layers.iter().any(|l| !(0.0..=MAX_COORD).contains(&l.thickness))
        || (sum - t).abs() > 1.0
    {
        return Vec::new();
    }
    // A file that says nothing of what its layers do: the thickest carries the rest.
    let told = refs(set.args.first()).into_iter().filter_map(|id| r.get(id)).any(|x| text(x.args.get(5)).is_some());
    if !told
        && let Some(k) = (0..layers.len()).max_by(|a, b| layers[*a].thickness.total_cmp(&layers[*b].thickness))
        && let Some(l) = layers.get_mut(k)
    {
        l.function = Function::Structure;
    }
    layers
}

/// The colours a file shades its materials in, by their names: each material's surface style's.
fn material_colours(r: &Reader, ids: &[u64]) -> HashMap<String, [u8; 3]> {
    let mut out = HashMap::new();
    // Surface styles by their own names, which name materials as BIMCraft and Revit write them;
    // then the materials' styles, over them.
    for id in ids {
        if let Some(x) = r.get(*id).filter(|x| x.name == "IFCSURFACESTYLE")
            && let (Some(name), Some(c)) = (text(x.args.first()), colour(r, vec![*id]))
        {
            out.insert(clip(name), c);
        }
    }
    for e in ids.iter().filter_map(|id| r.get(*id)).filter(|e| e.name == "IFCMATERIALDEFINITIONREPRESENTATION") {
        let Some(name) = r.at(e.args.get(3)).and_then(|m| text(m.args.first())) else { continue };
        let items = refs(e.args.get(2)).into_iter().filter_map(|rep| r.get(rep)).flat_map(|rep| refs(rep.args.get(3)));
        if let Some(c) = colour(r, items.filter_map(|i| r.get(i)).flat_map(|i| refs(i.args.get(1))).collect()) {
            out.insert(clip(name), c);
        }
    }
    out
}

/// The colour `styles` shade with: in IFC2x3 through style assignments; a surface style's, its
/// shading's or its rendering's.
fn colour(r: &Reader, mut styles: Vec<u64>) -> Option<[u8; 3]> {
    let mut seen = HashSet::new();
    while let Some(id) = styles.pop() {
        let Some(x) = r.get(id).filter(|_| seen.insert(id)) else { continue };
        match x.name.as_str() {
            "IFCPRESENTATIONSTYLEASSIGNMENT" => styles.extend(refs(x.args.first())),
            "IFCSURFACESTYLE" => styles.extend(refs(x.args.get(2))),
            "IFCSURFACESTYLESHADING" | "IFCSURFACESTYLERENDERING" => {
                if let Some(c) = r.at(x.args.first()).filter(|c| c.name == "IFCCOLOURRGB") {
                    let byte = |i: usize| real(c.args.get(i)).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    if let (Some(red), Some(green), Some(blue)) = (byte(1), byte(2), byte(3)) {
                        return Some([red, green, blue]);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// The name of the surface style each representation item is styled with: what a component's
/// form made of it is made of.
fn item_styles(r: &Reader, ids: &[u64]) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    for e in ids.iter().filter_map(|id| r.get(*id)).filter(|e| e.name == "IFCSTYLEDITEM") {
        let Some(item) = reference(e.args.first()) else { continue };
        let mut styles = refs(e.args.get(1));
        let mut seen = HashSet::new();
        while let Some(id) = styles.pop() {
            let Some(x) = r.get(id).filter(|_| seen.insert(id)) else { continue };
            match x.name.as_str() {
                "IFCPRESENTATIONSTYLEASSIGNMENT" => styles.extend(refs(x.args.first())),
                "IFCSURFACESTYLE" => {
                    if let Some(n) = text(x.args.first()) {
                        out.entry(item).or_insert_with(|| clip(n));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The material the file associates element `id`, or its type, with, by name: a material's, or
/// the first of a set's, a list's or a layer set's.
fn element_material(r: &Reader, l: &Links, id: u64) -> Option<String> {
    let mut at = l.material.get(&id).or_else(|| l.typed.get(&id).and_then(|t| l.material.get(t))).and_then(|x| r.get(*x))?;
    for _ in 0..8 {
        at = match at.name.as_str() {
            "IFCMATERIAL" => return text(at.args.first()).map(clip),
            "IFCMATERIALCONSTITUENTSET" => r.get(*refs(at.args.get(2)).first()?)?,
            "IFCMATERIALCONSTITUENT" => r.at(at.args.get(2))?,
            "IFCMATERIALLIST" | "IFCMATERIALLAYERSET" => r.get(*refs(at.args.first()).first()?)?,
            "IFCMATERIALLAYERSETUSAGE" | "IFCMATERIALLAYER" => r.at(at.args.first())?,
            _ => return None,
        };
    }
    None
}

/// Triangle `t` read from its least corner on, turned as it is: so the same triangle reads the same.
fn settled(t: [P3; 3]) -> [P3; 3] {
    let least = |p: &P3, q: &P3| p[0].total_cmp(&q[0]).then(p[1].total_cmp(&q[1])).then(p[2].total_cmp(&q[2])).is_le();
    let [a, b, c] = t;
    if least(&a, &b) && least(&a, &c) {
        [a, b, c]
    } else if least(&b, &c) {
        [b, c, a]
    } else {
        [c, a, b]
    }
}

/// A key for forms' shapes and what they're made of: the same forms, the same key.
fn shape_key(forms: &[Form]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in forms {
        f.material.hash(&mut h);
        f.mesh.iter().flatten().flatten().for_each(|x| x.to_bits().hash(&mut h));
    }
    h.finish()
}

/// Forms' mirror images across their x axis, as a mirrored component shows them.
fn mirrored(forms: &[Form]) -> Vec<Form> {
    let flip = |p: P3| [p[0], -p[1] + 0.0, p[2]];
    forms
        .iter()
        .map(|f| Form { mesh: Arc::new(f.mesh.iter().map(|[a, b, c]| settled([flip(*a), flip(*c), flip(*b)])).collect()), ..f.clone() })
        .collect()
}

/// A flat face's triangles, turned as it is: ears cut in its plane; past `MAX_PROFILE` corners,
/// a fan.
fn triangles(face: &[P3]) -> Vec<[P3; 3]> {
    let n = normal(face);
    if face.len() < 3 || dot(n, n) < 0.5 {
        return Vec::new();
    }
    let pt = |i: usize| face.get(i).copied().unwrap_or_default();
    if face.len() > crate::family::MAX_PROFILE {
        // ponytail: a fan, wrong for a large face that isn't convex; ear-cut faster if files bring them.
        return (1..face.len() - 1).map(|i| [pt(0), pt(i), pt(i + 1)]).collect();
    }
    let u = unit(cross(n, if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] }));
    let v = cross(n, u);
    let ring: Vec<P2> = face.iter().map(|p| P2(dot(*p, u), dot(*p, v))).collect();
    geom::triangulate(&ring).into_iter().map(|[a, b, c]| [pt(a), pt(b), pt(c)]).collect()
}

/// A type of `map` like `same` says, by `base` or `base (2)`, `base (3)`… made as `new` when
/// there's none.
fn kind<T>(map: &mut BTreeMap<String, T>, base: &str, same: impl Fn(&T) -> bool, new: T) -> String {
    let base = clip(base);
    let mut new = Some(new);
    let mut n = 1;
    loop {
        let name = if n == 1 { base.clone() } else { format!("{base} ({n})") };
        match map.get(&name) {
            Some(t) if same(t) => return name,
            Some(_) => n += 1,
            None => {
                if let Some(t) = new.take() {
                    map.insert(name.clone(), t);
                }
                return name;
            }
        }
    }
}

/// `base`, or `base (2)`… whichever `taken` doesn't hold yet; then it does.
fn fresh(taken: &mut BTreeSet<String>, base: &str) -> String {
    let base = clip(base);
    let name = (1..).map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") }).find(|n| !taken.contains(n)).unwrap_or(base);
    taken.insert(name.clone());
    name
}

/// What a property of text holds, read as JSON of type `T`.
fn read_json<T: serde::de::DeserializeOwned>(p: &Entity) -> Result<T, String> {
    serde_json::from_str(text(p.args.get(2)).ok_or("it holds no text")?).map_err(|e| e.to_string())
}

/// Sets the part of the project's phasing that `field` is to `new`, if the project can have it; else it keeps what it had,
/// and the rule `new` breaks is the error.
fn adopt<T>(m: &mut Model, field: fn(&mut Model) -> &mut T, new: T) -> Result<(), String> {
    let old = std::mem::replace(field(m), new);
    let checked = phasing::check_project(m);
    if checked.is_err() {
        *field(m) = old;
    }
    checked
}

const WALLS: &[&str] = &["IFCWALL", "IFCWALLSTANDARDCASE", "IFCWALLELEMENTEDCASE"];
const OPENINGS: &[&str] = &["IFCDOOR", "IFCDOORSTANDARDCASE", "IFCWINDOW", "IFCWINDOWSTANDARDCASE"];
const SLABS: &[&str] = &["IFCSLAB", "IFCSLABSTANDARDCASE", "IFCSLABELEMENTEDCASE"];
const COLUMNS: &[&str] = &["IFCCOLUMN", "IFCCOLUMNSTANDARDCASE"];

/// Reads IFC text into a model, named after the project (or `name`).
pub fn ifc(bytes: &[u8], name: &str) -> Result<Imported, String> {
    let file = step::parse(bytes)?;
    if !file.schema.starts_with("IFC") {
        let schema = if file.schema.is_empty() { "no schema" } else { &file.schema };
        return Err(format!("this STEP file is in {schema}, not IFC"));
    }
    let (mm, rad) = units(&file)?;
    let mut ids: Vec<u64> = file.entities.keys().copied().collect();
    ids.sort_unstable();
    let l = Links::read(&file, &ids);
    // Far from the origin (a site at its map coordinates), the model comes nearer.
    let probe = Reader { f: &file, mm, rad, shift: P2(0.0, 0.0) };
    let origins: Vec<P3> = l.within.keys().filter_map(|id| probe.get(*id)).map(|x| probe.frame(x).o).collect();
    let shift = match (span(origins.iter().map(|p| p[0])), span(origins.iter().map(|p| p[1]))) {
        (Some((x0, x1)), Some((y0, y1))) if x0.abs().max(x1.abs()).max(y0.abs()).max(y1.abs()) > 1e8 => {
            P2((x0 / 1000.0).floor() * 1000.0, (y0 / 1000.0).floor() * 1000.0)
        }
        _ => P2(0.0, 0.0),
    };
    let r = Reader { f: &file, mm, rad, shift };
    // What an element is made of: its layers, when they add up to how thick it is, and so how thick.
    let made_of = |id: u64, t: f64| {
        let layers = layers_of(&r, &l, id, t);
        (if layers.is_empty() { t } else { layers.iter().map(|l| l.thickness).sum() }, layers)
    };
    // A slab's from the top down: the other way round where its usage stacks them up from its bottom.
    let top_down = |id: u64, t: f64| {
        let (t, mut layers) = made_of(id, t);
        let up = r.get(id).is_some_and(|x| r.frame(x).z[2] >= 0.0);
        if stacked_up(&r, &l, id) == Some(up) {
            layers.reverse();
        }
        (t, layers)
    };
    let e = |id: u64| r.get(id);
    let named = |names: &[&str]| -> Vec<u64> {
        ids.iter().copied().filter(|id| e(*id).is_some_and(|x| names.contains(&x.name.as_str()))).collect()
    };
    let who = |id: u64| {
        e(id).map_or_else(
            || format!("#{id}"),
            |x| format!("{} #{id}{}", x.name, text(x.args.get(2)).map_or_else(String::new, |n| format!(" '{n}'"))),
        )
    };
    let label = |id: u64| l.type_name.get(&id).cloned().or_else(|| e(id).and_then(|x| text(x.args.get(4))).map(String::from));
    let storey_of = |id: u64| {
        let mut at = id;
        for _ in 0..8 {
            at = *l.within.get(&at).or_else(|| l.whole.get(&at))?;
            if e(at).is_some_and(|x| x.name == "IFCBUILDINGSTOREY") {
                return Some(at);
            }
        }
        None
    };

    // What carries load: the elements whose `Pset_*Common` says `LoadBearing` is true.
    let bearing: HashSet<u64> = named(&["IFCRELDEFINESBYPROPERTIES"])
        .into_iter()
        .filter_map(e)
        .filter_map(|rel| {
            let set = reference(rel.args.get(5)).and_then(e).filter(|x| {
                x.name == "IFCPROPERTYSET" && text(x.args.get(2)).is_some_and(|n| n.starts_with("Pset_") && n.ends_with("Common"))
            })?;
            let loaded = refs(set.args.get(4)).into_iter().filter_map(e).any(|p| {
                p.name == "IFCPROPERTYSINGLEVALUE"
                    && text(p.args.first()) == Some("LoadBearing")
                    && matches!(p.args.get(2), Some(V::Typed(_, v)) if matches!(&**v, V::Enum(b) if b == "T"))
            });
            loaded.then(|| refs(rel.args.get(4)))
        })
        .flatten()
        .collect();

    // What BIMCraft's property set on a run says: its justification and its category, by product.
    let mut run_props: HashMap<u64, (Option<Justify>, Option<String>)> = HashMap::new();
    for rel in named(&["IFCRELDEFINESBYPROPERTIES"]).into_iter().filter_map(e) {
        let Some(set) =
            reference(rel.args.get(5)).and_then(e).filter(|x| x.name == "IFCPROPERTYSET" && text(x.args.get(2)) == Some(crate::ifc::RUN))
        else {
            continue;
        };
        let said = |name: &str| {
            refs(set.args.get(4))
                .into_iter()
                .filter_map(e)
                .find(|p| p.name == "IFCPROPERTYSINGLEVALUE" && text(p.args.first()) == Some(name))
                .and_then(|p| match p.args.get(2) {
                    Some(V::Typed(_, v)) => match &**v {
                        V::Str(s) => Some(s.clone()),
                        _ => None,
                    },
                    _ => None,
                })
        };
        let (justify, category) = (said("Justification").and_then(|j| Justify::parse(&j)), said("Category"));
        for product in refs(rel.args.get(4)) {
            run_props.insert(product, (justify, category.clone()));
        }
    }

    let project = ids.iter().filter_map(|id| e(*id)).find(|x| x.name == "IFCPROJECT");
    let title = project.and_then(|p| text(p.args.get(2)).or_else(|| text(p.args.get(5)))).unwrap_or(name);
    let mut m = Model::new(&clip(title));
    m.elements.clear();
    m.next_id = FIRST_ID;
    let (mut skipped, mut seen): (Vec<String>, HashSet<u64>) = (Vec::new(), HashSet::new());
    // Each element made, and what it was in the file.
    let mut from: HashMap<Id, String> = HashMap::new();
    // The first element made of each in the file, which takes its GlobalId for its GUID.
    let mut origin: HashMap<u64, Id> = HashMap::new();
    // The elements made of each in the file.
    let mut elements_from: HashMap<u64, Vec<Id>> = HashMap::new();

    // Storeys, as levels, lowest first.
    let mut storeys: Vec<(u64, String, f64)> = named(&["IFCBUILDINGSTOREY"])
        .into_iter()
        .filter_map(|id| {
            let s = e(id)?;
            let z = match reference(s.args.get(5)) {
                Some(_) => r.frame(s).o[2],
                None => r.len(s.args.get(9)).unwrap_or(0.0),
            };
            Some((id, text(s.args.get(2)).or_else(|| text(s.args.get(7))).unwrap_or("Level").to_string(), (z * 1000.0).round() / 1000.0))
        })
        .collect();
    storeys.sort_by(|a, b| a.2.total_cmp(&b.2));
    if storeys.is_empty() {
        storeys.push((0, "Level 1".into(), 0.0));
    }
    let mut taken = BTreeSet::new();
    let mut level_of: HashMap<u64, Id> = HashMap::new();
    for (sid, n, z) in &storeys {
        let level = m.insert(Element::Level(Level::new(fresh(&mut taken, n), *z)));
        level_of.insert(*sid, level);
        origin.insert(*sid, level);
        seen.insert(*sid);
    }
    let levels: Vec<(Id, f64)> = m.levels().into_iter().map(|(id, lv)| (id, lv.elevation)).collect();
    // What's made points at a level, a wall, a stair by its id until the model is made: then at its GUID (`resolve`).
    let elevation = |level: Guid| levels.iter().find(|(id, _)| Guid::of_id(*id) == level).map_or(0.0, |(_, z)| *z);
    // An element's level: its storey's, else the highest at or below `z`.
    let level_for = |id: u64, z: f64| -> Guid {
        Guid::of_id(
            storey_of(id)
                .and_then(|s| level_of.get(&s).copied())
                .unwrap_or_else(|| levels.iter().rev().find(|(_, lz)| *lz <= z + 1.0).or(levels.first()).map_or(0, |(id, _)| *id)),
        )
    };
    let level_near = |z: f64| levels.iter().find(|(_, lz)| (lz - z).abs() <= 1.0).map(|(id, _)| Guid::of_id(*id));
    // Up to a level at `top` above `base`, else `top - bottom` tall.
    let topped = |base: Guid, bottom: f64, top: f64| -> (Option<Guid>, f64) {
        match levels.iter().find(|(id, lz)| Guid::of_id(*id) != base && *lz > elevation(base) + TOL && (lz - top).abs() <= 1.0) {
            Some((id, _)) => (Some(Guid::of_id(*id)), top - bottom),
            None => (None, top - bottom),
        }
    };
    let leave = |skipped: &mut Vec<String>, seen: &mut HashSet<u64>, id: u64, why: &str| {
        skipped.push(format!("{}: {why}", who(id)));
        seen.insert(id);
    };

    // Walls: the line the faces run along, and how thick they are across it.
    let mut raw: Vec<RawWall> = Vec::new();
    for id in named(WALLS) {
        let Some(w) = e(id) else { continue };
        let faces = r.body(w);
        let pts: Vec<P3> = faces.iter().flatten().copied().collect();
        let axis = r.axis(w);
        let f = r.frame(w);
        // BIMCraft lists a wall's layers from its left, its exterior: the wall runs so the file's
        // first is there, which its usage stacks from across its own y, or up it from the other side.
        let faced = |a: P2, b: P2| {
            let side = P2(f.y[0], f.y[1]).dot((b - a).perp());
            match stacked_up(&r, &l, id) {
                Some(up) if (if up { -side } else { side }) < 0.0 => (b, a),
                _ => (a, b),
            }
        };
        // A curved axis: the arc through its ends and its middle; the wall's line, the middle of its
        // body round the arc's centre, as thick as the body spreads.
        let bent: Vec<P2> = axis.iter().flatten().map(|x| r.flat(*x)).collect();
        if let [p, .., q] = bent.as_slice()
            && bent.iter().any(|x| geom::dist_to_segment(*x, *p, *q) > 1.0)
        {
            let mid = bent.get(bent.len() / 2).copied().unwrap_or(*p);
            let (p, q) = faced(*p, *q);
            let arc = walls::sweep_through(p, mid, q);
            let radii = Axis::new(p, q, arc).centre().and_then(|c| Some((c, arc_faces(pts.iter().map(|x| r.flat(*x).dist(c)))?)));
            let (Some((c, (r0, r1))), Some((z0, z1))) = (radii, span(pts.iter().map(|x| x[2]))) else {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            };
            let t = ((r1 - r0) * 100.0).round() / 100.0;
            let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
            let (t, layers) = made_of(id, t);
            let type_name = kind(
                &mut m.types.wall,
                &base,
                |k| (k.thickness - t).abs() < 0.05 && k.layers == layers,
                WallType { thickness: t, layers: layers.clone() },
            );
            let on = |x: P2| micron(c + (x - c).unit() * ((r0 + r1) / 2.0));
            let arc = (arc * 1e6).round() / 1e6;
            raw.push(RawWall {
                ifc: id,
                start: on(p),
                end: on(q),
                arc,
                t,
                z: (z0, z1),
                peak: r.flat(highest(&pts)),
                level: level_for(id, z0),
                type_name,
            });
            continue;
        }
        let (a, b) = match axis.as_deref() {
            Some([p, .., q]) if r.flat(*p).dist(r.flat(*q)) > TOL => (r.flat(*p), r.flat(*q)),
            // No axis: along the wall's own x, as far as its body goes.
            _ => {
                let o = r.flat(f.o);
                let x = P2(f.x[0], f.x[1]).unit();
                match span(pts.iter().map(|p| (r.flat(*p) - o).dot(x))) {
                    Some((s0, s1)) if x.length() > 0.5 => (o + x * s0, o + x * s1),
                    _ => (o, o),
                }
            }
        };
        let (a, b) = faced(a, b);
        let (u, n) = ((b - a).unit(), (b - a).unit().perp());
        let (Some((c0, c1)), Some((z0, z1))) = (span(pts.iter().map(|p| (r.flat(*p) - a).dot(n))), span(pts.iter().map(|p| p[2]))) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        if a.dist(b) <= TOL || c1 - c0 <= 0.0 || u.length() < 0.5 {
            leave(&mut skipped, &mut seen, id, "it has no length or thickness BIMCraft can read");
            continue;
        }
        let mid = n * ((c0 + c1) / 2.0);
        let t = (c1 - c0) * 100.0;
        let t = t.round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let (t, layers) = made_of(id, t);
        let type_name = kind(
            &mut m.types.wall,
            &base,
            |k| (k.thickness - t).abs() < 0.05 && k.layers == layers,
            WallType { thickness: t, layers: layers.clone() },
        );
        raw.push(RawWall {
            ifc: id,
            start: micron(a + mid),
            end: micron(b + mid),
            arc: 0.0,
            t,
            z: (z0, z1),
            peak: r.flat(highest(&pts)),
            level: level_for(id, z0),
            type_name,
        });
    }
    meet(&mut raw);
    // Each wall made: its id, its line, and the height of its base; and where it's highest.
    let mut walls: HashMap<u64, (Id, P2, P2, f64)> = HashMap::new();
    let mut peaks: Vec<(Id, P2, f64)> = Vec::new();
    for w in raw {
        let (top_level, height) = topped(w.level, w.z.0, w.z.1);
        let wall = Wall {
            params: Default::default(),
            type_name: w.type_name,
            level: w.level,
            start: w.start,
            end: w.end,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset: ((w.z.0 - elevation(w.level)) * 1000.0).round() / 1000.0,
            arc: w.arc,
            location: Default::default(),
            attach_top: None,
            attach_base: None,
        };
        let id = m.insert(Element::Wall(wall));
        from.insert(id, who(w.ifc));
        origin.entry(w.ifc).or_insert(id);
        elements_from.entry(w.ifc).or_default().push(id);
        walls.insert(w.ifc, (id, w.start, w.end, w.z.0));
        peaks.push((id, w.peak, w.z.1));
        seen.insert(w.ifc);
    }

    // The openings through floors, ceilings and roofs: what each voids, its outline in plan at its
    // top, and how high it reaches. Each comes in as a shaft, which takes the hole it cuts in what
    // it voids: a mesh carries the holes cut in it.
    let cuts: Vec<(u64, u64, Vec<P2>, f64, f64)> = named(&["IFCOPENINGELEMENT", "IFCOPENINGSTANDARDCASE"])
        .into_iter()
        .filter_map(|id| {
            let h = *l.host.get(&id)?;
            let host = e(h)?.name.as_str();
            let o = e(id).filter(|_| SLABS.contains(&host) || matches!(host, "IFCCOVERING" | "IFCROOF"))?;
            let faces = r.body(o);
            let (z0, z1) = span(faces.iter().flatten().map(|p| p[2]))?;
            let ring = outline(&faces, Plane::flat(z1), true)?.outer;
            Some((id, h, ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect(), z0, z1))
        })
        .collect();
    // The holes the openings through `id`, or through one of its parts, cut.
    let voided = |id: u64| -> Vec<Vec<P2>> {
        cuts.iter().filter(|(_, h, ..)| *h == id || l.whole.get(h) == Some(&id)).map(|(_, _, ring, ..)| ring.clone()).collect()
    };
    // The holes in the top of slab or ceiling `id` that are its own openings, in plan: less those
    // the openings through it cut, each from its south-west corner, in order: as the same file reads
    // the same.
    let openings_of = |id: u64, holes: Vec<Vec<P2>>| {
        let cut = voided(id);
        let mut openings: Vec<Vec<P2>> = holes
            .into_iter()
            .map(|h| tidy(&h.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect::<Vec<P2>>()))
            .filter(|h| !cut.iter().any(|c| same(h, c)))
            .collect();
        openings
            .sort_by(|a, b| a.first().map(|p| (p.1, p.0)).partial_cmp(&b.first().map(|p| (p.1, p.0))).unwrap_or(std::cmp::Ordering::Equal));
        openings
    };

    // Floors: their top outline and the holes in it, and how thick they are below it.
    let in_whole = |id: u64, names: &[&str]| l.whole.get(&id).and_then(|w| e(*w)).is_some_and(|w| names.contains(&w.name.as_str()));
    let mut roofs: Vec<u64> = named(&["IFCROOF"]).into_iter().filter(|id| !l.whole.contains_key(id)).collect();
    for id in named(SLABS) {
        let Some(s) = e(id) else { continue };
        if in_whole(id, &["IFCROOF", "IFCSTAIR", "IFCRAMP"]) {
            continue;
        }
        match enumeration(s.args.get(8)) {
            Some("ROOF") => {
                roofs.push(id);
                continue;
            }
            Some("LANDING") => {
                leave(&mut skipped, &mut seen, id, "landings aren't supported");
                continue;
            }
            _ => {}
        }
        let faces = r.body(s);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        // Level at its highest, or sloped: then its slope arrow runs up it from its lowest corner,
        // all the way across.
        let shape = outline(&faces, Plane::flat(z1), true).map(|shape| (Plane::flat(z1), shape, z1 - z0)).or_else(|| top_of(&faces));
        let Some((top, geom::Region { outer: ring, holes }, rise)) = shape else {
            leave(&mut skipped, &mut seen, id, "its top isn't flat");
            continue;
        };
        let (tan, flat) = (top.g.length(), |p: P2| micron(r.flat([p.0, p.1, 0.0])));
        let tail = ring.iter().copied().min_by(|a, b| top.at(*a).total_cmp(&top.at(*b))).unwrap_or_default();
        let arrow = (tan > 1e-9).then(|| {
            let u = top.g * (1.0 / tan);
            let reach = ring.iter().map(|q| (*q - tail).dot(u)).fold(0.0, f64::max);
            [flat(tail), flat(tail + u * reach)]
        });
        let t = ((rise * tan.atan().cos()) * 100.0).round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let (t, layers) = top_down(id, t);
        let type_name = kind(
            &mut m.types.floor,
            &base,
            |k| (k.thickness - t).abs() < 0.05 && k.layers == layers,
            FloorType { thickness: t, layers: layers.clone() },
        );
        let z = top.at(tail);
        let level = level_near(z).unwrap_or_else(|| level_for(id, z));
        let floor = Floor {
            params: Default::default(),
            sketch: Vec::new(),
            type_name,
            level,
            boundary: ring.into_iter().map(flat).collect(),
            openings: openings_of(id, holes),
            offset: ((z - elevation(level)) * 1000.0).round() / 1000.0 + 0.0,
            slope: if arrow.is_some() { (tan.atan().to_degrees() * 1e6).round() / 1e6 } else { 0.0 },
            arrow,
            structural: bearing.contains(&id),
            pattern_origin: P2(0.0, 0.0),
            pattern_angle: 0.0,
        };
        let fid = m.insert(Element::Floor(floor));
        from.insert(fid, who(id));
        origin.entry(id).or_insert(fid);
        elements_from.entry(id).or_default().push(fid);
        seen.insert(id);
    }

    // Ceilings: coverings that are, their outline and the holes in it, their underside's height and
    // their thickness.
    for id in named(&["IFCCOVERING"]) {
        let Some(c) = e(id).filter(|c| enumeration(c.args.get(8)) == Some("CEILING")) else { continue };
        let faces = r.body(c);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        let Some(geom::Region { outer: ring, holes }) = outline(&faces, Plane::flat(z1), true) else {
            leave(&mut skipped, &mut seen, id, "its top isn't level");
            continue;
        };
        let t = ((z1 - z0) * 100.0).round() / 100.0;
        let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(t)));
        let (t, layers) = top_down(id, t);
        let type_name = kind(
            &mut m.types.ceiling,
            &base,
            |k| (k.thickness - t).abs() < 0.05 && k.layers == layers,
            CeilingType { thickness: t, grid: 0.0, layers: layers.clone() },
        );
        let level = level_for(id, z0);
        let boundary = ring.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect();
        let offset = ((z0 - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let cid = m.insert(Element::Ceiling(Ceiling {
            params: Default::default(),
            sketch: Vec::new(),
            type_name,
            level,
            boundary,
            openings: openings_of(id, holes),
            offset,
        }));
        from.insert(cid, who(id));
        origin.entry(id).or_insert(cid);
        elements_from.entry(id).or_default().push(cid);
        seen.insert(id);
    }

    // Roofs: flat, else pitched over a rectangle, measured from the faces they turn up.
    for id in roofs {
        let Some(roof) = e(id) else { continue };
        let mut faces = r.body(roof);
        for part in l.parts.get(&id).into_iter().flatten() {
            if let Some(p) = e(*part) {
                faces.extend(r.body(p));
                seen.insert(*part);
            }
        }
        seen.insert(id);
        match roof_from(&r, &faces, &voided(id)) {
            Ok(rd) => {
                let base = label(id).unwrap_or_else(|| format!("Imported - {}mm", num(rd.t)));
                let (t, layers) = top_down(id, rd.t);
                let type_name = kind(
                    &mut m.types.roof,
                    &base,
                    |k| (k.thickness - t).abs() < 0.05 && k.layers == layers,
                    RoofType { thickness: t, layers: layers.clone() },
                );
                let pitched = rd.slope > 0.0;
                let level = if pitched { level_for(id, rd.z) } else { level_near(rd.z).unwrap_or_else(|| level_for(id, rd.z)) };
                // A pitched one reaches its level `overhang` in from its eaves, or its eaves stand
                // `offset` over it; any other sits on it.
                let below = elevation(level) - rd.z;
                let overhang = if pitched && below > 0.0 { below / rd.slope.to_radians().tan() } else { 0.0 };
                let offset = if pitched && below > 0.0 { 0.0 } else { -below };
                let roof = Roof {
                    params: Default::default(),
                    sketch: Vec::new(),
                    type_name,
                    level,
                    boundary: rd.boundary,
                    courtyards: rd.courtyards,
                    slope: rd.slope,
                    overhang: (overhang * 1000.0).round() / 1000.0,
                    gable: false,
                    gables: rd.gables,
                    offset: (offset * 100.0).round() / 100.0 + 0.0,
                    openings: rd.openings,
                    join: None,
                    extrusion: rd.extrusion,
                };
                match m.roof_faces(&roof) {
                    Ok(_) => {
                        let rid = m.insert(Element::Roof(roof));
                        from.insert(rid, who(id));
                        origin.entry(id).or_insert(rid);
                        elements_from.entry(id).or_default().push(rid);
                    }
                    Err(why) => leave(&mut skipped, &mut seen, id, &why),
                }
            }
            Err(why) => leave(&mut skipped, &mut seen, id, &why),
        }
    }

    // A wall whose top follows a roof, as Revit writes attached walls (cut to the roof): its highest
    // point under the roof, and the roof lower than that somewhere over the wall. One rising
    // through a roof is higher than it; one only touching it, along an edge, doesn't reach above it.
    let roofs: Vec<Id> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Roof(_))).map(|(id, _)| *id).collect();
    for (wid, q, top) in peaks {
        let Some(w) = m.wall(wid).cloned() else { continue };
        let axis = Axis::of(&w);
        let along: Vec<P2> = (0..=8).map(|i| axis.at(axis.length() * f64::from(i) / 8.0, 0.0)).collect();
        let under = |rid: Id| {
            walls::underside(&m, rid, q).is_some_and(|z| (z - top).abs() <= 5.0)
                && along.iter().any(|p| walls::underside(&m, rid, *p).is_some_and(|z| z < top - 5.0))
        };
        if let Some(rid) = roofs.iter().copied().find(|rid| under(*rid))
            && let Some(Element::Wall(w)) = m.elements.get_mut(wid)
        {
            w.attach_top = Some(Guid::of_id(rid));
        }
    }

    // Shafts: the openings through floors, ceilings and roofs; openings alike (a shaft voids each
    // it passes through), one shaft.
    let mut shafts: Vec<(u64, Vec<P2>, f64, f64)> = Vec::new();
    for (id, _, ring, z0, z1) in cuts {
        let alike = |(_, b, lo, hi): &(u64, Vec<P2>, f64, f64)| (lo - z0).abs() < 1.0 && (hi - z1).abs() < 1.0 && same(b, &ring);
        if !shafts.iter().any(alike) {
            shafts.push((id, ring, z0, z1));
        }
    }
    for (id, boundary, z0, z1) in shafts {
        let level = Guid::of_id(levels.iter().rev().find(|(_, lz)| *lz <= z0 + 1.0).or(levels.first()).map_or(0, |(id, _)| *id));
        let (top_level, height) = topped(level, z0, z1);
        let base_offset = ((z0 - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let sid = m.insert(Element::Shaft(Shaft {
            level,
            boundary,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset,
            sketch: Vec::new(),
        }));
        from.insert(sid, who(id));
        origin.entry(id).or_insert(sid);
        elements_from.entry(id).or_default().push(sid);
    }

    // Masses: proxies whose object type says so, each its outline and how high it stands. One outline: a courtyard
    // in it is more than a mass holds.
    // A lofted one is written as a face set and says in a property set of its own how its two outlines pair up.
    #[derive(serde::Deserialize)]
    struct Loft {
        bottom: Vec<P2>,
        top: Vec<P2>,
    }
    let lofts: Vec<(Vec<u64>, Loft)> = named(&["IFCRELDEFINESBYPROPERTIES"])
        .into_iter()
        .filter_map(e)
        .filter_map(|rel| {
            let set = reference(rel.args.get(5))
                .and_then(e)
                .filter(|x| x.name == "IFCPROPERTYSET" && text(x.args.get(2)) == Some(crate::ifc::LOFT))?;
            let loft = refs(set.args.get(4)).into_iter().filter_map(e).find(|p| text(p.args.first()) == Some("Loft"))?;
            Some((refs(rel.args.get(4)), read_json(loft).ok()?))
        })
        .collect();
    for id in named(&["IFCBUILDINGELEMENTPROXY"]) {
        let Some(x) = e(id).filter(|x| text(x.args.get(4)) == Some("Mass")) else { continue };
        let faces = r.body(x);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        let flat = |g: geom::Region| -> Vec<P2> { g.outer.into_iter().map(|p| micron(r.flat([p.0, p.1, 0.0]))).collect() };
        let region = outline(&faces, Plane::flat(z1), true).filter(|g| g.holes.is_empty());
        let Some(crown) = region.map(flat) else {
            leave(&mut skipped, &mut seen, id, "its top isn't one level outline");
            continue;
        };
        let base = outline(&faces, Plane::flat(z0), false).filter(|g| g.holes.is_empty()).map(flat);
        let noted = lofts.iter().find(|(on, _)| on.contains(&id)).map(|(_, l)| l);
        let (boundary, top) = match (noted, base) {
            (Some(l), Some(b))
                if same(&l.bottom, &b) && same(&l.top, &crown) && crate::massing::loft_problem(&l.bottom, &l.top).is_none() =>
            {
                (l.bottom.clone(), Some(l.top.clone()))
            }
            (_, Some(b)) if !same(&b, &crown) => {
                leave(
                    &mut skipped,
                    &mut seen,
                    id,
                    "its outline changes from its base to its top, and no BIMCraft_Mass property set says how its corners pair up",
                );
                continue;
            }
            _ => (crown, None),
        };
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z1);
        let base_offset = ((z0 - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let name = text(x.args.get(2)).map(clip).unwrap_or_default();
        let mass = Mass {
            name,
            level,
            boundary,
            top,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset,
            params: Default::default(),
        };
        let mid = m.insert(Element::Mass(mass));
        from.insert(mid, who(id));
        origin.entry(id).or_insert(mid);
        elements_from.entry(id).or_default().push(mid);
        seen.insert(id);
    }

    // Columns: a section across them, and how high they stand; one that carries load is a structural column, and a
    // footing that isn't a strip is an isolated one, a column laid down.
    let strip = |id: u64| e(id).is_some_and(|x| matches!(enumeration(x.args.get(8)), Some("STRIP_FOOTING" | "FOOTING_BEAM")));
    let pads: Vec<u64> = named(&["IFCFOOTING"]).into_iter().filter(|id| !strip(*id)).collect();
    for id in named(COLUMNS).into_iter().chain(pads) {
        let Some(c) = e(id) else { continue };
        let faces = r.body(c);
        let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        // Square to its longest outline edge: a level edge of one of its upright faces (a top
        // made of triangles has diagonals too).
        let f = r.frame(c);
        let mut along = (0.0, P2(f.x[0], f.x[1]).unit());
        for face in faces.iter().filter(|face| normal(face)[2].abs() < 0.1) {
            for (p, q) in face.iter().zip(face.iter().cycle().skip(1)) {
                let d = r.flat(*q) - r.flat(*p);
                if (p[2] - q[2]).abs() < 0.5 && d.length() > along.0 + 1e-6 {
                    along = (d.length(), d.unit());
                }
            }
        }
        let (u, v) = (along.1, along.1.perp());
        let plan: Vec<P2> = faces.iter().flatten().map(|p| r.flat(*p)).collect();
        let (Some((a0, a1)), Some((b0, b1))) = (span(plan.iter().map(|p| p.dot(u))), span(plan.iter().map(|p| p.dot(v)))) else { continue };
        let at = u * ((a0 + a1) / 2.0) + v * ((b0 + b1) / 2.0);
        let radii = span(plan.iter().map(|p| p.dist(at)));
        // Round: many corners, all as far out (a square's four are too).
        let corners: HashSet<(i64, i64)> = plan.iter().map(|p| ((p.0 * 2.0).round() as i64, (p.1 * 2.0).round() as i64)).collect();
        let round = corners.len() >= 12 && radii.is_some_and(|(lo, hi)| hi > 0.0 && (hi - lo) / hi < 0.03);
        let size = |x: f64| (x * 100.0).round() / 100.0;
        let (w, d) = if round {
            (size(2.0 * radii.map_or(0.0, |r| r.1)), size(2.0 * radii.map_or(0.0, |r| r.1)))
        } else {
            (size(a1 - a0), size(b1 - b0))
        };
        if w <= 0.0 || d <= 0.0 {
            leave(&mut skipped, &mut seen, id, "it has no section BIMCraft can read");
            continue;
        }
        let base = label(id)
            .unwrap_or_else(|| if round { format!("Imported Round {}", num(w)) } else { format!("Imported {} x {}", num(w), num(d)) });
        let same = |k: &ColumnType| k.round == round && (k.width - w).abs() < 0.05 && (k.depth - d).abs() < 0.05;
        let footing = c.name == "IFCFOOTING";
        let structural = bearing.contains(&id);
        let make: fn(Column) -> Element = if footing {
            Element::Footing
        } else if structural {
            Element::StructuralColumn
        } else {
            Element::Column
        };
        let types = if footing {
            &mut m.types.footing
        } else if structural {
            &mut m.types.structural_column
        } else {
            &mut m.types.column
        };
        let type_name = kind(types, &base, same, ColumnType { width: w, depth: d, round });
        let level = level_for(id, z0);
        let (top_level, height) = if footing { (None, z1 - z0) } else { topped(level, z0, z1) };
        let angle = if round { 0.0 } else { (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 };
        let column = Column {
            params: Default::default(),
            type_name,
            level,
            at: micron(at),
            angle,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            base_offset: ((z0 - elevation(level)) * 1000.0).round() / 1000.0,
        };
        let cid = m.insert(make(column));
        from.insert(cid, who(id));
        origin.entry(id).or_insert(cid);
        elements_from.entry(id).or_default().push(cid);
        seen.insert(id);
    }

    // Runs (beams, strip footings, pipes): found in their bodies as the longest direction they run along and the section
    // square to it. A run rolled about its axis reads as the box round it. A product's class and type say which, or, where
    // it is one of BIMCraft's, its property set.
    let classes: Vec<&str> = crate::runs::IFC.iter().map(|(_, class, _)| *class).chain(["IFCBEAMSTANDARDCASE"]).collect();
    for id in named(&classes).into_iter().filter(|id| e(*id).is_some_and(|x| x.name != "IFCFOOTING") || strip(*id)) {
        let Some(b) = e(id) else { continue };
        let declared = run_props.get(&id).and_then(|(_, c)| c.as_deref());
        let Some(category) = crate::runs::from_ifc(&b.name, enumeration(b.args.get(8)), declared) else { continue };
        let pts: Vec<P3> = r.body(b).into_iter().flatten().collect();
        let Some(found) = run_in(&pts) else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read, or it stands (a column) or lies flat");
            continue;
        };
        let geom = crate::runs::Geom { a: found.start, b: found.end, width: found.width, depth: found.depth, round: found.round };
        if let Err(why) = crate::runs::check(&who(id), category, &geom) {
            leave(&mut skipped, &mut seen, id, why.split_once(": ").map_or(why.as_str(), |(_, w)| w));
            continue;
        }
        let level = level_for(id, found.start[2].min(found.end[2]));
        let (w, d) = (found.width, found.depth);
        let base = label(id).unwrap_or_else(|| {
            if found.round { format!("Imported Round {}", num(w)) } else { format!("Imported {} x {}", num(w), num(d)) }
        });
        // Made of a material the library has, by name.
        let material = l
            .material
            .get(&id)
            .and_then(|x| e(*x))
            .filter(|x| x.name == "IFCMATERIAL")
            .and_then(|x| text(x.args.first()))
            .filter(|n| m.materials.contains_key(*n))
            .map(String::from)
            .unwrap_or_default();
        let same =
            |k: &RunType| k.round == found.round && (k.width - w).abs() < 0.05 && (k.depth - d).abs() < 0.05 && k.material == material;
        let types = m.types.runs.entry(category.to_string()).or_default();
        let type_name = kind(types, &base, same, RunType { width: w, depth: d, round: found.round, material: material.clone() });
        let height = |z: f64| ((z - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        // The body is the centre line; the justification BIMCraft wrote puts the heights back on the line they were typed at.
        let justify = run_props.get(&id).and_then(|(j, _)| *j).unwrap_or(Justify::Centre);
        let g = crate::runs::Geom { a: found.start, b: found.end, width: w, depth: d, round: found.round };
        let shift = match justify {
            Justify::Top => g.half(),
            Justify::Centre => 0.0,
            Justify::Bottom => -g.half(),
        };
        let (z0, z1) = (height(found.start[2] + shift), height(found.end[2] + shift));
        let system = l
            .group
            .get(&id)
            .and_then(|g| e(*g))
            .filter(|g| matches!(g.name.as_str(), "IFCSYSTEM" | "IFCDISTRIBUTIONSYSTEM"))
            .and_then(|g| text(g.args.get(2)))
            .map(clip)
            .unwrap_or_default();
        let run = Run {
            type_name,
            level,
            start: micron(r.flat(found.start)),
            end: micron(r.flat(found.end)),
            offset: z0,
            end_offset: ((z1 - z0).abs() > 1e-6).then_some(z1),
            justify,
            system,
            params: Default::default(),
        };
        let Some(made) = Element::of_run(category, run) else { continue };
        let rid = m.insert(made);
        from.insert(rid, who(id));
        origin.entry(id).or_insert(rid);
        elements_from.entry(id).or_default().push(rid);
        seen.insert(id);
    }

    // Furniture, sanitary terminals, light fixtures and proxies: components of families made of
    // their bodies, each a mesh in its own axes stood upright, its faces grouped by what they're made
    // of. A family for each name the file gives them ("Family:Type", as Revit and BIMCraft name
    // them), a type for each type; the file's family over the library's of its name.
    let styles = item_styles(&r, &ids);
    // Each family: its category, its types in the order they came (each its forms and its shapes'
    // key), the types by the name the file gives them, and how many forms it has.
    #[derive(Default)]
    struct Made {
        category: Category,
        types: Vec<(String, Vec<Form>, u64)>,
        named: HashMap<String, Vec<usize>>,
        names: HashSet<String>,
        forms: usize,
    }
    let mut made: BTreeMap<String, Made> = BTreeMap::new();
    let mut placed: Vec<(u64, String, String, bool, P2, f64, f64)> = Vec::new();
    let read = Category::read_classes();
    for category in Category::PLACED {
        for id in named(&read) {
            // A mass is read above.
            let Some(x) = e(id).filter(|_| !seen.contains(&id)) else { continue };
            // Its class and its type say which kind of family it is of; one of another kind is read in its turn.
            if Category::from_ifc(&x.name, enumeration(x.args.get(8)), text(x.args.get(4))) != Some(category) {
                continue;
            }
            let f = r.frame(x);
            let flat = |v: P3| unit([v[0], v[1], 0.0]);
            let along = [flat(f.x), flat(f.y), [1.0, 0.0, 0.0]].into_iter().find(|v| dot(*v, *v) > 0.5).unwrap_or([1.0, 0.0, 0.0]);
            let side = cross([0.0, 0.0, 1.0], along);
            let local = |p: P3| {
                let d = sub(f.at(p), f.o);
                [dot(d, along), dot(d, side), d[2]].map(|v| (v * 1e3).round() / 1e3 + 0.0)
            };
            let fallback = element_material(&r, &l, id).unwrap_or_default();
            let mut groups: BTreeMap<String, Vec<[P3; 3]>> = BTreeMap::new();
            for (item, faces) in r.body_items(x) {
                let tris = groups.entry(styles.get(&item).cloned().unwrap_or_else(|| fallback.clone())).or_default();
                for face in faces {
                    tris.extend(triangles(&face.into_iter().map(local).collect::<Vec<_>>()).into_iter().map(settled));
                }
            }
            groups.retain(|_, t| !t.is_empty());
            if groups.is_empty() {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            }
            if groups.values().map(Vec::len).sum::<usize>() > MAX_TRIANGLES {
                leave(&mut skipped, &mut seen, id, &format!("its body has more than {MAX_TRIANGLES} triangles"));
                continue;
            }
            let forms: Vec<Form> =
                groups.into_iter().map(|(material, mesh)| Form { mesh: Arc::new(mesh), material, ..Form::default() }).collect();
            // A product whose object type marks what it is (a lightning terminal) has its family and type in its name.
            let marked = category.ifc().2.is_some();
            let name =
                if marked { None } else { label(id) }.or_else(|| text(x.args.get(2)).map(String::from)).unwrap_or_else(|| x.name.clone());
            let mut parts = name.splitn(3, ':').map(str::trim);
            let family = clip(parts.next().unwrap_or_default());
            let base = clip(parts.next().unwrap_or(&family));
            let fam = made.entry(family.clone()).or_insert_with(|| Made { category, ..Made::default() });
            // A type of that name and shape, or its mirror image, found by its shapes' key; a type the
            // file gives another shape is another type, while the family has room for its forms.
            let flipped = mirrored(&forms);
            let (key, flipped_key) = (shape_key(&forms), shape_key(&flipped));
            let same = fam.named.get(&base).into_iter().flatten().find_map(|&i| {
                let (t, fs, k) = fam.types.get(i)?;
                (*k == key && *fs == forms)
                    .then(|| (t.clone(), false))
                    .or_else(|| (*k == flipped_key && *fs == flipped).then(|| (t.clone(), true)))
            });
            let pick = match same {
                Some(pick) => pick,
                None if fam.forms + forms.len() > crate::family::MAX_FORMS => {
                    let why = format!("family `{family}` has more shapes than one family holds ({} forms)", crate::family::MAX_FORMS);
                    leave(&mut skipped, &mut seen, id, &why);
                    continue;
                }
                None => {
                    let taken = fam.named.get(&base).map_or(0, Vec::len);
                    let t = (taken + 1..)
                        .map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") })
                        .find(|t| !fam.names.contains(t))
                        .unwrap_or_else(|| base.clone());
                    fam.forms += forms.len();
                    fam.names.insert(t.clone());
                    fam.named.entry(base.clone()).or_default().push(fam.types.len());
                    fam.types.push((t.clone(), forms, key));
                    (t, false)
                }
            };
            let (sin, cos) = (along[1], along[0]);
            placed.push((id, family, pick.0, pick.1, r.flat(f.o), (sin.atan2(cos).to_degrees() * 1e6).round() / 1e6 + 0.0, f.o[2]));
        }
    }
    let mut families: BTreeMap<String, Family> = BTreeMap::new();
    for (name, Made { category, types, .. }) in made {
        // Several types: each its own forms, shown by a parameter only its type sets.
        let several = types.len() > 1;
        let mut family = Family { category, ..Family::default() };
        if several {
            family.parameters.push(Parameter { name: "Variant".into(), kind: crate::family::Kind::Number, ..Parameter::default() });
        }
        for (k, (t, forms, _)) in (1..).zip(types) {
            let set = if several { BTreeMap::from([("Variant".to_string(), serde_json::json!(k))]) } else { BTreeMap::new() };
            family.types.insert(t, set);
            family.forms.extend(forms.into_iter().map(|f| Form { visible: several.then(|| Expr::Formula(format!("Variant = {k}"))), ..f }));
        }
        families.insert(name, family);
    }
    // A family that isn't sound leaves its components out, with why: each checked once.
    let unsound: HashMap<String, String> = families.iter().filter_map(|(n, f)| f.check(&|_| true).err().map(|e| (n.clone(), e))).collect();
    for (id, family, type_name, mirrored, at, angle, z) in placed {
        if let Some(why) = unsound.get(&family) {
            leave(&mut skipped, &mut seen, id, why);
            continue;
        }
        let level = level_for(id, z);
        let offset = ((z - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
        let c = Component { family, type_name, level, at: micron(at), angle, offset, mirrored, params: BTreeMap::new() };
        let cid = m.insert(Element::Component(c));
        from.insert(cid, who(id));
        origin.entry(id).or_insert(cid);
        elements_from.entry(id).or_default().push(cid);
        seen.insert(id);
    }
    for (name, f) in families.into_iter().filter(|(n, _)| !unsound.contains_key(n)) {
        m.families.insert(name, f);
    }

    // Stairs: each flight measured from its treads, a landing between each two and the way the next
    // one turns there. The railings among a stair's parts come in along it.
    let mut railings_on: HashMap<u64, Id> = HashMap::new();
    let flights_alone = named(&["IFCSTAIRFLIGHT"]).into_iter().filter(|id| !in_whole(*id, &["IFCSTAIR"]));
    for id in named(&["IFCSTAIR"]).into_iter().filter(|id| !l.whole.contains_key(id)).chain(flights_alone) {
        let parts: Vec<u64> = l.parts.get(&id).cloned().unwrap_or_default();
        seen.insert(id);
        seen.extend(&parts);
        let of = |name: &str| parts.iter().copied().filter(|p| e(*p).is_some_and(|x| x.name == name)).collect::<Vec<u64>>();
        let flights = of("IFCSTAIRFLIGHT");
        let sources = if flights.is_empty() { vec![id] } else { flights };
        let measured: Result<Vec<Flight>, String> = sources.iter().filter_map(|f| e(*f)).map(|s| stair_from(&r, s, &r.body(s))).collect();
        let mut fl = match measured {
            Ok(fl) if !fl.is_empty() => fl,
            Ok(_) => {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            }
            Err(why) => {
                leave(&mut skipped, &mut seen, id, &why);
                continue;
            }
        };
        fl.sort_by(|a, b| a.6.total_cmp(&b.6));
        let tread = |f: &Flight| if f.3 > 1 { f.5 / f64::from(f.3 - 1) } else { f.5 };
        let Some(&first) = fl.first() else { continue };
        let (start, angle, width, _, _, _, z0) = first;
        if fl.iter().any(|f| (f.2 - width).abs() > 1.0 || (tread(f) - tread(&first)).abs() > 1.0) {
            leave(&mut skipped, &mut seen, id, "its flights aren't all as wide, or their treads as deep");
            continue;
        }
        let risers: u32 = fl.iter().map(|f| f.3).sum();
        let rise: f64 = fl.iter().map(|f| f.4).sum();
        let mut below = 0;
        let landings = fl
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => {
                    below += a.3;
                    Some(Landing { at: f64::from(below), turn: turn_between((a.0, a.1, a.5), (b.0, b.1)) })
                }
                _ => None,
            })
            .collect();
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z0 + rise);
        let height = (height * 1000.0).round() / 1000.0;
        // Its riser, from the rise the model will measure, so it counts the same risers.
        let climb = top_level.map_or(height, |t| elevation(t) - elevation(level));
        let stair = Stair {
            params: Default::default(),
            level,
            top_level,
            height,
            start: micron(start),
            angle,
            width: (width * 100.0).round() / 100.0,
            riser: climb / f64::from(risers),
            tread: (tread(&first) * 1000.0).round() / 1000.0,
            landings,
        };
        let sid = m.insert(Element::Stair(stair));
        from.insert(sid, who(id));
        origin.entry(id).or_insert(sid);
        elements_from.entry(id).or_default().push(sid);
        railings_on.extend(of("IFCRAILING").into_iter().map(|p| (p, sid)));
    }

    // Ramps: each flight's slope from its top, landings between them as a stair's.
    for id in named(&["IFCRAMP"]).into_iter().filter(|id| !l.whole.contains_key(id)) {
        let parts: Vec<u64> = l.parts.get(&id).cloned().unwrap_or_default();
        seen.insert(id);
        seen.extend(&parts);
        let of = |name: &str| parts.iter().copied().filter(|p| e(*p).is_some_and(|x| x.name == name)).collect::<Vec<u64>>();
        let flights = of("IFCRAMPFLIGHT");
        let sources = if flights.is_empty() { vec![id] } else { flights };
        let measured: Result<Vec<RampRun>, String> = sources.iter().filter_map(|f| e(*f)).map(|f| ramp_from(&r, &r.body(f))).collect();
        let mut fl = match measured {
            Ok(fl) if !fl.is_empty() => fl,
            Ok(_) => {
                leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
                continue;
            }
            Err(why) => {
                leave(&mut skipped, &mut seen, id, &why);
                continue;
            }
        };
        fl.sort_by(|a, b| a.5.total_cmp(&b.5));
        let Some(&(start, angle, width, _, slope, z0)) = fl.first() else { continue };
        if fl.iter().any(|f| (f.2 - width).abs() > 1.0 || (f.4 - slope).abs() > 0.01) {
            leave(&mut skipped, &mut seen, id, "its flights aren't all as wide, or as steep");
            continue;
        }
        let up = |f: &RampRun| f.5 + f.3 * f.4 / 100.0 - z0;
        let landings = fl
            .windows(2)
            .filter_map(|w| match w {
                [a, b] => Some(Landing { at: (up(a) * 1000.0).round() / 1000.0, turn: turn_between((a.0, a.1, a.3), (b.0, b.1)) }),
                _ => None,
            })
            .collect();
        let rise = fl.last().map_or(0.0, up);
        let level = level_for(id, z0);
        let (top_level, height) = topped(level, z0, z0 + rise);
        let ramp = Ramp {
            params: Default::default(),
            level,
            top_level,
            height: (height * 1000.0).round() / 1000.0,
            start: micron(start),
            angle,
            width: (width * 100.0).round() / 100.0,
            slope: (slope * 1e6).round() / 1e6,
            landings,
        };
        let rid = m.insert(Element::Ramp(ramp));
        from.insert(rid, who(id));
        origin.entry(id).or_insert(rid);
        elements_from.entry(id).or_default().push(rid);
        railings_on.extend(of("IFCRAILING").into_iter().map(|p| (p, rid)));
    }

    // Railings: along their axis, the top of their rail; without one, a straight one along the middle
    // of its body. One among a stair's or a ramp's parts runs along the side its axis starts nearer.
    for id in named(&["IFCRAILING"]) {
        let Some(x) = e(id) else { continue };
        seen.insert(id);
        let Some(axis) = r.axis(x).filter(|a| a.len() >= 2).or_else(|| straight(&r, x)) else {
            leave(&mut skipped, &mut seen, id, "it has no axis for BIMCraft to follow, and it isn't straight");
            continue;
        };
        let first = axis.first().copied().unwrap_or_default();
        let named = label(id);
        let railing = match railings_on.get(&id) {
            Some(&host) => {
                let level = m.elements.get(host).and_then(|h| m.level_of(h)).unwrap_or_default();
                let on = |side| Railing {
                    params: Default::default(),
                    type_name: String::new(),
                    level,
                    path: Vec::new(),
                    base_offset: 0.0,
                    host: Some(Guid::of_id(host)),
                    side: Some(side),
                };
                let start = |side| stairs::railing(&m, &on(side)).first().copied();
                let side = [Side::Left, Side::Right]
                    .into_iter()
                    .filter_map(|side| Some((side, start(side)?)))
                    .min_by(|a, b| a.1.0.dist(r.flat(first)).total_cmp(&b.1.0.dist(r.flat(first))));
                let Some((side, (_, z))) = side else { continue };
                let h = first[2] - z;
                if !h.is_finite() || h <= 0.0 || h > MAX_COORD {
                    leave(&mut skipped, &mut seen, id, "its railing height cannot be measured above its host");
                    continue;
                }
                let base = named.clone().unwrap_or_else(|| format!("Imported - {}mm", num(h)));
                Railing {
                    type_name: kind(&mut m.types.railing, &base, |t| (t.height - h).abs() < 0.5, RailingType { height: h, spacing: 0.0 }),
                    ..on(side)
                }
            }
            None => {
                let pts: Vec<P3> = r.body(x).into_iter().flatten().collect();
                let span = span(pts.iter().map(|p| p[2])).map_or(0.0, |(lo, hi)| hi - lo);
                let h = named.as_ref().and_then(|n| m.types.railing.get(n)).map_or(span, |t| t.height);
                if !h.is_finite() || h <= 0.0 || h > MAX_COORD {
                    leave(&mut skipped, &mut seen, id, "its railing height cannot be measured from its body");
                    continue;
                }
                let base = named.clone().unwrap_or_else(|| format!("Imported - {}mm", num(h)));
                let type_name = kind(&mut m.types.railing, &base, |t| (t.height - h).abs() < 0.5, RailingType { height: h, spacing: 0.0 });
                let level = level_for(id, first[2] - h);
                let path = axis.iter().map(|p| micron(r.flat(*p))).collect();
                let base_offset = ((first[2] - h - elevation(level)) * 1000.0).round() / 1000.0 + 0.0;
                Railing { params: Default::default(), type_name, level, path, base_offset, host: None, side: None }
            }
        };
        let rid = m.insert(Element::Railing(railing));
        from.insert(rid, who(id));
        origin.entry(id).or_insert(rid);
        elements_from.entry(id).or_default().push(rid);
    }

    // Doors and windows: where their openings cut their walls.
    let (mut doors, mut windows) = (0u32, 0u32);
    for id in named(OPENINGS) {
        let Some(o) = e(id) else { continue };
        let is_door = o.name.starts_with("IFCDOOR");
        let opening = l.fills.get(&id).copied();
        let Some(&(host, a, b, base)) = opening.and_then(|op| l.host.get(&op)).and_then(|h| walls.get(h)) else {
            let why = if opening.is_some() { "its wall was left out" } else { "it isn't in a wall" };
            leave(&mut skipped, &mut seen, id, why);
            continue;
        };
        let mut faces = opening.and_then(e).map(|op| r.body(op)).unwrap_or_default();
        if faces.is_empty() {
            faces = r.body(o);
        }
        let u = (b - a).unit();
        let (Some((s0, s1)), Some((z0, z1))) =
            (span(faces.iter().flatten().map(|p| (r.flat(*p) - a).dot(u))), span(faces.iter().flatten().map(|p| p[2])))
        else {
            leave(&mut skipped, &mut seen, id, "it has no body BIMCraft can read");
            continue;
        };
        let size = |x: f64| (x * 100.0).round() / 100.0;
        let width = size(r.len(o.args.get(9)).filter(|w| *w > 0.0).unwrap_or(s1 - s0));
        let height = size(r.len(o.args.get(8)).filter(|h| *h > 0.0).unwrap_or(z1 - z0));
        let sill = size((z0 - base).max(0.0));
        let fallback = format!("Imported {} x {}", num(width), num(height));
        let base_name = label(id).unwrap_or(fallback);
        let same = |k: &OpeningType| (k.width - width).abs() < 0.05 && (k.height - height).abs() < 0.05;
        let new = OpeningType { width, height, sill: if is_door { 0.0 } else { sill } };
        let types = if is_door { &mut m.types.door } else { &mut m.types.window };
        let type_name = kind(types, &base_name, same, new);
        let count = if is_door { &mut doors } else { &mut windows };
        *count += 1;
        let flip_hand = enumeration(o.args.get(11)).is_some_and(|op| op.contains("RIGHT"));
        let op = Opening {
            params: Default::default(),
            family: None,
            type_name,
            host: Guid::of_id(host),
            offset: micron(P2((s0 + s1) / 2.0, 0.0)).0,
            sill,
            mark: count.to_string(),
            flip_facing: false,
            flip_hand,
        };
        let oid = m.insert(if is_door { Element::Door(op) } else { Element::Window(op) });
        from.insert(oid, who(id));
        origin.entry(id).or_insert(oid);
        elements_from.entry(id).or_default().push(oid);
        seen.insert(id);
    }

    // Rooms: a point inside their footprint.
    let numbers: Vec<String> = named(&["IFCSPACE"])
        .into_iter()
        .filter_map(&e)
        .filter(|s| text(s.args.get(7)).is_some())
        .filter_map(|s| text(s.args.get(2)).map(String::from))
        .collect();
    let mut numbers: BTreeSet<String> = numbers.into_iter().collect();
    for id in named(&["IFCSPACE"]) {
        let Some(s) = e(id) else { continue };
        let faces = r.body(s);
        let floor = span(faces.iter().flatten().map(|p| p[2])).map(|(z0, _)| z0);
        let footprint =
            floor.and_then(|z0| outline(&faces, Plane::flat(z0), false)).and_then(|g| inside(&g.outer)).map(|p| r.flat([p.0, p.1, 0.0]));
        let Some(point) = footprint.or_else(|| faces.is_empty().then(|| r.flat(r.frame(s).o))) else {
            leave(&mut skipped, &mut seen, id, "its footprint isn't one BIMCraft can read");
            continue;
        };
        let (long, short) = (text(s.args.get(7)), text(s.args.get(2)));
        let (room_name, number) = match (long, short) {
            (Some(n), Some(num)) => (n.to_string(), num.to_string()),
            (Some(n), None) | (None, Some(n)) => {
                let num = next_number(numbers.iter().map(String::as_str));
                numbers.insert(num.clone());
                (n.to_string(), num)
            }
            (None, None) => {
                let num = next_number(numbers.iter().map(String::as_str));
                numbers.insert(num.clone());
                ("Room".to_string(), num)
            }
        };
        let level = level_for(id, floor.unwrap_or(0.0));
        let rid = m.insert(Element::Room(Room {
            params: Default::default(),
            level,
            point: micron(point),
            name: clip(&room_name),
            number: clip(&number),
        }));
        from.insert(rid, who(id));
        origin.entry(id).or_insert(rid);
        elements_from.entry(id).or_default().push(rid);
        seen.insert(id);
    }

    // Grids: each axis a grid line.
    let mut grid_names = BTreeSet::new();
    for id in named(&["IFCGRID"]) {
        let Some(g) = e(id) else { continue };
        let f = r.frame(g);
        seen.insert(id);
        for axis in [7, 8, 9].into_iter().flat_map(|i| refs(g.args.get(i))).filter_map(&e) {
            let Some(pts) = r.curve(reference(axis.args.get(1)), 0) else { continue };
            let (Some(p), Some(q)) = (pts.first(), pts.last()) else { continue };
            let (mut start, mut end) = (micron(r.flat(f.at(*p))), micron(r.flat(f.at(*q))));
            if enumeration(axis.args.get(2)) == Some("F") {
                (start, end) = (end, start);
            }
            let tag = text(axis.args.first()).map_or_else(|| next_number(grid_names.iter().map(String::as_str)), String::from);
            let gid = m.insert(Element::Grid(Grid { name: fresh(&mut grid_names, &tag), start, end }));
            from.insert(gid, who(id));
            origin.entry(id).or_insert(gid);
            elements_from.entry(id).or_default().push(gid);
        }
    }

    // The project's parameters, from the property sets BIMCraft writes them in: each one's kind
    // from its first value's, for the categories of the elements holding one; up to as many as a
    // project holds, and as many values as `MAX_VALUES`.
    let (mut over, mut values) = (BTreeSet::new(), 0);
    'sets: for rel in named(&["IFCRELDEFINESBYPROPERTIES"]).into_iter().filter_map(e) {
        let Some(set) = reference(rel.args.get(5)).and_then(e).filter(|x| x.name == "IFCPROPERTYSET") else { continue };
        if text(set.args.get(2)) != Some(crate::ifc::PARAMETERS) {
            continue;
        }
        for prop in refs(set.args.get(4)).into_iter().filter_map(e).filter(|x| x.name == "IFCPROPERTYSINGLEVALUE") {
            let Some(name) = text(prop.args.first()).filter(|n| crate::family::nameable(n)) else { continue };
            let (kind, value) = match prop.args.get(2) {
                Some(V::Typed(t, v)) => match (t.as_str(), &**v) {
                    ("IFCTEXT" | "IFCLABEL" | "IFCIDENTIFIER", V::Str(s)) => (ParameterKind::Text, json!(clip(s))),
                    ("IFCBOOLEAN" | "IFCLOGICAL", V::Enum(b)) if b == "T" || b == "F" => (ParameterKind::YesNo, json!(b == "T")),
                    ("IFCLENGTHMEASURE" | "IFCPOSITIVELENGTHMEASURE", v) => match real(Some(v)) {
                        Some(x) => (ParameterKind::Length, json!((x * mm * 1e6).round() / 1e6)),
                        None => continue,
                    },
                    ("IFCREAL" | "IFCINTEGER" | "IFCNUMERICMEASURE" | "IFCCOUNTMEASURE", v) => match real(Some(v)) {
                        Some(x) => (ParameterKind::Number, json!(x)),
                        None => continue,
                    },
                    _ => continue,
                },
                _ => continue,
            };
            for id in refs(rel.args.get(4)).iter().filter_map(|x| elements_from.get(x)).flatten() {
                let Some(el) = m.elements.get(id) else { continue };
                let cat = el.category();
                let theirs = |f: &str| m.families.get(f).is_some_and(|f| f.parameters.iter().any(|p| p.name == name));
                let fits = PARAMETER_CATEGORIES.contains(&cat)
                    && !crate::schedule::own_field(cat, name)
                    && !matches!(el, Element::Component(c) if theirs(&c.family))
                    && m.parameters.get(name).is_none_or(|p| p.kind == kind);
                let p = ProjectParameter { kind, categories: vec![cat.to_string()] };
                if !fits || !p.holds(&value) {
                    continue;
                }
                if !m.parameters.contains_key(name) && m.parameters.len() >= MAX_PROJECT_PARAMETERS {
                    over.insert(name.to_string());
                    continue;
                }
                if values >= MAX_VALUES {
                    skipped.push(format!("project parameter values past the {MAX_VALUES} BIMCraft reads"));
                    break 'sets;
                }
                values += 1;
                let p = m.parameters.entry(name.to_string()).or_insert(p);
                if !p.categories.iter().any(|c| c == cat) {
                    p.categories.push(cat.to_string());
                    p.categories.sort();
                }
                if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                    params.insert(name.to_string(), value.clone());
                }
            }
        }
    }

    if !over.is_empty() {
        skipped.push(format!("{} project parameters: past the {MAX_PROJECT_PARAMETERS} a project holds", over.len()));
    }

    // The project's phases, and what it changed of its phase filters, overrides and design option sets, from the
    // property set on the project; then each element's phase and design option, from its own. What the project
    // can't have is left out with the rule, and the element stays.
    let phased: Vec<(Vec<u64>, BTreeMap<&str, &Entity>)> = named(&["IFCRELDEFINESBYPROPERTIES"])
        .into_iter()
        .filter_map(e)
        .filter_map(|rel| {
            let set = reference(rel.args.get(5))
                .and_then(e)
                .filter(|x| x.name == "IFCPROPERTYSET" && text(x.args.get(2)) == Some(crate::ifc::PHASING))?;
            let props = refs(set.args.get(4)).into_iter().filter_map(e).filter_map(|p| Some((text(p.args.first())?, p))).collect();
            Some((refs(rel.args.get(4)), props))
        })
        .collect();
    let project_id = ids.iter().copied().find(|id| e(*id).is_some_and(|x| x.name == "IFCPROJECT"));
    for (_, props) in phased.iter().filter(|(on, _)| project_id.is_some_and(|p| on.contains(&p))) {
        let mut note = |what: &str, taken: Result<(), String>| {
            if let Err(why) = taken {
                skipped.push(format!("the project's {what}: {why}"));
            }
        };
        if let Some(p) = props.get("Phases") {
            let names = list(p.args.get(2)).iter().map(|v| text(Some(v)).map(String::from)).collect::<Option<Vec<_>>>();
            note("phases", names.ok_or_else(|| "it isn't a list of names".to_string()).and_then(|n| adopt(&mut m, |m| &mut m.phases, n)));
        }
        if let Some(p) = props.get("Phase Filters") {
            note("phase filters", read_json(p).and_then(|f| adopt(&mut m, |m| &mut m.phase_filters, f)));
        }
        if let Some(p) = props.get("Phase Overrides") {
            note("phase overrides", read_json(p).and_then(|o| adopt(&mut m, |m| &mut m.phase_overrides, o)));
        }
        if let Some(p) = props.get("Design Option Sets") {
            note("design option sets", read_json(p).and_then(|s| adopt(&mut m, |m| &mut m.option_sets, s)));
        }
    }
    let (mut unknown, mut touched) = (0, Vec::new());
    for (on, props) in &phased {
        // An implied `Phase Created` is the first phase's name, said for another tool: the element holds none.
        let value = |name: &str| {
            let p = props.get(name).filter(|p| p.name == "IFCPROPERTYSINGLEVALUE")?;
            let implied = name == phasing::CREATED && text(p.args.get(1)) == Some(crate::ifc::IMPLIED);
            text(p.args.get(2)).filter(|_| !implied)
        };
        let values = [phasing::CREATED, phasing::DEMOLISHED, phasing::OPTION].map(|k| (k, value(k)));
        for id in on.iter().filter_map(|x| elements_from.get(x)).flatten() {
            for (key, v) in values.iter().filter_map(|(k, v)| Some((*k, (*v)?))) {
                if m.elements.get(id).and_then(Element::params).is_none() {
                    continue;
                }
                let known = if key == phasing::OPTION { phasing::option_exists(&m, v) } else { phasing::index(&m, v).is_some() };
                match m.elements.get_mut(id).and_then(Element::params_mut) {
                    Some(params) if known => {
                        params.insert(key.into(), json!(v));
                        touched.push(*id);
                    }
                    _ => unknown += 1,
                }
            }
        }
    }
    // Once all are in, since a door's phase is its wall's unless it says: nothing is taken down before it was made.
    touched.sort_unstable();
    touched.dedup();
    let mut early = 0;
    for id in touched {
        if m.elements.get(id).is_some_and(|x| phasing::check_order(&m, "", x).is_err()) {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.remove(phasing::DEMOLISHED);
            }
            early += 1;
        }
    }
    if unknown > 0 {
        skipped.push(format!("{unknown} phase or design option values: not ones the project has"));
    }
    if early > 0 {
        skipped.push(format!("{early} Phase Demolished values: before the phase the element is made in"));
    }

    // The materials the layers are made of, in the library, shaded as the file shades them.
    m.adopt_materials();
    for (name, colour) in material_colours(&r, &ids) {
        if let Some(x) = m.materials.get_mut(&name) {
            x.color = colour;
        }
    }
    // Each element's GUID is the GlobalId of what it was made of, where that's one; what points at an element points at it.
    let mut globals: HashMap<Id, u128> =
        origin.iter().filter_map(|(step, id)| Some((*id, crate::ifc::expand(text(e(*step)?.args.first())?)?))).collect();
    // The rest, GUIDs the file makes: the same file reads the same every time.
    let seed = bytes.iter().fold(0xCBF2_9CE4_8422_2325_u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01B3));
    for id in m.elements.keys() {
        globals.entry(*id).or_insert_with(|| Guid::seeded(seed, *id).as_u128());
    }
    m.elements = m.elements.regiven(&globals);
    m.resolve();
    // The library's GUIDs too, the file makes: the same file reads the same every time.
    m.library = crate::library::Library::default();
    m.forget_parts();
    m.sync_library_with(&mut |_, kind, name| {
        let n = format!("{}/{name}", kind.map_or_else(|| "part".to_string(), Kind::to_string))
            .bytes()
            .fold(0xCBF2_9CE4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3));
        Guid::seeded(seed, n)
    });
    // What breaks a rule is left out, with the rule; a wall takes its doors and windows along.
    for _ in 0..=m.elements.len() {
        let Err(error) = m.check() else { break };
        let id = culprit(&error).filter(|id| from.contains_key(id)).ok_or_else(|| error.clone())?;
        let why = error.split_once(": ").map_or(error.as_str(), |(_, why)| why);
        skipped.push(format!("{}: {why}", from.get(&id).cloned().unwrap_or_default()));
        let gone = m.guid(id);
        m.elements.remove(id);
        let hosted: Vec<Id> = m
            .elements
            .iter()
            .filter_map(|(k, x)| match x {
                Element::Door(o) | Element::Window(o) if o.host == gone => Some(*k),
                _ => None,
            })
            .collect();
        for h in hosted {
            skipped.push(format!("{}: its wall was left out", from.get(&h).cloned().unwrap_or_default()));
            m.elements.remove(h);
        }
    }
    m.check()?;

    // The rest of what's in the building, by kind.
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for id in l.within.keys().filter(|id| !seen.contains(id)) {
        if let Some(x) = e(*id) {
            *left.entry(x.name.as_str()).or_default() += 1;
        }
    }
    skipped.extend(
        left.into_iter().map(|(name, n)| format!("{n} {name}: BIMCraft has no element for {}", if n == 1 { "it" } else { "them" })),
    );
    let moved = (r.shift != P2(0.0, 0.0)).then_some(r.shift * -1.0);
    Ok(Imported { model: m, schema: file.schema.clone(), skipped, moved })
}

/// A roof as `roof_from` reads it: over its footprint, round its courtyards, less its openings,
/// flat or each edge but its gables sloping at `slope`; or a profile swept across, its heights up
/// from `z`. Its thickness, and `z`: how high its underside is at its eaves, or its profile's 0.
struct RoofRead {
    boundary: Vec<P2>,
    courtyards: Vec<Vec<P2>>,
    openings: Vec<Vec<P2>>,
    slope: f64,
    gables: Vec<usize>,
    extrusion: Option<Extrusion>,
    t: f64,
    z: f64,
}

fn roof_from(r: &Reader, faces: &[Vec<P3>], cut: &[Vec<P2>]) -> Result<RoofRead, String> {
    let round = |x: f64| (x * 100.0).round() / 100.0;
    let faces: Vec<Vec<P3>> = faces
        .iter()
        .map(|f| {
            f.iter()
                .map(|p| {
                    let q = r.flat(*p);
                    [q.0, q.1, p[2]]
                })
                .collect()
        })
        .collect();
    let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
        return Err("it has no body BIMCraft can read".into());
    };
    // Its faces turned up, each with its plane (none for a sliver of a mesh's): their loops its
    // outline and its openings.
    let up: Vec<(&Vec<P3>, Plane)> =
        faces.iter().filter(|f| f.len() >= 3 && normal(f)[2] > 0.01 && area3(f) > 1.0).filter_map(|f| Some((f, Plane::of(f)?))).collect();
    let mut rings: Vec<Vec<P2>> =
        loops(up.iter().map(|(f, _)| *f)).into_iter().map(|l| geom::corners(&l)).filter(|l| l.len() >= 3).collect();
    rings.sort_by(|a, b| geom::area(b).abs().total_cmp(&geom::area(a).abs()));
    // Each from its south-west corner, the outline counter-clockwise, the openings in order: as
    // the same file reads the same.
    let mut rings = rings.into_iter().map(|l| {
        let mut l: Vec<P2> = l.into_iter().map(micron).collect();
        let first = l.iter().enumerate().min_by(|(_, a), (_, b)| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap_or(std::cmp::Ordering::Equal));
        let first = first.map_or(0, |(i, _)| i);
        l.rotate_left(first);
        l
    });
    let mut boundary = rings.next().ok_or("its top has no outline")?;
    if geom::area(&boundary) < 0.0 {
        boundary.reverse();
        boundary.rotate_right(1);
    }
    // Its holes, less those the openings through it `cut`, courtyards or not.
    let mut openings: Vec<Vec<P2>> = rings.filter(|h| !cut.iter().any(|c| same(h, c))).collect();
    openings.sort_by(|a, b| a.first().map(|p| (p.1, p.0)).partial_cmp(&b.first().map(|p| (p.1, p.0))).unwrap_or(std::cmp::Ordering::Equal));
    let sloped: Vec<&(&Vec<P3>, Plane)> = up.iter().filter(|(_, p)| p.g.length() > 0.02).collect();
    let Some(&&(main, along)) = sloped.iter().max_by(|a, b| area3(a.0).total_cmp(&area3(b.0))) else {
        return Ok(RoofRead {
            boundary,
            courtyards: Vec::new(),
            openings,
            slope: 0.0,
            gables: Vec::new(),
            extrusion: None,
            t: round(z1 - z0),
            z: z0,
        });
    };
    // Its thickness: from its largest slope's top to the underside parallel to it.
    let n = normal(main);
    let top = dot(n, main.first().copied().unwrap_or_default());
    let t = faces
        .iter()
        .filter(|f| dot(normal(f), n) < -0.999)
        .map(|f| (top - dot(n, f.first().copied().unwrap_or_default())).abs())
        .filter(|t| *t > 1.0)
        .fold(f64::INFINITY, f64::min);
    if !t.is_finite() {
        return Err("its underside isn't parallel to its top".into());
    }
    let t = round(t);
    let rise = |p: &Plane| t * p.g.length().hypot(1.0);
    // One slope all round: each slope rising from an edge of its footprint at its eaves, where the
    // top is lowest; an edge whose middle is higher stands upright.
    let tan = along.g.length();
    let slope = round(tan.atan().to_degrees());
    let lowest = up.iter().flat_map(|(f, _)| f.iter().map(|p| p[2])).fold(f64::INFINITY, f64::min);
    let touches = |f: &Vec<P3>, q: P2| {
        let ring: Vec<P2> = f.iter().map(|p| P2(p[0], p[1])).collect();
        geom::contains(&ring, q) || geom::ring_edges(&ring).any(|(a, b)| geom::dist_to_segment(q, a, b) < 1.0)
    };
    let top_at = |q: P2| up.iter().filter(|(f, _)| touches(f, q)).map(|(_, p)| p.at(q)).fold(f64::INFINITY, f64::min);
    // A hole with eaves along it is a courtyard, clockwise as the slopes round it rise from it; one
    // cut through the slopes, an opening.
    let (courtyards, openings): (Vec<Vec<P2>>, Vec<Vec<P2>>) =
        openings.into_iter().partition(|h| geom::ring_edges(h).any(|(a, b)| top_at((a + b) * 0.5) <= lowest + 1.0));
    let courtyards: Vec<Vec<P2>> = courtyards
        .into_iter()
        .map(|mut c| {
            if geom::area(&c) > 0.0 {
                c.reverse();
                c.rotate_right(1);
            }
            c
        })
        .collect();
    let rings: Vec<&Vec<P2>> = std::iter::once(&boundary).chain(&courtyards).collect();
    let edges: Vec<(P2, P2)> = rings.iter().flat_map(|r| geom::ring_edges(r)).collect();
    let gables: Vec<usize> =
        (0..edges.len()).filter(|i| edges.get(*i).is_some_and(|(a, b)| top_at((*a + *b) * 0.5) > lowest + 1.0)).collect();
    let eave = |(a, b): &(P2, P2)| {
        let inward = (*b - *a).unit().perp();
        Plane { z: lowest - tan * a.dot(inward), g: inward * tan }
    };
    let eaves: Vec<Plane> = edges.iter().enumerate().filter(|(i, _)| !gables.contains(i)).map(|(_, e)| eave(e)).collect();
    let fits = |(f, p): &&(&Vec<P3>, Plane)| {
        let q = f.first().map_or(P2(0.0, 0.0), |q| P2(q[0], q[1]));
        eaves.iter().any(|e| e.g.dist(p.g) < 1e-4 && (e.at(q) - p.at(q)).abs() < 1.0)
    };
    let speeds: Vec<f64> = (0..edges.len()).map(|i| if gables.contains(&i) { 0.0 } else { 1.0 }).collect();
    let ridge = crate::skeleton::faces(&rings, &speeds)
        .map(|f| f.into_iter().flatten().flatten().map(|(_, k)| lowest + k * tan).fold(lowest, f64::max))
        .unwrap_or(f64::NAN);
    if sloped.iter().all(|(_, p)| (p.g.length() - tan).abs() < 1e-4) && sloped.iter().all(fits) && (ridge - z1).abs() < 1.0 {
        return Ok(RoofRead { boundary, courtyards, openings, slope, gables, extrusion: None, t, z: lowest - rise(&along) });
    }
    // Else one profile swept across: its slopes all one way, its underside's heights along it.
    let u = along.g.unit();
    let u = if u.0 > 1e-9 || (u.0.abs() <= 1e-9 && u.1 > 0.0) { u } else { u * -1.0 };
    if up.iter().any(|(_, p)| p.g.cross(u).abs() > 1e-4) {
        return Err("its slopes don't make a roof BIMCraft draws: one slope all round, or one profile swept across".into());
    }
    let unders: Vec<(f64, f64, Plane)> = up
        .iter()
        .filter_map(|(f, p)| {
            let (s0, s1) = span(f.iter().map(|q| P2(q[0], q[1]).dot(u)))?;
            Some((s0, s1, Plane { z: p.z - rise(p), ..*p }))
        })
        .collect();
    let mut cuts: Vec<f64> = unders.iter().flat_map(|(s0, s1, _)| [*s0, *s1]).collect();
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    let at = |s: f64, p: &Plane| p.z + p.g.dot(u) * s;
    let mut profile: Vec<P2> = Vec::new();
    for w in cuts.windows(2) {
        let &[a, b] = w else { continue };
        let p = unders
            .iter()
            .find(|(s0, s1, _)| *s0 <= (a + b) / 2.0 && (a + b) / 2.0 <= *s1)
            .map(|(.., p)| *p)
            .ok_or("its slopes leave a gap")?;
        match profile.last() {
            Some(last) if (last.1 - at(a, &p)).abs() > 1.0 => return Err("its underside steps up or down".into()),
            Some(_) => {}
            None => profile.push(P2(a, at(a, &p))),
        }
        // Straight on from the stretch before, one stretch.
        if let [.., x, y] = profile.as_slice()
            && ((y.1 - x.1) / (y.0 - x.0) - p.g.dot(u)).abs() < 1e-6
        {
            profile.pop();
        }
        profile.push(P2(b, at(b, &p)));
    }
    let across: Vec<f64> = up.iter().flat_map(|(f, _)| f.iter().map(|q| P2(q[0], q[1]).dot(u.perp()))).collect();
    let (Some((c0, c1)), Some(first), Some(last)) = (span(across.into_iter()), profile.first().copied(), profile.last().copied()) else {
        return Err("it has no body BIMCraft can read".into());
    };
    if ((last.0 - first.0) * (c1 - c0) - geom::area(&boundary)).abs() > 0.01 * geom::area(&boundary)
        || !openings.is_empty()
        || !courtyards.is_empty()
    {
        return Err("its outline isn't the rectangle its profile sweeps".into());
    }
    let z = profile.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let o = u * first.0 + u.perp() * c0;
    let line = [micron(o), micron(o + u * (last.0 - first.0))];
    let profile = profile.into_iter().map(|p| P2(round(p.0 - first.0), round(p.1 - z))).collect();
    Ok(RoofRead {
        boundary: Vec::new(),
        courtyards: Vec::new(),
        openings,
        slope: 0.0,
        gables: Vec::new(),
        extrusion: Some(Extrusion { line, profile, from: 0.0, to: round(c1 - c0) }),
        t,
        z,
    })
}

/// A straight flight: where its first riser's middle is, the way it climbs (degrees), its width,
/// how many risers, how high they climb, its run, and the height its first riser starts from.
type Flight = (P2, f64, f64, u32, f64, f64, f64);

/// A ramp's straight flight: the middle of its foot, the way it climbs (degrees), its width, its
/// run, its slope (percent), and the height of its foot.
type RampRun = (P2, f64, f64, f64, f64, f64);

/// How the run from one flight (its start, the way it climbs in degrees, its length) to the next
/// (its start and way) turns at the landing between: straight on, left or right, or back to
/// whichever side it lies on; not a turn BIMCraft makes, not a number.
fn turn_between((a, along, _): (P2, f64, f64), (b, toward): (P2, f64)) -> f64 {
    let d = (toward - along).rem_euclid(360.0);
    let d = if d > 180.0 { d - 360.0 } else { d };
    match d {
        d if d.abs() < 1.0 => 0.0,
        d if (d - 90.0).abs() < 1.0 => 90.0,
        d if (d + 90.0).abs() < 1.0 => -90.0,
        d if d.abs() > 179.0 => {
            if heading(along).cross(b - a) > 0.0 {
                180.0
            } else {
                -180.0
            }
        }
        _ => f64::NAN,
    }
}

/// A railing with no axis, straight and thin: the line along the middle of its body, along its
/// own x or y, at its top.
fn straight(r: &Reader, x: &Entity) -> Option<Vec<P3>> {
    let pts: Vec<P3> = r.body(x).into_iter().flatten().collect();
    let f = r.frame(x);
    let (_, top) = span(pts.iter().map(|p| p[2]))?;
    let spread = |u: P2| span(pts.iter().map(|p| P2(p[0], p[1]).dot(u)));
    let along = [P2(f.x[0], f.x[1]), P2(f.y[0], f.y[1])]
        .into_iter()
        .filter(|u| u.length() > 0.5)
        .map(|u| u.unit())
        .max_by(|a, b| spread(*a).map_or(0.0, |(lo, hi)| hi - lo).total_cmp(&spread(*b).map_or(0.0, |(lo, hi)| hi - lo)))?;
    let ((s0, s1), (c0, c1)) = (spread(along)?, spread(along.perp())?);
    // Thin: no wider than a rail and its balusters could be.
    (c1 - c0 <= 300.0 && s1 - s0 > TOL).then(|| {
        let at = |s: f64| {
            let p = along * s + along.perp() * ((c0 + c1) / 2.0);
            [p.0, p.1, top]
        };
        vec![at(s0), at(s1)]
    })
}

/// A ramp's flight, from its sloping top: up its slope, across it, and how steep.
fn ramp_from(r: &Reader, faces: &[Vec<P3>]) -> Result<RampRun, String> {
    let (top, geom::Region { outer: ring, .. }, _) = top_of(faces).ok_or("its top isn't one flat face BIMCraft can read")?;
    let tan = top.g.length();
    if tan < 1e-6 {
        return Err("its top is level".into());
    }
    let (u, n) = (top.g * (1.0 / tan), (top.g * (1.0 / tan)).perp());
    let (Some((s0, s1)), Some((c0, c1))) = (span(ring.iter().map(|p| p.dot(u))), span(ring.iter().map(|p| p.dot(n)))) else {
        return Err("it has no body BIMCraft can read".into());
    };
    let foot = u * s0 + n * ((c0 + c1) / 2.0);
    let angle = (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 + 0.0;
    Ok((r.flat([foot.0, foot.1, 0.0]), angle, c1 - c0, s1 - s0, tan * 100.0, top.at(foot)))
}

fn stair_from(r: &Reader, s: &Entity, faces: &[Vec<P3>]) -> Result<Flight, String> {
    let Some((z0, z1)) = span(faces.iter().flatten().map(|p| p[2])) else {
        return Err("it has no body BIMCraft can read".into());
    };
    // The treads: faces turned up, above the floor, one height each.
    let mut treads: Vec<(f64, P2)> = Vec::new();
    for f in faces.iter().filter(|f| normal(f)[2] > 0.99) {
        let z = f.iter().map(|p| p[2]).sum::<f64>() / f.len() as f64;
        let c = f.iter().fold(P2(0.0, 0.0), |c, p| c + r.flat(*p)) * (1.0 / f.len() as f64);
        if z > z0 + 1.0 && !treads.iter().any(|(tz, _)| (tz - z).abs() < 1.0) {
            treads.push((z, c));
        }
    }
    treads.sort_by(|a, b| a.0.total_cmp(&b.0));
    // A flight's own count, else one riser up to each tread and the last up to the floor above.
    let counted = real(s.args.get(8)).filter(|_| s.name == "IFCSTAIRFLIGHT").filter(|n| (2.0..=200.0).contains(n)).map(|n| n as u32);
    let risers = counted.unwrap_or(u32::try_from(treads.len()).unwrap_or(0) + 1);
    if risers < 2 {
        return Err("BIMCraft can't find its treads".into());
    }
    let each = r.len(s.args.get(10)).filter(|_| counted.is_some()).filter(|h| *h > 0.0).unwrap_or((z1 - z0) / f64::from(risers - 1));
    // It climbs along whichever of its own axes its treads spread out along.
    let f = r.frame(s);
    let axes = [P2(f.x[0], f.x[1]).unit(), P2(f.y[0], f.y[1]).unit()];
    let spread = |u: &P2| span(treads.iter().map(|(_, c)| c.dot(*u))).map_or(0.0, |(lo, hi)| hi - lo);
    let mut u =
        axes.into_iter().filter(|u| u.length() > 0.5).max_by(|a, b| spread(a).total_cmp(&spread(b))).ok_or("it doesn't stand upright")?;
    if let (Some((_, low)), Some((_, high))) = (treads.first(), treads.last())
        && (*high - *low).dot(u) < 0.0
    {
        u = u * -1.0;
    }
    let plan: Vec<P2> = faces.iter().flatten().map(|p| r.flat(*p)).collect();
    let n = u.perp();
    let (Some((s0, s1)), Some((c0, c1))) = (span(plan.iter().map(|p| p.dot(u))), span(plan.iter().map(|p| p.dot(n)))) else {
        return Err("it has no body BIMCraft can read".into());
    };
    let start = u * s0 + n * ((c0 + c1) / 2.0);
    let angle = (u.1.atan2(u.0).to_degrees() * 1e6).round() / 1e6 + 0.0;
    let from = treads.first().map_or(z0, |(z, _)| z - each);
    Ok((start, angle, c1 - c0, risers, each * f64::from(risers), s1 - s0, from))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An IFC4 file in mm whose wall is a triangle mesh with no axis, running north, and whose
    /// column takes its shape from its type through a mapped item; `site` places it all.
    fn meshes(site: &str) -> String {
        let pts = "(0.,-100.,0.),(5000.,-100.,0.),(5000.,100.,0.),(0.,100.,0.),(0.,-100.,2700.),(5000.,-100.,2700.),(5000.,100.,2700.),(0.,100.,2700.)";
        let tris = "(1,3,2),(1,4,3),(5,6,7),(5,7,8),(1,2,6),(1,6,5),(2,3,7),(2,7,6),(3,4,8),(3,8,7),(4,1,5),(4,5,8)";
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Meshes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT(#62,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
#10=IFCCARTESIANPOINTLIST3D(({pts}));
#11=IFCTRIANGULATEDFACESET(#10,$,.T.,({tris}),$);
#12=IFCSHAPEREPRESENTATION($,'Body','Tessellation',(#11));
#13=IFCPRODUCTDEFINITIONSHAPE($,$,(#12));
#14=IFCCARTESIANPOINT((1000.,2000.,0.));
#15=IFCDIRECTION((0.,1.,0.));
#16=IFCDIRECTION((0.,0.,1.));
#17=IFCAXIS2PLACEMENT3D(#14,#16,#15);
#18=IFCLOCALPLACEMENT(#6,#17);
#19=IFCWALL('w',$,'Wall',$,$,#18,#13,$,$);
#30=IFCRECTANGLEPROFILEDEF(.AREA.,$,$,300.,500.);
#31=IFCEXTRUDEDAREASOLID(#30,$,#16,3000.);
#32=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#31));
#33=IFCREPRESENTATIONMAP(#5,#32);
#34=IFCCARTESIANTRANSFORMATIONOPERATOR3D($,$,#4,$,$);
#35=IFCMAPPEDITEM(#33,#34);
#36=IFCSHAPEREPRESENTATION($,'Body','MappedRepresentation',(#35));
#37=IFCPRODUCTDEFINITIONSHAPE($,$,(#36));
#38=IFCCARTESIANPOINT((3000.,4000.,0.));
#39=IFCAXIS2PLACEMENT3D(#38,#16,$);
#40=IFCLOCALPLACEMENT(#6,#39);
#41=IFCCOLUMN('c',$,'Column',$,$,#40,#37,$,$);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#19,#41),#7);
#60=IFCCARTESIANPOINT(({site}));
#61=IFCAXIS2PLACEMENT3D(#60,$,$);
#62=IFCLOCALPLACEMENT($,#61);
ENDSEC;END-ISO-10303-21;"
        )
    }

    fn only<'a>(m: &'a Model, cat: &str) -> Vec<&'a Element> {
        m.elements.values().filter(|e| e.category() == cat).collect()
    }

    #[test]
    fn an_unmeasurable_railing_does_not_poison_the_whole_import() {
        let original = meshes("0.,0.,0.");
        let extra = "#70=IFCCARTESIANPOINT((0.,0.,0.));\n#71=IFCCARTESIANPOINT((5000.,0.,0.));\n#72=IFCPOLYLINE((#70,#71));\n#73=IFCSHAPEREPRESENTATION($,'Axis','Curve3D',(#72));\n#74=IFCPRODUCTDEFINITIONSHAPE($,$,(#73));\n#75=IFCRAILING('rail',$,'Unmeasurable rail',$,$,#6,#74,$,$);\n";
        let text =
            original.replace("(#19,#41)", "(#19,#41,#75)").replace("ENDSEC;END-ISO-10303-21;", &format!("{extra}ENDSEC;END-ISO-10303-21;"));
        let imported = ifc(text.as_bytes(), "Synthetic").unwrap();
        imported.model.check().unwrap();
        assert_eq!(only(&imported.model, "wall").len(), 1);
        assert_eq!(only(&imported.model, "railing").len(), 0);
        assert!(imported.skipped.iter().any(|s| s.contains("railing height")), "{:?}", imported.skipped);
    }

    #[test]
    fn meshes_and_mapped_shapes_come_in_as_extrusions_do() {
        let i = ifc(meshes("0.,0.,0.").as_bytes(), "x").unwrap();
        assert_eq!((i.skipped.len(), i.moved), (0, None), "{:?}", i.skipped);
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        assert_eq!((w.start, w.end, i.model.thickness(w), w.height), (P2(1000.0, 2000.0), P2(1000.0, 7000.0), 200.0, 2700.0));
        let Some(Element::Column(c)) = only(&i.model, "column").first().copied() else { panic!("no column") };
        let t = &i.model.types.column[&c.type_name];
        assert_eq!((c.at, c.angle, t.width, t.depth, t.round, c.height), (P2(3000.0, 4000.0), 90.0, 500.0, 300.0, false, 3000.0));
    }

    /// An IFC4 file in mm with a 6 x 4 m slab 200 mm thick, its profile round two holes, an
    /// opening element voiding it through the east one, as a mesh would carry it: `slab` makes the
    /// slab #35, and what it's part of; the storey holds `within`.
    fn holed(slab: &str, within: u32) -> String {
        let rect = |id: u32, (x0, y0): (f64, f64), (x1, y1): (f64, f64)| {
            let pts: Vec<String> = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
                .iter()
                .enumerate()
                .map(|(i, (x, y))| format!("#{}=IFCCARTESIANPOINT(({x:.1},{y:.1}));", id + 1 + i as u32))
                .collect();
            format!("{}\n#{id}=IFCPOLYLINE((#{},#{},#{},#{},#{}));", pts.join("\n"), id + 1, id + 2, id + 3, id + 4, id + 1)
        };
        format!(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Holes',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,-200.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,$,$,$,.ELEMENT.,0.);
{}
{}
{}
#30=IFCARBITRARYPROFILEDEFWITHVOIDS(.AREA.,$,#10,(#20,#25));
#31=IFCDIRECTION((0.,0.,1.));
#32=IFCEXTRUDEDAREASOLID(#30,$,#31,200.);
#33=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#32));
#34=IFCPRODUCTDEFINITIONSHAPE($,$,(#33));
{slab}
#40=IFCCARTESIANPOINT((4500.,1500.));
#41=IFCAXIS2PLACEMENT2D(#40,$);
#42=IFCRECTANGLEPROFILEDEF(.AREA.,$,#41,1000.,1000.);
#43=IFCEXTRUDEDAREASOLID(#42,$,#31,200.);
#44=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#43));
#45=IFCPRODUCTDEFINITIONSHAPE($,$,(#44));
#46=IFCOPENINGELEMENT('o',$,'Hole',$,$,#6,#45,$,.OPENING.);
#47=IFCRELVOIDSELEMENT('v',$,$,$,#35,#46);
#50=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,(#{within}),#7);
ENDSEC;END-ISO-10303-21;",
            rect(10, (0.0, 0.0), (6000.0, 4000.0)),
            rect(20, (1000.0, 1000.0), (2500.0, 2000.0)),
            rect(25, (4000.0, 1000.0), (5000.0, 2000.0)),
        )
    }

    /// `file` with 150 mm of brick then 50 of plaster on element `#id`, its usage stacking them
    /// along its own `axis` from where its offset starts them.
    fn layered(file: &str, id: u32, axis: &str, offset: f64) -> String {
        let set = "#80=IFCMATERIAL('Brick',$,$);
#81=IFCMATERIAL('Plaster',$,$);
#82=IFCMATERIALLAYER(#80,150.,$,$,$,'Finish',$);
#83=IFCMATERIALLAYER(#81,50.,$,$,$,'LoadBearing',$);
#84=IFCMATERIALLAYERSET((#82,#83),'Brick and plaster',$);";
        let usage = format!(
            "#85=IFCMATERIALLAYERSETUSAGE(#84,.{axis}.,.POSITIVE.,{offset:.1},$);\n#86=IFCRELASSOCIATESMATERIAL('m',$,$,$,(#{id}),#85);"
        );
        file.replace("ENDSEC;END", &format!("{set}\n{usage}\nENDSEC;END"))
    }

    #[test]
    fn layers_stacked_up_an_elements_own_axis_start_from_its_back() {
        // The wall runs north, its own y to the west: its first layer is east, so it runs south.
        let i = ifc(layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0).as_bytes(), "x").unwrap();
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        let names = |layers: &[Layer]| layers.iter().map(|l| l.material.clone()).collect::<Vec<_>>();
        assert_eq!(
            (w.start, w.end, names(&i.model.types.wall[&w.type_name].layers)),
            (P2(1000.0, 7000.0), P2(1000.0, 2000.0), vec!["Brick".into(), "Plaster".into()])
        );
        // The slab's first layer is at its bottom.
        let i = ifc(layered(&holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35), 35, "AXIS3", 0.0).as_bytes(), "x").unwrap();
        let Some(Element::Floor(f)) = only(&i.model, "floor").first().copied() else { panic!("no floor") };
        assert_eq!(names(&i.model.types.floor[&f.type_name].layers), ["Plaster", "Brick"]);
    }

    /// Each component's extent in the model: its outlines' corners and its heights, to the mm.
    fn extent(m: &Model, c: &crate::model::Component) -> (i64, i64, i64, i64, i64, i64) {
        let pieces = m.component_pieces(c);
        let pts: Vec<P2> = pieces.iter().flat_map(|p| p.outline.clone()).collect();
        let r = |x: f64| x.round() as i64;
        let (x, y) = (span(pts.iter().map(|p| p.0)).unwrap_or_default(), span(pts.iter().map(|p| p.1)).unwrap_or_default());
        let z = span(pieces.iter().flat_map(|p| [p.bottom, p.top])).unwrap_or_default();
        (r(x.0), r(x.1), r(y.0), r(y.1), r(z.0), r(z.1))
    }

    #[test]
    fn project_parameters_go_out_in_a_property_set_and_come_back_with_their_kinds() {
        let mut s = crate::engine::Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [4000, 0]}
               room {"point": [4000, 3000]}
               component {"family": "Chair", "at": [2000, 2000]}
               parameter {"name": "Fire Rating", "categories": ["wall", "door"]}
               parameter {"name": "Cost", "kind": "number", "categories": ["door", "component"]}
               parameter {"name": "Finish Height", "kind": "length", "categories": ["room"]}
               parameter {"name": "Accessible", "kind": "yesno", "categories": ["door", "room"]}"#,
        )
        .unwrap();
        let ids = |m: &Model, cat: &str| -> Vec<Id> { m.elements.iter().filter(|(_, e)| e.category() == cat).map(|(id, _)| *id).collect() };
        let (wall, door, room, chair) =
            (ids(&s.model, "wall")[0], ids(&s.model, "door")[0], ids(&s.model, "room")[0], ids(&s.model, "component")[0]);
        for (id, values) in [
            (wall, json!({"Fire Rating": "60 min"})),
            (door, json!({"Fire Rating": "30 min", "Cost": 450.5, "Accessible": true})),
            (room, json!({"Finish Height": 2400, "Accessible": false})),
            (chair, json!({"Cost": 80})),
        ] {
            s.execute("modify", &json!({"id": id, "params": values})).unwrap();
        }
        let file = crate::ifc::export(&s.model);
        assert_eq!(file.matches("IFCRELDEFINESBYPROPERTIES(").count(), 4);
        assert!(file.contains("IFCPROPERTYSINGLEVALUE('Finish Height',$,IFCLENGTHMEASURE(2400.),$)"), "{file}");
        assert!(file.contains("IFCPROPERTYSINGLEVALUE('Accessible',$,IFCBOOLEAN(.T.),$)"));
        let i = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        assert_eq!(i.model.parameters, s.model.parameters, "each kind, for the categories holding one");
        // Numbers as numbers: 2400 and 2400.0 alike.
        let values = |m: &Model, cat: &str| -> Option<BTreeMap<String, serde_json::Value>> {
            let params = m.elements.values().find(|e| e.category() == cat).and_then(Element::params)?;
            Some(params.iter().map(|(k, v)| (k.clone(), v.as_f64().map_or_else(|| v.clone(), |x| json!(x)))).collect())
        };
        for cat in ["wall", "door", "room", "component"] {
            assert_eq!(values(&i.model, cat), values(&s.model, cat), "{cat}");
        }
        // In metres, a length comes in in millimetres.
        let metres = file.replace(".MILLI.,.METRE.", "$,.METRE.").replace("IFCLENGTHMEASURE(2400.)", "IFCLENGTHMEASURE(2.4)");
        let i = ifc(metres.as_bytes(), "x").unwrap();
        assert_eq!(values(&i.model, "room").and_then(|p| p.get("Finish Height").cloned()), Some(json!(2400.0)));
    }

    #[test]
    fn a_property_set_of_more_parameters_than_a_project_holds_comes_in_up_to_them() {
        let mut s = crate::engine::Session::new();
        s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
        let file = crate::ifc::export(&s.model);
        // Every wall, related to one set of more parameters than a project holds.
        let walls: Vec<String> =
            file.lines().filter(|l| l.contains("=IFCWALL(")).filter_map(|l| l.split('=').next().map(String::from)).collect();
        let n = crate::model::MAX_PROJECT_PARAMETERS + 50;
        let mut extra = String::new();
        let props: Vec<String> = (0..n)
            .map(|i| {
                extra.push_str(&format!("#{}=IFCPROPERTYSINGLEVALUE('P{i}',$,IFCREAL(1.),$);\n", 900_000 + i));
                format!("#{}", 900_000 + i)
            })
            .collect();
        extra.push_str(&format!("#899998=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaaa',$,'Project Parameters',$,({}));\n", props.join(",")));
        extra.push_str(&format!("#899999=IFCRELDEFINESBYPROPERTIES('0bbbbbbbbbbbbbbbbbbbbb',$,$,$,({}),#899998);\n", walls.join(",")));
        let file = file.replacen("ENDSEC;\nEND-ISO", &format!("{extra}ENDSEC;\nEND-ISO"), 1);
        let i = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(i.model.parameters.len(), crate::model::MAX_PROJECT_PARAMETERS);
        assert!(i.skipped.iter().any(|s| s.contains("parameter")), "{:?}", i.skipped);
    }

    #[test]
    fn components_come_back_from_ifc_where_they_stood() {
        let mut s = crate::engine::Session::new();
        s.script(
            r#"component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
               component {"family": "WC", "at": [-500, 0]}
               component {"family": "WC", "at": [500, 0], "mirrored": true, "offset": 50}
               component {"family": "Bed", "at": [6000, 4000], "angle": -90}
               level {"elevation": 3000, "name": "Level 3"}"#,
        )
        .unwrap();
        let file = crate::ifc::export(&s.model);
        let i = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let comps = |m: &Model| -> Vec<crate::model::Component> {
            m.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c.clone()) } else { None }).collect()
        };
        let (mut before, mut after) = (comps(&s.model), comps(&i.model));
        before.sort_by(|a, b| a.family.cmp(&b.family));
        after.sort_by(|a, b| a.family.cmp(&b.family));
        assert_eq!(after.len(), 4);
        for (a, b) in before.iter().zip(&after) {
            assert_eq!((&a.family, &a.type_name, a.level), (&b.family, &b.type_name, b.level));
            assert_eq!(extent(&s.model, a), extent(&i.model, b), "{} {}", a.family, a.type_name);
        }
        let of = |family: &str| after.iter().rfind(|c| c.family == family).unwrap();
        assert_eq!((of("Table").angle, of("WC").offset), (30.0, 50.0));
        // A mirror image of a type is that type, mirrored.
        let mirrored: Vec<(&str, bool)> = after.iter().filter(|c| c.family == "WC").map(|c| (c.type_name.as_str(), c.mirrored)).collect();
        assert_eq!(mirrored, [("400 x 650", false), ("400 x 650", true)]);
        // Its forms, as the file had them: in their materials, which come in with their colours.
        let bed = &i.model.families["Bed"];
        let mut made_of: Vec<&str> = bed.forms.iter().map(|f| f.material.as_str()).collect();
        made_of.sort_unstable();
        made_of.dedup();
        assert_eq!(made_of, ["Fabric", "Timber"]);
        assert_eq!(i.model.materials["Fabric"].color, s.model.materials["Fabric"].color);
    }

    /// An IFC4 file in mm whose proxies are named `name` and are each a profile through `ring`'s
    /// corners, scaled by its number, extruded `depth` along `dir`.
    fn proxies(name: &str, n: usize, ring: &[(f64, f64)], dir: &str, depth: f64) -> String {
        let mut out = String::from(
            "ISO-10303-21;HEADER;FILE_SCHEMA(('IFC4'));ENDSEC;DATA;
#1=IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.);
#2=IFCUNITASSIGNMENT((#1));
#3=IFCPROJECT('p',$,'Proxies',$,$,$,$,$,#2);
#4=IFCCARTESIANPOINT((0.,0.,0.));
#5=IFCAXIS2PLACEMENT3D(#4,$,$);
#6=IFCLOCALPLACEMENT($,#5);
#7=IFCBUILDINGSTOREY('s',$,'Level 0',$,$,#6,$,$,.ELEMENT.,0.);
",
        );
        let mut ids = Vec::new();
        for k in 0..n {
            let base = 100 + k * 20;
            let scale = 1.0 + k as f64 / 10.0;
            let pts: Vec<String> = ring
                .iter()
                .enumerate()
                .map(|(i, (x, y))| format!("#{}=IFCCARTESIANPOINT(({:.1},{:.1}));", base + i, x * scale, y * scale))
                .collect();
            let refs: Vec<String> = (0..ring.len()).chain([0]).map(|i| format!("#{}", base + i)).collect();
            out.push_str(&format!(
                "{}\n#{l}=IFCPOLYLINE(({}));\n#{pr}=IFCARBITRARYCLOSEDPROFILEDEF(.AREA.,$,#{l});\n#{d}=IFCDIRECTION(({dir}));\n#{x}=IFCEXTRUDEDAREASOLID(#{pr},$,#{d},{depth:.1});\n#{rep}=IFCSHAPEREPRESENTATION($,'Body','SweptSolid',(#{x}));\n#{sh}=IFCPRODUCTDEFINITIONSHAPE($,$,(#{rep}));\n#{at}=IFCCARTESIANPOINT(({}.,0.,0.));\n#{ax}=IFCAXIS2PLACEMENT3D(#{at},$,$);\n#{pl}=IFCLOCALPLACEMENT(#6,#{ax});\n#{e}=IFCBUILDINGELEMENTPROXY('g{k}',$,'{name}',$,$,#{pl},#{sh},$,.ELEMENT.);\n",
                pts.join("\n"),
                refs.join(","),
                k * 3000,
                l = base + 10,
                pr = base + 11,
                d = base + 12,
                x = base + 13,
                rep = base + 14,
                sh = base + 15,
                at = base + 16,
                ax = base + 17,
                pl = base + 18,
                e = base + 19,
            ));
            ids.push(format!("#{}", base + 19));
        }
        out.push_str(&format!("#99=IFCRELCONTAINEDINSPATIALSTRUCTURE('r',$,$,$,({}),#7);\nENDSEC;END-ISO-10303-21;", ids.join(",")));
        out
    }

    /// A closed surface's volume: its triangles' signed tetrahedra, positive while they face out.
    fn volume(pieces: &[crate::family::Piece]) -> f64 {
        pieces.iter().flat_map(|p| p.tris.iter()).map(|[a, b, c]| dot(*a, cross(*b, *c)) / 6.0).sum()
    }

    #[test]
    fn a_clockwise_profile_or_a_downward_sweep_comes_in_turned_out() {
        let clockwise = [(0.0, 0.0), (0.0, 300.0), (500.0, 300.0), (500.0, 0.0)];
        for (dir, ring) in [("0.,0.,1.", &clockwise[..]), ("0.,0.,-1.", &[(0.0, 0.0), (500.0, 0.0), (500.0, 300.0), (0.0, 300.0)][..])] {
            let i = ifc(proxies("Box", 1, ring, dir, 450.0).as_bytes(), "x").unwrap();
            let Some(Element::Component(c)) = only(&i.model, "component").first().copied() else { panic!("no component") };
            let v = volume(&i.model.component_local_pieces(c));
            assert!((v - 500.0 * 300.0 * 450.0).abs() < 1.0, "{dir}: {v}");
        }
    }

    #[test]
    fn more_shapes_of_one_family_than_it_holds_leave_only_the_rest_out() {
        let square = [(0.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0)];
        let n = crate::family::MAX_FORMS + 3;
        let i = ifc(proxies("Thing:A", n, &square, "0.,0.,1.", 400.0).as_bytes(), "x").unwrap();
        assert_eq!(i.model.families["Thing"].types.len(), crate::family::MAX_FORMS);
        assert_eq!(only(&i.model, "component").len(), crate::family::MAX_FORMS);
        assert_eq!(i.skipped.len(), 3, "{:?}", i.skipped);
        assert!(i.skipped.iter().all(|s| s.contains("Thing")), "{:?}", i.skipped);
    }

    #[test]
    fn a_style_that_holds_itself_shades_its_material_once() {
        let style = "#90=IFCMATERIALDEFINITIONREPRESENTATION($,$,(#91),#80);
#91=IFCSTYLEDREPRESENTATION($,'Style','Material',(#92));
#92=IFCSTYLEDITEM($,(#93),$);
#93=IFCSURFACESTYLE('Brick',.BOTH.,(#93,#94));
#94=IFCSURFACESTYLESHADING(#95,$);
#95=IFCCOLOURRGB($,1.,0.,0.);";
        let file = layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0).replace("ENDSEC;END", &format!("{style}\nENDSEC;END"));
        let i = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(i.model.materials["Brick"].color, [255, 0, 0]);
    }

    #[test]
    fn a_layer_of_no_thickness_is_a_membrane() {
        let file = layered(&meshes("0.,0.,0."), 19, "AXIS2", -100.0)
            .replace("IFCMATERIALLAYERSET((#82,#83)", "IFCMATERIALLAYERSET((#82,#87,#83)")
            .replace("ENDSEC;END", "#87=IFCMATERIALLAYER(#81,0.,$,$,$,'Finish',$);\nENDSEC;END");
        let i = ifc(file.as_bytes(), "x").unwrap();
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        let layers = &i.model.types.wall[&w.type_name].layers;
        assert_eq!(
            layers.iter().map(|l| (l.function, l.thickness)).collect::<Vec<_>>(),
            [(Function::Finish, 150.0), (Function::Membrane, 0.0), (Function::Structure, 50.0)]
        );
    }

    #[test]
    fn holes_in_a_slab_come_in_as_its_openings_or_as_the_shafts_through_it() {
        let i = ifc(holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let Some(Element::Floor(f)) = only(&i.model, "floor").first().copied() else { panic!("no floor") };
        assert_eq!(f.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
        let Some(Element::Shaft(sh)) = only(&i.model, "shaft").first().copied() else { panic!("no shaft") };
        assert_eq!(geom::area(&sh.boundary).abs(), 1_000_000.0);
        let floor = i.model.elements.iter().find(|(_, e)| e.category() == "floor").map(|(id, _)| *id);
        assert_eq!(crate::element_json(&i.model, floor.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
    }

    #[test]
    fn holes_in_a_ceiling_come_in_as_its_openings_or_as_the_shafts_through_it() {
        let i = ifc(holed("#35=IFCCOVERING('f',$,'Ceiling',$,$,#6,#34,$,.CEILING.);", 35).as_bytes(), "x").unwrap();
        assert_eq!(i.skipped, Vec::<String>::new());
        let Some(Element::Ceiling(c)) = only(&i.model, "ceiling").first().copied() else { panic!("no ceiling") };
        assert_eq!(c.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
        assert_eq!(only(&i.model, "shaft").len(), 1);
        let ceiling = i.model.elements.iter().find(|(_, e)| e.category() == "ceiling").map(|(id, _)| *id);
        assert_eq!(crate::element_json(&i.model, ceiling.unwrap_or_default())["area_m2"], 21.5, "6 x 4 less 1.5 and 1");
    }

    #[test]
    fn holes_in_a_roof_come_in_as_its_openings_or_as_the_shafts_through_it() {
        // The slab as a roof, then as the part of one: the opening voids it either way.
        let part = "#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.ROOF.);
#36=IFCROOF('r',$,'Roof',$,$,#6,$,$,.FLAT_ROOF.);
#37=IFCRELAGGREGATES('a',$,$,$,#36,(#35));";
        for (slab, within) in [("#35=IFCROOF('f',$,'Roof',$,$,#6,#34,$,.FLAT_ROOF.);", 35), (part, 36)] {
            let i = ifc(holed(slab, within).as_bytes(), "x").unwrap();
            assert_eq!(i.skipped, Vec::<String>::new());
            let Some(Element::Roof(roof)) = only(&i.model, "roof").first().copied() else { panic!("no roof") };
            assert_eq!(roof.openings, [[P2(1000.0, 1000.0), P2(1000.0, 2000.0), P2(2500.0, 2000.0), P2(2500.0, 1000.0)]]);
            assert_eq!(only(&i.model, "shaft").len(), 1);
        }
    }

    #[test]
    fn a_file_reads_the_same_every_time() {
        // Its slab's and its shaft's rings, and a second project, in metres, after its own: each
        // read hashes afresh, so whatever came in a hash's order would change from read to read.
        let more = "#60=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);\n#61=IFCUNITASSIGNMENT((#60));\n#62=IFCPROJECT('q',$,'Other',$,$,$,$,$,#61);";
        let file = holed("#35=IFCSLAB('f',$,'Slab',$,$,#6,#34,$,.FLOOR.);", 35).replace("ENDSEC;END", &format!("{more}\nENDSEC;END"));
        // Each read is a project of its own, its uid seeding its own GlobalIds; the rest the same.
        let read = || Model { uid: 0, ..ifc(file.as_bytes(), "x").unwrap().model };
        let first = read();
        for _ in 0..8 {
            assert_eq!(read(), first);
        }
    }

    #[test]
    fn a_model_at_map_coordinates_comes_near_the_origin() {
        let i = ifc(meshes("500000000.,5000000000.,0.").as_bytes(), "x").unwrap();
        let Some(Element::Wall(w)) = only(&i.model, "wall").first().copied() else { panic!("no wall") };
        assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(0.0, 5000.0)));
        assert_eq!(i.moved, Some(P2(-500_001_000.0, -5_000_002_000.0)));
    }

    /// Where element `e` of `m` stands, to tell it from the others of its category: a wall's middle, a door's and a window's the
    /// middle of their wall, a component's point, a railing's the length of its host's category (none: 0); there is one of each
    /// of the others in these projects.
    fn spot(m: &Model, e: &Element) -> Option<(i64, i64)> {
        let at = |p: P2| (p.0.round() as i64, p.1.round() as i64);
        Some(match e {
            Element::Wall(w) => at((w.start + w.end) * 0.5),
            Element::Door(o) | Element::Window(o) => at(m.wall(o.host).map(|w| (w.start + w.end) * 0.5)?),
            Element::Component(c) => at(c.at),
            Element::Railing(r) => (r.host.and_then(|h| m.elements.get(h)).map_or(0, |h| h.category().len() as i64), 0),
            _ => (0, 0),
        })
    }

    /// What each element of `m` holds in its parameters, by category and where it stands.
    fn held(m: &Model) -> BTreeMap<(&'static str, (i64, i64)), serde_json::Value> {
        m.elements.values().filter_map(|e| Some(((e.category(), spot(m, e)?), json!(e.params()?)))).collect()
    }

    /// What each element of `m` that can be phased is in phase `phase`, keyed as `held` keys them.
    fn standing(m: &Model, phase: &str) -> BTreeMap<(&'static str, (i64, i64)), Option<crate::phasing::Status>> {
        let at = crate::phasing::index(m, phase).unwrap_or(0);
        m.elements
            .values()
            .filter(|e| e.params().is_some())
            .filter_map(|e| Some(((e.category(), spot(m, e)?), crate::phasing::status(m, e, at))))
            .collect()
    }

    /// A project's phasing, as a project file holds it.
    fn phasing_of(m: &Model) -> serde_json::Value {
        json!({"phases": m.phases, "filters": m.phase_filters, "overrides": m.phase_overrides, "sets": m.option_sets})
    }

    #[test]
    fn phases_and_design_options_survive_save_as_ifc_and_open() {
        use crate::graphics::testing::{house, run};
        let mut s = crate::engine::Session::new();
        let walls = house(&mut s);
        let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap();
        run(&mut s, "window", json!({"at": [8000, 3000]}));
        let late = run(&mut s, "door", json!({"at": [8000, 1500]}))["id"].as_u64().unwrap();
        let room = run(&mut s, "room", json!({"point": [4000, 3000]}))["id"].as_u64().unwrap();
        let chair = run(&mut s, "component", json!({"family": "Chair", "at": [2000, 2000]}))["id"].as_u64().unwrap();
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "phase", json!({"name": "Landscape"}));
        let filter = json!({"name": "Renovation", "new": "overridden", "existing": "by-category", "demolished": "not-displayed", "temporary": "not-displayed"});
        run(&mut s, "phase_filter", filter);
        run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"], "primary": "Glass"}));
        run(&mut s, "option", json!({"set": "Roof", "options": ["Hip", "Gable", "Flat"]}));
        for (id, values) in [
            (walls[1], json!({"Phase Created": "New Construction"})),
            (walls[2], json!({"Phase Demolished": "Demolition", "Design Option": "Facade: Brick"})),
            (walls[3], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"})),
            // The first phase, said and not: a door that says it doesn't follow its wall, even one made later, and neither
            // does a chair.
            (door, json!({"Phase Created": "Existing"})),
            (late, json!({"Phase Created": "Existing"})),
            (room, json!({"Design Option": "Roof: Gable"})),
            (chair, json!({"Phase Created": "Existing", "Design Option": "Facade: Glass"})),
        ] {
            run(&mut s, "modify", json!({"id": id, "params": values}));
        }
        let path = std::env::temp_dir().join(format!("bimcraft-phasing-{}.ifc", std::process::id()));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        let mut t = crate::engine::Session::new();
        let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(r["skipped"], json!([]));
        assert_eq!(phasing_of(&t.model), phasing_of(&s.model), "the phases in order, the filters, the overrides and the sets");
        assert_eq!(t.model.phases, ["Existing", "Demolition", "New Construction", "Landscape"]);
        let (before, after) = (held(&s.model), held(&t.model));
        assert_eq!(after.len(), 9, "four walls, two doors, a window, a room and a chair");
        assert_eq!(after, before, "every element holds what it held, and the others hold none");
        // What it comes to, in every phase: the window follows its wall into the new phase, and the door made in the first
        // is there before the wall it is in.
        for phase in &s.model.phases {
            assert_eq!(standing(&t.model, phase), standing(&s.model, phase), "in {phase}");
        }
        assert_eq!(standing(&s.model, "Existing")[&("door", (8000, 3000))], Some(crate::phasing::Status::New));
        assert_eq!(standing(&s.model, "Existing")[&("wall", (8000, 3000))], None);
    }

    #[test]
    fn every_kind_of_element_that_holds_parameters_keeps_its_phase_and_option() {
        use crate::graphics::testing::run;
        let mut s = crate::engine::Session::new();
        s.script(
            r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
               door {"at": [4000, 0]}
               window {"at": [8000, 3000]}
               room {"point": [4000, 3000]}
               floor {}
               ceiling {"point": [4000, 3000]}
               roof {}
               column {"at": [3000, 5500]}
               component {"family": "Chair", "at": [2000, 2000]}
               phase {"name": "Landscape"}
               option {"set": "Facade", "options": ["Brick", "Glass"]}"#,
        )
        .unwrap();
        let stair =
            run(&mut s, "stair", json!({"start": [20000, 0], "angle": 90, "width": 1100, "landings": [{"at": 9, "turn": 180}]}))["id"]
                .clone();
        run(&mut s, "railing", json!({"host": stair}));
        let ramp =
            run(&mut s, "ramp", json!({"start": [30000, 0], "angle": 0, "height": 450, "landings": [{"at": 300, "turn": -90}]}))["id"]
                .clone();
        run(&mut s, "railing", json!({"host": ramp, "side": "left"}));
        run(&mut s, "railing", json!({"path": [[0, -500], [3000, -500]]}));
        // Each of them holds something of its own: the kinds that have more than one differ within the kind.
        let values = [
            json!({"Phase Created": "New Construction"}),
            json!({"Phase Demolished": "New Construction"}),
            json!({"Design Option": "Facade: Glass"}),
            json!({"Phase Created": "Existing", "Design Option": "Facade: Brick"}),
            json!({"Phase Created": "New Construction", "Phase Demolished": "Landscape", "Design Option": "Facade: Glass"}),
        ];
        let ids: Vec<Id> = s.model.elements.iter().filter(|(_, e)| e.params().is_some()).map(|(id, _)| *id).collect();
        for (i, id) in ids.iter().enumerate() {
            run(&mut s, "modify", json!({"id": id, "params": values[i % values.len()]}));
        }
        let path = std::env::temp_dir().join(format!("bimcraft-phasing-kinds-{}.ifc", std::process::id()));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        let mut t = crate::engine::Session::new();
        let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(r["skipped"], json!([]));
        let (before, after) = (held(&s.model), held(&t.model));
        let kinds: BTreeSet<&str> = before.keys().map(|(cat, _)| *cat).collect();
        assert_eq!(kinds.len(), 12, "all twelve kinds that hold parameters: {kinds:?}");
        assert_eq!(after, before);
        for phase in &s.model.phases {
            assert_eq!(standing(&t.model, phase), standing(&s.model, phase), "in {phase}");
        }
    }

    #[test]
    fn names_in_other_scripts_and_with_quotes_come_back_whole() {
        use crate::graphics::testing::{house, run};
        let mut s = crate::engine::Session::new();
        run(&mut s, "new", json!({"language": "pt"}));
        let walls = house(&mut s);
        // What a STEP string and the JSON inside one both have to escape, and what neither writes in ASCII.
        let (phase, set, option) = (r#"O'Neil "Anexo" \ fase ç 😀"#, "Fachada", r#"Vidro "temperado""#);
        run(&mut s, "phase", json!({"name": phase}));
        run(&mut s, "option", json!({"set": set, "options": [option, "Tijolo"]}));
        run(&mut s, "phase_filter", json!({"name": r#"Só o "novo""#, "new": "by-category", "existing": "not-displayed"}));
        run(&mut s, "modify", json!({"id": walls[1], "params": {"Phase Created": phase, "Design Option": format!("{set}: {option}")}}));
        run(&mut s, "modify", json!({"id": walls[2], "params": {"Phase Created": "Nova construção"}}));
        let path = std::env::temp_dir().join(format!("bimcraft-phasing-names-{}.ifc", std::process::id()));
        run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
        let mut t = crate::engine::Session::new();
        let r = run(&mut t, "open", json!({"path": path.to_str().unwrap()}));
        let _ = std::fs::remove_file(&path);
        assert_eq!(r["skipped"], json!([]));
        assert_eq!(phasing_of(&t.model), phasing_of(&s.model));
        assert_eq!(t.model.phases, ["Existente", "Nova construção", phase]);
        assert_eq!(held(&t.model), held(&s.model));
    }

    /// The file BIMCraft writes of a house with a wall made in a later phase, one made then and taken down in another, one in
    /// an option, and the project's own filter and overrides.
    fn phased_file() -> String {
        use crate::graphics::testing::{house, run};
        let mut s = crate::engine::Session::new();
        let walls = house(&mut s);
        run(&mut s, "phase", json!({"name": "Demolition", "before": "New Construction"}));
        run(&mut s, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
        run(&mut s, "phase_filter", json!({"name": "Renovation", "new": "overridden", "temporary": "not-displayed"}));
        run(&mut s, "phase", json!({"overrides": {"new": {"cut": {"color": [0, 0, 200]}}}}));
        for (id, values) in [
            (walls[1], json!({"Phase Created": "New Construction"})),
            (walls[2], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"})),
            (walls[3], json!({"Design Option": "Facade: Glass"})),
        ] {
            run(&mut s, "modify", json!({"id": id, "params": values}));
        }
        crate::ifc::export(&s.model)
    }

    #[test]
    fn an_ifc_without_the_phasing_set_opens_as_a_new_project_does() {
        let new = Model::new("x");
        let usual = phasing_of(&new);
        let mut s = crate::engine::Session::new();
        s.script(crate::sample::SAMPLE).unwrap();
        // A file from another tool, and one of BIMCraft's own that uses no phases.
        for file in [meshes("0.,0.,0."), crate::ifc::export(&s.model)] {
            let i = ifc(file.as_bytes(), "x").unwrap();
            assert_eq!(phasing_of(&i.model), usual);
            assert!(held(&i.model).values().all(|v| v == &json!({})), "no element holds a phase or an option");
            // A set another tool calls its own, with a `Phase Created` in it, isn't BIMCraft's to read.
            let walls: Vec<String> =
                file.lines().filter(|l| l.contains("=IFCWALL(")).filter_map(|l| l.split('=').next().map(String::from)).collect();
            let foreign = format!(
                "#899990=IFCPROPERTYSINGLEVALUE('Phase Created',$,IFCLABEL('New Construction'),$);\n\
                 #899991=IFCPROPERTYSET('0aaaaaaaaaaaaaaaaaaaaa',$,'Phasing',$,(#899990));\n\
                 #899992=IFCRELDEFINESBYPROPERTIES('0bbbbbbbbbbbbbbbbbbbbb',$,$,$,({}),#899991);\n",
                walls.join(",")
            );
            let end = file.rfind("ENDSEC;").unwrap();
            let theirs = format!("{}{foreign}{}", &file[..end], &file[end..]);
            assert!(theirs.contains("'Phasing'"));
            let i = ifc(theirs.as_bytes(), "x").unwrap();
            assert_eq!(phasing_of(&i.model), usual);
            assert!(held(&i.model).values().all(|v| v == &json!({})), "{:?}", held(&i.model));
        }
    }

    #[test]
    fn a_phasing_set_that_breaks_a_rule_is_left_out_and_the_elements_stay() {
        let file = phased_file();
        let base = ifc(file.as_bytes(), "x").unwrap();
        assert_eq!(base.skipped, Vec::<String>::new());
        let good = held(&base.model);
        let wall = |x, y| ("wall", (x, y));
        // The walls: the south one holds nothing, the east a phase, the north a phase and a demolition, the west an option.
        assert_eq!(good[&wall(8000, 3000)], json!({"Phase Created": "New Construction"}));
        assert_eq!(good[&wall(4000, 6000)], json!({"Phase Created": "Demolition", "Phase Demolished": "New Construction"}));
        assert_eq!(good[&wall(0, 3000)], json!({"Design Option": "Facade: Glass"}));
        let text = |name: &str, value: &str| format!("IFCPROPERTYSINGLEVALUE('{name}',$,IFCTEXT('{value}'),$)");
        // (what to change in the file, what is then left out of the model, and what the import says).
        let cases = [
            (
                text("Phase Created", "New Construction"),
                text("Phase Created", "Nowhere"),
                vec![(wall(8000, 3000), json!({}))],
                "phase or design option",
            ),
            (
                text("Phase Demolished", "New Construction"),
                text("Phase Demolished", "Existing"),
                vec![(wall(4000, 6000), json!({"Phase Created": "Demolition"}))],
                "before",
            ),
            (
                text("Design Option", "Facade: Glass"),
                text("Design Option", "Roof: Gable"),
                vec![(wall(0, 3000), json!({}))],
                "phase or design option",
            ),
            // A project that can't have these phases has none of the values that name a phase it doesn't.
            (
                "IFCTEXT('Existing'),IFCTEXT('Demolition')".into(),
                "IFCTEXT('Existing'),IFCTEXT('Existing')".into(),
                vec![
                    (wall(4000, 6000), json!({"Phase Demolished": "New Construction"})),
                    (wall(8000, 3000), json!({"Phase Created": "New Construction"})),
                ],
                "the project's phases",
            ),
            // A list with something in it that isn't a name.
            (
                "IFCTEXT('Existing'),IFCTEXT('Demolition')".into(),
                "IFCTEXT('Existing'),IFCINTEGER(7)".into(),
                vec![(wall(4000, 6000), json!({"Phase Demolished": "New Construction"}))],
                "the project's phases: it isn't a list of names",
            ),
            (r#"{"Facade":"#.into(), r#"{"Facade"#.into(), vec![(wall(0, 3000), json!({}))], "the project's design option sets"),
            (r#""primary":"Brick""#.into(), r#""primary":"Nope""#.into(), vec![(wall(0, 3000), json!({}))], "its primary"),
            (r#""temporary":"not-displayed""#.into(), r#""temporary":"gone""#.into(), vec![], "the project's phase filters"),
            ("[0,0,200]".into(), "[0,0,999]".into(), vec![], "the project's phase overrides"),
        ];
        for (from, to, left_out, says) in cases {
            assert!(file.contains(&from), "{from}");
            let i = ifc(file.replacen(&from, &to, 1).as_bytes(), "x").unwrap_or_else(|e| panic!("{to}: {e}"));
            assert!(i.skipped.iter().any(|s| s.contains(says)), "{to}: {says}: {:?}", i.skipped);
            let mut want = good.clone();
            want.extend(left_out);
            let (got, usual) = (held(&i.model), phasing_of(&Model::new("x")));
            assert_eq!(got, want, "{to}");
            // What can't be had is the project's own, not a mix: it has the usual one.
            let kept = phasing_of(&i.model);
            match says {
                s if s.starts_with("the project's phases") => assert_eq!(kept["phases"], usual["phases"]),
                "the project's phase filters" => assert_eq!(kept["filters"], usual["filters"]),
                "the project's phase overrides" => assert_eq!(kept["overrides"], usual["overrides"]),
                "the project's design option sets" | "its primary" => assert_eq!(kept["sets"], usual["sets"]),
                _ => assert_eq!(kept, phasing_of(&base.model), "{to}"),
            }
        }
    }

    /// `file` with every property called `name` holding `value`, in STEP.
    fn with_value(file: &str, name: &str, value: &str) -> String {
        file.lines()
            .map(|l| match l.split_once('=') {
                Some((head, body)) if body.starts_with(&format!("IFCPROPERTYSINGLEVALUE('{name}',")) => {
                    format!("{head}=IFCPROPERTYSINGLEVALUE('{name}',$,{value},$);")
                }
                Some((head, body)) if body.starts_with(&format!("IFCPROPERTYLISTVALUE('{name}',")) => {
                    format!("{head}=IFCPROPERTYLISTVALUE('{name}',$,{value},$);")
                }
                _ => l.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn nothing_a_phasing_set_holds_stops_a_file_from_opening_or_takes_an_element_with_it() {
        let file = phased_file();
        let base = ifc(file.as_bytes(), "x").unwrap();
        let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
        let nasty = [
            "$".to_string(),
            "IFCTEXT('')".into(),
            "IFCTEXT('x')".into(),
            "IFCTEXT('{')".into(),
            "IFCTEXT('null')".into(),
            "IFCTEXT('[]')".into(),
            "IFCTEXT('{}')".into(),
            r#"IFCTEXT('{"a":1}')"#.into(),
            r#"IFCTEXT('"x"')"#.into(),
            r#"IFCTEXT('{"": {"options": [], "primary": ""}}')"#.into(),
            r#"IFCTEXT('{"S": {"options": ["a"], "primary": "a"}, "T: U": {"options": ["a"], "primary": "a"}}')"#.into(),
            format!("IFCTEXT('{deep}')"),
            format!("IFCTEXT('{}')", "x".repeat(300)),
            "IFCTEXT('\\X2\\D83D\\X0\\')".into(),
            "IFCINTEGER(7)".into(),
            "IFCREAL(1.5)".into(),
            "IFCBOOLEAN(.T.)".into(),
            "(IFCTEXT('a'),IFCTEXT('a'))".into(),
            format!("({})", vec!["IFCTEXT('p')"; 200].join(",")),
            "()".into(),
            "#999".into(),
        ];
        for name in
            ["Phases", "Phase Filters", "Phase Overrides", "Design Option Sets", "Phase Created", "Phase Demolished", "Design Option"]
        {
            for value in &nasty {
                let i = ifc(with_value(&file, name, value).as_bytes(), "x").unwrap_or_else(|e| panic!("{name} = {value:.60}: {e}"));
                assert_eq!(i.model.elements.len(), base.model.elements.len(), "{name} = {value:.60}");
                assert!(i.model.check().is_ok(), "{name} = {value:.60}");
            }
        }
    }
}
