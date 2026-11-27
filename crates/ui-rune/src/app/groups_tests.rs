//! Groups through the app, with no window: the engine makes and keeps them, the selection and the menu follow what is selected.

use bimcraft_core::groups as core_groups;
use bimcraft_core::model::Element;
use serde_json::{Value, json};

use crate::app::App;
use crate::context::Action;

/// Two walls of their own, away from the house's: the house's are held to its roof, which a group of them would leave outside.
fn two_walls() -> (App, Vec<u64>) {
    let mut app = App::sample();
    let made = app.session.execute("wall", &json!({"points": [[0, -9000], [4000, -9000], [4000, -5000]]}));
    let pair: Vec<u64> = made.expect("two walls")["ids"].as_array().into_iter().flatten().filter_map(Value::as_u64).collect();
    assert_eq!(pair.len(), 2, "a chain of two walls");
    app.settle();
    app.select(pair.clone());
    (app, pair)
}

fn actions(items: &[crate::context::Item]) -> Vec<Action> {
    items.iter().map(|i| i.action).collect()
}

#[test]
fn create_group_makes_one_of_the_selection_selects_it_and_ungroup_gives_the_walls_back() {
    let (mut app, walls) = two_walls();
    assert!(app.groupable() && actions(&app.group_items()) == [Action::CreateGroup]);
    assert!(app.create_group(), "{:?}", app.message);
    assert_eq!(app.groups_selected().len(), 1, "the group is what is selected");
    assert_eq!(app.selection.len(), 1);
    assert!(!app.groupable(), "a group is not grouped again");
    assert_eq!(actions(&app.group_items()), [Action::Ungroup, Action::EditGroup]);
    assert!(app.ungroup(), "{:?}", app.message);
    assert!(core_groups::instances(&app.session.model).is_empty(), "no group is left");
    assert_eq!(app.selection.len(), walls.len(), "the members are selected, plain again");
    assert!(app.selection.iter().all(|id| matches!(app.session.model.elements.get(*id), Some(Element::Wall(_)))));
}

#[test]
fn nothing_selected_cannot_be_grouped_ungrouped_or_edited_and_says_so() {
    let mut app = App::sample();
    assert!(!app.create_group() && app.message.is_some());
    app.message = None;
    assert!(!app.ungroup() && app.message.is_some());
    app.message = None;
    assert!(!app.edit_group() && app.message.is_some());
    assert!(app.group_items().is_empty() && !app.exclude_selected() && !app.restore_excluded());
    assert!(!app.add_to_group() && !app.remove_from_group());
}

#[test]
fn edit_group_offers_add_remove_finish_and_cancel_and_finish_and_cancel_each_end_it() {
    let (mut app, _) = two_walls();
    assert!(app.create_group());
    assert!(app.edit_group(), "{:?}", app.message);
    assert!(app.editing_group().is_some() && app.selection.is_empty(), "the group is open and nothing is selected");
    let items = app.group_items();
    assert_eq!(actions(&items), [Action::AddToGroup, Action::RemoveFromGroup, Action::FinishGroup, Action::CancelGroup]);
    assert!(!items[0].enabled && !items[1].enabled && items[2].enabled && items[3].enabled, "Add and Remove need a selection");
    assert!(app.cancel_group(), "{:?}", app.message);
    assert!(app.editing_group().is_none(), "Cancel ends it");
    let group = app.session.model.elements.iter().find(|(_, e)| core_groups::instance(e).is_some()).map(|(id, _)| *id).expect("the group");
    app.select(vec![group]);
    assert!(app.edit_group() && app.finish_group(), "{:?}", app.message);
    assert!(app.editing_group().is_none(), "Finish ends it");
}

#[test]
fn the_plans_menu_offers_the_group_items_before_the_clipboards_and_an_item_does_its_work() {
    let (mut app, _) = two_walls();
    let flat: Vec<Action> = app.menu_items().iter().flatten().map(|i| i.action).collect();
    let (group, paste) = (flat.iter().position(|a| *a == Action::CreateGroup), flat.iter().position(|a| *a == Action::Paste));
    assert!(group.is_some() && group < paste, "{flat:?}");
    app.act(Action::CreateGroup);
    assert_eq!(app.groups_selected().len(), 1, "{:?}", app.message);
    app.act(Action::Ungroup);
    assert!(core_groups::instances(&app.session.model).is_empty());
}

#[test]
fn gp_and_ug_typed_act_on_the_selection_instead_of_asking_for_ids() {
    let (mut app, _) = two_walls();
    for (i, c) in "GP".chars().enumerate() {
        app.tick(0.1 * i as f64);
        app.type_key(c);
    }
    assert_eq!(app.groups_selected().len(), 1, "GP is Create Group: {:?}", app.message);
    assert!(!app.dialogs.any(), "no form of ids opens");
    for (i, c) in "UG".chars().enumerate() {
        app.tick(1.0 + 0.1 * i as f64);
        app.type_key(c);
    }
    assert!(core_groups::instances(&app.session.model).is_empty(), "UG is Ungroup");
}
