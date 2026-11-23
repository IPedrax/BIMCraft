//! Wall geometry. A wall's location line runs from its start to its end, straight or an arc; its
//! faces and its layers' boundaries lie across it where its type and its location line put them,
//! the exterior side to its left. Where walls meet, end to end at any angle or one ending on
//! another, their faces meet in corners: each end's cap. Caps and faces make the outline plans
//! fill, 3D and sections cut, and IFC sweeps. A wall's top may follow a roof or a floor above it,
//! its base one below: the lowest of their planes, which is a pitched roof's surface.

use std::f64::consts::{PI, TAU};

mod compound;
mod index;
pub(crate) mod profile;
pub(crate) mod slant;
pub(crate) use index::{Index, reach};

use crate::curves::Curve;
use crate::geom::{self, P2};
use crate::model::{Element, Function, Guid, Id, Layer, Location, Model, Opening, TOL, Wall};

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

    /// The box round it that holds its body: an arc may bulge past its ends, so the box round its circle, else its ends.
    /// What a cheap "is anything near the cursor" asks before a wall's heights or an opening's type are worked out.
    pub fn extent(&self) -> [P2; 2] {
        match (self.centre(), self.radius()) {
            (Some(c), Some(r)) => [c + P2(-r, -r), c + P2(r, r)],
            _ => [self.start, self.end],
        }
    }

    /// An arc's centre, radius, the angle its start lies at from the centre and how far it sweeps (radians,
    /// counter-clockwise positive); none when it's straight.
    pub fn arc(&self) -> Option<(P2, f64, f64, f64)> {
        self.arc
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
pub fn depth(thickness: f64, layers: &[Layer], loc: Location) -> f64 {
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

/// Farther up or down than anything stands, mm: the open end of a band of height.
const FAR: f64 = 1.0e12;

/// The most a wall's faces may lean from upright, either way, degrees.
pub const MAX_SLANT: f64 = 60.0;

/// How far wall `w`'s faces have moved to its left at height `z`, leaning `slant` degrees about its location line at its base.
pub fn lean(m: &Model, w: &Wall, z: f64) -> f64 {
    if w.slant == 0.0 || !w.slant.is_finite() {
        return 0.0;
    }
    (z - m.own_base(w)) * w.slant.clamp(-MAX_SLANT, MAX_SLANT).to_radians().tan()
}

/// [`faces`] at height `z`: a slanted wall's moved over as it leans, and as much wider across in plan as a slope across it
/// is longer than the thickness. Without a height, or upright, its faces.
pub fn faces_at(m: &Model, w: &Wall, z: Option<f64>) -> (f64, f64) {
    let (ext, int) = faces(m, w);
    match z {
        Some(z) if w.slant != 0.0 && w.slant.is_finite() => {
            let wide = 1.0 / w.slant.clamp(-MAX_SLANT, MAX_SLANT).to_radians().cos();
            let d = lean(m, w, z);
            (ext * wide + d, int * wide + d)
        }
        _ => (ext, int),
    }
}

/// How far to the left of its location line a wall's centre line lies.
pub fn centre(m: &Model, w: &Wall) -> f64 {
    let (ext, int) = faces(m, w);
    (ext + int) / 2.0
}

/// A wall's face across its thickness: its location line, a finish face or a core face. The exterior side is the
/// one to its left, as [`faces`] says; `Centre` is the line half way through the wall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Face {
    /// The line half way through the wall ([`centre`]).
    #[default]
    Centre,
    /// The finish on the left side.
    ExteriorFinish,
    /// The finish on the right side.
    InteriorFinish,
    /// The structural core's face on the left side.
    ExteriorCore,
    /// The structural core's face on the right side.
    InteriorCore,
}

impl Face {
    /// Every face, as a command or a window names one.
    pub const ALL: [Face; 5] = [Face::Centre, Face::ExteriorFinish, Face::InteriorFinish, Face::ExteriorCore, Face::InteriorCore];

    /// Its name in a command: `centre`, `exterior-finish`, `interior-finish`, `exterior-core`, `interior-core`.
    pub fn named(self) -> &'static str {
        match self {
            Face::Centre => "centre",
            Face::ExteriorFinish => "exterior-finish",
            Face::InteriorFinish => "interior-finish",
            Face::ExteriorCore => "exterior-core",
            Face::InteriorCore => "interior-core",
        }
    }

    /// The face `s` names (also `center`), if it names one.
    pub fn parse(s: &str) -> Option<Face> {
        Some(match s {
            "centre" | "center" => Face::Centre,
            "exterior-finish" => Face::ExteriorFinish,
            "interior-finish" => Face::InteriorFinish,
            "exterior-core" => Face::ExteriorCore,
            "interior-core" => Face::InteriorCore,
            _ => return None,
        })
    }

    /// Whether it is a core face rather than a finish one.
    pub fn is_core(self) -> bool {
        matches!(self, Face::ExteriorCore | Face::InteriorCore)
    }

    /// The same face on the other side of the wall: a finish on its finish, a core on its core. The centre line has
    /// no other side.
    pub fn flipped(self) -> Face {
        match self {
            Face::ExteriorFinish => Face::InteriorFinish,
            Face::InteriorFinish => Face::ExteriorFinish,
            Face::ExteriorCore => Face::InteriorCore,
            Face::InteriorCore => Face::ExteriorCore,
            Face::Centre => Face::Centre,
        }
    }

    /// The face on the same side that is the core's, or the finish's.
    pub fn with_core(self, core: bool) -> Face {
        match (self, core) {
            (Face::ExteriorFinish | Face::ExteriorCore, true) => Face::ExteriorCore,
            (Face::ExteriorFinish | Face::ExteriorCore, false) => Face::ExteriorFinish,
            (Face::InteriorFinish | Face::InteriorCore, true) => Face::InteriorCore,
            (Face::InteriorFinish | Face::InteriorCore, false) => Face::InteriorFinish,
            (Face::Centre, _) => Face::Centre,
        }
    }

    /// Which way, left of the location line, the line `off` mm away from this face lies: outward from the wall for a
    /// finish or a core face, and to the left for the centre line.
    fn away(self, off: f64) -> f64 {
        match self {
            Face::InteriorFinish | Face::InteriorCore => -off,
            Face::Centre | Face::ExteriorFinish | Face::ExteriorCore => off,
        }
    }
}

/// The faces of a wall's structural core: how far to the left of its location line the core's exterior and interior
/// faces lie; none for a wall type with no structural layer.
pub fn core_faces(m: &Model, w: &Wall) -> Option<(f64, f64)> {
    let t = m.types.wall.get(&w.type_name)?;
    let mut core: Option<(f64, f64)> = None;
    let mut at = 0.0;
    for l in &t.layers {
        if l.function == Function::Structure {
            core = Some((core.map_or(at, |c| c.0), at + l.thickness));
        }
        at += l.thickness;
    }
    let (c0, c1) = core?;
    let ext = depth(t.thickness, &t.layers, w.location);
    Some((ext - c0, ext - c1))
}

/// How far to the left of a wall's location line a face lies; none for a core face of a wall that has no core.
pub fn face_offset(m: &Model, w: &Wall, face: Face) -> Option<f64> {
    match face {
        Face::Centre => Some(centre(m, w)),
        Face::ExteriorFinish => Some(faces(m, w).0),
        Face::InteriorFinish => Some(faces(m, w).1),
        Face::ExteriorCore => core_faces(m, w).map(|c| c.0),
        Face::InteriorCore => core_faces(m, w).map(|c| c.1),
    }
}

/// The line of a wall's face, out `offset` mm from it, cut at both ends where the wall's cap meets it: the line a
/// boundary picked on a wall lies on. A core face of a wall with no core falls back to its finish face. None for a
/// wall that isn't there or a line with no length.
pub fn face_line(m: &Model, id: Id, face: Face, offset: f64) -> Option<Curve> {
    face_line_in(m, id, face, offset, None)
}

