//! The 3D view: the model as solids (walls with their doors and windows cut out, floor slabs and
//! ceilings with the shafts through them cut out, door and window panels, columns, stairs' steps, roofs' slopes) in an axonometric projection, as Revit's default 3D view shows it. A
//! depth buffer keeps the nearest surface at each pixel; faces are shaded by how they turn to
//! the light, and an edge is drawn wherever the plane a pixel shows changes, so coplanar faces
//! (joined walls' tops, a face's triangles) read as one. The picture keeps which element each
//! pixel shows, for picking. Sections and elevations use the same pictures, clipped at their
//! line and in hidden line, and cut the same solids.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use crate::geom::{self, P2};
use crate::graphics::{Look, VisualStyle};
use crate::model::{Element, Id, Layer, Model};
use crate::paint;
use crate::render::{PAPER, Rgb, clip};
use crate::stairs;
use crate::texture::Sampler;
use crate::walls::{self, Axis, Plane};

/// A model point in mm: x east, y north, z up.
pub type P3 = [f64; 3];

/// Revit's default 3D view looks from the south-east, from above.
pub const AZIMUTH: f64 = -45.0;
pub const ELEVATION: f64 = 30.0;
/// The largest picture, in px a side.
pub const MAX_SIDE: u32 = 4096;
/// Where faces meet.
pub const EDGE: Rgb = Rgb(48, 48, 48);
/// What a level's plane is outlined in.
const DATUM: Rgb = Rgb(60, 100, 150);
const WALL: Rgb = Rgb(236, 236, 232);
const SLAB: Rgb = Rgb(214, 214, 210);
const DOOR: Rgb = Rgb(198, 168, 128);
const GLASS: Rgb = Rgb(166, 200, 228);
const ROOF: Rgb = Rgb(176, 118, 96);
const CEILING: Rgb = Rgb(244, 242, 236);
const MASS: Rgb = Rgb(158, 190, 224);
const RAIL: Rgb = Rgb(120, 122, 128);
/// A beam, a footing, a structural column made of nothing the library has.
const STRUCTURE: Rgb = Rgb(182, 184, 190);
/// A component's form made of nothing the library has.
const COMPONENT: Rgb = Rgb(214, 204, 188);
/// Room around a fitted model, px.
const MARGIN: f64 = 30.0;

pub(crate) fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub(crate) fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(crate) fn mul(a: P3, k: f64) -> P3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
pub(crate) fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(crate) fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(crate) fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { [0.0; 3] }
}

/// A triangle of an element's surface; on a curved one, which (its element's, and which of its
/// curved surfaces: a wall's outside or inside, a component's by its first triangle's place in the
/// scene), so the facets read as one.
struct Tri {
    p: [P3; 3],
    owner: Id,
    color: Rgb,
    /// The texture over its colour: one more than where its material is in the library; 0, none.
    tex: u32,
    curve: Option<(Id, usize)>,
}

/// A wall's plan frame: its start, along it, and square to it.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    o: P2,
    u: P2,
    n: P2,
}

impl Frame {
    /// A box from `a.0` to `a.1` along, `c.0` to `c.1` across and `z.0` to `z.1` up.
    fn cuboid(&self, out: &mut Vec<Tri>, owner: Id, (color, tex): (Rgb, u32), a: (f64, f64), c: (f64, f64), z: (f64, f64)) {
        let at = |a: f64, c: f64, z: f64| {
            let p = self.o + self.u * a + self.n * c;
            [p.0, p.1, z]
        };
        let v = [
            at(a.0, c.0, z.0),
            at(a.1, c.0, z.0),
            at(a.1, c.1, z.0),
            at(a.0, c.1, z.0),
            at(a.0, c.0, z.1),
            at(a.1, c.0, z.1),
            at(a.1, c.1, z.1),
            at(a.0, c.1, z.1),
        ];
        for [i, j, k, l] in [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            out.push(Tri { p: [v[i], v[j], v[k]], owner, color, tex, curve: None });
            out.push(Tri { p: [v[i], v[k], v[l]], owner, color, tex, curve: None });
        }
    }
}

/// A solid of the model: a box (a door's leaf, a window's glass, a stair's step); a slab: a flat
/// face, level or sloped, and what lies up to `rise` mm above it (a floor, a column, a roof's
/// slope); a prism: a ring in plan from one plane up to another (a wall's part; of a curved wall,
/// with its centre and its centre line's radius, its faces being one surface each); or triangles
/// (a component's form, which sections show and don't cut; a round run's tube, convex, which they do, as `cut` says): a round
/// one's sides, past `caps`, are one curved surface; any other's facets that turn little where they meet are ([`facets`]).
pub(crate) enum Solid {
    Box { owner: Id, color: Rgb, tex: u32, frame: Frame, a: (f64, f64), c: (f64, f64), z: (f64, f64) },
    Slab { owner: Id, color: Rgb, tex: u32, face: Vec<P3>, rise: f64 },
    Prism { owner: Id, color: Rgb, tex: u32, ring: Vec<P2>, bottom: Plane, top: Plane, curve: Option<(P2, f64)> },
    Mesh { owner: Id, color: Rgb, tex: u32, tris: Vec<[P3; 3]>, caps: usize, round: bool, cut: bool },
}

/// A level slab: `ring` from `z.0` up to `z.1`.
fn slab(owner: Id, color: Rgb, ring: &[P2], z: (f64, f64)) -> Solid {
    Solid::Slab { owner, color, tex: 0, face: ring.iter().map(|p| [p.0, p.1, z.0]).collect(), rise: z.1 - z.0 }
}

fn plan(face: &[P3]) -> Vec<P2> {
    face.iter().map(|p| P2(p[0], p[1])).collect()
}

impl Solid {
    pub(crate) fn owner(&self) -> Id {
        match self {
            Solid::Box { owner, .. } | Solid::Slab { owner, .. } | Solid::Prism { owner, .. } | Solid::Mesh { owner, .. } => *owner,
        }
    }

    #[cfg(test)]
    pub(crate) fn color(&self) -> Rgb {
        match self {
            Solid::Box { color, .. } | Solid::Slab { color, .. } | Solid::Prism { color, .. } | Solid::Mesh { color, .. } => *color,
        }
    }

    /// From its bottom to its top.
    pub(crate) fn heights(&self) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                let (lo, hi) = face.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
                (lo, hi + rise)
            }
            Solid::Prism { ring, bottom, top, .. } => {
                ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(bottom.at(*q)), hi.max(top.at(*q))))
            }
            Solid::Mesh { tris, .. } => tris.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2]))),
        }
    }

    /// Its bottom and top over plan point `q`.
    pub(crate) fn z_at(&self, q: P2) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                // The face's plane, by Newell's normal, over `q`.
                let n = face.iter().zip(face.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)));
                let o = face.first().copied().unwrap_or_default();
                let z = if n[2].abs() > 1e-9 { o[2] - (n[0] * (q.0 - o[0]) + n[1] * (q.1 - o[1])) / n[2] } else { o[2] };
                (z, z + rise)
            }
            Solid::Prism { bottom, top, .. } => (bottom.at(q), top.at(q)),
            // Its triangles' heights over `q`, lowest and highest; off it, all of it.
            Solid::Mesh { tris, .. } => {
                let zs = tris.iter().filter_map(|[a, b, c]| {
                    let (pa, pb, pc) = (P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1]));
                    let det = (pb - pa).cross(pc - pa);
                    if det.abs() < 1e-9 {
                        return None;
                    }
                    let (u, v) = ((q - pa).cross(pc - pa) / det, (pb - pa).cross(q - pa) / det);
                    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
                });
                let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
                if lo <= hi { (lo, hi) } else { self.heights() }
            }
        }
    }

    /// Whether plan point `q` lies within its footprint.
    pub(crate) fn over(&self, q: P2) -> bool {
        match self {
            Solid::Box { frame, a, c, .. } => {
                let (s, t) = ((q - frame.o).dot(frame.u), (q - frame.o).dot(frame.n));
                (a.0..=a.1).contains(&s) && (c.0..=c.1).contains(&t)
            }
            Solid::Slab { face, .. } => geom::contains(&plan(face), q),
            Solid::Prism { ring, .. } => geom::contains(ring, q),
            Solid::Mesh { tris, .. } => tris.iter().any(|[a, b, c]| geom::contains(&[P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1])], q)),
        }
    }

    /// Its corners.
    pub(crate) fn corners(&self) -> Vec<P3> {
        let mut out = Vec::new();
        match self {
            Solid::Box { frame, a, c, z, .. } => {
                for (s, t, h) in [a.0, a.1].into_iter().flat_map(|s| [(s, c.0), (s, c.1)]).flat_map(|(s, t)| [(s, t, z.0), (s, t, z.1)]) {
                    let p = frame.o + frame.u * s + frame.n * t;
                    out.push([p.0, p.1, h]);
                }
            }
            Solid::Slab { face, rise, .. } => out.extend(face.iter().flat_map(|p| [*p, [p[0], p[1], p[2] + rise]])),
            Solid::Prism { ring, bottom, top, .. } => {
                out.extend(ring.iter().flat_map(|q| [[q.0, q.1, bottom.at(*q)], [q.0, q.1, top.at(*q)]]))
            }
            Solid::Mesh { tris, .. } => out.extend(tris.iter().flatten()),
        }
        out
    }

    /// The stretches of the line from `p` along unit `u` that lie over it, as distances from `p`.
    pub(crate) fn across(&self, p: P2, u: P2) -> Vec<(f64, f64)> {
        match self {
            Solid::Box { frame, a, c, .. } => {
                // Where the line lies between the box's sides, a pair of them at a time.
                let span = |axis: P2, (lo, hi): (f64, f64)| {
                    let (k0, k1) = ((p - frame.o).dot(axis), u.dot(axis));
                    if k1.abs() < 1e-12 {
                        return (lo..=hi).contains(&k0).then_some((f64::NEG_INFINITY, f64::INFINITY));
                    }
                    let (s0, s1) = ((lo - k0) / k1, (hi - k0) / k1);
                    Some((s0.min(s1), s0.max(s1)))
                };
                match (span(frame.u, *a), span(frame.n, *c)) {
                    (Some(x), Some(y)) if x.0.max(y.0) < x.1.min(y.1) => vec![(x.0.max(y.0), x.1.min(y.1))],
                    _ => Vec::new(),
                }
            }
            Solid::Slab { face, .. } => crossings(&plan(face), p, u),
            Solid::Prism { ring, .. } => crossings(ring, p, u),
            // Shown beyond a section's line, as Revit shows furniture, and never cut.
            Solid::Mesh { .. } => Vec::new(),
        }
    }

    /// The rings a section along unit `u` from `p`, `len` long, cuts it into, as (along, up) in mm: where it lies over the
    /// line, level or along a sloped face; a convex mesh that `cut` allows is cut by the plane through the line, where its
    /// edges cross it (a tube across is its ring, an ellipse if it is oblique).
    pub(crate) fn cuts(&self, p: P2, u: P2, len: f64) -> Vec<Vec<P2>> {
        if let Solid::Mesh { tris, cut: true, .. } = self {
            let side = |q: P3| (P2(q[0], q[1]) - p).dot(u.perp());
            let at = |q: P3| P2((P2(q[0], q[1]) - p).dot(u), q[2]);
            let mut pts = Vec::new();
            for [a, b, c] in tris {
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    let (dx, dy) = (side(*x), side(*y));
                    if dx.abs() < 1e-9 {
                        pts.push(at(*x));
                    } else if dx * dy < 0.0 {
                        pts.push(at(*x) + (at(*y) - at(*x)) * (dx / (dx - dy)));
                    }
                }
            }
            let ring = geom::clip(&geom::clip(&geom::hull(pts), |q| q.0), |q| len - q.0);
            return if ring.len() >= 3 && geom::area(&ring) > 1e-6 { vec![ring] } else { Vec::new() };
        }
        self.across(p, u)
            .into_iter()
            .filter_map(|(a, b)| {
                let (a, b) = (a.max(0.0), b.min(len));
                // Level, or along a sloped face.
                let ((a0, a1), (b0, b1)) = (self.z_at(p + u * a), self.z_at(p + u * b));
                (b - a > 1e-6).then(|| vec![P2(a, a0), P2(b, b0), P2(b, b1), P2(a, a1)])
            })
            .collect()
    }

    fn tris(&self, out: &mut Vec<Tri>) {
        match self {
            Solid::Box { owner, color, tex, frame, a, c, z } => frame.cuboid(out, *owner, (*color, *tex), *a, *c, *z),
            Solid::Slab { owner, color, tex, face, rise } => prism(out, *owner, (*color, *tex), face, *rise),
            Solid::Prism { owner, color, tex, ring, bottom, top, curve } => {
                let (owner, color, tex) = (*owner, *color, *tex);
                // Counter-clockwise, so every face is turned out: a wall's ring runs the other way round.
                let turned: Vec<P2>;
                let ring = if geom::area(ring) < 0.0 {
                    turned = ring.iter().rev().copied().collect();
                    &turned
                } else {
                    ring
                };
                let at = |q: P2, p: &Plane| [q.0, q.1, p.at(q)];
                for [a, b, c] in geom::triangulate(ring) {
                    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
                    out.push(Tri { p: [at(pt(a), top), at(pt(b), top), at(pt(c), top)], owner, color, tex, curve: None });
                    out.push(Tri { p: [at(pt(a), bottom), at(pt(c), bottom), at(pt(b), bottom)], owner, color, tex, curve: None });
                }
                for (a, b) in geom::ring_edges(ring) {
                    let (a0, b0, a1, b1) = (at(a, bottom), at(b, bottom), at(a, top), at(b, top));
                    // Along a curved wall's face (not across it, where it's cut), which face.
                    let curve = curve
                        .filter(|(c, _)| (b - a).unit().cross((a - *c).unit()).abs() > 0.5)
                        .map(|(c, r)| (owner, usize::from(((a + b) * 0.5).dist(c) > r)));
                    out.push(Tri { p: [a0, b0, b1], owner, color, tex, curve });
                    out.push(Tri { p: [a0, b1, a1], owner, color, tex, curve });
                }
            }
            // Each surface keyed by its first triangle's place in `out`, which no other form's shares.
            // ponytail: worked out again for each component of a family; keep it per form if many big meshes slow a scene.
            Solid::Mesh { owner, color, tex, tris, caps, round, .. } => {
                let first = out.len();
                let surfaces = if *round { (0..tris.len()).map(|i| (i >= *caps).then_some(*caps)).collect() } else { facets(tris) };
                for (p, s) in tris.iter().zip(surfaces) {
                    out.push(Tri { p: *p, owner: *owner, color: *color, tex: *tex, curve: s.map(|s| (*owner, first + s)) });
                }
            }
        }
    }
}

