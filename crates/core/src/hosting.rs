//! What a component stands on and how it is placed (T4, T13): the project's reference planes, a work plane-based
//! component hosted on one ([`Hosted`]), and the one placement of a component's family axes in the model
//! ([`Model::component_placement`]) that every drawing, the 3D view, bounds, rooms and IFC read.
//!
//! A hosted component's truth is its place in its host's own axes (`uv`) and its Offset from Host along the host's normal;
//! its `at` (the insertion point in plan) and `offset` (its height over its Schedule Level, Elevation from Level) are worked
//! out from them after every command ([`Model::settle_hosts`]), so whatever reads a component's `at` reads where it is, and
//! moving the plane moves it. A command that moves the component itself (`move`, `copy`, `rotate`, `mirror`, `modify` of
//! `at` or `offset`) moves its `at`: that is projected back onto the plane, giving the new `uv`.

use serde::{Deserialize, Serialize};

use crate::family::{Frame, Piece};
use crate::geom::P2;
use crate::model::{Component, Element, Guid, Id, MAX_COORD, MAX_TEXT, Model};
use crate::view3d::{P3, cross, dot, sub};

/// How far apart two numbers of a derived place may be and still be the same, mm.
const SAME: f64 = 1e-9;

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

/// What a hosted component stands on. Tagged by kind, `{"plane": guid}`, so another kind of host (a face, `{"face": …}`,
/// which Place on Face will add) comes in beside it without changing what a file of planes holds. A work plane-based
/// component on a level has no host: its `level` is its work plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostRef {
    /// A reference plane of the project ([`ReferencePlane`]).
    Plane(Guid),
}

impl HostRef {
    /// The element it names.
    pub fn guid(&self) -> Guid {
        match self {
            HostRef::Plane(g) => *g,
        }
    }

    pub fn guid_mut(&mut self) -> &mut Guid {
        match self {
            HostRef::Plane(g) => g,
        }
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
                bottom: base + pc.bottom,
                top: base + pc.top,
                tris,
                ..pc.clone()
            };
        }
        let all = tris.iter().flatten();
        let (bottom, top) = all.clone().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
        let outline = crate::geom::hull(all.filter(|p| p.iter().all(|v| v.is_finite())).map(|p| P2(p[0], p[1])).collect());
        Piece { tris, outline, bottom, top, ..pc.clone() }
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
    /// the vertical and it turns `angle` degrees about it, counter-clockwise from east, unless it is hosted on a plane
    /// that isn't level and its family isn't Always Vertical: then it tilts with the plane, by the least turn that takes
    /// the vertical to the plane's normal, so on a level plane both are the same and `angle` still turns it about its own z.
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
        let Some(h) = c.host else { return upright };
        if self.families.get(&c.family).is_none_or(|f| f.always_vertical) {
            return upright;
        }
        let Some(frame) = self.host_frame(&h.host) else { return upright };
        let n = frame.z;
        if n[0].abs() < 1e-12 && n[1].abs() < 1e-12 && n[2] > 0.0 {
            return upright;
        }
        Placement { origin, axes: upright.axes.map(|a| tilted(a, n)), upright: false }
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

    /// The `at` and `offset` of a component hosted as `h` with Schedule Level `level`; none where its host isn't a plane.
    pub fn hosted_at(&self, h: &Hosted, level: Guid) -> Option<(P2, f64)> {
        let frame = self.host_frame(&h.host)?;
        let w = Self::hosted_point(&frame, h.uv, h.offset);
        Some((P2(w[0], w[1]), w[2] - self.elevation(level)))
    }

    /// [`Model::work_plane_name`] of element `e`, a component; empty for another.
    pub fn work_plane_name_of(&self, e: &Element) -> String {
        match e {
            Element::Component(c) => self.work_plane_name(c),
            _ => String::new(),
        }
    }

    /// What Work Plane shows of component `c`: `Level : Level 1`, or `Reference Plane : Ramp` (the plane's id where it has
    /// no name).
    pub fn work_plane_name(&self, c: &Component) -> String {
        match c.host {
            None => format!("Level : {}", self.level(c.level).map(|l| l.name.as_str()).unwrap_or_default()),
            Some(Hosted { host: HostRef::Plane(g), .. }) => match self.elements.get(g) {
                Some(Element::RefPlane(p)) if !p.name.is_empty() => format!("Reference Plane : {}", p.name),
                _ => format!("Reference Plane : {}", self.shown(g)),
            },
        }
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

    /// What [`Model::settle_hosts`] changes: each component, as it is to be.
    pub(crate) fn unsettled(&self, before: Option<&Model>) -> Vec<(Id, Component)> {
        let lookup = self.lookup();
        let planes_now = lookup.rare("refplane").next().is_some();
        let planes_then = before.is_some_and(|b| b.lookup().rare("refplane").next().is_some());
        if !planes_now && !planes_then {
            return Vec::new();
        }
        let candidates: Vec<Id> = match before.map(|b| self.changed_since(b)) {
            Some(crate::ids::Changed::Ids(ids)) => {
                let mut out = std::collections::BTreeSet::new();
                for id in ids {
                    out.insert(id);
                    if let Some(g) = self.elements.guid(id) {
                        out.extend(lookup.referrers(&g));
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
            if self.placed_by_points(c) {
                continue;
            }
            let Some(frame) = self.host_frame(&h.host) else { continue };
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
                let uv = Self::hosted_uv(&frame, w, h.offset);
                next.host = Some(Hosted { uv, ..h });
            }
            let Some(hosted) = next.host else { continue };
            let w = Self::hosted_point(&frame, hosted.uv, hosted.offset);
            (next.at, next.offset) = (P2(w[0], w[1]), w[2] - self.elevation(c.level));
            let moved = |a: f64, b: f64| (a - b).abs() > SAME || a.is_nan() != b.is_nan();
            // Its plane changed (turned about its point, say): it may tilt another way where it stands, which what is drawn
            // of it must see, so it changes too.
            let plane = |m: &Model| m.elements.get(h.host.guid()).cloned();
            let turned = before.is_some_and(|b| plane(b) != plane(self));
            if turned || next.host != c.host || moved(next.at.0, c.at.0) || moved(next.at.1, c.at.1) || moved(next.offset, c.offset) {
                out.push((id, next));
            }
        }
        out
    }
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
