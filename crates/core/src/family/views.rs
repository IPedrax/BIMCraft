//! A family's views (F4), as Revit's family editor has them: the Ref. Level floor and ceiling plans and the four elevations,
//! Front, Back, Left and Right. Each is the project's own drawing of a scratch model the family is placed in (`stage`): its
//! level is Ref. Level, each of the family's reference planes is a project reference plane there (named as it is, mapped back
//! to the family's plane by GUID; neither the project nor the family is touched), and the four elevations look at it from
//! outside, Front looking north (+y, the family's back), as Revit's do. The plan cuts the forms at its cut plane
//! ([`plan_cut`]); a project's plans never cut a component.

use std::collections::{BTreeMap, HashMap};

use super::{Family, Val};
use crate::geom::{self, P2};
use crate::hosting::ReferencePlane;
use crate::ids::{Guid, Id};
use crate::model::{Element, Model, Section};
use crate::render::{self, Drawing, View};
use crate::view3d::{self, P3};

/// The side of the family an elevation shows, as Revit names them: Front shows its front (-y), looking north (+y).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Side {
    Back,
    Front,
    Left,
    Right,
}

impl Side {
    /// In the order the Project Browser lists them.
    pub const ALL: [Side; 4] = [Side::Back, Side::Front, Side::Left, Side::Right];

    /// Its English name: the elevation's.
    pub fn name(self) -> &'static str {
        match self {
            Side::Back => "Back",
            Side::Front => "Front",
            Side::Left => "Left",
            Side::Right => "Right",
        }
    }

    /// The way it looks, in plan.
    pub fn looking(self) -> P2 {
        match self {
            Side::Front => P2(0.0, 1.0),
            Side::Back => P2(0.0, -1.0),
            Side::Left => P2(1.0, 0.0),
            Side::Right => P2(-1.0, 0.0),
        }
    }

    /// Its picture's right, in plan: along its line, start to end.
    pub fn right(self) -> P2 {
        let l = self.looking();
        P2(l.1, -l.0)
    }
}

/// One of a family's views besides {3D}.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Which {
    Plan,
    Ceiling,
    Elevation(Side),
}

impl Which {
    /// Every view besides {3D}, as the Project Browser lists them: the plans, then the elevations.
    pub const ALL: [Which; 6] = [
        Which::Plan,
        Which::Ceiling,
        Which::Elevation(Side::Back),
        Which::Elevation(Side::Front),
        Which::Elevation(Side::Left),
        Which::Elevation(Side::Right),
    ];

    /// The way it looks along, in the family's axes, either way: up and down for the plans, level for an elevation.
    pub fn eye(self) -> P3 {
        match self {
            Which::Plan | Which::Ceiling => [0.0, 0.0, 1.0],
            Which::Elevation(side) => {
                let l = side.looking();
                [l.0, l.1, 0.0]
            }
        }
    }

    /// Whether it shows a plane of `normal` flat, looking straight at it (either side): what can be sketched on in it.
    pub fn faces(self, normal: P3) -> bool {
        let e = self.eye();
        let n = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        n > 0.0 && ((normal[0] * e[0] + normal[1] * e[1] + normal[2] * e[2]) / n).abs() > 1.0 - 1e-9
    }
}

/// The views that show a plane of `normal` flat (Go To View's list, before {3D}, which shows any): the two plans for a
/// level plane, Front and Back or Left and Right for an upright one square to them; none for another.
pub fn flat_views(normal: P3) -> Vec<Which> {
    Which::ALL.into_iter().filter(|w| w.faces(normal)).collect()
}

/// A scratch model a family is placed in, made ready for its views.
pub struct Staged {
    pub model: Model,
    /// Ref. Level.
    pub level: Id,
    elevations: Vec<(Side, Id, P2)>,
    /// Each reference plane of the scratch model, the family's plane it stands for, by GUID.
    pub planes: BTreeMap<Id, Guid>,
    /// What the views frame, in the family's axes: what it takes up, 300 mm beyond, and at least 1.5 m from its origin each
    /// way but down, as the modelling view frames it.
    pub room: (P3, P3),
    /// The family's reference lines, each from its start to its end: drawn over every view.
    lines: Vec<(P3, P3)>,
}

/// How far the elevations stand outside what they look at.
const OUTSIDE: f64 = 1000.0;

/// Which of `family`'s forms (by place) view `which` (none, {3D}) at `detail` leaves out by their Visibility Settings (F8),
/// sized by `values`: a plan by Plan/RCP (only when cut, where its cut plane at `cut` mm passes through it), an elevation by
/// Front/Back or Left/Right as it looks along the family's y or x, {3D} by the detail level alone. What a family's views draw
/// halftone, so a hidden form can still be seen and picked, as Revit's family views do.
pub fn hidden_in(
    family: &Family,
    values: &BTreeMap<String, Val>,
    which: Option<Which>,
    detail: crate::graphics::Detail,
    cut: f64,
) -> Vec<bool> {
    use super::visibility::{Visibility, class};
    let shapes = family.shapes(values);
    family
        .forms
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let Some(v): Option<Visibility> = f.visibility else { return false };
            let shown = match which {
                None => v.in_3d(detail),
                Some(Which::Plan | Which::Ceiling) => {
                    let through = shapes.iter().any(|s| s.index == i && s.min[2] < cut && cut < s.max[2]);
                    v.in_plan(detail, through)
                }
                Some(Which::Elevation(side)) => v.along(class(side.looking(), P2(1.0, 0.0)), detail, false),
            };
            !shown
        })
        .collect()
}

/// What `family` takes up sized by `values`, in its axes, its least and greatest corners: what any of its types draws (a form
/// the type hides is still drawn, halftone); a family that builds nothing, two metres round its origin.
pub fn extents(family: &Family, values: &BTreeMap<String, Val>) -> (P3, P3) {
    let mut every = family.clone();
    every.forms.iter_mut().for_each(|f| f.visible = None);
    let shapes = every.shapes(values);
    let lo: P3 = std::array::from_fn(|i| shapes.iter().map(|s| s.min[i]).fold(f64::INFINITY, f64::min));
    let hi: P3 = std::array::from_fn(|i| shapes.iter().map(|s| s.max[i]).fold(f64::NEG_INFINITY, f64::max));
    if lo.iter().chain(&hi).all(|v| v.is_finite()) { (lo, hi) } else { ([-1000.0; 3], [1000.0; 3]) }
}

