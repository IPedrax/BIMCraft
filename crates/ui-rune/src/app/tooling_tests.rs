//! The tools through the app, with no window: clicks at model points, the commands the core runs, the status bar's message, undo.

use bimcraft_core::draw::Shape;
use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;
use crate::select::How;

/// A click at a model point: the press and release at the pixel it lies at.
fn click(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.release((x, y), (x, y), How::Replace);
}

/// The pointer at a model point, no click.
fn hover(app: &mut App, p: P2) {
    let (x, y) = app.camera.view(app.size.0, app.size.1).px(p);
    app.hover(x, y);
}

fn walls(app: &App) -> usize {
    app.session.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).count()
}

fn elements(app: &App) -> usize {
    app.session.model.elements.len()
}

/// Far from the house and everything in it, where a point lands on the nearest 10 mm or an angle step.
const A: P2 = P2(-30000.0, -30000.0);
const B: P2 = P2(-26000.0, -30000.0);
const C: P2 = P2(-26000.0, -26000.0);

#[test]
fn a_wall_chain_is_made_click_by_click_and_the_last_point_clicked_again_ends_it() {
    let mut app = App::sample();
    app.set_tool("wall");
    let before = walls(&app);
    click(&mut app, A);
    assert_eq!((walls(&app), app.picked.len()), (before, 1), "the first click only starts the chain");
    click(&mut app, B);
    assert_eq!((walls(&app), app.picked.len()), (before + 1, 2), "the second makes a wall and the chain goes on");
    click(&mut app, C);
    assert_eq!((walls(&app), app.picked.len()), (before + 2, 3));
    click(&mut app, C);
    assert_eq!(walls(&app), before + 2, "the same place again makes nothing");
    assert!(app.picked.is_empty() && app.tool == Some("wall"), "the chain is over and the tool is still in hand");
    assert!(app.message.is_none(), "{:?}", app.message);
    assert!(app.undo());
    assert_eq!(walls(&app), before + 1, "each wall is an undo step of its own");
}

#[test]
fn backspace_gives_the_last_point_back_and_a_wall_chain_goes_on_from_the_one_before() {
    let mut app = App::sample();
    assert!(!app.back_point() && !app.finish_tool(), "with no tool the keys are nobody's here");
    app.set_tool("wall");
    assert!(!app.back_point() && !app.finish_tool(), "and with a tool but no point");
    click(&mut app, A);
    click(&mut app, B);
    assert!(app.back_point());
    assert_eq!(app.picked.len(), 1);
    assert!(app.landing.is_some(), "the pointer still lands, from the point before");
    assert!(app.finish_tool() && app.picked.is_empty(), "Enter ends the chain");
}

#[test]
fn a_point_tool_makes_its_element_on_a_click_and_the_core_refusing_is_the_message() {
    let mut app = App::sample();
    app.set_tool("column");
    let before = elements(&app);
    click(&mut app, A);
    assert_eq!(elements(&app), before + 1, "{:?}", app.message);
    assert!(app.picked.is_empty(), "a point tool starts again at once");
    app.set_tool("door");
    click(&mut app, B);
    assert_eq!(elements(&app), before + 1, "a door with no wall under it is refused");
    assert!(app.message.as_deref().is_some_and(|m| !m.is_empty()), "and the core says why");
    assert!(app.picked.is_empty(), "the click that was refused is given back");
    let wall = (3000.0, 0.0);
    click(&mut app, P2(wall.0, wall.1));
    assert!(elements(&app) > before + 1, "a door in the house's south wall: {:?}", app.message);
    assert!(app.message.is_none());
}

#[test]
fn a_path_is_finished_by_enter_and_a_stair_by_its_second_click() {
    let mut app = App::sample();
    app.set_tool("railing");
    let before = elements(&app);
    for p in [A, B, C] {
        click(&mut app, p);
    }
    assert_eq!((elements(&app), app.picked.len()), (before, 3), "a path waits");
    assert!(app.finish_tool(), "Enter takes it");
    assert_eq!(elements(&app), before + 1, "{:?}", app.message);
    assert!(app.picked.is_empty());
    app.set_tool("stair");
    click(&mut app, A);
    assert_eq!(elements(&app), before + 1);
    click(&mut app, B);
    assert_eq!(elements(&app), before + 2, "{:?}", app.message);
}

#[test]
fn a_shape_makes_its_walls_as_one_undo_step_and_previews_what_the_next_click_makes() {
    let mut app = App::sample();
    app.set_tool("wall");
    app.set_shape(Shape::Rectangle);
    let before = walls(&app);
    click(&mut app, A);
    hover(&mut app, P2(-27000.0, -27000.0));
    assert_eq!(app.shape_preview().len(), 4, "the rectangle the next click would make");
    click(&mut app, P2(-27000.0, -27000.0));
    assert_eq!(walls(&app), before + 4, "{:?}", app.message);
    assert!(app.picked.is_empty(), "a shape is made whole");
    assert!(app.undo());
    assert_eq!(walls(&app), before, "four walls, one undo step");
    app.set_shape(Shape::Line);
    click(&mut app, A);
    hover(&mut app, B);
    assert!(app.shape_preview().is_empty(), "a line is drawn by the chain's own segment, which the pick layer shows");
    app.set_shape(Shape::Circle);
    assert!(app.picked.is_empty(), "another shape starts again");
}

#[test]
fn a_tool_that_modifies_leaves_the_points_it_picks_alone_and_the_shape_goes_with_the_tool() {
    let mut app = App::sample();
    app.set_tool("move");
    let before = elements(&app);
    click(&mut app, A);
    click(&mut app, B);
    assert_eq!((elements(&app), app.picked.len()), (before, 2), "nothing draws it yet");
    app.set_tool("beam");
    app.set_shape(Shape::Rectangle);
    assert!(app.shape_preview().is_empty(), "a beam takes no shape");
    click(&mut app, A);
    click(&mut app, B);
    assert_eq!(elements(&app), before + 1, "a beam is a line from click to click: {:?}", app.message);
    assert_eq!(app.picked.len(), 1, "and the run goes on from where it ended");
}
