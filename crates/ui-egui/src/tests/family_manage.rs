//! FE-19: the family document's Manage tab. Object Styles, Materials and Project Units are the project's own dialogs, which
//! say they change the project now (not on Load into Project), and whose changes are the project's steps: the family's Undo
//! leaves them, the project's takes them back. Object Styles opens at the family's category. Purge Unused takes out of the
//! family's draft what nothing in it uses, offered unticked, as one step of the family's history.
use super::family_views::{editor, open};
use super::*;

const SHELF: &str = r#"family {"name": "Shelf", "category": "furniture", "parameters": [{"name": "Depth", "value": 300}, {"name": "Spare", "value": 7}], "types": {"T": {}}, "forms": [{"profile": [[0, 0], [600, 0], [600, "Depth"], [0, "Depth"]], "to": 20}]}"#;

/// The family document's Manage tab shown (the project's ribbon names a tab Manage too).
fn manage(h: &mut Harness<BimApp>) {
    h.state_mut().active_family_mut().unwrap().doc_tab = crate::editor::DocTab::Manage;
    h.run();
}

fn ctrl_z(h: &mut Harness<BimApp>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

#[test]
fn materials_from_a_family_says_it_changes_the_project_and_its_change_is_the_projects_step() {
    let (mut h, _) = open(SHELF, "Shelf");
    manage(&mut h);
    press(&mut h, "Materials");
    assert!(h.query_by_label_contains("changes the project's materials now").is_some(), "the dialog says so");
    let before = h.state().session.model.materials.len();
    press(&mut h, "New");
    press(&mut h, "Custom");
    assert_eq!(h.state().session.model.materials.len(), before + 1, "a material of the project's, now");
    press(&mut h, "Close");
    // The family's Undo leaves it: it is no step of the family's.
    ctrl_z(&mut h);
    assert_eq!(h.state().session.model.materials.len(), before + 1, "the family's Undo doesn't take it back");
    // The project's does.
    press(&mut h, "Level 1");
    assert!(h.state().active_family().is_none(), "the project's tab");
    ctrl_z(&mut h);
    assert_eq!(h.state().session.model.materials.len(), before, "the project's Undo takes it back");
}

#[test]
fn object_styles_from_a_family_opens_at_its_category_and_says_it_changes_the_project() {
    let (mut h, _) = open(SHELF, "Shelf");
    manage(&mut h);
    press(&mut h, "Object Styles");
    assert!(h.query_by_label_contains("changes the project's object styles now").is_some(), "the dialog says so");
    let first = h.get_all_by_role(egui::accesskit::Role::ComboBox).next().map(|n| format!("{n:?}")).unwrap_or_default();
    let label = format!("label: \"{}: ", crate::lang::category("furniture", true));
    assert!(first.contains(&label), "the family's category first: {first}");
}

#[test]
fn project_units_from_a_family_says_it_changes_the_project() {
    let (mut h, _) = open(SHELF, "Shelf");
    manage(&mut h);
    press(&mut h, "Project Units");
    assert!(h.query_by_label_contains("changes the project's units now").is_some(), "the list says so");
    press(&mut h, "Metres");
    assert_eq!(h.state().session.model.units, bimcraft_core::units::Units::Metres, "the project's units");
    ctrl_z(&mut h);
    assert_eq!(h.state().session.model.units, bimcraft_core::units::Units::Metres, "the family's Undo leaves them");
    press(&mut h, "Level 1");
    ctrl_z(&mut h);
    assert_ne!(h.state().session.model.units, bimcraft_core::units::Units::Metres, "the project's Undo takes them back");
}

#[test]
fn purge_unused_in_a_family_offers_what_nothing_uses_unticked_and_takes_it_out_as_one_step() {
    let (mut h, _) = open(SHELF, "Shelf");
    manage(&mut h);
    press(&mut h, "Purge Unused");
    // Spare is offered; Depth, which the form reads, isn't.
    let spare = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Spare");
    assert!(format!("{spare:?}").contains("toggled: False"), "offered unticked: {spare:?}");
    assert!(h.query_by_role_and_label(egui::accesskit::Role::CheckBox, "Depth").is_none(), "Depth is used");
    // Ticked, and OK.
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Spare").click();
    h.run();
    press(&mut h, "OK");
    assert!(!editor(&h).has_parameter("Spare"), "taken out of the draft");
    assert!(editor(&h).has_parameter("Depth"));
    assert!(h.state().session.model.families["Shelf"].parameters.iter().any(|p| p.name == "Spare"), "the project's family, not yet");
    // One step of the family's.
    ctrl_z(&mut h);
    assert!(editor(&h).has_parameter("Spare"), "back with the family's Undo");
}

#[test]
fn purge_unused_in_a_family_with_nothing_unused_says_so() {
    let script = SHELF.replace(r#", {"name": "Spare", "value": 7}"#, "");
    let (mut h, _) = open(&script, "Shelf");
    manage(&mut h);
    press(&mut h, "Purge Unused");
    assert!(h.query_by_label_contains("Nothing in the family is unused").is_some());
}

#[test]
fn find_lists_the_manage_commands_and_opens_them() {
    let (mut h, _) = open(SHELF, "Shelf");
    for a in ["Object Styles", "Materials", "Project Units", "Purge Unused"] {
        assert!(h.state().family_find_actions().contains(&a), "{a}");
    }
    assert!(h.state_mut().family_find_action("Purge Unused"));
    h.run();
    h.run();
    assert!(h.query_by_role_and_label(egui::accesskit::Role::CheckBox, "Spare").is_some(), "Purge Unused offers Spare");
}

/// The family's Manage tab, Object Styles and Materials opened from it with their notes, Project Units' list, and Purge
/// Unused offering Spare, saved where `BIMCRAFT_FAMILY_MANAGE_DUMP` says (only then).
#[test]
fn family_manage_screenshots() {
    let Ok(folder) = std::env::var("BIMCRAFT_FAMILY_MANAGE_DUMP") else { return };
    std::fs::create_dir_all(&folder).unwrap();
    let save = |h: &mut Harness<BimApp>, name: &str| {
        h.run();
        h.run();
        h.render().unwrap().save(std::path::Path::new(&folder).join(format!("fe19-{name}.png"))).unwrap();
    };
    let mut h = app(SHELF);
    h.set_size(egui::vec2(1920.0, 1000.0));
    h.state_mut().edit_family("Shelf");
    h.run();
    manage(&mut h);
    save(&mut h, "ribbon");
    press(&mut h, "Object Styles");
    save(&mut h, "object-styles");
    press(&mut h, "Cancel");
    press(&mut h, "Materials");
    save(&mut h, "materials");
    press(&mut h, "Close");
    press(&mut h, "Project Units");
    save(&mut h, "units");
    key(&mut h, Key::Escape);
    press(&mut h, "Purge Unused");
    save(&mut h, "purge");
}