/// Model `m`, the family placed in it (at the origin, unturned: a component, or a door or a window in its wall), staged for
/// its views: the first level renamed Ref. Level and the others taken away; each of `family`'s planes `shown` a reference
/// plane of the model, across `room`; the four elevations. What `family` takes up is worked out for `values`.
pub fn stage(mut m: Model, family: &Family, values: &BTreeMap<String, Val>, shown: &dyn Fn(Guid) -> bool) -> Staged {
    let levels: Vec<Id> = m.levels().iter().map(|(id, _)| *id).collect();
    let level = levels.first().copied().unwrap_or_else(|| m.insert(Element::Level(crate::model::Level::new("Ref. Level", 0.0))));
    // A door's or a window's wall stands as tall as it did, unconnected: the level its top was tied to goes.
    let gone: Vec<Guid> = levels.iter().skip(1).map(|id| m.guid(*id)).collect();
    let tied: Vec<(Id, f64)> = m
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Wall(w) if w.top_level.is_some_and(|t| gone.contains(&t)) => Some((*id, m.own_top(w) - m.own_base(w))),
            _ => None,
        })
        .collect();
    for (id, height) in tied {
        if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
            (w.top_level, w.top_offset, w.height) = (None, 0.0, height);
        }
    }
    for id in levels.iter().skip(1) {
        m.elements.remove(*id);
    }
    if let Some(Element::Level(l)) = m.elements.get_mut(level) {
        l.name = "Ref. Level".into();
    }
    let (mut lo, mut hi) = extents(family, values);
    // The reference lines are in the room too, so the planes' traces and the level run past them.
    for p in family.lines.iter().flat_map(|l| [l.start, l.end]) {
        for i in 0..3 {
            (lo[i], hi[i]) = (lo[i].min(p[i]), hi[i].max(p[i]));
        }
    }
    let room: (P3, P3) = (
        std::array::from_fn(|i| if i == 2 { (lo[i] - 300.0).min(0.0) } else { (lo[i] - 300.0).min(-1500.0) }),
        std::array::from_fn(|i| (hi[i] + 300.0).max(1500.0)),
    );
    // Ref. Level runs across the room in every elevation, as the planes' traces do, and under the reference lines drawn
    // over the views, which aren't elements of the model and so wouldn't stretch it.
    if let Some(Element::Level(l)) = m.elements.get_mut(level) {
        l.extent = Some([P2(room.0[0], room.0[1]), P2(room.1[0], room.1[1])]);
    }
    let mut planes = BTreeMap::new();
    for p in family.planes.iter().filter(|p| !p.level && shown(p.guid)) {
        let f = p.frame();
        // The line it is drawn as, across the room: the room's corners taken onto its x.
        let along = |q: P3| view3d::dot(view3d::sub(q, f.origin), f.x);
        let corners = (0..8).map(|k| [[room.0, room.1][k & 1][0], [room.0, room.1][(k >> 1) & 1][1], [room.0, room.1][k >> 2][2]]);
        let (t0, t1) = corners.map(along).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)));
        let at = |t: f64| view3d::add(f.origin, view3d::mul(f.x, t));
        let plane = ReferencePlane { name: p.name.clone(), start: at(t0), end: at(t1), up: f.y };
        planes.insert(m.insert(Element::RefPlane(plane)), p.guid);
    }
    let mut elevations = Vec::new();
    for side in Side::ALL {
        let (l, r) = (side.looking(), side.right());
        let corners = [P2(room.0[0], room.0[1]), P2(room.1[0], room.0[1]), P2(room.1[0], room.1[1]), P2(room.0[0], room.1[1])];
        let back = corners.iter().map(|c| c.dot(l)).fold(f64::INFINITY, f64::min) - OUTSIDE;
        let (a, b) = corners.iter().map(|c| c.dot(r)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(t), b.max(t)));
        let start = l * back + r * a;
        let end = l * back + r * b;
        let section =
            Section { name: side.name().into(), start, end, depth: None, crop: None, graphics: crate::graphics::Graphics::default() };
        elevations.push((side, m.insert(Element::Elevation(section)), start));
    }
    let lines = family.lines.iter().map(|l| (l.start, l.end)).collect();
    Staged { model: m, level, elevations, planes, room, lines }
}

/// What one of a family's views draws, for a front end that paints it as vectors: its frame, its drawing (what picks), and
/// an elevation's hidden-line picture, which the drawing goes over.
pub struct Lines {
    pub view: View,
    pub drawing: Drawing,
    pub picture: Option<crate::view3d::Image>,
}

/// What one of a family's views shows: its frame, its drawing (what picks) and its pixels.
pub struct Shown {
    pub view: View,
    pub drawing: Drawing,
    pub rgba: Vec<u8>,
}

impl Staged {
    /// The elevation looking from `side`.
    pub fn elevation(&self, side: Side) -> Option<Id> {
        self.elevations.iter().find(|(s, ..)| *s == side).map(|(_, id, _)| *id)
    }

    /// The view element `which` is drawn from: the level, or an elevation.
    pub fn view_id(&self, which: Which) -> Option<Id> {
        match which {
            Which::Plan | Which::Ceiling => Some(self.level),
            Which::Elevation(side) => self.elevation(side),
        }
    }

    /// Where point `p` (the family's axes) is in view `which`: in plan its x and y; in an elevation how far along its picture
    /// from the picture's line's start, and its height.
    pub fn to_view(&self, which: Which, p: P3) -> P2 {
        match which {
            Which::Plan | Which::Ceiling => P2(p[0], p[1]),
            Which::Elevation(side) => {
                let start = self.elevations.iter().find(|(s, ..)| *s == side).map_or(P2(0.0, 0.0), |(.., st)| *st);
                P2((P2(p[0], p[1]) - start).dot(side.right()), p[2])
            }
        }
    }

    /// The point of the family a point `q` of view `which` stands for: on Ref. Level in plan, on the plane through the
    /// family's origin the elevation looks at in an elevation.
    pub fn from_view(&self, which: Which, q: P2) -> P3 {
        match which {
            Which::Plan | Which::Ceiling => [q.0, q.1, 0.0],
            Which::Elevation(side) => {
                let start = self.elevations.iter().find(|(s, ..)| *s == side).map_or(P2(0.0, 0.0), |(.., st)| *st);
                let r = side.right();
                let x = r * (start + r * q.0).dot(r);
                [x.0, x.1, q.1]
            }
        }
    }

    /// The direction in the family a move `dq` in view `which` stands for.
    pub fn vector(&self, which: Which, dq: P2) -> P3 {
        match which {
            Which::Plan | Which::Ceiling => [dq.0, dq.1, 0.0],
            Which::Elevation(side) => {
                let r = side.right();
                [r.0 * dq.0, r.1 * dq.0, dq.1]
            }
        }
    }

    /// The frame that shows the whole room in a `width` by `height` picture, `zoom` times as close, moved `pan` (mm of the
    /// view) from its middle.
    fn frame(&self, which: Which, width: u32, height: u32, zoom: f64, pan: P2) -> View {
        let corners: Vec<P2> = (0..8)
            .map(|k| {
                [[self.room.0, self.room.1][k & 1][0], [self.room.0, self.room.1][(k >> 1) & 1][1], [self.room.0, self.room.1][k >> 2][2]]
            })
            .map(|p| self.to_view(which, p))
            .collect();
        let lo = corners.iter().fold(P2(f64::INFINITY, f64::INFINITY), |a, c| P2(a.0.min(c.0), a.1.min(c.1)));
        let hi = corners.iter().fold(P2(f64::NEG_INFINITY, f64::NEG_INFINITY), |a, c| P2(a.0.max(c.0), a.1.max(c.1)));
        let (w, h) = (f64::from(width.max(16)), f64::from(height.max(16)));
        let fit = |lo: P2, hi: P2| 0.75 * (w / (hi.0 - lo.0).max(1.0)).min(h / (hi.1 - lo.1).max(1.0));
        // An elevation frames what it shows as the project's do (`render::section_extent`): its levels' lines run past the
        // room, with their names past their ends, which would be cut at the picture's edge.
        let (mut lo, mut hi) = (lo, hi);
        if let Which::Elevation(side) = which
            && let Some(id) = self.elevation(side)
        {
            let work = render::SectionWork::new(&self.model);
            if let Some([a, b]) = render::section_extent(&self.model, &work, id, fit(lo, hi), 1.0) {
                (lo, hi) = (P2(lo.0.min(a.0), lo.1.min(a.1)), P2(hi.0.max(b.0), hi.1.max(b.1)));
            }
        }
        let scale = fit(lo, hi) * zoom.clamp(0.05, 50.0);
        View { scale, cx: (lo.0 + hi.0) / 2.0 + pan.0, cy: (lo.1 + hi.1) / 2.0 + pan.1, width: width.max(16), height: height.max(16) }
    }