/// [`face_line`], its joins found through `index` when there is one.
pub(crate) fn face_line_in(m: &Model, id: Id, face: Face, offset: f64, index: Option<&Index>) -> Option<Curve> {
    let w = m.wall(id)?;
    let base = face_offset(m, w, face).or_else(|| match face {
        Face::ExteriorCore => face_offset(m, w, Face::ExteriorFinish),
        Face::InteriorCore => face_offset(m, w, Face::InteriorFinish),
        _ => None,
    })?;
    let at = base + face.away(offset);
    let axis = Axis::of(w);
    // A face that is one of the layers' boundaries keeps the corner a compound type shares; any other line is cut
    // where the wall's cap crosses it.
    if let Some(k) = boundaries(m, w).iter().position(|o| (*o - at).abs() <= 1e-9) {
        let line = layer_geometry(m, id, index).lines.get(k).cloned()?;
        return curve_of(&axis, line, at);
    }
    let len = axis.length();
    let ends = [end_geometry(m, id, false, index, &|_| true, None), end_geometry(m, id, true, index, &|_| true, None)];
    let mut line = axis.points(0.0, len, at);
    if line.len() < 2 {
        return None;
    }
    for (i, end) in ends.iter().enumerate() {
        let last = line.len() - 1;
        line[if i == 0 { 0 } else { last }] = on_cap(&axis, &end.outline, i == 1, at);
    }
    curve_of(&axis, line, at)
}

/// `line` through the points of a face: an arc of a curved wall, else a straight line.
fn curve_of(axis: &Axis, line: Vec<P2>, off: f64) -> Option<Curve> {
    let (start, end) = (line.first().copied()?, line.last().copied()?);
    if axis.arc().is_some() && line.len() >= 2 {
        Some(Curve::arc_through(start, end, axis.at(axis.length() / 2.0, off)))
    } else {
        Some(Curve::line(start, end))
    }
}

/// Whether two walls overlap in height, so they join.
pub fn stacked(m: &Model, a: &Wall, b: &Wall) -> bool {
    m.base_z(a) < m.top_z(b) && m.base_z(b) < m.top_z(a)
}