/// How far, in degrees, two triangles may turn at the edge they share and still be one surface:
/// the facets a curve is cut into turn less, a crease like a seat's rim more.
const SMOOTH: f64 = 20.0;

/// The curved surfaces a mesh's triangles make: those that share an edge and turn less than
/// [`SMOOTH`] degrees there are one. Each triangle's surface is its first triangle's index; none
/// for a flat face (`planes` finds those by their plane) or a triangle alone.
fn facets(tris: &[[P3; 3]]) -> Vec<Option<usize>> {
    /// The first triangle of `i`'s surface, as `up` links them, shortening the way as it goes.
    fn first(up: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = up.get(i)
            && p != i
        {
            let grand = up.get(p).copied().unwrap_or(p);
            if let Some(slot) = up.get_mut(i) {
                *slot = grand;
            }
            i = grand;
        }
        i
    }
    let limit = SMOOTH.to_radians().cos();
    let normals: Vec<P3> = tris.iter().map(|[a, b, c]| unit(cross(sub(*b, *a), sub(*c, *a)))).collect();
    let normal = |i: usize| normals.get(i).copied().unwrap_or_default();
    let mut up: Vec<usize> = (0..tris.len()).collect();
    // The first triangle on each edge, which its corners name to a micron (as a file rounds them).
    // ponytail: a corner on a rounding boundary may miss its twin, and a third triangle on an edge meets
    // only the first; probe the neighbouring cells and pair them all if facet lines show.
    let mut edges: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
    let mut bent = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        let corner = |k: usize| t.get(k % 3).map_or([0; 3], |p| p.map(|x| (x * 1e3).round() as i64));
        for k in 0..3 {
            let (a, b) = (corner(k), corner(k + 1));
            let j = *edges.entry((a.min(b), a.max(b))).or_insert(i);
            // A triangle with no area has no normal, so it turns 90° from every other.
            let turn = dot(normal(i), normal(j));
            if j != i && a != b && turn > limit {
                let (ri, rj) = (first(&mut up, i), first(&mut up, j));
                if let Some(slot) = up.get_mut(ri.max(rj)) {
                    *slot = ri.min(rj);
                }
                // Triangles in one plane (as `planes` takes it) are a flat face, not a curve.
                if turn < 1.0 - 1e-9 {
                    bent.push(i);
                }
            }
        }
    }
    let curved: HashSet<usize> = bent.into_iter().map(|i| first(&mut up, i)).collect();
    (0..tris.len()).map(|i| Some(first(&mut up, i)).filter(|s| curved.contains(s))).collect()
}

/// The corner normals of each triangle of a curved surface, for it to shade smoothly across its
/// facets: the unit normals of that surface's triangles that meet at a corner, averaged by the angle
/// each makes there (so a quad cut into two triangles weighs the same at every corner). A crease keeps
/// its step: only a surface's own triangles are averaged. None for the rest, flat faces among them.
fn corner_normals(tris: &[Tri]) -> Vec<Option<Box<[P3; 3]>>> {
    let face = |t: &Tri| unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    // A corner of a surface, to a micron: its triangles' own corners are a file's roundings of it.
    let at = |key: (Id, usize), p: P3| (key, p.map(|x| (x * 1e3).round() as i64));
    let mut sums: HashMap<((Id, usize), [i64; 3]), P3> = HashMap::new();
    for t in tris {
        let Some(key) = t.curve else { continue };
        let n = face(t);
        if dot(n, n) > 0.5 {
            for k in 0..3 {
                let (p, q, r) = (t.p[k], t.p[(k + 1) % 3], t.p[(k + 2) % 3]);
                let angle = dot(unit(sub(q, p)), unit(sub(r, p))).clamp(-1.0, 1.0).acos();
                let sum = sums.entry(at(key, p)).or_default();
                *sum = add(*sum, mul(n, angle));
            }
        }
    }
    tris.iter()
        .map(|t| {
            let (key, n) = (t.curve?, face(t));
            // Never turned from its own face; where nothing averages, its face's.
            let corner = |p: P3| {
                let m = sums.get(&at(key, p)).map_or([0.0; 3], |s| unit(*s));
                if dot(m, n) > 0.0 { m } else { n }
            };
            (dot(n, n) > 0.5).then(|| Box::new(t.p.map(corner)))
        })
        .collect()
}

/// The stretches of the line from `p` along unit `u` that lie over `ring`, as distances from `p`:
/// where it crosses the ring's edges, paired off, in and out.
pub(crate) fn crossings(ring: &[P2], p: P2, u: P2) -> Vec<(f64, f64)> {
    // ponytail: a line through a corner it only touches unpairs the rest; nudge it if slabs show gaps.
    let mut cross: Vec<f64> = geom::ring_edges(ring)
        .filter_map(|(a, b)| {
            let (e, den) = (b - a, u.cross(b - a));
            (den.abs() > 1e-12 && (0.0..1.0).contains(&((a - p).cross(u) / den))).then(|| (a - p).cross(e) / den)
        })
        .collect();
    cross.sort_by(f64::total_cmp);
    cross.as_chunks::<2>().0.iter().map(|[s0, s1]| (*s0, *s1)).collect()
}

/// The slabs `face`, and what lies `rise` over it, make: less the shafts through it, each piece of
/// it on the face's plane.
fn slabs(out: &mut Vec<Solid>, m: &Model, owner: Id, color: Rgb, face: Vec<P3>, rise: f64) {
    let (pieces, shafts) = m.less_shafts(&face, rise);
    match (shafts.is_empty(), Plane::of(&face)) {
        (false, Some(plane)) => out.extend(pieces.into_iter().map(|ring| Solid::Slab {
            owner,
            color,
            tex: 0,
            face: ring.iter().map(|q| [q.0, q.1, plane.at(*q)]).collect(),
            rise,
        })),
        _ => out.push(Solid::Slab { owner, color, tex: 0, face, rise }),
    }
}

