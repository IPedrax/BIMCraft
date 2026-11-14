//! What a component stands on and how it is placed (T4, T13, T16): the project's reference planes, a work plane-based
//! component hosted on one or on a face of a wall, a floor, a ceiling or a roof ([`Hosted`], [`FaceRole`]), and the one placement of a component's family axes in the model
//! ([`Model::component_placement`]) that every drawing, the 3D view, bounds, rooms and IFC read.
//!
//! A hosted component's truth is its place in its host's own axes (`uv`) and its Offset from Host along the host's normal;
//! its `at` (the insertion point in plan) and `offset` (its height over its Schedule Level, Elevation from Level) are worked
//! out from them after every command ([`Model::settle_hosts`]), so whatever reads a component's `at` reads where it is, and
//! moving the plane moves it. A command that moves the component itself (`move`, `copy`, `rotate`, `mirror`, `modify` of
//! `at` or `offset`) moves its `at`: that is projected back onto the plane, giving the new `uv`. On a face, `uv` is the
//! face's own: along and up a wall's side, the point in plan on a floor's, a ceiling's or a roof's top or bottom
//! ([`Model::face_at`]), so a change of the host's place, thickness, slope or type moves it.

use serde::{Deserialize, Serialize};

use crate::family::{Frame, Piece};
use crate::geom::{self, P2};
use crate::model::{Component, Element, Guid, Id, MAX_COORD, MAX_TEXT, Model, TOL};
use crate::view3d::{P3, cross, dot, sub, unit as view3d_unit};
use crate::walls::{self, Axis};

/// How far apart two numbers of a derived place may be and still be the same, mm.
const SAME: f64 = 1e-9;

/// A triangle in the model's axes.
type P3Tri = [P3; 3];

/// A reference plane of the project, as Revit's Reference Plane: the line it was drawn as, `start` to `end` (mm, the
/// model's axes), and `up`, its other direction; a plane drawn in plan is upright (`up` the vertical). Its own axes, as a
/// family's planes have them: x along the line, y `up` made square to it, z (its normal) `x × y`; its origin is `start`, so
/// what it hosts moves and turns with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferencePlane {
    /// What Work Plane shows of what it hosts; unique among the project's named planes, and may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub start: P3,
    pub end: P3,
    pub up: P3,
}

fn unit(a: P3) -> P3 {
    let l = dot(a, a).sqrt();
    if l > 0.0 { a.map(|x| x / l) } else { [0.0; 3] }
}

fn usable(p: &P3) -> bool {
    p.iter().all(|x| x.is_finite() && x.abs() <= MAX_COORD)
}

impl ReferencePlane {
    /// Its own axes in the model's.
    pub fn frame(&self) -> Frame {
        let x = unit(sub(self.end, self.start));
        let y = unit(sub(self.up, x.map(|v| v * dot(self.up, x))));
        Frame { origin: self.start, x, y, z: cross(x, y) }
    }

    /// Whether it lies upright (its normal level): what it hosts is placed by a point in space, not in plan.
    pub fn upright(&self) -> bool {
        self.frame().z[2].abs() < 1e-9
    }

    /// What's wrong with it, if anything: numbers within [`MAX_COORD`], a line with two ends, `up` across it, a name of
    /// one line.
    pub fn check(&self, who: &str) -> Result<(), String> {
        if !usable(&self.start) || !usable(&self.end) || !usable(&self.up) {
            return Err(format!("{who}: its start, end and up are numbers within {MAX_COORD} mm"));
        }
        let d = sub(self.end, self.start);
        if dot(d, d).sqrt() < 1e-3 {
            return Err(format!("{who}: its start and end are one point; it needs a line to be drawn along"));
        }
        let across = cross(unit(d), unit(self.up));
        if dot(across, across).sqrt() < 1e-6 {
            return Err(format!("{who}: its `up` lies along its line (or is [0, 0, 0]); it needs a direction across it"));
        }
        if self.name.chars().count() > MAX_TEXT || self.name.chars().any(char::is_control) {
            return Err(format!("{who}: a name is one line of up to {MAX_TEXT} characters"));
        }
        Ok(())
    }
}

/// What a hosted component stands on. Tagged by kind: `{"plane": guid}`, a reference plane of the project (T4, T13), or
/// `{"face": {"element": guid, "face": role}}`, a face of a wall, a floor, a ceiling or a roof (Place on Face, T16). A work
/// plane-based component on a level has no host: its `level` is its work plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostRef {
    /// A reference plane of the project ([`ReferencePlane`]).
    Plane(Guid),
    /// A face of an element, named by its role ([`FaceRole`]), as paint names it.
    Face { element: Guid, face: FaceRole },
}

