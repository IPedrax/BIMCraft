//! The handles a selected wall or column has in a 3D view, as a section box's faces have theirs: the wall's two ends, an arrow on
//! its top and one under its base; a column's place, and its two arrows. Where each stands, what a drag to a new place is as a
//! command, and how the element would stand meanwhile are here, for any front end; a top or a base attached to a roof or a floor
//! follows that and has no arrow. Heights land on a level within reach ([`settled`]).

use serde_json::{Value, json};

use crate::geom::P2;
use crate::model::{Column, Element, Id, Model, Wall};
use crate::view3d::P3;
use crate::walls::{self, Axis};

/// What a drag from it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// A wall's start, moved over the level at its height.
    Start,
    /// A wall's end.
    End,
    /// A column's place in plan.
    Place,
    /// Its top, up or down.
    Top,
    /// Its base, up or down.
    Base,
}

impl Handle {
    /// Whether a drag moves it up and down, rather than over a level.
    pub fn vertical(self) -> bool {
        matches!(self, Handle::Top | Handle::Base)
    }
}

/// The handles of element `id`, each where it stands. None for what has none, and for an attached top's or base's arrow.
pub fn of(m: &Model, id: Id) -> Vec<(Handle, P3)> {
    match m.elements.get(id) {
        Some(Element::Wall(w)) => wall_handles(m, w),
        Some(e @ (Element::Column(c) | Element::StructuralColumn(c))) => column_handles(m, e, c),
        _ => Vec::new(),
    }
}

fn wall_handles(m: &Model, w: &Wall) -> Vec<(Handle, P3)> {
    let axis = Axis::of(w);
    let (ext, int) = walls::faces(m, w);
    let (mid, long) = ((ext + int) / 2.0, axis.length());
    let on = |s: f64, z: f64| {
        let q = axis.at(s, mid);
        [q.0, q.1, z]
    };
    // Its ends where Revit has them, at its base on its location line.
    let end = |s: f64| {
        let q = axis.at(s, 0.0);
        [q.0, q.1, walls::base_at(m, w, s)]
    };
    let mut out = vec![(Handle::Start, end(0.0)), (Handle::End, end(long))];
    if w.attach_top.is_none() {
        out.push((Handle::Top, on(long / 2.0, m.own_top(w))));
    }
    if w.attach_base.is_none() {
        out.push((Handle::Base, on(long / 2.0, m.own_base(w))));
    }
    out
}

fn column_handles(m: &Model, e: &Element, c: &Column) -> Vec<(Handle, P3)> {
    let (bottom, top) = m.column_span(e);
    let on = |z: f64| [c.at.0, c.at.1, z];
    let mut out = vec![(Handle::Place, on(bottom))];
    if c.attach_top.is_none() {
        out.push((Handle::Top, on(top)));
    }
    if c.attach_base.is_none() {
        out.push((Handle::Base, on(bottom)));
    }
    out
}

/// A selected wall's location line in a 3D view, as a plan shows it: along its base, from its start to its end; on a curved
/// wall, along its arc. None for what isn't a wall.
pub fn location_line(m: &Model, id: Id) -> Vec<P3> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let long = axis.length();
    // Sampled, so a curved one's arc and a base that follows a sloping floor show as they are.
    let n = if axis.centre().is_some() || w.attach_base.is_some() { 32 } else { 1 };
    (0..=n)
        .map(|i| {
            let s = long * f64::from(i) / f64::from(n);
            let q = axis.at(s, 0.0);
            [q.0, q.1, walls::base_at(m, w, s)]
        })
        .collect()
}

/// Height `z`, or the level within `reach` mm of it, nearest first: where a dragged top or base lands.
pub fn settled(m: &Model, z: f64, reach: f64) -> f64 {
    m.elements
        .values()
        .filter_map(|e| match e {
            Element::Level(l) => Some(l.elevation),
            _ => None,
        })
        .filter(|l| (l - z).abs() <= reach)
        .min_by(|a, b| (a - z).abs().total_cmp(&(b - z).abs()))
        .unwrap_or(z)
}