/// Whether wall `id`'s start, or with `at_end` its end, is joined to another wall there: one whose body reaches the node
/// and overlaps it in height (see [`stacked`]). A wall refusing to join, a column's or a grid's meeting it, and nothing at
/// all, are no join.
pub(crate) fn joined_at(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> bool {
    let Some(w) = m.wall(id) else { return false };
    if !w.join {
        return false;
    }
    let (arms, _) = arms(m, id, at_end, index, &|_| true, true, None);
    arms.iter().any(|a| a.id != id)
}

/// The wall joined to wall `id` at its start, or with `at_end` its end, that runs square to it, if one does: what a
/// dimension made permanent there measures to, so that changing it moves that whole wall, not just its joined end.
pub fn joined_perpendicular(m: &Model, id: Id, at_end: bool) -> Option<Id> {
    joined_perpendicular_in(m, id, at_end, None)
}

/// [`joined_perpendicular`], its joins found through `index` when there is one.
pub(crate) fn joined_perpendicular_in(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> Option<Id> {
    let w = m.wall(id)?;
    if !w.join {
        return None;
    }
    let node = if at_end { w.end } else { w.start };
    let u = (w.end - w.start).unit();
    let near: Vec<Id> = match index {
        Some(index) => index.neighbours_at(id, node),
        None => {
            let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
            let at = ([node.0, node.1, node.0, node.1], m.base_z(w), m.top_z(w));
            m.lookup().walls_near_at(&[at], thickest + 1.0).into_iter().collect()
        }
    };
    near.into_iter()
        .filter_map(|other| Some((other, m.wall(other)?)))
        .filter(|(other, o)| *other != id && o.join && stacked(m, w, o))
        .filter(|(_, o)| u.dot((o.end - o.start).unit()).abs() <= 0.05)
        // Its own end must meet this wall's end: a wall merely running near the free end is no join.
        .filter(|(_, o)| o.start.dist(node) <= TOL || o.end.dist(node) <= TOL)
        .map(|(other, _)| other)
        .min()
}

/// The other walls joined to wall `id` at its start (or `at_end`, its end), each once, and whether it ends there on the
/// side of one of them (a tee).
pub(crate) fn joined(m: &Model, id: Id, at_end: bool) -> (Vec<Id>, bool) {
    let (arms, through) = arms(m, id, at_end, None, &|_| true, true, None);
    let mut others: Vec<Id> = arms.iter().map(|a| a.id).filter(|o| *o != id).collect();
    others.sort_unstable();
    others.dedup();
    (others, through)
}

/// A wall where it leaves a node: the way it goes, and where its faces' lines and its centre line
/// are there (each running that way): the face on the counter-clockwise side of that way, the one
/// on the clockwise side, and which wall's end it is (`id`, `at_end`). A run's arm at its end is one too ([`crate::runs`]),
/// naming no wall: its `id` is 0, below every element's.
#[derive(Default)]
pub(crate) struct Arm {
    pub(crate) id: Id,
    pub(crate) at_end: bool,
    pub(crate) mine: bool,
    pub(crate) way: P2,
    pub(crate) ccw: P2,
    pub(crate) cw: P2,
    pub(crate) centre: P2,
    pub(crate) thick: f64,
    /// Whether it is a slanted wall's: a join it takes part in is worked out the same way at every height.
    pub(crate) leans: bool,
    /// A curtain wall ending where a plain wall does: it doesn't mitre, the plain wall ends square against it.
    pub(crate) butts: bool,
}

/// Wall `w` leaving the node at its start, or with `at_end`, at its end.
/// At height `z`, a slanted wall's faces are where it has leant to there ([`faces_at`]); without one, at its base.
pub(crate) fn arm_at(m: &Model, id: Id, w: &Wall, at_end: bool, mine: bool, z: Option<f64>) -> Arm {
    let axis = Axis::of(w);
    let (ext, int) = faces_at(m, w, z);
    let s = if at_end { axis.length() } else { 0.0 };
    let t = axis.tangent(s);
    // Leaving from its end, it goes back: its interior face is on the counter-clockwise side.
    let (way, ccw, cw) = if at_end { (t * -1.0, int, ext) } else { (t, ext, int) };
    Arm {
        id,
        at_end,
        mine,
        way,
        ccw: axis.at(s, ccw),
        cw: axis.at(s, cw),
        centre: axis.at(s, (ext + int) / 2.0),
        thick: ext - int,
        leans: w.slant != 0.0,
        butts: false,
    }
}

/// The walls at wall `id`'s start (or `at_end`, its end), round the node counter-clockwise, its own
/// among them: those ending there, and a wall it ends on, both ways from where it does. Whether it
/// ends on one, too. With `physical`, respect Allow Join; plain-wall centre-line boundaries meet regardless.
/// At height `z`, slanted walls' faces are where they lean to there; without one, at their bases.
fn arms(
    m: &Model,
    id: Id,
    at_end: bool,
    index: Option<&Index>,
    keep: &impl Fn(&Wall) -> bool,
    physical: bool,
    z: Option<f64>,
) -> (Vec<Arm>, bool) {
    let Some(w) = m.wall(id) else { return (Vec::new(), false) };
    let node = if at_end { w.end } else { w.start };
    let mut arms = vec![arm_at(m, id, w, at_end, true, z)];
    // A wall standing in a curtain wall's cell joins nothing, and nothing it.
    if w.panel_of.is_some() {
        return (arms, false);
    }
    let mut through = false;
    // A wall that refuses to join meets nothing at this end: its own square end is its whole cap.
    if physical && !w.join {
        return (arms, false);
    }
    // A wall that refuses to join is no candidate for anyone either: neither side is mitred.
    let plain = crate::curtain::pattern(m, w).is_none();
    let joins = |o: &Wall| ((w.join && o.join) || (!physical && plain && crate::curtain::pattern(m, o).is_none())) && stacked(m, w, o);
    let butts = |o: &Wall| plain && crate::curtain::pattern(m, o).is_some();
    let candidates: Box<dyn Iterator<Item = (Id, &Wall)> + '_> = match index {
        Some(index) => Box::new(index.neighbours_at(id, node).into_iter().filter_map(|id| Some((id, m.wall(id)?)))),
        // The walls whose location lines come within the thickest wall's thickness of the node (those that end at it,
        // and those it ends on: the node within their thickness), of heights it overlaps (only those join it).
        None => {
            let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
            let at = ([node.0, node.1, node.0, node.1], m.base_z(w), m.top_z(w));
            let near = m.lookup().walls_near_at(&[at], thickest + 1.0);
            Box::new(near.into_iter().filter_map(|id| Some((id, m.wall(id)?))))
        }
    };
    // Where they meet is asked first: whether they overlap in height takes both walls' heights, under any roof or floor
    // they're attached to.
    for (other, o) in candidates {
        if other == id || !keep(o) || o.panel_of.is_some() {
            continue;
        }
        if o.start.dist(node) <= TOL {
            if joins(o) {
                arms.push(Arm { butts: butts(o), ..arm_at(m, other, o, false, false, z) });
            }
        } else if o.end.dist(node) <= TOL {
            if joins(o) {
                arms.push(Arm { butts: butts(o), ..arm_at(m, other, o, true, false, z) });
            }
        } else {
            // A wall it ends on: the node within its thickness, between its ends.
            // Whether it ends on the other is asked at their feet, so a slanted one leaning away from the node keeps the join all the
            // way up; its faces are where they have leant to.
            let ax = Axis::of(o);
            let (s, across) = ax.project(node);
            let (foot_ext, foot_int) = faces(m, o);
            let (ext, int) = faces_at(m, o, z);
            if s > TOL
                && s < ax.length() - TOL
                && (foot_int - TOL..=foot_ext + TOL).contains(&across)
                && ax.distance(node, across) <= TOL
                && joins(o)
            {
                let (t, (e, i), c) = (ax.tangent(s), (ax.at(s, ext), ax.at(s, int)), ax.at(s, (ext + int) / 2.0));
                let leans = o.slant != 0.0;
                arms.push(Arm {
                    id: other,
                    at_end: false,
                    mine: false,
                    way: t,
                    ccw: e,
                    cw: i,
                    centre: c,
                    thick: ext - int,
                    leans,
                    butts: false,
                });
                arms.push(Arm {
                    id: other,
                    at_end: false,
                    mine: false,
                    way: t * -1.0,
                    ccw: i,
                    cw: e,
                    centre: c,
                    thick: ext - int,
                    leans,
                    butts: false,
                });
                through = true;
            }
        }
    }
    arms.sort_by(|a, b| a.way.1.atan2(a.way.0).total_cmp(&b.way.1.atan2(b.way.0)));
    (arms, through)
}

/// The corner points that close wall `id`'s start, or with `at_end`, its end: from its face on the
/// clockwise side of the way it leaves the node round to the other. Walls ending where it ends meet
/// it at any angle, their faces running on until they cross (no farther from the node than the
/// thickest is thick, either way: past that, the end is square); a wall it ends on stops it at its
/// face. At a square compound Butt the shared cap follows the structure around the corner.
/// Where three or more walls meet, the node itself fills between them. A curtain wall takes no part in a mitre: its parts are
/// laid out on their own, and a plain wall ending at the end of one ends square at its far face.
pub fn cap(m: &Model, id: Id, at_end: bool) -> Vec<P2> {
    cap_in(m, id, at_end, None)
}

fn cap_in(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> Vec<P2> {
    end_geometry(m, id, at_end, index, &|_| true, None).outline
}

#[derive(Default)]
struct WallEnd {
    outline: Vec<P2>,
    butt: Option<compound::Butt>,
}

impl WallEnd {
    fn plain(outline: Vec<P2>) -> Self {
        Self { outline, butt: None }
    }
}

/// [`cap`] and how a compound corner shares it, joined only to walls `keep` keeps.
fn end_geometry(m: &Model, id: Id, at_end: bool, index: Option<&Index>, keep: &impl Fn(&Wall) -> bool, z: Option<f64>) -> WallEnd {
    let Some(w) = m.wall(id) else { return WallEnd::default() };
    let (arms, through) = arms(m, id, at_end, index, keep, true, z);
    let (against, arms): (Vec<Arm>, Vec<Arm>) = arms.into_iter().partition(|a| a.butts);
    let n = arms.len();
    let Some(i) = arms.iter().position(|a| a.id == id) else { return WallEnd::default() };
    let (prev, me, next) = (&arms[(i + n - 1) % n], &arms[i], &arms[(i + 1) % n]);
    if n < 2 {
        // Square, and against a curtain wall run on or back to its far face: its parts stand clear of any mitre.
        let reach = |a: &Arm| {
            [a.cw, a.ccw]
                .into_iter()
                .filter_map(|face| geom::line_intersection(me.centre, me.centre + me.way, face, face + a.way))
                .map(|c| (c - me.centre).dot(me.way))
                .filter(|d| d.abs() <= 2.0 * me.thick.max(a.thick))
                .fold(f64::MAX, f64::min)
        };
        let d = against.iter().map(reach).fold(f64::MAX, f64::min);
        return WallEnd::plain(if d < f64::MAX { vec![me.cw + me.way * d, me.ccw + me.way * d] } else { vec![me.cw, me.ccw] });
    }
    // A square compound corner is of upright walls: a slanted one mitres.
    let upright = |id: Id| m.wall(id).is_none_or(|x| x.slant == 0.0);
    if n == 2
        && !through
        && upright(me.id)
        && upright(next.id)
        && let Some(end) = compound::corner(m, me, next)
    {
        return end;
    }
    let node = if at_end { w.end } else { w.start };
    let limit = arms.iter().map(|a| a.thick).fold(0.0, f64::max);
    // Where a wall leans, the walls' centre lines cross elsewhere at this height than at their foot: the corner is there, and
    // how near it a crossing of faces must be is measured from there, so a corner is the same shape at every height. It is the
    // same point for every wall at the node: where each two neighbours' centre lines cross, taken together. Walls leaning apart
    // may cross far off, which is no corner: then each ends at its own end, square, at that height.
    // Half the shortest wall there: as far as a wall it ends on stops it.
    let half = arms.iter().filter_map(|a| m.wall(a.id)).map(|o| Axis::of(o).length() / 2.0).fold(f64::INFINITY, f64::min);
    // A join of slanted walls is worked out one way at every height, its foot too, so it keeps its shape as it rises: from where
    // the centre lines cross, a face's crossing allowed twice as far, as the corner moves among walls leaning unlike.
    let leaning = arms.iter().any(|a| a.leans);
    // How far the corner may move off the node: three times as far as the walls lean at their tops, and their thickness. It moves
    // as far again as they lean square to each other, more as they near parallel: past this it races off, a spike. Judged by
    // their tops, it is the same answer at every height.
    let top_lean = arms
        .iter()
        .filter(|a| a.leans)
        .filter_map(|a| m.wall(a.id))
        .map(|o| lean(m, o, m.top_z(o)).abs().max(lean(m, o, m.base_z(o)).abs()))
        .fold(0.0, f64::max);
    let hub = if leaning {
        let reach = 3.0 * (top_lean + limit);
        let pairs = if n > 2 { n } else { 1 };
        let crossings: Vec<P2> = (0..pairs)
            .filter_map(|k| {
                let (a, b) = (&arms[k], &arms[(k + 1) % n]);
                (a.id != b.id || a.at_end != b.at_end)
                    .then(|| geom::line_intersection(a.centre, a.centre + a.way, b.centre, b.centre + b.way))
                    .flatten()
            })
            .filter(|c| c.dist(node) <= reach)
            .collect();
        if crossings.is_empty() { me.centre } else { crossings.iter().fold(P2(0.0, 0.0), |a, c| a + *c) * (1.0 / crossings.len() as f64) }
    } else {
        node
    };
    // Against a wall it ends on, its face is the stop however sharp the angle, as far as the wall reaches; between ends, a
    // crossing must be near the corner.
    let limit = if through {
        (5.0 * limit).max(half)
    } else if leaning {
        2.0 * limit
    } else {
        limit
    };
    // Where a face of this wall crosses a neighbour's facing face, near enough to the corner either way; else its own
    // end. A crossing far along the wall is no corner at all: two walls meeting at a shallow angle cross metres in.
    let meet = |face: P2, other: P2, other_way: P2| {
        geom::line_intersection(face, face + me.way, other, other + other_way)
            // A face crossing exactly at the thickness can round just past it.
            .filter(|c| (*c - hub).dot(me.way).abs() <= limit + 1e-6)
            .unwrap_or(face)
    };
    let (cw, ccw) = (meet(me.cw, prev.ccw, prev.way), meet(me.ccw, next.cw, next.way));
    WallEnd::plain(if through { vec![cw, ccw] } else { vec![cw, hub, ccw] })
}

/// [`cap`] at height `z`: joined only to the walls standing there. Where a wall it mitres with stops below (or starts
/// above) `z` and nothing else joins it there, its end is square, as far out as the whole corner reaches, so the taller wall
/// closes the corner over the lower one instead of showing its mitre.
pub fn cap_at(m: &Model, id: Id, at_end: bool, z: f64) -> Vec<P2> {
    cap_at_in(m, id, at_end, None, z)
}

pub(crate) fn cap_at_in(m: &Model, id: Id, at_end: bool, index: Option<&Index>, z: f64) -> Vec<P2> {
    cap_with(m, id, at_end, index, z, z)
}

/// The cap at height `z`, joined to the walls standing at height `standing`: in a band of height whose joins are the same, its
/// cap at each of its two ends, the same points moved as the walls lean.
pub(crate) fn cap_with(m: &Model, id: Id, at_end: bool, index: Option<&Index>, z: f64, standing: f64) -> Vec<P2> {
    let stands = |o: &Wall| m.base_z(o) <= standing && standing < m.top_z(o);
    // Most ends join only walls standing at every height they do: their cap is the whole one.
    let (all_arms, _) = arms(m, id, at_end, index, &|_| true, true, Some(z));
    let missing = all_arms.iter().filter(|a| a.id != id && !m.wall(a.id).is_some_and(stands)).count();
    let full = end_geometry(m, id, at_end, index, &|_| true, Some(z));
    let Some(w) = m.wall(id).filter(|_| missing > 0) else { return full.outline };
    // The arms left where some stop: an arm is a wall's end, or a wall it ends on twice (once each way).
    let here = all_arms.iter().filter(|a| a.id == id || m.wall(a.id).is_some_and(stands)).filter(|a| !a.butts).count();
    if here >= 2 {
        return end_geometry(m, id, at_end, index, &stands, Some(z)).outline;
    }
    // Free here: square, out to the farthest the corner below reaches (back from the way it leaves the node).
    let me = arm_at(m, id, w, at_end, true, Some(z));
    let reach = full.outline.iter().map(|p| (*p - me.centre).dot(me.way)).fold(f64::INFINITY, f64::min);
    let reach = if reach.is_finite() { reach } else { 0.0 };
    vec![me.cw + me.way * reach, me.ccw + me.way * reach]
}

/// The heights between wall `id`'s base and top where what its ends join changes: the bases and tops of the walls joined
/// to it that stop inside its height. Its cap is the same from one to the next ([`cap_at`]).
pub(crate) fn cap_heights(m: &Model, id: Id, index: Option<&Index>) -> Vec<f64> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    if !w.join || w.panel_of.is_some() {
        return Vec::new();
    }
    let (lo, hi) = (m.base_z(w), m.top_z(w));
    let mut zs = Vec::new();
    for at_end in [false, true] {
        let (arms, _) = arms(m, id, at_end, index, &|_| true, true, None);
        for a in arms.iter().filter(|a| a.id != id && a.id != 0) {
            let Some(o) = m.wall(a.id) else { continue };
            for z in [m.base_z(o), m.top_z(o)] {
                if z.is_finite() && z > lo + TOL && z < hi - TOL {
                    zs.push(z);
                }
            }
        }
    }
    zs.sort_by(f64::total_cmp);
    zs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    zs
}

/// What a plan cutting at height `z` cuts of wall `id`: its outline there ([`outline_at`]), in parts where an edited profile
/// leaves it only some stretches at that height.
pub(crate) fn plan_cut(m: &Model, id: Id, index: Option<&Index>, z: f64) -> Vec<Vec<P2>> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    profile::clip(w, z - m.own_base(w), &outline_at_in(m, id, index, z))
}

