use super::*;

const VERSION: &str = "Overwrite the existing version";

/// A box with `element` of `family` in its wall, the family open as a document and its Family Category and Parameters open.
fn category_dialog(element: &str, family: &str) -> Harness<'static, BimApp> {
    let mut h = app(&format!("{BOX}\n{element} {{\"at\":[2000,0],\"family\":\"{family}\"}}"));
    h.set_size(vec2(1800.0, 1200.0));
    h.state_mut().edit_family(family);
    h.run();
    press(&mut h, "Family Category and Parameters");
    h
}

/// The open document's draft operation, by name.
fn draft_operation(h: &Harness<BimApp>) -> Option<&'static str> {
    h.state().active_family().unwrap().draft_family().unwrap().operation.map(|op| op.name())
}

#[test]
fn family_editor_selects_vertical_gate_operations_and_save_reopen_keeps_them() {
    for (label, operation) in [("Sectional", "sectional"), ("Roller", "roller")] {
        let mut h = category_dialog("door", "Single Door");
        let door = find(&h, |e| matches!(e, Element::Door(_)));
        choose(&mut h, "Operation", label);
        assert_eq!(combo_value(&h, "Operation"), label);
        press(&mut h, "OK");
        assert_eq!(draft_operation(&h), Some(operation));
        press(&mut h, "Load into Project");
        if h.query_by_label(VERSION).is_some() {
            press(&mut h, VERSION);
        }
        assert_eq!(h.state().session.model.families["Single Door"].operation.unwrap().name(), operation);
        // The dialog shows it again, and a change it doesn't keep is put back by Cancel.
        press(&mut h, "Family Category and Parameters");
        assert_eq!(combo_value(&h, "Operation"), label, "the family displays its operation");
        choose(&mut h, "Operation", "Swing");
        press(&mut h, "Cancel");
        assert_eq!(draft_operation(&h), Some(operation));
        // Saved and opened again, the project's family keeps it, and the door is the same element.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vertical-gate.bimcraft");
        let guid = h.state().session.model.elements.guid(door).unwrap();
        h.state_mut().session.execute("save", &json!({"path": path})).unwrap();
        h.state_mut().session.execute("open", &json!({"path": path})).unwrap();
        assert_eq!(h.state().session.model.elements.guid(door), Some(guid));
        assert_eq!(h.state().session.model.families["Single Door"].operation.unwrap().name(), operation);
    }
}

#[test]
fn a_window_family_is_offered_no_gate_operation() {
    let mut h = category_dialog("window", "Fixed Window");
    h.get_by_role_and_label(Role::ComboBox, "Operation").click_accesskit();
    h.run();
    // A window opens as a window does; the family would refuse a gate's operation.
    assert!(h.query_by_label("Sliding").is_some(), "Operation offers a window's");
    for label in ["Sectional", "Roller"] {
        assert!(h.query_by_label(label).is_none(), "a window is offered {label}");
    }
}
