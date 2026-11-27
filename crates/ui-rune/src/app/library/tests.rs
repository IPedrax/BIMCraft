//! The library managers' state and handlers with no window: an entry made by its command, chosen, a field typed, Apply, Rename and
//! Delete, and the project read back.

use serde_json::json;

use super::*;

fn with_text_type() -> App {
    let mut app = App::sample();
    app.execute("text_type", &json!({"name": "Rune Text", "size": 3})).expect("a text type");
    app.execute("text_type", &json!({"name": "Rune Other", "size": 4})).expect("another");
    app
}

fn typed(app: &mut App, field: &str, text: &str) {
    let d = app.dialogs.library.as_mut().expect("a manager is open");
    let f = d.fields.iter_mut().find(|f| f.name == field).expect("a field of that name");
    f.edit.replace_text(text);
}

fn size(app: &App, name: &str) -> f64 {
    app.session.model.text_types[name].size
}

#[test]
fn an_entry_is_chosen_a_field_typed_and_apply_sends_the_command_only_what_differs() {
    let mut app = with_text_type();
    app.open_library(Source::TextTypes);
    app.library_select("Rune Text");
    let d = app.dialogs.library.as_ref().expect("open");
    assert_eq!(d.selected.as_deref(), Some("Rune Text"));
    assert!(d.names.iter().any(|n| n == "Rune Other"), "the other entries are listed");
    assert!(d.fields.iter().any(|f| f.name == "size" && f.edit.text == "3.0"), "the field starts from the entry's value");

    assert!(app.library_apply(), "nothing is different, and that is no error");
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.done && d.error.is_none()));

    typed(&mut app, "size", "5");
    assert!(app.library_apply(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert_eq!(size(&app, "Rune Text"), 5.0);
    assert_eq!(size(&app, "Rune Other"), 4.0, "the other entry is untouched");
    let d = app.dialogs.library.as_ref().expect("still open");
    assert!(d.done && d.selected.as_deref() == Some("Rune Text") && d.fields.iter().any(|f| f.name == "size" && f.edit.text == "5.0"));
}

#[test]
fn a_field_that_is_no_number_is_said_in_the_dialog_and_changes_nothing() {
    let mut app = with_text_type();
    app.open_library(Source::TextTypes);
    app.library_select("Rune Text");
    typed(&mut app, "size", "abc");
    assert!(!app.library_apply());
    assert!(app.dialogs.library.as_ref().and_then(|d| d.error.as_deref()).is_some_and(|e| e.starts_with("size: ")));
    assert_eq!(size(&app, "Rune Text"), 3.0);
    assert!(app.message.is_none(), "the status bar is left alone");
}

#[test]
fn an_entry_is_renamed_and_deleted_through_its_command() {
    let mut app = with_text_type();
    app.open_library(Source::TextTypes);
    app.library_select("Rune Text");
    app.dialogs.library.as_mut().expect("open").rename.replace_text("Rune Renamed");
    assert!(app.library_apply(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert!(app.session.model.text_types.contains_key("Rune Renamed") && !app.session.model.text_types.contains_key("Rune Text"));
    assert_eq!(
        app.dialogs.library.as_ref().and_then(|d| d.selected.clone()).as_deref(),
        Some("Rune Renamed"),
        "the dialog follows the entry"
    );

    assert!(app.library_delete(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert!(!app.session.model.text_types.contains_key("Rune Renamed"));
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.done && d.selected.as_deref() != Some("Rune Renamed")));
}

#[test]
fn a_source_with_nothing_in_it_says_so_and_new_opens_the_commands_form() {
    let mut app = App::sample();
    app.open_library(Source::ViewFilters);
    let d = app.dialogs.library.as_ref().expect("open");
    if d.selected.is_none() {
        assert!(!app.library_apply());
        assert!(app.dialogs.library.as_ref().is_some_and(|d| d.error.is_some()), "there is nothing to apply to");
    }
    app.library_new();
    assert_eq!(app.dialogs.form.as_ref().map(|f| f.form.id), Some("filter"), "New is the command's own form");
    assert!(app.dialogs.library.is_none(), "the manager gives way to it");
    assert!(!app.library_delete() && !app.library_apply(), "nothing is open to act on");
}

#[test]
fn every_manager_opens_on_the_sample_house_and_an_object_style_is_not_deleted() {
    let mut app = App::sample();
    for info in &library::ALL {
        app.open_library(info.source);
        assert!(app.dialogs.library.is_some(), "{}", info.title);
        assert_eq!(app.dialogs.library.as_ref().map(|d| d.info.source), Some(info.source));
    }
    app.open_library(Source::ObjectStyles);
    assert!(!app.library_delete(), "an object style is reset, not deleted");
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.error.is_some()));
}

#[test]
fn a_location_is_changed_and_made_active_through_the_location_command() {
    let mut app = App::sample();
    app.execute("location", &json!({"name": "Rune Site", "east": 1000.0, "north": 2000.0})).expect("a site");
    app.open_library(Source::Locations);
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.names.iter().any(|n| n == "Rune Site")));
    app.library_select("Rune Site");
    let east = |app: &App| {
        let m = &app.session.model;
        m.site_named("Rune Site").map(|site| site.location(m.coordinates.survey).east)
    };
    assert!(east(&app).is_some_and(|e| (e - 1000.0).abs() < 1e-6), "{:?}", east(&app));
    typed(&mut app, "east", "5000");
    assert!(app.library_apply(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert!(east(&app).is_some_and(|e| (e - 5000.0).abs() < 1e-6), "{:?}", east(&app));
    // It is not the active one yet, so the dialog offers to make it so.
    assert_ne!(app.session.model.coordinates.active, "Rune Site");
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.json.get("active") == Some(&json!(false))));
    assert!(app.library_activate(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert_eq!(app.session.model.coordinates.active, "Rune Site");
    assert!(app.dialogs.library.as_ref().is_some_and(|d| d.json.get("active") == Some(&json!(true))));
}

#[test]
fn a_loaded_classification_table_is_an_entry_shown_and_taken_out_through_its_command() {
    let path = std::env::temp_dir().join(format!("rune-classification-{}.csv", std::process::id()));
    std::fs::write(&path, "Code,Title\nA1,First\nA2,Second\n").expect("a table to load");
    let mut app = App::sample();
    let loaded = app.execute("classification_table", &json!({"path": path.to_string_lossy(), "system": "custom", "name": "Rune Table"}));
    let _ = std::fs::remove_file(&path);
    assert!(loaded.is_ok(), "{loaded:?}");
    app.open_library(Source::Classification);
    let d = app.dialogs.library.as_ref().expect("open");
    assert!(d.names.iter().any(|n| n == "Rune Table"), "{:?}", d.names);
    assert!(d.fields.is_empty(), "a table is loaded again, not edited");
    assert_eq!(d.json.get("rows"), Some(&json!(2)), "{:?}", d.json);
    assert!(app.library_delete(), "{:?}", app.dialogs.library.as_ref().map(|d| &d.error));
    assert!(app.session.model.classification.tables.is_empty());
}
