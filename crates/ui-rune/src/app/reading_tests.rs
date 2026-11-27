//! The typed length and angle through the app, with no window: digits after the first point, Tab to the angle, Enter placing the point,
//! Backspace and Escape, a click spending what was typed, and Rotate's angle.

use bimcraft_core::draw::Shape;
use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;
use crate::select::How;

const A: P2 = P2(-30000.0, -30000.0);
const B: P2 = P2(-26000.0, -30000.0);

fn click(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.release((x, y), (x, y), How::Replace);
}

fn hover(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.hover(x, y);
}

fn walls(app: &App) -> usize {
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).count()
}

fn walls_ids(app: &App) -> Vec<u64> {
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect()
}

fn near(p: Option<&P2>, x: f64, y: f64) -> bool {
    p.is_some_and(|p| (p.0 - x).abs() < 2.0 && (p.1 - y).abs() < 2.0)
}

#[test]
fn a_length_typed_after_the_first_point_makes_the_wall_toward_the_pointer_and_tab_goes_on_to_the_angle() {
    let mut app = App::sample();
    assert!(!app.type_text("4"), "no tool, no typing");
    app.set_tool("wall");
    assert!(!app.type_text("4"), "and no point to measure from");
    click(&mut app, A);
    assert!(
        !app.type_text("x") && !app.typed_tab() && !app.typed_backspace(),
        "a letter, Tab and Backspace are nobody's with nothing typed"
    );
    assert!(app.type_text("4000"));
    assert!(app.status().contains("4000"), "the status line reads what is typed: {}", app.status());
    hover(&mut app, P2(A.0 + 100.0, A.1));
    let before = walls(&app);
    assert!(app.finish_tool(), "Enter places it");
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_eq!(walls(&app), before + 1);
    assert!(near(app.picked.last(), A.0 + 4000.0, A.1), "toward the pointer: {:?}", app.picked);
    assert!(app.typed.is_empty(), "what was typed is spent");

    assert!(app.type_text("3000") && app.typed_tab() && app.type_text("90"));
    assert_eq!((app.typed.length.as_str(), app.typed.angle.as_str()), ("3000", "90"));
    assert!(app.finish_tool());
    assert_eq!(walls(&app), before + 2);
    assert!(near(app.picked.last(), A.0 + 4000.0, A.1 + 3000.0), "straight north from the last point: {:?}", app.picked);
}

#[test]
fn backspace_and_escape_take_what_was_typed_before_the_points_and_a_click_spends_it() {
    let mut app = App::sample();
    app.set_tool("wall");
    click(&mut app, A);
    assert!(app.type_text("12"));
    assert!(app.back_point() && app.typed.length == "1", "Backspace takes the digit, not the point");
    assert_eq!(app.picked.len(), 1);
    assert!(app.escape() && app.typed.is_empty(), "Escape gives up what was typed first");
    assert_eq!((app.picked.len(), app.tool), (1, Some("wall")), "the point and the tool stay");
    assert!(app.type_text("12"));
    click(&mut app, B);
    assert!(app.typed.is_empty(), "a click spends it");
}

#[test]
fn a_length_that_is_no_length_is_the_message_and_the_point_stays_unpicked() {
    let mut app = App::sample();
    app.set_tool("wall");
    click(&mut app, A);
    app.typed.length = "9999999999999".into();
    let before = walls(&app);
    assert!(app.finish_tool());
    assert!(app.message.as_deref().is_some_and(|m| m.contains("9999999999999") && m.contains("length")), "{:?}", app.message);
    assert_eq!((walls(&app), app.picked.len()), (before, 1));
    assert!(!app.typed.is_empty(), "what was typed stays to be corrected");
}

