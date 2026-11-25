//! The temporary dimensions through the app, with no window: made for the selection, typed into and committed.

use bimcraft_core::model::Element;

use crate::app::App;
use crate::tempdim::Edit;

fn wall_selected() -> (App, u64) {
    let mut app = App::sample();
    let id =
        app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(w) if w.arc == 0.0).then_some(*id)).expect("a wall");
    app.select(vec![id]);
    (app, id)
}

#[test]
fn the_dimensions_follow_the_selection_and_go_with_a_tool_in_hand_or_a_grip_being_dragged() {
    let (mut app, id) = wall_selected();
    assert!(!app.temp.is_empty() && app.temp[0].dim.edit == Edit::WallLength(id));
    assert_eq!(app.dims_shown().len(), app.temp.len(), "shown with nothing in hand");
    assert!(!app.temp[0].edit.text.is_empty(), "the number to edit is the length");
    app.set_tool("beam");
    assert!(app.dims_shown().is_empty(), "a tool in hand takes the clicks");
    app.set_tool("beam");
    app.select(Vec::new());
    assert!(app.temp.is_empty(), "nothing selected, no dimensions");
    app.select(vec![id, id + 1]);
    assert!(app.temp.is_empty(), "two selected, none");
}

/// A wall of its own, far from the house and selected: the first wall of the house is held by a dimension, which refuses a new length.
fn lone_wall_selected() -> (App, u64) {
    let mut app = App::sample();
    let level = app.level;
    assert!(app.run("wall", &serde_json::json!({"points": [[-30000, -30000], [-26000, -30000]], "level": level})), "{:?}", app.message);
    let id = app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).max().expect("a wall");
    app.select(vec![id]);
    (app, id)
}

#[test]
fn a_length_typed_into_the_wall_dimension_changes_the_wall_and_a_bad_one_says_so() {
    let (mut app, id) = lone_wall_selected();
    let w = app.session.model.wall(id).expect("a wall");
    let before = w.start.dist(w.end);
    app.temp[0].edit.text = "abc".into();
    assert!(!app.commit_dim(0));
    assert!(app.message.as_deref().is_some_and(|m| m.contains("not a length")), "{:?}", app.message);
    let wanted = before - 500.0;
    app.temp[0].edit.text = app.session.model.field(wanted);
    assert!(app.commit_dim(0), "{:?}", app.message);
    let after = app.session.model.wall(id).map(|w| w.start.dist(w.end)).expect("a wall");
    assert!((after - wanted).abs() < 2.0, "the wall is now {after} long, not {wanted}");
    assert!(!app.temp.is_empty() && app.temp[0].dim.length - wanted < 2.0, "the dimensions were made again for what is now");
    assert!(!app.commit_dim(99), "a dimension that is not there is no commit");
    assert!(app.undo());
    let back = app.session.model.wall(id).map(|w| w.start.dist(w.end)).expect("a wall");
    assert!((back - before).abs() < 1.0, "one undo step");
}
