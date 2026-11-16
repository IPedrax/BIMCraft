//! The drawing's Snap References, as Archicad's: a point or an edge the user marks with a hover's ring or `Q`, and
//! what it lends the guides — its own line (collinear) and its direction (parallel, perpendicular). A mark lives for
//! the command; the core sees only the `GuideSource`s and points gathered here, never the model's other elements.
//!
//! A wall's edge is one of its three references — its exterior face, its centre line or its interior face — the same
//! `Face` the snap names, so what `Q` marks and what the diamond shows can never be two different lines.

use std::collections::BTreeMap;

use bimcraft_core::alignment::{GuideSource, ReferenceType};
use bimcraft_core::geom::{self, P2};
use bimcraft_core::model::{self, Element, Face, Guid, Id, Model, Reference};
use bimcraft_core::render::View;
use bimcraft_core::snap::{Found, Kind};
use bimcraft_core::walls::{self, Axis};

/// How many temporary references a view keeps: the oldest goes when a fourth is marked.
pub(crate) const CAP: usize = 3;
/// The pointer must rest this long before the ring starts: a fast sweep marks nothing.
pub(crate) const DEAD: f64 = 0.15;
/// Then the ring fills over this long: 0.45 s in all.
pub(crate) const FILL: f64 = 0.30;
/// The ring's radius on screen, points.
pub(crate) const RING: f32 = 9.0;
/// How far the pointer may drift while the ring fills, points: a `Nearest` point slides along its edge, and the hand
/// trembles.
pub(crate) const STILL: f32 = 4.0;
/// How near the pointer a straight edge must pass to be the one `Q` marks, points.
pub(crate) const REACH: f32 = 8.0;

/// What the user marked: a point (a corner, a midpoint, a column's centre) or an edge (a wall's face or centre line, a
/// detail line, a grid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PinTarget {
    Point {
        at: P2,
        kind: Kind,
        id: Option<Id>,
    },
    Edge {
        a: P2,
        b: P2,
        id: Option<Id>,
        /// A wall's line it is, when the edge is a wall's: its exterior face, centre line or interior face.
        face: Face,
        /// The end of the edge the pointer was near when it was marked: a face corner lends the wall it meets there.
        near: Option<P2>,
    },
}

impl PinTarget {
    /// The element it is of, when it is one: two faces of a wall are two references of one element.
    pub(crate) fn id(self) -> Option<Id> {
        match self {
            PinTarget::Point { id, .. } | PinTarget::Edge { id, .. } => id,
        }
    }

    /// Whether it is an edge rather than a point. A wall lends one edge pin, however many of its faces are marked.
    pub(crate) fn is_edge(self) -> bool {
        matches!(self, PinTarget::Edge { .. })
    }

    /// What identifies the target while the pointer rests on it: a point to the millimetre, an edge by its element and
    /// line, so a `Nearest` point sliding along its face does not restart the ring.
    pub(crate) fn key(self) -> PinKey {
        match self {
            PinTarget::Point { at, kind, id } => {
                PinKey::Point { x: (at.0 * 10.0).round() as i64, y: (at.1 * 10.0).round() as i64, kind: kind as u8, id }
            }
            PinTarget::Edge { id, face, .. } => PinKey::Edge { id, face },
        }
    }

    /// The lines and directions it lends the guides. A point lends the directions of the walls and lines that meet
    /// there; an edge lends itself, and, marked at one of its ends, the wall it meets there too.
    pub(crate) fn lines(self, m: &Model, level: Id, view: Guid) -> Vec<GuideSource> {
        match self {
            PinTarget::Point { at, .. } => connected(m, level, view, at, None),
            PinTarget::Edge { a, b, id, near, .. } => {
                let mut lines = vec![GuideSource { a, b, id }];
                if let Some(end) = near {
                    lines.extend(connected(m, level, view, end, id));
                }
                lines
            }
        }
    }
}

/// What the dwell compares from frame to frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PinKey {
    Point { x: i64, y: i64, kind: u8, id: Option<Id> },
    Edge { id: Option<Id>, face: Face },
}

