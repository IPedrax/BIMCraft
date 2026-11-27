//! Show as Underlay through the app, with no window: the engine keeps the underlay with the view, and the plan on screen draws it.

use bimcraft_core::underlay;
use serde_json::json;

use crate::app::App;
use crate::context::Action;
use crate::views::{Target, ViewKind};

/// The house's first level on screen, and a plan view of it made to stand under it.
fn with_a_plan_view() -> (App, u64, u64) {
    let mut app = App::sample();
    let level = app.level.expect("a level");
    let made = app.session.execute("duplicate_view", &json!({"view": level, "mode": "duplicate"}));
    let copy = made.expect("a plan view")["id"].as_u64().expect("its id");
    app.settle();
    (app, level, copy)
}

#[test]
fn a_plan_other_than_the_one_on_screen_can_go_under_it_and_nothing_else_can() {
    let (mut app, level, copy) = with_a_plan_view();
    assert_eq!(app.underlay_target(), Some(level));
    assert!(app.can_underlay(ViewKind::Plan(copy)));
    assert!(!app.can_underlay(ViewKind::Plan(level)), "not under itself");
    assert!(!app.can_underlay(ViewKind::Section(copy)), "a section is not drawn under a plan yet");
    assert!(
        !app.can_underlay(ViewKind::Schedule("wall")) && !app.can_underlay(ViewKind::Nothing) && !app.can_underlay(ViewKind::Ceiling(copy))
    );
    assert!(app.open_named("3d"));
    assert_eq!(app.underlay_target(), None, "a 3D view has none");
    assert!(!app.can_underlay(ViewKind::Plan(copy)));
}

#[test]
fn show_as_underlay_puts_the_view_under_the_plan_through_the_engine_and_draws_it_and_remove_takes_it_off() {
    let (mut app, level, copy) = with_a_plan_view();
    assert!(app.underlaid.is_none() && !app.has_underlay());
    assert!(app.show_as_underlay(copy), "{:?}", app.message);
    assert!(underlay::of(&app.session.model, level).is_some(), "the view has it, saved with it");
    assert!(app.has_underlay());
    let (drawing, front) = app.underlaid.as_ref().expect("drawn with the plan");
    assert!(!drawing.prims.is_empty(), "the house's lines");
    assert!(!front, "under the plan until it is asked for over it");
    assert!(app.remove_underlay(), "{:?}", app.message);
    assert!(underlay::of(&app.session.model, level).is_none() && app.underlaid.is_none() && !app.has_underlay());
}

#[test]
fn the_browsers_menu_offers_show_as_underlay_on_a_plan_and_remove_underlay_when_the_plan_on_screen_has_one() {
    let (mut app, level, copy) = with_a_plan_view();
    app.open_browser_menu(Target::View(ViewKind::Plan(copy)), 40.0, 50.0);
    let items: Vec<(Action, bool)> = app.browser_menu_items().iter().flatten().map(|i| (i.action, i.enabled)).collect();
    assert!(items.contains(&(Action::ShowAsUnderlay, true)), "{items:?}");
    assert!(items.contains(&(Action::RemoveUnderlay, false)), "nothing to remove yet: {items:?}");
    app.act(Action::ShowAsUnderlay);
    assert!(app.views.menu.is_none() && underlay::of(&app.session.model, level).is_some(), "{:?}", app.message);
    app.open_browser_menu(Target::View(ViewKind::Plan(copy)), 40.0, 50.0);
    assert!(app.browser_menu_items().iter().flatten().any(|i| i.action == Action::RemoveUnderlay && i.enabled));
    app.act(Action::RemoveUnderlay);
    assert!(underlay::of(&app.session.model, level).is_none());
    // The plan's own row cannot go under itself, and a schedule's row has no such item.
    app.open_browser_menu(Target::View(ViewKind::Plan(level)), 40.0, 50.0);
    assert!(app.browser_menu_items().iter().flatten().any(|i| i.action == Action::ShowAsUnderlay && !i.enabled));
    app.close_browser_menu();
    app.open_browser_menu(Target::View(ViewKind::Schedule("wall")), 40.0, 50.0);
    assert!(!app.browser_menu_items().iter().flatten().any(|i| i.action == Action::ShowAsUnderlay));
}

#[test]
fn with_no_plan_on_screen_there_is_nowhere_to_put_it_and_the_status_bar_says_so() {
    let (mut app, _, copy) = with_a_plan_view();
    assert!(app.open_named("3d"));
    assert!(!app.show_as_underlay(copy) && !app.remove_underlay());
    assert!(app.message.is_some());
}