/// Wall `id`'s outline at height `z`: [`outline`], its caps as [`cap_at`] has them there.
pub fn outline_at(m: &Model, id: Id, z: f64) -> Vec<P2> {
    outline_at_in(m, id, None, z)
}

pub(crate) fn outline_at_in(m: &Model, id: Id, index: Option<&Index>, z: f64) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let (ext, int) = faces_at(m, w, Some(z));
    let len = axis.length();
    let between = |pts: Vec<P2>| {
        let n = pts.len();
        pts.into_iter().skip(1).take(n.saturating_sub(2))
    };
    let mut ring = cap_at_in(m, id, false, index, z);
    ring.extend(between(axis.points(0.0, len, ext)));
    ring.extend(cap_at_in(m, id, true, index, z));
    ring.extend(between(axis.points(len, 0.0, int)));
    ring.dedup_by(|a, b| a.dist(*b) < 1e-6);
    ring
}

/// Wall `id`'s centre line, an arc as chords: each end it joins others at run on (or back) to the
/// nearest of their centre lines, so rooms close at the centre lines whatever line walls stand on. Plain walls' centre
/// lines meet whether or not they allow joins: Allow Join decides only their physical caps.
pub fn centre_line(m: &Model, id: Id) -> Vec<P2> {
    centre_line_in(m, id, None)
}

/// [`centre_line`], its joins found through `index` when there is one.
pub(crate) fn centre_line_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<P2> {
    centre_line_filtered(m, id, index, &|_| true)
}

/// A centre line joined only to walls kept by the caller, such as those bounding a level at its computation height.
pub(crate) fn centre_line_filtered(m: &Model, id: Id, index: Option<&Index>, keep: &impl Fn(&Wall) -> bool) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let mut line = axis.points(0.0, axis.length(), centre(m, w));
    for at_end in [false, true] {
        let (arms, _) = arms(m, id, at_end, index, keep, false, None);
        let Some(me) = arms.iter().find(|a| a.id == id) else { continue };
        let reach = 2.0 * arms.iter().map(|a| a.thick).fold(1.0, f64::max);
        let crossing = arms
            .iter()
            .filter(|a| a.id != id)
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
    outline_in(m, id, None)
}

pub(crate) fn outline_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<P2> {
    outline_filtered(m, id, index, &|_| true)
}

/// An outline whose caps join only walls kept by the caller; physical wall geometry still uses every overlapping wall.
pub(crate) fn outline_filtered(m: &Model, id: Id, index: Option<&Index>, keep: &impl Fn(&Wall) -> bool) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    let between = |pts: Vec<P2>| {
        let n = pts.len();
        pts.into_iter().skip(1).take(n.saturating_sub(2))
    };
    let mut ring = end_geometry(m, id, false, index, keep, None).outline;
    ring.extend(between(axis.points(0.0, len, ext)));
    ring.extend(end_geometry(m, id, true, index, keep, None).outline);
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
    layer_lines_in(m, id, None)
}

pub(crate) fn layer_lines_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<Vec<P2>> {
    layer_geometry(m, id, index).lines
}

#[derive(Default)]
pub(crate) struct LayerGeometry {
    pub(crate) lines: Vec<Vec<P2>>,
    // A core can reach beyond the finish beside it at either end.
    butt: [Option<compound::Butt>; 2],
}

impl LayerGeometry {
    pub(crate) fn joined(&self) -> bool {
        self.butt.iter().any(Option::is_some)
    }

