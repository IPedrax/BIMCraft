//! A family's views (F4), as Revit's family editor has them: the Ref. Level floor and ceiling plans and the four elevations,
//! Front, Back, Left and Right. Each is the project's own drawing of a scratch model the family is placed in (`stage`): its
//! level is Ref. Level, each of the family's reference planes is a project reference plane there (named as it is, mapped back
//! to the family's plane by GUID; neither the project nor the family is touched), and the four elevations look at it from
//! outside, Front looking north (+y, the family's back), as Revit's do. The plan cuts the forms at its cut plane
//! ([`plan_cut`]); a project's plans never cut a component.

use std::collections::{BTreeMap, HashMap};

use super::{Family, Val};
use crate::geom::P2;
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
}

/// How far the elevations stand outside what they look at.
const OUTSIDE: f64 = 1000.0;

/// Model `m`, the family placed in it (at the origin, unturned: a component, or a door or a window in its wall), staged for
/// its views: the first level renamed Ref. Level and the others taken away; each of `family`'s planes `shown` a reference
/// plane of the model, across `room`; the four elevations. What `family` takes up is worked out for `values`.
pub fn stage(mut m: Model, family: &Family, values: &BTreeMap<String, Val>, shown: &dyn Fn(Guid) -> bool) -> Staged {
    let levels: Vec<Id> = m.levels().iter().map(|(id, _)| *id).collect();
    let level = levels.first().copied().unwrap_or_else(|| m.insert(Element::Level(crate::model::Level::new("Ref. Level", 0.0))));
    for id in levels.iter().skip(1) {
        m.elements.remove(*id);
    }
    if let Some(Element::Level(l)) = m.elements.get_mut(level) {
        l.name = "Ref. Level".into();
    }
    // What it takes up in any type: a form the type hides is still drawn, halftone.
    let mut every = family.clone();
    every.forms.iter_mut().for_each(|f| f.visible = None);
    let shapes = every.shapes(values);
    let lo: P3 = std::array::from_fn(|i| shapes.iter().map(|s| s.min[i]).fold(f64::INFINITY, f64::min));
    let hi: P3 = std::array::from_fn(|i| shapes.iter().map(|s| s.max[i]).fold(f64::NEG_INFINITY, f64::max));
    let (lo, hi) = if lo.iter().chain(&hi).all(|v| v.is_finite()) { (lo, hi) } else { ([-1000.0; 3], [1000.0; 3]) };
    let room: (P3, P3) = (
        std::array::from_fn(|i| if i == 2 { (lo[i] - 300.0).min(0.0) } else { (lo[i] - 300.0).min(-1500.0) }),
        std::array::from_fn(|i| (hi[i] + 300.0).max(1500.0)),
    );
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
    Staged { model: m, level, elevations, planes, room }
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
        let scale = 0.75 * (w / (hi.0 - lo.0).max(1.0)).min(h / (hi.1 - lo.1).max(1.0)) * zoom.clamp(0.05, 50.0);
        View { scale, cx: (lo.0 + hi.0) / 2.0 + pan.0, cy: (lo.1 + hi.1) / 2.0 + pan.1, width: width.max(16), height: height.max(16) }
    }

    /// View `which`, `width` by `height` pixels, `zoom` times as close as fitted, moved `pan` mm. Its scale is the one at
    /// which, fitted, its annotation and pens are their size on paper, so the names read as a sheet's would, and grow
    /// and shrink as it is zoomed, as a project's views' do.
    pub fn draw(&mut self, which: Which, width: u32, height: u32, zoom: f64, pan: P2) -> Result<Shown, String> {
        let fitted = self.frame(which, width, height, 1.0, P2(0.0, 0.0));
        let n = (crate::sheet::DENSITY / fitted.scale).clamp(1.0, 1e5);
        if let Some(g) = self.view_id(which).and_then(|id| self.model.elements.get_mut(id)).and_then(crate::graphics::graphics_mut) {
            g.scale = Some(n);
        }
        let m = &self.model;
        let view = self.frame(which, width, height, zoom, pan);
        match which {
            Which::Plan => {
                let drawing = render::family_plan(m, self.level, view);
                let rgba = render::rgba(&drawing, None)?;
                Ok(Shown { view, drawing, rgba })
            }
            Which::Ceiling => {
                let drawing = render::draw_ceiling(m, self.level, view);
                let rgba = render::rgba(&drawing, None)?;
                Ok(Shown { view, drawing, rgba })
            }
            Which::Elevation(side) => {
                let id = self.elevation(side).ok_or("the family has no such elevation")?;
                let picture = render::section_picture(m, id, view, &[]).ok_or("the elevation can't be drawn")?;
                let drawing = render::section_over(m, id, view, &picture);
                let rgba = render::rgba(&drawing, Some(&picture))?;
                Ok(Shown { view, drawing, rgba })
            }
        }
    }

    /// The family's plane a reference plane of the scratch model drawn by `owner` stands for.
    pub fn plane_of(&self, owner: Id) -> Option<Guid> {
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
}