/// Which face of its element a face host is, by what it is to the element rather than by its triangles, so it lasts through
/// joins, openings, shape points and type changes: a wall's `exterior` (to the left of its start to its end) or `interior`
/// side; a floor's, a ceiling's or a roof's `top` or `bottom` (each slope of a roof, each plane of a shaped floor, found
/// again under the component's point). The names are paint's (`paint::faces`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceRole {
    Exterior,
    Interior,
    Top,
    Bottom,
}

impl FaceRole {
    /// Its name, as paint and commands take it.
    pub fn name(self) -> &'static str {
        match self {
            FaceRole::Exterior => "exterior",
            FaceRole::Interior => "interior",
            FaceRole::Top => "top",
            FaceRole::Bottom => "bottom",
        }
    }

    /// Whether an element of `category` has it to host on: a wall its two sides; a floor, a ceiling or a roof its top and
    /// bottom.
    pub fn of(self, category: &str) -> bool {
        match category {
            "wall" => matches!(self, FaceRole::Exterior | FaceRole::Interior),
            "floor" | "ceiling" | "roof" => matches!(self, FaceRole::Top | FaceRole::Bottom),
            _ => false,
        }
    }

    /// The other side of a wall: a flipped wall's exterior is where its interior was.
    pub fn other_side(self) -> FaceRole {
        match self {
            FaceRole::Exterior => FaceRole::Interior,
            FaceRole::Interior => FaceRole::Exterior,
            other => other,
        }
    }
}

/// The face a triangle of a wall turned `n` (a unit normal) is on, `left` the wall's left at it in plan: its exterior facing
/// left, its interior facing right; none for its top, bottom or ends. Paint and Place on Face both read faces this way.
pub fn wall_side(n: P3, left: P2) -> Option<FaceRole> {
    match n[0] * left.0 + n[1] * left.1 {
        d if d > 0.5 => Some(FaceRole::Exterior),
        d if d < -0.5 => Some(FaceRole::Interior),
        _ => None,
    }
}

/// The face a triangle of a floor, a ceiling or a roof turned `n` is on: its top facing up, its bottom facing down; none
/// for an edge.
pub fn slab_side(n: P3) -> Option<FaceRole> {
    match n[2] {
        z if z > 0.5 => Some(FaceRole::Top),
        z if z < -0.5 => Some(FaceRole::Bottom),
        _ => None,
    }
}

/// **Always Vertical, the one rule (T16).** The z a hosted component stands with, given its host's normal `n` where it
/// stands and its family's Always Vertical: `Some(z)` keeps it plumb with that z; `None` lets it follow the host, its z the
/// normal. As Revit's work plane-based families: Always Vertical on, z stays up on every host, a sloped one, an upright
/// one and an underside too (the component stands up from a ceiling's underside); off, z is the face's normal, so it tilts
/// with a slope, stands out from a wall and hangs from an underside. To hang from undersides even when Always Vertical,
/// return `Some([0.0, 0.0, -1.0])` here where `n[2] < 0.0`: everything else follows from this.
pub fn vertical_z(n: P3, always_vertical: bool) -> Option<P3> {
    let _ = n;
    always_vertical.then_some([0.0, 0.0, 1.0])
}

impl HostRef {
    /// The element it names.
    pub fn guid(&self) -> Guid {
        match self {
            HostRef::Plane(g) => *g,
            HostRef::Face { element, .. } => *element,
        }
    }

    pub fn guid_mut(&mut self) -> &mut Guid {
        match self {
            HostRef::Plane(g) => g,
            HostRef::Face { element, .. } => element,
        }
    }
}

/// Where a hosted component's host is at its place on it: `point`, on the host at `uv` (no Offset from Host); `normal`, the
/// way the host faces there (a unit vector); `across`, for a wall's face, its level direction there (to the right of someone
/// looking at it from outside), which the component's `angle` turns from, so it turns with its wall; and `on`, whether the
/// point is on the face at all (past a wall's end, out of a floor's outline or in its opening, it is on the face's plane
/// carried on, and `Model::warnings` says so).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostAt {
    pub point: P3,
    pub normal: P3,
    pub across: Option<P3>,
    pub on: bool,
}