    /// View `which`, `width` by `height` pixels, `zoom` times as close as fitted, moved `pan` mm. Its scale is the one at
    /// which, fitted, its annotation and pens are their size on paper, so the names read as a sheet's would, and grow
    /// and shrink as it is zoomed, as a project's views' do.
    pub fn draw_lines(&mut self, which: Which, width: u32, height: u32, zoom: f64, pan: P2) -> Result<Lines, String> {
        let fitted = self.frame(which, width, height, 1.0, P2(0.0, 0.0));
        let n = (crate::sheet::DENSITY / fitted.scale).clamp(1.0, 1e5);
        if let Some(g) = self.view_id(which).and_then(|id| self.model.elements.get_mut(id)).and_then(crate::graphics::graphics_mut) {
            g.scale = Some(n);
        }
        let m = &self.model;
        let view = self.frame(which, width, height, zoom, pan);
        // The reference lines, as the view sees them: seen from above in plan (and from below in the ceiling plan), across
        // an elevation where their plane is square to it, foreshortened where it isn't.
        let lines: Vec<(P2, P2)> = self.lines.iter().map(|(a, b)| (self.to_view(which, *a), self.to_view(which, *b))).collect();
        let (mut drawing, picture) = match which {
            Which::Plan => (render::family_plan(m, self.level, view), None),
            Which::Ceiling => (render::draw_ceiling(m, self.level, view), None),
            Which::Elevation(side) => {
                let id = self.elevation(side).ok_or("the family has no such elevation")?;
                let picture = render::section_picture(m, id, view, &[]).ok_or("the elevation can't be drawn")?;
                (render::section_over(m, id, view, &picture), Some(picture))
            }
        };
        render::reference_lines(&mut drawing, &lines);
        Ok(Lines { view, drawing, picture })
    }

    /// View `which` as [`draw_lines`](Self::draw_lines) draws it, rasterized: its drawing over its picture, on paper.
    pub fn draw(&mut self, which: Which, width: u32, height: u32, zoom: f64, pan: P2) -> Result<Shown, String> {
        let Lines { view, drawing, picture } = self.draw_lines(which, width, height, zoom, pan)?;
        let rgba = render::rgba(&drawing, picture.as_ref())?;
        Ok(Shown { view, drawing, rgba })
    }

    /// The family's reference line drawn nearest point `q` of view `which` (mm), within `reach` mm of it: its place in the
    /// family's `lines`. A line seen end on is a point there, and is picked as one.
    pub fn line_at(&self, which: Which, q: P2, reach: f64) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .map(|(i, (a, b))| (i, geom::dist_to_segment(q, self.to_view(which, *a), self.to_view(which, *b))))
            .filter(|(_, d)| d.is_finite() && *d <= reach)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(i, _)| i)
    }

    /// The family's plane a reference plane of the scratch model drawn by `owner` stands for: Ref. Level for the level an
    /// elevation draws it as, so it is picked as the planes are (a dimension to the sill, Align, a work plane).
    pub fn plane_of(&self, owner: Id) -> Option<Guid> {
        if owner == self.level {
            return Some(super::REF_LEVEL);
        }
        self.planes.get(&owner).copied()
    }
}

/// The ends and `up` of a reference plane drawn by two clicks `a` and `b` in view `which`: in plan upright (`up` the
/// vertical); in an elevation `up` the way it looks, so its normal lies in the elevation's plane. None for two clicks at
/// one place.
pub fn new_plane(st: &Staged, which: Which, a: P2, b: P2) -> Option<(P3, P3, P3)> {
    if a.dist(b) < 1.0 {
        return None;
    }
    let up = match which {
        Which::Plan | Which::Ceiling => [0.0, 0.0, 1.0],
        Which::Elevation(side) => {
            let l = side.looking();
            [l.0, l.1, 0.0]
        }
    };
    Some((st.from_view(which, a), st.from_view(which, b), up))
}

/// Where the level plane at height `z` cuts closed mesh `tris`, as rings in plan (mm). The same slicing a section's cut is
/// ([`view3d::slice`]), on the mesh turned so its height is the section's depth.
pub fn plan_cut(tris: &[[P3; 3]], z: f64) -> Vec<Vec<P2>> {
    let x0 = tris.iter().flatten().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let x1 = tris.iter().flatten().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    if !x0.is_finite() || !x1.is_finite() {
        return Vec::new();
    }
    // x stays x, the height becomes the depth across the slicing plane, y becomes its height.
    let turned: Vec<[P3; 3]> = tris.iter().map(|t| t.map(|p| [p[0], p[2], p[1]])).collect();
    let from = x0 - 1.0;
    view3d::slice(&turned, P2(from, z), P2(1.0, 0.0), x1 - from + 1.0)
        .unwrap_or_default()
        .into_iter()
        .map(|ring| ring.into_iter().map(|q| P2(from + q.0, q.1)).collect())
        .collect()
}

