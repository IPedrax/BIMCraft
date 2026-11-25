//! Type Properties' state and handlers with no window: a type made by the `type` command, a wall of it selected, its thickness typed,
//! and the project read back.

use serde_json::json;

use super::*;

/// An app with a wall of a type of its own (a thickness and no layers, so the thickness is the type's to change), selected.
fn with_wall() -> App {
    let mut app = App::sample();
    app.execute("type", &json!({"category": "wall", "name": "Rune Test Wall", "thickness": 200})).expect("a type");
    app.execute("wall", &json!({"points": [[50000, 50000], [53000, 50000]], "type": "Rune Test Wall"})).expect("a wall");
    let model = &app.session.model;
    let id = model
        .elements
        .iter()
        .map(|(id, _)| *id)
        .find(|id| bimcraft_core::element_json(model, *id)["type"] == "Rune Test Wall")
        .expect("the wall is in the model");
    app.select(vec![id]);
    app
}

fn typed(app: &mut App, field: &str, text: &str) {
    let d = app.dialogs.types.as_mut().expect("Type Properties is open");
    let f = d.fields.iter_mut().find(|f| f.name == field).expect("a field of that name");
    f.edit.replace_text(text);
}

fn thickness(app: &App) -> f64 {
    app.session.model.types.wall["Rune Test Wall"].thickness
}

#[test]
fn the_type_of_the_selection_opens_and_apply_changes_it_through_the_type_command() {
    let mut app = with_wall();
    assert!(app.open_type_of_selection());
    let d = app.dialogs.types.as_ref().expect("open");
    assert_eq!((d.key.category.as_str(), d.key.name.as_str()), ("wall", "Rune Test Wall"));
    assert!(d.names.iter().any(|n| n == "Rune Test Wall") && d.fields.iter().any(|f| f.name == "thickness" && f.edit.text == "200.0"));

    assert!(app.apply_type(), "nothing is different, and that is no error");
    assert!(app.dialogs.types.as_ref().is_some_and(|d| d.done && d.error.is_none()));
    assert_eq!(thickness(&app), 200.0);

    typed(&mut app, "thickness", "300");
    assert!(app.apply_type(), "{:?}", app.dialogs.types.as_ref().map(|d| &d.error));
    assert_eq!(thickness(&app), 300.0, "the type is changed everywhere it is used");
    let d = app.dialogs.types.as_ref().expect("still open");
    assert!(d.done && d.error.is_none());
    assert!(d.fields.iter().any(|f| f.name == "thickness" && f.edit.text == "300.0"), "and the dialog shows the type as it is now");
}

#[test]
fn a_field_that_is_no_number_is_said_in_the_dialog_and_changes_nothing() {
    let mut app = with_wall();
    app.open_type_of_selection();
    typed(&mut app, "thickness", "abc");
    assert!(!app.apply_type());
    let d = app.dialogs.types.as_ref().expect("open");
    assert!(d.error.as_deref().is_some_and(|e| e.starts_with("thickness: ")) && !d.done);
    assert_eq!(thickness(&app), 200.0);
    assert!(app.message.is_none(), "the status bar is left alone");
}

#[test]
fn another_type_of_the_category_is_shown_and_a_selection_that_is_not_one_typed_element_says_so() {
    let mut app = with_wall();
    app.open_type_of_selection();
    let other =
        app.dialogs.types.as_ref().and_then(|d| d.names.iter().find(|n| *n != "Rune Test Wall").cloned()).expect("another wall type");
    assert!(app.switch_type(&other));
    assert_eq!(app.dialogs.types.as_ref().map(|d| d.key.name.clone()), Some(other));
    assert!(!app.switch_type("no such type"), "a type the project does not hold is not shown");
    app.toggle_type_list();
    assert!(app.dialogs.types.as_ref().is_some_and(|d| d.open));
    app.close_type_list();
    assert!(app.dialogs.types.as_ref().is_some_and(|d| !d.open));

    app.close_dialogs();
    app.select(Vec::new());
    assert!(!app.open_type_of_selection());
    assert!(app.dialogs.types.is_none() && app.message.is_some(), "nothing is open, and the bar says why");
}