/// The model's solids: walls cut around their openings, door leaves and glass, floor slabs and
/// ceilings, columns, stairs' steps and roofs' slopes.
pub(crate) fn solids(m: &Model) -> Vec<Solid> {
    let mut out = Vec::new();
    for (&id, e) in &m.elements {
        match e {
            Element::Wall(_) => out.extend(walls::prisms(m, id, true).into_iter().map(|p| Solid::Prism {
                owner: id,
                color: WALL,
                tex: 0,
                ring: p.ring,
                bottom: p.bottom,
                top: p.top,
                curve: p.curve,
            })),
            Element::Floor(f) => {
                for (face, rise) in m.floor_slabs(f) {
                    slabs(&mut out, m, id, SLAB, face, rise);
                }
            }
            Element::Ceiling(c) => {
                for (face, rise) in m.ceiling_slabs(c) {
                    slabs(&mut out, m, id, CEILING, face, rise);
                }
            }
            Element::Column(col) => {
                out.push(slab(id, WALL, &m.column_outline(e), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            Element::StructuralColumn(col) | Element::Footing(col) => {
                out.push(slab(id, STRUCTURE, &m.column_outline(e), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            // A rectangular run is a sloped prism, which a section cuts; a round one a tube, shown beyond its line.
            crate::model::run_arm!(_) => {
                if let (Some(t), Some(g)) = (m.run_type(e), m.run_geom(e)) {
                    let color = m.materials.get(&t.material).map_or(STRUCTURE, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                    let tex = skin(m, &t.material);
                    if g.round {
                        let (tris, caps) = g.tube();
                        out.push(Solid::Mesh { owner: id, color, tex, tris, caps, round: true, cut: true });
                    } else {
                        let (ring, bottom, top) = g.prism();
                        out.push(Solid::Prism { owner: id, color, tex, ring, bottom, top, curve: None });
                    }
                }
            }
            // Lofted, a skin of triangles from the footprint to the top outline.
            Element::Mass(ms) if ms.lofted() => {
                let tris = crate::massing::tris(&ms.footprint(), &ms.top_ring(), m.span(ms.level, ms.base_offset, ms.top_level, ms.height));
                out.push(Solid::Mesh { owner: id, color: MASS, tex: 0, tris, caps: 0, round: false, cut: false });
            }
            Element::Mass(ms) => out.push(slab(id, MASS, &ms.footprint(), m.span(ms.level, ms.base_offset, ms.top_level, ms.height))),
            // Each form in its material's colour; a curved one's facets one surface, as a curved wall's are.
            Element::Component(c) => {
                for piece in m.component_pieces(c) {
                    let color = m.materials.get(&piece.material).map_or(COMPONENT, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                    out.push(Solid::Mesh {
                        owner: id,
                        color,
                        tex: skin(m, &piece.material),
                        tris: piece.tris,
                        caps: piece.caps,
                        round: piece.round,
                        cut: false,
                    });
                }
            }
            // Each tread a block from the floor up to it, each landing a slab up to its top; the last
            // riser of a run climbs onto a landing, or the floor above.
            Element::Stair(st) => {
                let (_, each, _) = m.flight(st);
                let (base, l) = (m.elevation(st.level), stairs::stair(m, st));
                for run in &l.runs {
                    let frame = Frame { o: run.start, u: run.u, n: run.u.perp() };
                    for k in 1..run.risers {
                        let k = f64::from(k);
                        let (a, c) = (((k - 1.0) * st.tread, k * st.tread), (-st.width / 2.0, st.width / 2.0));
                        out.push(Solid::Box { owner: id, color: SLAB, tex: 0, frame, a, c, z: (base, run.z.0 + k * each) });
                    }
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            // Each run solid from its level up to its slope, each landing up to its top.
            Element::Ramp(r) => {
                let (base, l) = (m.elevation(r.level), stairs::ramp(m, r));
                for run in &l.runs {
                    let g = run.u * ((run.z.1 - run.z.0) / run.length.max(1e-9));
                    let top = Plane { z: run.z.0 - g.dot(run.start), g };
                    out.push(Solid::Prism {
                        owner: id,
                        color: SLAB,
                        tex: 0,
                        ring: run.ring(r.width),
                        bottom: Plane::flat(base),
                        top,
                        curve: None,
                    });
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            Element::Railing(rl) => out.extend(stairs::railing_parts(m, rl).into_iter().map(|(ring, bottom, top)| Solid::Prism {
                owner: id,
                color: RAIL,
                tex: 0,
                ring,
                bottom,
                top,
                curve: None,
            })),
            Element::Roof(r) => {
                for (face, rise) in m.roof_slabs(r) {
                    slabs(&mut out, m, id, ROOF, face, rise);
                }
            }
            // Of a family: each form in its material's colour, shown beyond a section's line and never cut, as a
            // component's are. Else a door's leaf or a window's glass: a 50 mm panel in the middle of the wall, square to it.
            Element::Door(o) | Element::Window(o) => {
                if o.family.is_some() {
                    let fallback = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    for piece in m.opening_pieces(e) {
                        let color = m.materials.get(&piece.material).map_or(fallback, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                        out.push(Solid::Mesh {
                            owner: id,
                            color,
                            tex: skin(m, &piece.material),
                            tris: piece.tris,
                            caps: piece.caps,
                            round: piece.round,
                            cut: false,
                        });
                    }
                } else if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                    let color = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    let z = m.own_base(w) + o.sill;
                    let axis = Axis::of(w);
                    let u = axis.tangent(o.offset);
                    let frame = Frame { o: axis.at(o.offset, walls::centre(m, w)), u, n: u.perp() };
                    let a = (-t.width / 2.0, t.width / 2.0);
                    out.push(Solid::Box { owner: id, color, tex: 0, frame, a, c: (-25.0, 25.0), z: (z, z + t.height) });
                }
            }
            _ => {}
        }
    }
    out
}

/// A slab: `face`, its copy `rise` above, and the sides between them.
fn prism(out: &mut Vec<Tri>, owner: Id, (color, tex): (Rgb, u32), face: &[P3], rise: f64) {
    // Counter-clockwise seen from above, so every face is turned out.
    let turned: Vec<P3>;
    let face = if geom::area(&plan(face)) < 0.0 {
        turned = face.iter().rev().copied().collect();
        &turned
    } else {
        face
    };
    let up = |p: P3| [p[0], p[1], p[2] + rise];
    let pt = |i: usize| face.get(i).copied().unwrap_or_default();
    for [a, b, c] in geom::triangulate(&plan(face)) {
        out.push(Tri { p: [up(pt(a)), up(pt(b)), up(pt(c))], owner, color, tex, curve: None });
        out.push(Tri { p: [pt(a), pt(c), pt(b)], owner, color, tex, curve: None });
    }
    for (a, b) in face.iter().zip(face.iter().cycle().skip(1)) {
        out.push(Tri { p: [*a, *b, up(*b)], owner, color, tex, curve: None });
        out.push(Tri { p: [*a, up(*b), up(*a)], owner, color, tex, curve: None });
    }
}

/// Each triangle's plane, as an index the triangles lying in it (within half a millimetre) share;
/// the facets of one curved surface (a curved wall's face, a mesh's curve) share one too.
fn planes(tris: &[Tri]) -> Vec<usize> {
    // Planes by their normal, in cells wider than the tolerance, and a millimetre's slab of distance;
    // a match is looked for in the nearer neighbouring cell on each axis and the slabs either side,
    // so the tolerance holds across a cell's edge.
    let mut found: HashMap<[i64; 4], Vec<(P3, f64, usize)>> = HashMap::new();
    let mut count = 0;
    let mut curves: BTreeMap<(Id, usize), usize> = BTreeMap::new();
    tris.iter()
        .map(|t| {
            if let Some(c) = t.curve {
                let next = tris.len() + curves.len();
                return *curves.entry(c).or_insert(next);
            }
            let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            count += 1;
            // No area, no plane to share: its own.
            if dot(n, n) < 0.5 {
                return count - 1;
            }
            let d = dot(n, t.p[0]);
            // A plane and its flip are one: each faces where its first ratio that isn't zero is positive.
            let (n, d) = if n.iter().find(|x| x.abs() > 1e-9).is_some_and(|x| *x < 0.0) { (mul(n, -1.0), -d) } else { (n, d) };
            let cell = n.map(|x| x * 1e4);
            let q = cell.map(|x| x.round() as i64);
            let near = |k: usize| [q[k], q[k] + if cell[k] < q[k] as f64 { -1 } else { 1 }];
            let slab = d.floor() as i64;
            for a in near(0) {
                for b in near(1) {
                    for c in near(2) {
                        for s in slab - 1..=slab + 1 {
                            let same = found
                                .get(&[a, b, c, s])
                                .and_then(|v| v.iter().find(|(m, e, _)| dot(*m, n) > 1.0 - 1e-9 && (e - d).abs() < 0.5));
                            if let Some(&(_, _, id)) = same {
                                count -= 1;
                                return id;
                            }
                        }
                    }
                }
            }
            found.entry([q[0], q[1], q[2], slab]).or_default().push((n, d, count - 1));
            count - 1
        })
        .collect()
}

/// The texture id a library material gives what is made of it: one more than where it is in the library's order, 0
/// for one with no texture, or none in the library.
fn skin(m: &Model, name: &str) -> u32 {
    m.materials.iter().position(|(k, x)| k == name && x.texture.is_some()).map_or(0, |i| i as u32 + 1)
}

/// The model's solids as its pictures show them: an element of a compound type split into its
/// layers, each in its material's colour; a slab's from the top down, a wall's from its exterior in.
pub(crate) fn shaded(m: &Model) -> Vec<Solid> {
    solids(m).into_iter().flat_map(|s| layered(m, s)).collect()
}

fn layered(m: &Model, s: Solid) -> Vec<Solid> {
    let layers = match m.elements.get(s.owner()) {
        Some(Element::Wall(w)) => m.type_layers("wall", &w.type_name),
        Some(Element::Floor(f)) => m.type_layers("floor", &f.type_name),
        Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
        Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
        _ => &[],
    };
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    if total <= 0.0 {
        return vec![s];
    }
    let colour = |l: &Layer, or: Rgb| m.materials.get(&l.material).map_or(or, |x| Rgb(x.color[0], x.color[1], x.color[2]));
    match s {
        Solid::Slab { owner, color, tex, face, rise } => {
            let mut out = Vec::new();
            let mut above = 0.0;
            for l in layers {
                let r = rise * l.thickness / total;
                if r > 1e-9 {
                    let lift = rise - above - r;
                    let face = face.iter().map(|p| [p[0], p[1], p[2] + lift]).collect();
                    out.push(Solid::Slab { owner, color: colour(l, color), tex: skin(m, &l.material).max(tex), face, rise: r });
                }
                above += r;
            }
            out
        }
        // Across the wall from its exterior face in, each layer between two of its boundaries; the
        // outermost and innermost reach whatever lies past them.
        Solid::Prism { owner, color, tex, ring, bottom, top, curve } => {
            let Some(w) = m.wall(owner) else { return vec![Solid::Prism { owner, color, tex, ring, bottom, top, curve }] };
            let (axis, b) = (Axis::of(w), walls::boundaries(m, w));
            let off = |q: P2| axis.project(q).1;
            let n = layers.len();
            layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.thickness > 1e-9)
                .filter_map(|(k, l)| {
                    let mut part = ring.clone();
                    if let Some(hi) = b.get(k).filter(|_| k > 0) {
                        part = geom::clip(&part, |q| hi - off(q));
                    }
                    if let Some(lo) = b.get(k + 1).filter(|_| k + 1 < n) {
                        part = geom::clip(&part, |q| off(q) - lo);
                    }
                    (part.len() >= 3 && geom::area(&part).abs() > 1e-6).then(|| Solid::Prism {
                        owner,
                        color: colour(l, color),
                        tex: skin(m, &l.material).max(tex),
                        ring: part,
                        bottom,
                        top,
                        curve,
                    })
                })
                .collect()
        }
        other => vec![other],
    }
}

/// Paint laid on the faces of elements: each triangle on a painted face takes the material it's painted, its colour and its
/// texture, in place of what its type is made of.
fn painted(m: &Model, tris: &mut [Tri], faces: &[&'static str]) {
    let paints: HashMap<Id, &serde_json::Map<String, serde_json::Value>> =
        m.elements.iter().filter_map(|(id, e)| Some((*id, e.params()?.get(paint::KEY)?.as_object()?))).collect();
    if paints.is_empty() {
        return;
    }
    for (t, face) in tris.iter_mut().zip(faces) {
        let Some(name) = paints.get(&t.owner).and_then(|p| p.get(*face)).and_then(|v| v.as_str()) else { continue };
        if let Some(x) = m.materials.get(name) {
            (t.color, t.tex) = (Rgb(x.color[0], x.color[1], x.color[2]), skin(m, name));
        }
    }
}

/// The model as triangles, each with its plane, and its corner normals where it's a curve's: what
/// every picture of it is drawn from.
pub struct Scene {
    tris: Vec<Tri>,
    planes: Vec<usize>,
    smooth: Vec<Option<Box<[P3; 3]>>>,
    /// Each triangle's face, as paint names them: `exterior`, `interior`, `top`, `bottom`, `all`, or none.
    faces: Arc<Vec<&'static str>>,
    /// Each texture the scene uses, by id; and each textured triangle's way across and up its plane.
    skins: Vec<Option<Sampler>>,
    bases: Vec<Option<[P3; 2]>>,
    /// The edges a wireframe draws, found when one is first asked for.
    edges: OnceLock<Vec<(usize, [P3; 2])>>,
    /// The masses, which a section's picture leaves out: it outlines where its line cuts them.
    masses: BTreeSet<Id>,
    /// Each level's plane: its name, and the corners of its extent at its elevation.
    levels: Vec<(Id, String, [P3; 4])>,
}

/// How an element's triangles name their faces.
enum Rule {
    /// A wall's: its exterior is to the left of its start to its end.
    Wall(Axis),
    /// A floor's, a roof's or a ceiling's: facing up or down.
    Slab,
    /// Any other: all of it is one face.
    Whole,
}

impl Rule {
    /// The face a triangle is on, or none where it's on no face that can be painted (a wall's top or end).
    fn face(&self, p: &[P3; 3]) -> &'static str {
        let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        match self {
            Rule::Wall(axis) => {
                let c = P2((p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0);
                let left = axis.tangent(axis.project(c).0).perp();
                match n[0] * left.0 + n[1] * left.1 {
                    d if d > 0.5 => "exterior",
                    d if d < -0.5 => "interior",
                    _ => "",
                }
            }
            Rule::Slab if n[2] > 0.5 => "top",
            Rule::Slab if n[2] < -0.5 => "bottom",
            Rule::Slab => "",
            Rule::Whole => "all",
        }
    }
}

/// Two unit vectors across a triangle's plane, the way a texture's across and up lie on it: across is level (to the
/// right of someone looking at the face from outside, east where it lies flat), up goes up it (north where it lies flat).
fn basis(t: &Tri) -> [P3; 2] {
    let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    let level = cross([0.0, 0.0, 1.0], n);
    let across = if dot(level, level) < 1e-4 { [1.0, 0.0, 0.0] } else { unit(level) };
    [across, unit(cross(n, across))]
}

/// Where a level's name goes in a 3D view: a vertical plane, unseen, past the right end of its
/// plane's far edges and in line with the one that ends there, lettered with the name. It is where an
/// elevation marker (an annotation family, later) will stand.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelHead {
    pub level: Id,
    pub name: String,
    /// Along its foot, then up its far side: on the level's elevation, and its lettering's height above.
    pub corners: [P3; 4],
}

/// How tall a level's name is lettered in 3D, px; and the room round it.
const HEAD_TEXT: f32 = 11.0;
const HEAD_PAD: f32 = 3.0;

impl Scene {
    pub fn new(m: &Model) -> Scene {
        let mut tris = Vec::new();
        for solid in shaded(m) {
            solid.tris(&mut tris);
        }
        // Each paintable element's rule for naming the faces of its triangles; paint gives those it's laid on their material.
        let rules: HashMap<Id, Rule> = m
            .elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Wall(w) => Some((*id, Rule::Wall(Axis::of(w)))),
                e if matches!(e.category(), "floor" | "roof" | "ceiling") => Some((*id, Rule::Slab)),
                e if paint::faces(e.category()).contains(&"all") => Some((*id, Rule::Whole)),
                _ => None,
            })
            .collect();
        let faces: Vec<&'static str> = tris.iter().map(|t| rules.get(&t.owner).map_or("", |r| r.face(&t.p))).collect();
        painted(m, &mut tris, &faces);
        // The textures the scene uses, ready to look up, and what each textured triangle lies across.
        let used: BTreeSet<u32> = tris.iter().map(|t| t.tex).filter(|t| *t > 0).collect();
        let mut skins: Vec<Option<Sampler>> = vec![None; m.materials.len() + 1];
        for id in used {
            let found = m.materials.values().nth(id as usize - 1);
            if let (Some(x), Some(slot)) = (found, skins.get_mut(id as usize)) {
                *slot = x.texture.as_ref().and_then(|t| Sampler::new(t, x.color, &m.images));
            }
        }
        let bases = tris.iter().map(|t| skins.get(t.tex as usize).is_some_and(Option::is_some).then(|| basis(t))).collect();
        let (planes, smooth) = (planes(&tris), corner_normals(&tris));
        let masses = m.elements.iter().filter(|(_, e)| matches!(e, Element::Mass(_))).map(|(id, _)| *id).collect();
        let default = std::cell::OnceCell::new();
        let levels = m
            .levels()
            .into_iter()
            .map(|(id, l)| {
                let [lo, hi] = l.extent.unwrap_or_else(|| *default.get_or_init(|| m.model_extent()));
                let z = l.elevation;
                (id, l.name.clone(), [[lo.0, lo.1, z], [hi.0, lo.1, z], [hi.0, hi.1, z], [lo.0, hi.1, z]])
            })
            .collect();
        Scene { tris, planes, smooth, faces: Arc::new(faces), skins, bases, masses, levels, edges: OnceLock::new() }
    }

    /// A view of the whole model from `azimuth` and `elevation` (degrees), fitted into the picture.
    pub fn fit(&self, azimuth: f64, elevation: f64, width: u32, height: u32) -> Camera {
        self.fit_in(azimuth, elevation, width, height, &Look::default())
    }

    /// A view of what `look` shows of the model, fitted into the picture.
    pub fn fit_in(&self, azimuth: f64, elevation: f64, width: u32, height: u32, look: &Look) -> Camera {
        let mut cam = Camera { azimuth, elevation: elevation.clamp(-89.0, 89.0), scale: 1.0, target: [0.0; 3], width, height };
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
        let solids = self.tris.iter().filter(|t| look.shows(t.owner)).flat_map(|t| t.p);
        let levels = self.levels.iter().filter(|(id, ..)| look.shows(*id)).flat_map(|(.., c)| *c);
        let mut pts: Vec<P3> = solids.chain(levels).collect();
        let Some((target, w, h)) = extent(&cam, &pts) else {
            cam.scale = room(width).min(room(height)) / 10_000.0;
            return cam;
        };
        cam.target = target;
        cam.scale = (room(width) / w.max(1000.0)).min(room(height) / h.max(1000.0)).clamp(1e-6, 50.0);
        // The levels' names take room at the scale that gives: fitted again with their heads.
        pts.extend(self.level_heads(&cam).into_iter().filter(|h| look.shows(h.level)).flat_map(|h| h.corners));
        if let Some((target, w, h)) = extent(&cam, &pts) {
            cam.target = target;
            cam.scale = (room(width) / w.max(1000.0)).min(room(height) / h.max(1000.0)).clamp(1e-6, 50.0);
        }
        cam
    }

    /// Each level's head as `cam` sees it: see [`LevelHead`].
    pub fn level_heads(&self, cam: &Camera) -> Vec<LevelHead> {
        let px = 1.0 / cam.scale.max(1e-9);
        self.levels
            .iter()
            .map(|(id, name, c)| {
                let near = nearest(cam, c);
                let (far, sides) = (c[(near + 2) % 4], [c[(near + 1) % 4], c[(near + 3) % 4]]);
                // The far edges' outer ends: the one further right.
                let end = if cam.project(sides[0]).0 >= cam.project(sides[1]).0 { sides[0] } else { sides[1] };
                let d = unit(sub(end, far));
                let (w, h) =
                    (f64::from(crate::font::width(name, HEAD_TEXT) + 2.0 * HEAD_PAD) * px, f64::from(HEAD_TEXT + 2.0 * HEAD_PAD) * px);
                let foot = add(end, mul(d, 2.0 * f64::from(HEAD_PAD) * px));
                let up = [0.0, 0.0, h];
                let corners = [foot, add(foot, mul(d, w)), add(add(foot, mul(d, w)), up), add(foot, up)];
                LevelHead { level: *id, name: name.clone(), corners }
            })
            .collect()
    }

    /// The picture `cam` takes. Elements in `tints` show halfway to their tint, and their edges in it.
    pub fn render(&self, cam: &Camera, tints: &[(Id, Rgb)]) -> Image {
        self.render_in(cam, tints, &Look::default())
    }

    /// The picture `cam` takes of what `look` shows, in the colours it gives.
    pub fn render_in(&self, cam: &Camera, tints: &[(Id, Rgb)], look: &Look) -> Image {
        self.picture(cam, None, tints, look)
    }

    /// A section's or an elevation's picture: what `clip` keeps, in hidden line (faces the paper's
    /// colour, edges dark), for the cut to be drawn over.
    pub fn cut(&self, cam: &Camera, clip: &Clip, tints: &[(Id, Rgb)]) -> Image {
        self.cut_in(cam, clip, tints, &Look::default())
    }

    /// A section's or an elevation's picture of what `look` shows.
    pub fn cut_in(&self, cam: &Camera, clip: &Clip, tints: &[(Id, Rgb)], look: &Look) -> Image {
        self.picture(cam, Some(clip), tints, look)
    }

    /// The triangles `keep` takes, drawn into `into`'s depths and hits: the nearest at each pixel of a
    /// `size` picture through `cam`; with `front`, only where they are nearer than its depths. Where
    /// `lights` says how lit a triangle's corners are (a curve's), its pixels take the blend of the three.
    fn raster(
        &self,
        (cam, clip): (&Camera, Option<&Clip>),
        (w, h): (usize, usize),
        keep: impl Fn(&Tri) -> bool,
        front: Option<&[f32]>,
        lights: &dyn Fn(usize) -> Option<[f64; 3]>,
        (depth, hit, lit): (&mut [f32], &mut [u32], &mut [f32]),
    ) {
        for (i, t) in self.tris.iter().enumerate() {
            if !keep(t) {
                continue;
            }
            let [a, b, c] = t.p.map(|p| cam.project(p));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            // Edge-on (or not a number): nothing to fill.
            if area.is_nan() || area.abs() <= 1e-12 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
            let (y0, y1) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
            if !(x1 >= 0.0 && x0 < w as f64 && y1 >= 0.0 && y0 < h as f64) {
                continue;
            }
            let corners = lights(i);
            for py in y0.max(0.0) as usize..=y1.min(h as f64 - 1.0) as usize {
                for px in x0.max(0.0) as usize..=x1.min(w as f64 - 1.0) as usize {
                    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                    // Barycentric weights; a hair's tolerance so neighbours leave no gap between them.
                    let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / area;
                    let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / area;
                    let wc = 1.0 - wa - wb;
                    if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                        continue;
                    }
                    let z = wa * a.2 + wb * b.2 + wc * c.2;
                    if clip.is_some_and(|k| {
                        z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1 || y < k.y.0 || y > k.y.1
                    }) {
                        continue;
                    }
                    let z = z as f32;
                    let k = py * w + px;
                    if front.is_some_and(|f| f.get(k).is_some_and(|d| z <= *d)) {
                        continue;
                    }
                    if let (Some(d), Some(s)) = (depth.get_mut(k), hit.get_mut(k))
                        && z > *d
                    {
                        (*d, *s) = (z, i as u32);
                        if let (Some([ka, kb, kc]), Some(slot)) = (corners, lit.get_mut(k)) {
                            *slot = (wa * ka + wb * kb + wc * kc) as f32;
                        }
                    }
                }
            }
        }
    }

    /// The edges of the model's surfaces: where two meet at an angle (not in one plane, nor one curve) and where one ends,
    /// each as the triangle it is an edge of and its ends, in the order of their triangles.
    fn edges(&self) -> &[(usize, [P3; 2])] {
        self.edges.get_or_init(|| {
            // Each edge by its corners to a micron, as `facets` names them: the triangles on it, and where on each.
            let corner = |t: &Tri, k: usize| t.p.get(k % 3).copied().unwrap_or_default();
            let q = |p: P3| p.map(|x| (x * 1e3).round() as i64);
            type Corner = [i64; 3];
            let mut on: HashMap<(Corner, Corner), Vec<(usize, usize)>> = HashMap::new();
            for (i, t) in self.tris.iter().enumerate() {
                for k in 0..3 {
                    let (qa, qb) = (q(corner(t, k)), q(corner(t, k + 1)));
                    if qa != qb {
                        on.entry((qa.min(qb), qa.max(qb))).or_default().push((i, k));
                    }
                }
            }
            let plane = |i: usize| self.planes.get(i).copied();
            let mut edges: Vec<(usize, [P3; 2])> = on
                .into_values()
                .filter(|on| !matches!(on.as_slice(), [(x, _), (y, _)] if plane(*x) == plane(*y)))
                .filter_map(|on| {
                    let &(i, k) = on.first()?;
                    let t = self.tris.get(i)?;
                    Some((i, [corner(t, k), corner(t, k + 1)]))
                })
                .collect();
            edges.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal)));
            edges
        })
    }

    /// The picture `cam` takes in wireframe: paper, and every edge of what `look` shows, in the colour it gives them (a
    /// tinted element's in its tint, over the rest). A pixel within two of a line picks the element the line is of.
    fn wireframe(&self, cam: &Camera, tints: &[(Id, Rgb)], look: &Look) -> Image {
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        let mut rgba = vec![255u8; w * h * 4];
        let mut hit = vec![u32::MAX; w * h];
        for tinted in [false, true] {
            for &(i, [a, b]) in self.edges() {
                let Some(owner) = self.tris.get(i).map(|t| t.owner).filter(|o| look.shows(*o) && tints.contains_key(o) == tinted) else {
                    continue;
                };
                let Rgb(r, g, bl) = tints.get(&owner).copied().unwrap_or_else(|| look.edge(owner, EDGE));
                let (pa, pb) = (cam.project(a), cam.project(b));
                if ![pa.0, pa.1, pb.0, pb.1].iter().all(|x| x.is_finite()) {
                    continue;
                }
                let Some((p, q)) = clip(P2(pa.0, pa.1), P2(pb.0, pb.1), P2(0.0, 0.0), P2(w as f64, h as f64)) else { continue };
                let steps = (p.dist(q).ceil() as usize).max(1);
                for k in 0..=steps {
                    let t = k as f64 / steps as f64;
                    let (x, y) = ((p.0 + (q.0 - p.0) * t).floor() as i64, (p.1 + (q.1 - p.1) * t).floor() as i64);
                    for (dx, dy) in (-2..=2).flat_map(|dx| (-2..=2).map(move |dy| (dx, dy))) {
                        let (px, py) = (x + dx, y + dy);
                        if (0..w as i64).contains(&px) && (0..h as i64).contains(&py) {
                            let at = py as usize * w + px as usize;
                            if let Some(slot) = hit.get_mut(at) {
                                *slot = i as u32;
                            }
                            if (dx, dy) == (0, 0)
                                && let Some(px) = rgba.get_mut(4 * at..4 * at + 3)
                            {
                                px.copy_from_slice(&[r, g, bl]);
                            }
                        }
                    }
                }
            }
        }
        Image {
            width: w as u32,
            height: h as u32,
            rgba,
            hit,
            owners: self.tris.iter().map(|t| t.owner).collect(),
            faces: Arc::clone(&self.faces),
        }
    }

    fn picture(&self, cam: &Camera, clip: Option<&Clip>, tints: &[(Id, Rgb)], look: &Look) -> Image {
        // A section's picture is hidden line, whatever the view says.
        let style = if clip.is_some() { VisualStyle::HiddenLine } else { look.style() };
        if style == VisualStyle::Wireframe {
            return self.wireframe(cam, tints, look);
        }
        let flat = style != VisualStyle::Shaded;
        let plain = look.is_plain();
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let (back, right, up) = cam.axes();
        // Lit from over the viewer's left shoulder; a face turned away from the light keeps 62%.
        let light = unit(add(add(mul(right, -0.35), mul(up, 0.55)), mul(back, 0.75)));
        let lit = |n: P3| 0.62 + 0.38 * dot(n, light).max(0.0);
        let paint = |color: Rgb, k: f64| {
            let c = |v: u8| (f64::from(v) * k).round() as u8;
            Rgb(c(color.0), c(color.1), c(color.2))
        };
        // The colour a surface is shaded in: the view's, unless it gives none.
        let colour = |t: &Tri| if plain { t.color } else { look.surface(t.owner, t.color) };
        // A curve's corners are lit by their own normals, and its pixels take the blend of the three;
        // the faces of a section are paper.
        let smooth: &[Option<Box<[P3; 3]>>] = if flat { &[] } else { &self.smooth };
        // How lit each corner of a curve is: its normals turned to the viewer as its face is.
        let corner_light = |i: usize| {
            let (t, n) = (self.tris.get(i)?, smooth.get(i)?.as_deref()?);
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            Some(n.map(|n| lit(if dot(face, back) < 0.0 { mul(n, -1.0) } else { n })))
        };
        // What the view sees through is drawn over what lies behind it, after the opaque surfaces: the
        // nearest of them at each pixel, where nothing opaque is nearer. (Not in a section's hidden line.)
        let see_through = |t: &Tri| !flat && !plain && look.transparency(t.owner) > 0;
        let shown = |t: &Tri| (plain || look.shows(t.owner)) && !(clip.is_some() && self.masses.contains(&t.owner));
        let mut depth = vec![f32::NEG_INFINITY; w * h];
        let mut hit = vec![u32::MAX; w * h];
        let mut curve_lit = vec![0.0; w * h];
        let opaque = |t: &Tri| shown(t) && !see_through(t);
        self.raster((cam, clip), (w, h), opaque, None, &corner_light, (&mut depth, &mut hit, &mut curve_lit));
        // Only a picture with something in it to see through needs the second pass, and its buffers.
        let through_any = !plain && self.tris.iter().any(|t| shown(t) && see_through(t));
        let (mut seen, mut seen_lit, mut near) = (Vec::new(), Vec::new(), Vec::new());
        if through_any {
            near = vec![f32::NEG_INFINITY; w * h];
            (seen, seen_lit) = (vec![u32::MAX; w * h], vec![0.0; w * h]);
            let clear = |t: &Tri| shown(t) && see_through(t);
            self.raster((cam, clip), (w, h), clear, Some(&depth), &corner_light, (&mut near, &mut seen, &mut seen_lit));
        }
        // How lit each triangle is, as its face turns to the light; and its flat colour, which a section's is the paper's.
        let light: Vec<f64> = self
            .tris
            .iter()
            .map(|t| {
                let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
                lit(if dot(n, back) < 0.0 { mul(n, -1.0) } else { n })
            })
            .collect();
        let shade: Vec<Rgb> = self.tris.iter().zip(&light).map(|(t, l)| if flat { PAPER } else { paint(colour(t), *l) }).collect();
        // A textured triangle's colour at pixel `k`, where its surface lies `z` toward the camera: the texture at that point of
        // its plane, for a pixel's width of it. None for any other, or where the view colours it its own way, or in a section.
        let texturing = !flat && self.skins.iter().any(Option::is_some);
        let foot = 1.0 / cam.scale;
        let textured = |t: usize, k: usize, z: f32| -> Option<Rgb> {
            if !texturing {
                return None;
            }
            let tri = self.tris.get(t)?;
            let sampler = self.skins.get(tri.tex as usize)?.as_ref()?;
            if !plain && colour(tri) != tri.color {
                return None;
            }
            let [across, up] = (*self.bases.get(t)?).as_ref()?;
            let (x, y) = ((k % w) as f64 + 0.5, (k / w) as f64 + 0.5);
            let at = add(cam.unproject((x, y)), mul(back, f64::from(z)));
            Some(sampler.at(dot(at, *across), dot(at, *up), foot))
        };
        // What triangle `t` shows at pixel `k`, whose surface is `depths` there: a curve's, the blend of its corners' lighting; a
        // flat face's, its own shade; a texture's colour lit as the face is.
        let shaded = |t: usize, k: usize, curve: &[f32], depths: &[f32]| {
            let tri = self.tris.get(t)?;
            let corners = curve.get(k).filter(|_| smooth.get(t).is_some_and(Option::is_some));
            let skin = textured(t, k, depths.get(k).copied().unwrap_or_default());
            Some(match (skin, corners) {
                (Some(c), Some(l)) => paint(c, f64::from(*l)),
                (Some(c), None) => paint(c, *light.get(t)?),
                (None, Some(l)) => paint(colour(tri), f64::from(*l)),
                (None, None) => *shade.get(t)?,
            })
        };
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        let tri = |k: usize| hit.get(k).map(|&t| t as usize).filter(|t| *t < self.tris.len());
        let through = |k: usize| seen.get(k).map(|&t| t as usize).filter(|t| *t < self.tris.len());
        let plane = |k: usize| tri(k).and_then(|t| self.planes.get(t)).copied();
        let mut rgba = Vec::with_capacity(w * h * 4);
        for py in 0..h {
            for px in 0..w {
                let k = py * w + px;
                let here = plane(k);
                let edge = (px + 1 < w && plane(k + 1) != here) || (py + 1 < h && plane(k + w) != here);
                let owner_at = |k: usize| tri(k).and_then(|t| self.tris.get(t)).map(|t| t.owner);
                let tint = owner_at(k).and_then(|o| tints.get(&o));
                // An edge is drawn in the colour the view gives the edges of what it bounds.
                let edge_of = || [k, k + 1, k + w].into_iter().find_map(owner_at).map_or(EDGE, |o| look.edge(o, EDGE));
                let c = match (edge, tri(k).and_then(|t| shaded(t, k, &curve_lit, &depth))) {
                    (true, _) => tint.copied().unwrap_or_else(edge_of),
                    (false, Some(s)) => tint.map_or(s, |c| mix(s, *c)),
                    (false, None) => PAPER,
                };
                // Over it, the nearest surface seen through, as much of it as the view lets show (and a
                // tinted one always a little, or selecting it would show nothing).
                let c = match through(k).and_then(|t| Some((shaded(t, k, &seen_lit, &near)?, self.tris.get(t)?.owner))) {
                    Some((s, owner)) => {
                        let tint = tints.get(&owner);
                        let weight = 1.0 - f64::from(look.transparency(owner)) / 100.0;
                        blend(c, tint.map_or(s, |c| mix(s, *c)), if tint.is_some() { weight.max(0.35) } else { weight })
                    }
                    None => c,
                };
                rgba.extend([c.0, c.1, c.2, 255]);
            }
        }
        // What a pixel shows, for picking: a surface seen through is the one in front.
        if through_any {
            hit = hit.iter().zip(&seen).map(|(h, s)| if *s == u32::MAX { *h } else { *s }).collect();
        }
        let mut owners: Vec<Id> = self.tris.iter().map(|t| t.owner).collect();
        // Over it, each level's plane outlined where nothing nearer hides it (not in a section's picture,
        // which draws its own), picked as the level.
        // Unselected, the two edges nearer the camera are left out, as in Revit; selected, all four.
        // Its head goes past the far edges' right end: its name lettered, all of it picking the level.
        if clip.is_none() {
            let slack = 1.5 / cam.scale.max(1e-9);
            let heads = self.level_heads(cam);
            for ((id, _, corners), head) in self.levels.iter().zip(&heads).filter(|((id, ..), _)| plain || look.shows(*id)) {
                let c = tints.get(id).copied().unwrap_or_else(|| look.edge(*id, DATUM));
                let slot = owners.len() as u32;
                owners.push(*id);
                let mut paint = |k: usize| {
                    if let (Some(px), Some(t)) = (rgba.get_mut(4 * k..4 * k + 3), hit.get_mut(k)) {
                        px.copy_from_slice(&[c.0, c.1, c.2]);
                        *t = slot;
                    }
                };
                let near = nearest(cam, corners);
                for (i, (a, b)) in corners.iter().zip(corners.iter().cycle().skip(1)).enumerate() {
                    if tints.contains_key(id) || (i != near && (i + 1) % 4 != near) {
                        outline((cam.project(*a), cam.project(*b)), (w, h), 2, slack, &depth, &mut paint);
                    }
                }
                let [foot, along, ..] = head.corners;
                let (d, px) = (unit(sub(along, foot)), 1.0 / cam.scale.max(1e-9));
                let height = f64::from(HEAD_TEXT + 2.0 * HEAD_PAD);
                let to3 = |(x, y): (f32, f32)| add(add(foot, mul(d, f64::from(x) * px)), [0.0, 0.0, (height - f64::from(y)) * px]);
                for stroke in crate::font::strokes(&head.name, HEAD_PAD, HEAD_PAD + HEAD_TEXT, HEAD_TEXT) {
                    for seg in stroke.windows(2) {
                        if let [a, b] = seg {
                            outline((cam.project(to3(*a)), cam.project(to3(*b))), (w, h), 1, slack, &depth, &mut paint);
                        }
                    }
                }
                // The rest of the head picks the level too, unpainted.
                let ring: Vec<P2> = head.corners.iter().map(|p| cam.project(*p)).map(|(x, y, _)| P2(x, y)).collect();
                let z = head.corners.iter().map(|p| cam.project(*p).2).sum::<f64>() / 4.0;
                let (lo, hi) = ring.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
                    (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
                });
                let (x0, x1) = (lo.0.max(0.0).floor() as usize, hi.0.min(w as f64 - 1.0).max(0.0) as usize);
                let (y0, y1) = (lo.1.max(0.0).floor() as usize, hi.1.min(h as f64 - 1.0).max(0.0) as usize);
                for py in y0..=y1.min(y0 + 4 * MAX_SIDE as usize) {
                    for px in x0..=x1.min(x0 + 4 * MAX_SIDE as usize) {
                        let k = py * w + px;
                        if geom::contains(&ring, P2(px as f64 + 0.5, py as f64 + 0.5))
                            && depth.get(k).is_some_and(|d| f64::from(*d) <= z + slack)
                            && let Some(t) = hit.get_mut(k)
                        {
                            *t = slot;
                        }
                    }
                }
            }
        }
        Image { width: w as u32, height: h as u32, rgba, hit, owners, faces: Arc::clone(&self.faces) }
    }
}

/// Which of a level plane's `corners` is nearest the camera: the two edges from it are its near ones.
fn nearest(cam: &Camera, corners: &[P3; 4]) -> usize {
    (0..4).max_by(|a, b| cam.project(corners[*a]).2.total_cmp(&cam.project(corners[*b]).2)).unwrap_or(0)
}

/// The pixels of a `w` x `h` picture a line from `a` to `b` (each a pixel and its nearness)
/// covers, `wide` (1 or 2) px wide, where nothing in `depth` lies more than `slack` mm nearer: each to `paint`.
fn outline(
    (a, b): ((f64, f64, f64), (f64, f64, f64)),
    (w, h): (usize, usize),
    wide: u8,
    slack: f64,
    depth: &[f32],
    mut paint: impl FnMut(usize),
) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil();
    if !steps.is_finite() || steps > 4.0 * MAX_SIDE as f64 {
        return;
    }
    let steps = steps.max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let (x, y, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        let offsets: &[(f64, f64)] = if wide > 1 { &[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] } else { &[(0.5, 0.5)] };
        for &(dx, dy) in offsets {
            let (px, py) = ((x - 0.5 + dx).floor(), (y - 0.5 + dy).floor());
            if px < 0.0 || py < 0.0 || px >= w as f64 || py >= h as f64 {
                continue;
            }
            let k = py as usize * w + px as usize;
            if depth.get(k).is_some_and(|d| f64::from(*d) <= z + slack) {
                paint(k);
            }
        }
    }
}

/// What a section's or an elevation's picture keeps: what lies beyond its plane, depths (toward
/// the camera) below `near`, and no more than `far` past it; between pixel columns `x.0` and `x.1`,
/// and rows `y.0` and `y.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub near: f64,
    pub far: Option<f64>,
    pub x: (f64, f64),
    pub y: (f64, f64),
}

/// Where `pts` lie as `cam` looks at them: the point in their middle, and how far they spread
/// across the picture and up it, mm.
fn extent(cam: &Camera, pts: &[P3]) -> Option<(P3, f64, f64)> {
    let first = *pts.first()?;
    let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
    });
    let middle = mul(add(lo, hi), 0.5);
    let (_, right, up) = cam.axes();
    let (mut x, mut y) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
    for p in pts {
        let d = sub(*p, middle);
        let (px, py) = (dot(d, right), dot(d, up));
        (x, y) = ((x.0.min(px), x.1.max(px)), (y.0.min(py), y.1.max(py)));
    }
    Some((add(middle, add(mul(right, (x.0 + x.1) / 2.0), mul(up, (y.0 + y.1) / 2.0))), x.1 - x.0, y.1 - y.0))
}