/// A ring filling on the reference under the pointer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dwell {
    pub key: PinKey,
    /// Where the pointer was when the ring started: it may drift [`STILL`] while the ring fills.
    pub from: (f32, f32),
    /// When the pointer came to rest on the target.
    pub start: f64,
    /// The mark this rest has already made, so it is made once, not every frame after.
    pub marked: bool,
}

impl Dwell {
    /// How much of the ring is drawn, 0 to 1: nothing in the dead zone, then the fill.
    pub(crate) fn progress(self, now: f64) -> f32 {
        (((now - self.start - DEAD) / FILL).clamp(0.0, 1.0)) as f32
    }

    /// Whether the dead zone is over and the ring is to be drawn.
    pub(crate) fn filling(self, now: f64) -> bool {
        now - self.start >= DEAD
    }
}

/// One marked reference and the lines it lends.
#[derive(Clone, Debug)]
pub(crate) struct Pin {
    pub(crate) target: PinTarget,
    pub(crate) lines: Vec<GuideSource>,
}

/// What marking did, for the status box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Marked {
    /// A new reference, in a free place.
    New,
    /// The reference was marked already: it is cleared.
    Cleared,
    /// Another face of a wall already marked: that one pin moved to this face.
    Moved,
}

/// The marked references of each view, oldest first.
#[derive(Clone, Debug, Default)]
pub(crate) struct Pins(BTreeMap<Id, Vec<Pin>>);

