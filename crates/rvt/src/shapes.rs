//! A family type's shape: the solids of its graphics ([`crate::graphics`]) as triangles, in millimetres in the family's axes.
//!
//! A solid is an object of the class `Geometry`, a boundary representation: its faces (`m_pFaces`) and its edges (`m_pEdges`).
//! A face (`Face`) lies on a surface (`m_pSurf`) and is bounded by loops of edges in the surface's own parameters `u` and `v`: its
//! first loop (`m_pFirstLoop`) is its outline, and each loop names the next (`m_nextLoop`), its holes. A loop names its first
//! edge (`m_next`); an edge (`Edge`) names the two faces it bounds (`m_pFace`), and for each the next edge round that face's
//! loop (`m_next`), and holds its points (its first and last, `m_firstAndLastEdgePnts`, and those between,
//! `m_interiorEdgePnts`), each the point's `u` and `v` on either face (`uv`). The surfaces read, in feet in the family's axes:
//!
//! - `Plane`: `origin + u·xVec + v·yVec`;
//! - `CylSurf`: `center + r·cos(u)·xVec + r·sin(u)·yVec + v·zVec`;
//! - `SurfRev`: its profile curve (`m_pProfileCurve`) at `v`, in the surface's own axes, turned by `u` about its z, and then
//!   `center + X·xVec + Y·yVec + Z·zVec`; the profile is a `GArc` (`center + r·cos(t)·xVec + r·sin(t)·yVec`) or a `GLine`
//!   (`origin + t·dirVec`).
//!
//! The shape is read only where all of it reads, and it checks: every face of every solid on a surface above, every loop closed,
//! and both faces of every edge putting each of its points in the same place ([`MEET`]). A face is cut into triangles in its
//! surface's parameters, curved ones every fifteen degrees, by the core's constrained triangulation. The graphics of a type
//! may hold more than its solids (lines drawn in plan, the conditions on the views it shows in): only the solids are read, and
//! a type whose graphics hold a nested family or a mesh, which are not read, has no shape here.

use bimcraft_core::geom::P2;
use bimcraft_core::triangulation;

use crate::graphics::{Layouts, Obj, Tree, Value};

/// How far apart the two faces of an edge may put one of its points, feet (0.03 mm).
pub(crate) const MEET: f64 = 1e-4;
/// The most triangles a type's shape may have.
pub(crate) const MAX_TRIANGLES: usize = bimcraft_core::family::MAX_TRIANGLES;
/// The most edges a loop may have.
const MAX_LOOP: usize = 10_000;
/// The step round a curved surface, radians.
const STEP: f64 = std::f64::consts::PI / 12.0;
/// Feet to millimetres.
const MM: f64 = 304.8;
/// The scale the parameters of a face are triangulated at, units to the millimetre: the triangulation joins corners within a
/// millimetre of its units, and a family's detail is finer than that.
const FINE: f64 = 100.0;

/// The classes whose objects hold geometry this module does not read: a type whose graphics hold one has no shape here.
const UNREAD: [&str; 3] = ["GInstance", "GPolyMesh", "FacetedTopol"];

type P3 = [f64; 3];
type Step<T> = std::result::Result<T, String>;

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(k: f64, a: P3) -> P3 {
    [k * a[0], k * a[1], k * a[2]]
}

fn dist(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// A curve a surface of revolution turns.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Curve {
    Arc { center: P3, x: P3, y: P3, r: f64 },
    Line { origin: P3, dir: P3 },
}

impl Curve {
    fn at(&self, t: f64) -> P3 {
        match *self {
            Curve::Arc { center, x, y, r } => add(center, add(scale(r * t.cos(), x), scale(r * t.sin(), y))),
            Curve::Line { origin, dir } => add(origin, scale(t, dir)),
        }
    }
}

/// A surface a face lies on, feet.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Surface {
    Plane { origin: P3, x: P3, y: P3 },
    Cylinder { center: P3, x: P3, y: P3, z: P3, r: f64 },
    Revolved { center: P3, x: P3, y: P3, z: P3, profile: Curve },
}

impl Surface {
    fn at(&self, [u, v]: [f64; 2]) -> P3 {
        match *self {
            Surface::Plane { origin, x, y } => add(origin, add(scale(u, x), scale(v, y))),
            Surface::Cylinder { center, x, y, z, r } => add(center, add(scale(r * u.cos(), x), add(scale(r * u.sin(), y), scale(v, z)))),
            Surface::Revolved { center, x, y, z, profile } => {
                let [a, b, c] = profile.at(v);
                let (s, k) = u.sin_cos();
                add(center, add(scale(a * k - b * s, x), add(scale(a * s + b * k, y), scale(c, z))))
            }
        }
    }