impl HostAt {
    /// Its point moved `offset` mm along its normal: the component's insertion point.
    pub fn lifted(&self, offset: f64) -> P3 {
        std::array::from_fn(|i| self.point[i] + self.normal[i] * offset)
    }
}

/// A component on a host: `uv`, its insertion point's place in the host's own axes (mm), and `offset`, its Offset from
/// Host, mm along the host's normal.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hosted {
    pub host: HostRef,
    pub uv: [f64; 2],
    #[serde(default)]
    pub offset: f64,
}

/// Where a component's family axes are in the model: `origin`, its insertion point, and the model's directions of its x,
/// y and z. `upright` when z is the vertical, as every component but one tilted with its plane is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub origin: P3,
    pub axes: [P3; 3],
    pub upright: bool,
}

impl Placement {
    /// Point `p` of the family's axes in the model's. Upright, this is to the last bit `at + (x cos - y sin, x sin + y cos,
    /// z)`, as components always were placed: the extra terms add zero.
    pub fn place(&self, p: P3) -> P3 {
        let [a, b, c] = self.axes;
        std::array::from_fn(|i| self.origin[i] + p[0] * a[i] + p[1] * b[i] + p[2] * c[i])
    }

    /// A piece of the family's (mirrored already, if it is) where the component stands. Upright, its outline is placed and
    /// its bottom and top are its own over the origin's height; tilted, its outline is what its triangles cover seen from
    /// above (their hull) and its bottom and top theirs.
    pub fn piece(&self, pc: &Piece) -> Piece {
        let tris: Vec<[P3; 3]> = pc.tris.iter().map(|t| t.map(|p| self.place(p))).collect();
        if self.upright {
            let base = self.origin[2];
            return Piece {
                outline: pc.outline.iter().map(|q| self.place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1])).collect(),
                holes: pc.holes.iter().map(|h| h.iter().map(|q| self.place([q.0, q.1, 0.0])).map(|p| P2(p[0], p[1])).collect()).collect(),
                bottom: base + pc.bottom,
                top: base + pc.top,
                tris,
                ..pc.clone()
            };
        }
        let all = tris.iter().flatten();
        let (bottom, top) = all.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        let outline = crate::geom::hull(all.filter(|p| p.iter().all(|v| v.is_finite())).map(|p| P2(p[0], p[1])).collect());
        Piece { tris, outline, holes: Vec::new(), bottom, top, ..pc.clone() }
    }
}

/// `v` turned by the least turn that takes the vertical to `n`, a unit vector (Rodrigues' formula about up × n).
fn tilted(v: P3, n: P3) -> P3 {
    let s = (n[0] * n[0] + n[1] * n[1]).sqrt();
    if s < 1e-12 {
        // Upside down: half a turn about x.
        return if n[2] >= 0.0 { v } else { [v[0], -v[1], -v[2]] };
    }
    let (k, c) = ([-n[1] / s, n[0] / s, 0.0], n[2]);
    let kv = cross(k, v);
    let along = dot(k, v) * (1.0 - c);
    std::array::from_fn(|i| v[i] * c + kv[i] * s + k[i] * along)
}

impl Model {
    /// The plane host `h` names, in the model's axes; none where it names no reference plane.
    pub fn host_frame(&self, h: &HostRef) -> Option<Frame> {
        match (h, self.elements.get(h.guid())) {
            (HostRef::Plane(_), Some(Element::RefPlane(p))) => Some(p.frame()),
            _ => None,
        }
    }