    pub(crate) fn band(&self, k: usize) -> Vec<P2> {
        let a = &self.lines[k];
        let b = &self.lines[k + 1];
        let mut ring: Vec<_> = a.iter().chain(b.iter().rev()).copied().collect();
        if k == 1 {
            let n = ring.len();
            if let Some(start) = &self.butt[0] {
                [ring[0], ring[n - 1]] = start.core;
            }
            if let Some(end) = &self.butt[1] {
                [ring[a.len() - 1], ring[a.len()]] = end.core;
            }
        }
        ring
    }

    pub(crate) fn interfaces(&self) -> impl Iterator<Item = &[P2]> {
        self.lines
            .iter()
            .skip(1)
            .take(self.lines.len().saturating_sub(2))
            .map(Vec::as_slice)
            .chain(self.butt.iter().flatten().filter(|b| b.draw_seam).map(|b| b.core.as_slice()))
    }
}

pub(crate) fn layer_geometry(m: &Model, id: Id, index: Option<&Index>) -> LayerGeometry {
    layer_geometry_at(m, id, index, None)
}

/// [`layer_geometry`] at height `z`, its ends as [`cap_at`] has them there; at every height, without one.
pub(crate) fn layer_geometry_at(m: &Model, id: Id, index: Option<&Index>, z: Option<f64>) -> LayerGeometry {
    let Some(w) = m.wall(id) else { return LayerGeometry::default() };
    let axis = Axis::of(w);
    let len = axis.length();
    let end = |at_end: bool| {
        let whole = end_geometry(m, id, at_end, index, &|_| true, z);
        match z {
            Some(z) => {
                let here = cap_at_in(m, id, at_end, index, z);
                if here == whole.outline { whole } else { WallEnd::plain(here) }
            }
            None => whole,
        }
    };
    let ends = [end(false), end(true)];
    // A slanted wall's layers lean with its faces.
    let (ext, _) = faces(m, w);
    let (ext_z, int_z) = faces_at(m, w, z);
    let thick = m.thickness(w);
    let wide = if thick > 0.0 { (ext_z - int_z) / thick } else { 1.0 };
    let lines = boundaries(m, w)
        .into_iter()
        .map(|off| ext_z - (ext - off) * wide)
        .enumerate()
        .map(|(k, off)| {
            let mut line = axis.points(0.0, len, off);
            for (i, end) in ends.iter().enumerate() {
                let last = line.len() - 1;
                line[if i == 0 { 0 } else { last }] = match &end.butt {
                    Some(b) => b.turns[k],
                    None => on_cap(&axis, &end.outline, i == 1, off),
                };
            }
            line
        })
        .collect();
    LayerGeometry { lines, butt: ends.map(|e| e.butt) }
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
            // From below its underside (a shaped one's level under a variable layer), from above its top.
            Some(Element::Roof(r)) => m
                .roof_prisms(r)
                .into_iter()
                .flat_map(|(ring, bottom, up)| {
                    let plane = if top { up } else { bottom };
                    convex_pieces(&ring).into_iter().map(move |c| (c, plane))
                })
                .collect(),
            // A shaped one's underside under a variable layer: level.
            Some(Element::Floor(f)) if !top && m.floor_under(f).is_some() => {
                let under = m.floor_under(f).map_or(Plane::flat(0.0), |(p, _)| p);
                convex_pieces(&f.boundary).into_iter().map(|c| (c, under)).collect()
            }
            // A shaped one's triangles, each its own plane.
            Some(Element::Floor(f)) if !f.shape.is_empty() => {
                let depth = if top { 0.0 } else { m.floor_depth(f) };
                m.shape_triangles(f).into_iter().map(|(t, p)| (t, Plane { z: p.z - depth, ..p })).collect()
            }
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

#[cfg(test)]
thread_local! {
    /// The polygons cut by [`split`] on this thread: the tests of what is drawn assert on it, not on time, which varies with the
    /// load of the machine.
    pub(crate) static SPLITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `ring` cut by the convex counter-clockwise `piece`: the part inside it (if any), and the parts
/// outside, apart. A repeated point of the piece's is no edge to cut by.
pub(crate) fn split(ring: Vec<P2>, piece: &[P2]) -> (Option<Vec<P2>>, Vec<Vec<P2>>) {
    #[cfg(test)]
    SPLITS.with(|n| n.set(n.get() + 1));
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
    // The box round a ring: a piece clear of the hole's piece's box is left whole, without cutting it (a big concave ring and
    // a big concave hole make hundreds of pieces each, and cutting each by each is cubic).
    let extent = |ring: &[P2]| {
        ring.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        })
    };
    let holes: Vec<_> = convex_pieces(hole).into_iter().map(|p| (extent(&p), p)).collect();
    rings
        .iter()
        .flat_map(|r| {
            holes.iter().fold(convex_pieces(r), |parts, ((lo, hi), piece)| {
                parts
                    .into_iter()
                    .flat_map(|p| {
                        let (a, b) = extent(&p);
                        if b.0 < lo.0 || a.0 > hi.0 || b.1 < lo.1 || a.1 > hi.1 {
                            Some(p).filter(|p| p.len() >= 3 && geom::area(p).abs() > 1.0).into_iter().collect::<Vec<_>>()
                        } else {
                            split(p, piece).1
                        }
                    })
                    .collect()
            })
        })
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

/// A wall's base under the point `s` mm along its centre line.
pub fn base_at(m: &Model, w: &Wall, s: f64) -> f64 {
    bounds(m, w).0.at(Axis::of(w).at(s, centre(m, w)))
}

/// A wall's top over plan point `q`: what it's attached to there, else its own top.
pub(crate) fn top_over(m: &Model, w: &Wall, q: P2) -> f64 {
    bounds(m, w).1.at(q)
}

/// A wall's top over the point `s` mm along its centre line: of an edited profile, the highest it reaches there.
pub fn top_at(m: &Model, w: &Wall, s: f64) -> f64 {
    if let Some(up) = profile::top_at(w, s) {
        return m.own_base(w) + up;
    }
    bounds(m, w).1.at(Axis::of(w).at(s, centre(m, w)))
}

/// Along the line `off` mm to the left of straight wall `w`'s location line, from `s0` to `s1`: its base (with `top`, its
/// top) as straight pieces `[from, to, height at from, height at to]`, in order, a new one wherever it changes plane.
pub(crate) fn profile(m: &Model, w: &Wall, top: bool, off: f64, s0: f64, s1: f64) -> Vec<[f64; 4]> {
    let (base, up) = bounds(m, w);
    let bound = if top { up } else { base };
    let axis = Axis::of(w);
    let (p, q) = (axis.at(s0, off), axis.at(s1, off));
    let mut cuts = vec![s0, s1];
    for (ring, _) in &bound.regions {
        cuts.extend(geom::ring_edges(ring).filter_map(|(a, b)| geom::crossing(p, q, a, b)).map(|t| s0 + t * (s1 - s0)));
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let mut out: Vec<[f64; 4]> = Vec::new();
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        let plane = bound.over(axis.at((a + b) / 2.0, off)).unwrap_or(Plane::flat(bound.own));
        let (za, zb) = (plane.at(axis.at(a, off)), plane.at(axis.at(b, off)));
        // One piece on with the last where it goes on in the same line.
        match out.last_mut() {
            Some(last) if (last[3] - za).abs() < 1e-6 && ((last[3] - last[2]) / (last[1] - last[0]) - (zb - za) / (b - a)).abs() < 1e-9 => {
                [last[1], last[3]] = [b, zb];
            }
            _ => out.push([a, b, za, zb]),
        }
    }
    out
}

/// A wall's lowest base and highest top along its faces and its centre line, where what it's
/// attached to bends (sampled: a ridge along it may fall between samples).
pub fn heights(m: &Model, w: &Wall) -> (f64, f64) {
    if let Some((lo, hi)) = profile::extent(w) {
        let base = m.own_base(w);
        return (base + lo, base + hi);
    }
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

/// A door's or a window's hole in its host wall, in the wall's own frame: `s0..s1` along its axis,
/// its centre line at 0, and `bottom..top` above z 0, its base plus the opening's sill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub s0: f64,
    pub s1: f64,
    pub bottom: f64,
    pub top: f64,
}

/// The hole `o` cuts in wall `w`: its type's width either side of its middle and its height above its bottom
/// (`Model::opening_middle`). Which jamb a leaf is hinged on, and which way it opens, is the opening's, not the hole's.
pub fn hole(m: &Model, w: &Wall, e: &Element, o: &Opening) -> Option<Hole> {
    let t = m.opening_type(e)?;
    let (middle, sill) = m.opening_middle(o);
    let bottom = m.own_base(w) + sill;
    Some(Hole { s0: middle - t.width / 2.0, s1: middle + t.width / 2.0, bottom, top: bottom + t.height })
}

/// A column's base and top: what it's attached to, or a level plane.
fn column_bounds(m: &Model, col: &crate::model::Column) -> (Bound, Bound) {
    (Bound::of(m, col.attach_base, true, m.column_own_base(col)), Bound::of(m, col.attach_top, false, m.column_own_top(col)))
}

/// A column's lowest base and highest top over its outline, where what it's attached to bends.
pub(crate) fn column_heights(m: &Model, e: &Element, col: &crate::model::Column) -> (f64, f64) {
    if col.attach_base.is_none() && col.attach_top.is_none() {
        return (m.column_own_base(col), m.column_own_top(col));
    }
    let (base, top) = column_bounds(m, col);
    let mut pts = m.column_outline(e);
    pts.push(col.at);
    let lo = pts.iter().map(|q| base.at(*q)).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|q| top.at(*q)).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

/// An attached column's solid: its outline cut where what it stands on or follows changes plane, each part from the one to the
/// other. None for one attached to nothing, which is a plain slab.
pub fn column_prisms(m: &Model, e: &Element) -> Vec<Prism> {
    let Some(col) = e.column().filter(|c| c.attach_base.is_some() || c.attach_top.is_some()) else { return Vec::new() };
    let (base, top) = column_bounds(m, col);
    // A tube's pieces round its hole, as an unattached one's are.
    let pieces = if matches!(e, Element::Column(_)) { vec![m.column_outline(e)] } else { m.column_pieces(e) };
    let mut out = Vec::new();
    for outline in pieces {
        for (part, bottom) in base.parts(outline) {
            for (ring, up) in top.parts(part) {
                out.push(Prism { ring, bottom, top: up, curve: None });
            }
        }
    }
    out
}

/// The sides of the notches wall `id`'s doors and windows cut where they rise past its top, as Revit lets them: a line across
/// the wall from face to face at each jamb whose head reaches the top there, which a plan seeing the wall from above draws.
pub fn notches(m: &Model, id: Id) -> Vec<[P2; 2]> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let guid = m.guid(id);
    let (axis, (ext, int)) = (Axis::of(w), faces(m, w));
    let mut out = Vec::new();
    for e in m.lookup().referrers(&guid).filter_map(|k| m.elements.get(k)) {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let Some(h) = hole(m, w, e, o).filter(|_| o.host == guid) else { continue };
        for s in [h.s0, h.s1] {
            if h.top >= top_at(m, w, s) - 1e-6 {
                out.push([axis.at(s, int), axis.at(s, ext)]);
            }
        }
    }
    out
}