    /// The steps a face on it is cut at, in `u` and `v`: none along a straight direction.
    fn steps(&self) -> [Option<f64>; 2] {
        match self {
            Surface::Plane { .. } => [None, None],
            Surface::Cylinder { .. } => [Some(STEP), None],
            Surface::Revolved { profile: Curve::Arc { .. }, .. } => [Some(STEP), Some(STEP)],
            Surface::Revolved { profile: Curve::Line { .. }, .. } => [Some(STEP), None],
        }
    }
}

/// What a type's shape reads from its tree.
struct Reader<'a, 'l, 's> {
    tree: &'a Tree,
    layouts: &'l Layouts<'s>,
}

impl<'a> Reader<'a, '_, '_> {
    fn name(&self, o: &Obj) -> &str {
        self.layouts.name(o.tag)
    }

    fn get(&self, o: &'a Obj, field: &str) -> Step<&'a Value> {
        self.layouts.get(o, field).ok_or_else(|| format!("a {} has no `{field}`", self.name(o)))
    }

    fn point(&self, o: &'a Obj, field: &str) -> Step<P3> {
        self.get(o, field)?.point().ok_or_else(|| format!("a {}'s `{field}` is no point", self.name(o)))
    }

    fn real(&self, o: &'a Obj, field: &str) -> Step<f64> {
        self.get(o, field)?.real().ok_or_else(|| format!("a {}'s `{field}` is no number", self.name(o)))
    }

    /// The object an owned field names: in the tree, or inline in `o`.
    fn owned(&self, o: &'a Obj, field: &str) -> Step<Option<&'a Obj>> {
        match self.get(o, field)? {
            Value::Null => Ok(None),
            Value::Owned(i, _) => self.tree.get(*i).map(Some).ok_or_else(|| format!("object {i} is not in the tree")),
            Value::Inline(k) => o.inline.get(*k).map(Some).ok_or_else(|| format!("a {}'s inline `{field}` is missing", self.name(o))),
            _ => Err(format!("a {}'s `{field}` is no object", self.name(o))),
        }
    }

    fn weak(&self, v: Option<&Value>) -> Step<u32> {
        v.and_then(Value::weak).ok_or_else(|| "a pointer is no index".to_string())
    }

    fn curve(&self, o: &'a Obj) -> Step<Curve> {
        Ok(match self.name(o) {
            "GArc" => Curve::Arc {
                center: self.point(o, "m_center")?,
                x: self.point(o, "m_xVec")?,
                y: self.point(o, "m_yVec")?,
                r: self.real(o, "m_radius")?,
            },
            "GLine" => Curve::Line { origin: self.point(o, "m_origin")?, dir: self.point(o, "m_dirVec")? },
            other => return Err(format!("a surface of revolution turns a {other}, which is not read")),
        })
    }

    fn surface(&self, face: &'a Obj) -> Step<Surface> {
        let s = self.owned(face, "m_pSurf")?.ok_or("a face has no surface")?;
        Ok(match self.name(s) {
            "Plane" => Surface::Plane { origin: self.point(s, "m_origin")?, x: self.point(s, "m_xVec")?, y: self.point(s, "m_yVec")? },
            "CylSurf" => Surface::Cylinder {
                center: self.point(s, "m_center")?,
                x: self.point(s, "m_xVec")?,
                y: self.point(s, "m_yVec")?,
                z: self.point(s, "m_zVec")?,
                r: self.real(s, "m_radius")?,
            },
            "SurfRev" => Surface::Revolved {
                center: self.point(s, "m_center")?,
                x: self.point(s, "m_xVec")?,
                y: self.point(s, "m_yVec")?,
                z: self.point(s, "m_zVec")?,
                profile: self.curve(self.owned(s, "m_pProfileCurve")?.ok_or("a surface of revolution has no profile")?)?,
            },
            other => return Err(format!("a face lies on a {other}, which is not read")),
        })
    }

    /// An edge's points on its side `side`, first to last.
    fn edge_points(&self, edge: &'a Obj, side: usize) -> Step<Vec<[f64; 2]>> {
        let ends = self.get(edge, "m_firstAndLastEdgePnts")?.list().ok_or("an edge's ends are no list")?;
        let inner = self.get(edge, "m_interiorEdgePnts")?.list().ok_or("an edge's points are no list")?;
        let (first, last) = match ends {
            [a, b] => (a, b),
            _ => return Err("an edge has not two ends".to_string()),
        };
        let mut out = Vec::new();
        for p in std::iter::once(first).chain(inner).chain(std::iter::once(last)) {
            let Value::Object(tag, values) = p else { return Err("an edge's point is no object".to_string()) };
            let uv = self.layouts.field(*tag, values, "uv").and_then(Value::list).ok_or("an edge's point has no `uv`")?;
            out.push(uv.get(side).and_then(Value::pair).ok_or("an edge's point has no `uv` on that side")?);
        }
        Ok(out)
    }

    /// The edge `e`'s side that bounds face `face`, and the index of the next edge round it.
    fn side(&self, edge: &'a Obj, face: u32) -> Step<(usize, u32)> {
        let faces = self.get(edge, "m_pFace")?.list().ok_or("an edge's faces are no list")?;
        let side = (0..2).find(|&k| faces.get(k).and_then(Value::weak) == Some(face)).ok_or("an edge of a loop does not bound its face")?;
        let next = self.get(edge, "m_next")?.list().ok_or("an edge's next are no list")?;
        Ok((side, self.weak(next.get(side))?))
    }

    /// A loop of face `face` in its surface's parameters: its edges' points chained end to end.
    fn loop_uv(&self, face: u32, lp: &'a Obj, index: u32) -> Step<Vec<[f64; 2]>> {
        let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut e = self.weak(Some(self.get(lp, "m_next")?))?;
        while e != index && e != 0 {
            if segments.len() >= MAX_LOOP {
                return Err(format!("a loop has more than {MAX_LOOP} edges"));
            }
            let edge = self.tree.get(e).ok_or_else(|| format!("edge {e} is not in the tree"))?;
            let (side, next) = self.side(edge, face)?;
            segments.push(self.edge_points(edge, side)?);
            e = next;
        }
        Ok(chain(segments))
    }

    /// A face's loops: its outline, then its holes.
    fn loops(&self, face: &'a Obj, index: u32) -> Step<Vec<Vec<[f64; 2]>>> {
        let mut out = Vec::new();
        let mut at = match self.get(face, "m_pFirstLoop")? {
            Value::Owned(i, _) => Some(*i),
            Value::Null => None,
            _ => return Err("a face's loop is not in the tree".to_string()),
        };
        while let Some(i) = at {
            if out.len() >= MAX_LOOP {
                return Err(format!("a face has more than {MAX_LOOP} loops"));
            }
            let lp = self.tree.get(i).ok_or_else(|| format!("loop {i} is not in the tree"))?;
            out.push(self.loop_uv(index, lp, i)?);
            at = match self.get(lp, "m_nextLoop")? {
                Value::Owned(i, _) => Some(*i),
                Value::Null => None,
                _ => return Err("a loop's next is not in the tree".to_string()),
            };
        }
        Ok(out)
    }
}

/// Whether two points of a surface's parameters are one.
fn same(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7
}

/// Edges' points chained end to end, each turned round where that joins it to the one before.
fn chain(segments: Vec<Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    let next_first = segments.get(1).and_then(|s| s.first().copied());
    let next_last = segments.get(1).and_then(|s| s.last().copied());
    for mut seg in segments {
        match out.last().copied() {
            None => {
                if let Some(first) = seg.first().copied()
                    && (next_first.is_some_and(|p| same(first, p)) || next_last.is_some_and(|p| same(first, p)))
                {
                    seg.reverse();
                }
                out.extend(seg);
            }
            Some(end) => {
                if seg.last().is_some_and(|p| same(*p, end)) && !seg.first().is_some_and(|p| same(*p, end)) {
                    seg.reverse();
                }
                let skip = usize::from(seg.first().is_some_and(|p| same(*p, end)));
                out.extend(seg.into_iter().skip(skip));
            }
        }
    }
    if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| same(*a, *b)) {
        out.pop();
    }
    out
}

/// Whether point `p` lies inside polygon `poly` (even-odd).
fn inside(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
    }
    c
}

