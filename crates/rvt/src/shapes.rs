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
//!   `center + X·xVec + Y·yVec + Z·zVec`; the profile is a `GArc` (`center + r·cos(t)·xVec + r·sin(t)·yVec`), a `GEllipse`
//!   (`center + xLen·cos(t)·xVec + yLen·sin(t)·yVec`), a `GLine` (`origin + t·dirVec`) or a `GHermiteSpline` (its nodes, each
//!   `SplineNode`'s parameter, point and tangent; between two, the cubic Hermite of their points and tangents, the tangents scaled
//!   by the span between their parameters);
//! - `RuledSurf`: straight across from its first end to its second, `(1 − v)·A(u) + v·B(u)`, where each end is a profile curve
//!   above at `u` of the way between its `m_endParams` (`m_pProfileCurve1`, `m_pProfileCurve2`), or where it names none, a point
//!   (`m_Point1`, `m_Point2`);
//! - `ConeSurf`: `center + v·(sin(h)·(cos(u)·xVec + sin(u)·yVec) + cos(h)·zVec)`, `h` its `m_halfAngle`, `center` its apex;
//! - `HermiteSurf`: in each cell of its grid of parameters (`m_uParams`, `m_vParams`), the bicubic Hermite of its corner nodes'
//!   points, derivatives along `u` and `v` and mixed ones (`SplineSrfNode`; `u` runs fastest in `m_NodeArray`).
//!
//! The shape is read only where all of it reads, and it checks: every face of every solid on a surface above, every loop closed,
//! and both faces of every edge putting each of its points in the same place ([`MEET`]). A face is cut into triangles in its
//! surface's parameters, curved ones every fifteen degrees (a spline every third of a span between nodes, a Hermite surface every
//! half cell), by the core's constrained triangulation. The graphics of a type
//! may hold more than its solids (lines drawn in plan, the conditions on the views it shows in): only the solids are read, and
//! a family nested in the type's (a `GInstance`) is drawn in by its own type's shape, placed by its transform ([`nest`]).

use bimcraft_core::geom::P2;
use bimcraft_core::triangulation;

use crate::graphics::{Layouts, MAX_OBJECTS, Obj, Tree, Value};

/// How far apart the two faces of an edge may put one of its points, feet (0.03 mm).
pub(crate) const MEET: f64 = 1e-4;
/// The most triangles a type's shape may have.
pub(crate) const MAX_TRIANGLES: usize = bimcraft_core::family::MAX_TRIANGLES;
/// The most edges a loop may have.
const MAX_LOOP: usize = 10_000;
/// The most edges the loops of all a tree's faces may walk together: each edge bounds two faces, so a tree that holds at most
/// [`MAX_OBJECTS`] objects walks fewer, and a hostile one that sends loops round the same edges again and again is refused.
const MAX_WALK: usize = 2 * MAX_OBJECTS;
/// The step round a curved surface, radians.
const STEP: f64 = std::f64::consts::PI / 12.0;
/// Feet to millimetres.
const MM: f64 = 304.8;
/// How many times finer than its faces a curved surface cuts an edge it shares with a flat face ([`Surface::fine`]).
const CARRY: f64 = 2.0;
/// The scale the parameters of a face are triangulated at, units to the millimetre: the triangulation joins corners within a
/// millimetre of its units, and a family's detail is finer than that.
const FINE: f64 = 100.0;

use std::collections::BTreeSet;

type P3 = [f64; 3];
type Step<T> = std::result::Result<T, String>;

fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: P3, b: P3) -> P3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn scale(k: f64, a: P3) -> P3 {
    [k * a[0], k * a[1], k * a[2]]
}

fn dist(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The most nodes a spline may have.
const MAX_NODES: usize = 10_000;

/// A node of a cubic Hermite spline: its parameter, its point and the curve's derivative there by its parameter.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    pub t: f64,
    pub at: P3,
    pub tangent: P3,
}

/// A curve a surface of revolution turns, or a ruled surface runs along.
#[derive(Clone, Debug)]
pub(crate) enum Curve {
    Arc {
        center: P3,
        x: P3,
        y: P3,
        r: f64,
    },
    Line {
        origin: P3,
        dir: P3,
    },
    /// Half its length along `x` is `rx`, along `y` `ry`.
    Ellipse {
        center: P3,
        x: P3,
        y: P3,
        rx: f64,
        ry: f64,
    },
    /// A cubic Hermite spline through its nodes, their parameters rising: between two, the cubic with their points and tangents.
    Spline(std::sync::Arc<[Node]>),
}

impl Curve {
    fn at(&self, t: f64) -> P3 {
        match self {
            &Curve::Arc { center, x, y, r } => add(center, add(scale(r * t.cos(), x), scale(r * t.sin(), y))),
            &Curve::Line { origin, dir } => add(origin, scale(t, dir)),
            &Curve::Ellipse { center, x, y, rx, ry } => add(center, add(scale(rx * t.cos(), x), scale(ry * t.sin(), y))),
            Curve::Spline(nodes) => {
                // The span `t` lies in, the first or the last where it lies before or past them all.
                let last = nodes.len().saturating_sub(2);
                let k = nodes.partition_point(|n| n.t <= t).saturating_sub(1).min(last);
                let (Some(a), Some(b)) = (nodes.get(k), nodes.get(k + 1)) else { return nodes.first().map_or([0.0; 3], |n| n.at) };
                let d = b.t - a.t;
                let ([pa, pb], [ta, tb]) = hermite((t - a.t) / d);
                add(add(scale(pa, a.at), scale(ta * d, a.tangent)), add(scale(pb, b.at), scale(tb * d, b.tangent)))
            }
        }
    }

