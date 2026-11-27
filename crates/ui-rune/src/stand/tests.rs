//! What stands where the next click would put it: a wall's outline by its type and location line, a door in the wall under the pointer,
//! a column of the type picked.

use bimcraft_core::model::Location;

use super::*;
use crate::app::App;
use crate::options::TypePick;

/// The corners of the outline of a wall from the origin to 1000 mm east.
fn ring(m: &Model, s: &Settings) -> Vec<P2> {
    match wall(m, s, P2(0.0, 0.0), P2(1000.0, 0.0)) {
        Some(Curve::Poly { points, closed: true, .. }) => points,
        other => panic!("not a closed outline: {other:?}"),
    }
}

#[test]
fn a_wall_is_outlined_by_the_thickness_of_its_type_on_the_side_the_location_line_puts_it() {
    let app = App::sample();
    let m = &*app.session.model;
    let name = m.default_type("wall").expect("a default wall type").to_string();
    let t = m.types.wall.get(&name).expect("the type").thickness;
    let mut s = Settings::default();
    let r = ring(m, &s);
    assert_eq!(r.len(), 4);
    assert!((r[0].1 - t / 2.0).abs() < 1e-6 && (r[2].1 + t / 2.0).abs() < 1e-6, "half the thickness each side of the line: {r:?}");
    assert!(r[0].0.abs() < 1e-6 && (r[1].0 - 1000.0).abs() < 1e-6, "from one end to the other: {r:?}");
    s.location = Location::ExteriorFinish;
    let r = ring(m, &s);
    assert!(r[0].1.abs() < 1e-6 && (r[2].1 + t).abs() < 1e-6, "the exterior face on the line, the wall to its right: {r:?}");
    s.location = Location::InteriorFinish;
    let r = ring(m, &s);
    assert!((r[0].1 - t).abs() < 1e-6 && r[2].1.abs() < 1e-6, "the interior face on the line: {r:?}");
    // Another type's thickness.
    let other = m.type_names("wall").into_iter().find(|n| m.types.wall.get(*n).is_some_and(|ty| (ty.thickness - t).abs() > 1.0)).expect("another type");
    let thick = m.types.wall.get(other).expect("the type").thickness;
    s.location = Location::Centre;
    s.picks.insert("wall", TypePick { name: other.to_string(), family: None });
    let r = ring(m, &s);
    assert!((r[0].1 - r[2].1 - thick).abs() < 1e-6, "the type picked sets the thickness: {r:?}");
    assert!(wall(m, &s, P2(0.0, 0.0), P2(0.0, 0.0)).is_none() && wall(m, &s, P2(f64::NAN, 0.0), P2(1.0, 0.0)).is_none());
}

#[test]
fn a_door_stands_in_the_wall_under_the_pointer_and_nowhere_else_and_a_column_takes_the_type_picked() {
    let app = App::sample();
    let (m, level) = (&*app.session.model, app.level.expect("a level"));
    let mut s = Settings::default();
    let near = 200.0;
    let door = element(m, &s, "door", level, P2(1000.0, 0.0), near);
    assert!(matches!(door, Some(Element::Door(_))), "the house's south wall: {door:?}");
    let window = element(m, &s, "window", level, P2(1000.0, 0.0), near);
    assert!(matches!(window, Some(Element::Window(_))), "{window:?}");
    let far = P2(-30000.0, -30000.0);
    assert!(element(m, &s, "door", level, far, near).is_none() && element(m, &s, "window", level, far, near).is_none(), "over no wall");
    let column = element(m, &s, "column", level, far, near);
    assert!(matches!(column, Some(Element::Column(_))), "a column stands anywhere: {column:?}");
    let other = m.type_names("column").into_iter().last().expect("a column type").to_string();
    s.picks.insert("column", TypePick { name: other.clone(), family: None });
    assert!(matches!(element(m, &s, "column", level, far, near), Some(Element::Column(c)) if c.type_name == other), "the type picked");
    assert!(element(m, &s, "beam", level, far, near).is_none(), "another tool stands as a line, not here");
    assert!(element(m, &s, "column", level, P2(f64::NAN, 0.0), near).is_none());
}