    /// Where component `c`'s family axes are in the model. Its insertion point is `at`, `offset` over its level. Its z is
    /// the vertical and it turns `angle` degrees about it, counter-clockwise from east, unless it is hosted and
    /// [`vertical_z`] lets it follow its host: then on a plane, a floor, a ceiling or a roof it tilts with it, by the least
    /// turn that takes the vertical to the normal, so on a level face both are the same and `angle` still turns it about
    /// its own z. On a wall's face `angle` turns it from the face's level direction, about z (plumb) or the face's normal
    /// (following it), so it turns with its wall; following it, its y goes up the face.
    ///
    /// An adaptive component ([`Model::placed_by_points`]) takes its place from its own points, which are held in these
    /// upright axes (`Model::component_axes`): it is always placed upright, whatever `host` it might carry, which
    /// `Model::check` refuses on one anyway, so no work plane ever tilts it.
    pub fn component_placement(&self, c: &Component) -> Placement {
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let origin = [c.at.0, c.at.1, self.elevation(c.level) + c.offset];
        let upright = Placement { origin, axes: [[cos, sin, 0.0], [-sin, cos, 0.0], [0.0, 0.0, 1.0]], upright: true };
        if self.placed_by_points(c) {
            return upright;
        }
        if let (None, Some([x, z])) = (c.host, c.tilt) {
            let turn = |v: P3| [v[0] * cos - v[1] * sin, v[0] * sin + v[1] * cos, v[2]];
            let (x, z) = (turn(x), turn(z));
            return Placement { origin, axes: [x, cross(z, x), z], upright: false };
        }
        let Some(h) = c.host else { return upright };
        let Some(at) = self.host_at(&h) else { return upright };
        let always_vertical = self.families.get(&c.family).is_none_or(|f| f.always_vertical);
        let z = vertical_z(at.normal, always_vertical).unwrap_or(at.normal);
        let up = z[0].abs() < 1e-12 && z[1].abs() < 1e-12 && z[2] > 0.0;
        match at.across {
            Some(r) => {
                let x: P3 = std::array::from_fn(|i| r[i] * cos + cross(z, r)[i] * sin);
                Placement { origin, axes: [x, cross(z, x), z], upright: up }
            }
            None if up => upright,
            None => Placement { origin, axes: upright.axes.map(|a| tilted(a, z)), upright: false },
        }
    }

    /// Whether component `c` takes its place from its adaptive points: it has frames of its own (`adaptive`), or its family
    /// is adaptive and it stands on the family's test placement. Such a component has no work plane but its level.
    pub fn placed_by_points(&self, c: &Component) -> bool {
        c.adaptive.is_some() || self.families.get(&c.family).is_some_and(|f| f.adaptive.is_some())
    }

    /// The insertion point of a component hosted on `frame` at `uv`, `offset` off it along its normal.
    pub fn hosted_point(frame: &Frame, uv: [f64; 2], offset: f64) -> P3 {
        frame.point(P2(uv[0], uv[1]), offset)
    }

    /// The place on `frame` (`uv`) of a point `w` in space with an Offset from Host of `offset`: the point less the offset,
    /// taken onto the plane straight up or down (along its normal, for an upright plane), so a move in plan keeps the
    /// point in plan where the plane is not upright.
    pub fn hosted_uv(frame: &Frame, w: P3, offset: f64) -> [f64; 2] {
        let n = frame.z;
        let b: P3 = std::array::from_fn(|i| w[i] - n[i] * offset);
        let off = dot(sub(b, frame.origin), n);
        let p = if n[2].abs() > 1e-9 { [b[0], b[1], b[2] - off / n[2]] } else { std::array::from_fn(|i| b[i] - n[i] * off) };
        let d = sub(p, frame.origin);
        [dot(d, frame.x), dot(d, frame.y)]
    }

    /// The `at` and `offset` of a component hosted as `h` with Schedule Level `level`; none where its host is gone.
    pub fn hosted_at(&self, h: &Hosted, level: Guid) -> Option<(P2, f64)> {
        let w = self.host_at(h)?.lifted(h.offset);
        Some((P2(w[0], w[1]), w[2] - self.elevation(level)))
    }

    /// Where host `h` is at its place `h.uv` on it ([`HostAt`]); none where it names nothing that hosts.
    pub fn host_at(&self, h: &Hosted) -> Option<HostAt> {
        match h.host {
            HostRef::Plane(_) => {
                let frame = self.host_frame(&h.host)?;
                Some(HostAt { point: frame.point(P2(h.uv[0], h.uv[1]), 0.0), normal: frame.z, across: None, on: true })
            }
            HostRef::Face { element, face } => self.face_at(element, face, h.uv),
        }
    }

    /// The place on host `h` of a point `w` in space, the component there having `h`'s Offset from Host: on a plane, as
    /// [`Model::hosted_uv`]; on a face, the point less the offset along the face's normal, then on a wall's face its
    /// nearest point, on a floor's, a ceiling's or a roof's the point in plan.
    pub fn host_uv(&self, h: &Hosted, w: P3) -> Option<[f64; 2]> {
        match h.host {
            HostRef::Plane(_) => Some(Self::hosted_uv(&self.host_frame(&h.host)?, w, h.offset)),
            HostRef::Face { element, face } => {
                let n = self.host_at(h).map_or([0.0, 0.0, 1.0], |a| a.normal);
                let b: P3 = std::array::from_fn(|i| w[i] - n[i] * h.offset);
                self.face_uv(element, face, b)
            }
        }
    }