    /// The step along it a face is cut at, in its parameter: none along a straight line, a third of a spline's span between nodes.
    fn step(&self) -> Option<f64> {
        match self {
            Curve::Arc { .. } | Curve::Ellipse { .. } => Some(STEP),
            Curve::Line { .. } => None,
            Curve::Spline(nodes) => {
                let (first, last) = (nodes.first()?, nodes.last()?);
                Some((last.t - first.t) / (3.0 * (nodes.len() - 1) as f64))
            }
        }
    }
}

/// An end of a ruled surface: a curve between the parameters its ends are at, or a point.
#[derive(Clone, Debug)]
pub(crate) enum End {
    Curve(Curve, [f64; 2]),
    Point(P3),
}

impl End {
    /// The point a part `u` of the way along it (0 at its first end, 1 at its last).
    fn at(&self, u: f64) -> P3 {
        match self {
            End::Curve(c, [t0, t1]) => c.at(t0 + u * (t1 - t0)),
            End::Point(p) => *p,
        }
    }

    /// The step along it a face is cut at, as a part of its length: none along a straight line or at a point.
    fn step(&self) -> Option<f64> {
        match self {
            End::Curve(c, [t0, t1]) if (t1 - t0).abs() > 0.0 => c.step().map(|s| s / (t1 - t0).abs()),
            _ => None,
        }
    }
}

/// A surface a face lies on, feet.
#[derive(Clone, Debug)]
pub(crate) enum Surface {
    Plane {
        origin: P3,
        x: P3,
        y: P3,
    },
    Cylinder {
        center: P3,
        x: P3,
        y: P3,
        z: P3,
        r: f64,
    },
    Revolved {
        center: P3,
        x: P3,
        y: P3,
        z: P3,
        profile: Curve,
    },
    /// Straight lines across from one end to the other: at `(u, v)`, a part `v` of the way from the first end's point a part `u` along
    /// it to the second's.
    Ruled {
        ends: [End; 2],
    },
    /// A cone from its apex `center` round its axis `z`, its side `half_angle` from it: `v` down the side, `u` round from `x`.
    Cone {
        center: P3,
        x: P3,
        y: P3,
        z: P3,
        half_angle: f64,
    },
    /// A bicubic Hermite surface through a grid of nodes at the parameters `u` and `v` (each rising), `u` running fastest in
    /// `nodes`: in each cell, the bicubic with its corners' points and derivatives.
    Hermite {
        u: std::sync::Arc<[f64]>,
        v: std::sync::Arc<[f64]>,
        nodes: std::sync::Arc<[Patch]>,
    },
}

/// A node of a Hermite surface: its point, the surface's derivatives there along `u` and `v`, and its mixed one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Patch {
    pub at: P3,
    pub du: P3,
    pub dv: P3,
    pub twist: P3,
}

/// The cubic Hermite weights at `s` (0 to 1 across a span): of the two ends' values, and of their derivatives.
fn hermite(s: f64) -> ([f64; 2], [f64; 2]) {
    let (s2, s3) = (s * s, s * s * s);
    ([2.0 * s3 - 3.0 * s2 + 1.0, 3.0 * s2 - 2.0 * s3], [s3 - 2.0 * s2 + s, s3 - s2])
}

/// The span of rising parameters `ps` that `t` lies in (the first or the last where it lies before or past them all): its first
/// index, how far across it `t` is (0 to 1 within it), and its length.
fn span(ps: &[f64], t: f64) -> Option<(usize, f64, f64)> {
    let k = ps.partition_point(|p| *p <= t).saturating_sub(1).min(ps.len().checked_sub(2)?);
    let (a, b) = (ps.get(k)?, ps.get(k + 1)?);
    Some((k, (t - a) / (b - a), b - a))
}

impl Surface {
    /// Its point at `(u, v)`, feet.
    pub(crate) fn at(&self, [u, v]: [f64; 2]) -> P3 {
        match self {
            &Surface::Plane { origin, x, y } => add(origin, add(scale(u, x), scale(v, y))),
            &Surface::Cylinder { center, x, y, z, r } => add(center, add(scale(r * u.cos(), x), add(scale(r * u.sin(), y), scale(v, z)))),
            &Surface::Revolved { center, x, y, z, ref profile } => {
                let [a, b, c] = profile.at(v);
                let (s, k) = u.sin_cos();
                add(center, add(scale(a * k - b * s, x), add(scale(a * s + b * k, y), scale(c, z))))
            }
            &Surface::Cone { center, x, y, z, half_angle } => {
                let (s, k) = half_angle.sin_cos();
                add(center, scale(v, add(scale(s * u.cos(), x), add(scale(s * u.sin(), y), scale(k, z)))))
            }
            Surface::Hermite { u: us, v: vs, nodes } => {
                let (Some((i, s, du)), Some((j, w, dv))) = (span(us, u), span(vs, v)) else { return [0.0; 3] };
                let ((pu, tu), (pv, tv)) = (hermite(s), hermite(w));
                let mut at = [0.0; 3];
                for (a, b) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let Some(n) = nodes.get((j + b) * us.len() + i + a) else { return [0.0; 3] };
                    let terms =
                        [(pu[a] * pv[b], n.at), (tu[a] * du * pv[b], n.du), (pu[a] * tv[b] * dv, n.dv), (tu[a] * du * tv[b] * dv, n.twist)];
                    for (k, p) in terms {
                        at = add(at, scale(k, p));
                    }
                }
                at
            }
            Surface::Ruled { ends: [a, b] } => {
                let (p, q) = (a.at(u), b.at(u));
                add(p, scale(v, sub(q, p)))
            }
        }
    }

    /// The steps an edge between it and a flat face is cut at, in `u` and `v`: finer than [`Surface::steps`], so a thin face
    /// between two curves (a lamp's rim) keeps clear of itself.
    pub(crate) fn fine(&self) -> [Option<f64>; 2] {
        self.steps().map(|s| s.map(|s| s / CARRY))
    }

    /// The steps a face on it is cut at, in `u` and `v`: none along a straight direction.
    fn steps(&self) -> [Option<f64>; 2] {
        match self {
            Surface::Plane { .. } => [None, None],
            Surface::Cylinder { .. } | Surface::Cone { .. } => [Some(STEP), None],
            // Half a cell's span on average each way.
            Surface::Hermite { u, v, .. } => {
                let half = |ps: &[f64]| Some((ps.last()? - ps.first()?) / (2.0 * (ps.len() - 1) as f64));
                [half(u), half(v)]
            }
            Surface::Revolved { profile, .. } => [Some(STEP), profile.step()],
            Surface::Ruled { ends: [a, b] } => {
                let step = match (a.step(), b.step()) {
                    (Some(p), Some(q)) => Some(p.min(q)),
                    (p, q) => p.or(q),
                };
                [step, None]
            }
        }
    }
}