/// The command a drag of `handle` makes: to height `z` for an arrow, over to plan point `to` for the rest. A base keeps its
/// element's top where it is, as far as its top constraint lets it. None for a drag that changes nothing, or what has no such
/// handle.
pub fn command(m: &Model, id: Id, handle: Handle, to: P2, z: f64) -> Option<(&'static str, Value)> {
    const EPS: f64 = 1e-6;
    let vertical = |own_base: f64, own_top: f64, top_level: bool, height: f64, base_offset: f64, top_offset: f64| match handle {
        Handle::Top if (z - own_top).abs() <= EPS => None,
        Handle::Top if top_level => Some(json!({"id": id, "top_offset": top_offset + z - own_top})),
        Handle::Top => Some(json!({"id": id, "height": height + z - own_top})),
        Handle::Base if (z - own_base).abs() <= EPS => None,
        Handle::Base if top_level => Some(json!({"id": id, "base_offset": base_offset + z - own_base})),
        Handle::Base => Some(json!({"id": id, "base_offset": base_offset + z - own_base, "height": height - (z - own_base)})),
        _ => None,
    };
    match (m.elements.get(id)?, handle) {
        (Element::Wall(w), Handle::Start | Handle::End) => {
            let start = handle == Handle::Start;
            let by = to - if start { w.start } else { w.end };
            (by.length() > EPS).then(|| ("move", json!({"ids": [id], "by": [by.0, by.1], "end": if start { "start" } else { "end" }})))
        }
        (Element::Wall(w), Handle::Top | Handle::Base) => {
            vertical(m.own_base(w), m.own_top(w), w.top_level.is_some(), w.height, w.base_offset, w.top_offset).map(|p| ("modify", p))
        }
        (Element::Column(c) | Element::StructuralColumn(c), Handle::Place) => {
            let by = to - c.at;
            (by.length() > EPS).then(|| ("move", json!({"ids": [id], "by": [by.0, by.1]})))
        }
        (Element::Column(c) | Element::StructuralColumn(c), Handle::Top | Handle::Base) => {
            vertical(m.column_own_base(c), m.column_own_top(c), c.top_level.is_some(), c.height, c.base_offset, c.top_offset)
                .map(|p| ("modify", p))
        }
        _ => None,
    }
}