/// A part of a wall's solid: a ring in plan, from a bottom plane up to a top one; of a curved wall,
/// with its centre and its centre line's radius.
#[derive(Debug, PartialEq)]
pub struct Prism {
    pub ring: Vec<P2>,
    pub bottom: Plane,
    pub top: Plane,
    pub curve: Option<(P2, f64)>,
}

/// A prism's volume, mm³: signed triangles of its plan ring times their mean height between the planes.
pub fn prism_volume(p: &Prism) -> f64 {
    let Some(&o) = p.ring.first() else { return 0.0 };
    let rise = |q| p.top.at(q) - p.bottom.at(q);
    p.ring.windows(2).skip(1).map(|w| (w[0] - o).cross(w[1] - o) / 2.0 * (rise(o) + rise(w[0]) + rise(w[1])) / 3.0).sum::<f64>().abs()
}

/// Wall `id` as prisms: its outline cut across where its doors and windows start and end (with
/// `openings`) and along an arc into short straight pieces, each piece from its base up to its
/// top less the openings over it, cut where what it's attached to changes plane. An opening whose family cuts its wall
/// with voids of its own ([`host_voids_in`]) leaves no rectangle: the voids cut what is left, as the 3D view's solids do.
pub fn prisms(m: &Model, id: Id, openings: bool) -> Vec<Prism> {
    prisms_in(m, id, openings, None)
}

pub(crate) fn prisms_in(m: &Model, id: Id, openings: bool, index: Option<&Index>) -> Vec<Prism> {
    prisms_holed(m, id, openings, true, index)
}

/// [`prisms_in`], every opening its rectangle, voids or none: what stands where cutting by the voids fails.
pub(crate) fn prisms_rect_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<Prism> {
    prisms_holed(m, id, true, false, index)
}

/// The doors and windows in wall `id`, or the openings `index` lists in it.
fn openings_of<'a>(m: &'a Model, id: Id, index: Option<&'a Index>) -> Box<dyn Iterator<Item = &'a Element> + 'a> {
    let guid = m.guid(id);
    match index {
        Some(index) => Box::new(index.openings(guid).iter().filter_map(|id| m.elements.get(*id))),
        None => Box::new(m.elements.values().filter(move |e| matches!(e, Element::Door(o) | Element::Window(o) if o.host == guid))),
    }
}

/// The voids wall `id`'s doors and windows cut it with, in the model's axes ([`Model::opening_voids`]): those of the
/// openings whose families have their own, which cut the wall in their shape in place of a rectangle; and those of the
/// components on its faces ([`Model::hosted_voids`]).
pub(crate) fn host_voids_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<Vec<[crate::view3d::P3; 3]>> {
    let mut out: Vec<_> = openings_of(m, id, index).flat_map(|e| m.opening_voids(e)).collect();
    out.extend(m.hosted_voids(id).into_iter().flat_map(|(_, v)| v));
    out
}

