//! Manage > Project Information, driven as a user does.

use super::*;

fn open(h: &mut Harness<BimApp>) {
    press(h, "Manage");
    press(h, "Project Information");
}

/// Types `text` into the field named `label`, without pressing Enter (which an address's lines take).
fn type_in(h: &mut Harness<BimApp>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.key_press(Key::Backspace);
    h.get_by_role_and_label(Role::TextInput, label).type_text(text);
    h.run();
}

#[test]
fn project_information_is_typed_in_its_dialog_and_ok_sets_it_in_one_step() {
    let mut h = app(BOX);
    h.state_mut().run("parameter", json!({"name": "Permit Number", "categories": ["project"]}));
    h.state_mut().run("parameter", json!({"name": "Listed", "kind": "yesno", "categories": ["project"]}));
    h.run();
    // The category is named as Revit names it, not by its key.
    assert_eq!(
        (crate::lang::category("project", false), crate::lang::category("project", true)),
        ("Project Information".into(), "Project Information".into())
    );
    let steps = h.state().session.history().len();
    open(&mut h);
    type_in(&mut h, "Project Number", "2026-014");
    type_in(&mut h, "Client Name", "Ana Lima");
    type_in(&mut h, "Permit Number", "PR-88");
    h.get_by_role_and_label(Role::CheckBox, "Listed").click();
    h.run();
    assert!(h.state().session.model.project_info().is_none(), "nothing until OK");
    press(&mut h, "OK");
    let m = &h.state().session.model;
    let (_, info) = m.project_info().expect("OK made it");
    assert_eq!((info.number.as_str(), info.client.as_str()), ("2026-014", "Ana Lima"));
    assert_eq!((info.params["Permit Number"].clone(), info.params["Listed"].clone()), (json!("PR-88"), json!(true)));
    assert_eq!(h.state().session.history().len(), steps + 1, "one undo step");
    assert!(h.state().project_info_dialog.is_none(), "closed");

    // Opened again, it shows what the project holds; a refusal stays in the dialog, which stays open.
    open(&mut h);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Client Name").value().as_deref(), Some("Ana Lima"));
    type_in(&mut h, "Project Status", "a".repeat(300).as_str());
    press(&mut h, "OK");
    assert!(h.state().project_info_dialog.as_ref().is_some(), "still open");
    assert!(h.query_by_label_contains("holds up to 256").is_some(), "says why");
    press(&mut h, "Cancel");
    assert!(h.state().project_info_dialog.is_none());
    assert_eq!(h.state().session.model.project_info().map(|(_, i)| i.status.as_str()), Some(""));
}