#[test]
fn a_wall_drawn_as_a_rectangle_takes_no_typed_line_and_rotate_turns_by_the_angle_typed() {
    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Rectangle);
    click(&mut app, A);
    assert!(!app.type_text("5"), "a rectangle's numbers are not a line's length");
    app.set_shape(Shape::Line);
    // A wall of its own: the house's may be held by dimensions, which refuse to turn.
    let level = app.level.unwrap_or_default();
    assert!(app.run("wall", &serde_json::json!({"points": [[-20000, -30000], [-16000, -30000]], "level": level})));
    let wall = walls_ids(&app).into_iter().max().unwrap_or_default();
    let start = |app: &App| app.session.model.wall(wall).map(|w| w.start);
    app.select(vec![wall]);
    app.set_tool("rotate");
    let was = start(&app);
    click(&mut app, A);
    assert!(app.type_text("-90"), "an angle may be negative");
    assert_eq!(app.typed.angle, "-90", "Rotate's number is an angle");
    assert!(app.finish_tool());
    assert!(app.message.is_none(), "{:?}", app.message);
    assert_ne!(start(&app), was, "the wall turned about the centre");
    assert!(app.tool.is_none(), "Rotate is done with once it has turned");
    assert!(app.undo());
    assert_eq!(start(&app), was);
}

#[test]
fn a_rectangles_width_and_height_and_a_circles_radius_are_typed_and_enter_makes_the_walls() {
    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Rectangle);
    assert!(!app.type_text("3"), "no corner yet: nothing to measure from");
    click(&mut app, A);
    let before = walls(&app);
    hover(&mut app, P2(A.0 + 100.0, A.1 + 100.0));
    assert!(app.type_text("3000") && app.typed_tab() && app.type_text("2000"));
    assert!(app.status().contains("Width 3000") && app.status().contains("Height 2000"), "{}", app.status());
    assert!(app.finish_tool() && app.message.is_none(), "{:?}", app.message);
    assert!(walls(&app) > before && app.picked.is_empty() && app.typed.is_empty(), "{:?}", app.picked);
    let corner =
        app.session.model.elements.iter().any(|(_, e)| matches!(e, Element::Wall(w) if near(Some(&w.end), A.0 + 3000.0, A.1 + 2000.0)));
    assert!(corner, "the far corner is the width and the height from the first, on the pointer's side");

    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Circle);
    click(&mut app, A);
    let before = walls(&app);
    assert!(app.type_text("1500") && !app.typed_tab(), "a circle has one number: Tab is nobody's");
    assert!(app.finish_tool() && app.message.is_none(), "{:?}", app.message);
    let round = app
        .session
        .model
        .elements
        .iter()
        .any(|(_, e)| matches!(e, Element::Wall(w) if w.arc != 0.0 && (w.start.dist(A) - 1500.0).abs() < 2.0));
    assert!(walls(&app) > before && round, "half circles of the radius typed");
}

#[test]
fn an_arcs_angle_is_typed_in_degrees_and_what_cannot_be_drawn_is_said_and_keeps_the_points() {
    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::CentreArc);
    click(&mut app, A);
    click(&mut app, P2(A.0 + 1000.0, A.1));
    hover(&mut app, P2(A.0, A.1 + 1000.0));
    let before = walls(&app);
    assert!(app.type_text("90°") && app.status().contains("Angle"), "{}", app.status());
    assert!(app.finish_tool() && app.message.is_none(), "{:?}", app.message);
    let arc = app.session.model.elements.iter().find_map(|(_, e)| match e {
        Element::Wall(w) if w.arc != 0.0 => Some(w.arc),
        _ => None,
    });
    assert!(walls(&app) == before + 1 && arc.is_some_and(|a| (a.abs() - 90.0).abs() < 1.0), "a quarter turn: {arc:?}");

    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Arc);
    click(&mut app, A);
    click(&mut app, B);
    // The chord is 4000 mm: no arc of radius 1000 spans it.
    assert!(app.type_text("1000") && app.finish_tool());
    assert!(app.message.as_deref().is_some_and(|m| m.contains("can't be drawn")), "{:?}", app.message);
    assert_eq!(app.picked.len(), 2, "the points stay");
    assert!(app.typed.is_empty() && !app.type_text("x"), "what was typed is spent, and a letter is nobody's");
}