    /// Face `face` of element `element` at `uv`: on a wall's side `[along, up]`, mm along its location line from its
    /// start (as a door's offset is) and up from its base (as a sill is, `Model::own_base`); on a floor's, a ceiling's or a
    /// roof's top or bottom `[x, y]`, the point in plan, the face's height found under it (the piece of a shaped floor or
    /// a roof's slope there; past every piece, the nearest one's plane). None where the element doesn't have that face.
    pub fn face_at(&self, element: Guid, face: FaceRole, uv: [f64; 2]) -> Option<HostAt> {
        let e = self.elements.get(element)?;
        if !face.of(e.category()) || !uv.iter().all(|v| v.is_finite()) {
            return None;
        }
        if let Element::Wall(w) = e {
            let axis = Axis::of(w);
            let (ext, int) = walls::faces(self, w);
            let exterior = face == FaceRole::Exterior;
            let q = axis.at(uv[0], if exterior { ext } else { int });
            let left = axis.tangent(uv[0]).perp();
            let n2 = if exterior { left } else { left * -1.0 };
            let normal = [n2.0, n2.1, 0.0];
            let z = self.own_base(w) + uv[1];
            let (lo, hi) = (self.base_z(w), self.top_z(w));
            let on = uv[0] >= -TOL && uv[0] <= axis.length() + TOL && z >= lo - TOL && z <= hi + TOL;
            return Some(HostAt { point: [q.0, q.1, z], normal, across: Some([-normal[1], normal[0], 0.0]), on });
        }
        let q = P2(uv[0], uv[1]);
        let pieces = self.slab_pieces(e);
        let near = |r: &[P2]| geom::contains(r, q) || geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
        // A ceiling's openings are holes in its one piece; a floor's and a roof's pieces leave theirs out already.
        let holes: &[Vec<P2>] = if let Element::Ceiling(c) = e { &c.openings } else { &[] };
        let found = pieces.iter().find(|(r, ..)| near(r)).filter(|_| !holes.iter().any(|h| geom::contains(h, q)));
        let (piece, on) = match found {
            Some(p) => (p, true),
            None if !holes.is_empty() && pieces.iter().any(|(r, ..)| near(r)) => (pieces.first()?, false),
            None => {
                let away = |r: &[P2]| geom::ring_edges(r).map(|(a, b)| geom::dist_to_segment(q, a, b)).fold(f64::INFINITY, f64::min);
                (pieces.iter().min_by(|a, b| away(&a.0).total_cmp(&away(&b.0)))?, false)
            }
        };
        let plane = if face == FaceRole::Top { piece.2 } else { piece.1 };
        let up = view3d_unit([-plane.g.0, -plane.g.1, 1.0]);
        let normal = if face == FaceRole::Top { up } else { up.map(|v| -v) };
        Some(HostAt { point: [q.0, q.1, plane.at(q)], normal, across: None, on })
    }

    /// The place on face `face` of element `element` of a point `b` on it (or near it): on a wall's side, its nearest point,
    /// `[along, up]` (along the line carried on past its ends, if it's straight); on a floor's, a ceiling's or a roof's, the
    /// point in plan.
    pub fn face_uv(&self, element: Guid, face: FaceRole, b: P3) -> Option<[f64; 2]> {
        let e = self.elements.get(element)?;
        if !face.of(e.category()) {
            return None;
        }
        match e {
            Element::Wall(w) => {
                let axis = Axis::of(w);
                let p = P2(b[0], b[1]);
                let along = if w.arc == 0.0 { (p - w.start).dot(axis.tangent(0.0)) } else { axis.project(p).0 };
                Some([along, b[2] - self.own_base(w)])
            }
            _ => Some([b[0], b[1]]),
        }
    }

    /// A floor's, a ceiling's or a roof's pieces in plan, each with the planes of its underside and its top.
    fn slab_pieces(&self, e: &Element) -> Vec<(Vec<P2>, walls::Plane, walls::Plane)> {
        match e {
            Element::Floor(f) => self.floor_prisms(f),
            Element::Roof(r) => self.roof_prisms(r),
            Element::Ceiling(c) => {
                let under = walls::Plane::flat(self.elevation(c.level) + c.offset);
                let t = self.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
                vec![(c.boundary.clone(), under, walls::Plane::flat(under.z + t))]
            }
            _ => Vec::new(),
        }
    }