/// The solid form of `family` drawn at point `at` of view `which` (sized by `values`), its place in `forms`: the one nearest
/// the eye there, in plan the highest. None where none is.
pub fn form_at(family: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which, at: P2) -> Option<usize> {
    let mut read = HashMap::new();
    let mut best: Option<(f64, usize)> = None;
    for i in 0..family.forms.len() {
        if family.forms[i].void {
            continue;
        }
        let Some(piece) = family.solid(i, values, &mut read).ok().flatten() else { continue };
        for t in &piece.tris {
            let q = t.map(|p| st.to_view(which, p));
            if !inside(&q, at) {
                continue;
            }
            let near = match which {
                Which::Plan => t.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max),
                Which::Ceiling => -t.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min),
                Which::Elevation(side) => -t.iter().map(|p| P2(p[0], p[1]).dot(side.looking())).fold(f64::INFINITY, f64::min),
            };
            if best.is_none_or(|(b, _)| near > b) {
                best = Some((near, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// The nested instance of `family` drawn at point `at` of view `which` (sized by `values`), its place in `nested_instances`:
/// the one nearest the eye there, as [`form_at`] finds a form. None where none is.
pub fn nested_at(family: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which, at: P2) -> Option<usize> {
    let mut best: Option<(f64, usize)> = None;
    for i in 0..family.nested_instances.len() {
        for t in family.instance_pieces(i, values).iter().flat_map(|p| p.tris.iter()) {
            let q = t.map(|p| st.to_view(which, p));
            if !inside(&q, at) {
                continue;
            }
            let near = match which {
                Which::Plan => t.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max),
                Which::Ceiling => -t.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min),
                Which::Elevation(side) => -t.iter().map(|p| P2(p[0], p[1]).dot(side.looking())).fold(f64::INFINITY, f64::min),
            };
            if best.is_none_or(|(b, _)| near > b) {
                best = Some((near, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// The outline nested instance `i` of `family` is seen with in view `which`: the hull of its pieces' corners there.
pub fn nested_outline(family: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which, i: usize) -> Vec<P2> {
    let corners = family.instance_pieces(i, values).iter().flat_map(|p| p.tris.iter().flatten().map(|q| st.to_view(which, *q))).collect();
    crate::geom::hull(corners)
}

/// The solid form of `family` (sized by `values`) shown at pixel `px` of a picture taken through `cam`, the family at the
/// origin as {3D} shows it: the one nearest the eye there.
pub fn form_at_3d(family: &Family, values: &BTreeMap<String, Val>, cam: &view3d::Camera, px: (f64, f64)) -> Option<usize> {
    let mut read = HashMap::new();
    let mut best: Option<(f64, usize)> = None;
    for i in 0..family.forms.len() {
        if family.forms[i].void {
            continue;
        }
        let Some(piece) = family.solid(i, values, &mut read).ok().flatten() else { continue };
        for t in &piece.tris {
            let q = t.map(|p| cam.project(p));
            if q.iter().any(|(x, y, z)| !(x.is_finite() && y.is_finite() && z.is_finite())) {
                continue;
            }
            if !inside(&q.map(|(x, y, _)| P2(x, y)), P2(px.0, px.1)) {
                continue;
            }
            let near = q.iter().map(|(_, _, z)| *z).fold(f64::NEG_INFINITY, f64::max);
            if best.is_none_or(|(b, _)| near > b) {
                best = Some((near, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// What of a family its {3D} shows at a pixel, by its place: a form (solid or void) or a nested instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    Form(usize),
    Nested(usize),
}

/// The solid form or the nested instance of `family` (sized by `values`) seen at pixel `px` of a picture taken through
/// `cam`, the family at the origin as {3D} shows it: the one whose surface there is nearest the eye. A void, never seen, is
/// not; nor is a form or an instance its `visible` leaves out.
pub fn part_at_3d(family: &Family, values: &BTreeMap<String, Val>, cam: &view3d::Camera, px: (f64, f64)) -> Option<Part> {
    let flexed = family.flexed(values).ok()?;
    let mut read = HashMap::new();
    let mut best: Option<(f64, Part)> = None;
    let mut seen = |tris: &[[P3; 3]], part: Part| {
        for t in tris {
            if let Some(z) = depth_at(t.map(|p| cam.project(p)), px)
                && best.is_none_or(|(b, _)| z > b)
            {
                best = Some((z, part));
            }
        }
    };
    for i in 0..flexed.forms.len() {
        if flexed.forms[i].void {
            continue;
        }
        if let Some(piece) = flexed.solid(i, values, &mut read).ok().flatten() {
            seen(&piece.tris, Part::Form(i));
        }
    }
    for i in 0..family.nested_instances.len() {
        for piece in family.instance_pieces(i, values) {
            seen(&piece.tris, Part::Nested(i));
        }
    }
    best.map(|(_, part)| part)
}

/// How near the eye triangle `q` (its corners projected: pixel and nearness) is at pixel `p`, where it covers it.
fn depth_at(q: [(f64, f64, f64); 3], p: (f64, f64)) -> Option<f64> {
    if q.iter().any(|(x, y, z)| !(x.is_finite() && y.is_finite() && z.is_finite())) {
        return None;
    }
    let [a, b, c] = q;
    let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
    if area.abs() < 1e-9 {
        return None;
    }
    let wb = ((p.0 - a.0) * (c.1 - a.1) - (p.1 - a.1) * (c.0 - a.0)) / area;
    let wc = ((b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)) / area;
    let wa = 1.0 - wb - wc;
    let e = -1e-9;
    (wa >= e && wb >= e && wc >= e).then_some(a.2 * wa + b.2 * wb + c.2 * wc)
}

/// The outline form `i` of `family` is seen with in view `which`: the hull of its solid's corners there. Empty for a form
/// that builds nothing.
pub fn outline(family: &Family, values: &BTreeMap<String, Val>, st: &Staged, which: Which, i: usize) -> Vec<P2> {
    if i >= family.forms.len() {
        return Vec::new();
    }
    let Some(piece) = family.solid(i, values, &mut HashMap::new()).ok().flatten() else { return Vec::new() };
    crate::geom::hull(piece.tris.iter().flatten().map(|p| st.to_view(which, *p)).collect())
}

/// Whether `p` is inside triangle `t` (either way round), its edges counting.
fn inside(t: &[P2; 3], p: P2) -> bool {
    let side = |a: P2, b: P2| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let s = [side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0])];
    let area = (t[1].0 - t[0].0) * (t[2].1 - t[0].1) - (t[1].1 - t[0].1) * (t[2].0 - t[0].0);
    area.abs() > 1e-9 && (s.iter().all(|x| *x >= -1e-9) || s.iter().all(|x| *x <= 1e-9))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::json;

    /// A box 500 wide, 300 deep, `[z0, z1]` high, off to the east (+x) and the back (+y), and a plane of the family's own.
    fn boxed(z0: f64, z1: f64) -> Family {
        let mut f: Family = serde_json::from_value(json!({
            "category": "generic",
            "types": {"T": {}},
            "forms": [{"profile": [[500, 300], [1000, 300], [1000, 600], [500, 600]], "from": z0, "to": z1}],
        }))
        .unwrap();
        f.planes.push(crate::family::RefPlane {
            id: 0,
            guid: Guid::new(),
            name: "Edge".into(),
            start: [-400.0, -2000.0, 0.0],
            end: [-400.0, 2000.0, 0.0],
            up: [0.0, 0.0, 1.0],
            reference: Default::default(),
            defines_origin: false,
            subcategory: None,
            level: false,
            pivot: None,
            turned: None,
        });
        f
    }

    fn staged(f: &Family) -> Staged {
        let mut s = Session::new();
        s.model_mut().families.insert("Box".into(), f.clone());
        s.execute("component", &json!({"family": "Box", "type": "T", "at": [0, 0]})).unwrap();
        let values = f.values("T", &BTreeMap::new()).unwrap();
        stage(std::sync::Arc::unwrap_or_clone(s.model.clone()), f, &values, &|_| true)
    }

    fn pixel(s: &Shown, at: (f32, f32)) -> [u8; 4] {
        let (x, y) = (at.0.round() as usize, at.1.round() as usize);
        let i = (y * s.view.width as usize + x) * 4;
        [s.rgba[i], s.rgba[i + 1], s.rgba[i + 2], s.rgba[i + 3]]
    }

    fn green(c: [u8; 4]) -> bool {
        i16::from(c[1]) - i16::from(c[0]) > 12 && i16::from(c[1]) - i16::from(c[2]) > 8
    }

    /// Whether a pixel near `at` (within 2) is green.
    fn green_near(s: &Shown, at: (f32, f32)) -> bool {
        (-2..=2).any(|dx| (-2..=2).any(|dy| green(pixel(s, (at.0 + dx as f32, at.1 + dy as f32)))))
    }

    /// F10: a window's frame, one extrusion across the wall with its light a hole, is cut in plan as its two jambs, as the
    /// plane cuts it, not outlined whole: in the family's plan and the project's.
    #[test]
    fn a_window_familys_plan_cuts_its_frame_as_its_jambs() {
        let f: Family = serde_json::from_value(json!({
            "category": "window",
            "parameters": [{"name": "Width", "value": 1200}, {"name": "Height", "value": 1500}],
            "types": {"T": {}},
            "forms": [{"host": {"plane": crate::family::CENTER_FRONT_BACK}, "from": -40, "to": 40, "sketch": [
                {"start": [-600, 0], "end": [600, 0]}, {"start": [600, 0], "end": [600, 1500]},
                {"start": [600, 1500], "end": [-600, 1500]}, {"start": [-600, 1500], "end": [-600, 0]},
                {"start": [-550, 50], "end": [-550, 1450]}, {"start": [-550, 1450], "end": [550, 1450]},
                {"start": [550, 1450], "end": [550, 50]}, {"start": [550, 50], "end": [-550, 50]}]}],
        }))
        .unwrap();
        let mut s = Session::new();
        s.model_mut().families.insert("W".into(), f.clone());
        s.execute("wall", &json!({"points": [[-2500, 0], [2500, 0]]})).unwrap();
        let window = s.execute("window", &json!({"at": [0, 0], "family": "W", "type": "T", "sill": 0})).unwrap()["id"].as_u64().unwrap();
        let values = f.values("T", &BTreeMap::new()).unwrap();
        let mut st = stage(std::sync::Arc::unwrap_or_clone(s.model.clone()), &f, &values, &|_| true);
        let d = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap().drawing;
        let cut = d
            .prims
            .iter()
            .zip(&d.parts)
            .zip(&d.owners)
            .filter(|((p, part), owner)| {
                **part == render::Part::Cut && owner.is_some_and(|o| o == window) && matches!(p, render::Prim::Line { closed: true, .. })
            })
            .count();
        assert_eq!(cut, 2, "each jamb cut");
        // Each a 50 by 80 ring, at either end of the opening.
        let rings: Vec<Vec<P2>> = d
            .prims
            .iter()
            .zip(&d.parts)
            .zip(&d.owners)
            .filter(|((_, part), owner)| **part == render::Part::Cut && owner.is_some_and(|o| o == window))
            .filter_map(|((p, _), _)| match p {
                render::Prim::Line { pts, closed: true, .. } => Some(pts.iter().map(|q| d.view.mm(*q)).collect()),
                _ => None,
            })
            .collect();
        for ring in &rings {
            let (x0, x1) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.min(q.0), b.max(q.0)));
            assert!((x1 - x0 - 50.0).abs() < 2.0 && (x0.abs() - 575.0).abs() < 30.0, "a jamb: {x0} to {x1}");
        }
    }

    /// The lines drawn in projection of what `window` owns in `d`, in mm: each straight piece.
    fn seen_lines(d: &render::Drawing, window: crate::model::Id) -> Vec<(P2, P2)> {
        d.prims
            .iter()
            .zip(&d.parts)
            .zip(&d.owners)
            .filter(|((_, part), owner)| **part == render::Part::Projection && **owner == Some(window))
            .flat_map(|((p, _), _)| match p {
                render::Prim::Line { pts, .. } => pts.windows(2).map(|w| (d.view.mm(w[0]), d.view.mm(w[1]))).collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    /// F10b: a window's forms below the plan's cut plane are seen from it, in projection, as walls below the cut are: the
    /// frame's lower rail between its cut jambs, and a sill board outside the wall; its head and a hood over the cut are not
    /// drawn. In the family's plan and the project's.
    #[test]
    fn a_window_familys_plan_sees_its_forms_below_the_cut_and_not_above() {
        let f: Family = serde_json::from_value(json!({
            "category": "window",
            "parameters": [{"name": "Width", "value": 1200}, {"name": "Height", "value": 1500}],
            "types": {"T": {}},
            "forms": [
                {"host": {"plane": crate::family::CENTER_FRONT_BACK}, "from": -40, "to": 40, "sketch": [
                    {"start": [-600, 0], "end": [600, 0]}, {"start": [600, 0], "end": [600, 1500]},
                    {"start": [600, 1500], "end": [-600, 1500]}, {"start": [-600, 1500], "end": [-600, 0]},
                    {"start": [-550, 50], "end": [-550, 1450]}, {"start": [-550, 1450], "end": [550, 1450]},
                    {"start": [550, 1450], "end": [550, 50]}, {"start": [550, 50], "end": [-550, 50]}]},
                {"profile": [[-650, -150], [650, -150], [650, -40], [-650, -40]], "from": 0, "to": 30},
                {"profile": [[-650, 40], [650, 40], [650, 160], [-650, 160]], "from": 1400, "to": 1450}
            ],
        }))
        .unwrap();
        let mut s = Session::new();
        s.model_mut().families.insert("W".into(), f.clone());
        s.execute("wall", &json!({"points": [[-2500, 0], [2500, 0]]})).unwrap();
        let window = s.execute("window", &json!({"at": [0, 0], "family": "W", "type": "T", "sill": 0})).unwrap()["id"].as_u64().unwrap();
        let values = f.values("T", &BTreeMap::new()).unwrap();
        let mut st = stage(std::sync::Arc::unwrap_or_clone(s.model.clone()), &f, &values, &|_| true);
        let family = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap().drawing;
        let staged_window =
            st.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Window(_))).map(|(id, _)| *id).unwrap();
        let view = render::View { scale: 0.2, cx: 0.0, cy: 0.0, width: 600, height: 400 };
        let project = render::draw(&s.model, 100000, view);
        for (d, owner, which) in [(&family, staged_window, "family"), (&project, window, "project")] {
            let lines = seen_lines(d, owner);
            let along = |y: f64, x0: f64, x1: f64| {
                lines.iter().any(|(a, b)| (a.1 - y).abs() < 2.0 && (b.1 - y).abs() < 2.0 && a.0.min(b.0) <= x0 && a.0.max(b.0) >= x1)
            };
            let at = |y: f64| lines.iter().any(|(a, b)| (a.1 - y).abs() < 2.0 && (b.1 - y).abs() < 2.0 && a.dist(*b) > 50.0);
            // The lower rail's faces between the jambs, which the cut covers.
            let rail = [-40.0, 40.0].iter().filter(|y| along(**y, -500.0, 500.0)).count();
            assert_eq!(rail, 2, "{which}: the lower rail between the jambs: {lines:?}");
            assert!(
                !lines.iter().any(|(a, b)| (a.1.abs() - 40.0).abs() < 2.0 && a.0.abs().min(b.0.abs()) > 560.0),
                "{which}: not over the cut jambs: {lines:?}"
            );
            // The sill board's outer edge on one side, all along it; nothing of the hood over the cut on the other.
            let (board, hood) = (at(150.0) || at(-150.0), at(160.0) || at(-160.0));
            assert!(board, "{which}: the sill board below the cut: {lines:?}");
            assert!(!hood, "{which}: nothing above the cut: {lines:?}");
            // Lighter than the cut: the projection pen.
            let width = |part: render::Part| {
                d.prims.iter().zip(&d.parts).zip(&d.owners).filter(|((_, p), o)| **p == part && **o == Some(owner)).find_map(
                    |((p, _), _)| match p {
                        render::Prim::Line { width, .. } => Some(*width),
                        _ => None,
                    },
                )
            };
            assert!(width(render::Part::Projection).unwrap() < width(render::Part::Cut).unwrap(), "{which}: lighter than the cut");
        }
    }

    #[test]
    fn the_plan_cuts_a_form_crossing_its_cut_plane_and_sees_the_others() {
        for (z0, z1, crossing) in [(0.0, 2000.0, true), (0.0, 800.0, false), (1500.0, 2500.0, false)] {
            let f = boxed(z0, z1);
            assert_eq!(
                plan_cut(&f.pieces(&f.values("T", &BTreeMap::new()).unwrap()).unwrap()[0].tris, 1200.0).len(),
                usize::from(crossing)
            );
            let mut st = staged(&f);
            let shown = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
            let d = &shown.drawing;
            let cut = d
                .prims
                .iter()
                .zip(&d.parts)
                .filter(|(p, part)| **part == render::Part::Cut && matches!(p, render::Prim::Line { closed: true, .. }))
                .count();
            assert_eq!(cut > 0, crossing, "from {z0} to {z1}: cut only where it crosses 1200");
            // Seen whatever its height: ink along its south edge.
            let at = shown.view.px(P2(750.0, 300.0));
            let dark = (-2..=2).any(|dy| pixel(&shown, (at.0, at.1 + dy as f32))[0] < 200);
            assert!(dark, "from {z0} to {z1}: its outline is drawn");
        }
        // The cut is wider than what is seen: the cut pen.
        let mut st = staged(&boxed(0.0, 2000.0));
        let d = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap().drawing;
        let width = |part: render::Part| {
            d.prims
                .iter()
                .zip(&d.parts)
                .filter(|(_, p)| **p == part)
                .filter_map(|(p, _)| match p {
                    render::Prim::Line { width, .. } => Some(*width),
                    _ => None,
                })
                .fold(0.0f32, f32::max)
        };
        assert!(width(render::Part::Cut) > width(render::Part::Projection), "cut heavier than seen");
    }

    #[test]
    fn the_plan_draws_the_center_planes_and_the_familys_own_dashed_green_named() {
        let mut st = staged(&boxed(0.0, 2000.0));
        assert_eq!(st.planes.len(), 3, "the two Center datums and Edge; Ref. Level is the level");
        let shown = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
        for (x, y) in [(0.0, 1000.0), (1000.0, 0.0), (-400.0, 1000.0)] {
            let at = shown.view.px(P2(x, y));
            // Dashed: somewhere within a dash's length along it.
            let along_x = x == 1000.0;
            let some = (0..20).any(|k| {
                let d = k as f32;
                green_near(&shown, if along_x { (at.0 + d, at.1) } else { (at.0, at.1 + d) })
            });
            assert!(some, "a green trace at ({x}, {y})");
        }
        assert!(st.model.levels().len() == 1 && st.model.levels()[0].1.name == "Ref. Level");
    }

    /// F10: Ref. Level, drawn as a level in an elevation, is picked there as the family's datum, as a plane's trace is.
    #[test]
    fn ref_level_drawn_as_a_level_in_an_elevation_is_picked_as_the_datum() {
        let mut st = staged(&boxed(0.0, 1500.0));
        let shown = st.draw(Which::Elevation(Side::Front), 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
        let at = shown.view.px(st.to_view(Which::Elevation(Side::Front), [-1300.0, 0.0, 0.0]));
        let picked: Vec<Guid> = shown.drawing.pick_all(at, 6.0).into_iter().filter_map(|id| st.plane_of(id)).collect();
        assert_eq!(picked.first(), Some(&super::super::REF_LEVEL), "{picked:?}");
    }

    /// Each elevation sees the box (east of the origin, behind it) on its own side of the Center plane across it: Front
    /// (looking north) on the right, Back on the left, Left (looking east) on the left, Right on the right, as Revit's.
    #[test]
    fn each_elevation_looks_from_its_own_side() {
        let mut st = staged(&boxed(0.0, 1500.0));
        for (side, right) in [(Side::Front, true), (Side::Back, false), (Side::Left, false), (Side::Right, true)] {
            let shown = st.draw(Which::Elevation(side), 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
            let centre = shown.view.px(st.to_view(Which::Elevation(side), [0.0, 0.0, 0.0])).0;
            let row = shown.view.px(P2(0.0, 1000.0)).1;
            // The box's edges, dark, at half its height.
            let dark: Vec<f32> = (0..shown.view.width)
                .map(|x| x as f32)
                .filter(|x| {
                    let c = pixel(&shown, (*x, row));
                    c[0] < 100 && c[1] < 100 && c[2] < 100
                })
                .collect();
            assert!(!dark.is_empty(), "{side:?}: the box is seen");
            assert!(
                dark.iter().all(|x| (*x > centre) == right),
                "{side:?}: the box on the {} of {centre}: {dark:?}",
                if right { "right" } else { "left" }
            );
            // Ref. Level, at height 0, as a level: its name.
            assert!(shown.drawing.prims.len() > 4);
            // The Center plane across it, dashed green.
            let at = shown.view.px(st.to_view(Which::Elevation(side), [0.0, 0.0, 1000.0]));
            assert!((0..20).any(|k| green_near(&shown, (at.0, at.1 + k as f32))), "{side:?}: the Center plane across it");
        }
    }

    #[test]
    fn a_plane_drawn_in_plan_stands_upright_and_in_an_elevation_faces_across_it() {
        let st = staged(&boxed(0.0, 500.0));
        let (s, e, up) = new_plane(&st, Which::Plan, P2(0.0, -500.0), P2(0.0, 500.0)).unwrap();
        assert_eq!((s, e, up), ([0.0, -500.0, 0.0], [0.0, 500.0, 0.0], [0.0, 0.0, 1.0]));
        for side in Side::ALL {
            let w = Which::Elevation(side);
            let (a, b) = (st.to_view(w, [0.0, 0.0, 100.0]), st.to_view(w, [0.0, 0.0, 900.0]));
            let (s, e, up) = new_plane(&st, w, a, b).unwrap();
            assert!(view3d::dot(view3d::sub(s, [0.0, 0.0, 100.0]), view3d::sub(s, [0.0, 0.0, 100.0])) < 1e-6, "{side:?}: {s:?}");
            assert!((e[2] - 900.0).abs() < 1e-6);
            let normal = view3d::cross(view3d::sub(e, s), up);
            let l = side.looking();
            assert!((normal[0] * l.0 + normal[1] * l.1).abs() < 1e-6, "{side:?}: its normal lies in the elevation's plane");
        }
        assert!(new_plane(&st, Which::Plan, P2(1.0, 1.0), P2(1.0, 1.0)).is_none());
    }

    /// Go To View's list: the views that show a plane flat, either side of it; none for an askew one.
    #[test]
    fn the_views_that_show_a_plane_flat_are_the_plans_for_a_level_one_and_two_elevations_for_an_upright_one() {
        assert_eq!(flat_views([0.0, 0.0, -1.0]), [Which::Plan, Which::Ceiling]);
        assert_eq!(flat_views([0.0, -1.0, 0.0]), [Which::Elevation(Side::Back), Which::Elevation(Side::Front)]);
        assert_eq!(flat_views([1.0, 0.0, 0.0]), [Which::Elevation(Side::Left), Which::Elevation(Side::Right)]);
        assert!(flat_views([1.0, 1.0, 0.0]).is_empty(), "askew to every elevation");
        assert!(Which::Plan.faces([0.0, 0.0, 2.0]) && !Which::Plan.faces([0.0, 1.0, 0.0]));
        assert!(!Which::Plan.faces([0.0, 0.0, 0.0]));
    }

    /// Pick a line: the reference line drawn nearest the click, within reach; one seen end on is a point.
    #[test]
    fn a_reference_line_is_found_where_a_view_draws_it() {
        let mut f = boxed(0.0, 500.0);
        let line = |start: P3, end: P3, host: Guid| crate::family::RefLine {
            guid: Guid::new(),
            start,
            end,
            host: crate::family::PlaneRef::Plane(host),
            ..Default::default()
        };
        f.lines.push(line([-1200.0, -1000.0, 0.0], [-600.0, -1000.0, 0.0], crate::family::REF_LEVEL));
        f.lines.push(line([-1500.0, 0.0, 0.0], [-1500.0, 0.0, 1200.0], crate::family::CENTER_FRONT_BACK));
        let st = staged(&f);
        assert_eq!(st.line_at(Which::Plan, P2(-900.0, -1010.0), 20.0), Some(0));
        assert_eq!(st.line_at(Which::Plan, P2(-900.0, -1100.0), 20.0), None, "out of reach");
        assert_eq!(st.line_at(Which::Plan, P2(-1505.0, 0.0), 20.0), Some(1), "end on, a point");
        let front = Which::Elevation(Side::Front);
        let at = st.to_view(front, [-1500.0, 0.0, 600.0]);
        assert_eq!(st.line_at(front, at, 5.0), Some(1));
    }

    /// The family's reference lines in its plan and its Front elevation: solid green, thinner than the planes' dashed traces,
    /// each as the view sees it; one seen end on (upright, in plan) draws nothing there.
    #[test]
    fn reference_lines_are_drawn_solid_green_and_thin_in_plan_and_in_an_elevation() {
        let mut f = boxed(0.0, 500.0);
        let line = |start: P3, end: P3, host: Guid| crate::family::RefLine {
            guid: Guid::new(),
            start,
            end,
            host: crate::family::PlaneRef::Plane(host),
            ..Default::default()
        };
        // One on Ref. Level, in front; one upright on Center (Front/Back).
        f.lines.push(line([-1200.0, -1000.0, 0.0], [-600.0, -1000.0, 0.0], crate::family::REF_LEVEL));
        f.lines.push(line([-1500.0, 0.0, 0.0], [-1500.0, 0.0, 1200.0], crate::family::CENTER_FRONT_BACK));
        let mut st = staged(&f);
        let solid = |d: &Drawing| -> Vec<f32> {
            d.prims
                .iter()
                .zip(&d.owners)
                .filter_map(|(p, o)| match p {
                    render::Prim::Line { dash: None, color, width, .. } if o.is_none() && *color == render::REFERENCE_PLANE => Some(*width),
                    _ => None,
                })
                .collect()
        };
        let dashed = |d: &Drawing| -> f32 {
            d.prims
                .iter()
                .filter_map(|p| match p {
                    render::Prim::Line { dash: Some(_), color, width, .. } if *color == render::REFERENCE_PLANE => Some(*width),
                    _ => None,
                })
                .fold(f32::MAX, f32::min)
        };
        let plan = st.draw(Which::Plan, 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
        assert_eq!(solid(&plan.drawing).len(), 1, "in plan the level line; the upright one is seen end on");
        assert!(solid(&plan.drawing).iter().all(|w| *w < dashed(&plan.drawing)), "thinner than a plane");
        // Green all along it, unbroken.
        for x in [-1150.0, -1000.0, -900.0, -800.0, -650.0] {
            assert!(green_near(&plan, plan.view.px(P2(x, -1000.0))), "in plan at {x}");
        }
        assert!(!green_near(&plan, plan.view.px(P2(-1000.0, -1300.0))), "nothing beside it");
        let front = Which::Elevation(Side::Front);
        let shown = st.draw(front, 600, 400, 1.0, P2(0.0, 0.0)).unwrap();
        assert_eq!(solid(&shown.drawing).len(), 2, "in Front both");
        for x in [-1150.0, -900.0, -650.0] {
            assert!(green_near(&shown, shown.view.px(st.to_view(front, [x, -1000.0, 0.0]))), "across Front at {x}");
        }
        for z in [100.0, 600.0, 1100.0] {
            assert!(green_near(&shown, shown.view.px(st.to_view(front, [-1500.0, 0.0, z]))), "upright in Front at {z}");
        }
    }

    /// A plane drawn into a picture of the box through its middle (level at 1000): where the box stands in front of it (above
    /// it, seen from above) the box keeps its colours; where the box is behind it, and where nothing is, the plane tints.
    #[test]
    fn a_plane_drawn_into_a_picture_is_hidden_by_forms_in_front_of_it() {
        let f = boxed(0.0, 2000.0);
        let st = staged(&f);
        let scene = view3d::Scene::new(&st.model);
        let cam = scene.fit(view3d::AZIMUTH, view3d::ELEVATION, 400, 300);
        let before = scene.render(&cam, &[]);
        let mut after = before.clone();
        let (across, up) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let tint = [30, 70, 140, 60];
        after.draw_plane(&cam, [0.0, 0.0, 1000.0], (across, up), ((-3000.0, -3000.0), (3000.0, 3000.0)), 100.0, tint, tint, tint);
        let (mut front, mut behind, mut empty) = (0, 0, 0);
        for y in 0..300 {
            for x in 0..400 {
                let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let k = 4 * (y * 400 + x) as usize;
                let changed = before.rgba[k..k + 3] != after.rgba[k..k + 3];
                match before.point(&cam, fx, fy) {
                    Some(p) if p[2] > 1050.0 => {
                        assert!(!changed, "({x}, {y}) on the box above the plane keeps its colour");
                        front += 1;
                    }
                    Some(p) if p[2] < 950.0 && cam.on((fx, fy), [0.0, 0.0, 1000.0], [0.0, 0.0, 1.0]).is_some() => {
                        assert!(changed, "({x}, {y}) on the box below the plane is tinted");
                        behind += 1;
                    }
                    None if cam
                        .on((fx, fy), [0.0, 0.0, 1000.0], [0.0, 0.0, 1.0])
                        .is_some_and(|q| q[0].abs() < 2900.0 && q[1].abs() < 2900.0) =>
                    {
                        assert!(changed, "({x}, {y}) on nothing is tinted");
                        empty += 1;
                    }
                    _ => {}
                }
            }
        }
        assert!(front > 50 && behind > 50 && empty > 50, "{front} {behind} {empty}");
    }

    /// A mark on the box's top, where it lies, shows there (no face it lies on hides it); a line through the box is hidden
    /// where the box stands in front of it; and {3D}'s picking finds the box where it is seen and nothing beside it.
    #[test]
    fn a_mark_shows_on_a_face_it_lies_on_a_line_is_hidden_inside_a_form_and_3d_finds_the_form_seen() {
        let f = boxed(0.0, 2000.0);
        let st = staged(&f);
        let scene = view3d::Scene::new(&st.model);
        let cam = scene.fit(view3d::AZIMUTH, view3d::ELEVATION, 400, 300);
        let before = scene.render(&cam, &[]);
        let red = [200, 0, 0, 255];
        let level = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let mark = view3d::PlaneMark {
            origin: [0.0, 0.0, 2000.0],
            axes: level,
            lo: (-3000.0, -3000.0),
            hi: (3000.0, 3000.0),
            step: 0.0,
            fill: red,
            grid: red,
            edge: red,
            edge_width: 1.0,
        };
        let mut after = before.clone();
        after.draw_marks(&cam, &[mark]);
        for (x, y) in [(600.0, 350.0), (750.0, 450.0), (950.0, 550.0)] {
            let (px, py, _) = cam.project([x, y, 2000.0]);
            assert_eq!(after.color(px, py), crate::render::Rgb(200, 0, 0), "the box's top at ({x}, {y}) shows the mark");
        }
        // A line west to east through the box at 1000: hidden where the box is in front of it, drawn where nothing is.
        let mut lined = before.clone();
        lined.draw_line(&cam, [[-1000.0, 450.0, 1000.0], [2500.0, 450.0, 1000.0]], 3.0, red);
        let at = |x: f64| cam.project([x, 450.0, 1000.0]);
        let (inside, outside) = (at(750.0), at(-500.0));
        assert_eq!(lined.color(inside.0, inside.1), before.color(inside.0, inside.1), "hidden inside the box");
        assert_eq!(lined.color(outside.0, outside.1), crate::render::Rgb(200, 0, 0), "drawn where nothing hides it");
        let values = f.values("T", &BTreeMap::new()).unwrap();
        let top = cam.project([750.0, 450.0, 2000.0]);
        assert_eq!(part_at_3d(&f, &values, &cam, (top.0, top.1)), Some(Part::Form(0)));
        assert_eq!(part_at_3d(&f, &values, &cam, (2.5, 2.5)), None);
    }

    #[test]
    fn a_form_is_found_where_it_is_drawn() {
        let f = boxed(0.0, 500.0);
        let st = staged(&f);
        let values = f.values("T", &BTreeMap::new()).unwrap();
        assert_eq!(form_at(&f, &values, &st, Which::Plan, P2(750.0, 450.0)), Some(0));
        assert_eq!(form_at(&f, &values, &st, Which::Plan, P2(-750.0, 450.0)), None);
        let front = Which::Elevation(Side::Front);
        assert_eq!(form_at(&f, &values, &st, front, st.to_view(front, [750.0, 0.0, 250.0])), Some(0));
        assert_eq!(form_at(&f, &values, &st, front, st.to_view(front, [750.0, 0.0, 900.0])), None);
        assert!(!outline(&f, &values, &st, front, 0).is_empty());
    }

    /// A box family's planes, the datums with Left, Right, Front, Back and Top of its own, drawn in every view: no plane's
    /// name lies on any plane's trace, its own or another's.
    #[test]
    fn no_plane_name_lies_on_a_trace_in_any_view() {
        use crate::render::{Part, Prim};
        let mut f = boxed(0.0, 900.0);
        f.planes.retain(|p| p.datum());
        let plane = |name: &str, start: [f64; 3], end: [f64; 3], up: [f64; 3]| crate::family::RefPlane {
            id: 0,
            guid: Guid::new(),
            name: name.into(),
            start,
            end,
            up,
            reference: Default::default(),
            defines_origin: false,
            subcategory: None,
            level: false,
            pivot: None,
            turned: None,
        };
        let up = [0.0, 0.0, 1.0];
        f.planes.extend([
            plane("Left", [-500.0, -2000.0, 0.0], [-500.0, 2000.0, 0.0], up),
            plane("Right", [500.0, -2000.0, 0.0], [500.0, 2000.0, 0.0], up),
            plane("Front", [-2000.0, -300.0, 0.0], [2000.0, -300.0, 0.0], up),
            plane("Back", [-2000.0, 300.0, 0.0], [2000.0, 300.0, 0.0], up),
            plane("Top", [-2000.0, 0.0, 900.0], [2000.0, 0.0, 900.0], [0.0, 1.0, 0.0]),
        ]);
        let mut st = staged(&f);
        let views = [Which::Plan, Which::Ceiling].into_iter().chain(Side::ALL.iter().map(|s| Which::Elevation(*s)));
        for which in views {
            for (w, h) in [(1200, 800), (900, 900)] {
                let shown = st.draw(which, w, h, 1.0, P2(0.0, 0.0)).unwrap();
                if let Ok(dir) = std::env::var("TMP_DUMP") {
                    let (x0, y0, cw, ch) = (shown.view.width / 2 - 200, 0u32, 400u32, 120u32);
                    let mut px = Vec::new();
                    for y in y0..y0 + ch {
                        for x in x0..x0 + cw {
                            let i = ((y * shown.view.width + x) * 4) as usize;
                            px.extend_from_slice(&shown.rgba[i..i + 4]);
                        }
                    }
                    let pm = tiny_skia::Pixmap::from_vec(px, tiny_skia::IntSize::from_wh(cw, ch).unwrap()).unwrap();
                    std::fs::write(format!("{dir}/{which:?}-{w}.png"), pm.encode_png().unwrap()).unwrap();
                }
                let d = &shown.drawing;
                let green = crate::render::REFERENCE_PLANE;
                let traces: Vec<((f32, f32), (f32, f32))> = d
                    .prims
                    .iter()
                    .filter_map(|p| match p {
                        Prim::Line { pts, dash: Some(_), color, .. } if *color == green && pts.len() == 2 => Some((pts[0], pts[1])),
                        _ => None,
                    })
                    .collect();
                assert!(!traces.is_empty(), "{which:?}: its traces");
                let mut names: BTreeMap<Id, [f32; 4]> = BTreeMap::new();
                for ((prim, o), part) in d.prims.iter().zip(&d.owners).zip(&d.parts) {
                    if let (Prim::Line { pts, color, .. }, Some(id), Part::Text) = (prim, o, part)
                        && *color == green
                    {
                        let b = names.entry(*id).or_insert([f32::MAX, f32::MAX, f32::MIN, f32::MIN]);
                        for &(x, y) in pts {
                            *b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                        }
                    }
                }
                assert!(names.len() >= 3, "{which:?}: its planes' names: {}", names.len());
                for (id, r) in &names {
                    for (a, b) in &traces {
                        // Sampled along the trace: a point of it inside the name's box is a name on a trace.
                        let on = (0..=2000).any(|i| {
                            let t = i as f32 / 2000.0;
                            let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                            x > r[0] && x < r[2] && y > r[1] && y < r[3]
                        });
                        assert!(!on, "{which:?} {w}x{h}: the name of {id} at {r:?} lies on the trace {a:?}-{b:?}");
                    }
                }
            }
        }
    }
}
