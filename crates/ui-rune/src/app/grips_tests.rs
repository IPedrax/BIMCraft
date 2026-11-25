//! The grips through the app, with no window: the press that takes an end, the drag, the move, and Escape.

use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;

/// A wall far from the house, made through the app and selected alone, and its ends.
fn lone_wall() -> (App, u64, (P2, P2)) {
    let mut app = App::sample();
    let level = app.level;
    assert!(app.run("wall", &serde_json::json!({"points": [[-30000, -30000], [-26000, -30000]], "level": level})), "{:?}", app.message);
    let id = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).max().expect("a wall");
    app.select(vec![id]);
    let ends = crate::grips::ends(&app.session.model, id).expect("ends");
    (app, id, ends)
}

fn px(app: &App, p: P2) -> (f32, f32) {
    app.camera.view(app.size.0, app.size.1).px(p)
}

#[test]
fn the_element_selected_alone_shows_its_grips_and_a_tool_in_hand_or_a_second_element_takes_them_away() {
    let (mut app, id, ends) = lone_wall();
    let shown = app.grips_shown().expect("a wall's grips");
    assert_eq!((shown.id, shown.ends, shown.held), (id, [ends.0, ends.1], None));
    app.set_tool("wall");
    assert!(app.grips_shown().is_none(), "a tool in hand takes the clicks");
    app.set_tool("wall");
    let other = app
        .session
        .model
        .elements
        .iter()
        .find_map(|(i, e)| matches!(e, Element::Wall(_)).then_some(*i).filter(|i| *i != id))
        .expect("another wall");
    app.select(vec![id, other]);
    assert!(app.grips_shown().is_none(), "two selected have no grips");
}

#[test]
fn a_press_on_an_end_takes_the_grip_and_the_drag_follows_the_pointer_and_the_release_moves_the_end() {
    let (mut app, id, ends) = lone_wall();
    let (x, y) = px(&app, ends.1);
    assert!(!app.grip_down(x + 60.0, y + 60.0), "away from both ends there is no grip");
    assert!(!app.gripping());
    assert!(app.grip_down(x + 3.0, y - 2.0), "within reach of the end");
    assert!(app.gripping());
    let (tx, ty) = px(&app, P2(ends.1.0, ends.1.1 + 3000.0));
    app.grip_move(tx, ty, false);
    let shown = app.grips_shown().expect("shown");
    assert!(shown.held.is_some() && shown.ends[1].dist(ends.1) > 1000.0, "the dragged end follows: {:?}", shown.ends);
    assert!(app.grip_reading().is_some_and(|(_, text)| text.contains('\u{b0}')), "the ruler's reading");
    app.grip_up(false);
    assert!(!app.gripping());
    let after = crate::grips::ends(&app.session.model, id).expect("ends");
    assert!(
        after.1.dist(P2(ends.1.0, ends.1.1 + 3000.0)) < 150.0 && after.0.dist(ends.0) < 1.0,
        "the end moved, the other stayed: {after:?}"
    );
    assert!(app.undo());
    assert_eq!(crate::grips::ends(&app.session.model, id), Some(ends), "one undo step");
}

#[test]
fn a_cancelled_drag_and_a_press_that_does_not_move_change_nothing() {
    let (mut app, id, ends) = lone_wall();
    let (x, y) = px(&app, ends.0);
    assert!(app.grip_down(x, y));
    let (tx, ty) = px(&app, P2(ends.0.0 - 2000.0, ends.0.1));
    app.grip_move(tx, ty, false);
    app.grip_up(true);
    assert_eq!(crate::grips::ends(&app.session.model, id), Some(ends), "Escape put it back");
    assert!(app.grip_down(x, y));
    app.grip_up(false);
    assert_eq!(crate::grips::ends(&app.session.model, id), Some(ends), "a press with no travel moves nothing");
    assert!(!app.gripping());
    assert!(app.message.is_none(), "{:?}", app.message);
}

#[test]
fn a_press_that_takes_a_grip_leaves_the_selection_alone() {
    let (mut app, id, ends) = lone_wall();
    let (x, y) = px(&app, ends.1);
    // The plan's handler asks `grip_down` first, so a press on an end is the drag of it and never the click that would select.
    assert!(app.grip_down(x, y));
    app.grip_up(false);
    assert_eq!(app.selection, vec![id], "the wall is still the selection");
}