fn prisms_holed(m: &Model, id: Id, openings: bool, voided: bool, index: Option<&Index>) -> Vec<Prism> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let guid = m.guid(id);
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    // The openings: along the wall, and up it; one whose voids cut the wall leaves its rectangle to them.
    let candidates: Box<dyn Iterator<Item = &Element> + '_> =
        if !openings { Box::new(std::iter::empty()) } else { openings_of(m, id, index) };
    let holes: Vec<Hole> = candidates
        .filter_map(|e| match e {
            Element::Door(o) | Element::Window(o) if o.host == guid => {
                if voided && !m.opening_voids_local(e).is_empty() {
                    return None;
                }
                hole(m, w, e, o)
            }
            _ => None,
        })
        .collect();
    let mut cuts = axis.stations(0.0, len);
    cuts.extend(holes.iter().flat_map(|h| [h.s0, h.s1]).filter(|s| *s > 0.0 && *s < len));
    // An edited profile turns at its corners: each stretch between two has its own bottom and top.
    cuts.extend(profile::stations(w).into_iter().filter(|s| *s > 0.0 && *s < len));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let (start, end) = (cap_in(m, id, false, index), cap_in(m, id, true, index));
    // Where a wall it joins stops within its height, its ends change there: each band of height its own caps.
    let bands = cap_heights(m, id, index);
    let (base, top) = bounds(m, w);
    let curve = axis.centre().map(|c| (c, axis.at(0.0, (ext + int) / 2.0).dist(c)));
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        let ring_at = |z: Option<f64>| {
            let cap = |at_end: bool, whole: &Vec<P2>| match z {
                Some(z) if !bands.is_empty() => cap_at_in(m, id, at_end, index, z),
                _ => whole.clone(),
            };
            let mut ring = if a <= 0.0 { cap(false, &start) } else { vec![axis.at(a, int), axis.at(a, ext)] };
            ring.extend(if b >= len { cap(true, &end) } else { vec![axis.at(b, ext), axis.at(b, int)] });
            ring
        };
        let ends = a <= 0.0 || b >= len;
        let ring = ring_at(None);
        // Up from its base to its top, past the openings over its middle.
        let mid = (a + b) / 2.0;
        let mut gaps: Vec<(f64, f64)> = holes.iter().filter(|h| h.s0 <= mid && mid <= h.s1).map(|h| (h.bottom, h.top)).collect();
        if ends {
            // A band's edge is a gap of no height, but for one within an opening, which is no wall at all.
            let open = |z: f64| gaps.iter().any(|(g0, g1)| *g0 <= z && z <= *g1);
            let edges: Vec<f64> = bands.iter().copied().filter(|z| !open(*z)).collect();
            gaps.extend(edges.into_iter().map(|z| (z, z)));
        }
        gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
        if !w.profile.is_empty() {
            // Each stretch the profile covers here, from its edge below to its edge above, less the openings. At its ends, each
            // band of height between where the walls it joins stop has its own caps: the band's limits are gaps too.
            let real: Vec<(f64, f64)> = gaps.iter().copied().filter(|(g0, g1)| g1 > g0).collect();
            let mut limits = vec![f64::NEG_INFINITY];
            if ends {
                limits.extend(bands.iter().copied());
            }
            limits.push(f64::INFINITY);
            for (bottom, top) in profile::planes_at(m, w, mid) {
                for pair in limits.windows(2) {
                    let &[lo, hi] = pair else { continue };
                    let ring = match (lo.is_finite(), hi.is_finite()) {
                        (false, false) => ring.clone(),
                        (true, true) => ring_at(Some((lo + hi) / 2.0)),
                        (true, false) => ring_at(Some(lo + 1.0)),
                        (false, true) => ring_at(Some(hi - 1.0)),
                    };
                    let mut g = real.clone();
                    if lo.is_finite() {
                        g.push((-FAR, lo));
                    }
                    if hi.is_finite() {
                        g.push((hi, FAR));
                    }
                    g.sort_by(|x, y| x.0.total_cmp(&y.0));
                    out.extend(profile::stack(ring, bottom, top, &g).into_iter().map(|(ring, bottom, top)| Prism {
                        ring,
                        bottom,
                        top,
                        curve,
                    }));
                }
            }
            continue;
        }
        let flat = |z: f64| Bound { regions: Vec::new(), own: z };
        let mut floors: Vec<Bound> = vec![base.clone()];
        let mut ceilings: Vec<Bound> = Vec::new();
        for (g0, g1) in gaps {
            ceilings.push(flat(g0));
            floors.push(flat(g1));
        }
        ceilings.push(top.clone());
        let middle = axis.at(mid, (ext + int) / 2.0);
        let last = ceilings.len().saturating_sub(1);
        for (k, (under, over)) in floors.iter().zip(&ceilings).enumerate() {
            let ring = if ends && !bands.is_empty() { ring_at(Some((under.at(middle) + over.at(middle)) / 2.0)) } else { ring.clone() };
            for (part, bottom) in under.parts(ring) {
                for (part, up) in over.parts(part) {
                    let mut pieces = vec![(part, bottom, up)];
                    // A level edge (an opening's, or where a wall it joins stops) holds only under the wall's own top and over its
                    // base: where a roof it follows dips under it, the roof is the top there.
                    if k < last {
                        pieces =
                            pieces.into_iter().flat_map(|(r, b, t)| below(&top, r, t.z).into_iter().map(move |(r, t)| (r, b, t))).collect();
                    }
                    if k > 0 {
                        pieces = pieces
                            .into_iter()
                            .flat_map(|(r, b, t)| above(&base, r, b.z).into_iter().map(move |(r, b)| (r, b, t)))
                            .collect();
                    }
                    for (part, bottom, top) in pieces {
                        // Only where it has some height.
                        let part = geom::clip(&part, |q| top.at(q) - bottom.at(q));
                        if part.len() >= 3 && geom::area(&part).abs() > 1.0 {
                            out.push(Prism { ring: part, bottom, top, curve });
                        }
                    }
                }
            }
        }
    }
    out
}

/// `ring` under the level `z`, but where `top` is lower: the parts, each with the plane over it, `top`'s or the level's.
fn below(top: &Bound, ring: Vec<P2>, z: f64) -> Vec<(Vec<P2>, Plane)> {
    if ring.iter().all(|q| top.at(*q) >= z - 1e-9) {
        return vec![(ring, Plane::flat(z))];
    }
    top.parts(ring).into_iter().flat_map(|(r, real)| profile::split(r, real, z, true)).collect()
}

