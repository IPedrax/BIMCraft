//! Elevations that look at a reference: an elevation placed on a wall, a line or a slab's edge keeps that edge as a line of
//! its own ([`Section::reference`]) and looks square at it, from whichever side of it its mark stands on. Moved across the
//! line, it turns round. Nothing ties it to what had the edge: the wall may move or go, the line stays where it was taken.

use crate::engine::Session;
use crate::geom::P2;
use crate::ids::{Changed, Id};
use crate::model::{Element, Face, Line, Model, Reference, Section, TOL};

/// How far from its reference a mark stands when it is placed by a point on the reference itself, as a click on a wall is.
pub const OUT: f64 = 1500.0;
/// A point nearer the reference than this is a pick of the reference, not a place for the mark.
const ON: f64 = 1000.0;
/// How far past the reference's ends a new elevation's line reaches.
const PAST: f64 = 1000.0;

/// The straight edge of element `id` an elevation placed at `at` looks at: a wall's centre line, else its edge or axis
/// nearest `at` (a line, a floor's, a roof's or a ceiling's edge, a grid, a beam, a line of a CAD drawing). None for what
/// has no straight edge to look at (an arc), and for what isn't looked at: views' marks, notes, openings, links as a whole.
/// `m` is the model the views draw ([`crate::link::seen`]) for an element of a link.
pub fn reference(m: &Model, id: Id, at: P2) -> Option<Line> {
    match m.elements.get(id)? {
        Element::Wall(w) if w.arc == 0.0 => m.reference_line(&Reference::line(m.guid(id), Face::Centre)).ok(),
        Element::Wall(_)
        | Element::Door(_)
        | Element::Window(_)
        | Element::Section(_)
        | Element::Elevation(_)
        | Element::Text(_)
        | Element::PointCloud(_)
        | Element::Link(_) => None,
        _ => crate::align::nearest(m, id, at).map(|(l, _)| l),
    }
}

/// Where a new elevation's mark stands for a pick at `at`: at `at`, or when that is on the reference (within a metre of
/// its line), [`OUT`] from the line on `at`'s side, to the left of it when dead on.
pub fn place((a, b): Line, at: P2) -> P2 {
    let n = (b - a).unit().perp();
    let d = (at - a).dot(n);
    if d.abs() >= ON { at } else { at + n * (if d < 0.0 { -OUT } else { OUT } - d) }
}

/// The line of an elevation whose mark stands at `mid`, `half` mm long each way, looking square at `reference` from its
/// side of it. None on the reference's line itself, where there is no side to look from.
pub fn line((a, b): Line, mid: P2, half: f64) -> Option<Line> {
    let n = (b - a).unit().perp();
    let d = (mid - a).dot(n);
    if d.abs() <= TOL || n == P2(0.0, 0.0) {
        return None;
    }
    // It looks to its left: along the reference when that is to the left, against it otherwise.
    let look = if d > 0.0 { n * -1.0 } else { n };
    let along = P2(look.1, -look.0);
    Some((mid - along * half, mid + along * half))
}

/// A new elevation's line, from a mark at `mid`: long enough to see the whole reference and a metre past each end.
pub fn new_line(reference: Line, mid: P2) -> Option<Line> {
    let u = (reference.1 - reference.0).unit();
    let reach = (reference.0 - mid).dot(u).abs().max((reference.1 - mid).dot(u).abs());
    line(reference, mid, reach + PAST)
}

/// `v` turned about its mark to look at its reference, when it has one and doesn't already: whether it changed.
pub fn settle(v: &mut Section) -> bool {
    let Some([a, b]) = v.reference else { return false };
    let Some((start, end)) = line((a, b), (v.start + v.end) * 0.5, v.start.dist(v.end) / 2.0) else { return false };
    if start.dist(v.start) <= TOL && end.dist(v.end) <= TOL {
        return false;
    }
    (v.start, v.end) = (crate::commands::micron(start), crate::commands::micron(end));
    true
}