/// The whole model from `azimuth` and `elevation` at `scale` px per mm: the picture as large as
/// that makes it (up to [`MAX_SIDE`] a side), `room` px clear around it. A 3D view on a sheet.
pub fn camera(m: &Model, azimuth: f64, elevation: f64, scale: f64, room: f64) -> Camera {
    camera_in(m, azimuth, elevation, scale, room, &Look::default())
}

/// [`camera`] for what `look` shows of the model.
pub fn camera_in(m: &Model, azimuth: f64, elevation: f64, scale: f64, room: f64, look: &Look) -> Camera {
    let mut cam = Camera { azimuth, elevation: elevation.clamp(-89.0, 89.0), scale, target: [0.0; 3], width: 1, height: 1 };
    let pts: Vec<P3> = solids(m).iter().filter(|s| look.shows(s.owner())).flat_map(Solid::corners).collect();
    if let Some((target, w, h)) = extent(&cam, &pts) {
        let side = |d: f64| (d * scale + 2.0 * room).ceil().clamp(1.0, f64::from(MAX_SIDE)) as u32;
        (cam.target, cam.width, cam.height) = (target, side(w), side(h));
    }
    cam
}

/// Each element's top over plan point `p`: the highest top of its solids standing over it.
pub(crate) fn tops_at(m: &Model, p: P2) -> BTreeMap<Id, f64> {
    let mut tops = BTreeMap::new();
    for s in solids(m).iter().filter(|s| s.over(p)) {
        let z = s.z_at(p).1;
        let top = tops.entry(s.owner()).or_insert(z);
        *top = f64::max(*top, z);
    }
    tops
}

