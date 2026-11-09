use super::*;

#[test]
fn family_editor_selects_vertical_gate_operations_and_save_reopen_keeps_them() {
    for (label, operation) in [("Sectional", "sectional"), ("Roller", "roller")] {
        let mut h = app(&format!("{BOX}\ndoor {{\"at\":[2000,0],\"family\":\"Single Door\"}}"));
        let door = find(&h, |e| matches!(e, Element::Door(_)));
        h.state_mut().selection = vec![door];
        h.set_size(vec2(1800.0, 1000.0));
        press(&mut h, "Manage");
        press(&mut h, "Edit Family");
        pick(&mut h, Role::ComboBox, "Operation");
        assert!(h.query_by_label(label).is_some(), "Operation offers {label}");
        h.get_by_label(label).scroll_to_me();
        h.run();
        press(&mut h, label);
        assert_eq!(combo_value(&h, "Operation"), label);
        press_near(&mut h, "Apply", "Cancel");
        assert_eq!(h.state().session.model.families["Single Door"].operation.unwrap().name(), operation);
        press(&mut h, "Cancel");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vertical-gate.bimcraft");
        let guid = h.state().session.model.elements.guid(door).unwrap();
        h.state_mut().session.execute("save", &json!({"path":path})).unwrap();
        h.state_mut().session.execute("open", &json!({"path":path})).unwrap();
        assert_eq!(h.state().session.model.elements.guid(door), Some(guid));
        h.state_mut().selection = vec![door];
        press(&mut h, "Edit Family");
        assert_eq!(combo_value(&h, "Operation"), label, "reopened family displays its operation");
        // An unapplied selector change is discarded by Cancel.
        pick(&mut h, Role::ComboBox, "Operation");
        press(&mut h, "Swing");
        press(&mut h, "Cancel");
        assert_eq!(h.state().session.model.families["Single Door"].operation.unwrap().name(), operation);
    }
}