/// The distance from `p` to the segment `a`–`b`.
fn to_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// A loop with points put in along each side so that no step is longer than `steps` says.
fn densify(lp: &[[f64; 2]], steps: [Option<f64>; 2]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let n = lp.len();
    for i in 0..n {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        let cuts = (0..2).filter_map(|k| steps[k].map(|s| ((b[k] - a[k]).abs() / s).ceil())).fold(1.0_f64, f64::max).min(1_000.0) as usize;
        for c in 0..cuts.max(1) {
            let t = c as f64 / cuts.max(1) as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// A face's triangles, millimetres. Its loops may bound several regions: a loop inside an even number of the others is an
/// outline, and a loop inside it and one more is one of its holes. A loop is inside another where most of its corners are,
/// since two loops may touch.
pub(crate) fn triangles(surface: &Surface, loops: &[Vec<[f64; 2]>], out: &mut Vec<[P3; 3]>) -> Step<()> {
    let steps = surface.steps();
    let loops: Vec<Vec<[f64; 2]>> = loops.iter().map(|l| densify(l, steps)).filter(|l| l.len() >= 3).collect();
    let within = |i: usize, j: usize| i != j && loops[i].iter().filter(|p| inside(**p, &loops[j])).count() * 2 > loops[i].len();
    let depth: Vec<usize> = (0..loops.len()).map(|i| (0..loops.len()).filter(|&j| within(i, j)).count()).collect();
    for (i, outline) in loops.iter().enumerate().filter(|(i, _)| depth[*i].is_multiple_of(2)) {
        let holes: Vec<&Vec<[f64; 2]>> =
            (0..loops.len()).filter(|&j| depth[j] == depth[i] + 1 && within(j, i)).map(|j| &loops[j]).collect();
        region(surface, steps, outline, &holes, out)?;
    }
    Ok(())
}

/// The triangles of one region of a face, its outline and holes cut in the surface's parameters, scaled so a unit is about
/// `1 / FINE` mm on the surface.
fn region(surface: &Surface, steps: [Option<f64>; 2], outline: &[[f64; 2]], holes: &[&Vec<[f64; 2]>], out: &mut Vec<[P3; 3]>) -> Step<()> {
    // The scale of each parameter: how far a step in it goes on the surface, at the middle of the outline.
    let n = outline.len() as f64;
    let mid = [outline.iter().map(|p| p[0]).sum::<f64>() / n, outline.iter().map(|p| p[1]).sum::<f64>() / n];
    let h = 1e-4;
    let speed = |k: usize| {
        let mut p = mid;
        p[k] += h;
        (dist(surface.at(p), surface.at(mid)) / h * MM * FINE).max(1.0)
    };
    let (su, sv) = (speed(0), speed(1));
    let flat = |p: &[f64; 2]| P2(p[0] * su, p[1] * sv);
    // Points inside a face curved both ways, on the grid of its steps, clear of its loops.
    let mut points = Vec::new();
    if let [Some(du), Some(dv)] = steps {
        let (u0, u1) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[0]), b.max(p[0])));
        let (v0, v1) = outline.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p[1]), b.max(p[1])));
        let (nu, nv) = (((u1 - u0) / du).ceil().min(200.0) as usize, ((v1 - v0) / dv).ceil().min(200.0) as usize);
        let clear = 3.0 / su.min(sv);
        let near = |q: [f64; 2], l: &[[f64; 2]]| (0..l.len()).any(|k| to_segment(q, l[k], l[(k + 1) % l.len()]) < clear);
        for i in 1..nu {
            for j in 1..nv {
                let q = [u0 + (u1 - u0) * i as f64 / nu as f64, v0 + (v1 - v0) * j as f64 / nv as f64];
                if inside(q, outline) && !holes.iter().any(|h| inside(q, h) || near(q, h)) && !near(q, outline) {
                    points.push(flat(&q));
                }
            }
        }
    }
    let t = triangulation::constrained(
        &outline.iter().map(flat).collect::<Vec<_>>(),
        &holes.iter().map(|h| h.iter().map(flat).collect()).collect::<Vec<_>>(),
        &points,
        &[],
    )?;
    let corner = |k: usize| t.points.get(k).map(|p| scale(MM, surface.at([p.0 / su, p.1 / sv])));
    for [a, b, c] in &t.triangles {
        let (Some(a), Some(b), Some(c)) = (corner(*a), corner(*b), corner(*c)) else { continue };
        if out.len() >= MAX_TRIANGLES {
            return Err(format!("it has more than {MAX_TRIANGLES} triangles"));
        }
        out.push([a, b, c]);
    }
    Ok(())
}