/// `a`, and `b` over it: `b` as much as `weight` (0 to 1) says.
fn blend(a: Rgb, b: Rgb, weight: f64) -> Rgb {
    let m = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * weight.clamp(0.0, 1.0)).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Halfway from `a` to `b`.
fn mix(a: Rgb, b: Rgb) -> Rgb {
    let m = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Where the 3D view looks from, and how much of the model its picture holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Degrees counter-clockwise from east, around the model, of where it looks from.
    pub azimuth: f64,
    /// Degrees above the horizon it looks down from, within ±89.
    pub elevation: f64,
    /// Pixels per mm.
    pub scale: f64,
    /// The model point in the middle of the picture.
    pub target: P3,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    /// Toward the camera, and the picture's right and up, in the model.
    fn axes(&self) -> (P3, P3, P3) {
        let (a, e) = (self.azimuth.to_radians(), self.elevation.clamp(-89.0, 89.0).to_radians());
        let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
        let right = [-a.sin(), a.cos(), 0.0];
        (back, right, cross(back, right))
    }

    /// The pixel `p` shows at, and how near the camera it is (larger is nearer).
    pub fn project(&self, p: P3) -> (f64, f64, f64) {
        let (back, right, up) = self.axes();
        let d = sub(p, self.target);
        (f64::from(self.width) / 2.0 + dot(d, right) * self.scale, f64::from(self.height) / 2.0 - dot(d, up) * self.scale, dot(d, back))
    }

    /// The point at pixel `(x, y)` on the horizontal plane at height `z`; none where the view is
    /// level with it.
    pub fn on_plane(&self, at: (f64, f64), z: f64) -> Option<P3> {
        let (back, ..) = self.axes();
        let p = self.unproject(at);
        let t = (z - p[2]) / back[2];
        (back[2].abs() > 1e-9 && t.is_finite()).then(|| add(p, mul(back, t)))
    }

    /// The model point at pixel `(x, y)` on the plane through the target, square to the view.
    pub fn unproject(&self, (x, y): (f64, f64)) -> P3 {
        let (_, right, up) = self.axes();
        let (dx, dy) = ((x - f64::from(self.width) / 2.0) / self.scale, (y - f64::from(self.height) / 2.0) / self.scale);
        add(self.target, sub(mul(right, dx), mul(up, dy)))
    }

    /// Turned `around` degrees about the target and raised `up` degrees. The azimuth stays within
    /// (-180, 180].
    pub fn orbit(self, around: f64, up: f64) -> Camera {
        let a = (self.azimuth + around).rem_euclid(360.0);
        Camera { azimuth: if a > 180.0 { a - 360.0 } else { a }, elevation: (self.elevation + up).clamp(-89.0, 89.0), ..self }
    }

    /// Moved so the model point under pixel `from` comes under pixel `to`.
    pub fn pan(self, from: (f64, f64), to: (f64, f64)) -> Camera {
        Camera { target: add(self.target, sub(self.unproject(from), self.unproject(to))), ..self }
    }

    /// Zoomed by `factor` about pixel `at`, which keeps its model point.
    pub fn zoom(self, factor: f64, at: (f64, f64)) -> Camera {
        let p = self.unproject(at);
        let zoomed = Camera { scale: (self.scale * factor).clamp(1e-6, 50.0), ..self };
        zoomed.pan(zoomed.project_xy(p), at)
    }

    fn project_xy(&self, p: P3) -> (f64, f64) {
        let (x, y, _) = self.project(p);
        (x, y)
    }
}

/// A picture of the 3D view: its pixels, and which element each one shows.
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row by row: red, green, blue and alpha.
    pub rgba: Vec<u8>,
    /// Each pixel's triangle (`u32::MAX` for none), and each triangle's element.
    hit: Vec<u32>,
    owners: Vec<Id>,
    faces: Arc<Vec<&'static str>>,
}