impl Pins {
    pub(crate) fn at(&self, view: Id) -> &[Pin] {
        self.0.get(&view).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn is_marked(&self, view: Id, target: PinTarget) -> bool {
        self.at(view).iter().any(|p| p.target.key() == target.key())
    }

    /// Marks the target, or unmarks it when it is marked already. A wall lends one edge pin: marking another of its faces
    /// moves that pin rather than spending a second of the three. A fourth mark lets the oldest go.
    pub(crate) fn toggle(&mut self, view: Id, target: PinTarget, lines: Vec<GuideSource>) -> Marked {
        let list = self.0.entry(view).or_default();
        if let Some(i) = list.iter().position(|p| p.target.key() == target.key()) {
            list.remove(i);
            return Marked::Cleared;
        }
        let mut moved = false;
        if target.is_edge()
            && let Some(id) = target.id()
            && let Some(i) = list.iter().position(|p| p.target.is_edge() && p.target.id() == Some(id))
        {
            list.remove(i);
            moved = true;
        }
        if list.len() >= CAP {
            list.remove(0);
        }
        list.push(Pin { target, lines });
        if moved { Marked::Moved } else { Marked::New }
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// The snap's point when it is a discrete one; `Nearest` and an angle step are edges and directions, not points.
fn point_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Endpoint
            | Kind::Midpoint
            | Kind::Intersection
            | Kind::Centre
            | Kind::Perpendicular
            | Kind::Quadrant
            | Kind::Tangent
            | Kind::Point
            | Kind::Anchor
    )
}

/// The reference under the pointer: the wall line the snap itself has chosen, else a snapped point, else the nearest
/// straight edge (a wall's face or centre line, a detail line, a grid) within [`REACH`] points. `under` are the elements
/// the plan's picking found there.
pub(crate) fn target_at(
    m: &Model,
    level: Id,
    view_key: Id,
    v: &View,
    cursor: (f32, f32),
    found: &Found,
    under: &[Id],
) -> Option<PinTarget> {
    let cursor_at = v.mm(cursor);
    let screen = |q: P2| {
        let (x, y) = v.px(q);
        P2(f64::from(x), f64::from(y))
    };
    let here = screen(cursor_at);
    let near = f64::from(REACH) / v.scale;
    // The snap is on a wall's own line: that line is the reference, so `Q` marks exactly what the diamond shows.
    if let (Some(face), Some(id)) = (found.face, found.source)
        && let Some(target) = wall_line(m, id, face, cursor_at, near)
    {
        return Some(target);
    }
    let point = point_kind(found.kind).then_some(PinTarget::Point { at: found.at, kind: found.kind, id: found.source });
    let mut best: Option<(f64, PinTarget)> = None;
    let mut consider = |a: P2, b: P2, id: Option<Id>, face: Face| {
        if a.dist(b) <= model::TOL {
            return;
        }
        let d = geom::dist_to_segment(here, screen(a), screen(b));
        if d <= f64::from(REACH) && best.as_ref().is_none_or(|(nearest, _)| d < *nearest) {
            let near = if cursor_at.dist(a) <= near {
                Some(a)
            } else if cursor_at.dist(b) <= near {
                Some(b)
            } else {
                None
            };
            best = Some((d, PinTarget::Edge { a, b, id, face, near }));
        }
    };
    let view_guid = m.guid(view_key);
    for id in under {
        match m.elements.get(id) {
            // A wall's three references, exactly as the model gives them to dimensions: its faces and its centre line.
            Some(Element::Wall(w)) if w.arc == 0.0 && m.cut_by(w, m.guid(level)) => {
                for (r, (a, b)) in m.reference_lines(*id) {
                    if matches!(r.face, Face::Centre | Face::Left | Face::Right) {
                        consider(a, b, Some(*id), r.face);
                    }
                }
            }
            Some(Element::DetailLine(l)) if l.view == view_guid && l.curve.kind() == "line" => {
                consider(l.curve.start(), l.curve.end(), Some(*id), Face::Centre);
            }
            Some(Element::Grid(g)) => consider(g.start, g.end, Some(*id), Face::Centre),
            _ => {}
        }
    }
    // The reference nearest the pointer wins: a face under it beats a centre line's midpoint a few pixels away, and a
    // corner under it beats the faces that meet there.
    match (point, best) {
        (Some(point), Some((d, edge))) => {
            let p = match point {
                PinTarget::Point { at, .. } => at,
                PinTarget::Edge { .. } => found.at,
            };
            if here.dist(screen(p)) <= d + 0.5 { Some(point) } else { Some(edge) }
        }
        (Some(point), None) => Some(point),
        (None, best) => best.map(|(_, target)| target),
    }
}

/// A wall's line `face` as a stand-alone target, when the pointer is near one of its ends.
fn wall_line(m: &Model, id: Id, face: Face, cursor_at: P2, near: f64) -> Option<PinTarget> {
    let (a, b) = m.reference_line(&Reference::line(m.guid(id), face)).ok()?;
    if a.dist(b) <= model::TOL {
        return None;
    }
    let near = if cursor_at.dist(a) <= near {
        Some(a)
    } else if cursor_at.dist(b) <= near {
        Some(b)
    } else {
        None
    };
    Some(PinTarget::Edge { a, b, id: Some(id), face, near })
}

/// The walls, detail lines and grids that meet at `point`, as directions through it: a rotated corner lends its walls'
/// own angles, which world X and Y cannot. The wall `skip` meets there too and is left out (an edge already lends
/// itself).
pub(crate) fn connected(m: &Model, level: Id, view: Guid, point: P2, skip: Option<Id>) -> Vec<GuideSource> {
    let level_guid = m.guid(level);
    let mut out: Vec<GuideSource> = Vec::new();
    for (id, e) in m.elements.iter() {
        if Some(*id) == skip {
            continue;
        }
        let dir = match e {
            Element::Wall(w) if w.arc == 0.0 && m.cut_by(w, level_guid) => {
                let (axis, (ext, int)) = (Axis::of(w), walls::faces(m, w));
                let len = axis.length();
                let slack = 4.0 * m.thickness(w) + 10.0;
                let lines = [
                    (axis.at(0.0, 0.0), axis.at(len, 0.0)),
                    (axis.at(0.0, ext), axis.at(len, ext)),
                    (axis.at(0.0, int), axis.at(len, int)),
                ];
                lines.iter().find(|(a, b)| on_segment_line(point, *a, *b, slack)).map(|_| (axis.end - axis.start).unit())
            }
            Element::DetailLine(l) if l.view == view && l.curve.kind() == "line" => {
                let (a, b) = (l.curve.start(), l.curve.end());
                (geom::dist_to_segment(point, a, b) <= 1.0).then(|| (b - a).unit())
            }
            Element::Grid(g) => (geom::dist_to_segment(point, g.start, g.end) <= 1.0).then(|| (g.end - g.start).unit()),
            _ => None,
        };
        let Some(dir) = dir.filter(|d| d.length() > 0.5) else { continue };
        let source = GuideSource { a: point - dir * 1000.0, b: point + dir * 1000.0, id: Some(*id) };
        if !out.iter().any(|s| s.id == source.id && (s.b - s.a).unit().dot(dir).abs() > 1.0 - 1e-9) {
            out.push(source);
        }
    }
    out
}

/// Whether `p` lies on the line through `a`–`b` and within the segment's span, with `slack` at either end for a mitre.
fn on_segment_line(p: P2, a: P2, b: P2, slack: f64) -> bool {
    let d = b - a;
    let len = d.length();
    if len <= model::TOL {
        return false;
    }
    let u = d * (1.0 / len);
    (p - a).cross(u).abs() <= 1.0 && {
        let t = (p - a).dot(u);
        t >= -slack && t <= len + slack
    }
}

/// The wall face or centre line nearest `p`, within `reach` mm: the edge a chain started on. A straight wall only.
pub(crate) fn wall_edge_at(m: &Model, level: Id, p: P2, reach: f64) -> Option<GuideSource> {
    let level_guid = m.guid(level);
    let mut best: Option<(f64, GuideSource)> = None;
    for (id, e) in m.elements.iter() {
        let Element::Wall(w) = e else { continue };
        if w.arc != 0.0 || !m.cut_by(w, level_guid) {
            continue;
        }
        for (r, (a, b)) in m.reference_lines(*id) {
            if !matches!(r.face, Face::Centre | Face::Left | Face::Right) {
                continue;
            }
            let d = geom::dist_to_segment(p, a, b);
            if d <= reach && best.as_ref().is_none_or(|(nearest, _)| d < *nearest) {
                best = Some((d, GuideSource { a, b, id: Some(*id) }));
            }
        }
    }
    best.map(|(_, source)| source)
}

/// The structural grids near `cursor`, as lines the guides may use: only those within `reach` points (capped at
/// 300 mm in the model), so a zoomed-out plan does not turn every grid into a guide.
pub(crate) fn grids_near(m: &Model, v: &View, cursor: P2, reach: f32) -> Vec<GuideSource> {
    let reach = (f64::from(reach) / v.scale).min(300.0);
    // A live draw tool asks this every frame, so the lookup's grid list once a view has made it, and a walk of every
    // element only until then: a million elements is a model this is drawn on, not one it should search per frame.
    let ids: Vec<Id> = match m.derived.made(&m.elements) {
        Some(look) => look.grids().collect(),
        None => m.elements.iter().filter(|(_, e)| matches!(e, Element::Grid(_))).map(|(id, _)| *id).collect(),
    };
    ids.into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Grid(g)) if g.start.dist(g.end) > model::TOL => {
                (geom::dist_to_segment(cursor, g.start, g.end) <= reach).then_some(GuideSource { a: g.start, b: g.end, id: Some(id) })
            }
            _ => None,
        })
        .collect()
}

/// The points a marked reference lends the alignment service: its own point, and the kind it is ranked by there — an
/// endpoint ahead of a middle, a centre among the middles, a crossing last. A pin is used whatever the snap switches say:
/// `from_context` hands its points over as they are, and only the chain's are gated.
pub(crate) fn reference(target: PinTarget) -> Option<(P2, ReferenceType)> {
    match target {
        PinTarget::Point { at, kind, .. } => Some((
            at,
            match kind {
                Kind::Midpoint | Kind::Centre | Kind::Quadrant => ReferenceType::Midpoint,
                Kind::Intersection => ReferenceType::Intersection,
                _ => ReferenceType::Endpoint,
            },
        )),
        PinTarget::Edge { .. } => None,
    }
}
