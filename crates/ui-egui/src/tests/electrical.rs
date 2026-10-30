//! Manage > Electrical Settings: the voltages, distribution systems and wire types, edited as a draft and applied as one undo
//! step; a refusal said inside the dialog.

use super::*;

/// The error the open dialog says, if it says one.
fn said(h: &Harness<BimApp>) -> Option<String> {
    h.state().electrical_dialog.as_ref().and_then(|d| d.error()).map(String::from)
}

fn open(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    press(h, "Electrical Settings");
    assert!(h.state().electrical_dialog.is_some(), "the button opens the dialog");
}

#[test]
fn the_dialog_lists_the_projects_voltages_systems_and_wire_types() {
    let mut h = app(BOX);
    open(&mut h);
    // The starter voltages, one row each, in the order of their names.
    for (row, name) in [(1, "127 V"), (3, "230 V"), (7, "480 V")] {
        assert!(h.query_by_label(&format!("Voltage {row} name")).is_some(), "{name}");
    }
    assert_eq!(h.state().session.model.voltages.len(), 7);
    press(&mut h, "Distribution Systems");
    assert!(h.query_by_label("System 3 name").is_some() && h.query_by_label("System 5 name").is_none());
    press(&mut h, "Wire Types");
    assert!(h.query_by_label("Size 1 section").is_some(), "the first wire type's table");
    press(&mut h, "Cancel");
    assert!(h.state().electrical_dialog.is_none());
}

#[test]
fn a_voltage_and_a_system_that_names_it_are_applied_as_one_undo_step() {
    let mut h = app(BOX);
    let steps = h.state().session.history().len();
    open(&mut h);
    press(&mut h, "Add Voltage");
    type_into(&mut h, "Voltage 8 name", "690 V");
    type_role(&mut h, Role::SpinButton, "Voltage 8 nominal", "690");
    type_role(&mut h, Role::SpinButton, "Voltage 8 minimum", "621");
    type_role(&mut h, Role::SpinButton, "Voltage 8 maximum", "759");
    press(&mut h, "Distribution Systems");
    press(&mut h, "Add Distribution System");
    type_into(&mut h, "System 5 name", "Mina");
    choose(&mut h, "System 5 line to line", "690 V");
    choose(&mut h, "System 5 earthing", "IT");
    // Nothing is the project's until OK.
    assert!(!h.state().session.model.voltages.contains_key("690 V"));
    press(&mut h, "OK");
    assert!(h.state().electrical_dialog.is_none(), "OK closes it");
    let m = &h.state().session.model;
    let v = &m.voltages["690 V"];
    assert_eq!((v.nominal, v.min, v.max), (690.0, 621.0, 759.0));
    let s = &m.distribution_systems["Mina"];
    assert_eq!((s.phases, s.wires, s.l_l.as_deref(), s.ground.as_deref()), (3, 4, Some("690 V"), Some("IT")));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert_eq!(h.state().session.history().last(), Some(&"Electrical Settings"));
}

#[test]
fn rows_renamed_and_taken_away_are_applied_in_an_order_the_core_accepts() {
    let mut h = app(BOX);
    open(&mut h);
    // 230 V renamed (the systems that name it follow); the 277/480 V system and then the 480 V and 277 V it named taken away.
    type_into(&mut h, "Voltage 3 name", "Mains 230");
    press(&mut h, "Distribution Systems");
    pick(&mut h, Role::Button, "Delete system 4");
    press(&mut h, "Voltages");
    pick(&mut h, Role::Button, "Delete voltage 7");
    pick(&mut h, Role::Button, "Delete voltage 4");
    press(&mut h, "OK");
    assert!(h.state().electrical_dialog.is_none(), "{:?}", said(&h));
    let m = &h.state().session.model;
    assert!(m.voltages.contains_key("Mains 230") && !m.voltages.contains_key("230 V"));
    assert!(!m.voltages.contains_key("480 V") && !m.voltages.contains_key("277 V"));
    assert!(!m.distribution_systems.contains_key("277/480 V"));
    assert_eq!(m.distribution_systems["230/400 V"].l_n.as_deref(), Some("Mains 230"));
    assert_eq!(m.check(), Ok(()));
    assert!(m.library_in_step());
}

#[test]
fn a_voltage_a_system_supplies_is_not_taken_away() {
    let mut h = app(BOX);
    open(&mut h);
    pick(&mut h, Role::Button, "Delete voltage 3");
    let e = said(&h).expect("the dialog says why");
    assert!(e.contains("230 V") && e.contains("230/400 V"), "{e}");
    assert!(h.query_by_label("Voltage 7 name").is_some(), "the row is still there");
}