/// What a type's shape reads from its tree.
struct Reader<'a, 'l, 's> {
    tree: &'a Tree,
    layouts: &'l Layouts<'s>,
    /// The edges the loops read so far have walked ([`MAX_WALK`]).
    walked: std::cell::Cell<usize>,
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
            "GEllipse" => Curve::Ellipse {
                center: self.point(o, "m_center")?,
                x: self.point(o, "m_xVec")?,
                y: self.point(o, "m_yVec")?,
                rx: self.real(o, "m_xLen")?,
                ry: self.real(o, "m_yLen")?,
            },
            "GHermiteSpline" => self.spline(o)?,
            other => return Err(format!("a surface's profile is a {other}, which is not read")),
        })
    }

    /// A bicubic Hermite surface: its nodes (`SplineSrfNode`: point, the two derivatives, the mixed one), one for each pair of its
    /// parameters (`m_uParams`, `m_vParams`, each at least two and rising), `u` running fastest. One not built, or one that closes
    /// on itself either way, is not read.
    fn hermite_surface(&self, s: &'a Obj) -> Step<Surface> {
        let periodic = self.get(s, "m_periodic")?.list().ok_or("a Hermite surface's periodic flags are no list")?;
        if !periodic.iter().all(|p| matches!(p, Value::Int(0))) {
            return Err("a Hermite surface closes on itself (periodic), which is not read".to_string());
        }
        if !matches!(self.get(s, "m_constructedOK")?, Value::Int(1)) {
            return Err("a Hermite surface is marked not built".to_string());
        }
        let params = |field: &str| -> Step<Vec<f64>> {
            let list = self.get(s, field)?.list().ok_or("a Hermite surface's parameters are no list")?;
            let ps: Vec<f64> = list.iter().map(Value::real).collect::<Option<_>>().ok_or("a Hermite surface's parameter is no number")?;
            if ps.len() < 2 || ps.len() > MAX_NODES || ps.windows(2).any(|w| w[1] <= w[0]) {
                return Err("a Hermite surface's parameters are not 2 or more rising".to_string());
            }
            Ok(ps)
        };
        let (u, v) = (params("m_uParams")?, params("m_vParams")?);
        let list = self.get(s, "m_NodeArray")?.list().ok_or("a Hermite surface's nodes are no list")?;
        if u.len().checked_mul(v.len()) != Some(list.len()) || list.len() > MAX_NODES {
            return Err(format!("a Hermite surface's {} nodes are not its grid of {} by {}", list.len(), u.len(), v.len()));
        }
        let mut nodes = Vec::with_capacity(list.len());
        for n in list {
            let Value::Object(tag, values) = n else { return Err("a Hermite surface's node is no object".to_string()) };
            let field = |name: &str| self.layouts.field(*tag, values, name);
            let tangents = field("m_iTangent").and_then(Value::list).ok_or("a Hermite surface's node has no derivatives")?;
            let (Some(du), Some(dv)) = (tangents.first().and_then(Value::point), tangents.get(1).and_then(Value::point)) else {
                return Err("a Hermite surface's node has not two derivatives".to_string());
            };
            nodes.push(Patch {
                at: field("m_iPoint").and_then(Value::point).ok_or("a Hermite surface's node has no point")?,
                du,
                dv,
                twist: field("m_iMixedDer").and_then(Value::point).ok_or("a Hermite surface's node has no mixed derivative")?,
            });
        }
        Ok(Surface::Hermite { u: u.into(), v: v.into(), nodes: nodes.into() })
    }

    /// A cubic Hermite spline: its nodes (`SplineNode`: parameter, point, tangent), at least two and their parameters rising; one
    /// that closes on itself is not read.
    fn spline(&self, o: &'a Obj) -> Step<Curve> {
        if !matches!(self.get(o, "m_Periodic")?, Value::Int(0)) {
            return Err("a spline closes on itself (periodic), which is not read".to_string());
        }
        let list = self.get(o, "m_NodeArray")?.list().ok_or("a spline's nodes are no list")?;
        if list.len() < 2 || list.len() > MAX_NODES {
            return Err(format!("a spline has {} nodes, not 2 to {MAX_NODES}", list.len()));
        }
        let mut nodes: Vec<Node> = Vec::with_capacity(list.len());
        for v in list {
            let Value::Object(tag, values) = v else { return Err("a spline's node is no object".to_string()) };
            let field = |name: &str| self.layouts.field(*tag, values, name);
            let node = Node {
                t: field("m_iParametr").and_then(Value::real).ok_or("a spline's node has no parameter")?,
                at: field("m_iPoint").and_then(Value::point).ok_or("a spline's node has no point")?,
                tangent: field("m_iTangent").and_then(Value::point).ok_or("a spline's node has no tangent")?,
            };
            if nodes.last().is_some_and(|n| node.t <= n.t) {
                return Err("a spline's nodes do not go on in their parameter".to_string());
            }
            nodes.push(node);
        }
        Ok(Curve::Spline(nodes.into()))
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
            "RuledSurf" => {
                Surface::Ruled { ends: [self.end(s, "m_pProfileCurve1", "m_Point1")?, self.end(s, "m_pProfileCurve2", "m_Point2")?] }
            }
            "HermiteSurf" => self.hermite_surface(s)?,
            "ConeSurf" => Surface::Cone {
                center: self.point(s, "m_center")?,
                x: self.point(s, "m_xVec")?,
                y: self.point(s, "m_yVec")?,
                z: self.point(s, "m_zVec")?,
                half_angle: self.real(s, "m_halfAngle")?,
            },
            other => return Err(format!("a face lies on a {other}, which is not read")),
        })
    }

    /// An end of ruled surface `s`: the curve its field `curve` names, between its end parameters, or where it names none, the point
    /// its field `point` gives.
    fn end(&self, s: &'a Obj, curve: &str, point: &str) -> Step<End> {
        Ok(match self.owned(s, curve)? {
            Some(c) => End::Curve(self.curve(c)?, self.get(c, "m_endParams")?.pair().ok_or("a curve's ends are no pair")?),
            None => End::Point(self.point(s, point)?),
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

    /// A loop of face `face` in its surface's parameters: its edges' points chained end to end; where `carry`, a flat face's taking
    /// those of an edge its curved neighbour puts on its curve ([`along`]), and a curved face's cut as finely on such an edge.
    /// `surfaces` are the faces' of its solid.
    fn loop_uv(&self, face: u32, lp: &'a Obj, index: u32, surfaces: &Surfaces, carry: bool) -> Step<Vec<[f64; 2]>> {
        let own = surfaces.get(&face).ok_or("a face has no surface")?;
        let mut segments: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut e = self.weak(Some(self.get(lp, "m_next")?))?;
        while e != index && e != 0 {
            if segments.len() >= MAX_LOOP {
                return Err(format!("a loop has more than {MAX_LOOP} edges"));
            }
            self.walked.set(self.walked.get() + 1);
            if self.walked.get() > MAX_WALK {
                return Err(format!("its loops walk more than {MAX_WALK} edges"));
            }
            let edge = self.tree.get(e).ok_or_else(|| format!("edge {e} is not in the tree"))?;
            let (side, next) = self.side(edge, face)?;
            let mine = self.edge_points(edge, side)?;
            let neighbour = self.get(edge, "m_pFace")?.list().and_then(|f| f.get(1 - side)).and_then(Value::weak);
            // An edge between a flat face and a curved one follows the curve on both, at the same points.
            let carried = match neighbour.and_then(|n| surfaces.get(&n)).filter(|_| carry) {
                Some(other @ Surface::Plane { .. }) if !matches!(own, Surface::Plane { .. }) => {
                    let fine = split(&mine, own.fine()).into_iter().map(|(p, _)| p).collect();
                    (other.fine() == [None, None]).then_some(fine)
                }
                Some(other) if matches!(own, Surface::Plane { .. }) => along(own, other, &mine, &self.edge_points(edge, 1 - side)?),
                _ => None,
            };
            segments.push(carried.unwrap_or(mine));
            e = next;
        }
        Ok(chain(segments, own))
    }

    /// A face's loops: its outline, then its holes. `surfaces` are the faces' of its solid; `carry` as [`Reader::loop_uv`].
    fn loops(&self, face: &'a Obj, index: u32, surfaces: &Surfaces, carry: bool) -> Step<Vec<Vec<[f64; 2]>>> {
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
            out.push(self.loop_uv(index, lp, i, surfaces, carry)?);
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

/// The surfaces of a solid's faces, by the faces' indices.
type Surfaces = std::collections::BTreeMap<u32, Surface>;

/// Points `ps` of a surface's parameters with points put in between each two, so no step is longer than `steps` says (at most a
/// thousand to a span); each with the index of the point of `ps` it is, where it is one.
pub(crate) fn split(ps: &[[f64; 2]], steps: [Option<f64>; 2]) -> Vec<([f64; 2], Option<usize>)> {
    let mut out = Vec::new();
    for (k, p) in ps.iter().enumerate() {
        if let Some(q) = ps.get(k + 1) {
            out.push((*p, Some(k)));
            let cuts =
                (0..2).filter_map(|i| steps[i].map(|s| ((q[i] - p[i]).abs() / s).ceil())).fold(1.0_f64, f64::max).min(1_000.0) as usize;
            for j in 1..cuts {
                let t = j as f64 / cuts as f64;
                out.push(([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t], None));
            }
        } else {
            out.push((*p, Some(k)));
        }
    }
    out
}

/// The points of an edge between flat face `own` and its curved neighbour `other`, in `own`'s parameters: the edge's own points
/// (`mine` on `own`, `theirs` on `other`), and between each two those `other` puts it at, cut along its parameters as it cuts its
/// own side of the edge ([`Surface::fine`]), so the edge follows the curve `other` puts it on rather than the straight lines
/// between its points (a circle's, a lamp's rim), and both faces meet at the same points. None where `own` is curved, `other` is
/// flat, or a point so put does not lie on `own`.
pub(crate) fn along(own: &Surface, other: &Surface, mine: &[[f64; 2]], theirs: &[[f64; 2]]) -> Option<Vec<[f64; 2]>> {
    let Surface::Plane { origin, x, y } = *own else { return None };
    let steps = other.fine();
    if steps == [None, None] || mine.len() != theirs.len() || mine.len() < 2 {
        return None;
    }
    // A point of the plane's parameters, where the point lies on it.
    let (a, b, c) = (dot(x, x), dot(x, y), dot(y, y));
    let det = a * c - b * b;
    if det.abs() < 1e-12 {
        return None;
    }
    let on_plane = |p: P3| -> Option<[f64; 2]> {
        let d = sub(p, origin);
        let (e, f) = (dot(d, x), dot(d, y));
        let uv = [(e * c - f * b) / det, (a * f - b * e) / det];
        (dist(own.at(uv), p) <= MEET).then_some(uv)
    };
    split(theirs, steps).into_iter().map(|(q, k)| k.and_then(|k| mine.get(k).copied()).or_else(|| on_plane(other.at(q)))).collect()
}

/// Edges' points chained end to end, each turned round where that joins it to the one before: by their parameters on `surface`, or
/// where those differ, by the points they are on it (two edges meeting at a pole, or across a seam).
pub(crate) fn chain(segments: Vec<Vec<[f64; 2]>>, surface: &Surface) -> Vec<[f64; 2]> {
    let same = |a: [f64; 2], b: [f64; 2]| same(a, b) || dist(surface.at(a), surface.at(b)) <= MEET;
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
                // A point the edge before ends at is not put in again; one it meets only on the surface is, so the loop runs
                // along the pole or the seam between them in the parameters.
                let skip = usize::from(seg.first().is_some_and(|p| self::same(*p, end)));
                out.extend(seg.into_iter().skip(skip));
            }
        }
    }
    if out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| self::same(*a, *b)) {
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

/// A loop with points put in along each side so that no step is longer than `steps` says; none where that takes more than `room`
/// points (which it uses up) in all.
fn densify(lp: &[[f64; 2]], steps: [Option<f64>; 2], room: &mut usize) -> Option<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    let n = lp.len();
    for i in 0..n {
        let (a, b) = (lp[i], lp[(i + 1) % n]);
        let cuts = (0..2).filter_map(|k| steps[k].map(|s| ((b[k] - a[k]).abs() / s).ceil())).fold(1.0_f64, f64::max).min(1_000.0) as usize;
        let cuts = cuts.max(1);
        *room = room.checked_sub(cuts)?;
        for c in 0..cuts {
            let t = c as f64 / cuts as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    Some(out)
}

/// A face's triangles, millimetres. Its loops may bound several regions: a loop inside an even number of the others is an
/// outline, and a loop inside it and one more is one of its holes. A loop is inside another where most of its corners are,
/// since two loops may touch.
pub(crate) fn triangles(surface: &Surface, loops: &[Vec<[f64; 2]>], out: &mut Vec<[P3; 3]>) -> Step<()> {
    let steps = surface.steps();
    // More corners than the triangulation takes are refused there, so a face that comes to more is refused here, before its loops
    // are compared with each other (which takes their corners squared).
    let mut room = bimcraft_core::triangulation::MAX_CORNERS;
    let mut dense = Vec::new();
    for l in loops {
        dense.push(densify(l, steps, &mut room).ok_or_else(|| "it has more corners than a triangulation takes".to_string())?);
    }
    let loops: Vec<Vec<[f64; 2]>> = dense.into_iter().filter(|l| l.len() >= 3).collect();
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
    /// The points its lines, arcs and points drawn with its solids pass through ([`marks`]), millimetres in the family's axes.
    pub marks: Vec<P3>,
    /// The material each triangle is drawn in, as its face or mesh names it (an element id); none where it names none (it is
    /// drawn as its category says). One for each triangle.
    pub materials: Vec<Option<u32>>,
    /// The symbols it nests (a family nested in its family: `GInstance`), each with the transform it is placed by, read but not
    /// yet drawn in: [`nest`] draws them in and empties this.
    pub placed: Vec<(u32, crate::placement::Transform)>,
    /// The box its graphics give round their solids (`m_tightbBox`), feet in the family's axes.
    pub tight: [f64; 6],
}

/// How far a transform's axes may be from a turn (unit length, square to each other), as a placement's.
const SQUARE: f64 = 1e-6;

/// Draws `shape` placed by `t` (its origin feet, the shape millimetres) into `tris`, `marks` and `materials` (one for each
/// triangle): mirrored, its triangles turn the other way round, so they still face out. Refused where `t` is no turn or the
/// triangles would pass [`MAX_TRIANGLES`].
pub(crate) fn place(
    shape: &Shape,
    t: &crate::placement::Transform,
    tris: &mut Vec<[P3; 3]>,
    marks: &mut Vec<P3>,
    materials: &mut Vec<Option<u32>>,
) -> Step<()> {
    let [x, y, z] = t.axes;
    let dot = |a: P3, b: P3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let turn = t.axes.iter().chain(std::iter::once(&t.origin)).flatten().all(|v| v.is_finite())
        && [(x, x, 1.0), (y, y, 1.0), (z, z, 1.0), (x, y, 0.0), (y, z, 0.0), (z, x, 0.0)]
            .iter()
            .all(|(a, b, want)| (dot(*a, *b) - want).abs() <= SQUARE);
    if !turn {
        return Err("a symbol is placed by no turn".to_string());
    }
    if tris.len() + shape.triangles.len() > MAX_TRIANGLES {
        return Err(format!("it draws more than {MAX_TRIANGLES} triangles"));
    }
    let mirrored = dot(cross(x, y), z) < 0.0;
    let at = |p: P3| -> P3 { std::array::from_fn(|k| t.origin[k] * MM + p[0] * x[k] + p[1] * y[k] + p[2] * z[k]) };
    materials.resize(tris.len(), None);
    tris.extend(shape.triangles.iter().map(|[a, b, c]| if mirrored { [at(*a), at(*c), at(*b)] } else { [at(*a), at(*b), at(*c)] }));
    materials.extend(shape.materials.iter().take(shape.triangles.len()));
    materials.resize(tris.len(), None);
    marks.extend(shape.marks.iter().map(|p| at(*p)));
    Ok(())
}

/// Why a symbol has no shape when its graphics draw nothing solid: one placed beside what an element draws adds nothing to it.
pub(crate) const NO_SOLID: &str = "its graphics hold no solid";

/// The most symbols deep one symbol's nested families may go.
pub(crate) const MAX_NESTING: usize = 8;

/// Draws in the symbols each shape of `shapes` (keyed by its symbol) nests, each placed by its transform, so a shape is all its
/// family draws; checked as a shape is, against the box its graphics give round their solids. A shape whose nested symbol is not
/// read (none in the file, not read itself, nesting it in turn, or nested more than [`MAX_NESTING`] deep) is none, saying why.
pub(crate) fn nest(shapes: &mut std::collections::BTreeMap<u32, Step<Shape>>) {
    let nesting: Vec<u32> = shapes.iter().filter(|(_, s)| s.as_ref().is_ok_and(|s| !s.placed.is_empty())).map(|(k, _)| *k).collect();
    let mut done = std::collections::BTreeMap::new();
    for id in nesting {
        // The outcome is kept in `done`.
        let _ = drawn_in(shapes, &mut done, id, &mut Vec::new());
    }
    shapes.extend(done);
}

/// Shape `id` of `shapes` with the symbols it nests drawn in, kept in `done`, through the symbols `path` (those nesting it).
fn drawn_in(
    shapes: &std::collections::BTreeMap<u32, Step<Shape>>,
    done: &mut std::collections::BTreeMap<u32, Step<Shape>>,
    id: u32,
    path: &mut Vec<u32>,
) -> Step<()> {
    if let Some(held) = done.get(&id) {
        return held.as_ref().map(|_| ()).map_err(Clone::clone);
    }
    let shape = match shapes.get(&id) {
        None => return Err("the file holds no graphics of it".to_string()),
        Some(Err(why)) => return Err(why.clone()),
        Some(Ok(s)) if s.placed.is_empty() => return Ok(()),
        Some(Ok(s)) => s,
    };
    if path.contains(&id) {
        return Err("it nests itself".to_string());
    }
    if path.len() >= MAX_NESTING {
        return Err(format!("its families nest more than {MAX_NESTING} deep"));
    }
    path.push(id);
    let mut out = Shape { placed: Vec::new(), ..shape.clone() };
    let made = (|| -> Step<()> {
        for (symbol, t) in &shape.placed {
            let nested = |why: String| {
                if why.starts_with("a symbol it nests") { why } else { format!("a symbol it nests is not read: {why}") }
            };
            drawn_in(shapes, done, *symbol, path).map_err(nested)?;
            let inner =
                done.get(symbol).or_else(|| shapes.get(symbol)).and_then(|s| s.as_ref().ok()).ok_or("a symbol it nests is not read")?;
            place(inner, t, &mut out.triangles, &mut out.marks, &mut out.materials)?;
            out.solids += inner.solids;
            out.faces += inner.faces;
        }
        if out.triangles.is_empty() {
            return Err(NO_SOLID.to_string());
        }
        usable(&out.triangles)?;
        if !fills_either(&out.triangles, &out.marks, &out.tight, &out.whole) {
            return Err("its solids read are not the box its graphics give round them".to_string());
        }
        Ok(())
    })();
    path.pop();
    done.insert(id, made.clone().map(|()| out));
    made
}

/// The material an object names in field `field` (an `ElementId`): none where it names none (unset, or one of Revit's own below
/// zero).
fn material_of(r: &Reader, o: &Obj, field: &str) -> Option<u32> {
    r.layouts.get(o, field).and_then(crate::graphics::element_id).and_then(|id| u32::try_from(id).ok()).filter(|id| *id != u32::MAX)
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

/// The triangles of the meshes of `tree` (`GPolyMesh`) into `out`, millimetres in the tree's axes: each mesh's faceted topology,
/// its points (feet, four-byte or eight-byte numbers, moved by its `m_offset` where it has one) and its facets, three indices each,
/// each facet turned to face the way its normal says where the mesh gives one a facet. A mesh that reaches outside `tight`, the
/// box the graphics give round their solids, is none of them (a light's photometric web) and is left out. The meshes read.
fn meshes(r: &Reader, tight: &[f64; 6], out: &mut Vec<[P3; 3]>, materials: &mut Vec<Option<u32>>) -> Step<usize> {
    let mut count = 0;
    for mesh in r.tree.objects.values().filter(|o| r.name(o) == "GPolyMesh") {
        let t = r.owned(mesh, "m_pFacetedTopology")?.ok_or("a mesh has no facets")?;
        let points = r.get(t, "m_pointsArr")?.list().ok_or("a mesh's points are no list")?;
        let offset = match r.layouts.get(t, "m_offset") {
            Some(v) => v.point().ok_or("a mesh's offset is no point")?,
            None => [0.0; 3],
        };
        let points: Vec<P3> = points
            .iter()
            .map(|p| p.point().map(|p| scale(MM, add(p, offset))).ok_or("a mesh's point is not three numbers"))
            .collect::<Result<_, _>>()?;
        let inside = points.iter().all(|p| {
            (0..3).all(|k| {
                let s = slack(tight[k + 3] - tight[k]);
                p[k] / MM >= tight[k] - s && p[k] / MM <= tight[k + 3] + s
            })
        });
        if !inside {
            continue;
        }
        count += 1;
        let facets = r.get(t, "m_facetsArr")?.list().ok_or("a mesh's facets are no list")?;
        let normals = r.layouts.get(t, "m_normalsArr").and_then(Value::list).filter(|n| n.len() == facets.len());
        for (k, facet) in facets.iter().enumerate() {
            if out.len() >= MAX_TRIANGLES {
                return Err(format!("its meshes hold more than {MAX_TRIANGLES} triangles"));
            }
            let index = |v: &Value| match v {
                Value::Int(i) => {
                    points.get(*i as usize).copied().ok_or_else(|| format!("a facet names point {i}, which the mesh does not have"))
                }
                _ => Err("a facet's corner is no index".to_string()),
            };
            let [a, b, c] = match facet.list() {
                Some([a, b, c]) => [index(a)?, index(b)?, index(c)?],
                _ => return Err("a facet is not three corners".to_string()),
            };
            let n = cross(sub(b, a), sub(c, a));
            let flip = normals.and_then(|ns| ns.get(k)?.point()).is_some_and(|w| n[0] * w[0] + n[1] * w[1] + n[2] * w[2] < 0.0);
            out.push(if flip { [a, c, b] } else { [a, b, c] });
        }
        materials.resize(out.len(), material_of(r, mesh, "m_materialID"));
    }
    Ok(count)
}

/// Whether an object of class `name` holds geometry this module does not read: a faceted topology that is no mesh's.
fn unread(name: &str) -> bool {
    name == "FacetedTopol"
}

/// The solids of `tree` (its `Geometry` objects) as triangles into `out`, millimetres in the tree's axes: whether every face reads,
/// and both faces of every edge put its points in one place. The solids and faces read.
fn solids(r: &Reader, out: &mut Vec<[P3; 3]>, materials: &mut Vec<Option<u32>>) -> Step<(usize, usize)> {
    let (mut count, mut faces_read) = (0, 0);
    for o in r.tree.objects.values().filter(|o| r.name(o) == "Geometry") {
        count += 1;
        faces_read += solid(r, o, out, materials)?;
    }
    Ok((count, faces_read))
}

/// One solid, a `Geometry`, as triangles into `out` (and their materials into `materials`): whether every face reads, and both faces
/// of every edge put its points in one place. The faces read.
fn solid(r: &Reader, solid: &Obj, out: &mut Vec<[P3; 3]>, materials: &mut Vec<Option<u32>>) -> Step<usize> {
    let tree = r.tree;
    let mut faces_read = 0;
    let faces: Vec<u32> = match r.get(solid, "m_pFaces")?.list() {
        Some(l) => l.iter().filter_map(|v| if let Value::Owned(i, _) = v { Some(*i) } else { None }).collect(),
        None => return Err("a solid's faces are no list".to_string()),
    };
    let mut surfaces = Surfaces::new();
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
        let on: Vec<&Surface> = (0..2).filter_map(|k| sides.get(k).and_then(Value::weak)).filter_map(|f| surfaces.get(&f)).collect();
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
        // Cut as its edges' points give it; where that fails (a thin face between two curves whose points, joined straight,
        // cross), again with its edges following the curves its curved neighbours put them on.
        let mut cut = Vec::new();
        if let Err(why) = triangles(surface, &r.loops(face, i, &surfaces, false)?, &mut cut) {
            cut.clear();
            let fine = r.loops(face, i, &surfaces, true)?;
            triangles(surface, &fine, &mut cut).map_err(|_| format!("a face is not cut into triangles: {why}"))?;
        }
        out.extend(cut);
        materials.resize(out.len(), material_of(r, face, "m_renderStyleId"));
        faces_read += 1;
    }
    Ok(faces_read)
}

/// The points, lines and arcs a tree draws (`GPoint`, `GLine`, `GArc`: nodes of its groups, not a surface's profile), as points
/// millimetres in its axes: a point, a line's ends, an arc's ends and its points every [`STEP`]. The box a tree gives round its
/// solids may take them in (a stair's support's, the points of its path).
fn marks(r: &Reader) -> Step<Vec<P3>> {
    let nodes: BTreeSet<u32> = r
        .tree
        .objects
        .values()
        .filter_map(|o| r.layouts.get(o, "m_subNodes")?.list())
        .flatten()
        .filter_map(|v| if let Value::Owned(i, _) = v { Some(*i) } else { None })
        .collect();
    let mut out = Vec::new();
    for o in nodes.iter().filter_map(|i| r.tree.get(*i)) {
        match r.name(o) {
            "GPoint" => out.push(r.point(o, "m_coord")?),
            "GLine" | "GArc" | "GEllipse" => {
                let curve = r.curve(o)?;
                let [t0, t1] = r.get(o, "m_endParams")?.pair().ok_or("a curve's ends are no pair")?;
                out.push(curve.at(t0));
                out.push(curve.at(t1));
                if matches!(curve, Curve::Arc { .. } | Curve::Ellipse { .. }) {
                    let n = ((t1 - t0).abs() / STEP).ceil().min(64.0) as usize;
                    out.extend((1..n).map(|k| curve.at(t0 + (t1 - t0) * k as f64 / n as f64)));
                }
            }
            _ => {}
        }
    }
    Ok(out.into_iter().map(|p| scale(MM, p)).collect())
}

/// Whether triangles `tris` and points `marks` (millimetres) fill the box a tree's root gives round its solids (`m_tightbBox`,
/// [`fills`]), or else the box round all it draws (`m_bBox`) while reaching every side of the first: the box round the solids may
/// leave out solids Revit draws (a tread's nosing), where the other is the box round them exactly; one the solids stop short of
/// still holds a solid not read.
pub(crate) fn fills_either(tris: &[[P3; 3]], marks: &[P3], tight: &[f64; 6], whole: &[f64; 6]) -> bool {
    fills(tris, marks, tight) || (fills(tris, marks, whole) && reaches(tris, marks, tight))
}

/// Whether triangles `tris` and points `marks` (millimetres; one past the box counts as at it) reach every side of box `b` (feet),
/// to [`slack`].
fn reaches(tris: &[[P3; 3]], marks: &[P3], b: &[f64; 6]) -> bool {
    let mut e = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    let points = tris.iter().flatten().map(|p| p.map(|v| v / MM));
    let marks = marks.iter().map(|p| std::array::from_fn::<f64, 3, _>(|k| (p[k] / MM).clamp(b[k].min(b[k + 3]), b[k + 3].max(b[k]))));
    for p in points.chain(marks) {
        for k in 0..3 {
            e[k] = e[k].min(p[k]);
            e[k + 3] = e[k + 3].max(p[k]);
        }
    }
    (0..3).all(|k| {
        let s = slack(b[k + 3] - b[k]);
        e[k] <= b[k] + s && e[k + 3] >= b[k + 3] - s
    })
}

/// Whether triangles `tris` and points `marks` (millimetres) fill box `tight` (feet), the box the graphics give round their
/// solids, to [`slack`]: the triangles lie in it, and each of its sides is reached by a triangle or a mark (a mark past it counts
/// as at it, since the box need not take in every line). A solid left unread would leave them short of it.
pub(crate) fn fills(tris: &[[P3; 3]], marks: &[P3], tight: &[f64; 6]) -> bool {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for p in tris.iter().flatten() {
        for k in 0..3 {
            b[k] = b[k].min(p[k] / MM);
            b[k + 3] = b[k + 3].max(p[k] / MM);
        }
    }
    let inside = (0..3).all(|k| {
        let s = slack(tight[k + 3] - tight[k]);
        b[k] >= tight[k] - s && b[k + 3] <= tight[k + 3] + s
    });
    for p in marks {
        for k in 0..3 {
            let v = (p[k] / MM).clamp(tight[k].min(tight[k + 3]), tight[k + 3].max(tight[k]));
            b[k] = b[k].min(v);
            b[k + 3] = b[k + 3].max(v);
        }
    }
    inside
        && (0..3).all(|k| {
            let s = slack(tight[k + 3] - tight[k]);
            (b[k] - tight[k]).abs() <= s && (b[k + 3] - tight[k + 3]).abs() <= s
        })
}

/// The two boxes a tree's root gives: round all it draws (`m_bBox`) and round its solids (`m_tightbBox`), feet.
fn boxes(tree: &Tree, layouts: &Layouts) -> Step<([f64; 6], [f64; 6])> {
    let root = tree.objects.values().next().ok_or("the tree is empty")?;
    let whole = bounds(layouts.get(root, "m_bBox")).ok_or("its graphics give no box")?;
    let tight = bounds(layouts.get(root, "m_tightbBox")).ok_or("its graphics give no box round their solids")?;
    Ok((whole, tight))
}

/// Whether every corner of `triangles` is a number the model reaches: a family's mesh form is refused, and with it the whole import,
/// for a corner that is not.
fn usable(triangles: &[[P3; 3]]) -> Step<()> {
    if triangles.iter().flatten().flatten().any(|x| !x.is_finite() || x.abs() > bimcraft_core::model::MAX_COORD) {
        return Err("a corner of it is not a usable number".to_string());
    }
    Ok(())
}

/// The shape of a type whose tree is `tree`; or why it has none.
pub(crate) fn shape(tree: &Tree, layouts: &Layouts) -> Step<Shape> {
    let r = Reader { tree, layouts, walked: std::cell::Cell::new(0) };
    let (whole, tight) = boxes(tree, layouts)?;
    if let Some(o) = tree.objects.values().find(|o| unread(r.name(o))) {
        return Err(format!("its graphics hold a {}, which is not read", r.name(o)));
    }
    let placed = crate::graphics::placed(tree, layouts)?;
    let mut out = Shape { whole, tight, marks: marks(&r)?, ..Shape::default() };
    (out.solids, out.faces) = solids(&r, &mut out.triangles, &mut out.materials)?;
    out.solids += meshes(&r, &tight, &mut out.triangles, &mut out.materials)?;
    // A family nesting others is checked once they are drawn in ([`nest`]).
    if !placed.is_empty() {
        out.placed = placed;
        return Ok(out);
    }
    if out.triangles.is_empty() {
        return Err(NO_SOLID.to_string());
    }
    usable(&out.triangles)?;
    if !fills_either(&out.triangles, &out.marks, &tight, &whole) {
        return Err("its solids read are not the box its graphics give round them".to_string());
    }
    Ok(out)
}

/// What an element's own graphics draw: the symbols it places (`GInstance`: each symbol and the transform it is drawn by), and
/// solids of its own (a stair's run, a wall sweep, a rail, drawn where they stand), with the two boxes its root gives round them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Drawing {
    pub placed: Vec<(u32, crate::placement::Transform)>,
    /// Its own solids, millimetres in the project's axes; none where it has none.
    pub body: Option<Shape>,
    /// The points its own lines, arcs and points pass through ([`marks`]), millimetres in the project's axes.
    pub marks: Vec<P3>,
    /// The box round all it draws and the box round its solids, feet in the project's axes.
    pub whole: [f64; 6],
    pub tight: [f64; 6],
}

/// What the tree of an element, `tree`, draws; or why it is not read.
pub(crate) fn drawing(tree: &Tree, layouts: &Layouts) -> Step<Drawing> {
    let r = Reader { tree, layouts, walked: std::cell::Cell::new(0) };
    let (whole, tight) = boxes(tree, layouts)?;
    if let Some(o) = tree.objects.values().find(|o| unread(r.name(o))) {
        return Err(format!("its graphics hold a {}, which is not read", r.name(o)));
    }
    let placed = crate::graphics::placed(tree, layouts)?;
    let mut body = Shape { whole, tight, marks: marks(&r)?, ..Shape::default() };
    (body.solids, body.faces) = solids(&r, &mut body.triangles, &mut body.materials)?;
    body.solids += meshes(&r, &tight, &mut body.triangles, &mut body.materials)?;
    usable(&body.triangles)?;
    let marks = std::mem::take(&mut body.marks);
    let body = (!body.triangles.is_empty()).then_some(body);
    if placed.is_empty() && body.is_none() {
        return Err("its graphics hold no solid and no instance".to_string());
    }
    Ok(Drawing { placed, body, marks, whole, tight })
}
