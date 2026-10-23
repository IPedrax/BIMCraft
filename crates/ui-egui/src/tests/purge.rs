//! Purge Unused on the Manage tab, and Delete in the Project Browser's right-click menu on a type, driven as a user drives them.

use super::*;
use egui_kittest::kittest::NodeT;

const FIND: &str = "Search the Project Browser";

/// The first entry named `label` in the Project Browser: a button left of the views' tabs (Properties names types in labels).
fn in_browser<'a>(h: &'a Harness<BimApp>, label: &'a str) -> egui_kittest::Node<'a> {
    let entries = h.get_all_by_label(label).filter(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button);
    entries.min_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y)).unwrap()
}

fn with_spare_types() -> Harness<'static, BimApp> {
    app(&format!(
        "{BOX}\ntype {{\"category\": \"wall\", \"name\": \"Spare 100\", \"thickness\": 100}}\n\
         type {{\"category\": \"window\", \"name\": \"Round 600\", \"width\": 600, \"height\": 600}}"
    ))
}

/// The dialog's button that purges, named by how many it takes out (`Purge 12`).
fn purge_button<'a>(h: &'a Harness<BimApp>) -> Option<egui_kittest::Node<'a>> {
    h.get_all_by_role(Role::Button).find(|n| {
        n.accesskit_node()
            .label()
            .is_some_and(|l| l.strip_prefix("Purge ").is_some_and(|r| r.chars().all(|c| c.is_ascii_digit()) && !r.is_empty()))
    })
}

#[test]
fn purge_unused_lists_what_the_checked_kinds_would_take_out_and_takes_it_out_in_one_step() {
    let mut h = with_spare_types();
    press(&mut h, "Manage");
    press(&mut h, "Purge Unused");
    let listed = |h: &Harness<BimApp>| h.state().purge_dialog.as_ref().map_or(0, |d| d.listed.len());
    let (types, steps) = (h.state().session.model.types.wall.len(), h.state().session.history().len());
    assert!(listed(&h) > 0 && h.state().purge_dialog.as_ref().is_some_and(|d| d.kinds[..2] == [true, true]));
    // Unchecking Types and Families takes everything off the list; a kind of its own puts some back.
    pick(&mut h, Role::CheckBox, "Types");
    pick(&mut h, Role::CheckBox, "Families");
    assert_eq!(listed(&h), 0);
    assert!(h.query_by_label("Nothing is unused.").is_some());
    assert!(purge_button(&h).is_some_and(|b| b.accesskit_node().is_disabled()), "nothing to purge: the button is greyed");
    pick(&mut h, Role::CheckBox, "Materials");
    assert!(listed(&h) > 0);
    pick(&mut h, Role::CheckBox, "Materials");
    pick(&mut h, Role::CheckBox, "Types");
    let n = listed(&h);
    assert!(n > 0 && h.query_by_label("wall type: Spare 100").is_some());
    purge_button(&h).expect("a Purge button").click_accesskit();
    h.run();
    assert!(h.state().purge_dialog.is_none(), "it closes");
    let m = &h.state().session.model;
    assert!(m.types.wall.len() < types && !m.types.wall.contains_key("Spare 100") && !m.types.window.contains_key("Round 600"));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert!(h.state().status.text.contains("Purged"), "{}", h.state().status.text);
}

#[test]
fn pu_opens_purge_unused_and_cancel_leaves_the_project_as_it_was() {
    let mut h = with_spare_types();
    let before = h.state().session.model.types.wall.len();
    shortcut(&mut h, "PU");
    assert!(h.state().purge_dialog.is_some());
    press(&mut h, "Cancel");
    assert!(h.state().purge_dialog.is_none());
    assert_eq!(h.state().session.model.types.wall.len(), before);
}

#[test]
fn the_browsers_families_list_the_systems_types_and_a_right_click_deletes_an_unused_one() {
    let mut h = with_spare_types();
    press(&mut h, "List Families");
    type_into(&mut h, FIND, "generic - 200");
    // A type in use is refused, saying so.
    in_browser(&h, "Generic - 200mm").click_secondary();
    h.run();
    press(&mut h, "Delete");
    assert!(h.state().status.error && h.state().status.text.contains("is used"), "{}", h.state().status.text);
    assert!(h.state().session.model.types.wall.contains_key("Generic - 200mm"));
    type_into(&mut h, FIND, "spare");
    in_browser(&h, "Spare 100").click_secondary();
    h.run();
    press(&mut h, "Delete");
    assert!(!h.state().session.model.types.wall.contains_key("Spare 100"));
}