/// `ring` over the level `z`, but where `base` is higher: the parts, each with the plane under it, `base`'s or the level's.
fn above(base: &Bound, ring: Vec<P2>, z: f64) -> Vec<(Vec<P2>, Plane)> {
    if ring.iter().all(|q| base.at(*q) <= z + 1e-9) {
        return vec![(ring, Plane::flat(z))];
    }
    base.parts(ring).into_iter().flat_map(|(r, real)| profile::split(r, real, z, false)).collect()
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
    fn a_taller_wall_closes_the_corner_square_over_the_lower_one_it_mitres_with() {
        // An L: the east wall 3000 high, the north one 6000. Below 3000 they mitre; above, the north wall's end is square,
        // flush with the east wall's outer face, with no 45° face over the lower wall's top.
        let s = session(
            r#"wall {"points": [[4000, 0], [0, 0]], "height": 3000}
               wall {"points": [[0, 0], [0, 4000]], "height": 6000}"#,
        );
        let tall = 100003;
        let below = cap_at(&s.model, tall, false, 1500.0);
        assert_eq!(below, cap(&s.model, tall, false), "mitred where both stand");
        assert_eq!(below.len(), 3, "{below:?}");
        let above = cap_at(&s.model, tall, false, 4500.0);
        assert_eq!(above.len(), 2, "square: {above:?}");
        // Square across the wall at the lower wall's outer face, 100 mm south of the corner.
        assert!(above.iter().all(|p| (p.1 + 100.0).abs() < 1e-6), "{above:?}");
        assert!(above.iter().any(|p| near(*p, P2(-100.0, -100.0))) && above.iter().any(|p| near(*p, P2(100.0, -100.0))), "{above:?}");
        // The lower wall never changes.
        assert_eq!(cap_at(&s.model, 100002, true, 1500.0), cap(&s.model, 100002, true));
        // Its solid: prisms below 3000 end in the mitre, those above square; together, the whole wall.
        let prisms = prisms(&s.model, tall, false);
        assert_eq!(prisms.len(), 2, "{prisms:?}");
        let high = prisms.iter().find(|p| p.bottom.at(P2(0.0, 0.0)) > 2999.0).unwrap_or_else(|| panic!("{prisms:?}"));
        assert!(high.ring.iter().all(|p| p.1 > -100.0 - 1e-6), "{:?}", high.ring);
        assert!(!high.ring.iter().any(|p| near(*p, P2(0.0, 0.0))), "no mitre's node up there: {:?}", high.ring);
        let low = prisms.iter().find(|p| p.top.at(P2(0.0, 0.0)) < 3001.0).unwrap_or_else(|| panic!("{prisms:?}"));
        assert!(low.ring.iter().any(|p| near(*p, P2(0.0, 0.0))), "{:?}", low.ring);
        // A plan cutting above the lower wall sees the square end.
        assert_eq!(outline_at(&s.model, tall, 4500.0).len(), 4);
    }

    #[test]
    fn an_upright_wall_meets_a_slanted_one_where_it_has_leant_to_with_no_spike_to_its_foot() {
        // A slanted wall east along y = 0 leaning 15° north, and an upright one north from its start. At 1200 mm the slanted wall
        // has leant 322 mm north: the upright wall's end meets it there, and nothing of it reaches back to the corner at its foot.
        let s = session(
            r#"wall {"points": [[4000, 0], [0, 0]], "height": 3000, "slant": -15}
               wall {"points": [[0, 0], [0, 4000]], "height": 3000}"#,
        );
        let lean = 1200.0 * 15f64.to_radians().tan();
        let half = 100.0 / 15f64.to_radians().cos();
        for id in [100002, 100003] {
            let o = outline_at(&s.model, id, 1200.0);
            let low = o.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            assert!(low > lean - half - 1e-6, "{id}: {o:?}");
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
    fn walls_meeting_at_a_shallow_angle_square_off_rather_than_run_on_metres() {
        // 7.5° apart: their faces would cross 1.5 m along each wall, which is no corner but a wedge. A wall's end goes
        // square instead, no farther from the node than the thickest wall there is thick.
        let s = corner(7.5);
        for id in [100002, 100003] {
            let o = outline(&s.model, id);
            assert!(o.iter().all(|p| p.length() < 300.0 || p.length() > 3000.0), "{id}: {o:?}");
        }
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
    fn a_walls_core_faces_lie_where_its_layers_put_them() {
        let s = session(
            r#"type {"category": "wall", "name": "C60", "layers": [{"function": "finish", "thickness": 50}, {"function": "structure", "thickness": 100}, {"function": "finish", "thickness": 50}]}
               wall {"points": [[0, 0], [5000, 0]], "type": "C60"}
               wall {"points": [[0, 2000], [5000, 2000]], "type": "C60", "location": "exterior-finish"}
               wall {"points": [[0, 4000], [5000, 4000]], "type": "C60", "location": "interior-core"}
               wall {"points": [[0, 6000], [5000, 6000]], "type": "C60", "location": "exterior-core"}"#,
        );
        let m = &s.model;
        let ids: Vec<Id> = m.elements.keys().copied().filter(|id| m.wall(*id).is_some()).collect();
        let at = |id| {
            let w = m.wall(id).unwrap_or_else(|| panic!("no wall {id}"));
            (core_faces(m, w), face_offset(m, w, Face::Centre), faces(m, w))
        };
        assert_eq!(at(ids[0]), (Some((50.0, -50.0)), Some(0.0), (100.0, -100.0)));
        assert_eq!(at(ids[1]), (Some((-50.0, -150.0)), Some(-100.0), (0.0, -200.0)));
        assert_eq!(at(ids[2]), (Some((100.0, 0.0)), Some(50.0), (150.0, -50.0)));
        assert_eq!(at(ids[3]), (Some((0.0, -100.0)), Some(-50.0), (50.0, -150.0)));
    }

    #[test]
    fn a_wall_without_a_core_falls_back_to_its_finish_face() {
        let s = session(r#"wall {"points": [[0, 0], [5000, 0]]}"#);
        let m = &s.model;
        let w = m.wall(100002).unwrap_or_else(|| panic!("no wall"));
        assert_eq!(core_faces(m, w), None);
        let line = face_line(m, 100002, Face::ExteriorCore, 0.0).unwrap_or_else(|| panic!("no line"));
        assert!(near(line.start(), P2(0.0, 100.0)) && near(line.end(), P2(5000.0, 100.0)), "{line:?}");
        assert!(face_line(m, 100002, Face::ExteriorFinish, 0.0).is_some(), "the finish is there");
    }

    #[test]
    fn face_lines_of_walls_meeting_at_a_corner_cross_on_the_same_point() {
        let s = session(
            r#"type {"category": "wall", "name": "C60", "layers": [{"function": "finish", "thickness": 50}, {"function": "structure", "thickness": 100}, {"function": "finish", "thickness": 50}]}
               wall {"points": [[0, 0], [4000, 0]], "type": "C60"}
               wall {"points": [[0, 0], [0, -4000]], "type": "C60"}"#,
        );
        let m = &s.model;
        let ids: Vec<Id> = m.elements.keys().copied().filter(|id| m.wall(*id).is_some()).collect();
        let a = face_line(m, ids[0], Face::ExteriorCore, 0.0).unwrap_or_else(|| panic!("no line"));
        let b = face_line(m, ids[1], Face::ExteriorCore, 0.0).unwrap_or_else(|| panic!("no line"));
        // Each face line is cut at its wall's cap; their lines cross on the corner's bisector, 50 mm out from
        // each location line, where the floor outline miters them.
        let cross = geom::line_intersection(a.start(), a.end(), b.start(), b.end()).unwrap_or_else(|| panic!("parallel"));
        assert!(near(cross, P2(50.0, 50.0)), "{a:?} {b:?} {cross:?}");
    }

    #[test]
    fn a_curved_walls_face_line_is_an_arc_at_the_faces_off_the_axis() {
        let s = session(r#"wall {"points": [[0, 0], [4000, 0]], "arc": 180}"#);
        let m = &s.model;
        let w = m.wall(100002).unwrap_or_else(|| panic!("no wall"));
        let axis = Axis::of(w);
        let line = face_line(m, 100002, Face::InteriorFinish, 0.0).unwrap_or_else(|| panic!("no line"));
        assert!(near(line.start(), axis.at(0.0, -100.0)) && near(line.end(), axis.at(axis.length(), -100.0)), "{line:?}");
        let centre = axis.centre().unwrap_or_else(|| panic!("no centre"));
        assert!(line.points().iter().all(|p| (p.dist(centre) - 2100.0).abs() < 1.0), "its face runs 2100 mm from the centre");
    }

    #[test]
    fn a_face_lines_offset_moves_it_away_from_the_wall() {
        let s = session(r#"wall {"points": [[0, 0], [5000, 0]]}"#);
        let m = &s.model;
        let at = |face, off: f64| {
            let line = face_line(m, 100002, face, off).unwrap_or_else(|| panic!("no line"));
            line.start()
        };
        assert!(near(at(Face::ExteriorFinish, 0.0), P2(0.0, 100.0)));
        assert!(near(at(Face::ExteriorFinish, 50.0), P2(0.0, 150.0)), "out from the exterior face");
        assert!(near(at(Face::ExteriorFinish, -50.0), P2(0.0, 50.0)), "in toward the wall");
        assert!(near(at(Face::InteriorFinish, 0.0), P2(0.0, -100.0)));
        assert!(near(at(Face::InteriorFinish, 50.0), P2(0.0, -150.0)), "out from the interior face");
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
        // Attached under a gable: up to where the walls either side stop (3000), its ends mitred with theirs; over that, the
        // parts either side of the ridge, under the two slopes, square at their ends.
        let whole = prisms(&m, 100003, false);
        assert_eq!(whole.len(), 3);
        let tan = 30f64.to_radians().tan();
        let (low, high): (Vec<&Prism>, Vec<&Prism>) = whole.iter().partition(|p| p.top.g == P2(0.0, 0.0));
        assert_eq!((low.len(), high.len()), (1, 2));
        assert!((low[0].top.z - 3000.0).abs() < 1e-6);
        assert!(high.iter().all(|p| (p.top.at(P2(8000.0, 3000.0)) - (3000.0 + 3100.0 * tan)).abs() < 1e-6));
    }

    #[test]
    fn an_openings_hole_is_its_type_in_its_hosts_frame() {
        // A wall 6000 long whose base is 500 above its level, a 900 x 2100 door, and a 1200 x 1500
        // window with its sill 900 up.
        let s = session(
            r#"wall {"points": [[0,0],[6000,0]], "base_offset": 500}
               door {"wall": 100002, "offset": 2000}
               window {"wall": 100002, "offset": 4500}"#,
        );
        let m = &s.model;
        let w = m.wall(100002).unwrap_or_else(|| panic!("no wall"));
        let at = |id: Id| {
            let e = m.elements.get(id).unwrap_or_else(|| panic!("no element {id}"));
            let (Element::Door(o) | Element::Window(o)) = e else { panic!("{id} isn't an opening") };
            hole(m, w, e, o).unwrap_or_else(|| panic!("no hole for {id}"))
        };
        assert_eq!(at(100003), Hole { s0: 1550.0, s1: 2450.0, bottom: 500.0, top: 2600.0 });
        assert_eq!(at(100004), Hole { s0: 3900.0, s1: 5100.0, bottom: 1400.0, top: 2900.0 });
    }
}
