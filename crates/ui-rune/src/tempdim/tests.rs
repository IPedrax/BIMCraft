//! The dimensions of what the sample house has, and what typing a length into one does.

use bimcraft_core::model::Element;
use serde_json::json;

use super::*;
use crate::app::App;

fn house() -> App {
    App::sample()
}

fn first(app: &App, pick: fn(&Element) -> bool) -> Id {
    app.session.model.elements.iter().find_map(|(id, e)| pick(e).then_some(*id)).expect("an element of that kind")
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_straight_wall_shows_its_length_first_and_its_clearances_to_what_runs_beside_it() {
    let app = house();
    let m = &app.session.model;
    let id = first(&app, |e| matches!(e, Element::Wall(w) if w.arc == 0.0));
    let d = dims(m, id);
    let w = m.wall(id).expect("a wall");
    assert!((1..=3).contains(&d.len()), "the length and up to two clearances: {}", d.len());
    assert_eq!(d[0].edit, Edit::WallLength(id));
    assert!(near(d[0].length, w.start.dist(w.end)) && d[0].a == w.start && d[0].b == w.end);
    for c in &d[1..] {
        assert!(matches!(c.edit, Edit::Clearance { id: i, .. } if i == id) && c.length > 0.0, "{c:?}");
    }
}

#[test]
fn a_door_shows_the_two_stretches_of_its_wall_and_a_column_or_component_its_four_clearances() {
    let app = house();
    let m = &app.session.model;
    let door = first(&app, |e| matches!(e, Element::Door(_)));
    let d = dims(m, door);
    assert!(!d.is_empty() && d.len() <= 2, "{d:?}");
    assert!(d.iter().all(|x| matches!(x.edit, Edit::Opening { id, .. } if id == door)));
    let sofa = first(&app, |e| matches!(e, Element::Component(_)));
    let c = dims(m, sofa);
    assert!(c.len() <= 4 && c.iter().all(|x| matches!(x.edit, Edit::Clearance { .. }) && x.length > 0.0), "{c:?}");
    let level = app.level.expect("a level");
    assert!(dims(m, level).is_empty(), "a level has none");
}

#[test]
fn a_length_typed_into_a_wall_dimension_is_its_new_length_and_the_same_one_changes_nothing() {
    let app = house();
    let m = &app.session.model;
    let id = first(&app, |e| matches!(e, Element::Wall(w) if w.arc == 0.0));
    let dim = dims(m, id)[0];
    let (name, params) = apply(m, &dim, &m.field(dim.length / 2.0)).expect("a length").expect("a command");
    assert_eq!((name, &params["id"]), ("wall_length", &json!(id)));
    assert!(params["value"].as_f64().is_some_and(|v| (v - dim.length / 2.0).abs() < 1.0), "{params}");
    assert_eq!(apply(m, &dim, &m.field(dim.length)).expect("a length"), None, "the length it has is no change");
    assert!(apply(m, &dim, "abc").is_err() && apply(m, &dim, "0").is_err() && apply(m, &dim, "-5").is_err());
    assert!(apply(m, &dim, "abc").err().is_some_and(|e| e.contains("not a length")));
}

#[test]
fn a_clearance_typed_moves_the_element_toward_the_line_by_the_difference_and_an_opening_stays_on_its_wall() {
    let clearance = Dim {
        a: P2(0.0, 0.0),
        b: P2(2000.0, 0.0),
        side: P2(0.0, 1.0),
        off: Off { mm: 0.0, px: 30.0 },
        length: 2000.0,
        edit: Edit::Clearance { id: 7, toward: P2(1.0, 0.0), distance: 2000.0 },
    };
    let app = house();
    let m = &app.session.model;
    let (name, params) = apply(m, &clearance, &m.field(500.0)).expect("a length").expect("a command");
    assert_eq!(name, "move");
    assert!(params["by"][0].as_f64().is_some_and(|x| (x - 1500.0).abs() < 1.0) && params["by"][1].as_f64() == Some(0.0), "{params}");
    assert!(apply(m, &clearance, "-1").is_err(), "a clearance is not negative");
    let opening = Dim { edit: Edit::Opening { id: 9, start: 0.0, end: 3000.0, from_start: true, offset: 1000.0 }, ..clearance };
    let (name, params) = apply(m, &opening, &m.field(2000.0)).expect("a length").expect("a command");
    assert_eq!((name, &params["id"]), ("modify", &json!(9)));
    assert!(params["offset"].as_f64().is_some_and(|x| (x - 2000.0).abs() < 1.0), "{params}");
    let from_end = Dim { edit: Edit::Opening { id: 9, start: 0.0, end: 3000.0, from_start: false, offset: 1000.0 }, ..clearance };
    assert!(
        apply(m, &from_end, &m.field(500.0))
            .expect("a length")
            .is_some_and(|(_, p)| p["offset"].as_f64().is_some_and(|x| (x - 2500.0).abs() < 1.0))
    );
    let err = apply(m, &opening, &m.field(4000.0)).expect_err("past its wall");
    assert!(err.contains("wall"), "{err}");
}
