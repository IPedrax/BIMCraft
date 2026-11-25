//! Which element has grips, which end a press takes, where a drag lands and the `move` it ends in.

use bimcraft_core::model::Element;

use super::*;
use crate::app::App;

fn house() -> (App, Id) {
    let app = App::sample();
    let wall = app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    (app, wall)
}

#[test]
fn an_element_selected_alone_with_no_tool_in_hand_has_grips_at_its_ends() {
    let (app, wall) = house();
    let m = &app.session.model;
    assert_eq!(gripped(m, &[wall], false), Some(wall));
    assert_eq!(gripped(m, &[wall], true), None, "a tool in hand takes the clicks");
    assert_eq!(gripped(m, &[wall, wall + 1], false), None, "two are no one's grips");
    assert_eq!(gripped(m, &[], false), None);
    let level = app.level.expect("a level");
    assert_eq!(gripped(m, &[level], false), None, "a level has no ends");
    let (a, b) = ends(m, wall).expect("a wall's ends");
    assert!(a.dist(b) > 0.0);
}

#[test]
fn a_press_takes_the_nearer_end_within_reach_and_nothing_further() {
    let (a, b) = (P2(0.0, 0.0), P2(4000.0, 0.0));
    assert_eq!(end_at((a, b), P2(30.0, 40.0), 100.0), Some(true));
    assert_eq!(end_at((a, b), P2(3950.0, 20.0), 100.0), Some(false));
    assert_eq!(end_at((a, b), P2(2000.0, 0.0), 100.0), None, "the middle is no grip");
    assert_eq!(end_at((a, b), P2(f64::NAN, 0.0), 100.0), None, "a point that is no number takes nothing");
}

#[test]
fn a_drag_lands_where_the_pointer_did_and_with_ctrl_keeps_the_length_about_the_fixed_end() {
    let ends = (P2(0.0, 0.0), P2(4000.0, 0.0));
    assert_eq!(target(ends, false, P2(5000.0, 1000.0), false), P2(5000.0, 1000.0));
    let turned = target(ends, false, P2(0.0, 9000.0), true);
    assert!(turned.dist(P2(0.0, 4000.0)) < 1e-3, "the end turned about the start, its length 4000: {turned:?}");
    let start = target(ends, true, P2(4000.0, -7000.0), true);
    assert!(start.dist(P2(4000.0, -4000.0)) < 1e-3, "{start:?}");
    assert_eq!(target(ends, false, P2(0.0, 0.0), true), P2(0.0, 0.0), "on the fixed end there is no direction to keep");
}

#[test]
fn a_drag_ends_in_a_move_of_that_end_and_a_drag_that_goes_nowhere_in_none() {
    // A wall of its own: the first of the house is held by a dimension, and the core refuses to move its end.
    let (mut app, _) = house();
    let level = app.level;
    app.session.execute("wall", &json!({"points": [[-30000, -30000], [-26000, -30000]], "level": level})).expect("a wall");
    let wall = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).max().expect("a wall");
    let (a, b) = ends(&app.session.model, wall).expect("ends");
    let to = b + P2(250.0, 125.0);
    let (name, params) = command(&app.session.model, Grip { id: wall, start: false, to }).expect("a move");
    assert_eq!(name, "move");
    assert_eq!(params["end"], json!("end"));
    assert_eq!(params["by"], json!([250.0, 125.0]));
    assert_eq!(command(&app.session.model, Grip { id: wall, start: true, to: a }), None);
    assert!(app.session.execute(name, &params).is_ok(), "the core takes it");
    let (_, after) = ends(&app.session.model, wall).expect("ends");
    assert!(after.dist(to) < 1.0, "the end went where it was dragged: {after:?} for {to:?}");
}