    /// The face a plan of `level` places on at plan point `q` (Place on Face in plan): in a reflected ceiling plan, the
    /// underside of the lowest ceiling of the level over it; in a floor plan, the top of the highest floor or roof of the
    /// level under it. None where there is none.
    pub fn face_in_plan(&self, level: impl crate::model::Key, q: P2, ceiling_plan: bool) -> Option<(Id, FaceRole)> {
        let level = self.elements.guid(self.elements.id(level)?)?;
        let face = if ceiling_plan { FaceRole::Bottom } else { FaceRole::Top };
        let mut best: Option<(Id, f64)> = None;
        for (id, e) in self.elements.iter() {
            let ok = match e {
                Element::Ceiling(c) => ceiling_plan && c.level == level,
                Element::Floor(f) => !ceiling_plan && f.level == level,
                Element::Roof(r) => !ceiling_plan && r.level == level,
                _ => false,
            };
            let Some(at) = ok.then(|| self.face_at(self.guid(*id), face, [q.0, q.1])).flatten().filter(|a| a.on) else { continue };
            let z = at.point[2];
            if best.is_none_or(|(_, b)| if ceiling_plan { z < b } else { z > b }) {
                best = Some((*id, z));
            }
        }
        best.map(|(id, _)| (id, face))
    }

    /// The face role of element `id` turned `normal` at point `at`, as a click in the 3D view finds it: a wall's side by
    /// which way it faces, a floor's, a ceiling's or a roof's top or bottom; none for any other element or an edge.
    pub fn face_role(&self, id: Id, at: P3, normal: P3) -> Option<FaceRole> {
        match self.elements.get(id)? {
            Element::Wall(w) => {
                let axis = Axis::of(w);
                wall_side(view3d_unit(normal), axis.tangent(axis.project(P2(at[0], at[1])).0).perp())
            }
            e if matches!(e.category(), "floor" | "ceiling" | "roof") => slab_side(view3d_unit(normal)),
            _ => None,
        }
    }

    /// [`Model::work_plane_name`] of element `e`, a component; empty for another.
    pub fn work_plane_name_of(&self, e: &Element) -> String {
        match e {
            Element::Component(c) => self.work_plane_name(c),
            _ => String::new(),
        }
    }

    /// What Work Plane shows of component `c`: `Level : Level 1`, `Reference Plane : Ramp` (the plane's id where it has
    /// no name), or a face's element, its type and the face: `Floor : Generic - 200mm : Top`.
    pub fn work_plane_name(&self, c: &Component) -> String {
        match c.host {
            None => format!("Level : {}", self.level(c.level).map(|l| l.name.as_str()).unwrap_or_default()),
            Some(Hosted { host: HostRef::Plane(g), .. }) => match self.elements.get(g) {
                Some(Element::RefPlane(p)) if !p.name.is_empty() => format!("Reference Plane : {}", p.name),
                _ => format!("Reference Plane : {}", self.shown(g)),
            },
            Some(Hosted { host: HostRef::Face { element, face }, .. }) => {
                let (kind, type_name) = match self.elements.get(element) {
                    Some(Element::Wall(w)) => ("Wall", w.type_name.as_str()),
                    Some(Element::Floor(f)) => ("Floor", f.type_name.as_str()),
                    Some(Element::Ceiling(c)) => ("Ceiling", c.type_name.as_str()),
                    Some(Element::Roof(r)) => ("Roof", r.type_name.as_str()),
                    _ => ("Face", ""),
                };
                let role = face.name();
                format!("{kind} : {type_name} : {}{}", role[..1].to_uppercase(), &role[1..])
            }
        }
    }