/// A type's shape: the triangles of its solids, millimetres in the family's axes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Shape {
    pub triangles: Vec<[P3; 3]>,
    /// The solids and faces read.
    pub solids: usize,
    pub faces: usize,
    /// The box its graphics give round all they draw (`m_bBox`: its solids, and the lines and controls drawn with them), feet in
    /// the family's axes.
    pub whole: [f64; 6],
}

/// How far a side of the box round a shape's triangles may lie from the box its graphics give round its solids, feet: a curved
/// face is drawn as flat facets inside it, a fifteen-degree step taking up to 0.9 % of its radius.
pub(crate) fn slack(size: f64) -> f64 {
    (1.0 / 304.8_f64).max(size * 0.01)
}

/// A box its graphics give, two points of three numbers: corners low and high, feet.
fn bounds(v: Option<&Value>) -> Option<[f64; 6]> {
    match v?.list()? {
        [lo, hi] => {
            let (lo, hi) = (lo.point()?, hi.point()?);
            Some([lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]])
        }
        _ => None,
    }
}

/// The shape of a type whose tree is `tree`; or why it has none.
pub(crate) fn shape(tree: &Tree, layouts: &Layouts) -> Step<Shape> {
    let r = Reader { tree, layouts };
    let root = tree.objects.values().next().ok_or("the tree is empty")?;
    let whole = bounds(layouts.get(root, "m_bBox")).ok_or("its graphics give no box")?;
    let tight = bounds(layouts.get(root, "m_tightbBox")).ok_or("its graphics give no box round their solids")?;
    if let Some(o) = tree.objects.values().find(|o| UNREAD.contains(&r.name(o))) {
        return Err(format!("its graphics hold a {}, which is not read", r.name(o)));
    }
    let mut out = Shape { whole, ..Shape::default() };
    for solid in tree.objects.values().filter(|o| r.name(o) == "Geometry") {
        out.solids += 1;
        let faces: Vec<u32> = match r.get(solid, "m_pFaces")?.list() {
            Some(l) => l.iter().filter_map(|v| if let Value::Owned(i, _) = v { Some(*i) } else { None }).collect(),
            None => return Err("a solid's faces are no list".to_string()),
        };
        let mut surfaces = std::collections::BTreeMap::new();
        for &i in &faces {
            let face = tree.get(i).ok_or_else(|| format!("face {i} is not in the tree"))?;
            surfaces.insert(i, r.surface(face)?);
        }
        // Both faces of each edge must put its points in the same place.
        let edges = r.get(solid, "m_pEdges")?.list().ok_or("a solid's edges are no list")?;
        for v in edges {
            let Value::Owned(e, _) = v else { continue };
            let edge = tree.get(*e).ok_or_else(|| format!("edge {e} is not in the tree"))?;
            let sides = r.get(edge, "m_pFace")?.list().ok_or("an edge's faces are no list")?;
            let on: Vec<Surface> =
                (0..2).filter_map(|k| sides.get(k).and_then(Value::weak)).filter_map(|f| surfaces.get(&f).copied()).collect();
            if let [a, b] = on[..] {
                for (p, q) in r.edge_points(edge, 0)?.into_iter().zip(r.edge_points(edge, 1)?) {
                    if dist(a.at(p), b.at(q)) > MEET {
                        return Err("two faces put a point of the edge between them in different places".to_string());
                    }
                }
            }
        }
        for &i in &faces {
            let face = tree.get(i).ok_or_else(|| format!("face {i} is not in the tree"))?;
            let surface = surfaces.get(&i).ok_or("a face has no surface")?;
            let loops = r.loops(face, i)?;
            triangles(surface, &loops, &mut out.triangles).map_err(|why| format!("a face is not cut into triangles: {why}"))?;
            out.faces += 1;
        }
    }
    if out.triangles.is_empty() {
        return Err("its graphics hold no solid".to_string());
    }
    // The solids read must fill the box the graphics give round their solids: a solid left unread would leave it short.
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in out.triangles.iter().flatten() {
        for k in 0..3 {
            b[k] = b[k].min(p[k] / MM);
            b[k + 3] = b[k + 3].max(p[k] / MM);
        }
    }
    let fills = (0..3).all(|k| {
        let s = slack(tight[k + 3] - tight[k]);
        (b[k] - tight[k]).abs() <= s && (b[k + 3] - tight[k + 3]).abs() <= s
    });
    if !fills {
        return Err("its solids read are not the box its graphics give round them".to_string());
    }
    Ok(out)
}
