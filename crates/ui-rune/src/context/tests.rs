//! The menu's items, from the sample house and what is selected.

use bimcraft_core::model::Element;
use serde_json::json;

use super::*;
use crate::app::App;

fn house() -> (App, Id) {
    let app = App::sample();
    let wall = app.session.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).expect("a wall");
    (app, wall)
}

fn actions(groups: &[Vec<Item>]) -> Vec<Action> {
    groups.iter().flatten().map(|i| i.action).collect()
}

fn enabled(groups: &[Vec<Item>], action: Action) -> Option<bool> {
    groups.iter().flatten().find(|i| i.action == action).map(|i| i.enabled)
}

#[test]
fn with_nothing_selected_and_no_tool_it_offers_cancel_disabled_and_the_zoom() {
    let (app, _) = house();
    let g = items(&app.session.model, &[], false, false);
    assert_eq!(
        actions(&g),
        [
            Action::Cancel,
            Action::Paste,
            Action::PasteSamePlace,
            Action::PasteCurrentView,
            Action::PasteSelectedLevels,
            Action::PasteSelectedViews,
            Action::ZoomToFit,
            Action::ZoomIn,
            Action::ZoomOut
        ]
    );
    assert_eq!(enabled(&g, Action::Cancel), Some(false), "nothing to cancel");
    assert_eq!(enabled(&items(&app.session.model, &[], true, false), Action::Cancel), Some(true), "a tool in hand is cancelled");
}

#[test]
fn a_tool_with_points_picked_can_be_finished_first() {
    let (app, _) = house();
    let g = items(&app.session.model, &[], true, true);
    assert_eq!(g[0].first().map(|i| i.action), Some(Action::Finish), "Finish leads the first group");
}

#[test]
fn a_selected_wall_can_be_deleted_pinned_and_have_its_joins_turned_off_and_each_item_says_if_it_would_change_anything() {
    let (mut app, wall) = house();
    let g = items(&app.session.model, &[wall], false, false);
    assert_eq!(enabled(&g, Action::Delete), Some(true));
    assert_eq!((enabled(&g, Action::Pin), enabled(&g, Action::Unpin)), (Some(true), Some(false)), "nothing is pinned yet");
    let joined = app.session.model.wall(wall).is_some_and(|w| w.join);
    assert_eq!(enabled(&g, Action::DisallowJoin), Some(joined));
    assert_eq!(enabled(&g, Action::AllowJoin), Some(!joined));
    app.session.execute("pin", &json!({"ids": [wall]})).expect("pinned");
    let g = items(&app.session.model, &[wall], false, false);
    assert_eq!((enabled(&g, Action::Pin), enabled(&g, Action::Unpin)), (Some(false), Some(true)), "now it can be unpinned");
}

#[test]
fn an_element_with_no_joins_offers_none_and_every_action_has_a_label() {
    let (app, _) = house();
    let level = app.level.expect("a level");
    let g = items(&app.session.model, &[level], false, false);
    assert!(enabled(&g, Action::DisallowJoin).is_none() && enabled(&g, Action::AllowJoin).is_none(), "a level has no joins");
    for a in [Action::Finish, Action::Cancel, Action::Delete, Action::Pin, Action::Unpin, Action::DisallowJoin, Action::AllowJoin] {
        assert!(!a.label().is_empty(), "{a:?}");
    }
    assert_eq!((Action::Cancel.key(), Action::Pin.key()), (Some("Esc"), None));
}
