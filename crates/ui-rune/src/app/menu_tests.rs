//! The right-click menu's actions through the app, with no window.

use bimcraft_core::geom::P2;
use bimcraft_core::model::Element;

use crate::app::App;
use crate::context::Action;
use crate::select::How;

fn with_wall() -> (App, u64) {
    let mut app = App::sample();
    let wall = app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    app.select(vec![wall]);
    (app, wall)
}

#[test]
fn the_menu_opens_where_the_click_was_and_closes_and_a_place_that_is_no_number_opens_nothing() {
    let mut app = App::sample();
    app.open_menu(f32::NAN, 4.0);
    app.open_menu(3.0, f32::INFINITY);
    assert!(app.tooling.menu.is_none());
    app.open_menu(120.0, 80.0);
    assert_eq!(app.tooling.menu, Some((120.0, 80.0)));
    app.close_menu();
    assert!(app.tooling.menu.is_none());
}

#[test]
fn the_menu_offers_what_the_selection_and_the_tool_allow_and_an_action_closes_it() {
    let (mut app, _) = with_wall();
    assert!(app.menu_items().iter().flatten().any(|i| i.action == Action::Delete));
    app.open_menu(10.0, 10.0);
    app.act(Action::ZoomIn);
    assert!(app.tooling.menu.is_none(), "an item closes the menu");
    app.set_tool("wall");
    let far = |app: &App, p: P2| app.camera.view(app.size.0, app.size.1).px(p);
    let (x, y) = far(&app, P2(-30000.0, -30000.0));
    app.release((x, y), (x, y), How::Replace);
    let first = app.menu_items();
    assert_eq!(first[0].first().map(|i| i.action), Some(Action::Finish), "a point is picked: the chain can be finished");
    app.act(Action::Finish);
    assert!(app.picked.is_empty() && app.tool == Some("wall"), "Finish ends the chain, not the tool");
    app.act(Action::Cancel);
    assert!(app.tool.is_none(), "Cancel puts the tool down");
}

#[test]
fn delete_pin_unpin_and_the_joins_work_on_the_selection_through_the_core() {
    let (mut app, wall) = with_wall();
    app.act(Action::Pin);
    assert!(app.session.model.is_pinned(wall), "{:?}", app.message);
    app.act(Action::Unpin);
    assert!(!app.session.model.is_pinned(wall));
    app.act(Action::DisallowJoin);
    assert_eq!(app.session.model.wall(wall).map(|w| w.join), Some(false), "{:?}", app.message);
    app.act(Action::AllowJoin);
    assert_eq!(app.session.model.wall(wall).map(|w| w.join), Some(true));
    app.act(Action::Delete);
    assert!(app.session.model.elements.get(wall).is_none(), "{:?}", app.message);
    assert!(app.selection.is_empty());
    assert!(app.undo() && app.session.model.elements.get(wall).is_some(), "and Delete is an undo step");
}

#[test]
fn the_zoom_items_zoom() {
    let mut app = App::sample();
    let scale = app.camera.scale;
    app.act(Action::ZoomIn);
    assert!(app.camera.scale > scale);
    app.act(Action::ZoomToFit);
    assert_eq!(app.camera.scale, scale, "Fit frames the plan as it was first shown");
    app.act(Action::ZoomOut);
    assert!(app.camera.scale < scale);
}