#[test]
fn a_name_left_blank_or_used_twice_is_said_in_the_dialog_and_nothing_changes() {
    let mut h = app(BOX);
    let steps = h.state().session.history().len();
    open(&mut h);
    press(&mut h, "Add Voltage");
    type_into(&mut h, "Voltage 8 name", "230 V");
    press_near(&mut h, "OK", "Cancel");
    assert!(h.state().electrical_dialog.is_some(), "a refusal keeps the dialog open");
    assert!(said(&h).is_some_and(|e| e.contains("230 V")), "{:?}", said(&h));
    assert_eq!((h.state().session.model.voltages.len(), h.state().session.history().len()), (7, steps));
    type_into(&mut h, "Voltage 8 name", "  ");
    press_near(&mut h, "OK", "Cancel");
    assert!(said(&h).is_some_and(|e| e.contains("needs a name")), "{:?}", said(&h));
}

#[test]
fn what_the_core_refuses_is_said_in_the_dialog_and_nothing_changes() {
    let mut h = app(BOX);
    let steps = h.state().session.history().len();
    open(&mut h);
    press(&mut h, "Distribution Systems");
    // 400 V between lines is not root three times 380 V to the neutral.
    choose(&mut h, "System 3 line to neutral", "380 V");
    press_near(&mut h, "OK", "Cancel");
    assert!(h.state().electrical_dialog.is_some());
    assert!(said(&h).is_some_and(|e| e.contains("1.73")), "{:?}", said(&h));
    let m = &h.state().session.model;
    assert_eq!(m.distribution_systems["230/400 V"].l_n.as_deref(), Some("230 V"));
    assert_eq!(h.state().session.history().len(), steps);
    // Cancel drops the draft.
    press(&mut h, "Cancel");
    assert!(h.state().electrical_dialog.is_none());
    assert_eq!(h.state().session.model.distribution_systems["230/400 V"].l_n.as_deref(), Some("230 V"));
}

#[test]
fn a_wire_type_is_a_table_of_sections_and_ampacities() {
    let mut h = app(BOX);
    open(&mut h);
    press(&mut h, "Wire Types");
    press(&mut h, "Add Wire Type");
    type_into(&mut h, "Wire type name", "Al 90");
    choose(&mut h, "Wire material", "aluminium");
    type_role(&mut h, Role::SpinButton, "Wire temperature", "90");
    type_into(&mut h, "Wire insulation", "XLPE");
    type_role(&mut h, Role::SpinButton, "Size 1 section", "16");
    type_role(&mut h, Role::SpinButton, "Size 1 ampacity", "61");
    press(&mut h, "Add Size");
    type_role(&mut h, Role::SpinButton, "Size 2 section", "25");
    type_role(&mut h, Role::SpinButton, "Size 2 ampacity", "79");
    type_into(&mut h, "Size 2 label", "AA 25");
    press(&mut h, "OK");
    assert!(h.state().electrical_dialog.is_none(), "{:?}", said(&h));
    let w = &h.state().session.model.wire_types["Al 90"];
    assert_eq!((w.material.as_str(), w.temperature, w.insulation.as_str()), ("aluminium", 90.0, "XLPE"));
    assert_eq!(w.sizes.len(), 2);
    assert_eq!((w.sizes[0].section, w.sizes[0].ampacity, w.sizes[0].label.as_deref()), (16.0, 61.0, None));
    assert_eq!((w.sizes[1].section, w.sizes[1].ampacity, w.sizes[1].label.as_deref()), (25.0, 79.0, Some("AA 25")));
    assert_eq!(h.state().session.model.wire_ampacity("Al 90", 25.0), Some(79.0));
    // And taken away again.
    open(&mut h);
    press(&mut h, "Wire Types");
    choose(&mut h, "Wire type:", "Al 90");
    press(&mut h, "Delete Wire Type");
    press(&mut h, "OK");
    assert!(!h.state().session.model.wire_types.contains_key("Al 90"));
}

#[test]
fn apply_keeps_the_dialog_open_on_what_the_project_now_has() {
    let mut h = app(BOX);
    open(&mut h);
    press(&mut h, "Add Voltage");
    type_into(&mut h, "Voltage 8 name", "690 V");
    press_near(&mut h, "Apply", "Cancel");
    assert!(h.state().electrical_dialog.is_some(), "Apply keeps it open");
    assert!(h.state().session.model.voltages.contains_key("690 V"));
    // Applied again with nothing changed: no new step.
    let steps = h.state().session.history().len();
    press_near(&mut h, "Apply", "Cancel");
    assert_eq!(h.state().session.history().len(), steps);
    press(&mut h, "Cancel");
}