impl Image {
    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        (x >= 0.0 && y >= 0.0 && x < w && y < h).then(|| y as usize * self.width as usize + x as usize)
    }

    /// The element the pixel at `(x, y)` shows.
    pub fn pick(&self, x: f64, y: f64) -> Option<Id> {
        let t = *self.hit.get(self.index(x, y)?)?;
        self.owners.get(t as usize).copied()
    }

    /// The face of the element the pixel at `(x, y)` shows, as paint names it: `exterior` or `interior` for a wall's, `top` or
    /// `bottom` for a floor's, a roof's or a ceiling's, `all` for another's; none where it's on no face that can be painted.
    pub fn face(&self, x: f64, y: f64) -> Option<&'static str> {
        let t = *self.hit.get(self.index(x, y)?)?;
        self.faces.get(t as usize).copied().filter(|f| !f.is_empty())
    }

    /// The elements the picture shows in the box from pixel `a` to `b`: those it shows only
    /// inside it, or with `crossing`, any of.
    pub fn in_box(&self, a: (f64, f64), b: (f64, f64), crossing: bool) -> Vec<Id> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: BTreeMap<Id, (bool, bool)> = BTreeMap::new();
        for (k, t) in self.hit.iter().enumerate() {
            let Some(&id) = self.owners.get(*t as usize) else { continue };
            let (x, y) = ((k % self.width as usize) as f64 + 0.5, (k / self.width as usize) as f64 + 0.5);
            let inside = (lo.0..=hi.0).contains(&x) && (lo.1..=hi.1).contains(&y);
            let e = seen.entry(id).or_insert((true, false));
            *e = (e.0 && inside, e.1 || inside);
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }

    /// Every element the picture shows.
    pub fn shows(&self) -> BTreeSet<Id> {
        self.hit.iter().filter_map(|t| self.owners.get(*t as usize).copied()).collect()
    }

    /// The colour of the pixel at `(x, y)`; the paper's, off the picture.
    pub fn color(&self, x: f64, y: f64) -> Rgb {
        match self.index(x, y).and_then(|k| self.rgba.get(4 * k..4 * k + 3)) {
            Some([r, g, b]) => Rgb(*r, *g, *b),
            _ => PAPER,
        }
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let size = tiny_skia::IntSize::from_wh(self.width, self.height).ok_or("the picture's size is out of range")?;
        let pm = tiny_skia::Pixmap::from_vec(self.rgba.clone(), size).ok_or("the picture's size is out of range")?;
        pm.encode_png().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coplanar_triangles_share_a_plane_and_degenerate_ones_each_their_own() {
        let tri = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: None };
        // A face's triangles a micron off its plane, as a file rounds them, are one plane;
        // triangles with no area are each their own, and many of them stay quick.
        let mut tris: Vec<Tri> = (0..200)
            .map(|k| {
                let (x, e) = (f64::from(k) * 100.0, f64::from(k % 7) * 1e-4);
                tri([[x, 0.0, 1000.0 + e], [x + 100.0, 0.0, 1000.0], [x, 100.0, 1000.0 - e]])
            })
            .collect();
        tris.extend((0..20_000).map(|k| tri([[f64::from(k), 0.0, 0.0]; 3])));
        let started = std::time::Instant::now();
        let ids = planes(&tris);
        assert!(started.elapsed().as_secs_f64() < 1.0, "{:?}", started.elapsed());
        assert!(ids.iter().take(200).all(|id| *id == ids[0]), "one plane");
        let degenerate: std::collections::BTreeSet<usize> = ids.iter().skip(200).copied().collect();
        assert_eq!(degenerate.len(), 20_000);
    }

    /// A cylinder of `n` sides standing on the origin, turned out: its `n` top triangles, its `n`
    /// bottom ones, then its `2n` sides. The last corner of a ring is a hair off the first, as a
    /// file's rounding leaves it. Four sides make a box.
    fn cylinder(n: usize, r: f64, h: f64) -> Vec<[P3; 3]> {
        let ring = |z: f64, k: usize| {
            let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
            [r * c, r * s, z]
        };
        let (mut top, mut bottom, mut sides) = (Vec::new(), Vec::new(), Vec::new());
        for k in 0..n {
            let (a, b, c, d) = (ring(0.0, k), ring(0.0, k + 1), ring(h, k), ring(h, k + 1));
            top.push([[0.0, 0.0, h], c, d]);
            bottom.push([[0.0, 0.0, 0.0], b, a]);
            sides.extend([[a, b, d], [a, d, c]]);
        }
        [top, bottom, sides].concat()
    }

    #[test]
    fn a_meshs_facets_that_turn_little_read_as_one_surface_and_its_creases_keep_their_edges() {
        let mesh = |owner, tris: Vec<[P3; 3]>| Solid::Mesh { owner, color: EDGE, tex: 0, caps: tris.len(), round: false, cut: false, tris };
        // Each cylinder's planes: its sides one surface where they turn 20° or less at their edges,
        // else a plane to each side; and a plane to each cap, however many triangles make it.
        for (n, planes_of) in [(4, 6), (8, 10), (16, 18), (20, 3), (24, 3), (64, 3)] {
            let mut out = Vec::new();
            mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
            let ids = planes(&out);
            let (top, bottom, sides) = (&ids[..n], &ids[n..2 * n], &ids[2 * n..]);
            assert!(top.iter().all(|id| *id == top[0]) && bottom.iter().all(|id| *id == bottom[0]), "{n} sides: a cap is one plane");
            assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), planes_of, "{n} sides: {ids:?}");
            assert!(top[0] != bottom[0] && !sides.contains(&top[0]) && !sides.contains(&bottom[0]), "{n} sides: the caps keep their edges");
        }
        // Two forms of one element, one on the other, each its own surface where their facets touch.
        let n = 24;
        let mut out = Vec::new();
        for _ in 0..2 {
            mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
        }
        let ids = planes(&out);
        assert_ne!(ids[2 * n], ids[6 * n], "{ids:?}");
        // A mesh as big as a file may bring stays quick.
        let started = std::time::Instant::now();
        let mut out = Vec::new();
        mesh(1, cylinder(25_000, 200.0, 450.0)).tris(&mut out);
        let (ids, normals) = (planes(&out), corner_normals(&out));
        assert!(started.elapsed().as_secs_f64() < 1.0, "{:?}", started.elapsed());
        assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
        assert_eq!(normals.iter().flatten().count(), 50_000, "its sides, and not its caps");
        // Corners that aren't numbers, and triangles with no area, join nothing, and don't crash it.
        let odd = [[[f64::NAN; 3]; 3], [[f64::INFINITY; 3]; 3], [[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]], [[0.0; 3]; 3]];
        assert_eq!(facets(&odd), vec![None; 4]);
    }

    use crate::engine::Session;

    /// The 8 x 6 m box of 200 mm walls on Level 1 (walls 3 south, 4 east, 5 north, 6 west), a door
    /// (7) in the south wall, a window (8) in the east one, and a floor (9).
    fn house() -> Session {
        let mut s = Session::new();
        s.script(
            r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
               door {"at": [2000, 0]}
               window {"at": [8000, 3000]}
               floor {}"#,
        )
        .unwrap();
        s
    }

    fn view(s: &Session) -> (Camera, Image) {
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let img = scene.render(&cam, &[]);
        (cam, img)
    }

    fn at(cam: &Camera, p: P3) -> (f64, f64) {
        let (x, y, _) = cam.project(p);
        (x, y)
    }

    #[test]
    fn each_pixel_shows_the_nearest_surface_and_the_element_it_belongs_to() {
        let s = house();
        let (cam, img) = view(&s);
        let pick = |p: P3| {
            let (x, y) = at(&cam, p);
            img.pick(x, y)
        };
        assert_eq!(pick([5000.0, -100.0, 1500.0]), Some(100002), "the south wall's outer face");
        assert_eq!(pick([2000.0, -25.0, 1000.0]), Some(100006), "the door, seen through its opening");
        assert_eq!(pick([8025.0, 3000.0, 1650.0]), Some(100007), "the window's glass");
        assert_eq!(pick([2000.0, 4500.0, 0.0]), Some(100008), "the floor, seen over the walls");
        assert_eq!(pick([4000.0, 1000.0, 0.0]), Some(100002), "the floor behind the south wall: the wall hides it");
        assert_eq!(img.pick(1.0, 1.0), None, "the background");
    }

    #[test]
    fn faces_shade_evenly_and_edges_mark_where_they_meet() {
        let s = house();
        let (cam, img) = view(&s);
        let color = |p: P3| {
            let (x, y) = at(&cam, p);
            img.color(x, y)
        };
        // Along the south wall's face past the door: one shade, no seams from the triangles under it.
        let row: Vec<Rgb> = (0..20).map(|i| color([3000.0 + 140.0 * f64::from(i), -100.0, 1800.0])).collect();
        assert!(row.iter().all(|c| *c == row[0]) && row[0] != EDGE, "{row:?}");
        // Over the corner where the south and east walls overlap, their tops make one face: zoomed in
        // on it, across it, away from its outer edges.
        let close = cam.zoom(4.0, at(&cam, [8000.0, 0.0, 3000.0]));
        let img4 = Scene::new(&s.model).render(&close, &[]);
        let top: Vec<Rgb> = (0..9)
            .map(|i| {
                let (x, y) = at(&close, [7860.0 + 20.0 * f64::from(i), 40.0, 3000.0]);
                img4.color(x, y)
            })
            .collect();
        assert!(top.iter().all(|c| *c == top[0]) && top[0] != EDGE, "{top:?}");
        // Where the south wall's top meets its face, an edge.
        let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
        let near: Vec<Rgb> = [-1.0, 0.0, 1.0].iter().flat_map(|dx| [-1.0, 0.0, 1.0].map(|dy| img.color(x + dx, y + dy))).collect();
        assert!(near.contains(&EDGE), "{near:?}");
    }

    /// A session whose one component is a post: a mesh form, a cylinder of 24 sides, 400 mm across and a metre tall.
    fn post() -> Session {
        let mut s = Session::new();
        let post = serde_json::json!({"name": "Post", "types": {"A": {}}, "forms": [{"mesh": cylinder(24, 200.0, 1000.0)}]});
        s.execute("family", &post).unwrap();
        s.execute("component", &serde_json::json!({"family": "Post", "at": [0, 0]})).unwrap();
        s
    }

    /// The most any channel moves between neighbours along `colors`.
    fn step(colors: &[Rgb]) -> u8 {
        colors.windows(2).map(|w| w[0].0.abs_diff(w[1].0).max(w[0].1.abs_diff(w[1].1)).max(w[0].2.abs_diff(w[1].2))).max().unwrap_or(0)
    }

    #[test]
    fn a_curves_corners_take_the_normals_of_the_surface_they_are_on() {
        let n = 24;
        let mut out = Vec::new();
        Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: cylinder(n, 200.0, 450.0) }.tris(&mut out);
        let normals = corner_normals(&out);
        // Its caps are flat faces: none. Each corner of its sides points straight out from the axis, the top
        // ones as the bottom ones (two triangles meet at some corners, one at others).
        assert!(normals[..2 * n].iter().all(Option::is_none));
        for (t, ns) in out[2 * n..].iter().zip(&normals[2 * n..]) {
            for (p, v) in t.p.iter().zip(ns.as_deref().unwrap()) {
                assert!(dot(*v, unit([p[0], p[1], 0.0])) > 1.0 - 1e-9, "{p:?} {v:?}");
            }
        }
        // A cone on it, a surface of its own: the ring they share keeps a step, the sides' corners
        // level and the cone's tilted up, its apex straight up.
        let ring = |k: usize| {
            let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
            [200.0 * c, 200.0 * s, 450.0]
        };
        let mut roofed = cylinder(n, 200.0, 450.0);
        roofed.extend((0..n).map(|k| [ring(k), ring(k + 1), [0.0, 0.0, 850.0]]));
        let mut out = Vec::new();
        Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: roofed }.tris(&mut out);
        let normals = corner_normals(&out);
        let normal = |i: usize| normals[i].as_deref().unwrap();
        assert!((2 * n..4 * n).all(|i| normal(i).iter().all(|v| v[2].abs() < 1e-9)));
        assert!((4 * n..5 * n).all(|i| normal(i)[0][2] > 0.4 && normal(i)[1][2] > 0.4 && normal(i)[2][2] > 1.0 - 1e-9));
        // Triangles with corners that aren't numbers, or no area, have none, and don't crash it.
        let odd = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: Some((1, 0)) };
        let odd = [
            odd([[f64::NAN; 3]; 3]),
            odd([[f64::INFINITY; 3]; 3]),
            odd([[0.0; 3]; 3]),
            odd([[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]]),
        ];
        assert!(corner_normals(&odd).iter().all(Option::is_none));
    }

    #[test]
    fn a_curve_shades_smoothly_across_its_facets_and_a_flat_face_in_one_shade() {
        let s = post();
        let mut scene = Scene::new(&s.model);
        // Its shading, not the levels' planes.
        scene.levels.clear();
        let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
        let img = scene.render(&cam, &[]);
        // Across its side, edge to edge: no step between neighbours bigger than a level, though it does change.
        let (x, y) = at(&cam, [0.0, 0.0, 500.0]);
        let reach = (190.0 * cam.scale) as i32;
        let across: Vec<Rgb> = (-reach..=reach).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
        let (first, last) = (across[0], across[across.len() - 1]);
        assert!(step(&across) <= 1 && step(&[first, last]) > 15, "{} px, a step of {}, {first:?} to {last:?}", across.len(), step(&across));
        // Across its top, one shade.
        let (x, y) = at(&cam, [0.0, 0.0, 1000.0]);
        let top: Vec<Rgb> = (-40..=40).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
        assert!(top.iter().all(|c| *c == top[0]), "{top:?}");
    }

    #[test]
    fn a_curve_shades_as_its_own_facets_would_to_within_their_turn_whichever_side_is_seen() {
        // An open tube: the far wall of it is seen from inside, its normals turned away.
        let mut s = Session::new();
        let tube = serde_json::json!({"name": "Tube", "types": {"A": {}}, "forms": [{"mesh": cylinder(24, 200.0, 1000.0)[48..]}]});
        s.execute("family", &tube).unwrap();
        s.execute("component", &serde_json::json!({"family": "Tube", "at": [0, 0]})).unwrap();
        let (smooth, mut flat) = (Scene::new(&s.model), Scene::new(&s.model));
        flat.smooth.clear();
        let cam = smooth.fit(AZIMUTH, ELEVATION, 800, 600);
        let (a, b) = (smooth.render(&cam, &[]), flat.render(&cam, &[]));
        let apart: Vec<u8> = a.rgba.iter().zip(&b.rgba).map(|(x, y)| x.abs_diff(*y)).collect();
        assert!(apart.iter().max().is_some_and(|d| (1..=6).contains(d)), "{:?}", apart.iter().max());
    }

    #[test]
    fn a_curved_wall_shades_smoothly_along_its_face() {
        let mut s = Session::new();
        s.script(r#"wall {"points": [[0, 0], [4000, 0]], "arc": 180}"#).unwrap();
        let mut scene = Scene::new(&s.model);
        scene.levels.clear();
        let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
        let img = scene.render(&cam, &[]);
        // Along its face at mid-height, between its ends: the row's pixels that show the wall.
        let (_, y) = at(&cam, [2000.0, -2000.0, 1500.0]);
        let shown: Vec<i32> = (0..800).filter(|px| img.pick(f64::from(*px) + 0.5, y.floor() + 0.5).is_some()).collect();
        let (lo, hi) = (shown.iter().min().unwrap() + 6, shown.iter().max().unwrap() - 6);
        let along: Vec<Rgb> = (lo..=hi).map(|px| img.color(f64::from(px) + 0.5, y.floor() + 0.5)).collect();
        let (first, last) = (along[0], along[along.len() - 1]);
        assert!(
            along.len() > 200 && step(&along) <= 1 && step(&[first, last]) > 8,
            "{} px, a step of {}, {first:?} to {last:?}",
            along.len(),
            step(&along)
        );
    }

    #[test]
    fn an_imported_curve_shows_no_facet_lines_and_keeps_the_edge_of_its_rim() {
        let s = post();
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
        let img = scene.render(&cam, &[]);
        // Across its side at mid-height, every pixel from one edge of it to the other.
        let (x, y) = at(&cam, [0.0, 0.0, 500.0]);
        let reach = (190.0 * cam.scale) as i32;
        let across: Vec<Rgb> = (-reach..=reach).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
        assert!(!across.contains(&EDGE), "{across:?}");
        // Where its top meets its side, nearest the camera, an edge.
        let (x, y) = at(&cam, [141.0, -141.0, 1000.0]);
        let rim: Vec<Rgb> = (-3..=3).map(|dy| img.color(x, y + f64::from(dy))).collect();
        assert!(rim.contains(&EDGE), "{rim:?}");
    }

    #[test]
    fn a_family_door_and_window_show_their_forms_in_the_wall_and_mirror_with_their_flips() {
        let mut s = house();
        s.script(
            r#"delete {"ids": [100006, 100007]}
               door {"at": [2000, 0], "family": "Single Door", "type": "800 x 2100"}
               window {"at": [8000, 3000], "family": "Fixed Window", "type": "600 x 900"}"#,
        )
        .unwrap();
        // The door and the window are 10 and 11 now (9 is the floor).
        let (door, window) = (100009, 100010);
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
        let pick_at = |cam: &Camera, img: &Image, p: P3| {
            let (x, y) = at(cam, p);
            img.pick(x, y)
        };
        let near = |p: P3| {
            let close = cam.zoom(6.0, at(&cam, p));
            let img = scene.render(&close, &[]);
            pick_at(&close, &img, p)
        };
        // Seen through the opening: the leaf's face (40 thick, in the middle of the wall), a jamb of the frame, a handle.
        assert_eq!(near([2000.0, -20.0, 1000.0]), Some(door), "the leaf");
        assert_eq!(near([1625.0, -100.0, 1000.0]), Some(door), "the jamb");
        assert_eq!(near([2250.0, -75.0, 1015.0]), Some(door), "the handle, 55 proud of the leaf");
        // The window's glass is 6 thick, in the middle of its wall, and not what a classic window's 50 mm panel is.
        assert_eq!(near([8003.0, 3000.0, 1550.0]), Some(window), "the glass");
        assert_eq!(near([5000.0, -100.0, 1500.0]), Some(100002), "the wall beside them");
        // Its hinge side's jamb is still there, on the other jamb's side, once flipped.
        run_flip(&mut s);
        let scene = Scene::new(&s.model);
        let close = cam.zoom(6.0, at(&cam, [2000.0, -20.0, 1000.0]));
        let img = scene.render(&close, &[]);
        // The handle moved to the other side of the leaf: where it was is only the leaf's face now.
        assert_eq!(pick_at(&close, &img, [1750.0, -75.0, 1015.0]), Some(door), "the handle on the other side");
    }

    fn run_flip(s: &mut Session) {
        s.execute("modify", &serde_json::json!({"id": 100009, "flip_hand": true})).unwrap();
    }

    /// The house with its walls of two layers: brick on the left of each, and a block to its right.
    fn brick_house() -> Session {
        let mut s = house();
        s.script(
            r#"type {"category": "wall", "name": "Brick wall", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Concrete block", "thickness": 100}]}
               modify {"ids": [100002, 100003, 100004, 100005], "type": "Brick wall"}"#,
        )
        .unwrap();
        s
    }

    /// A height in the middle of a brick course (each 75 mm), up a wall.
    const COURSE: f64 = 1537.5;

    /// The view of `s` from the default angle, a picture 1600 by 1200 centred on `target`, `scale` px to the mm.
    fn closeup(s: &Session, target: P3, scale: f64) -> (Scene, Camera) {
        let scene = Scene::new(&s.model);
        let cam = Camera { target, scale, ..scene.fit(AZIMUTH, ELEVATION, 1600, 1200) };
        (scene, cam)
    }

    /// The colours along the north wall's face toward the house (its brick), from `x0` to `x1` mm at 1537.5 up (the middle of a course: 1500 is
    /// a joint), seen close up.
    fn along_the_brick(s: &Session, x0: f64, x1: f64, n: i32) -> Vec<Rgb> {
        let (scene, close) = closeup(s, [4000.0, 5900.0, COURSE], 0.8);
        let img = scene.render(&close, &[]);
        (0..n)
            .map(|i| {
                let (x, y) = at(&close, [x0 + (x1 - x0) * f64::from(i) / f64::from(n - 1), 5900.0, COURSE]);
                img.color(x, y)
            })
            .collect()
    }

    #[test]
    fn a_textured_material_varies_over_its_faces_at_its_real_size_and_none_is_one_shade() {
        let mut s = brick_house();
        let row = along_the_brick(&s, 3200.0, 4800.0, 120);
        let distinct: BTreeSet<(u8, u8, u8)> = row.iter().map(|c| (c.0, c.1, c.2)).collect();
        assert!(distinct.len() > 8, "bricks and joints: {} colours", distinct.len());
        assert!(row.iter().all(|c| c.0 > c.2), "brick red and the mortar's cream, never blue: {:?}", &row[..4]);
        // Its tile is 900 mm across: what's at one point is at the next, a tile along, within a level or two.
        let (a, b) = (along_the_brick(&s, 3300.0, 3300.0 + 1e-9, 2), along_the_brick(&s, 4200.0, 4200.0 + 1e-9, 2));
        let near = |p: Rgb, q: Rgb| p.0.abs_diff(q.0) <= 4 && p.1.abs_diff(q.1) <= 4 && p.2.abs_diff(q.2) <= 4;
        assert!(near(a[0], b[0]), "{:?} {:?}", a[0], b[0]);
        // A size of half that, and the pattern repeats twice as often: now a half tile along is the same.
        s.execute("material", &serde_json::json!({"name": "Brick", "texture": {"size": [450, 300]}})).unwrap();
        let (c, d) = (along_the_brick(&s, 3300.0, 3300.0 + 1e-9, 2), along_the_brick(&s, 3750.0, 3750.0 + 1e-9, 2));
        assert!(near(c[0], d[0]), "{:?} {:?}", c[0], d[0]);
        // Without its texture, one shade along the whole face, as before.
        s.execute("material", &serde_json::json!({"name": "Brick", "texture": null})).unwrap();
        let plain = along_the_brick(&s, 3200.0, 4800.0, 120);
        assert!(plain.iter().all(|c| *c == plain[0]), "{:?}", &plain[..4]);
        // The same wall in a section is paper and line, as it was: a texture is for the shaded view.
        s.execute("material", &serde_json::json!({"name": "Brick", "texture": {"kind": "brick"}})).unwrap();
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let clip = Clip { near: f64::MAX, far: None, x: (0.0, 400.0), y: (0.0, 300.0) };
        let cut = scene.cut(&cam, &clip, &[]);
        assert!(cut.rgba.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]), "grey, never a colour");
    }

    /// A PNG file whose left half is red and whose right half is blue, in the scratch folder.
    fn halves(name: &str) -> String {
        let (w, h) = (64u32, 8u32);
        let rgba: Vec<u8> = (0..w * h).flat_map(|k| if k % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
        let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
        let path = std::env::temp_dir().join(format!("bimcraft-{name}-{}.png", std::process::id()));
        std::fs::write(&path, png).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_picture_made_a_texture_lies_on_the_floor_as_laid_and_paint_gives_a_face_its_own_material() {
        let mut s = house();
        let path = halves("floor");
        // A floor of a material textured with the picture: a tile of 1000 by 1000 mm, red where x is under 500 from a metre's mark.
        s.script(&format!(
            r##"material {{"name": "Stripes", "color": "#808080", "texture": {{"kind": "image", "path": {path:?}, "size": [1000, 1000]}}}}
               type {{"category": "floor", "name": "Striped", "layers": [{{"material": "Stripes", "thickness": 200}}]}}
               delete {{"ids": [100008]}}
               floor {{"type": "Striped"}}"##
        ))
        .unwrap();
        // Seen from over the house's south-east, a floor point is hidden by the south wall unless it's over 3.7 m north of it, and by
        // the east wall unless it's over 3.7 m west of it: these, 4500 north and 1250 to 3750 east, aren't.
        let (scene, close) = closeup(&s, [2500.0, 4500.0, 0.0], 0.4);
        let img = scene.render(&close, &[]);
        let at_floor = |x: f64| {
            let (px, py) = at(&close, [x, 4500.0, 0.0]);
            img.color(px, py)
        };
        let (red, blue) = (|c: Rgb| i32::from(c.0) - i32::from(c.2), |c: Rgb| i32::from(c.2) - i32::from(c.0));
        for x in [1250.0, 2250.0, 3250.0] {
            let c = at_floor(x);
            assert!(red(c) > 80, "{x}: red, {c:?}");
        }
        for x in [1750.0, 2750.0, 3750.0] {
            let c = at_floor(x);
            assert!(blue(c) > 80, "{x}: blue, {c:?}");
        }
        // Moved by 500 mm, the halves swap; the picture itself is the project's, once.
        s.execute("material", &serde_json::json!({"name": "Stripes", "texture": {"offset": [500, 0]}})).unwrap();
        let (scene, close) = closeup(&s, [2500.0, 4500.0, 0.0], 0.4);
        let img = scene.render(&close, &[]);
        let (px, py) = at(&close, [3250.0, 4500.0, 0.0]);
        let c = img.color(px, py);
        assert!(blue(c) > 80, "blue now: {c:?}");
        assert_eq!(s.model.images.len(), 1);
        std::fs::remove_file(path).unwrap();
        // Paint: the face of a wall toward the house (the exterior of these walls, drawn the way round they are) in a material
        // of its own; the face away from it, the other.
        let mut s = brick_house();
        s.execute("paint", &serde_json::json!({"ids": [100004], "face": "exterior", "material": "Timber"})).unwrap();
        let (scene, close) = closeup(&s, [4000.0, 5900.0, COURSE], 0.8);
        let img = scene.render(&close, &[]);
        let (x, y) = at(&close, [4000.0, 5900.0, COURSE]);
        assert_eq!((img.pick(x, y), img.face(x, y)), (Some(100004), Some("exterior")));
        let row: Vec<Rgb> = (0..60).map(|i| img.color(at(&close, [3200.0 + 25.0 * f64::from(i), 5900.0, COURSE]).0, y)).collect();
        // Timber's tan, in boards (a gap between two, a joint along one, darker): where brick was red.
        let tan = row.iter().filter(|c| i32::from(c.0) - i32::from(c.2) > 30 && u32::from(c.1) * 100 > u32::from(c.0) * 70).count();
        assert!(tan >= 50 && row.iter().all(|c| c.0 >= c.2), "{tan} of 60 tan: {:?}", &row[..3]);
        // The outer face of the south wall is on its interior (a wall's exterior is to the left of its start to its end), and the
        // floor and the sky have faces of their own or none.
        let far = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
        let seen = scene.render(&far, &[]);
        let (fx, fy) = at(&far, [4000.0, -100.0, 1500.0]);
        assert_eq!((seen.pick(fx, fy), seen.face(fx, fy)), (Some(100002), Some("interior")));
        assert_eq!(seen.face(1.0, 1.0), None, "the sky");
        let top = at(&far, [4000.0, 4500.0, 0.0]);
        assert_eq!((seen.pick(top.0, top.1), seen.face(top.0, top.1)), (Some(100008), Some("top")), "the floor's");
    }

    #[test]
    fn paint_changes_only_the_face_it_names() {
        let mut s = brick_house();
        let before = Scene::new(&s.model);
        s.execute("paint", &serde_json::json!({"ids": [100004], "face": "exterior", "material": "Timber"})).unwrap();
        let after = Scene::new(&s.model);
        let timber = s.model.materials["Timber"].color;
        assert_eq!(before.tris.len(), after.tris.len());
        let (mut painted, mut kept) = (0, 0);
        for (i, (a, b)) in before.tris.iter().zip(&after.tris).enumerate() {
            if a.owner == 100004 && after.faces[i] == "exterior" {
                assert_eq!(b.color, Rgb(timber[0], timber[1], timber[2]), "triangle {i}");
                painted += 1;
            } else {
                assert_eq!(
                    (a.color, a.tex),
                    (b.color, b.tex),
                    "triangle {i}: the {:?} face of {} keeps what it was",
                    after.faces[i],
                    a.owner
                );
                kept += usize::from(a.owner == 100004);
            }
        }
        assert!(painted > 0 && kept > 0, "{painted} painted, {kept} left of the wall's others");
    }

    #[test]
    fn a_texture_on_a_wall_runs_across_to_the_right_of_someone_looking_at_its_face_from_outside() {
        let mut s = house();
        let path = halves("wall");
        // Red where x is under 500 from a metre's mark, laid on the south wall's outer face: its face looks south, so across is east.
        s.script(&format!(
            r##"material {{"name": "Stripes", "color": "#808080", "texture": {{"kind": "image", "path": {path:?}, "size": [1000, 1000]}}}}
               paint {{"ids": [100002], "face": "interior", "material": "Stripes"}}"##
        ))
        .unwrap();
        let scene = Scene::new(&s.model);
        let far = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
        let img = scene.render(&far, &[]);
        let colour = |x: f64| {
            let (px, py) = at(&far, [x, -100.0, 1500.0]);
            let c = img.color(px, py);
            i32::from(c.0) - i32::from(c.2)
        };
        for x in [4250.0, 5250.0, 6250.0] {
            assert!(colour(x) > 80, "{x}: red");
        }
        for x in [4750.0, 5750.0, 6750.0] {
            assert!(colour(x) < -80, "{x}: blue");
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tinted_elements_show_in_their_tint() {
        let s = house();
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let (x, y) = at(&cam, [5000.0, -100.0, 1500.0]);
        let plain = scene.render(&cam, &[]).color(x, y);
        let tinted = scene.render(&cam, &[(100002, Rgb(37, 99, 235))]).color(x, y);
        assert!(plain.2 <= plain.0 && tinted.2 > tinted.0 + 40, "{plain:?} {tinted:?}");
    }

    #[test]
    fn the_camera_fits_the_model_and_orbits_pans_and_zooms() {
        let s = house();
        let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 400, 300);
        for (x, y) in [(0.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (0.0, 6000.0)] {
            for z in [0.0, 3000.0] {
                let (px, py) = at(&cam, [x, y, z]);
                assert!((0.0..400.0).contains(&px) && (0.0..300.0).contains(&py), "[{x}, {y}, {z}] at {px}, {py}");
            }
        }
        // Orbiting turns about the target, which stays in the middle of the image.
        let turned = cam.orbit(90.0, 100.0);
        assert_eq!((turned.azimuth, turned.elevation), (45.0, 89.0), "never quite straight down");
        let (x, y) = at(&turned, turned.target);
        assert!((x - 200.0).abs() < 1e-9 && (y - 150.0).abs() < 1e-9);
        // A pan carries the point under one pixel to another; a zoom keeps the point under the cursor.
        let p = cam.unproject((100.0, 80.0));
        let (x, y) = at(&cam.pan((100.0, 80.0), (150.0, 120.0)), p);
        assert!((x - 150.0).abs() < 1e-6 && (y - 120.0).abs() < 1e-6);
        let zoomed = cam.zoom(2.0, (100.0, 80.0));
        let (x, y) = at(&zoomed, p);
        assert!((x - 100.0).abs() < 1e-6 && (y - 80.0).abs() < 1e-6 && zoomed.scale == cam.scale * 2.0);
    }

    #[test]
    fn the_picture_saves_as_png() {
        let s = house();
        let (_, img) = view(&s);
        let pm = tiny_skia::Pixmap::decode_png(&img.png().unwrap()).unwrap();
        assert_eq!((pm.width(), pm.height()), (400, 300));
    }

    /// The house drawn in `style`, from the default view, through the default 3D view's look.
    fn styled(s: &mut Session, style: &str) -> (Camera, Image) {
        s.execute("visual_style", &serde_json::json!({"style": style})).unwrap();
        let scene = Scene::new(&s.model);
        let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
        let img = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
        (cam, img)
    }

    /// The pixels of `img` that are lines or shading: not paper.
    fn inked(img: &Image) -> usize {
        img.rgba.chunks(4).filter(|c| c[..3] != [255, 255, 255]).count()
    }

    /// Whether a line passes within a pixel of `p`.
    fn near_a_line(cam: &Camera, img: &Image, p: P3) -> bool {
        let (x, y) = at(cam, p);
        (-1..=1).any(|dy| (-1..=1).any(|dx| img.color(x + f64::from(dx), y + f64::from(dy)) != PAPER))
    }

    #[test]
    fn hidden_line_draws_faces_as_paper_with_their_edges_and_wireframe_every_edge_through_them() {
        let mut s = house();
        let (cam, shaded) = view(&s);
        let south = at(&cam, [5000.0, -100.0, 1500.0]);
        assert_ne!(shaded.color(south.0, south.1), PAPER, "shaded: a lit face");
        // Hidden line: paper faces, and their edges dark, as a section's picture has them.
        let (_, hidden) = styled(&mut s, "hidden_line");
        assert_eq!(hidden.color(south.0, south.1), PAPER);
        assert!(inked(&hidden) > 300 && inked(&hidden) < inked(&shaded) / 4, "{} of {}", inked(&hidden), inked(&shaded));
        let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
        assert_ne!(hidden.color(x, y), PAPER, "the south wall's top edge");
        // The outer corner of the north west walls stands at the back, behind the walls: wireframe draws it through them.
        let hidden_edge = [-100.0, 6100.0, 1500.0];
        let (_, wire) = styled(&mut s, "wireframe");
        assert!(near_a_line(&cam, &wire, hidden_edge), "wireframe shows every edge");
        let through = wire.rgba.chunks(4).zip(hidden.rgba.chunks(4)).filter(|(w, h)| w[..3] != [255; 3] && h[..3] == [255; 3]).count();
        assert!(through > 50, "lines hidden line keeps hidden: {through}");
        // Faces stay paper; picking an edge finds its wall, a face nothing.
        assert_eq!(wire.color(south.0, south.1), PAPER);
        assert!(inked(&wire) > 300 && inked(&wire) < inked(&hidden) * 2, "{}", inked(&wire));
        assert_eq!(wire.pick(south.0, south.1), None, "a face isn't there to pick");
        assert_eq!(wire.pick(x, y), Some(100002), "the top edge of the south wall");
        // Back to shaded, the picture is as it was.
        let (_, again) = styled(&mut s, "shaded");
        assert_eq!(again.rgba, shaded.rgba);
    }

    #[test]
    fn wireframe_keeps_what_the_view_hides_hidden_and_takes_a_tint_and_an_edge_colour() {
        let mut s = house();
        let (cam, all) = styled(&mut s, "wireframe");
        s.script(r#"graphics {"view": "{3D}", "hide": ["door"]}"#).unwrap();
        let scene = Scene::new(&s.model);
        let look = Look::of_3d(&s.model);
        let plain = scene.render_in(&cam, &[], &look);
        assert!(inked(&plain) < inked(&all), "the door's edges are gone: {} of {}", inked(&plain), inked(&all));
        let tinted = scene.render_in(&cam, &[(100002, Rgb(255, 0, 0))], &look);
        let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
        assert_eq!(tinted.color(x, y), Rgb(255, 0, 0), "a selected wall's edges in the selection's colour");
        let colours = plain.rgba.chunks(4).filter(|c| c[..3] != [255, 255, 255]).map(|c| (c[0], c[1], c[2])).collect::<BTreeSet<_>>();
        assert_eq!(colours, BTreeSet::from([(EDGE.0, EDGE.1, EDGE.2)]), "lines in the edge colour, nothing else");
    }

    #[test]
    fn a_section_cuts_a_tube_in_its_ring_an_oblique_one_in_an_ellipse_and_leaves_other_meshes_whole() {
        use crate::runs::Geom;
        // A pipe 200 across along east at 500 up, from x = 0 to 4000, and a section 2000 along it, level and then at 45°.
        let g = Geom { a: [0.0, 0.0, 500.0], b: [4000.0, 0.0, 500.0], width: 200.0, depth: 200.0, round: true };
        let (tris, caps) = g.tube();
        let tube = |cut| Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris: tris.clone(), caps, round: true, cut };
        let extent = |rings: &[Vec<P2>]| {
            let pts: Vec<P2> = rings.iter().flatten().copied().collect();
            let (lo, hi) = pts.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
                (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
            });
            (hi.0 - lo.0, hi.1 - lo.1, lo.0, lo.1)
        };
        let square = tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0);
        assert_eq!(square.len(), 1);
        let (w, h, x0, z0) = extent(&square);
        assert!(
            (w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6 && (x0 - 900.0).abs() < 1e-6 && (z0 - 400.0).abs() < 1e-6,
            "{w} {h} {x0} {z0}"
        );
        // A regular 24-gon in a circle of radius 100: its area, and counter-clockwise.
        let area = 12.0 * 100.0 * 100.0 * (std::f64::consts::TAU / 24.0).sin();
        assert!((geom::area(&square[0]) - area).abs() < 1e-6 * area, "{}", geom::area(&square[0]));
        // Slanting, its ring is longer along the line by 1 / sin 45°, as high as before.
        let slanted = tube(true).cuts(P2(1000.0, -1000.0), P2(1.0, 1.0).unit(), 3000.0);
        let (w, h, ..) = extent(&slanted);
        assert!((w - 200.0 * 2f64.sqrt()).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
        // Past the section's end it is clipped; clear of the pipe, or not allowed to be cut, it leaves nothing.
        let (w, ..) = extent(&tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 1050.0));
        assert!((w - 150.0).abs() < 1e-6, "{w}");
        // A line that starts inside the pipe cuts what is past its start only.
        let (w, _, x0, _) = extent(&tube(true).cuts(P2(2000.0, -50.0), P2(0.0, 1.0), 2000.0));
        assert!((w - 150.0).abs() < 1e-6 && x0.abs() < 1e-6, "{w} {x0}");
        assert!(tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 500.0).is_empty());
        assert!(tube(true).cuts(P2(5000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
        assert!(tube(false).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
        // A plane that lies in a face of a convex mesh (vertices exactly in it, no edge crossing it) gives that face.
        let corner = |x: f64, y: f64, z: f64| [x, y, z];
        let (a, b, c, d) = (corner(0.0, -100.0, 0.0), corner(200.0, -100.0, 0.0), corner(200.0, 100.0, 0.0), corner(0.0, 100.0, 0.0));
        let (e, f, g, h) =
            (corner(0.0, -100.0, 200.0), corner(200.0, -100.0, 200.0), corner(200.0, 100.0, 200.0), corner(0.0, 100.0, 200.0));
        let tris = vec![
            [a, c, b],
            [a, d, c],
            [e, f, g],
            [e, g, h],
            [a, b, f],
            [a, f, e],
            [b, c, g],
            [b, g, f],
            [c, d, h],
            [c, h, g],
            [d, a, e],
            [d, e, h],
        ];
        let cube = Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris, caps: 0, round: false, cut: true };
        let (w, h, ..) = extent(&cube.cuts(P2(0.0, 100.0), P2(1.0, 0.0), 1000.0));
        assert!((w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
        // A section along the pipe cuts it lengthwise: a strip as long as the pipe is and as high as it is round.
        let along = tube(true).cuts(P2(0.0, 0.0), P2(1.0, 0.0), 4000.0);
        let (w, h, ..) = extent(&along);
        assert!((w - 4000.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
    }

    #[test]
    fn a_lofted_mass_is_a_closed_skin_of_triangles_turned_out_that_encloses_its_exact_volume() {
        use serde_json::json;
        let mut s = crate::engine::Session::new();
        s.execute("level", &json!({"elevation": 6000, "name": "Level 3"})).unwrap();
        let (base, crown) =
            (json!([[0, 0], [10000, 0], [10000, 6000], [0, 6000]]), json!([[2000, 2000], [8000, 2000], [8000, 4000], [2000, 4000]]));
        let r = s.execute("mass", &json!({"boundary": base, "height": 6000, "top": crown})).unwrap();
        let id = r["id"].as_u64().unwrap_or_default();
        let mut tris = Vec::new();
        let solids = solids(&s.model);
        assert!(solids.iter().filter(|x| x.owner() == id).all(|x| matches!(x, Solid::Mesh { .. })));
        solids.iter().filter(|x| x.owner() == id).for_each(|x| x.tris(&mut tris));
        // Turned out, the sum of the tetrahedra to the origin is the volume: 6 m x (60 + 4 x 32 + 12) / 6 m².
        let volume: f64 = tris.iter().map(|t| dot(t.p[0], cross(t.p[1], t.p[2])) / 6.0).sum();
        assert!((volume - 200e9).abs() < 1.0, "{volume}");
        // Closed: each edge is walked once one way and once the other.
        let key = |p: P3| p.map(|x| (x * 1e3).round() as i64);
        let mut walked: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
        for t in &tris {
            for k in 0..3 {
                *walked.entry((key(t.p[k]), key(t.p[(k + 1) % 3]))).or_default() += 1;
            }
        }
        assert!(walked.iter().all(|((a, b), n)| *n == 1 && walked.get(&(*b, *a)) == Some(&1)), "an open or doubled edge");
        // Over its middle it stands 6 m; over a corner of its footprint, past the top outline, it is lower.
        assert_eq!(tops_at(&s.model, P2(5000.0, 3000.0)).get(&id), Some(&6000.0));
        let corner = tops_at(&s.model, P2(500.0, 500.0)).get(&id).copied().unwrap_or(f64::MAX);
        assert!(corner > 0.0 && corner < 6000.0, "{corner}");
        // A flat-sided loft is twelve triangles: two to each of four sides, and two to each cap.
        assert_eq!(tris.len(), 12);
    }

    #[test]
    fn a_twisted_loft_is_cut_into_strips_that_keep_its_volume_within_two_percent() {
        use serde_json::json;
        let mut s = crate::engine::Session::new();
        // 4 x 4 m at the base, turned 45° over 3 m: the exact volume is 3 m x (16 + 4 x 13.657 + 16) / 6 = 43.31 m³.
        let top = json!([[2000.0, -828.427], [4828.427, 2000.0], [2000.0, 4828.427], [-828.427, 2000.0]]);
        let r = s.execute("mass", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "height": 3000, "top": top})).unwrap();
        let id = r["id"].as_u64().unwrap_or_default();
        let mut tris = Vec::new();
        solids(&s.model).iter().filter(|x| x.owner() == id).for_each(|x| x.tris(&mut tris));
        let volume: f64 = tris.iter().map(|t| dot(t.p[0], cross(t.p[1], t.p[2])) / 6.0).sum();
        let exact = r["volume_m3"].as_f64().unwrap_or_default() * 1e9;
        assert!((exact - 43.31e9).abs() < 0.01e9, "{exact}");
        assert!(volume < exact && volume > exact * 0.98, "{volume} of {exact}");
        assert!(tris.len() > 12 * 4, "its sides are strips: {}", tris.len());
    }
}