    /// The face-hosted components that are off their face (`HostAt::on`): past a wall's end or out of its height, out of a
    /// floor's, a ceiling's or a roof's outline or in an opening; each with what Work Plane shows of it.
    pub fn off_their_faces(&self) -> Vec<(Id, String)> {
        self.elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Component(c) => match c.host {
                    Some(h @ Hosted { host: HostRef::Face { .. }, .. }) if self.host_at(&h).is_some_and(|a| !a.on) => {
                        Some((*id, self.work_plane_name(c)))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The voids face-hosted component `c` cuts its host with (T16b), in the model's axes: its family's voids that cut none
    /// of its own forms ([`crate::family::Family::face_voids`]), mirrored if it is and placed where it stands. None for a
    /// component on a level or a plane, of a family with none, or one that doesn't build. Whether each reaches its host is
    /// for the host's solids to find: one that doesn't cuts nothing.
    pub fn face_voids(&self, c: &Component) -> Vec<Vec<P3Tri>> {
        if !matches!(c.host, Some(Hosted { host: HostRef::Face { .. }, .. })) || self.placed_by_points(c) {
            return Vec::new();
        }
        let Some(f) = self.families.get(&c.family) else { return Vec::new() };
        if !f.forms.iter().any(|x| x.void) {
            return Vec::new();
        }
        let Ok(voids) = f.values(&c.type_name, &c.params).and_then(|v| f.face_voids(&v)) else { return Vec::new() };
        let placed = self.component_placement(c);
        let place = |p: P3| placed.place(if c.mirrored { [p[0], -p[1], p[2]] } else { p });
        voids
            .into_iter()
            .map(|v| {
                v.iter()
                    .map(|[a, b, t]| if c.mirrored { [place(*a), place(*t), place(*b)] } else { [place(*a), place(*b), place(*t)] })
                    .collect()
            })
            .filter(|v: &Vec<P3Tri>| v.iter().flatten().all(|p| p.iter().all(|x| x.is_finite())))
            .collect()
    }

    /// The components on a face of wall, floor, ceiling or roof `host` whose voids cut it, each with its voids
    /// ([`Model::face_voids`]); none for another kind of element, or one nothing with voids is placed on.
    pub fn hosted_voids(&self, host: Id) -> Vec<(Id, Vec<Vec<P3Tri>>)> {
        let (Some(e), Some(g)) = (self.elements.get(host), self.elements.guid(host)) else { return Vec::new() };
        if !matches!(e.category(), "wall" | "floor" | "ceiling" | "roof") {
            return Vec::new();
        }
        let lookup = self.lookup();
        let mut ids: Vec<Id> = lookup.referrers(&g).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| match self.elements.get(id) {
                Some(Element::Component(c)) if matches!(c.host, Some(Hosted { host: HostRef::Face { element, .. }, .. }) if element == g) =>
                {
                    let voids = self.face_voids(c);
                    (!voids.is_empty()).then_some((id, voids))
                }
                _ => None,
            })
            .collect()
    }

    /// The hosted components' `at` and `offset` worked out again from their hosts, and their place on the host from their
    /// `at` where a command moved them. With `before` (the model a command started from), only what the command could have
    /// reached is looked at: a component it moved (its `at`, `offset` or a new one) is taken onto its plane at its new
    /// point; one whose host, place on it, plane or level changed is placed by its host. Without (a file read), every
    /// hosted component is placed by its host.
    pub fn settle_hosts(&mut self, before: Option<&Model>) {
        let found = self.unsettled(before);
        self.settle(found);
    }

    /// Each component of `found` put in place.
    pub(crate) fn settle(&mut self, found: Vec<(Id, Component)>) {
        for (id, c) in found {
            if let Some(Element::Component(old)) = self.elements.get_mut(id) {
                *old = c;
            }
        }
    }

    /// What [`Model::settle_hosts`] changes: each component, as it is to be. What a command changed is looked into, what
    /// points at it, and what a wall, a floor, a ceiling or a roof among those hosts (a level moved moves its walls' faces).
    pub(crate) fn unsettled(&self, before: Option<&Model>) -> Vec<(Id, Component)> {
        let lookup = self.lookup();
        let candidates: Vec<Id> = match before.map(|b| self.changed_since(b)) {
            Some(crate::ids::Changed::Ids(ids)) => {
                let mut out = std::collections::BTreeSet::new();
                for id in ids {
                    out.insert(id);
                    let Some(g) = self.elements.guid(id) else { continue };
                    for r in lookup.referrers(&g) {
                        out.insert(r);
                        if self.elements.get(r).is_some_and(|e| matches!(e.category(), "wall" | "floor" | "ceiling" | "roof"))
                            && let Some(rg) = self.elements.guid(r)
                        {
                            out.extend(lookup.referrers(&rg));
                        }
                    }
                }
                out.into_iter().collect()
            }
            _ => self.elements.iter().filter(|(_, e)| matches!(e, Element::Component(c) if c.host.is_some())).map(|(id, _)| *id).collect(),
        };
        let mut out = Vec::new();
        for id in candidates {
            let Some(Element::Component(c)) = self.elements.get(id) else { continue };
            let Some(h) = c.host else { continue };
            // An adaptive component is where its points put it: no host moves it.
            if self.placed_by_points(c) || self.host_at(&h).is_none() {
                continue;
            }
            let prev = before.map(|b| match b.elements.get(id) {
                Some(Element::Component(p)) => Some(p),
                _ => None,
            });
            // Placed by its host: from a file; or where the command changed its host or left its `at` and `offset` alone.
            let by_host = match prev {
                None => true,
                Some(None) => false,
                Some(Some(p)) => p.host != c.host || (p.at == c.at && p.offset == c.offset),
            };
            let mut next = c.clone();
            if !by_host {
                let w = [c.at.0, c.at.1, self.elevation(c.level) + c.offset];
                if let Some(uv) = self.host_uv(&h, w) {
                    next.host = Some(Hosted { uv, ..h });
                }
            }
            // A plane turned about the vertical turns what it hosts with it, as Revit's does (a wall's face turns it by its
            // own level direction, a floor's is carried by the command that turns it).
            if by_host
                && let (HostRef::Plane(g), Some(Some(p)), Some(b)) = (h.host, prev, before)
                && p.angle == c.angle
                && let (Some(Element::RefPlane(was)), Some(Element::RefPlane(now))) = (b.elements.get(g), self.elements.get(g))
                && let (Some(from), Some(to)) = (heading(was), heading(now))
            {
                let d = (to - from + 180.0).rem_euclid(360.0) - 180.0;
                let d = (d * 1e9).round() / 1e9;
                if d != 0.0 {
                    next.angle = ((c.angle + d) * 1e9).round() / 1e9 + 0.0;
                }
            }
            let Some(hosted) = next.host else { continue };
            let Some(w) = self.host_at(&hosted).map(|a| a.lifted(hosted.offset)) else { continue };
            (next.at, next.offset) = (P2(w[0], w[1]), w[2] - self.elevation(c.level));
            let moved = |a: f64, b: f64| (a - b).abs() > SAME || a.is_nan() != b.is_nan();
            // Its host changed (turned about its point, say): it may tilt another way where it stands, which what is drawn
            // of it must see, so it changes too.
            let host = |m: &Model| m.elements.get(h.host.guid()).cloned();
            let turned = before.is_some_and(|b| host(b) != host(self));
            if turned
                || next.host != c.host
                || next.angle != c.angle
                || moved(next.at.0, c.at.0)
                || moved(next.at.1, c.at.1)
                || moved(next.offset, c.offset)
            {
                out.push((id, next));
            }
        }
        out
    }
}

/// Which way reference plane `p` heads in plan, degrees counter-clockwise from east: its line's, or its normal's where the
/// line stands upright.
fn heading(p: &ReferencePlane) -> Option<f64> {
    let f = p.frame();
    [f.x, f.z].into_iter().find(|v| v[0].hypot(v[1]) > 1e-6).map(|v| v[1].atan2(v[0]).to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilt_takes_up_to_the_normal_and_keeps_axes_square() {
        let n = [0.0, -0.5, 0.75_f64.sqrt()];
        let z = tilted([0.0, 0.0, 1.0], n);
        assert!(z.iter().zip(n).all(|(a, b)| (a - b).abs() < 1e-12), "{z:?}");
        let (x, y) = (tilted([1.0, 0.0, 0.0], n), tilted([0.0, 1.0, 0.0], n));
        assert!(dot(x, y).abs() < 1e-12 && dot(x, z).abs() < 1e-12 && (dot(cross(x, y), z) - 1.0).abs() < 1e-12);
        // Turned about the x axis of the slope: x stays east.
        assert!((x[0] - 1.0).abs() < 1e-12);
        let down = tilted([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
        assert_eq!(down, [0.0, 0.0, -1.0]);
    }

    #[test]
    fn a_point_taken_onto_a_sloped_plane_keeps_its_place_in_plan() {
        let p = ReferencePlane { name: String::new(), start: [0.0; 3], end: [1.0, 0.0, 0.0], up: [0.0, 1.0, 1.0] };
        let f = p.frame();
        let uv = Model::hosted_uv(&f, [300.0, 400.0, 0.0], 0.0);
        let w = Model::hosted_point(&f, uv, 0.0);
        assert!((w[0] - 300.0).abs() < 1e-9 && (w[1] - 400.0).abs() < 1e-9 && (w[2] - 400.0).abs() < 1e-9, "{w:?}");
    }
}