/// After a command: every elevation it made or changed looks at its reference, however it was moved, turned or edited.
pub(crate) fn keep(s: &mut Session, before: &Model, verified: bool) {
    let turned = |m: &Model, id: Id| match m.elements.get(id) {
        Some(Element::Elevation(v)) if v.reference.is_some() => {
            let mut v = v.clone();
            settle(&mut v).then_some((id, v))
        }
        _ => None,
    };
    let m = &s.model;
    let fixed: Vec<(Id, Section)> = match (verified, m.changed_since(before)) {
        (true, Changed::Ids(ids)) => ids.into_iter().filter_map(|id| turned(m, id)).collect(),
        _ => m.elements.keys().filter_map(|id| turned(m, *id)).collect(),
    };
    for (id, v) in fixed {
        if let Some(Element::Elevation(old)) = s.model_mut().elements.get_mut(id) {
            *old = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::engine::Session;
    use crate::geom::P2;
    use crate::ids::Id;
    use crate::model::Element;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    fn id(v: &Value) -> Id {
        v["id"].as_u64().unwrap()
    }

    fn a_wall(s: &mut Session, p: Value) -> Id {
        run(s, "wall", p)["ids"][0].as_u64().unwrap()
    }

    /// Where the elevation's mark stands (the middle of its line) and the way it looks, to the millimetre.
    fn mark(s: &Session, e: Id) -> ((i64, i64), (i64, i64)) {
        let v = s.model.section(e).unwrap();
        let (mid, l) = ((v.start + v.end) * 0.5, v.looking());
        ((mid.0.round() as i64, mid.1.round() as i64), (l.0.round() as i64, l.1.round() as i64))
    }

    fn reference(s: &Session, e: Id) -> Value {
        serde_json::to_value(s.model.elements.get(e).unwrap()).unwrap()["reference"].clone()
    }

    #[test]
    fn an_elevation_on_a_wall_looks_square_at_it_from_the_side_clicked() {
        let mut s = Session::new();
        // A wall running north-east, at 45°.
        let wall = a_wall(&mut s, json!({"points": [[0, 0], [6000, 6000]]}));
        // Clicked on the wall, a little to its south-east: the mark stands out from it on that side, looking back.
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [3050, 2950]})));
        let v = s.model.section(e).unwrap().clone();
        let (mid, look) = ((v.start + v.end) * 0.5, v.looking());
        let north_west = P2(-1.0, 1.0).unit();
        assert!((look - north_west).length() < 1e-6, "{look:?}");
        assert!(((mid - P2(3000.0, 3000.0)).dot(north_west) + 1500.0).abs() < 1e-3, "1.5 m out from the centre line: {mid:?}");
        assert!((mid - P2(3000.0, 3000.0)).dot(P2(1.0, 1.0).unit()).abs() < 1e-3, "opposite where it was clicked: {mid:?}");
        // Its line reaches a metre past the wall's ends.
        assert!((v.start.dist(v.end) - (P2(6000.0, 6000.0).length() + 2000.0)).abs() < 1.0, "{v:?}");
        // The wall's centre line is kept as a line, not the wall.
        assert_eq!(reference(&s, e), json!([[0.0, 0.0], [6000.0, 6000.0]]));
        assert!(matches!(s.model.elements.get(e), Some(Element::Elevation(_))));
    }

    #[test]
    fn an_elevation_stands_where_it_is_asked_when_that_is_off_the_reference() {
        let mut s = Session::new();
        let wall = a_wall(&mut s, json!({"points": [[0, 0], [8000, 0]]}));
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [2000, -4000]})));
        assert_eq!(mark(&s, e), ((2000, -4000), (0, 1)));
        // From its mark its line covers the whole wall and a metre more: 6 m to the far end, 7 m each way.
        assert_eq!(s.model.section(e).unwrap().start.dist(s.model.section(e).unwrap().end).round(), 14000.0);
    }

    #[test]
    fn an_elevation_moved_across_its_reference_turns_round_to_look_at_it() {
        let mut s = Session::new();
        let wall = a_wall(&mut s, json!({"points": [[0, 0], [8000, 0]]}));
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [4000, -2000]})));
        assert_eq!(mark(&s, e), ((4000, -2000), (0, 1)));
        let length = s.model.section(e).unwrap().start.dist(s.model.section(e).unwrap().end);
        // Along the wall it looks the same way.
        run(&mut s, "move", json!({"ids": [e], "by": [1000, 500]}));
        assert_eq!(mark(&s, e), ((5000, -1500), (0, 1)));
        // To the wall's other side: it flips, and is as long as it was.
        run(&mut s, "move", json!({"ids": [e], "by": [0, 4000]}));
        assert_eq!(mark(&s, e), ((5000, 2500), (0, -1)));
        assert!((s.model.section(e).unwrap().start.dist(s.model.section(e).unwrap().end) - length).abs() < 1e-3);
        // Past the wall's end it still looks at the wall's line.
        run(&mut s, "move", json!({"ids": [e], "by": [9000, 0]}));
        assert_eq!(mark(&s, e), ((14000, 2500), (0, -1)));
        // Turned, it turns back.
        run(&mut s, "rotate", json!({"ids": [e], "about": [14000, 2500], "angle": 30}));
        assert_eq!(mark(&s, e), ((14000, 2500), (0, -1)));
        s.execute("undo", &json!({})).unwrap();
        s.execute("undo", &json!({})).unwrap();
        assert_eq!(mark(&s, e), ((5000, 2500), (0, -1)));
    }

    #[test]
    fn the_reference_is_a_line_taken_once_not_a_tie_to_the_wall() {
        let mut s = Session::new();
        let wall = a_wall(&mut s, json!({"points": [[0, 0], [8000, 0]]}));
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [4000, -2000]})));
        // The wall goes away, and the elevation stays as it is.
        run(&mut s, "move", json!({"ids": [wall], "by": [0, -5000]}));
        assert_eq!(mark(&s, e), ((4000, -2000), (0, 1)));
        run(&mut s, "delete", json!({"ids": [wall]}));
        assert_eq!(mark(&s, e), ((4000, -2000), (0, 1)));
        assert_eq!(reference(&s, e), json!([[0.0, 0.0], [8000.0, 0.0]]));
        // Moved with what it looks at, its reference goes too: it doesn't flip for being carried.
        let mut s = Session::new();
        let wall = a_wall(&mut s, json!({"points": [[0, 0], [8000, 0]]}));
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [4000, -2000]})));
        run(&mut s, "move", json!({"ids": [wall, e], "by": [0, 5000]}));
        assert_eq!(mark(&s, e), ((4000, 3000), (0, 1)));
        assert_eq!(reference(&s, e), json!([[0.0, 5000.0], [8000.0, 5000.0]]));
        // It is written and read back.
        let again = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
        assert_eq!(again.section(e), s.model.section(e));
    }

    #[test]
    fn a_line_a_slab_edge_and_a_given_line_are_references_too() {
        let mut s = Session::new();
        let floor = id(&run(&mut s, "floor", json!({"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]})));
        // The east edge, clicked just outside it.
        let e = id(&run(&mut s, "elevation", json!({"on": floor, "at": [6040, 1000]})));
        assert_eq!(mark(&s, e), ((7500, 1000), (-1, 0)));
        assert_eq!(reference(&s, e), json!([[6000.0, 0.0], [6000.0, 4000.0]]));
        let line = id(&run(&mut s, "detail_line", json!({"points": [[0, 9000], [5000, 9000]]})));
        let e = id(&run(&mut s, "elevation", json!({"on": line, "at": [2500, 8990]})));
        assert_eq!(mark(&s, e), ((2500, 7500), (0, 1)));
        // A line given outright, with a name.
        let made = run(&mut s, "elevation", json!({"reference": [[0, 0], [0, 4000]], "at": [-3000, 2000], "name": "West wing"}));
        assert_eq!(made["name"], json!("West wing"));
        assert_eq!(mark(&s, id(&made)), ((-3000, 2000), (1, 0)));
    }

    #[test]
    fn a_wall_of_a_link_is_looked_at_as_the_projects_own() {
        let mut other = Session::new();
        run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0]]}));
        let path = std::env::temp_dir().join(format!("bimcraft-elevation-link-{}.bimcraft", std::process::id()));
        let path = path.to_string_lossy().into_owned();
        run(&mut other, "save", json!({"path": path}));
        let mut s = Session::new();
        run(&mut s, "link", json!({"path": path, "at": [1000, 2000]}));
        let _ = std::fs::remove_file(&path);
        let seen = crate::link::seen(&s.model);
        let wall = seen.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).unwrap();
        assert!(crate::link::is_linked(wall));
        let e = id(&run(&mut s, "elevation", json!({"on": wall, "at": [5000, 2050]})));
        assert_eq!(mark(&s, e), ((5000, 3500), (0, -1)));
        assert_eq!(reference(&s, e), json!([[1000.0, 2000.0], [9000.0, 2000.0]]));
        assert_eq!(s.model.check(), Ok(()));
    }

    #[test]
    fn what_cannot_be_looked_at_says_so() {
        let mut s = Session::new();
        let arc = a_wall(&mut s, json!({"points": [[0, 0], [8000, 0]], "arc": 90}));
        let err = s.execute("elevation", &json!({"on": arc, "at": [4000, 100]})).unwrap_err();
        assert!(err.contains("straight"), "{err}");
        let err = s.execute("elevation", &json!({"on": 123456, "at": [0, 0]})).unwrap_err();
        assert!(err.contains("123456"), "{err}");
        let err = s.execute("elevation", &json!({"on": arc})).unwrap_err();
        assert!(err.contains("`at`"), "{err}");
        let err = s.execute("elevation", &json!({"reference": [[0, 0], [0, 0]], "at": [5, 5]})).unwrap_err();
        assert!(err.contains("reference"), "{err}");
        let err = s.execute("elevation", &json!({"reference": [[0, 0], [1e300, 0]], "at": [5, 5]})).unwrap_err();
        assert!(err.contains("reference"), "{err}");
    }
}