/// How element `id` would stand with `handle` dragged there, as closed outlines in space: a wall's rectangle along its line, a
/// column's outline at its base and its top.
pub fn preview(m: &Model, id: Id, handle: Handle, to: P2, z: f64) -> Vec<Vec<P3>> {
    let mid = |lo: f64, hi: f64| match handle {
        Handle::Top => (lo, z),
        Handle::Base => (z, hi),
        _ => (lo, hi),
    };
    match m.elements.get(id) {
        Some(Element::Wall(w)) => {
            let (lo, hi) = mid(m.base_z(w), m.top_z(w));
            let (a, b) = match handle {
                Handle::Start => (to, w.end),
                Handle::End => (w.start, to),
                _ => (w.start, w.end),
            };
            vec![vec![[a.0, a.1, lo], [b.0, b.1, lo], [b.0, b.1, hi], [a.0, a.1, hi]]]
        }
        Some(e @ (Element::Column(c) | Element::StructuralColumn(c))) => {
            let (lo, hi) = m.column_span(e);
            let (lo, hi) = mid(lo, hi);
            let by = if handle == Handle::Place { to - c.at } else { P2(0.0, 0.0) };
            let ring = m.column_outline(e);
            [lo, hi].iter().map(|h| ring.iter().map(|q| [q.0 + by.0, q.1 + by.1, *h]).collect()).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Session;
    use serde_json::Value;

    fn run(s: &mut Session, c: &str, p: Value) -> Value {
        s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
    }

    /// A house's one storey with a level above, a wall to its top level, a column and a roof over them.
    fn house() -> (Session, Id, Id, Id) {
        let mut s = Session::new();
        let ground = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Level(_))).map_or(0, |(id, _)| *id);
        let upper = run(&mut s, "level", json!({"name": "Upper", "elevation": 3000}))["id"].as_u64().unwrap_or_default();
        let wall = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "level": ground, "top_level": upper}))["ids"][0]
            .as_u64()
            .unwrap_or_default();
        let column =
            run(&mut s, "column", json!({"at": [4000, 3000], "level": ground, "top_level": upper}))["id"].as_u64().unwrap_or_default();
        let roof =
            run(&mut s, "roof", json!({"boundary": [[-500, -500], [8500, -500], [8500, 6500], [-500, 6500]], "level": upper, "slope": 20}))
                ["id"]
                .as_u64()
                .unwrap_or_default();
        (s, wall, column, roof)
    }

    fn apply(s: &mut Session, id: Id, h: Handle, to: P2, z: f64) {
        let (c, p) = command(&s.model, id, h, to, z).expect("a command");
        run(s, c, p);
    }

    #[test]
    fn a_wall_has_its_ends_and_two_arrows_and_loses_an_arrow_once_attached() {
        let (mut s, wall, _, roof) = house();
        let kinds = |s: &Session| of(&s.model, wall).iter().map(|h| h.0).collect::<Vec<_>>();
        assert_eq!(kinds(&s), [Handle::Start, Handle::End, Handle::Top, Handle::Base]);
        // Its ends at its base, on its location line, as Revit has them; and that line, drawn along its base.
        let ends: Vec<P3> = of(&s.model, wall).into_iter().filter(|h| !h.0.vertical()).map(|h| h.1).collect();
        assert_eq!(ends, [[0.0, 0.0, 0.0], [8000.0, 0.0, 0.0]]);
        assert_eq!(location_line(&s.model, wall), [[0.0, 0.0, 0.0], [8000.0, 0.0, 0.0]]);
        run(&mut s, "modify", json!({"id": wall, "attach_top": roof}));
        assert_eq!(kinds(&s), [Handle::Start, Handle::End, Handle::Base]);
    }

    #[test]
    fn dragging_a_wall_s_arrows_changes_its_offsets_and_its_end_moves_it() {
        let (mut s, wall, ..) = house();
        let w = |s: &Session| s.model.wall(wall).cloned().unwrap_or_else(|| panic!("the wall"));
        let at = P2(0.0, 0.0);
        apply(&mut s, wall, Handle::Top, at, 3500.0);
        assert_eq!(w(&s).top_offset, 500.0);
        assert_eq!(s.model.top_z(&w(&s)), 3500.0);
        apply(&mut s, wall, Handle::Base, at, -300.0);
        assert_eq!(w(&s).base_offset, -300.0);
        assert_eq!(s.model.top_z(&w(&s)), 3500.0, "the top stays");
        apply(&mut s, wall, Handle::End, P2(9000.0, 0.0), 0.0);
        assert_eq!(w(&s).end, P2(9000.0, 0.0));
        assert_eq!(w(&s).start, P2(0.0, 0.0));
        assert!(command(&s.model, wall, Handle::Top, at, 3500.0).is_none(), "where it is already");
    }

    #[test]
    fn an_unconnected_wall_s_base_keeps_its_top_by_its_height() {
        let mut s = Session::new();
        let wall = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "height": 2500}))["ids"][0].as_u64().unwrap_or_default();
        apply(&mut s, wall, Handle::Base, P2(0.0, 0.0), 500.0);
        let w = s.model.wall(wall).cloned().unwrap_or_else(|| panic!("the wall"));
        assert_eq!((w.base_offset, w.height, s.model.top_z(&w)), (500.0, 2000.0, 2500.0));
    }

    #[test]
    fn a_column_s_arrows_change_its_heights_and_its_place_moves_it() {
        let (mut s, _, column, _) = house();
        let span = |s: &Session| s.model.elements.get(column).map_or((0.0, 0.0), |e| s.model.column_span(e));
        assert_eq!(span(&s), (0.0, 3000.0));
        apply(&mut s, column, Handle::Top, P2(0.0, 0.0), 3400.0);
        assert_eq!(span(&s), (0.0, 3400.0), "a top offset over its top level");
        apply(&mut s, column, Handle::Base, P2(0.0, 0.0), 200.0);
        assert_eq!(span(&s), (200.0, 3400.0));
        apply(&mut s, column, Handle::Place, P2(1000.0, 1000.0), 0.0);
        let at = s.model.elements.get(column).and_then(Element::column).map(|c| c.at);
        assert_eq!(at, Some(P2(1000.0, 1000.0)));
    }

    #[test]
    fn a_column_attached_under_a_sloping_roof_follows_it_and_comes_loose_when_it_goes() {
        let (mut s, _, column, roof) = house();
        run(&mut s, "modify", json!({"id": column, "attach_top": roof}));
        let e = s.model.elements.get(column).cloned().unwrap_or_else(|| panic!("the column"));
        let (_, top) = s.model.column_span(&e);
        assert!(top > 3000.0, "under the roof's slope, above its eaves: {top}");
        let prisms = walls::column_prisms(&s.model, &e);
        assert!(!prisms.is_empty());
        let under = |q: P2| walls::underside(&s.model, s.model.elements.id(s.model.guid(roof)).unwrap_or_default(), q);
        for p in &prisms {
            for q in &p.ring {
                let expected = under(*q).unwrap_or_else(|| panic!("the roof is over {q:?}"));
                assert!((p.top.at(*q) - expected).abs() < 1e-6, "{q:?}: {} under {expected}", p.top.at(*q));
            }
        }
        // Its top moves with what it follows, and is its own again when that goes.
        run(&mut s, "delete", json!({"ids": [roof]}));
        let e = s.model.elements.get(column).cloned().unwrap_or_else(|| panic!("the column"));
        assert_eq!(e.column().and_then(|c| c.attach_top), None);
        assert_eq!(s.model.column_span(&e), (0.0, 3000.0));
    }

    #[test]
    fn a_column_attaches_only_to_a_roof_or_a_floor_and_a_footing_never() {
        let (mut s, wall, column, roof) = house();
        assert!(s.execute("modify", &json!({"id": column, "attach_top": wall})).is_err());
        let footing = run(&mut s, "footing", json!({"at": [100, 100]}))["id"].as_u64().unwrap_or_default();
        assert!(s.execute("modify", &json!({"id": footing, "attach_top": roof})).is_err());
        assert!(s.execute("modify", &json!({"id": column, "attach_top": roof})).is_ok());
    }

    #[test]
    fn a_dragged_height_lands_on_a_level_within_reach() {
        let (s, ..) = house();
        assert_eq!(settled(&s.model, 2990.0, 20.0), 3000.0);
        assert_eq!(settled(&s.model, 2900.0, 20.0), 2900.0);
    }
}
